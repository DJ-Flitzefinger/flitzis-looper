//! Python source seconds -> loaded frames -> real source clock -> Native pitch.
//!
//! Nonzero pitch is admitted first through the public captured-action worker. Accepted timing
//! is then published through the real callback queue, and the ordinary source-history worker
//! must prepare/adopt that acknowledged projection. Expanded profiles may first need actual
//! post-publication history; bounded virtual playback and host waits prove readiness only.
//! These scalar/software checks do not establish musical quality or a device deadline.

use super::*;
use crate::audio_engine::constant_timing::{AcceptedTimingProjection, PreparedConstantTiming};
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::pitch_actions::PitchKeyMode;
use crate::audio_engine::pitch_runtime_tests::Fixture;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::scalar_grid::ScalarSourceGrid;
use crate::messages::ControlMessage;
use pyo3::prelude::*;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const PROBE_FRAMES: usize = 2_077;
const LEGACY_PERIOD: f64 = 0.625; // 96 BPM, deliberately different from every accepted period.
const ORIGIN: Fraction = Fraction(-1, 8);

/// Independent physical clock oracle: retain integer ratios until the final comparison.
/// No production grid, tempo resolver, SourcePlayback or pitch conversion supplies its values.
#[derive(Clone, Copy)]
struct Fraction(i128, i128);

impl Fraction {
    fn new(numerator: i128, denominator: i128) -> Self {
        assert!(denominator > 0);
        let (mut a, mut b) = (numerator.abs(), denominator);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Self(numerator / a, denominator / a)
    }

    fn add(self, rhs: Self) -> Self {
        Self::new(self.0 * rhs.1 + rhs.0 * self.1, self.1 * rhs.1)
    }

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.0 * rhs.1 - rhs.0 * self.1, self.1 * rhs.1)
    }

    fn mul(self, rhs: Self) -> Self {
        Self::new(self.0 * rhs.0, self.1 * rhs.1)
    }

    fn div(self, rhs: Self) -> Self {
        assert!(rhs.0 > 0);
        Self::new(self.0 * rhs.1, self.1 * rhs.0)
    }

    fn value(self) -> f64 {
        self.0 as f64 / self.1 as f64
    }
}

#[derive(Clone, Copy)]
struct ClockCase {
    name: &'static str,
    source_period: Fraction,
    master_period: Fraction,
}

const CLOCKS: [ClockCase; 4] = [
    ClockCase {
        name: "half",
        source_period: Fraction(1, 2),
        master_period: Fraction(1, 1),
    },
    ClockCase {
        name: "unity",
        source_period: Fraction(1, 2),
        master_period: Fraction(1, 2),
    },
    ClockCase {
        name: "double",
        source_period: Fraction(1, 1),
        master_period: Fraction(1, 2),
    },
    ClockCase {
        name: "fractional",
        source_period: Fraction(60_000, 119_999), // 60 / 119.999 BPM.
        master_period: Fraction(60_000_000_000, 137_123_456_789),
    },
];

fn inverse_source_seconds(period: Fraction, beats: &[Fraction]) -> Vec<f64> {
    // Exercise the actual Python constructor/method boundary. The GIL is released before any
    // publication, rendering or Native worker wait; this is never called from realtime work.
    Python::attach(|py| {
        let grid = py
            .get_type::<ScalarSourceGrid>()
            .call1((ORIGIN.value(), period.value()))
            .unwrap();
        beats
            .iter()
            .map(|beat| {
                grid.call_method1("source_at_beat", (beat.value(),))
                    .unwrap()
                    .extract::<Option<f64>>()
                    .unwrap()
                    .unwrap()
            })
            .collect()
    })
}

fn physical_source_seconds(case: ClockCase, rate: u32, active_frames: u64) -> Fraction {
    // The fixture's intentional source birth is exactly Fs/10, and no tested interval wraps.
    Fraction(1, 10).add(
        Fraction::new(i128::from(active_frames), i128::from(rate))
            .div(case.master_period)
            .mul(case.source_period),
    )
}

