use super::constant_timing::AcceptedTimingProjection;
use super::native_history_permit::NativeHistoryPermit;
use super::prepared_native_history::{
    NativeAdapterState, PITCH_SCALE_EPSILON, PREPARED_HISTORY_FRAMES,
};
use super::productive_source_history::{ProductiveSourceBinding, ProductiveSourceHistory};
use super::source_playback::SourcePlayback;
use super::source_reader::SourceReadPlan;
#[cfg(test)]
use crate::audio_engine::key_lock_preparation::create_key_lock_preparation;
use crate::audio_engine::key_lock_preparation::{
    KeyLockPreparationLane, KeyLockPreparationWorker, SourceExchange,
};
#[cfg(test)]
use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
use crate::audio_engine::rubberband_backend::pitch_scale_for_tempo_ratio;
use crate::messages::{PreparedStemSet, SampleBuffer};

pub(crate) struct ProductiveSourceFeed<'a> {
    pub(crate) sample: &'a SampleBuffer,
    pub(crate) stems: Option<&'a PreparedStemSet>,
    pub(crate) sample_rate_hz: u32,
    pub(crate) accepted: Option<AcceptedTimingProjection>,
    pub(crate) plan: SourceReadPlan,
    pub(crate) playback: &'a SourcePlayback,
    pub(crate) permit: Option<&'a NativeHistoryPermit>,
    pub(crate) output_frame: Option<u64>,
}

/// Default maximum block size handled by the per-voice DSP wrapper.
///
/// Source playback fills these fixed output-domain buffers before Key Lock processing. The bound
/// covers the 512-frame CPAL callback and bounded internal segments without resizing.
pub const DEFAULT_BLOCK_SAMPLES: usize = 1024;

#[cfg(test)]
const DEFAULT_SAMPLE_RATE_HZ: f32 = 48_000.0;
#[cfg(test)]
const RUBBERBAND_MIN_SAMPLE_RATE_HZ: f32 = 8_000.0;

pub struct StretchProcessor {
    channels: usize,
    varispeed: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    native: Box<NativeAdapterState>,
    preparation_epoch: u64,
    request_id: u64,
    request_pending: bool,
    adopted_request_id: Option<u64>,
    preparation: KeyLockPreparationLane,
    productive_history: Option<ProductiveSourceHistory>,
    // Standalone processors retain their worker. Mixer processors share an engine worker.
    _preparation_worker: Option<KeyLockPreparationWorker>,
}

unsafe impl Send for StretchProcessor {}

impl StretchProcessor {
    #[cfg(test)]
    pub fn new(channels: usize) -> Self {
        Self::with_sample_rate(channels, DEFAULT_SAMPLE_RATE_HZ)
    }

    #[cfg(test)]
    pub fn with_sample_rate(channels: usize, sample_rate_hz: f32) -> Self {
        let (mut lanes, worker) =
            create_key_lock_preparation(channels, sample_rate_to_u32(sample_rate_hz), 1)
                .expect("failed to prepare standalone Key Lock processor");
        let mut processor = Self::with_preparation_lane(channels, lanes.remove(0));
        processor._preparation_worker = Some(worker);
        processor
    }

    pub(crate) fn with_preparation_lane(
        channels: usize,
        mut preparation: KeyLockPreparationLane,
    ) -> Self {
        let varispeed = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();
        let output = (0..channels)
            .map(|_| vec![0.0; DEFAULT_BLOCK_SAMPLES])
            .collect();

        let rubberband = preparation.take_initial();
        Self {
            channels,
            varispeed,
            output,
            native: Box::new(NativeAdapterState::new(rubberband, channels)),
            preparation_epoch: 0,
            request_id: 0,
            request_pending: false,
            adopted_request_id: None,
            preparation,
            productive_history: None,
            _preparation_worker: None,
        }
    }

    pub fn reset(&mut self) {
        self.invalidate_prepared();
        self.productive_history = None;
        for channel in &mut self.varispeed {
            channel.fill(0.0);
        }
        for channel in &mut self.output {
            channel.fill(0.0);
        }
        self.reset_rubberband_state();
    }

    /// Fence a pending source-specific candidate without touching current audio/native history.
    pub(crate) fn invalidate_prepared(&mut self) {
        self.preparation_epoch = self.preparation_epoch.saturating_add(1);
        self.preparation.invalidate_source(self.preparation_epoch);
    }

