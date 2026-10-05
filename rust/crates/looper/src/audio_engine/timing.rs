//! Shared input epoch and bounded estimates of output-device time.
//!
//! CPAL stream instants have a backend-private origin. Only their differences
//! cross into our engine epoch. These estimates support diagnostics; they do
//! not change the launch policy or account for per-voice DSP latency.

use crate::audio_engine::transport::{GRID_64THS_PER_BEAT, QuantizeGrid};
use cpal::{OutputCallbackInfo, StreamInstant};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const NS_PER_SECOND: f64 = 1_000_000_000.0;
const MAX_DEVICE_DELAY_NS: u64 = 2_000_000_000;
const MAX_EXTRAPOLATION_NS: u64 = 2_000_000_000;
const MIN_FRESHNESS_NS: u64 = 100_000_000;
const MIN_DISCONTINUITY_TOLERANCE_NS: u64 = 20_000_000;
const SNAPSHOT_FIELDS: usize = 8;

/// One origin shared by UI capture, native MIDI and the output callback.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InputClock {
    origin: Instant,
}

impl InputClock {
    pub(crate) fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    pub(crate) fn capture_ns(self) -> u64 {
        self.origin.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }
}

/// Zero is a real epoch timestamp. Future / foreign-clock values fall back.
pub(crate) fn validated_input_timestamp(received_at_ns: Option<u64>, now_ns: u64) -> Option<u64> {
    received_at_ns.filter(|received_at_ns| *received_at_ns <= now_ns)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OutputClockSnapshot {
    pub(crate) valid: bool,
    pub(crate) observed_at_ns: u64,
    pub(crate) audible_at_ns: u64,
    pub(crate) output_frame: u64,
    pub(crate) sample_rate_hz: u32,
    pub(crate) master_bpm: Option<f32>,
    pub(crate) downbeat_frame: u64,
    pub(crate) freshness_ns: u64,
}

impl OutputClockSnapshot {
    pub(crate) fn is_fresh(self, now_ns: u64) -> bool {
        self.valid
            && now_ns
                .checked_sub(self.observed_at_ns)
                .is_some_and(|age_ns| age_ns <= self.freshness_ns)
    }

    /// Nearest musical boundary selected by captured time; midpoint ties go future.
    pub(crate) fn input_target_frame(
        self,
        received_at_ns: Option<u64>,
        now_ns: u64,
        grid: QuantizeGrid,
    ) -> Option<u64> {
        if !self.is_fresh(now_ns) || self.sample_rate_hz == 0 {
            return None;
        }
        let received_at_ns = validated_input_timestamp(received_at_ns, now_ns)?;
        if received_at_ns.abs_diff(self.observed_at_ns) > MAX_EXTRAPOLATION_NS {
            return None;
        }
        let delta_ns = i128::from(received_at_ns) - i128::from(self.audible_at_ns);
        let input_frame = self.output_frame as f64
            + delta_ns as f64 * f64::from(self.sample_rate_hz) / NS_PER_SECOND;
        nearest_grid_frame(
            input_frame,
            self.sample_rate_hz,
            self.master_bpm?,
            self.downbeat_frame,
            grid,
        )
    }
}

fn nearest_grid_frame(
    input_frame: f64,
    sample_rate_hz: u32,
    master_bpm: f32,
    downbeat_frame: u64,
    grid: QuantizeGrid,
) -> Option<u64> {
    if !input_frame.is_finite()
        || input_frame < 0.0
        || sample_rate_hz == 0
        || !master_bpm.is_finite()
        || master_bpm <= 0.0
    {
        return None;
    }
    let frames_per_grid = f64::from(sample_rate_hz) * 60.0 / f64::from(master_bpm)
        * f64::from(grid.step_64ths())
        / f64::from(GRID_64THS_PER_BEAT);
    let position = (input_frame - downbeat_frame as f64) / frames_per_grid;
    let target = downbeat_frame as f64 + (position + 0.5).floor() * frames_per_grid;
    if !target.is_finite() || target < 0.0 || target >= u64::MAX as f64 {
        return None;
    }
    // Fractional grid boundaries become the first frame at/after that boundary.
    Some(target.ceil() as u64)
}

/// Callback-owned mapper; all storage and work is fixed-size.
pub(crate) struct OutputClockMapper {
    previous: Option<OutputClockSnapshot>,
    previous_callback: Option<StreamInstant>,
    previous_playback: Option<StreamInstant>,
}

impl OutputClockMapper {
    pub(crate) fn new() -> Self {
        Self {
            previous: None,
            previous_callback: None,
            previous_playback: None,
        }
    }

    pub(crate) fn observe_callback(
        &mut self,
        info: &OutputCallbackInfo,
        mut snapshot: OutputClockSnapshot,
        callback_frames: usize,
    ) -> OutputClockSnapshot {
        let timestamp = info.timestamp();
        let delay_ns = timestamp
            .playback
            .duration_since(&timestamp.callback)
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok());
        let stream_progress_valid = self
            .previous_callback
            .zip(self.previous_playback)
            .is_none_or(|(callback, playback)| {
                timestamp
                    .callback
                    .duration_since(&callback)
                    .is_some_and(|duration| !duration.is_zero())
                    && timestamp
                        .playback
                        .duration_since(&playback)
                        .is_some_and(|duration| !duration.is_zero())
            });
        self.previous_callback = Some(timestamp.callback);
        self.previous_playback = Some(timestamp.playback);
        if !stream_progress_valid {
            snapshot.valid = false;
        }
        self.observe(snapshot, delay_ns, callback_frames)
    }

    fn observe(
        &mut self,
        mut snapshot: OutputClockSnapshot,
        delay_ns: Option<u64>,
        callback_frames: usize,
    ) -> OutputClockSnapshot {
        let callback_ns = if snapshot.sample_rate_hz == 0 {
            0
        } else {
            ((callback_frames as u128 * 1_000_000_000) / u128::from(snapshot.sample_rate_hz))
                .min(u128::from(u64::MAX)) as u64
        };
        snapshot.freshness_ns = callback_ns.saturating_mul(4).max(MIN_FRESHNESS_NS);
        let audible_at_ns = delay_ns
            .filter(|delay| *delay <= MAX_DEVICE_DELAY_NS)
            .and_then(|delay| snapshot.observed_at_ns.checked_add(delay));
        snapshot.valid &=
            snapshot.sample_rate_hz != 0 && callback_frames != 0 && audible_at_ns.is_some();
        snapshot.audible_at_ns = audible_at_ns.unwrap_or(snapshot.observed_at_ns);

        if let Some(previous) = self.previous {
            let frame_delta = snapshot.output_frame.checked_sub(previous.output_frame);
            let elapsed_ns = snapshot.observed_at_ns.checked_sub(previous.observed_at_ns);
            let predicted_audible_ns = frame_delta.and_then(|frames| {
                let delta_ns = (u128::from(frames) * 1_000_000_000)
                    / u128::from(snapshot.sample_rate_hz.max(1));
                u64::try_from(delta_ns)
                    .ok()
                    .and_then(|delta| previous.audible_at_ns.checked_add(delta))
            });
            let tolerance_ns = callback_ns
                .saturating_mul(4)
                .max(MIN_DISCONTINUITY_TOLERANCE_NS);
            snapshot.valid &= snapshot.sample_rate_hz == previous.sample_rate_hz
                && elapsed_ns.is_some_and(|elapsed| elapsed <= snapshot.freshness_ns)
                && predicted_audible_ns.is_some_and(|predicted| {
                    predicted.abs_diff(snapshot.audible_at_ns) <= tolerance_ns
                });
        }
        // A discontinuity invalidates this observation, then re-establishes from
        // the next usable one. It never changes transport or playing voices.
        self.previous = snapshot.valid.then_some(snapshot);
        snapshot
    }
}