fn accepted_source_ready(
    fixture: &mut Fixture,
    projection: AcceptedTimingProjection,
) -> (u64, usize, u64) {
    let initial_frame = fixture.transport.output_frame();
    let mut request_frame = initial_frame - 1; // The caller just rendered the first request frame.
    let pitch = fixture.voice(0).stretch.native.pitch_scale;
    let history_frames = fixture
        .voice(0)
        .stretch
        .native
        .rubberband
        .as_ref()
        .unwrap()
        .source_history_frames(pitch)
        .unwrap();
    let frame_limit = initial_frame + history_frames as u64 + 8_192;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let voice = fixture
            .mixer
            .voices
            .iter_mut()
            .find(|voice| voice.active && voice.sample_id == 0)
            .unwrap();
        assert!(!voice.stretch.wet_mode_failed());
        if let Some(state) = voice.stretch.preparation.prepared_state() {
            let source = state.source.as_ref().unwrap();
            assert_eq!(source.binding.accepted, Some(projection));
            assert!(
                source
                    .permit
                    .current(&source.sample, fixture.sample_rate_hz)
            );
            assert!(source.permit.matches_projection(Some(projection)));
            assert!(state.active && state.used && !state.dirty);
            let result = (
                source.target_output_frame,
                state.rubberband.as_ref().unwrap().state_address(),
                fixture.transport.output_frame() - initial_frame,
            );
            assert!(source.target_output_frame >= fixture.transport.output_frame());
            assert_eq!(source.target_output_frame, request_frame + 4_096);
            return result;
        }
        // A refused expanded-profile request is terminal, not a live wait. Advance only real
        // post-publication source history before the callback may make its next request. The
        // map, original pitch action and F+4096 preparation policy remain unchanged.
        let finished = voice
            .stretch
            .preparation
            .source_work_finished(voice.stretch.request_id);
        // Completion's acquire observes a ready publication that raced with the first peek.
        if finished && voice.stretch.preparation.prepared_state().is_some() {
            continue;
        }
        assert!(
            Instant::now() < deadline,
            "accepted-source host readiness wait exceeded 3 s"
        );
        if finished {
            assert!(
                fixture.transport.output_frame() + 512 <= frame_limit,
                "accepted-source bounded history exhausted: Fs={} history={history_frames}",
                fixture.sample_rate_hz
            );
            request_frame = fixture.transport.output_frame();
            fixture.render(512);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn python_inverse_seconds_reach_actual_accepted_source_clock_and_native_pitch() {
    Python::initialize();
    for rate in [44_100, 48_000, 96_000] {
        for case in CLOCKS {
            let mut off_cursors: Option<(f64, f64)> = None;
            for key_lock in [false, true] {
                let mut fixture = Fixture::new_format(key_lock, rate, 1);
                let expected_rate = case.source_period.div(case.master_period).value();
                // Initial pitch admission uses the actual legacy BPM resolver, not a directly
                // injected SourcePlayback rate. The accepted projection replaces this period.
                fixture.mixer.set_pad_bpm(0, Some(96.0));
                fixture.mixer.set_bpm_lock(true);
                let initial_master = LEGACY_PERIOD / expected_rate;
                fixture.mixer.set_master_period(initial_master);
                assert!(fixture.transport.set_master_period(initial_master));
                fixture.rows(PitchPadProjection {
                    key_lock,
                    source_root: Some(0),
                    source_mode: Some(PitchKeyMode::Major),
                    sync: false,
                    ..PitchPadProjection::default()
                });
                let action = fixture.capture(0, "pitch.extra:7:set_retrigger");
                assert!(!action.sync());
                assert_eq!(action.render().pitch, PitchIntent::new(0, 7).unwrap());
                fixture.publish(&action);
                fixture.wait_worker(&[&action]);
                fixture.render_to(action.execution_target());
                fixture.render(1);
                assert!(
                    action.effective(),
                    "{} {:?}",
                    action.state(),
                    action.reason()
                );
                let birth = action.execution_target();
                let voice_generation = fixture.voice(0).generation;
                let starts = fixture.starts(0);
                let source = fixture.voice(0).sample.as_ref().unwrap().clone();
                assert!(fixture.voice(0).source_timing.accepted.is_none());
                assert_eq!(
                    fixture.voice(0).source_timing.legacy_period_seconds,
                    Some(LEGACY_PERIOD)
                );
                assert!(
                    (fixture.voice(0).source_playback.tempo_ratio() - expected_rate).abs()
                        < 1.0e-14
                );

                let engine = &fixture.control.engine;
                let ownership = &engine.input_runtime_ownership;
                assert!(ownership.is_source_tracked());
                assert!(ownership.source_current(0, &source, rate));
                let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
                assert_eq!(action.content_key(), key);
                assert_eq!(
                    engine
                        .current_timing_acknowledgements
                        .current_for_key(ownership, key),
                    0
                );
                let epoch = engine.prepared_source_epoch(0).unwrap().clone();
                let publication_epoch = epoch.load(Ordering::Acquire);
                assert_ne!(publication_epoch, 0);
                let permit = PreparedSourcePermit::for_epoch(epoch, publication_epoch);
                permit.mark_pending().unwrap();
                let projection = AcceptedTimingProjection {
                    revision: [0x73; 32],
                    period_seconds: case.source_period.value(),
                    origin_seconds: ORIGIN.value(),
                    sample_rate_hz: rate,
                    publication_epoch,
                };
                assert_ne!(projection.period_seconds, LEGACY_PERIOD);
                let before_publication = fixture.voice(0).source_playback.position();
                // Change both resolver inputs before the next render. Their physical ratio
                // stays constant, so this proof needs no rate-smoothing oracle.
                fixture.mixer.set_master_period(case.master_period.value());
                assert!(
                    fixture
                        .transport
                        .set_master_period(case.master_period.value())
                );
                let voice_index = fixture
                    .mixer
                    .voices
                    .iter()
                    .position(|voice| voice.active && voice.sample_id == 0)
                    .unwrap();
                let legacy_target = fixture
                    .mixer
                    .capture_pitch_voice_snapshot(voice_index)
                    .unwrap()
                    .playback
                    .rate_target();
                assert_eq!(legacy_target, LEGACY_PERIOD / case.master_period.value());
                assert!((legacy_target - expected_rate).abs() > 0.1);
                fixture
                    .control
                    .producer
                    .lock()
                    .unwrap()
                    .push(ControlMessage::PublishConstantTiming {
                        id: 0,
                        timing: PreparedConstantTiming {
                            content_key: Some(key),
                            reference: source.clone(),
                            publication: permit.clone(),
                            projection,
                        },
                    })
                    .unwrap();
                assert_eq!(permit.status(), "pending");
                fixture.drain();
                assert_eq!(permit.status(), "accepted");
                let accepted_target = fixture
                    .mixer
                    .capture_pitch_voice_snapshot(voice_index)
                    .unwrap()
                    .playback
                    .rate_target();
                assert!((accepted_target - expected_rate).abs() < 1.0e-14);
                assert_eq!(
                    fixture.voice(0).source_playback.position(),
                    before_publication
                );
                assert_eq!(
                    fixture
                        .control
                        .engine
                        .current_timing_acknowledgements
                        .current_for_key(&fixture.control.engine.input_runtime_ownership, key,),
                    publication_epoch
                );

                let request_frame = fixture.transport.output_frame();
                fixture.render(1); // Actual callback captures the accepted source-history request.
                let (adoption, prepared_native, additional_history) =
                    accepted_source_ready(&mut fixture, projection);
                assert!(adoption >= request_frame + 4_096);
                fixture.render_to(adoption);
                fixture.render(1);
                assert_eq!(
                    fixture.voice(0).stretch.native_state_address(),
                    prepared_native
                );
                // Compare both modes at the same declared physical output time, independent
                // of worker speed and extra post-publication history needed by a wider profile.
                // Even r=2 remains inside the selected two-second source region through probe.
                let probe_frame = birth + u64::from(rate) * 3 / 4;
                assert!(fixture.transport.output_frame() <= probe_frame);
                fixture.render_to(probe_frame);
                let before_frame = fixture.transport.output_frame();
                let before = fixture.voice(0).source_playback.position();
                let before_cursor = before.frame as f64 + before.fraction;
                let history_before = fixture.voice(0).stretch.productive_history().unwrap();
                let pcm = fixture.pcm(PROBE_FRAMES);
                assert!(pcm.iter().all(|sample| sample.is_finite()));
                assert!(pcm.iter().any(|sample| sample.abs() > 1.0e-7));
                let voice = fixture.voice(0);
                let after = voice.source_playback.position();
                let after_cursor = after.frame as f64 + after.fraction;
                let active_before = before_frame - birth;
                let active_after = fixture.transport.output_frame() - birth;
                assert_eq!(active_after - active_before, PROBE_FRAMES as u64);
                let physical = [
                    physical_source_seconds(case, rate, active_before),
                    physical_source_seconds(case, rate, active_after),
                ];
                let beats = physical.map(|seconds| seconds.sub(ORIGIN).div(case.source_period));
                let inverse = inverse_source_seconds(case.source_period, &beats);
                let voice_grid = voice.source_timing.grid(f64::from(rate)).unwrap();
                for index in 0..2 {
                    // Absolute anchors catch an origin/unit error that a delta could conceal.
                    assert!((inverse[index] - physical[index].value()).abs() < 1.0e-12);
                    let cursor = [before_cursor, after_cursor][index];
                    let loaded_frame = inverse[index] * f64::from(rate); // Exactly one conversion.
                    assert!((loaded_frame - cursor).abs() < 2.0e-8);
                    let actual_grid_frame =
                        voice_grid.source_at_beat(beats[index].value()).unwrap();
                    assert!((actual_grid_frame - loaded_frame).abs() < 2.0e-8);
                    assert!(loaded_frame >= f64::from(rate) / 10.0);
                    assert!(loaded_frame < 2.1 * f64::from(rate)); // No wrap/reduction in this proof.
                    assert!((inverse[index] - cursor).abs() > 1.0); // Omitted seconds -> frames.
                    assert!((loaded_frame * f64::from(rate) - cursor).abs() > 1.0); // Double Fs.
                }
                let inverse_rate =
                    (inverse[1] - inverse[0]) * f64::from(rate) / PROBE_FRAMES as f64;
                let actual_rate = voice.source_playback.tempo_ratio();
                assert!((inverse_rate - expected_rate).abs() < 1.0e-11);
                assert!((actual_rate - expected_rate).abs() < 1.0e-14);
                assert_eq!(actual_rate, voice.source_playback.rate_target());
                assert!(
                    (after_cursor - before_cursor - PROBE_FRAMES as f64 * expected_rate).abs()
                        < 2.0e-8
                );
                assert_eq!(voice.source_timing.accepted, Some(projection));
                assert_eq!(
                    voice.source_timing.period_seconds(),
                    Some(projection.period_seconds)
                );
                let history = voice.stretch.productive_history().unwrap();
                assert_eq!(history.binding.accepted, Some(projection));
                assert_eq!(history.binding.sample_rate_hz, rate);
                assert_eq!(history.binding.source_address, source.source_address());
                assert_eq!(
                    history.fed_output_frames - history_before.fed_output_frames,
                    PROBE_FRAMES as u64
                );
                assert_eq!(voice.stretch.native_state_address(), prepared_native);
                let native_source = voice.stretch.native.source.as_ref().unwrap();
                assert_eq!(native_source.binding.accepted, Some(projection));
                assert!(native_source.permit.current(&source, rate));
                assert!(native_source.permit.matches_projection(Some(projection)));
                assert!(
                    voice
                        .stretch
                        .owns_pitch_contract(action.render().pitch, key_lock)
                );
                let h = 2.0_f64.powf(7.0 / 12.0);
                let resolved_pitch = if key_lock { h / actual_rate } else { h };
                let inverse_pitch = if key_lock { h / inverse_rate } else { h };
                assert_eq!(voice.stretch.native.pitch_scale, resolved_pitch);
                let dll_pitch = voice
                    .stretch
                    .native
                    .rubberband
                    .as_ref()
                    .unwrap()
                    .pitch_scale();
                assert_eq!(dll_pitch, resolved_pitch);
                assert!((dll_pitch - inverse_pitch).abs() < 1.0e-10);
                assert_ne!(dll_pitch, f64::from(dll_pitch as f32));
                let current_beat = fixture.mixer.active_pad_beat_position(0).unwrap();
                assert!((current_beat - beats[1].value()).abs() < 1.0e-11);
                // The retained legacy period and zero origin would give a different absolute
                // grid. Merely keeping the initially correct rate cannot satisfy this anchor.
                let old_legacy_beat = after_cursor / (LEGACY_PERIOD * f64::from(rate));
                assert!((current_beat - old_legacy_beat).abs() > 1.0e-3);
                assert!(action.effective());
                assert_eq!(voice.generation, voice_generation);
                assert_eq!(fixture.starts(0), starts);
                if let Some((off_before, off_after)) = off_cursors {
                    assert!((before_cursor - off_before).abs() < 2.0e-8);
                    assert!((after_cursor - off_after).abs() < 2.0e-8);
                } else {
                    off_cursors = Some((before_cursor, after_cursor));
                }
                eprintln!(
                    "coordinate_chain Fs={rate} clock={} lock={key_lock} r_map={inverse_rate:.15} r_live={actual_rate:.15} native={dll_pitch:.15} additional_history={additional_history} accepted_source=true nonwrap=true",
                    case.name,
                );
            }
        }
    }
}

#[test]
fn unequal_loaded_and_output_rate_formula_is_arithmetic_only() {
    Python::initialize();
    // All nine rate pairs are dimensional arithmetic. The production loader resamples into
    // FsOutput; unequal FsLoaded/FsOutput here does not claim a supported mixer/source format.
    let source_period = Fraction(3, 5);
    let master_period = Fraction(7, 10);
    for loaded_rate in [44_100, 48_000, 96_000] {
        for output_rate in [44_100, 48_000, 96_000] {
            let seconds_before = Fraction(1, 10);
            let output_frames = 4_093;
            let elapsed = Fraction::new(output_frames, i128::from(output_rate));
            let seconds_after = seconds_before.add(elapsed.div(master_period).mul(source_period));
            let beats = [seconds_before, seconds_after]
                .map(|seconds| seconds.sub(ORIGIN).div(source_period));
            let inverse = inverse_source_seconds(source_period, &beats);
            assert!((inverse[0] - seconds_before.value()).abs() < 1.0e-12);
            assert!((inverse[1] - seconds_after.value()).abs() < 1.0e-12);
            let expected = source_period
                .div(master_period)
                .mul(Fraction::new(
                    i128::from(loaded_rate),
                    i128::from(output_rate),
                ))
                .value();
            let observed =
                (inverse[1] - inverse[0]) * f64::from(loaded_rate) / output_frames as f64;
            assert!((observed - expected).abs() < 1.0e-11);
            assert!((observed / f64::from(loaded_rate) - expected).abs() > 0.1);
            assert!((observed * f64::from(loaded_rate) - expected).abs() > 1.0);
            if loaded_rate != output_rate {
                let wrong_output_units =
                    (inverse[1] - inverse[0]) * f64::from(output_rate) / output_frames as f64;
                assert!((wrong_output_units - expected).abs() > 1.0e-3);
            }
        }
    }
}
