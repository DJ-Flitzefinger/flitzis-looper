//! Same-pass mono contributions for the explicit group-response diagnostic only.
//! Storage is prepared and consumed by the test control thread, never in render.

use super::NUM_SAMPLES;

struct Lane {
    source: (usize, usize),
    pcm: Box<[f32]>,
}

pub(in crate::audio_engine) struct GroupResponseCapture {
    start: u64,
    frames: usize,
    lanes: Box<[Option<Lane>]>,
    invalid: bool,
}

impl GroupResponseCapture {
    pub(in crate::audio_engine) fn new(
        start: u64,
        frames: usize,
        sources: &[(usize, (usize, usize))],
    ) -> Self {
        assert!(frames > 0 && !sources.is_empty());
        assert!(start.checked_add(frames as u64).is_some());
        let mut lanes = std::iter::repeat_with(|| None)
            .take(NUM_SAMPLES)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        for &(id, source) in sources {
            assert!(lanes[id].is_none());
            lanes[id] = Some(Lane {
                source,
                pcm: vec![0.0; frames].into_boxed_slice(),
            });
        }
        Self {
            start,
            frames,
            lanes,
            invalid: false,
        }
    }

    // Fixed indexed writes only. Missing frames, source changes and capacity
    // violations invalidate the capture; they never resize, panic or log on RT.
    pub(super) fn record(
        &mut self,
        id: usize,
        source: (usize, usize),
        frame: Option<u64>,
        channel: usize,
        contribution: f32,
    ) {
        let index = frame
            .and_then(|frame| frame.checked_sub(self.start))
            .and_then(|frame| usize::try_from(frame).ok());
        let lane = self.lanes.get_mut(id).and_then(Option::as_mut);
        if let (Some(lane), Some(index)) = (lane, index)
            && channel == 0
            && source == lane.source
            && let Some(value) = lane.pcm.get_mut(index)
        {
            *value += contribution;
            return;
        }
        self.invalid = true;
    }

    pub(in crate::audio_engine) fn pcm(&self, id: usize) -> &[f32] {
        &self.lanes[id].as_ref().expect("declared PAD").pcm
    }

    pub(in crate::audio_engine) fn validate(&self, mix: &[f32]) -> Result<(), &'static str> {
        if self.invalid || mix.len() != self.frames {
            return Err("incomplete or invalid group PCM capture");
        }
        for (frame, &actual) in mix.iter().enumerate() {
            let sum: f32 = self
                .lanes
                .iter()
                .flatten()
                .map(|lane| lane.pcm[frame])
                .sum();
            // Same actual scaled contributions; only grouping same-PAD voices
            // may change floating-point addition order. No waveform fitting.
            if !actual.is_finite()
                || !sum.is_finite()
                || (sum - actual).abs() > 1.0e-6 + 1.0e-6 * actual.abs()
            {
                return Err("group contributions do not reconstruct actual mix");
            }
        }
        if self.lanes.iter().flatten().any(|lane| {
            lane.pcm.iter().any(|value| !value.is_finite())
                || !lane.pcm.iter().any(|value| value.abs() > 1.0e-7)
        }) {
            return Err("missing individual PAD output");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audible_mix_does_not_hide_a_silent_wet_member() {
        let mut capture = GroupResponseCapture::new(100, 3, &[(0, (10, 3)), (1, (20, 3))]);
        capture.record(0, (10, 3), Some(101), 0, 0.5);
        capture.record(1, (20, 3), Some(101), 0, 0.0);
        assert_eq!(
            capture.validate(&[0.0, 0.5, 0.0]),
            Err("missing individual PAD output")
        );
        capture.record(1, (20, 3), Some(102), 0, -0.25);
        assert_eq!(capture.validate(&[0.0, 0.5, -0.25]), Ok(()));
    }

    #[test]
    fn same_pad_voices_accumulate_at_actual_frames() {
        let mut capture = GroupResponseCapture::new(900, 4, &[(5, (10, 20))]);
        capture.record(5, (10, 20), Some(901), 0, 0.25);
        capture.record(5, (10, 20), Some(901), 0, 0.5);
        capture.record(5, (10, 20), Some(903), 0, -0.25);
        assert_eq!(capture.pcm(5), &[0.0, 0.75, 0.0, -0.25]);
        assert_eq!(capture.validate(&[0.0, 0.75, 0.0, -0.25]), Ok(()));
        assert!(capture.validate(&[0.0, 0.75, 0.0]).is_err());
        assert!(capture.validate(&[0.0, 0.5, 0.0, -0.25]).is_err());
    }

    #[test]
    fn identity_frame_channel_and_capacity_errors_fail_closed() {
        for (id, source, frame, channel) in [
            (1, (10, 3), Some(100), 0),
            (0, (11, 3), Some(100), 0),
            (0, (10, 4), Some(100), 0),
            (0, (10, 3), None, 0),
            (0, (10, 3), Some(99), 0),
            (0, (10, 3), Some(103), 0),
            (0, (10, 3), Some(100), 1),
        ] {
            let mut capture = GroupResponseCapture::new(100, 3, &[(0, (10, 3))]);
            capture.record(0, (10, 3), Some(101), 0, 0.5);
            capture.record(id, source, frame, channel, 0.25);
            assert!(capture.validate(&[0.0, 0.5, 0.0]).is_err());
        }
    }
}
