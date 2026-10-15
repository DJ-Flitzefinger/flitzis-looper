//! Normal worker-request catch-up, independently addressed finite PCM and raw native/FIFO output.
//! These checks do not certify readiness of the first ordinary performer attack.

use super::*;
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
use crate::audio_engine::native_history_permit::{NativeHistoryContext, NativeHistoryPermit};
use crate::audio_engine::native_source_coverage::NormalLoopCoverage;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_reader::{
    ExplicitSeekMode, FrameRange, SourceReadPlan, StemRenderSelection, StemTransition,
};
use crate::audio_engine::stretch_processor::FixedFifo;
use crate::messages::{ResidentContext, SampleBuffer};
use std::collections::VecDeque;

const RATE: u32 = 48_000;
const SOURCE_HZ: f64 = 160.0;
const REGION: FrameRange = FrameRange {
    start: 100,
    end: 12_100,
};
const ORIGIN_OFFSET: usize = 17;
const ACTIVE_AT_CAPTURE: usize = 157;
const CAPTURE_FRAME: u64 = 12_345;

struct Source {
    full: SampleBuffer,
    sample: SampleBuffer,
    ownership: Arc<InputRuntimeOwnership>,
    acknowledgements: Arc<CurrentTimingAcknowledgements>,
    playback: SourcePlayback,
    plan: SourceReadPlan,
    pitch: PitchIntent,
    key_lock: bool,
    ratio: f64,
}

impl Source {
    fn new(base: i32, extra: i32, ratio: f64, key_lock: bool) -> Self {
        let full = SampleBuffer {
            channels: 1,
            residency: None,
            samples: (0..16_000)
                .map(|frame| {
                    (frame as f64 * SOURCE_HZ * std::f64::consts::TAU / f64::from(RATE)).sin()
                        as f32
                        * 0.25
                })
                .collect::<Vec<_>>()
                .into(),
        }
        .with_complete_source(RATE);
        let sample = full
            .window(
                REGION.start,
                REGION.end,
                2,
                ResidentContext::KeyLockFiniteLoop,
            )
            .unwrap();
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        ownership.publish_test_source(0, &sample, RATE, 7);
        let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
        assert!(ownership.acknowledge_callback_source(key));
        let mut playback = SourcePlayback::new(
            REGION.start + ORIGIN_OFFSET,
            ExplicitSeekMode::Normal,
            ratio,
        );
        playback.configure(full.frame_count(), REGION);
        playback.advance(ACTIVE_AT_CAPTURE);
        let plan = SourceReadPlan {
            channels: 1,
            sample_frames: full.frame_count(),
            frame_pos: playback.position().frame,
            loop_region: REGION,
            loop_period: None,
            seek_mode: ExplicitSeekMode::Normal,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        };
        Self {
            full,
            sample,
            ownership,
            acknowledgements: Arc::default(),
            playback,
            plan,
            pitch: PitchIntent::new(base, extra).unwrap(),
            key_lock,
            ratio,
        }
    }

    fn permit(&self, lane: &KeyLockPreparationLane) -> NativeHistoryPermit {
        NativeHistoryContext {
            id: 0,
            ownership: &self.ownership,
            acknowledgements: &self.acknowledgements,
            preparation_epoch: lane.source_epoch_owner(),
        }
        .capture(&self.sample, RATE, None)
        .unwrap()
    }

    fn feed<'a>(
        &'a self,
        permit: &'a NativeHistoryPermit,
        playback: &'a SourcePlayback,
        output_frame: u64,
    ) -> ProductiveSourceFeed<'a> {
        let position = playback.position();
        ProductiveSourceFeed {
            pitch_intent: self.pitch,
            key_lock: self.key_lock,
            sample: &self.sample,
            stems: None,
            outgoing_stems: None,
            sample_rate_hz: RATE,
            accepted: None,
            playback,
            plan: SourceReadPlan {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..self.plan
            },
            permit: Some(permit),
            output_frame: Some(output_frame),
        }
    }

    /// Independent signed output coordinate and physical loop interpolation. No production
    /// source clock, reader, pitch conversion or native adapter supplies this PCM reference.
    fn pcm(&self, first_active_frame: i64, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|offset| {
                let phase = (ORIGIN_OFFSET as f64
                    + (first_active_frame as f64 + offset as f64) * self.ratio)
                    .rem_euclid((REGION.end - REGION.start) as f64);
                let local = phase.floor() as usize;
                let left = self.full.samples[REGION.start + local];
                let right =
                    self.full.samples[REGION.start + (local + 1) % (REGION.end - REGION.start)];
                left + (right - left) * phase.fract() as f32
            })
            .collect()
    }
}

struct RawNative {
    native: RubberBandLiveShifter,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
    block: usize,
    history: usize,
}

