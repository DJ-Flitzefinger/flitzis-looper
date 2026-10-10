//! Independent PCM-knot oracle for finite residency. These synthetic accepted
//! projections exercise the actual mixer reader; productive evidence/ACK is
//! separately covered by the saved finite-source persistence integration test.

use super::*;
use crate::messages::{ResidentContext, STEM_BUFFER_COUNT};
use std::collections::BTreeSet;
use std::sync::Arc;

const CHANNELS: usize = 2;
const SOURCE_FRAMES: usize = 379;
const START: usize = 71;
const LENGTH: usize = 137;

fn knot(frame: usize, channel: usize, tag: usize) -> f32 {
    let word = (frame * frame * 17 + frame * (29 + tag * 11) + channel * 191 + tag * 73) % 1009;
    (word as f32 + 17.0) / 8192.0
}

fn complete_source(rate: u32, tag: usize) -> SampleBuffer {
    SampleBuffer {
        channels: CHANNELS,
        samples: Arc::from(
            (0..SOURCE_FRAMES)
                .flat_map(|frame| (0..CHANNELS).map(move |channel| knot(frame, channel, tag)))
                .collect::<Vec<_>>(),
        ),
        residency: None,
    }
    .with_complete_source(rate)
}

/// The oracle works in phase and absolute knot indices. It never calls the
/// productive source addressing, interpolation or playback-position helpers.
fn taps(start: usize, length: usize, period: f64, distance: f64) -> (usize, usize, f32) {
    let phase = distance.rem_euclid(period);
    if period == length as f64 {
        let lower = phase.floor() as usize;
        (
            start + lower,
            start + (lower + 1) % length,
            (phase - lower as f64) as f32,
        )
    } else {
        let last = ((period.ceil() as usize) - 1).min(length - 1);
        if phase >= last as f64 {
            (
                start + last,
                start,
                ((phase - last as f64) / (period - last as f64)) as f32,
            )
        } else {
            let lower = phase.floor() as usize;
            (
                start + lower,
                start + lower + 1,
                (phase - lower as f64) as f32,
            )
        }
    }
}

fn expected(
    start: usize,
    length: usize,
    period: f64,
    distance: f64,
    channel: usize,
    tag: usize,
) -> f32 {
    let (left, right, alpha) = taps(start, length, period, distance);
    let lower = knot(left, channel, tag);
    lower + (knot(right, channel, tag) - lower) * alpha
}

fn plan(length: usize, period: f64, distance: f64) -> (SourceReadPlan, f64) {
    let phase = distance.rem_euclid(period);
    let whole = phase.floor() as usize;
    (
        SourceReadPlan {
            channels: CHANNELS,
            sample_frames: SOURCE_FRAMES,
            frame_pos: START + whole,
            loop_region: FrameRange {
                start: START,
                end: START + length,
            },
            loop_period: Some(period),
            seek_mode: ExplicitSeekMode::Normal,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        },
        phase - whole as f64,
    )
}

#[test]
fn finite_normal_read_set_matches_independent_virtual_seam_oracle() {
    for length in [1_usize, 2, 7, LENGTH] {
        let mut periods = vec![
            length as f64 - 0.375,
            length as f64,
            length as f64 + 0.375,
            length as f64 + 1.0,
        ];
        if length > 1 {
            periods.push(length as f64 - 1.0);
        }
        if length == 1 {
            periods.extend([0.25, 0.5, 0.75]);
        }
        for period in periods {
            let source = complete_source(48_000, 0);
            let source_identity = source.source_address();
            let backing = Arc::downgrade(&source.samples);
            let finite = source
                .window(START, START + length, 2, ResidentContext::FiniteLoop)
                .unwrap();
            assert!(finite.same_source(&source));
            assert_eq!(finite.source_sample_count(), SOURCE_FRAMES * CHANNELS);
            assert_eq!(finite.samples.len(), length * CHANNELS);
            assert_eq!(finite.source_address(), source_identity);
            assert!(!Arc::ptr_eq(&finite.samples, &source.samples));
            drop(source);
            assert!(
                backing.upgrade().is_none(),
                "finite descriptor retained complete PCM"
            );
            let mut observed_knots = BTreeSet::new();
            let mut distances: Vec<f64> = (0..2048).map(|n| n as f64 * period / 2048.0).collect();
            for integer in 0..=period.ceil() as usize {
                distances.extend([
                    integer as f64,
                    integer as f64 + 0.25,
                    integer as f64 + 0.875,
                ]);
            }
            distances.extend([period, period * 17.0 + 0.125]);
            for distance in distances {
                let (left, right, _) = taps(START, length, period, distance);
                assert!((START..START + length).contains(&left));
                assert!((START..START + length).contains(&right));
                observed_knots.extend([left, right]);
                let (reader, fraction) = plan(length, period, distance);
                for channel in 0..CHANNELS {
                    let actual = reader.sample_fractional(&finite, None, fraction, 0.0, channel);
                    let reference = expected(START, length, period, distance, channel, 0);
                    assert!(
                        (actual - reference).abs() < 2e-6,
                        "H={length} P={period} distance={distance} channel={channel}: {actual} != {reference}"
                    );
                }
            }
            let last = if period == length as f64 {
                length - 1
            } else {
                ((period.ceil() as usize) - 1).min(length - 1)
            };
            assert_eq!(observed_knots, (START..=START + last).collect());
        }
    }
}