    pub(crate) fn retire_prepared(&mut self) {
        self.preparation.retire_prepared();
    }

    pub(crate) fn chunk_until_prepared_adoption(
        &mut self,
        output_frame: u64,
        max_frames: usize,
    ) -> usize {
        self.preparation
            .chunk_until_prepared_adoption(output_frame, max_frames)
    }

    /// Read the owning voice's real canonical source before feeding live native/adapter state.
    /// Projection refreshes retain chronological history only across exact source continuity.
    pub(crate) fn process_source(
        &mut self,
        feed: ProductiveSourceFeed<'_>,
        output_frames: usize,
        preserve_pitch: bool,
    ) {
        let frames = output_frames.min(DEFAULT_BLOCK_SAMPLES);
        if frames == 0 {
            return;
        }
        let wet = preserve_pitch
            && (pitch_scale_for_tempo_ratio(feed.playback.tempo_ratio()) - 1.0).abs()
                > PITCH_SCALE_EPSILON;
        if !wet && self.request_pending {
            self.invalidate_prepared();
        }
        if self.request_pending && self.preparation.source_work_finished(self.request_id) {
            self.request_pending = false;
        }
        match self.preparation.exchange_source(
            &mut self.native,
            &feed,
            self.preparation_epoch,
            self.request_id,
        ) {
            SourceExchange::Adopted(request_id) => {
                self.request_pending = false;
                self.adopted_request_id = Some(request_id);
                self.productive_history = Some(ProductiveSourceHistory {
                    binding: ProductiveSourceBinding::new(
                        feed.sample,
                        feed.sample_rate_hz,
                        feed.accepted,
                    ),
                    next_position: feed.playback.position(),
                    fed_output_frames: PREPARED_HISTORY_FRAMES as u64,
                });
            }
            SourceExchange::Rejected(_request_id) => self.request_pending = false,
            SourceExchange::Pending => {}
        }
        if wet
            && !self.request_pending
            && !self
                .native
                .source
                .as_ref()
                .is_some_and(|source| source.matches_contract(&feed, self.preparation_epoch))
            && let Some(request_id) = self.request_id.checked_add(1)
            && self.preparation_epoch != u64::MAX
            && self
                .preparation
                .request_source(&feed, self.preparation_epoch, request_id)
        {
            self.request_id = request_id;
            self.request_pending = true;
        }
        let binding = ProductiveSourceBinding::new(feed.sample, feed.sample_rate_hz, feed.accepted);
        let position = feed.playback.position();
        if self
            .productive_history
            .is_some_and(|history| !history.continues(binding, position))
        {
            // Discard only fixed adapter storage. Used native state stays uniquely owned until
            // the existing worker lane can exchange and reset it off the callback.
            self.reset_rubberband_state();
        }
        let prior_frames = self
            .productive_history
            .map_or(0, |history| history.fed_output_frames);
        feed.plan.fill_fractional_buffers(
            feed.sample,
            feed.stems,
            feed.playback,
            self.resampled_buffers_mut(frames),
            frames,
        );
        self.process_resampled(frames, feed.playback.tempo_ratio(), preserve_pitch);
        if self.native.active {
            self.productive_history = Some(ProductiveSourceHistory {
                binding,
                next_position: feed.playback.position_at(frames),
                fed_output_frames: prior_frames.saturating_add(frames as u64),
            });
        } else {
            // Dry bypass and failed reserve admission own no productive wet history.
            self.productive_history = None;
        }
    }

    /// Returns fixed feed storage whose first `output_frames` samples the source reader fills.
    pub fn resampled_buffers_mut(&mut self, output_frames: usize) -> &mut [Vec<f32>] {
        debug_assert!(output_frames <= DEFAULT_BLOCK_SAMPLES);
        &mut self.varispeed
    }

    /// Applies Key Lock or dry bypass to the already resampled output-domain feed.
    pub fn process_resampled(
        &mut self,
        output_frames: usize,
        tempo_ratio: f64,
        preserve_pitch: bool,
    ) {
        if self.channels == 0 {
            return;
        }

        let output_samples = output_frames.min(DEFAULT_BLOCK_SAMPLES);
        let pitch_scale = pitch_scale_for_tempo_ratio(tempo_ratio);

        if preserve_pitch
            && (pitch_scale - 1.0).abs() > PITCH_SCALE_EPSILON
            && self.native.rubberband.is_some()
        {
            self.process_rubberband(output_samples, pitch_scale);
        } else {
            self.deactivate_rubberband_if_needed();
            self.copy_varispeed_output(output_samples);
        }
    }