impl RawNative {
    fn new(pitch: f64) -> Self {
        let mut native = RubberBandLiveShifter::new(RATE, 1).unwrap();
        let block = native.block_size();
        // Independent explicit native setup, without the production preparation/history helper.
        if pitch > 2.0 {
            native.set_pitch_scale(1.0).unwrap();
            native.reset_for_preparation();
            native
                .shift(&[vec![0.0; block]], &mut [vec![0.0; block]])
                .unwrap();
            native.set_pitch_scale(pitch).unwrap();
        } else {
            native.set_pitch_scale(pitch).unwrap();
            native.reset_for_preparation();
        }
        let history = if (0.5..=2.0).contains(&pitch) {
            4096
        } else {
            // Pinned 48-kHz Short window = 2048. Native delay and two adapter blocks
            // complete the declared profile; do not ask production source_history_frames.
            ((2048.0 * pitch.max(1.0 / pitch)).ceil() as usize + native.start_delay() + 2 * block)
                .max(4096)
                .div_ceil(block)
                * block
        };
        Self {
            native,
            input: VecDeque::new(),
            output: std::iter::repeat_n(0.0, block - 1).collect(),
            block,
            history,
        }
    }

    fn render(&mut self, feed: &[f32]) -> Vec<f32> {
        self.input.extend(feed.iter().copied());
        while self.input.len() >= self.block {
            let input = vec![self.input.drain(..self.block).collect::<Vec<_>>()];
            let mut output = vec![vec![0.0; self.block]];
            self.native.shift(&input, &mut output).unwrap();
            self.output.extend(output[0].iter().copied());
        }
        (0..feed.len())
            .map(|_| self.output.pop_front().unwrap_or(0.0))
            .collect()
    }
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "catch-up worker deadline"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn assert_frequency(pcm: &[f32], expected: f64) {
    let crossings = pcm
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
        .map(|(index, pair)| index as f64 - f64::from(pair[0]) / f64::from(pair[1] - pair[0]))
        .collect::<Vec<_>>();
    assert!(crossings.len() > 8, "prepared continuation is inaudible");
    let actual =
        (crossings.len() - 1) as f64 * f64::from(RATE) / (crossings.last().unwrap() - crossings[0]);
    assert!(
        (1200.0 * (actual / expected).log2()).abs() < 20.0,
        "prepared frequency={actual}, requested={expected}"
    );
}

#[test]
fn normal_worker_requests_keep_e_and_prepare_all_pitch_totals_with_real_finite_pcm() {
    let (mut lanes, _worker) = create_key_lock_preparation(1, RATE, 1).unwrap();
    let lane = &mut lanes[0];
    let mut current = Box::new(NativeAdapterState::new(lane.take_initial(), 1));
    let cases = (-23..=24)
        .filter(|total| *total != 0)
        .map(|total| {
            let base = if total < -18 {
                -5
            } else if total > 18 {
                6
            } else {
                0
            };
            (base, total - base, 1.0, false)
        })
        .chain([
            (-5, -18, 2.0, true),
            (6, 18, 0.5, true),
            (0, 7, 0.5, false),
            (0, -7, 2.0, false),
            (0, 0, 0.5, true),
            (0, 0, 2.0, true),
        ]);
    for (index, (base, extra, ratio, key_lock)) in cases.enumerate() {
        let source = Source::new(base, extra, ratio, key_lock);
        let permit = source.permit(lane);
        let id = index as u64 + 1;
        assert!(lane.request_source(
            &source.feed(&permit, &source.playback, CAPTURE_FRAME),
            0,
            id,
        ));
        wait_until(|| lane.prepared_state().is_some() || lane.worker_failed());
        assert!(!lane.worker_failed());
        let endpoint = CAPTURE_FRAME + 4096;
        let pitch =
            2.0_f64.powf(f64::from(base + extra) / 12.0) / if key_lock { ratio } else { 1.0 };
        let mut raw = RawNative::new(pitch);
        let ready = lane.prepared_state().unwrap();
        let owner = ready.source.as_ref().unwrap();
        assert_eq!(owner.target_output_frame, endpoint);
        assert_eq!(owner.request_id, id);
        assert_eq!(owner.pitch_intent, source.pitch);
        assert_eq!(owner.key_lock, key_lock);
        assert_eq!(ready.pitch_scale, pitch);
        assert_eq!(ready.primed_output_frames, raw.history);
        let taps = owner.coverage.as_ref().unwrap().prepared_taps;
        assert_eq!(taps.missing_reads, 0);
        assert!(taps.min_frame.unwrap() >= REGION.start);
        assert!(taps.max_frame.unwrap() < REGION.end);
        let mut playback = source.playback;
        playback.advance(4096);
        playback.chunk(1024);
        assert!(owner.playback.matches_exact(&playback));
        let phase = (ORIGIN_OFFSET as f64 + (ACTIVE_AT_CAPTURE + 4096) as f64 * ratio)
            .rem_euclid((REGION.end - REGION.start) as f64);
        assert_eq!(
            owner.playback.position().frame,
            REGION.start + phase.floor() as usize
        );
        assert_eq!(
            owner.playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        let first_history = ACTIVE_AT_CAPTURE as i64 + 4096 - raw.history as i64;
        let _ = raw.render(&source.pcm(first_history, raw.history));
        assert!(matches!(
            lane.exchange_source(
                &mut current,
                &source.feed(&permit, &playback, endpoint),
                0,
                id,
            ),
            SourceExchange::Adopted(actual) if actual == id
        ));
        let mut actual_pcm = Vec::new();
        let mut consumed = 0;
        for frames in [1, 31, 127, 512, 777, 96, 257].into_iter().cycle() {
            let frames = frames.min(24_000 - consumed);
            let feed = source.pcm((ACTIVE_AT_CAPTURE + 4096 + consumed) as i64, frames);
            let expected = raw.render(&feed);
            let mut output = vec![vec![0.0; frames]];
            current.render(&[feed], &mut output, frames, pitch).unwrap();
            assert_eq!(
                output[0], expected,
                "base={base} extra={extra} ratio={ratio}"
            );
            assert_eq!(current.input_fifo[0].len(), raw.input.len());
            assert_eq!(current.output_fifo[0].len(), raw.output.len());
            actual_pcm.extend_from_slice(&output[0]);
            consumed += frames;
            if consumed == 24_000 {
                break;
            }
        }
        assert_frequency(
            &actual_pcm,
            SOURCE_HZ
                * 2.0_f64.powf(f64::from(base + extra) / 12.0)
                * if key_lock { 1.0 } else { ratio },
        );
    }
}

#[test]
fn unavailable_expanded_trajectory_completes_its_request_and_keeps_both_lanes_available() {
    let (mut lanes, _worker) = create_key_lock_preparation(1, RATE, 2).unwrap();
    let mut source = Source::new(6, 18, 1.0, false);
    source.playback.set_target(1.05);
    let permit = source.permit(&lanes[0]);
    assert!(lanes[0].request_source(&source.feed(&permit, &source.playback, CAPTURE_FRAME), 0, 1,));
    wait_until(|| lanes[0].source_work_finished(1));
    assert!(lanes[0].prepared_state().is_none());
    assert!(lanes.iter().all(|lane| !lane.worker_failed()));
    source.playback.set_target(1.0);
    for (index, lane) in lanes.iter_mut().enumerate() {
        let permit = source.permit(lane);
        assert!(lane.request_source(
            &source.feed(&permit, &source.playback, CAPTURE_FRAME),
            0,
            index as u64 + 2,
        ));
    }
    wait_until(|| lanes.iter_mut().all(|lane| lane.prepared_state().is_some()));
    assert!(lanes.iter().all(|lane| !lane.worker_failed()));
}

#[test]
fn missing_coverage_is_local_but_native_fifo_invalid_block_size_remains_fatal() {
    let (mut lanes, _worker) = create_key_lock_preparation(1, RATE, 1).unwrap();
    let lane = &mut lanes[0];
    let source = Source::new(6, 18, 1.0, false);
    let permit = source.permit(lane);
    let request = |coverage| NativeHistoryRequest {
        pitch_intent: source.pitch,
        key_lock: false,
        sample: source.sample.clone(),
        stems: None,
        permit: permit.clone(),
        coverage,
        binding: ProductiveSourceBinding::new(&source.sample, RATE, None),
        playback: source.playback,
        plan: source.plan,
        target_output_frame: CAPTURE_FRAME + 4096,
        request_id: 1,
        epoch: 0,
    };
    assert!(
        lane.requests
            .push(super::SourcePreparationRequest::Ordinary(request(None)))
            .is_ok()
    );
    wait_until(|| lane.source_work_finished(1));
    assert!(!lane.worker_failed());
    assert!(lane.prepared_state().is_none());
    assert!(lane.request_source(&source.feed(&permit, &source.playback, CAPTURE_FRAME), 0, 2,));
    wait_until(|| lane.prepared_state().is_some());
    assert!(!lane.worker_failed());

    let coverage = NormalLoopCoverage::capture(
        &source.sample,
        None,
        RATE,
        None,
        source.plan,
        &source.playback,
    )
    .unwrap();
    let mut adapter =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 1).unwrap()), 1);
    adapter.input_fifo[0] = FixedFifo::new(1);
    assert!(matches!(
        adapter.prepare_live_catchup(request(Some(coverage))),
        Err(NativeSourcePreparationError::Backend(
            RubberBandError::InvalidBlockSize
        ))
    ));
}
