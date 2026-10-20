//! Stable selected complete readers compose with the real piano/MIDI rate route.
//!
//! Initial declared stem PCM publication uses the existing source-bound fixture seam.
//! Selection commands, source clocks, input captures, work and effects are genuine.
//! Independent ordered component sums supply every reference tap; neither a DUT
//! source plan nor its scalar journal supplies the PCM/rate/reset oracle.

use super::*;
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::source_reader::StemRenderSelection;
use crate::messages::{ControlMessage, PreparedStemSet, STEM_BUFFER_COUNT, StemMixMode};

const SELECTED_CHILD: &str = "FLITZI_SELECTED_PIANO_RATE_GLIDE_CHILD";
const SELECTED_TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::piano_rate_controller_tests::piano_rate_glide_tests::selected_stem_rate_glide_tests::selected_stem_piano_retrigger_keeps_original_glide";
const SELECTED_HASH: u64 = 0x71_65_79_67;
pub(super) const SELECTED_MASK: u8 = 0b0101;

fn declared_component(component: usize, extent: usize) -> SampleBuffer {
    SampleBuffer {
        samples: (0..extent)
            .map(|frame| {
                let time = frame as f64 / f64::from(RATE);
                (0.073
                    * (std::f64::consts::TAU
                        * ((197.0 + 83.0 * component as f64) * time
                            + (1.7 + component as f64) * time * time)
                        + 0.13 * component as f64)
                        .sin()
                    + 0.019
                        * (std::f64::consts::TAU * (571.0 + 107.0 * component as f64) * time
                            + 0.41)
                            .cos()) as f32
            })
            .collect::<Vec<_>>()
            .into(),
        channels: 1,
        residency: None,
    }
}

fn selected_pcm_source(stems: &PreparedStemSet, mask: u8) -> SampleBuffer {
    // Mix the integer taps first, in explicit ascending component order. The
    // parent's scalar_pcm interpolates these independently copied f32 taps.
    SampleBuffer {
        samples: (0..stems.frame_count)
            .map(|frame| {
                let mut mixed = 0.0_f32;
                for (component, stem) in stems.stems.iter().enumerate() {
                    if mask & (1 << component) != 0 {
                        mixed += stem.samples[frame];
                    }
                }
                mixed
            })
            .collect::<Vec<_>>()
            .into(),
        channels: 1,
        residency: None,
    }
}