    pub fn output_buffers(&self) -> &[Vec<f32>] {
        &self.output
    }

    fn process_rubberband(&mut self, output_samples: usize, pitch_scale: f64) {
        if !self.native.active && self.native.dirty {
            if !self.preparation.exchange(&mut self.native.rubberband) {
                self.silence_output(output_samples);
                return;
            }
            self.native.dirty = false;
            self.native.used = false;
            self.native.pitch_scale = 1.0;
        }
        if self
            .native
            .render(
                &self.varispeed,
                &mut self.output,
                output_samples,
                pitch_scale,
            )
            .is_err()
        {
            self.reset_rubberband_state();
            self.silence_output(output_samples);
        }
    }

    fn copy_varispeed_output(&mut self, output_samples: usize) {
        for channel in 0..self.channels {
            self.output[channel][..output_samples]
                .copy_from_slice(&self.varispeed[channel][..output_samples]);
        }
    }

    fn silence_output(&mut self, output_samples: usize) {
        for channel in &mut self.output {
            channel[..output_samples].fill(0.0);
        }
    }

    fn deactivate_rubberband_if_needed(&mut self) {
        if self.native.active {
            self.reset_rubberband_state();
        }
    }

    fn reset_rubberband_state(&mut self) {
        self.productive_history = None;
        self.preparation
            .retire_source_owner(&mut self.native.source);
        self.native.reset_adapter();
    }

    #[cfg(test)]
    pub(crate) fn processing_capacity(&self) -> usize {
        self.varispeed.first().map_or(0, Vec::len)
    }

    #[cfg(test)]
    pub(crate) fn varispeed_buffers(&self) -> &[Vec<f32>] {
        &self.varispeed
    }

    #[cfg(test)]
    pub(crate) fn rubberband_block_size(&self) -> usize {
        self.native.block_size
    }

    #[cfg(test)]
    pub(crate) fn rubberband_start_delay(&self) -> usize {
        self.native
            .rubberband
            .as_ref()
            .map_or(0, RubberBandLiveShifter::start_delay)
    }

    #[cfg(test)]
    pub(crate) fn adapter_delay_frames(&self) -> usize {
        self.native.block_size.saturating_sub(1)
    }

    #[cfg(test)]
    pub(crate) fn rubberband_input_fifo_capacity(&self) -> usize {
        self.native
            .input_fifo
            .first()
            .map_or(0, FixedFifo::capacity)
    }

    #[cfg(test)]
    pub(crate) fn rubberband_output_fifo_capacity(&self) -> usize {
        self.native
            .output_fifo
            .first()
            .map_or(0, FixedFifo::capacity)
    }

    #[cfg(test)]
    pub(crate) fn productive_history(&self) -> Option<ProductiveSourceHistory> {
        self.productive_history
    }

    #[cfg(test)]
    pub(crate) fn native_state_address(&self) -> usize {
        self.native
            .rubberband
            .as_ref()
            .map_or(0, RubberBandLiveShifter::state_address)
    }

    #[cfg(test)]
    pub(crate) fn pending_fifo_frames(&self) -> (usize, usize) {
        (
            self.native.input_fifo[0].len(),
            self.native.output_fifo[0].len(),
        )
    }

    #[cfg(test)]
    pub(crate) fn fail_preparation_worker(&self) {
        self.preparation.fail_worker();
    }

    #[cfg(test)]
    pub(crate) fn prepared_target_output_frame(&mut self) -> Option<u64> {
        self.preparation
            .prepared_state()?
            .source
            .as_ref()
            .map(|source| source.target_output_frame)
    }

    #[cfg(test)]
    pub(crate) fn source_preparation_ready(&mut self) -> bool {
        self.preparation.prepared_state().is_some()
    }

    #[cfg(test)]
    pub(crate) fn prepared_native_address(&mut self) -> usize {
        self.preparation
            .prepared_state()
            .and_then(|state| state.rubberband.as_ref())
            .map_or(0, RubberBandLiveShifter::state_address)
    }

