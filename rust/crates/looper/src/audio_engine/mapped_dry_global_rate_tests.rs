// Productive dry SYNC uses the existing rate transaction and the original map owner.
// Declared master segments below are independent of SourcePlayback and its current epoch.
use super::*;
use crate::audio_engine::global_rate_status::{RateError, RateState};
use crate::messages::ControlParameterMessage;

mod mapped_dry_global_cohort_tests {
    include!("mapped_dry_global_cohort_tests.rs");
}

struct Parameters {
    consumer: rtrb::Consumer<ControlParameterMessage>,
}

impl Parameters {
    fn new(path: &mut LaunchPath) -> Self {
        let (producer, consumer) = rtrb::RingBuffer::new(16);
        path.source.engine.test_parameter_producer =
            Some(Arc::new(std::sync::Mutex::new(producer)));
        Self { consumer }
    }

    fn drain(&mut self, path: &mut LaunchPath) {
        drain_parameter_messages(
            &mut self.consumer,
            &mut path.source.mixer,
            &mut path.timeline,
        );
        assert!(self.consumer.is_empty());
    }

    fn coupled(&mut self, path: &mut LaunchPath, speed: f64, period: f64) {
        path.source
            .engine
            .set_speed_and_master_period(speed, period)
            .unwrap();
        path.drain();
        self.drain(path);
        path.render(0);
    }

    fn speed(&mut self, path: &mut LaunchPath, speed: f64) {
        path.source.engine.set_speed(speed).unwrap();
        path.drain();
        self.drain(path);
        path.render(0);
    }
}

fn effect(path: &LaunchPath) -> crate::audio_engine::global_rate_status::RateEffect {
    path.source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
        .effect
}

#[derive(Clone, Copy)]
struct MasterSegment {
    frame: u64,
    beat: f64,
    period: f64,
    actual_epoch: TransportTimeline,
}

fn master_at(segments: &[MasterSegment], frame: u64) -> f64 {
    let segment = segments
        .iter()
        .rev()
        .find(|segment| segment.frame <= frame)
        .unwrap();
    segment.beat + (frame - segment.frame) as f64 / (f64::from(RATE) * segment.period)
}

fn coordinate(segments: &[MasterSegment], frame: u64, launch: u64, variable: bool) -> f64 {
    let (beginning, quarters) = if variable {
        mapped_fixed_pitch_birth_tests::independent_extent()
    } else {
        (0.5, 0.5)
    };
    let beat =
        beginning + (master_at(segments, frame) - master_at(segments, launch)).rem_euclid(quarters);
    let seconds = if !variable {
        beat * (64.0 / f64::from(RATE))
    } else if beat < 0.6 {
        (beat / 0.6) * (48.0 / f64::from(RATE))
    } else {
        (48.0 / f64::from(RATE))
            + ((beat - 0.6) / (1.0 - 0.6)) * ((64.0 / f64::from(RATE)) - (48.0 / f64::from(RATE)))
    };
    seconds * f64::from(RATE)
}

fn pcm_at(source: &crate::messages::SampleBuffer, coordinate: f64) -> [f32; 2] {
    let left = 32 + (coordinate.floor() as usize - 32) % 32;
    let right = 32 + (left + 1 - 32) % 32;
    let fraction = coordinate.fract() as f32;
    std::array::from_fn(|channel| {
        let a = source.samples[(left - source.resident_start()) * 2 + channel];
        let b = source.samples[(right - source.resident_start()) * 2 + channel];
        a + (b - a) * fraction
    })
}

fn render_to(path: &mut LaunchPath, end: u64, partitions: &[usize], output: &mut Vec<f32>) {
    let mut index = 0;
    while path.timeline.output_frame() < end {
        let frames =
            partitions[index % partitions.len()].min((end - path.timeline.output_frame()) as usize);
        output.extend(path.render(frames));
        index += 1;
    }
}

fn verify_history(
    path: &LaunchPath,
    captured: &CapturedPadStart,
    segments: &[MasterSegment],
    variable: bool,
) {
    let mut history = SourceFeedSnapshot::new();
    let voice = path.voice();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let origin = captured.target_frame().unwrap();
    let end = path.timeline.output_frame();
    assert_eq!(history.retained_range(), Some(0..end - origin));
    let mut next = origin;
    let mut boundaries = 0;
    for span in history.spans() {
        let start = span.playback.mapped_output_frame().unwrap();
        assert_eq!(start, next);
        let original_epoch = segments
            .iter()
            .rev()
            .find(|segment| segment.frame <= start)
            .unwrap();
        assert!(
            span.playback
                .mapped_epoch()
                .unwrap()
                .matches_timeline(&original_epoch.actual_epoch)
        );
        for offset in 0..=span.frames {
            // The exclusive endpoint belongs to this original span, including old epochs.
            let position = span.playback.position_at(offset).unwrap();
            let actual = position.frame as f64 + position.fraction;
            let expected = coordinate(segments, start + offset as u64, origin, variable);
            assert!(
                (actual - expected).abs() < 1e-9,
                "actual source n={} got={actual} expected={expected}",
                start + offset as u64
            );
        }
        next += span.frames as u64;
        boundaries += 1;
    }
    assert_eq!(next, end);
    assert!(boundaries > 12, "actual reads must cross many knots/wraps");
}