/// Changes step immediately and again after 512 active output frames. The
/// reference accumulates source distance per output sample, independently of
/// callback partitions and the productive clock's epoch/rebase representation.
fn distances(initial: f64, changes: &[(usize, f64)], frames: usize) -> Vec<f64> {
    let mut rate = initial;
    let mut target = initial;
    let mut next_step = 0;
    let mut distance = 0.0;
    let mut result = Vec::with_capacity(frames + 1);
    for frame in 0..frames {
        if let Some((_, value)) = changes.iter().find(|(at, _)| *at == frame) {
            target = *value;
            next_step = frame;
        }
        if frame == next_step {
            rate += (target - rate).clamp(-0.05, 0.05);
            next_step = frame + 512;
        }
        result.push(distance);
        distance += rate;
    }
    result.push(distance);
    result
}

fn dry_mixer(sample: SampleBuffer, rate: u32, period: f64, initial: f64) -> RtMixer {
    let mut mixer = RtMixer::new(CHANNELS, rate as f32);
    mixer.load_sample(0, sample);
    mixer.set_pad_loop_region(
        0,
        START as f64 / f64::from(rate),
        Some((START + LENGTH) as f64 / f64::from(rate)),
    );
    mixer.pad_accepted_timing[0] = Some(AcceptedTimingProjection {
        revision: [37; 32],
        period_seconds: period / f64::from(rate),
        origin_seconds: START as f64 / f64::from(rate),
        sample_rate_hz: rate,
        publication_epoch: 1,
    });
    mixer.current_timing_acknowledgements.acknowledge(0, 1);
    mixer.set_key_lock(false);
    mixer.set_bpm_lock(false);
    mixer.set_speed(initial);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn rendered(
    mixer: &mut RtMixer,
    partitions: &[usize],
    changes: &[(usize, f64)],
    frames: usize,
) -> Vec<f32> {
    let mut result = Vec::with_capacity(frames * CHANNELS);
    let mut output_frame = 0;
    let mut index = 0;
    let mut peaks = [0.0; NUM_SAMPLES];
    while output_frame < frames {
        if let Some((_, value)) = changes.iter().find(|(at, _)| *at == output_frame) {
            mixer.set_speed(*value);
        }
        let next_change = changes
            .iter()
            .map(|(at, _)| *at)
            .find(|at| *at > output_frame)
            .unwrap_or(frames);
        let count = partitions[index % partitions.len()]
            .min(next_change - output_frame)
            .min(frames - output_frame);
        let mut output = vec![0.0; count * CHANNELS];
        mixer.render_at_output_frame(output_frame as u64, &mut output, &mut peaks);
        result.extend(output);
        output_frame += count;
        index += 1;
    }
    result
}

#[test]
fn finite_actual_mixer_fractional_period_and_rate_ramp_match_complete_oracle() {
    let changes = [(137, 1.25), (1430, 0.5), (2201, 1.999)];
    let frames = 4097;
    let source_distances = distances(0.73, &changes, frames);
    for rate in [44_100, 48_000] {
        for period in [LENGTH as f64 - 0.375, LENGTH as f64, LENGTH as f64 + 0.375] {
            let expected: Vec<f32> = source_distances[..frames]
                .iter()
                .flat_map(|distance| {
                    (0..CHANNELS)
                        .map(move |channel| expected(START, LENGTH, period, *distance, channel, 0))
                })
                .collect();
            for partitions in [&[512][..], &[1, 127, 384, 96, 257, 512, 31][..]] {
                let complete = complete_source(rate, 0);
                let backing = Arc::downgrade(&complete.samples);
                let finite = complete
                    .window(START, START + LENGTH, 2, ResidentContext::FiniteLoop)
                    .unwrap();
                let mut full_mixer = dry_mixer(complete, rate, period, 0.73);
                let full_output = rendered(&mut full_mixer, partitions, &changes, frames);
                drop(full_mixer);
                assert!(backing.upgrade().is_none());
                let mut finite_mixer = dry_mixer(finite, rate, period, 0.73);
                let finite_output = rendered(&mut finite_mixer, partitions, &changes, frames);
                assert_eq!(
                    full_output, finite_output,
                    "rate={rate} P={period} partitions={partitions:?}"
                );
                for (index, (actual, reference)) in finite_output.iter().zip(&expected).enumerate()
                {
                    assert!(
                        (actual - reference).abs() < 2e-6,
                        "sample={index} rate={rate} P={period}: {actual} != {reference}"
                    );
                }
                let voice = finite_mixer
                    .voices
                    .iter()
                    .find(|voice| voice.active)
                    .unwrap();
                let position = voice.source_playback.position();
                let phase = source_distances[frames].rem_euclid(period);
                assert_eq!(position.frame, START + phase.floor() as usize);
                assert!((position.fraction - phase.fract()).abs() < 3e-8);
                assert_eq!(position.seek_mode, ExplicitSeekMode::Normal);
                assert!((voice.source_playback.loop_period().unwrap() - period).abs() < 1e-10);
                assert!(backing.upgrade().is_none());
            }
        }
    }
}

#[test]
fn finite_stem_transition_uses_the_same_knots_and_releases_complete_backings() {
    let source = complete_source(48_000, 0);
    let stems = PreparedStemSet {
        complete_set_identity: Arc::new([19; 32]),
        accepted_timing: None,
        reference_samples: source.samples.clone(),
        publication: super::super::prepared_source::PreparedSourcePermit::unrestricted(),
        source_version_hash: 42,
        sample_rate_hz: 48_000,
        channels: CHANNELS,
        frame_count: SOURCE_FRAMES,
        available_mask: full_stem_available_mask(),
        stems: std::array::from_fn(|index| {
            let mut stem = complete_source(48_000, index + 1);
            stem.residency = source.residency.clone();
            stem
        }),
    };
    let complete_backings: Vec<_> = std::iter::once(Arc::downgrade(&source.samples))
        .chain(stems.stems.iter().map(|stem| Arc::downgrade(&stem.samples)))
        .collect();
    let finite = source
        .window(START, START + LENGTH, 2, ResidentContext::FiniteLoop)
        .unwrap();
    let finite_stems = stems.window_for(&finite).unwrap();
    drop(source);
    assert!(
        complete_backings
            .iter()
            .all(|backing| backing.upgrade().is_none())
    );
    assert_eq!(finite_stems.stems.len(), STEM_BUFFER_COUNT);
    for period in [LENGTH as f64 - 0.375, LENGTH as f64 + 0.375] {
        for distance in [0.0, 1.25, 135.875, 136.25, 136.875, 137.125, 5001.75] {
            for progress in [0.0_f64, 0.375, 64.25, 127.5, 128.0] {
                let (mut reader, fraction) = plan(LENGTH, period, distance);
                reader.selection =
                    StemRenderSelection::from_state(StemMixMode::AllStems, 42, STEM_COMPONENT_MASK);
                reader.transition = StemTransition::start(StemRenderSelection::full_mix(), 128);
                let gain = progress.min(128.0) as f32 / 128.0;
                for channel in 0..CHANNELS {
                    let from = expected(START, LENGTH, period, distance, channel, 0);
                    // Four live components; the fifth instrumental cache entry
                    // would double count and is deliberately absent from this sum.
                    let to: f32 = (1..=4)
                        .map(|tag| expected(START, LENGTH, period, distance, channel, tag))
                        .sum();
                    let reference = from * (1.0 - gain) + to * gain;
                    let actual = reader.sample_fractional(
                        &finite,
                        Some(&finite_stems),
                        fraction,
                        progress,
                        channel,
                    );
                    assert!((actual - reference).abs() < 2e-6);
                }
            }
        }
    }
    assert!(
        complete_backings
            .iter()
            .all(|backing| backing.upgrade().is_none())
    );
}

#[test]
fn finite_normal_tap_context_covers_key_lock_geometry_and_guards_intro_tail() {
    let complete = complete_source(48_000, 0);
    let finite = complete
        .window(START, START + LENGTH, 2, ResidentContext::FiniteLoop)
        .unwrap();
    let region = FrameRange {
        start: START,
        end: START + LENGTH,
    };
    assert!(resident_read_context_available(
        &finite,
        region,
        ExplicitSeekMode::Normal,
        false
    ));
    for mode in [
        ExplicitSeekMode::Normal,
        ExplicitSeekMode::BeforeLoop,
        ExplicitSeekMode::AfterLoop,
    ] {
        assert!(resident_read_context_available(
            &complete, region, mode, true
        ));
        assert_eq!(
            resident_read_context_available(&finite, region, mode, true),
            mode == ExplicitSeekMode::Normal
        );
        assert_eq!(
            resident_read_context_available(&finite, region, mode, false),
            mode == ExplicitSeekMode::Normal
        );
    }
    for outside in [
        FrameRange {
            start: START - 1,
            end: START + LENGTH,
        },
        FrameRange {
            start: START,
            end: START + LENGTH + 1,
        },
    ] {
        assert!(!resident_read_context_available(
            &finite,
            outside,
            ExplicitSeekMode::Normal,
            false
        ));
        assert!(!resident_read_context_available(
            &finite,
            outside,
            ExplicitSeekMode::Normal,
            true
        ));
    }
}