pub(super) fn publish_selected(route: &mut RateRoute, source: &SampleBuffer) -> PreparedStemSet {
    let (publication, accepted) = Python::attach(|py| {
        let engine = route.route.engine.borrow(py);
        let captured = engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap();
        assert!(captured.current());
        // The public control resolver deliberately returns runtime_revision=0.
        // The existing Python configure route has already published the Native
        // input row. Retain its actual revision, exactly as selection_setup does;
        // this does not change or acknowledge source/window/timing authority.
        let binding = InputPadBinding {
            runtime_revision: engine.input_runtime_ownership.runtime_revision(0),
            ..captured.binding
        };
        assert!(engine.input_runtime_ownership.authority_current(0, binding));
        assert!(
            engine
                .input_runtime_ownership
                .binding_window_current(0, binding)
        );
        assert!(engine.input_runtime_ownership.current(0, binding));
        assert_eq!(binding.source_address, source.source_address());
        assert_eq!(binding.sample_count, source.source_sample_count());
        assert_eq!(binding.channels, source.channels);
        assert_eq!(
            engine.current_timing_acknowledgements.current_for_binding(
                &engine.input_runtime_ownership,
                0,
                binding,
            ),
            binding
                .accepted
                .map_or(0, |accepted| accepted.publication_epoch)
        );
        let permit = PreparedSourcePermit::for_native_binding_for_test(
            0,
            binding,
            engine.input_runtime_ownership.clone(),
            engine.current_timing_acknowledgements.clone(),
            engine.prepared_source_epoch(0).unwrap().clone(),
        )
        .unwrap();
        (permit, binding.accepted)
    });
    let complete_stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x71; 32]),
        accepted_timing: accepted,
        reference_samples: source.samples.clone(),
        reference_residency: source.residency.clone(),
        publication,
        source_version_hash: SELECTED_HASH,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: source.frame_count(),
        available_mask: (1 << STEM_BUFFER_COUNT) - 1,
        stems: std::array::from_fn(|component| declared_component(component, source.frame_count())),
    };
    // Preserve an independently mixed complete source before the productive
    // descriptor crops its component views to the actually acknowledged window.
    let selected = selected_pcm_source(&complete_stems, SELECTED_MASK);
    let difference: f64 = route.reference_sources[0]
        .samples
        .iter()
        .zip(selected.samples.iter())
        .map(|(full, selected)| f64::from(full - selected).powi(2))
        .sum();
    assert!(difference > 100.0, "selected PCM must distinguish FullMix");
    assert_eq!(route.sources[0].source_address(), source.source_address());
    route.reference_sources[0] = selected;
    let stems = complete_stems.window_for(source).unwrap();
    assert!(route.route.mixer.publish_prepared_stems(0, stems.clone()));
    assert_eq!(stems.publication.status(), "accepted");

    // The queue and normal production handlers apply selection while stopped,
    // so the entire actual lifecycle has this same stable selected reader.
    Python::attach(|py| {
        let engine = route.route.engine.borrow(py);
        let mut producer = engine
            .test_control_channels
            .as_ref()
            .unwrap()
            .0
            .lock()
            .unwrap();
        producer
            .push(ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: SELECTED_MASK,
                source_version_hash: SELECTED_HASH,
            })
            .unwrap();
        producer
            .push(ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::AllStems,
                source_version_hash: SELECTED_HASH,
            })
            .unwrap();
    });
    route.callback(0);
    route.assert_no_attacks();
    eprintln!(
        "selected_stem_rate_glide_source {}",
        serde_json::json!({
            "mask":SELECTED_MASK,"components":STEM_BUFFER_COUNT,
            "selected_vs_FullMix_squared_difference":difference,
            "physical_source":route.sources[0].source_address(),
            "peer_source":route.sources[1].source_address(),
            "set_owner":Arc::as_ptr(&stems.complete_set_identity) as usize,
            "publication_owner":Arc::as_ptr(&stems.publication.epoch) as usize,
            "publication_generation":stems.publication.expected,
            "source_window_start":source.resident_start(),"source_window_end":source.resident_end(),
            "scope":"declared component PCM, source-bound initial publication seam, genuine queued stable selection before first birth; independently mixed complete reference before component window cropping"
        })
    );
    stems
}

fn selected_start(lock: bool) -> (RateRoute, [Original; 2], PreparedStemSet) {
    let mut route = RateRoute::new(lock);
    let source = route.sources[0].clone();
    let stems = publish_selected(&mut route, &source);
    public_speed(&route, INITIAL_SPEED);
    route.callback(0);
    route.assert_no_attacks();
    let rate = route.rate();
    assert_eq!(rate.state, "applied");
    assert_eq!(rate.effective_speed, INITIAL_SPEED);
    assert_eq!(rate.effective_period, Some(PERIODS[0] / INITIAL_SPEED));
    route.observe_rate();
    let originals = [
        initial_birth(&mut route, 0, INITIAL_SPEED),
        initial_birth(&mut route, 1, INITIAL_SPEED),
    ];
    route.wait_claims();
    route.callback(733);
    route.assert_no_attacks();
    ensure_positive_pre_t_history(&mut route, &originals[0], INITIAL_SPEED, &[7, 12]);
    assert_selected(&route, &stems);
    (route, originals, stems)
}

pub(super) fn assert_selected(route: &RateRoute, stems: &PreparedStemSet) {
    let index = route
        .route
        .mixer
        .voices
        .iter()
        .position(|voice| voice.active && voice.sample_id == 0)
        .unwrap();
    let snapshot = route
        .route
        .mixer
        .capture_pitch_voice_snapshot(index)
        .unwrap();
    assert_eq!(
        snapshot.plan.selection,
        StemRenderSelection::for_stems(stems, SELECTED_MASK)
    );
    assert!(!snapshot.plan.transition.is_active());
    let held = snapshot.stems.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &held.complete_set_identity,
        &stems.complete_set_identity
    ));
    assert!(Arc::ptr_eq(
        &held.reference_samples,
        &stems.reference_samples
    ));
    assert!(Arc::ptr_eq(
        &held.publication.epoch,
        &stems.publication.epoch
    ));
    assert_eq!(held.publication.expected, stems.publication.expected);
    assert_eq!(held.accepted_timing, stems.accepted_timing);
    for (actual, original) in held.stems.iter().zip(&stems.stems) {
        assert!(Arc::ptr_eq(&actual.samples, &original.samples));
    }
    assert_eq!(
        snapshot.sample.source_address(),
        route.sources[0].source_address()
    );
    assert_ne!(route.voice(0).stretch.native_state_address(), 0);
    assert_ne!(route.voice(1).stretch.native_state_address(), 0);
    assert_ne!(
        route.voice(0).stretch.native_state_address(),
        route.voice(1).stretch.native_state_address()
    );
}