fn run_piecewise(variable: bool, rates: &[f64], partitions: &[usize]) -> Vec<f32> {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    let (_job, delivery) = if variable {
        variable_original_delivery(&path)
    } else {
        original_delivery(&path.source)
    };
    publish_original(&path, &delivery, 1);
    let captured = path.start();
    let sample = path.voice().sample.as_ref().unwrap().clone();
    assert!(!crate::audio_engine::native_source_coverage::complete_view_available(&sample));
    assert_eq!((sample.resident_start(), sample.resident_end()), (32, 64));
    let generation = path.voice().generation;
    let initial_epoch = path.voice().source_playback.mapped_epoch().unwrap();
    // Later selected intent is not the authority for an already effective voice.
    publish_original(&path, &delivery, 2);
    let mut segments = vec![MasterSegment {
        frame: 0,
        beat: 0.0,
        period: PERIOD,
        actual_epoch: path.timeline,
    }];
    let mut parameters = Parameters::new(&mut path);
    let mut output = Vec::new();
    for (index, rate) in rates.iter().copied().enumerate() {
        // Declared boundaries precede both the request and inspection of its result.
        let execution = [17, 113, 267, 431][index];
        render_to(&mut path, execution, partitions, &mut output);
        let position = path.voice().source_playback.position();
        let beat = master_at(&segments, execution);
        let old_period = path.timeline.master_period_seconds().unwrap();
        let expected_epoch = initial_epoch.with_timeline(path.timeline).unwrap();
        assert!(
            path.voice()
                .source_playback
                .mapped_epoch()
                .unwrap()
                .matches_exact(expected_epoch)
        );
        parameters.coupled(&mut path, rate, PERIOD / rate);
        let status = effect(&path);
        assert_eq!(status.state, RateState::Applied, "{:?}", status.error);
        assert_eq!(status.execution_frame, Some(execution));
        assert_eq!(status.effective.speed.to_bits(), rate.to_bits());
        assert_eq!(
            status.effective.period.unwrap().to_bits(),
            (PERIOD / rate).to_bits()
        );
        assert_eq!(
            path.timeline
                .beat_position_at_frame(execution)
                .unwrap()
                .to_bits(),
            beat.to_bits()
        );
        assert_eq!(path.voice().source_playback.position(), position);
        assert_eq!(path.voice().generation, generation);
        assert!(
            path.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&captured)
        );
        assert_eq!(captured.target_frame(), Some(0));
        assert_eq!(captured.effective_frame(), Some(0));
        assert!(!path.voice().stretch.has_active_native_ownership());
        assert!(!path.voice().stretch.has_retained_source_input());
        if old_period.to_bits() != (PERIOD / rate).to_bits() {
            segments.push(MasterSegment {
                frame: execution,
                beat,
                period: PERIOD / rate,
                actual_epoch: path.timeline,
            });
        }
        // Generic live-pitch/history capture must keep refusing mapped voices.
        let voice_index = path
            .source
            .mixer
            .voices
            .iter()
            .position(|voice| voice.active && voice.sample_id == 0)
            .unwrap();
        assert!(
            path.source
                .mixer
                .capture_pitch_voice_snapshot(voice_index)
                .is_none()
        );
    }
    render_to(&mut path, 829, partitions, &mut output);
    verify_history(&path, &captured, &segments, variable);
    let expected: Vec<f32> = (0..829)
        .flat_map(|frame| pcm_at(&sample, coordinate(&segments, frame, 0, variable)))
        .collect();
    assert_eq!(output.len(), expected.len());
    let error = output
        .iter()
        .zip(&expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0_f32, f32::max);
    assert!(error < 3e-6, "complete dry PCM error={error}");
    assert!(output.iter().any(|sample| sample.abs() > 0.01));
    output
}