/// Single callback writer, bounded single-attempt control reader. Sequentially
/// consistent atomic fields avoid unsafe memory access or callback locks.
pub(crate) struct SharedOutputClock {
    sequence: AtomicU64,
    fields: [AtomicU64; SNAPSHOT_FIELDS],
}

impl SharedOutputClock {
    pub(crate) fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            fields: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    pub(crate) fn publish(&self, snapshot: OutputClockSnapshot) {
        let sequence = self.sequence.load(Ordering::SeqCst).wrapping_add(1) | 1;
        self.sequence.store(sequence, Ordering::SeqCst);
        let fields = [
            u64::from(snapshot.valid),
            snapshot.observed_at_ns,
            snapshot.audible_at_ns,
            snapshot.output_frame,
            u64::from(snapshot.sample_rate_hz),
            u64::from(snapshot.master_bpm.map_or(0, f32::to_bits)),
            snapshot.downbeat_frame,
            snapshot.freshness_ns,
        ];
        for (field, value) in self.fields.iter().zip(fields) {
            field.store(value, Ordering::SeqCst);
        }
        self.sequence
            .store(sequence.wrapping_add(1), Ordering::SeqCst);
    }

    pub(crate) fn read(&self) -> Option<OutputClockSnapshot> {
        let before = self.sequence.load(Ordering::SeqCst);
        if before == 0 || before & 1 != 0 {
            return None;
        }
        let fields = self
            .fields
            .each_ref()
            .map(|field| field.load(Ordering::SeqCst));
        if self.sequence.load(Ordering::SeqCst) != before {
            return None;
        }
        let bpm = f32::from_bits(fields[5] as u32);
        Some(OutputClockSnapshot {
            valid: fields[0] != 0,
            observed_at_ns: fields[1],
            audible_at_ns: fields[2],
            output_frame: fields[3],
            sample_rate_hz: fields[4] as u32,
            master_bpm: (bpm.is_finite() && bpm > 0.0).then_some(bpm),
            downbeat_frame: fields[6],
            freshness_ns: fields[7],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(frame: u64, observed_at_ns: u64) -> OutputClockSnapshot {
        OutputClockSnapshot {
            valid: true,
            observed_at_ns,
            audible_at_ns: observed_at_ns.saturating_add(10_000_000),
            output_frame: frame,
            sample_rate_hz: 48_000,
            master_bpm: Some(120.0),
            downbeat_frame: 0,
            freshness_ns: MIN_FRESHNESS_NS,
        }
    }

    #[test]
    fn nearest_grid_selects_earlier_exact_and_midpoint_future_boundaries() {
        let grid = QuantizeGrid::from_step_64ths(4).unwrap();
        for (input, target) in [
            (6_000.0, 6_000),
            (6_001.0, 6_000),
            (8_999.0, 6_000),
            (9_000.0, 12_000),
            (9_001.0, 12_000),
        ] {
            assert_eq!(
                nearest_grid_frame(input, 48_000, 120.0, 0, grid),
                Some(target)
            );
        }
        // Signed distance before a positive anchor follows the same tie rule.
        assert_eq!(
            nearest_grid_frame(3_000.0, 48_000, 120.0, 12_000, grid),
            Some(6_000)
        );
    }

    #[test]
    fn captured_input_target_is_identical_across_delays_and_callback_partitions() {
        let grid = QuantizeGrid::from_step_64ths(4).unwrap();
        let captured_ns = 1_197_400_000; // output frame 8,995.2, just before midpoint
        for (frame, observed) in [
            (8_192, 1_170_666_667),
            (9_216, 1_192_000_000),
            (12_000, 1_250_000_000),
        ] {
            let clock = snapshot(frame, observed);
            let now_ns = observed.max(captured_ns) + 10_000_000;
            assert_eq!(
                clock.input_target_frame(Some(captured_ns), now_ns, grid),
                Some(6_000)
            );
        }
        // Precisely representable callback/grid ties retain the future rule.
        let clock = snapshot(9_600, 1_200_000_000);
        assert_eq!(
            clock.input_target_frame(Some(1_197_500_000), 1_250_000_000, grid),
            Some(12_000)
        );
    }

    #[test]
    fn missing_future_stale_and_out_of_range_times_have_no_diagnostic_target() {
        let clock = snapshot(48_000, 1_000_000_000);
        let grid = QuantizeGrid::from_step_64ths(4).unwrap();
        assert_eq!(validated_input_timestamp(Some(0), 0), Some(0));
        assert_eq!(validated_input_timestamp(Some(11), 10), None);
        assert_eq!(clock.input_target_frame(None, 1_010_000_000, grid), None);
        assert_eq!(
            clock.input_target_frame(Some(1_020_000_000), 1_010_000_000, grid),
            None
        );
        assert_eq!(
            clock.input_target_frame(Some(1_000_000_000), 1_100_000_001, grid),
            None
        );
        assert_eq!(clock.input_target_frame(Some(0), 1_010_000_000, grid), None); // before output frame zero
        let mut bad = clock;
        bad.master_bpm = None;
        assert_eq!(
            bad.input_target_frame(Some(1_000_000_000), 1_010_000_000, grid),
            None
        );
    }

    #[test]
    fn invalid_driver_delay_and_discontinuity_invalidate_then_recover() {
        let mut mapper = OutputClockMapper::new();
        assert!(
            mapper
                .observe(snapshot(0, 1_000_000_000), Some(10_000_000), 480)
                .valid
        );
        assert!(
            mapper
                .observe(snapshot(480, 1_010_000_000), Some(10_000_000), 480)
                .valid
        );
        assert!(
            !mapper
                .observe(snapshot(960, 1_020_000_000), Some(100_000_000), 480)
                .valid
        );
        assert!(
            mapper
                .observe(snapshot(1_440, 1_030_000_000), Some(10_000_000), 480)
                .valid
        );
        for delay in [None, Some(MAX_DEVICE_DELAY_NS + 1)] {
            assert!(
                !mapper
                    .observe(snapshot(1_920, 1_040_000_000), delay, 480)
                    .valid
            );
        }
        assert!(
            !mapper
                .observe(snapshot(1_920, u64::MAX), Some(1), 480)
                .valid
        );
    }

    #[test]
    fn shared_snapshot_is_coherent_and_busy_writer_never_blocks_reader() {
        let shared = SharedOutputClock::new();
        assert_eq!(shared.read(), None);
        let value = snapshot(48_000, 1_000_000_000);
        shared.publish(value);
        assert_eq!(shared.read(), Some(value));
        shared.sequence.store(3, Ordering::SeqCst);
        assert_eq!(shared.read(), None);
    }
}