fn selected_wet_attack(lock: bool, midi: bool) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals, stems) = selected_start(lock);
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    assert_selected(&route, &stems);
    let case = format!(
        "selected-{}-{}-stable-glide",
        if midi { "midi" } else { "gui" },
        if lock { "on" } else { "off" }
    );
    apply_wet_attack(
        &mut route,
        &originals,
        &accepted,
        INITIAL_SPEED,
        7,
        (&case, midi),
        None,
    );
    assert_selected(&route, &stems);
    assert!(route.compared > 25_000);
    assert_eq!(route.starts, [2, 1]);
    assert!(route.maximum < 2e-5);
    eprintln!(
        "selected_stem_rate_glide_success {case} complete exact held readers, independent mixed PCM and unchanged peer; no acoustic/finite/continuous-O/deadline/resource/Human acceptance"
    );
    route.route.close();
}

fn selected_young_history_refusal_then_fresh_retry() {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, [old_birth, peer_birth], stems) = selected_start(true);
    // A genuine new attack starts a new active journal lifetime. No fabricated
    // gap, edited journal or injected source-clock state creates this shortage.
    let young = initial_birth_note(&mut route, 0, INITIAL_SPEED, -4);
    let accepted = begin_glide(&mut route);
    let rate_f = accepted.execution.unwrap();
    assert_eq!(rate_f, young.e + 1);
    route.callback(73);
    route.assert_no_attacks();
    assert_selected(&route, &stems);
    let old = VoiceWitness::capture(&route, 0);
    let peer = VoiceWitness::capture(&route, 1);
    let starts = route.starts;
    let refused = route.admit(0, 12, false);
    assert_eq!(refused.t, rate_f + 73);
    assert_eq!(refused.e, refused.t + 8192);
    let timeline = original_timeline(&young, INITIAL_SPEED, rate_f, refused.e);
    let t_index = usize::try_from(refused.t - young.e).unwrap();
    assert!(reset_bound(&timeline[..=t_index], 12, true).is_none());
    wait_refusal(&mut route, &refused);
    old.unchanged(&route, 0);
    peer.unchanged(&route, 1);
    assert_eq!(route.starts, starts);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(-4, 0).unwrap())
    );
    route.render_to(refused.e);
    let at_e = VoiceWitness::capture(&route, 0);
    let peer_at_e = VoiceWitness::capture(&route, 1);
    route.callback(0);
    at_e.unchanged(&route, 0);
    peer_at_e.unchanged(&route, 1);
    assert_rate_clock(&route, &refused, timeline.last().unwrap());
    route.check_voices();
    route.assert_no_attacks();
    settle_refusal(&route, &refused);
    assert_selected(&route, &stems);
    route.wait_claims();
    assert_eq!(route.starts, starts);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_rate_receipt(&route, &accepted);
    emit_case(
        &route,
        "selected-on-genuine-young-history-refusal",
        &refused,
        &accepted,
    );

    // Retain the genuine new birth as O. The fresh routed MIDI +7 attack uses
    // this worker and this unchanged source, while the public glide still runs.
    assert!(route.voice(0).source_playback.tempo_ratio() < GLIDE_SPEED);
    let originals = [young, peer_birth];
    apply_wet_attack(
        &mut route,
        &originals,
        &accepted,
        INITIAL_SPEED,
        7,
        ("selected-on-young-history-fresh-midi-retry", true),
        None,
    );
    assert_selected(&route, &stems);
    assert_eq!(route.starts, [starts[0] + 1, starts[1]]);
    assert!(route.voice(0).stretch.native_state_address() != old.native);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    old_birth.unchanged();
    refused.unchanged();
    assert_eq!(refused.action.reason(), Some("preparation_failed"));
    assert!(!refused.action.effective());
    eprintln!(
        "selected_stem_rate_glide_success young-history-refusal-then-fresh-midi-retry available_active_frames={} independently_insufficient_H_for_plus12; original refusal retained, same worker/current pin fresh +7 succeeds",
        timeline.len() - 1
    );
    route.route.close();
}