#[test]
fn dry_mapped_coupled_fader_uses_one_piecewise_master_and_each_original_actual_receipt() {
    // Representative continuous controls; these are not the separate 37 pitch obligations.
    for (variable, rates) in [
        (true, &[0.81, 1.16, 1.0, 1.12][..]),
        (false, &[0.80, 1.16, 0.5, 2.0][..]),
    ] {
        let single = run_piecewise(variable, rates, &[1]);
        let unequal = run_piecewise(variable, rates, &[127, 1, 383, 17, 256]);
        assert_eq!(
            single, unequal,
            "caller partitions cannot change dry addresses or PCM"
        );
    }
}

#[test]
fn dry_mapped_speed_observation_and_equal_tuple_keep_epoch_pending_target_and_pcm() {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    let (_job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    let original = path.start();
    let source = path.voice().sample.as_ref().unwrap().clone();
    let initial_epoch = path.timeline;
    let segments = [MasterSegment {
        frame: 0,
        beat: 0.0,
        period: PERIOD,
        actual_epoch: initial_epoch,
    }];
    let mut output = path.render(1);
    let pending = path.capture();
    mapped_fixed_pitch_birth_tests::drain_grid(&mut path);
    assert_eq!(pending.target_frame(), Some(64));
    assert!(
        path.source
            .engine
            .publish_pad_start(&pending, None)
            .unwrap()
    );
    path.drain();
    let mut parameters = Parameters::new(&mut path);
    parameters.speed(&mut path, 1.5);
    assert_eq!(effect(&path).state, RateState::Applied);
    assert!(path.timeline.matches_clock_epoch(&initial_epoch));
    assert!(
        path.voice()
            .mapped_start
            .as_ref()
            .unwrap()
            .same_owner(&original)
    );
    output.extend(path.render(16));
    parameters.coupled(&mut path, 1.5, PERIOD);
    assert_eq!(effect(&path).state, RateState::Applied);
    assert_eq!(effect(&path).effective.speed, 1.5);
    assert!(path.timeline.matches_clock_epoch(&initial_epoch));
    output.extend(path.render(47));
    output.extend(path.render(89));
    assert_eq!(
        (
            pending.state(),
            pending.target_frame(),
            pending.execution_frame(),
            pending.effective_frame()
        ),
        ("applied", Some(64), Some(64), Some(64))
    );
    for frame in 0..64 {
        let expected = pcm_at(&source, coordinate(&segments, frame, 0, true));
        for channel in 0..2 {
            assert!((output[frame as usize * 2 + channel] - expected[channel]).abs() < 3e-6);
        }
    }
    // Retrigger owns a fresh creative phase at the unchanged absolute grid target.
    let mut history = SourceFeedSnapshot::new();
    assert!(path.voice().source_feed_history.capture_into(&mut history));
    assert!(
        path.voice()
            .mapped_start
            .as_ref()
            .unwrap()
            .same_owner(&pending)
    );
    for span in history.spans() {
        let start = span.playback.mapped_output_frame().unwrap();
        for offset in 0..=span.frames {
            let position = span.playback.position_at(offset).unwrap();
            assert!(
                (position.frame as f64 + position.fraction
                    - coordinate(&segments, start + offset as u64, 64, true))
                .abs()
                    < 1e-9
            );
        }
    }
}

#[test]
fn dry_mapped_unsettled_drain_supersession_commits_latest_at_first_settled_actual_frame() {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    let (_job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    let captured = path.start();
    path.render(17);
    let old_epoch = path.timeline;
    let generation = path.voice().generation;
    let mut parameters = Parameters::new(&mut path);
    path.source
        .engine
        .set_speed_and_master_period(0.8, PERIOD / 0.8)
        .unwrap();
    parameters.drain(&mut path); // Commands have not observed this actual revision yet.
    assert_eq!(effect(&path).state, RateState::Preparing);
    assert_eq!(effect(&path).execution_frame, None);
    path.render(5);
    assert!(path.timeline.matches_clock_epoch(&old_epoch));
    path.source
        .engine
        .set_speed_and_master_period(1.16, PERIOD / 1.16)
        .unwrap();
    parameters.drain(&mut path);
    assert_eq!(effect(&path).state, RateState::Preparing);
    let execution = 22;
    assert_eq!(path.timeline.output_frame(), execution);
    let before = path.voice().source_playback.position();
    path.drain();
    path.render(0);
    assert_eq!(
        effect(&path).state,
        RateState::Applied,
        "{:?}",
        effect(&path).error
    );
    assert_eq!(effect(&path).execution_frame, Some(execution));
    assert_eq!(effect(&path).effective.speed, 1.16);
    assert_eq!(path.voice().source_playback.position(), before);
    assert_eq!(path.voice().generation, generation);
    assert!(
        path.voice()
            .mapped_start
            .as_ref()
            .unwrap()
            .same_owner(&captured)
    );
    assert!(!path.source.mixer.has_pending_global_rate());
    path.render(807);
    let segments = [
        MasterSegment {
            frame: 0,
            beat: 0.0,
            period: PERIOD,
            actual_epoch: old_epoch,
        },
        MasterSegment {
            frame: execution,
            beat: execution as f64 / 64.0,
            period: PERIOD / 1.16,
            actual_epoch: path.timeline,
        },
    ];
    verify_history(&path, &captured, &segments, true);
}

#[test]
fn dry_mapped_whole_map_limits_and_stale_source_or_unsupported_cohort_refuse_atomically() {
    for invalid in 0..8 {
        let mut path = mapped_fixed_pitch_birth_tests::configured(0);
        let (_job, delivery) = variable_original_delivery(&path);
        publish_original(&path, &delivery, 1);
        let captured = path.start();
        path.render(17);
        let old_epoch = path.timeline;
        let generation = path.voice().generation;
        let before = path.voice().source_playback;
        let mut speed = 1.16;
        match invalid {
            // The current segment may fit; the complete original map must fit too.
            0 => speed = 0.5,
            1 => speed = 2.0,
            2 => {
                assert!(
                    path.source
                        .engine
                        .current_timing_acknowledgements
                        .acknowledge_for_key(
                            &path.source.engine.input_runtime_ownership,
                            captured.content_key(),
                            1
                        )
                );
            }
            3 => {
                let mut replacement = path.voice().sample.as_ref().unwrap().clone();
                replacement.samples = replacement.samples.to_vec().into();
                path.source
                    .engine
                    .input_runtime_ownership
                    .publish_window(0, &replacement);
            }
            4 => {
                let index = path
                    .source
                    .mixer
                    .voices
                    .iter()
                    .position(|voice| voice.active && voice.sample_id == 0)
                    .unwrap();
                path.source.mixer.voices[index].paused = true;
            }
            5 | 6 => {
                // Genuine Native processing makes this an owned wet participant even
                // without a retained source input. No copied receipt grants this state.
                let index = path
                    .source
                    .mixer
                    .voices
                    .iter()
                    .position(|voice| voice.active && voice.sample_id == 0)
                    .unwrap();
                path.source.mixer.voices[index]
                    .stretch
                    .process_resampled_with_pitch(
                        if invalid == 5 { 512 } else { 1 },
                        1.0,
                        false,
                        crate::audio_engine::pitch_intent::PitchIntent::new(7, 0).unwrap(),
                    );
                assert!(
                    path.source.mixer.voices[index]
                        .stretch
                        .has_active_native_ownership()
                );
                assert_ne!(
                    path.source.mixer.voices[index]
                        .stretch
                        .native_state_address(),
                    0
                );
                assert_eq!(
                    path.source.mixer.rate_voice_needs_history_for_test(
                        index,
                        crate::audio_engine::global_rate_status::RateTuple {
                            speed,
                            period: Some(PERIOD / speed)
                        }
                    ),
                    Ok(false)
                );
            }
            // Binary64 whole-map arithmetic puts this variable fixture just below
            // the actual .5 bound at .80. The flat fixture covers supported .80.
            _ => speed = 0.80,
        }
        let mut parameters = Parameters::new(&mut path);
        parameters.coupled(&mut path, speed, PERIOD / speed);
        assert_eq!(effect(&path).state, RateState::Rejected, "case {invalid}");
        assert_eq!(effect(&path).effective.speed, 1.0);
        assert_eq!(effect(&path).effective.period, Some(PERIOD));
        if !(2..5).contains(&invalid) {
            assert_eq!(effect(&path).error, RateError::MappedTimingUnavailable);
        }
        assert!(path.timeline.matches_clock_epoch(&old_epoch));
        assert_eq!(path.voice().generation, generation);
        assert!(path.voice().source_playback.matches_exact(&before));
        assert!(
            path.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&captured)
        );
    }
}

#[test]
fn dry_mapped_changed_epoch_fences_old_target_then_nonunity_new_start_retrigger_and_stop_work() {
    let mut path = mapped_fixed_pitch_birth_tests::configured(0);
    let (_job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    let original = path.start();
    path.render(1);
    let pending = path.capture();
    mapped_fixed_pitch_birth_tests::drain_grid(&mut path);
    assert_eq!(pending.target_frame(), Some(64));
    assert!(
        path.source
            .engine
            .publish_pad_start(&pending, None)
            .unwrap()
    );
    path.drain();
    path.render(16);
    let generation = path.voice().generation;
    let mut parameters = Parameters::new(&mut path);
    parameters.coupled(&mut path, 1.16, PERIOD / 1.16);
    assert_eq!(effect(&path).state, RateState::Applied);
    path.render(47);
    path.render(1);
    assert_eq!(pending.state(), "rejected");
    assert_eq!(pending.reason(), Some("mapped_timing_unavailable"));
    assert_eq!(pending.target_frame(), Some(64));
    assert_eq!(path.voice().generation, generation);
    assert!(
        path.voice()
            .mapped_start
            .as_ref()
            .unwrap()
            .same_owner(&original)
    );
    for stop_first in [false, true] {
        if stop_first {
            path.source
                .mixer
                .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
        }
        let execution = path.timeline.output_frame();
        let next = path.capture();
        path.publish_capture(&next);
        assert_eq!(
            (next.state(), next.target_frame(), next.effective_frame()),
            ("applied", Some(execution), Some(execution))
        );
        assert!(
            path.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&next)
        );
        assert_eq!(path.voice().source_playback.position().frame, 32);
        assert!(
            path.voice()
                .source_playback
                .mapped_epoch()
                .unwrap()
                .matches_timeline(&path.timeline)
        );
        path.render(97);
    }
    path.source
        .mixer
        .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
    assert!(
        !path
            .source
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
}

#[test]
fn dry_mapped_and_scalar_peer_share_atomic_tuple_while_scalar_keeps_existing_glide() {
    let mut mixed = mapped_fixed_pitch_birth_tests::configured(0);
    let (_job, delivery) = variable_original_delivery(&mixed);
    publish_original(&mixed, &delivery, 1);
    let captured = mixed.start();
    let mapped_source = mixed.voice().sample.as_ref().unwrap().clone();
    let mut scalar = mapped_fixed_pitch_birth_tests::configured(0);
    for path in [&mut mixed, &mut scalar] {
        path.source.mixer.set_pad_loop_region(
            1,
            32.0 / f64::from(RATE),
            Some(64.0 / f64::from(RATE)),
        );
        assert!(path.source.mixer.play_sample(1, 0.25));
    }
    let scalar_voice = |path: &LaunchPath| {
        path.source
            .mixer
            .voices
            .iter()
            .position(|voice| voice.active && voice.sample_id == 1)
            .unwrap()
    };
    let generation = mixed.source.mixer.voices[scalar_voice(&mixed)].generation;
    let mut mixed_parameters = Parameters::new(&mut mixed);
    let mut scalar_parameters = Parameters::new(&mut scalar);
    let mut segments = vec![MasterSegment {
        frame: 0,
        beat: 0.0,
        period: PERIOD,
        actual_epoch: mixed.timeline,
    }];
    let mut mixed_output = Vec::new();
    let mut scalar_output = Vec::new();
    for (execution, rate) in [(17, 0.81), (113, 1.16), (267, 1.0)] {
        render_to(&mut mixed, execution, &[17, 1, 127], &mut mixed_output);
        render_to(&mut scalar, execution, &[17, 1, 127], &mut scalar_output);
        let beat = master_at(&segments, execution);
        mixed_parameters.coupled(&mut mixed, rate, PERIOD / rate);
        scalar_parameters.coupled(&mut scalar, rate, PERIOD / rate);
        assert_eq!(effect(&mixed).state, RateState::Applied);
        assert_eq!(effect(&mixed).execution_frame, Some(execution));
        let actual = &mixed.source.mixer.voices[scalar_voice(&mixed)];
        let reference = &scalar.source.mixer.voices[scalar_voice(&scalar)];
        assert_eq!(actual.generation, generation);
        assert!(
            actual
                .source_playback
                .matches_exact(&reference.source_playback)
        );
        segments.push(MasterSegment {
            frame: execution,
            beat,
            period: PERIOD / rate,
            actual_epoch: mixed.timeline,
        });
    }
    render_to(&mut mixed, 829, &[17, 1, 127], &mut mixed_output);
    render_to(&mut scalar, 829, &[17, 1, 127], &mut scalar_output);
    assert!(
        mixed.source.mixer.voices[scalar_voice(&mixed)]
            .source_playback
            .matches_exact(&scalar.source.mixer.voices[scalar_voice(&scalar)].source_playback)
    );
    verify_history(&mixed, &captured, &segments, true);
    for frame in 0..829 {
        let mapped = pcm_at(&mapped_source, coordinate(&segments, frame, 0, true));
        for (channel, mapped_sample) in mapped.into_iter().enumerate() {
            let index = frame as usize * 2 + channel;
            assert!((mixed_output[index] - mapped_sample - scalar_output[index]).abs() < 4e-6);
        }
    }
}
