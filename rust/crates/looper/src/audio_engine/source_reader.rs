//! Shared source-domain loop, seek and prepared-stem reading policy.
//!
//! These helpers borrow already loaded immutable buffers and use only scalar state. The mixer
//! and non-realtime preparation code can therefore read the same source without sharing a voice,
//! advancing transport, or changing persisted loop markers.

use super::source_playback::SourcePlayback;
use crate::messages::{
    PreparedStemSet, STEM_BUFFER_COUNT, STEM_COMPONENT_MASK, SampleBuffer, StemMixMode,
};

pub(crate) const STEM_TRANSITION_RAMP_FRAMES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExplicitSeekMode {
    Normal,
    BeforeLoop,
    AfterLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl FrameRange {
    pub(crate) fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

pub(crate) fn effective_loop_region(
    start: usize,
    end: Option<usize>,
    sample_frames: usize,
) -> Option<FrameRange> {
    if sample_frames == 0 {
        return None;
    }

    let start = start.min(sample_frames);
    let end = end.unwrap_or(sample_frames).min(sample_frames);
    if end <= start {
        Some(FrameRange {
            start: 0,
            end: sample_frames,
        })
    } else {
        Some(FrameRange { start, end })
    }
}

pub(crate) fn playhead_before_render(
    frame: usize,
    region: FrameRange,
    mode: ExplicitSeekMode,
) -> usize {
    if mode == ExplicitSeekMode::Normal && (frame < region.start || frame >= region.end) {
        region.start
    } else {
        frame
    }
}

pub(crate) fn explicit_seek_mode_for_frame(
    frame: usize,
    loop_region: FrameRange,
    sample_frames: usize,
) -> ExplicitSeekMode {
    if frame < loop_region.start {
        ExplicitSeekMode::BeforeLoop
    } else if frame >= loop_region.end && loop_region.end < sample_frames {
        ExplicitSeekMode::AfterLoop
    } else {
        ExplicitSeekMode::Normal
    }
}

pub(crate) fn source_frame_for_playback(
    frame_pos: usize,
    offset: usize,
    sample_frames: usize,
    loop_region: FrameRange,
    seek_mode: ExplicitSeekMode,
) -> usize {
    let loop_len = loop_region.len();
    debug_assert!(loop_len > 0);

    match seek_mode {
        ExplicitSeekMode::Normal => {
            let base = frame_pos.saturating_sub(loop_region.start);
            loop_region.start + ((base + offset) % loop_len)
        }
        ExplicitSeekMode::BeforeLoop => {
            let frame = frame_pos.saturating_add(offset);
            if frame < loop_region.start {
                frame
            } else {
                loop_region.start + ((frame - loop_region.start) % loop_len)
            }
        }
        ExplicitSeekMode::AfterLoop => {
            let frame = frame_pos.saturating_add(offset);
            if frame < sample_frames {
                frame
            } else {
                loop_region.start + ((frame - sample_frames) % loop_len)
            }
        }
    }
}

pub(crate) fn advance_playback_position(
    frame_pos: usize,
    input_frames: usize,
    sample_frames: usize,
    loop_region: FrameRange,
    seek_mode: ExplicitSeekMode,
) -> (usize, ExplicitSeekMode) {
    let loop_len = loop_region.len();
    debug_assert!(loop_len > 0);

    match seek_mode {
        ExplicitSeekMode::Normal => {
            let base = frame_pos.saturating_sub(loop_region.start);
            (
                loop_region.start + ((base + input_frames) % loop_len),
                ExplicitSeekMode::Normal,
            )
        }
        ExplicitSeekMode::BeforeLoop => {
            let frame = frame_pos.saturating_add(input_frames);
            if frame < loop_region.start {
                (frame, ExplicitSeekMode::BeforeLoop)
            } else {
                (
                    loop_region.start + ((frame - loop_region.start) % loop_len),
                    ExplicitSeekMode::Normal,
                )
            }
        }
        ExplicitSeekMode::AfterLoop => {
            let frame = frame_pos.saturating_add(input_frames);
            if frame < sample_frames {
                (frame, ExplicitSeekMode::AfterLoop)
            } else {
                (
                    loop_region.start + ((frame - sample_frames) % loop_len),
                    ExplicitSeekMode::Normal,
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StemRenderSelection {
    mode: StemMixMode,
    source_version_hash: u64,
    enabled_mask: u8,
}

impl StemRenderSelection {
    pub(crate) fn full_mix() -> Self {
        Self {
            mode: StemMixMode::FullMix,
            source_version_hash: 0,
            enabled_mask: STEM_COMPONENT_MASK,
        }
    }

    pub(crate) fn from_state(
        mode: StemMixMode,
        source_version_hash: u64,
        enabled_mask: u8,
    ) -> StemRenderSelection {
        match mode {
            StemMixMode::FullMix => StemRenderSelection::full_mix(),
            StemMixMode::AllStems => StemRenderSelection {
                mode,
                source_version_hash,
                enabled_mask: enabled_mask & STEM_COMPONENT_MASK,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StemTransition {
    from: StemRenderSelection,
    elapsed_frames: f64,
    total_frames: usize,
}

impl StemTransition {
    fn inactive() -> Self {
        Self {
            from: StemRenderSelection::full_mix(),
            elapsed_frames: 0.0,
            total_frames: 0,
        }
    }

    pub(crate) fn start(from: StemRenderSelection, total_frames: usize) -> Self {
        if total_frames == 0 {
            return Self::inactive();
        }

        Self {
            from,
            elapsed_frames: 0.0,
            total_frames,
        }
    }

    pub(crate) fn is_active(self) -> bool {
        self.total_frames > 0 && self.elapsed_frames < self.total_frames as f64
    }

    fn gains_at(self, frame_offset: f64) -> (f32, f32) {
        if !self.is_active() {
            return (0.0, 1.0);
        }

        let elapsed = (self.elapsed_frames + frame_offset).min(self.total_frames as f64);
        let to_gain = elapsed as f32 / self.total_frames as f32;
        (1.0 - to_gain, to_gain)
    }

    #[cfg(test)]
    pub(crate) fn advance(&mut self, frames: usize) {
        self.advance_fractional(frames as f64);
    }

    pub(crate) fn advance_fractional(&mut self, frames: f64) {
        if !self.is_active() {
            return;
        }

        self.elapsed_frames += frames;
        if self.elapsed_frames >= self.total_frames as f64 {
            self.clear();
        }
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::inactive();
    }
}

impl Default for StemTransition {
    fn default() -> Self {
        Self::inactive()
    }
}

pub(crate) fn full_stem_available_mask() -> u8 {
    ((1_u16 << STEM_BUFFER_COUNT) - 1) as u8
}

pub(crate) fn stem_index_mask(index: usize) -> u8 {
    if index >= u8::BITS as usize {
        return 0;
    }

    1_u8 << index
}

pub(crate) fn prepared_stem_set_matches_sample(
    stems: &PreparedStemSet,
    sample: &SampleBuffer,
    channels: usize,
    sample_rate_hz: f32,
    sample_frames: usize,
) -> bool {
    if channels == 0 || sample_frames == 0 || sample.samples.len() != sample_frames * channels {
        return false;
    }

    let rounded_sample_rate_hz = sample_rate_hz.round();
    if !rounded_sample_rate_hz.is_finite()
        || rounded_sample_rate_hz <= 0.0
        || stems.sample_rate_hz != rounded_sample_rate_hz as u32
    {
        return false;
    }

    if stems.channels != channels
        || stems.frame_count != sample_frames
        || stems.available_mask != full_stem_available_mask()
        || stems.source_version_hash == 0
        || !std::sync::Arc::ptr_eq(&stems.reference_samples, &sample.samples)
    {
        return false;
    }

    stems
        .stems
        .iter()
        .all(|stem| stem.channels == channels && stem.samples.len() == sample.samples.len())
}

pub(crate) fn prepared_stem_set_for_render<'a>(
    stems: Option<&'a PreparedStemSet>,
    sample: &SampleBuffer,
    channels: usize,
    sample_rate_hz: f32,
    sample_frames: usize,
) -> Option<&'a PreparedStemSet> {
    stems.filter(|stems| {
        prepared_stem_set_matches_sample(stems, sample, channels, sample_rate_hz, sample_frames)
    })
}

pub(crate) fn render_source_sample(
    sample: &SampleBuffer,
    stems: Option<&PreparedStemSet>,
    enabled_stem_mask: u8,
    frame: usize,
    channels: usize,
    channel: usize,
) -> f32 {
    let index = frame * channels + channel;
    if let Some(stems) = stems {
        let enabled_stem_mask = enabled_stem_mask & STEM_COMPONENT_MASK;
        stems
            .stems
            .iter()
            .enumerate()
            .filter(|(stem_index, _)| enabled_stem_mask & stem_index_mask(*stem_index) != 0)
            .map(|(_, stem)| stem.samples[index])
            .sum()
    } else {
        sample.samples[index]
    }
}

pub(crate) fn render_source_selection_sample(
    sample: &SampleBuffer,
    stems: Option<&PreparedStemSet>,
    selection: StemRenderSelection,
    frame: usize,
    channels: usize,
    channel: usize,
) -> f32 {
    match selection.mode {
        StemMixMode::FullMix => sample.samples[frame * channels + channel],
        StemMixMode::AllStems => {
            let matching_stems = stems.filter(|stems| {
                selection.source_version_hash != 0
                    && stems.source_version_hash == selection.source_version_hash
            });
            render_source_sample(
                sample,
                matching_stems,
                selection.enabled_mask,
                frame,
                channels,
                channel,
            )
        }
    }
}

/// One immutable view of source selection and source-frame addressing for a render segment.
///
/// Advancing a voice or a transition remains the caller's responsibility. This keeps a worker's
/// source reads independent from live transport and makes repeated reads deterministic.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceReadPlan {
    pub(crate) channels: usize,
    pub(crate) sample_frames: usize,
    pub(crate) frame_pos: usize,
    pub(crate) loop_region: FrameRange,
    pub(crate) seek_mode: ExplicitSeekMode,
    pub(crate) selection: StemRenderSelection,
    pub(crate) transition: StemTransition,
}

impl SourceReadPlan {
    /// Fill fixed output-domain storage from a configured constant-ratio source chunk.
    ///
    /// Neither the source clock nor this plan's selection transition advances. The caller owns
    /// their progression, so a worker can fill the same feed using independent copied state.
    pub(crate) fn fill_fractional_buffers(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        playback: &SourcePlayback,
        buffers: &mut [Vec<f32>],
        output_frames: usize,
    ) {
        debug_assert!(self.channels > 0 && buffers.len() >= self.channels);
        debug_assert_eq!(sample.samples.len() / self.channels, self.sample_frames);
        debug_assert!(
            buffers
                .iter()
                .take(self.channels)
                .all(|buffer| buffer.len() >= output_frames)
        );
        let tempo_ratio = f64::from(playback.tempo_ratio());
        for frame in 0..output_frames {
            let position = playback.position_at(frame);
            let plan = Self {
                frame_pos: position.frame,
                seek_mode: position.seek_mode,
                ..self
            };
            let source_progress = frame as f64 * tempo_ratio;
            for (channel, buffer) in buffers.iter_mut().enumerate().take(self.channels) {
                buffer[frame] = plan.sample_fractional(
                    sample,
                    stems,
                    position.fraction,
                    source_progress,
                    channel,
                );
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn sample_at_offset(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        source_offset: usize,
        channel: usize,
    ) -> f32 {
        self.sample_at_offset_with_progress(
            sample,
            stems,
            source_offset,
            source_offset as f64,
            channel,
        )
    }

    fn sample_at_offset_with_progress(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        source_offset: usize,
        transition_progress: f64,
        channel: usize,
    ) -> f32 {
        let frame = source_frame_for_playback(
            self.frame_pos,
            source_offset,
            self.sample_frames,
            self.loop_region,
            self.seek_mode,
        );
        if self.transition.is_active() {
            let from_sample = render_source_selection_sample(
                sample,
                stems,
                self.transition.from,
                frame,
                self.channels,
                channel,
            );
            let to_sample = render_source_selection_sample(
                sample,
                stems,
                self.selection,
                frame,
                self.channels,
                channel,
            );
            let (from_gain, to_gain) = self.transition.gains_at(transition_progress);
            from_sample * from_gain + to_sample * to_gain
        } else {
            render_source_selection_sample(
                sample,
                stems,
                self.selection,
                frame,
                self.channels,
                channel,
            )
        }
    }

    /// Interpolate two independently addressed source taps at the same crossfade progress.
    /// Lookahead follows loop/intro/tail policy without advancing playback or the ramp.
    pub(crate) fn sample_fractional(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        fraction: f64,
        transition_progress: f64,
        channel: usize,
    ) -> f32 {
        let left =
            self.sample_at_offset_with_progress(sample, stems, 0, transition_progress, channel);
        if fraction == 0.0 {
            return left;
        }
        let right =
            self.sample_at_offset_with_progress(sample, stems, 1, transition_progress, channel);
        left + (right - left) * fraction as f32
    }

    /// Fills preallocated planar buffers with integer source-frame reads, before varispeed.
    ///
    /// The sample and optional stem set must have the validated channel/frame layout, and every
    /// channel buffer must have room for `input_frames`. No interpolation or timeline advancement
    /// is performed here; those keep their existing processor/mixer ownership.
    #[cfg(test)]
    pub(crate) fn fill_buffers(
        self,
        sample: &SampleBuffer,
        stems: Option<&PreparedStemSet>,
        input: &mut [Vec<f32>],
        input_frames: usize,
    ) {
        let channels = self.channels;
        debug_assert!(channels > 0 && input.len() >= channels);
        debug_assert_eq!(sample.samples.len() / channels, self.sample_frames);
        for (channel, buffer) in input.iter_mut().enumerate().take(channels) {
            debug_assert!(buffer.len() >= input_frames);
            for (offset, sample_ref) in buffer.iter_mut().enumerate().take(input_frames) {
                *sample_ref = self.sample_at_offset(sample, stems, offset, channel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{STEM_MASK_MELODY, STEM_MASK_VOCALS};
    use std::sync::Arc;

    fn stereo_sample(gain: f32) -> SampleBuffer {
        SampleBuffer {
            channels: 2,
            samples: Arc::from(
                (0..6)
                    .flat_map(|frame| [frame as f32 * gain, (frame + 10) as f32 * gain])
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn prepared_stems(reference: &SampleBuffer) -> PreparedStemSet {
        PreparedStemSet {
            reference_samples: reference.samples.clone(),
            publication: crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted(),
            source_version_hash: 42,
            sample_rate_hz: 48_000,
            channels: 2,
            frame_count: 6,
            available_mask: full_stem_available_mask(),
            stems: [1.0, 10.0, 0.0, 0.0, 1000.0].map(stereo_sample),
        }
    }

    fn plan(frame_pos: usize, seek_mode: ExplicitSeekMode) -> SourceReadPlan {
        SourceReadPlan {
            channels: 2,
            sample_frames: 6,
            frame_pos,
            loop_region: FrameRange { start: 2, end: 5 },
            seek_mode,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        }
    }

    #[test]
    fn loop_resolution_and_pre_render_normalization_keep_live_edit_policy() {
        assert_eq!(effective_loop_region(0, None, 0), None);
        assert_eq!(
            effective_loop_region(2, Some(100), 6),
            Some(FrameRange { start: 2, end: 6 })
        );
        for (start, end) in [(6, Some(4)), (2, Some(2)), (100, None)] {
            assert_eq!(
                effective_loop_region(start, end, 6),
                Some(FrameRange { start: 0, end: 6 })
            );
        }
        let region = FrameRange { start: 2, end: 5 };
        assert_eq!(
            playhead_before_render(3, region, ExplicitSeekMode::Normal),
            3
        );
        assert_eq!(
            playhead_before_render(5, region, ExplicitSeekMode::Normal),
            2
        );
        assert_eq!(
            playhead_before_render(1, region, ExplicitSeekMode::BeforeLoop),
            1
        );
        assert_eq!(
            playhead_before_render(5, region, ExplicitSeekMode::AfterLoop),
            5
        );
    }

    #[test]
    fn explicit_seek_reads_track_intro_or_tail_once_then_wraps_the_loop() {
        let sample = stereo_sample(1.0);
        let cases = [
            (
                1,
                ExplicitSeekMode::BeforeLoop,
                [1.0, 2.0, 3.0, 4.0, 2.0, 3.0],
            ),
            (3, ExplicitSeekMode::Normal, [3.0, 4.0, 2.0, 3.0, 4.0, 2.0]),
            (
                5,
                ExplicitSeekMode::AfterLoop,
                [5.0, 2.0, 3.0, 4.0, 2.0, 3.0],
            ),
        ];
        for (frame, mode, expected) in cases {
            let plan = plan(frame, mode);
            assert_eq!(
                explicit_seek_mode_for_frame(frame, plan.loop_region, 6),
                mode
            );
            let mut output = [vec![0.0; 6], vec![0.0; 6]];
            plan.fill_buffers(&sample, None, &mut output, 6);
            assert_eq!(output[0], expected);
            assert_eq!(
                output[1],
                expected.map(|value| value + 10.0),
                "source channels must retain their own stride"
            );
        }
    }

    #[test]
    fn playback_advance_matches_reads_across_split_intro_tail_and_loop_segments() {
        let region = FrameRange { start: 2, end: 5 };
        for (start, mode) in [
            (1, ExplicitSeekMode::BeforeLoop),
            (3, ExplicitSeekMode::Normal),
            (5, ExplicitSeekMode::AfterLoop),
        ] {
            for split in 0..16 {
                let (next, next_mode) = advance_playback_position(start, split, 6, region, mode);
                for offset in 0..16 {
                    assert_eq!(
                        source_frame_for_playback(start, split + offset, 6, region, mode),
                        source_frame_for_playback(next, offset, 6, region, next_mode),
                        "start={start} split={split} offset={offset} mode={mode:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn prepared_stem_selection_keeps_stereo_wrap_and_excludes_instrumental() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let validated = prepared_stem_set_for_render(Some(&stems), &sample, 2, 48_000.0, 6);
        let mut plan = plan(4, ExplicitSeekMode::Normal);
        plan.selection = StemRenderSelection::from_state(StemMixMode::AllStems, 42, u8::MAX);
        let mut output = [vec![0.0; 4], vec![0.0; 4]];
        plan.fill_buffers(&sample, validated, &mut output, 4);
        assert_eq!(output[0], [44.0, 22.0, 33.0, 44.0]);
        assert_eq!(output[1], [154.0, 132.0, 143.0, 154.0]);
    }

    #[test]
    fn missing_or_stale_stems_fall_back_but_an_empty_matching_mask_is_silent() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let mut plan = plan(4, ExplicitSeekMode::Normal);
        plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 7, STEM_MASK_VOCALS);
        assert_eq!(plan.sample_at_offset(&sample, Some(&stems), 0, 1), 1400.0);
        plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_VOCALS);
        assert_eq!(plan.sample_at_offset(&sample, None, 0, 1), 1400.0);
        assert_eq!(plan.sample_at_offset(&sample, Some(&stems), 0, 1), 14.0);
        plan.selection = StemRenderSelection::from_state(StemMixMode::AllStems, 42, 0);
        assert_eq!(plan.sample_at_offset(&sample, Some(&stems), 0, 1), 0.0);
    }

    #[test]
    fn transition_across_segments_reads_both_sides_at_the_same_wrapped_frame() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let mut plan = plan(4, ExplicitSeekMode::Normal);
        plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_MELODY);
        plan.transition = StemTransition::start(
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_VOCALS),
            4,
        );
        let mut whole = [vec![0.0; 6], vec![0.0; 6]];
        plan.fill_buffers(&sample, Some(&stems), &mut whole, 6);
        let mut first = [vec![0.0; 2], vec![0.0; 2]];
        plan.fill_buffers(&sample, Some(&stems), &mut first, 2);
        plan.frame_pos = source_frame_for_playback(4, 2, 6, plan.loop_region, plan.seek_mode);
        plan.transition.advance(2);
        let mut second = [vec![0.0; 4], vec![0.0; 4]];
        plan.fill_buffers(&sample, Some(&stems), &mut second, 4);
        for channel in 0..2 {
            first[channel].extend_from_slice(&second[channel]);
            assert_eq!(whole[channel], first[channel]);
        }
        assert_eq!(whole[0], [4.0, 6.5, 16.5, 31.0, 20.0, 30.0]);
        assert_eq!(whole[1], [14.0, 39.0, 71.5, 108.5, 120.0, 130.0]);
        plan.transition.advance(4);
        assert!(!plan.transition.is_active());
    }

    #[test]
    fn fractional_feed_uses_clock_ratio_and_borrows_source_transition_progress() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        let mut read_plan = plan(1, ExplicitSeekMode::BeforeLoop);
        read_plan.selection =
            StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_MASK_VOCALS);
        read_plan.transition = StemTransition::start(StemRenderSelection::full_mix(), 4);
        read_plan.transition.advance_fractional(0.5);
        let transition = read_plan.transition;
        let mut playback = SourcePlayback::new(3, ExplicitSeekMode::Normal, 0.73);
        playback.configure(6, read_plan.loop_region);
        playback.advance(2);
        playback.set_target(1.25);
        let (frames, ratio) = playback.chunk(12);
        assert_eq!(frames, 12);
        let position = playback.position();
        let mut buffers = [vec![-99.0; 13], vec![-99.0; 13]];
        read_plan.fill_fractional_buffers(&sample, Some(&stems), &playback, &mut buffers, frames);

        // Independent address/interpolation oracle: after two 0.73 frames the base is frame 4,
        // fraction 0.46. The newly accepted target has made only its first 0.05 ratio step.
        let initial_fraction = (2.0 * f64::from(0.73_f32)).fract();
        for frame in 0..frames {
            let source_progress = frame as f64 * f64::from(ratio);
            let distance = initial_fraction + source_progress;
            let lower = 2 + (2 + distance.floor() as usize) % 3;
            let upper = 2 + (lower - 2 + 1) % 3;
            let fraction = distance.fract() as f32;
            let to_gain = ((0.5 + source_progress).min(4.0) / 4.0) as f32;
            for (channel, buffer) in buffers.iter().enumerate() {
                let lower_sample = (lower + channel * 10) as f32;
                let upper_sample = (upper + channel * 10) as f32;
                let interpolated = lower_sample + (upper_sample - lower_sample) * fraction;
                let expected = interpolated * (100.0 * (1.0 - to_gain) + to_gain);
                assert!((buffer[frame] - expected).abs() < 2.0e-4);
            }
        }
        assert_eq!(buffers[0][frames], -99.0);
        assert_eq!(buffers[1][frames], -99.0);
        assert_eq!(playback.position(), position);
        assert_eq!(read_plan.transition, transition);
    }

    #[test]
    fn prepared_source_validation_rejects_incomplete_or_mismatched_buffers() {
        let sample = stereo_sample(100.0);
        let stems = prepared_stems(&sample);
        assert!(prepared_stem_set_matches_sample(
            &stems, &sample, 2, 48_000.0, 6
        ));
        let same_values_new_source = stereo_sample(100.0);
        assert!(!prepared_stem_set_matches_sample(
            &stems,
            &same_values_new_source,
            2,
            48_000.0,
            6
        ));
        for (channels, rate, frames) in [(1, 48_000.0, 6), (2, 44_100.0, 6), (2, 48_000.0, 5)] {
            assert!(!prepared_stem_set_matches_sample(
                &stems, &sample, channels, rate, frames
            ));
        }
        let mut incomplete = stems.clone();
        incomplete.available_mask = STEM_COMPONENT_MASK;
        assert!(prepared_stem_set_for_render(Some(&incomplete), &sample, 2, 48_000.0, 6).is_none());
        let mut stale = stems.clone();
        stale.source_version_hash = 0;
        assert!(prepared_stem_set_for_render(Some(&stale), &sample, 2, 48_000.0, 6).is_none());
        let mut truncated = stems.clone();
        truncated.stems[0].samples = Arc::from(vec![0.0; 10]);
        assert!(prepared_stem_set_for_render(Some(&truncated), &sample, 2, 48_000.0, 6).is_none());
    }
}