fn selected_publication_invalidation_after_ready(lock: bool) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals, stems) = selected_start(lock);
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    let old = VoiceWitness::capture(&route, 0);
    let peer = VoiceWitness::capture(&route, 1);
    let starts = route.starts;
    let mut refused = route.admit(0, 7, true);
    assert_eq!(refused.e, refused.t + 8192);
    route.wait_action(&mut refused);
    assert!(refused.prepared_native.is_some());
    assert_ne!(refused.prepared_native.unwrap(), old.native);
    old.unchanged(&route, 0);
    peer.unchanged(&route, 1);
    assert_selected(&route, &stems);
    assert_eq!(
        stems
            .publication
            .epoch
            .load(std::sync::atomic::Ordering::Acquire),
        stems.publication.expected
    );
    // Explicit publication-invalidation seam, after genuine productive READY.
    // It does not simulate public Generate/V2, alter admitted PCM/descriptor,
    // restore an epoch, or authorize another publication through numeric equality.
    assert_eq!(
        stems
            .publication
            .epoch
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel),
        stems.publication.expected
    );
    assert!(!stems.publication.current());
    route.render_to(refused.e);
    let old_at_e = VoiceWitness::capture(&route, 0);
    let peer_at_e = VoiceWitness::capture(&route, 1);
    route.callback(0);
    old_at_e.unchanged(&route, 0);
    peer_at_e.unchanged(&route, 1);
    route.assert_no_attacks();
    assert_eq!(refused.action.state(), "rejected");
    assert!(
        refused.action.reason().is_some(),
        "publication refusal must be observable"
    );
    assert!(!refused.action.effective());
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(originals[0].note, 0).unwrap())
    );
    assert_eq!(route.starts, starts);
    assert_eq!(route.voice(0).generation, old.generation);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_selected(&route, &stems);
    Python::attach(|py| {
        let pitch = route
            .route
            .probe
            .bind(py)
            .getattr("transport")
            .unwrap()
            .getattr("pitch")
            .unwrap();
        pitch.call_method0("poll").unwrap();
        assert!(
            !pitch
                .getattr("_owners")
                .unwrap()
                .call_method1("__contains__", (refused.sequence,))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        let (status, error): (String, Option<String>) = pitch
            .call_method1("status", (0,))
            .unwrap()
            .extract()
            .unwrap();
        assert_eq!(status, "error");
        assert!(error.is_some());
        assert!(
            refused
                .py
                .getattr(py, "effective_output_frame")
                .unwrap()
                .extract::<Option<u64>>(py)
                .unwrap()
                .is_none()
        );
    });
    route.render_to(refused.e + 4097);
    route.wait_claims();
    assert_eq!(route.starts, starts);
    assert_eq!(route.voice(0).generation, old.generation);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_selected(&route, &stems);
    assert_rate_receipt(&route, &accepted);
    for original in originals.iter().chain([&refused]) {
        original.unchanged();
    }
    assert!(route.scheduler.is_empty());
    emit_case(
        &route,
        "selected-publication-invalidated-after-ready",
        &refused,
        &accepted,
    );
    eprintln!(
        "selected_stem_rate_glide_success publication-invalidation-after-ready lock={lock}; test-only atomic publication seam, not public Generate/V2; old exact selected PCM/Native/FIFO/source/gen and peer retained"
    );
    route.route.close();
}

#[test]
fn selected_stem_piano_retrigger_keeps_original_glide() {
    if let Ok(child) = std::env::var(SELECTED_CHILD) {
        match child.as_str() {
            "gui-off" => selected_wet_attack(false, false),
            "gui-on" => selected_wet_attack(true, false),
            "midi-off" => selected_wet_attack(false, true),
            "midi-on" => selected_wet_attack(true, true),
            "young-on-refusal-retry" => selected_young_history_refusal_then_fresh_retry(),
            "publication-off-after-ready" => selected_publication_invalidation_after_ready(false),
            "publication-on-after-ready" => selected_publication_invalidation_after_ready(true),
            _ => panic!("unknown declared selected-stem rate-glide child"),
        }
        return;
    }
    for child in [
        "gui-off",
        "gui-on",
        "midi-off",
        "midi-on",
        "young-on-refusal-retry",
        "publication-off-after-ready",
        "publication-on-after-ready",
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SELECTED_TEST, "--nocapture", "--test-threads=1"])
            .env(SELECTED_CHILD, child)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "isolated selected-stem rate-glide {child} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains("selected_stem_rate_glide_success"));
        print!("{stdout}");
        eprint!("{stderr}");
    }
}