    #[cfg(test)]
    pub(crate) fn prepared_fifo_frames(&mut self) -> Option<(usize, usize)> {
        self.preparation
            .prepared_state()
            .map(|state| (state.input_fifo[0].len(), state.output_fifo[0].len()))
    }

    #[cfg(test)]
    pub(crate) fn adopted_request_id(&self) -> Option<u64> {
        self.adopted_request_id
    }
}

#[cfg(test)]
fn sample_rate_to_u32(sample_rate_hz: f32) -> u32 {
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return DEFAULT_SAMPLE_RATE_HZ as u32;
    }

    sample_rate_hz
        .round()
        .clamp(RUBBERBAND_MIN_SAMPLE_RATE_HZ, u32::MAX as f32) as u32
}

pub(crate) struct FixedFifo {
    buffer: Vec<f32>,
    read_pos: usize,
    write_pos: usize,
    len: usize,
}

impl FixedFifo {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity.max(1)],
            read_pos: 0,
            write_pos: 0,
            len: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.read_pos = 0;
        self.write_pos = 0;
        self.len = 0;
    }

    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.buffer.len()
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn push_slice(&mut self, input: &[f32]) -> usize {
        let mut written = 0;
        for sample in input {
            if self.len == self.buffer.len() {
                break;
            }
            self.buffer[self.write_pos] = *sample;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
            self.len += 1;
            written += 1;
        }
        written
    }

    pub(super) fn push_silence(&mut self, frames: usize) {
        debug_assert!(self.len + frames <= self.buffer.len());
        for _ in 0..frames.min(self.buffer.len().saturating_sub(self.len)) {
            self.buffer[self.write_pos] = 0.0;
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
            self.len += 1;
        }
    }

    pub(super) fn pop_into(&mut self, output: &mut [f32]) -> usize {
        let mut read = 0;
        for sample in output {
            if self.len == 0 {
                break;
            }
            *sample = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.buffer.len();
            self.len -= 1;
            read += 1;
        }
        read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate_frequency(samples: &[f32], sample_rate_hz: f32) -> f32 {
        let mut crossings = Vec::new();
        for index in 1..samples.len() {
            let prev = samples[index - 1];
            let current = samples[index];
            if prev <= 0.0 && current > 0.0 {
                let denom = current - prev;
                let frac = if denom.abs() > f32::EPSILON {
                    -prev / denom
                } else {
                    0.0
                };
                crossings.push(index as f32 - 1.0 + frac);
            }
        }

        if crossings.len() < 2 {
            return 0.0;
        }

        let span = crossings[crossings.len() - 1] - crossings[0];
        if span <= 0.0 {
            return 0.0;
        }

        (crossings.len() - 1) as f32 * sample_rate_hz / span
    }

    #[test]
    fn buffers_are_preallocated_for_callback_bounds() {
        let processor = StretchProcessor::new(2);

        assert_eq!(processor.processing_capacity(), DEFAULT_BLOCK_SAMPLES);
        assert!(processor.rubberband_block_size() > 0);
        assert!(processor.rubberband_start_delay() > 0);
        assert!(processor.rubberband_input_fifo_capacity() >= DEFAULT_BLOCK_SAMPLES);
        assert!(processor.rubberband_output_fifo_capacity() >= DEFAULT_BLOCK_SAMPLES);
    }

    #[test]
    fn pitch_scale_tracks_inverse_tempo_ratio() {
        assert!((pitch_scale_for_tempo_ratio(2.0) - 0.5).abs() < f64::EPSILON);
        assert!((pitch_scale_for_tempo_ratio(0.5) - 2.0).abs() < f64::EPSILON);
        assert!((pitch_scale_for_tempo_ratio(1.0) - 1.0).abs() < f64::EPSILON);
        assert!((pitch_scale_for_tempo_ratio(f64::NAN) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn live_processor_applies_binary64_inverse_rate_to_native_pitch() {
        let mut processor = StretchProcessor::new(1);
        let frames = processor.rubberband_block_size().min(DEFAULT_BLOCK_SAMPLES);
        for ratio in [1.371234567890123_f64, 1.3976543210987654_f64] {
            let expected = 1.0 / ratio;
            assert_ne!(expected, f64::from(1.0_f32 / ratio as f32));
            processor.resampled_buffers_mut(frames)[0][..frames].fill(0.0);
            processor.process_resampled(frames, ratio, true);
            assert_eq!(processor.native.pitch_scale, expected);
            assert_eq!(
                processor.native.rubberband.as_ref().unwrap().pitch_scale(),
                expected
            );
            assert!(
                processor.output_buffers()[0][..frames]
                    .iter()
                    .all(|value| value.is_finite())
            );
        }
    }

    #[test]
    fn neutral_key_lock_is_transparent() {
        let mut processor = StretchProcessor::new(1);
        let input = processor.resampled_buffers_mut(256);
        for (index, sample) in input[0].iter_mut().take(256).enumerate() {
            *sample = (index as f32 * 0.01).sin();
        }

        processor.process_resampled(256, 1.0, true);
        let output = &processor.output_buffers()[0][..256];

        for (index, sample) in output.iter().enumerate() {
            let expected = (index as f32 * 0.01).sin();
            assert!((*sample - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn key_lock_off_copies_already_resampled_input() {
        let mut processor = StretchProcessor::new(1);
        let input = processor.resampled_buffers_mut(512);
        for (index, sample) in input[0].iter_mut().take(512).enumerate() {
            *sample = index as f32 * 2.0;
        }

        processor.process_resampled(512, 2.0, false);

        assert_eq!(
            &processor.output_buffers()[0][..512],
            &processor.varispeed_buffers()[0][..512]
        );
        assert_eq!(processor.output_buffers()[0][511], 1022.0);
    }

    #[test]
    fn unavailable_rubberband_output_uses_silence_fallback() {
        let mut processor = StretchProcessor::new(1);
        let output_samples = processor
            .rubberband_block_size()
            .saturating_sub(1)
            .clamp(1, DEFAULT_BLOCK_SAMPLES);
        let input = processor.resampled_buffers_mut(output_samples);
        for sample in input[0].iter_mut().take(output_samples) {
            *sample = 0.5;
        }

        processor.process_resampled(output_samples, 2.0, true);

        assert!(
            processor.output_buffers()[0][..output_samples]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn reset_clears_pending_rubberband_output() {
        let mut processor = StretchProcessor::new(1);
        let block_size = processor.rubberband_block_size().min(DEFAULT_BLOCK_SAMPLES);
        for chunk in 0..4 {
            let input = processor.resampled_buffers_mut(block_size);
            for (index, sample) in input[0].iter_mut().take(block_size).enumerate() {
                *sample = ((chunk * block_size + index) as f32 * 0.031).sin();
            }
            processor.process_resampled(block_size, 2.0, true);
        }

        processor.reset();

        let output_samples = block_size.saturating_sub(1).max(1);
        let input = processor.resampled_buffers_mut(output_samples);
        for sample in input[0].iter_mut().take(output_samples) {
            *sample = 0.5;
        }
        processor.process_resampled(output_samples, 2.0, true);

        assert!(
            processor.output_buffers()[0][..output_samples]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn rubberband_key_lock_reduces_varispeed_pitch_shift() {
        let sample_rate_hz = 48_000.0;
        let input_hz = 440.0;
        let mut processor = StretchProcessor::new(1);
        let mut varispeed = Vec::new();
        let mut locked = Vec::new();

        // The source reader has already applied a 2x source rate to this immutable feed.
        let feed = (0..48 * 512)
            .map(|output_frame| {
                let source_frame = output_frame as f64 * 2.0;
                (source_frame * f64::from(input_hz) * std::f64::consts::TAU
                    / f64::from(sample_rate_hz))
                .sin() as f32
            })
            .collect::<Vec<_>>();
        for input in feed.chunks(512) {
            processor.resampled_buffers_mut(input.len())[0][..input.len()].copy_from_slice(input);
            processor.process_resampled(input.len(), 2.0, false);
            varispeed.extend_from_slice(&processor.output_buffers()[0][..input.len()]);
        }

        processor.reset();
        for input in feed.chunks(512) {
            processor.resampled_buffers_mut(input.len())[0][..input.len()].copy_from_slice(input);
            processor.process_resampled(input.len(), 2.0, true);
            locked.extend_from_slice(&processor.output_buffers()[0][..input.len()]);
        }

        let skip = processor.rubberband_start_delay() + processor.rubberband_block_size() * 2;
        let varispeed_hz = estimate_frequency(&varispeed[skip..], sample_rate_hz);
        let locked_hz = estimate_frequency(&locked[skip..], sample_rate_hz);

        assert!(varispeed_hz > 800.0, "varispeed_hz={varispeed_hz}");
        assert!((360.0..560.0).contains(&locked_hz), "locked_hz={locked_hz}");
    }

    #[test]
    fn rubberband_key_lock_renders_finite_output() {
        let mut processor = StretchProcessor::new(1);
        for chunk in 0..8 {
            let input = processor.resampled_buffers_mut(512);
            for (index, sample) in input[0].iter_mut().take(512).enumerate() {
                let phase = (chunk * 512 + index) as f32 * 1.5 * 0.031;
                *sample = phase.sin() * 0.5;
            }

            processor.process_resampled(512, 1.5, true);

            assert!(
                processor.output_buffers()[0][..512]
                    .iter()
                    .all(|sample| sample.is_finite())
            );
        }
    }

    fn shifted_sequence(partitions: &[usize], total_frames: usize) -> Vec<f32> {
        let mut processor = StretchProcessor::new(1);
        assert_eq!(
            processor.adapter_delay_frames(),
            processor.rubberband_block_size() - 1
        );
        let feed = (0..total_frames)
            .map(|position| {
                let signal = (position as f32 * 0.057).sin() * 0.2;
                if position == 2000 {
                    signal + 0.7
                } else {
                    signal
                }
            })
            .collect::<Vec<_>>();
        let mut output = Vec::with_capacity(total_frames);
        let mut cursor = 0;
        let mut partition = 0;
        while cursor < total_frames {
            let frames = partitions[partition % partitions.len()].min(total_frames - cursor);
            processor.resampled_buffers_mut(frames)[0][..frames]
                .copy_from_slice(&feed[cursor..cursor + frames]);
            processor.process_resampled(frames, 2.0, true);
            output.extend_from_slice(&processor.output_buffers()[0][..frames]);
            // Output always drains completely; no segment can insert extra silence/latency.
            let pending_input = processor.native.input_fifo[0].len();
            let pending_output = processor.native.output_fifo[0].len();
            assert_eq!(
                pending_input + pending_output,
                processor.adapter_delay_frames()
            );
            cursor += frames;
            partition += 1;
        }
        output
    }

    #[test]
    fn fixed_adapter_delay_preserves_output_under_irregular_partitions() {
        let regular = shifted_sequence(&[512], 24_000);
        assert!(regular.iter().any(|sample| sample.abs() > 0.05));
        for partitions in [&[64][..], &[128][..], &[1, 127, 384, 96, 257, 512, 31][..]] {
            let irregular = shifted_sequence(partitions, regular.len());
            assert_eq!(
                regular, irregular,
                "adapter changed output for {partitions:?}"
            );
        }
    }

    #[test]
    fn exhausted_preparation_retains_dirty_state_and_silences_only_shifted_audio() {
        let mut processor = StretchProcessor::new(1);
        // Stop the owned test worker outside rendering. Exactly one startup reserve remains.
        drop(processor._preparation_worker.take());
        processor.resampled_buffers_mut(512)[0][..512].fill(0.5);
        processor.process_resampled(512, 2.0, true);
        processor.reset();
        processor.resampled_buffers_mut(512)[0][..512].fill(0.5);
        processor.process_resampled(512, 2.0, true);
        assert!(!processor.native.dirty);
        assert!(processor.native.used);

        processor.reset();
        for _ in 0..4 {
            processor.resampled_buffers_mut(512)[0][..512].fill(0.5);
            processor.process_resampled(512, 2.0, true);
            assert!(processor.native.dirty);
            assert!(!processor.native.active);
            assert!(processor.native.rubberband.is_some());
            assert!(
                processor.output_buffers()[0][..512]
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        processor.process_resampled(512, 2.0, false);
        assert!(
            processor.output_buffers()[0][..512]
                .iter()
                .all(|sample| *sample == 0.5)
        );
        assert!(processor.native.dirty);
    }

    #[test]
    fn unused_warm_state_survives_repeated_start_invalidations() {
        let mut processor = StretchProcessor::new(1);
        for _ in 0..10 {
            processor.reset();
            assert!(!processor.native.dirty);
        }
        processor.resampled_buffers_mut(512)[0][..512].fill(0.2);
        processor.process_resampled(512, 2.0, true);
        assert!(processor.native.used);
        assert!(!processor.native.dirty);
    }
}
