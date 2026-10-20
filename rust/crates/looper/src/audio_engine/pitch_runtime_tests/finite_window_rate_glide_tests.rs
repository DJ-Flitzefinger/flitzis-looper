//! Actual finite WindowWork/WindowACK before genuine piano/MIDI source history.
//!
//! Declared original f32 PCM is copied independently, written into canonical cold
//! artifacts and attached before SourceACK. This direct PCM publication seam is
//! not a decoder, model, device or Human test. The range request, worker, actual
//! callback transaction, accepted ticket, input capture and scheduler are real.

use super::selected_stem_rate_glide_tests::{SELECTED_MASK, assert_selected, publish_selected};
use super::*;
use crate::audio_engine::cold_store::{ColdTransaction, PcmArtifactInput};
use crate::audio_engine::resident_relocation::{
    ResidentWindowTicket, WindowRequest, prepare_window_with_producer, reconcile,
};
use crate::audio_engine::source_reader::{
    reset_tap_observation_for_test, tap_observation_for_test,
};
use crate::messages::PreparedStemSet;
use serde_json::json;
use std::io::Write;

const FINITE_CHILD: &str = "FLITZI_FINITE_PIANO_RATE_GLIDE_CHILD";
const FINITE_TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::piano_rate_controller_tests::piano_rate_glide_tests::finite_window_rate_glide_tests::finite_window_piano_retrigger_keeps_original_glide";

fn write_declared_wav(path: &std::path::Path, samples: &[f32]) {
    let bytes = u32::try_from(samples.len() * 4).unwrap();
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(bytes + 36).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&3_u16.to_le_bytes()).unwrap(); // IEEE binary32 PCM
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&RATE.to_le_bytes()).unwrap();
    file.write_all(&(RATE * 4).to_le_bytes()).unwrap();
    file.write_all(&4_u16.to_le_bytes()).unwrap();
    file.write_all(&32_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&bytes.to_le_bytes()).unwrap();
    for sample in samples {
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
    file.sync_all().unwrap();
}

fn cold_route(lock: bool) -> (std::path::PathBuf, RateRoute) {
    let temp_root = std::env::temp_dir().canonicalize().unwrap();
    assert!(
        temp_root.is_dir(),
        "the caller's existing test temporary directory must exist"
    );
    let directory = tempfile::Builder::new()
        .prefix("declared-cold-")
        .tempdir_in(temp_root)
        .unwrap()
        .keep();
    // Keep material even if a regression panics while a cold worker still owns
    // a reader. The root cleans this owned artifact subtree after child exit.
    std::fs::write(directory.join("OWNER.md"),
        "Owned disposable declared-PCM finite piano/glide fixture. No Human/private audio. Keep until this route, cold work and Native owners are closed.\n").unwrap();
    let original = complete_source(0);
    // An actual allocation copy precedes the canonical identity attachment.
    // Worker output, live finite PCM and descriptors cannot seed the oracle.
    let reference = SampleBuffer {
        samples: original.samples.to_vec().into(),
        channels: original.channels,
        residency: None,
    };
    assert!(!Arc::ptr_eq(&original.samples, &reference.samples));
    let source_file = directory.join("declared-original.wav");
    write_declared_wav(&source_file, &original.samples);
    let mut transaction =
        ColdTransaction::capture(&directory.join("samples"), &source_file, true, &|| false)
            .unwrap();
    let manifest = transaction
        .write_pcm_artifacts(
            PcmArtifactInput {
                samples: &original.samples,
                rate_hz: RATE,
                channels: 1,
                provenance: json!({"decoder":"declared-test-pcm-direct-v1"}),
            },
            PcmArtifactInput {
                samples: &original.samples,
                rate_hz: RATE,
                channels: 1,
                provenance: json!({"processing":"declared-exact-original-f32-v1"}),
            },
            json!({"kind":"declared-test-identity","from_rate":RATE,"to_rate":RATE}),
            &|| false,
        )
        .unwrap();
    transaction.commit(&|| false).unwrap();
    let lease = transaction.into_lease();
    // attach intentionally refuses to replace an existing identity. Start from
    // a fresh no-residency image, before it has any published Native authority.
    let mut source = SampleBuffer {
        samples: original.samples.clone(),
        channels: 1,
        residency: None,
    };
    crate::audio_engine::cold_residency::attach(&mut source, &manifest).unwrap();
    lease.verify_reference(&source).unwrap();
    lease.bind_sample(&source).unwrap();
    let mut route =
        RateRoute::new_with_sources(lock, [source, complete_source(1)], |fixture, sources| {
            fixture
                .control
                .engine
                .project_assets
                .retain_cold(
                    lease.clone(),
                    &sources[0],
                    Arc::downgrade(&fixture.control.engine.project_asset_engine_owner),
                )
                .unwrap();
            fixture.control.engine.cold_leases.lock().unwrap()[0] = Some(lease.clone());
        });
    assert_eq!(
        route.sources[0].samples.as_ref(),
        reference.samples.as_ref()
    );
    assert!(!Arc::ptr_eq(&route.sources[0].samples, &reference.samples));
    route.reference_sources[0] = reference;
    (directory, route)
}

struct FiniteWindow {
    ticket: ResidentWindowTicket,
    sample: SampleBuffer,
    accepted: Option<crate::audio_engine::constant_timing::AcceptedTimingProjection>,
}

impl FiniteWindow {
    fn prepare(route: &mut RateRoute) -> Self {
        let lock = route.lock;
        Self::prepare_range(route, LOOP_START, ENDS[0], Some(lock))
    }

    fn prepare_range(route: &mut RateRoute, start: usize, end: usize, lock: Option<bool>) -> Self {
        let (ticket, accepted, previous) = Python::attach(|py| {
            let engine = route.route.engine.borrow(py);
            let before = engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap();
            assert!(before.current());
            let previous = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
            assert_eq!(before.binding.resident, previous.resident_binding());
            let producer = engine.test_control_channels.as_ref().unwrap().0.clone();
            let ticket = prepare_window_with_producer(
                &engine,
                0,
                WindowRequest {
                    storage_range: Some((
                        start as f64 / f64::from(RATE),
                        end as f64 / f64::from(RATE),
                    )),
                    key_lock: lock,
                    ..WindowRequest::default()
                },
                producer,
            )
            .unwrap();
            assert_ne!(
                ticket.publication_status(),
                "accepted",
                "no callback has acknowledged this new range request"
            );
            (ticket, before.binding.accepted, previous)
        });
        let frame = route.route.transport.output_frame();
        let deadline = Instant::now() + WAIT;
        loop {
            route.callback(0);
            route.assert_no_attacks();
            Python::attach(|py| reconcile(&route.route.engine.borrow(py)).unwrap());
            if ticket.publication_status() == "accepted" {
                assert!(ticket.is_current());
                break;
            }
            assert!(
                !matches!(
                    ticket.publication_status(),
                    "failed" | "cancelled" | "rejected"
                ),
                "{:?}",
                ticket.error().unwrap()
            );
            assert!(
                Instant::now() < deadline,
                "actual finite WindowACK exceeded host wait"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(route.route.transport.output_frame(), frame);
        let sample = Python::attach(|py| {
            route.route.engine.borrow(py).sample_cache.lock().unwrap()[0]
                .clone()
                .unwrap()
        });
        assert_eq!(sample.resident_start(), start);
        assert_eq!(sample.resident_end(), end);
        assert!(sample.resident_start() > 0);
        assert!(sample.resident_end() < sample.frame_count());
        assert!(!sample.same_window(&previous));
        assert!(sample.same_source(&route.sources[0]));
        assert_ne!(sample.window_revision(), previous.window_revision());
        assert_eq!(sample.samples.len(), end - start);
        for (offset, actual) in sample.samples.iter().enumerate() {
            assert_eq!(
                actual.to_bits(),
                route.reference_sources[0].samples[start + offset].to_bits()
            );
        }
        let witness = Self {
            ticket,
            sample,
            accepted,
        };
        witness.bank_current(route);
        eprintln!(
            "finite_piano_glide_WindowACK {}",
            json!({
                "frame":frame,"status":witness.ticket.publication_status(),
                "source":witness.sample.source_address(),"start":witness.sample.resident_start(),
                "end":witness.sample.resident_end(),"full_extent":witness.sample.frame_count(),
                "window_revision":witness.sample.window_revision(),
                "before_first_voice_birth":route.references[0].is_none(),
                "scope":"declared canonical PCM source seam; real WindowWork/range-read/control publication and own actual callback ACK; no decode/inference/device/Human claim"
            })
        );
        witness
    }

    fn bank_current(&self, route: &RateRoute) {
        assert_eq!(self.ticket.publication_status(), "accepted");
        assert!(self.ticket.is_current());
        Python::attach(|py| {
            let engine = route.route.engine.borrow(py);
            let binding = engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap();
            assert!(binding.current());
            assert_eq!(binding.binding.resident, self.sample.resident_binding());
            assert_eq!(binding.binding.accepted, self.accepted);
            assert_eq!(binding.binding.source_address, self.sample.source_address());
            assert!(
                engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .same_window(&self.sample)
            );
        });
    }

    fn current(&self, route: &RateRoute) {
        self.bank_current(route);
        for voice in route
            .route
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active && voice.sample_id == 0)
        {
            assert!(voice.sample.as_ref().unwrap().same_window(&self.sample));
            assert_eq!(
                voice.sample.as_ref().unwrap().samples.as_ptr(),
                self.sample.samples.as_ptr()
            );
        }
    }
}

// All callers inspect a fresh birth's preparation, not existing-source replay.
fn assert_prepared_finite_taps(
    route: &RateRoute,
) -> crate::audio_engine::source_reader::TapReadObservation {
    let taps = route
        .voice(0)
        .stretch
        .native_tap_observation()
        .expect("finite request must retain NormalLoopCoverage");
    assert_eq!(taps.left_reads, 0);
    assert_eq!(taps.right_reads, 0);
    assert_eq!(taps.missing_reads, 0);
    assert_eq!(taps.min_frame, None);
    assert_eq!(taps.max_frame, None);
    eprintln!(
        "finite_piano_glide_prepared_taps {}",
        json!({
            "left":taps.left_reads,"right":taps.right_reads,"missing":taps.missing_reads,
            "min":taps.min_frame,"max":taps.max_frame,"checksum":taps.address_checksum
        })
    );
    taps
}

fn initialize_finite(
    route: &mut RateRoute,
    window: &FiniteWindow,
    selected: bool,
) -> ([Original; 2], Option<PreparedStemSet>) {
    let stems: Option<PreparedStemSet> = selected.then(|| publish_selected(route, &window.sample));
    if let Some(stems) = &stems {
        assert!(
            stems
                .stems
                .iter()
                .all(|component| component.resident_start() == LOOP_START
                    && component.resident_end() == ENDS[0])
        );
        assert!(stems.publication.current());
    }
    window.current(route);
    public_speed(route, INITIAL_SPEED);
    route.callback(0);
    route.assert_no_attacks();
    let initial_rate = route.rate();
    assert_eq!(initial_rate.state, "applied");
    assert_eq!(initial_rate.effective_speed, INITIAL_SPEED);
    assert_eq!(
        initial_rate.effective_period,
        Some(PERIODS[0] / INITIAL_SPEED)
    );
    route.observe_rate();
    let originals = [
        initial_birth(route, 0, INITIAL_SPEED),
        initial_birth(route, 1, INITIAL_SPEED),
    ];
    route.wait_claims();
    route.callback(733);
    route.assert_no_attacks();
    ensure_positive_pre_t_history(route, &originals[0], INITIAL_SPEED, &[7, 12]);
    window.current(route);
    (originals, stems)
}

fn finite_attack(lock: bool, selected: bool) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (directory, mut route) = cold_route(lock);
    let window = FiniteWindow::prepare(&mut route);
    let (originals, stems) = initialize_finite(&mut route, &window, selected);
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    window.current(&route);
    if let Some(stems) = &stems {
        assert_selected(&route, stems);
    }
    let midi = lock != selected;
    let case = format!(
        "finite-{}-{}-{}",
        if selected { "selected" } else { "FullMix" },
        if lock { "on" } else { "off" },
        if midi { "midi" } else { "gui" }
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
    window.current(&route);
    assert_eq!(assert_prepared_finite_taps(&route).right_reads, 0);
    if let Some(stems) = &stems {
        assert_selected(&route, stems);
    }
    // Independently predict a physical right-seam tap during ordinary settled
    // continuation; no seek or production clock seeds this traversal.
    let mut scalar = route.references[0].as_ref().unwrap().clock.clone();
    let mut seam_taps = 0;
    for _ in 0..50_003 {
        scalar.before_sample();
        let phase = scalar.phase();
        seam_taps += usize::from(phase.floor() as usize == ENDS[0] - 1 && phase.fract() > 0.0);
        scalar.advance();
    }
    assert!(
        seam_taps > 0,
        "the independent chronology must traverse the actual last-to-first interpolation interval"
    );
    reset_tap_observation_for_test();
    route.callback(50_003);
    route.assert_no_attacks();
    let taps = tap_observation_for_test();
    assert!(taps.left_reads > 0 && taps.right_reads > 0);
    assert_eq!(taps.missing_reads, 0);
    assert_eq!(taps.min_frame, Some(LOOP_START));
    assert_eq!(taps.max_frame, Some(ENDS[0] - 1));
    window.current(&route);
    assert_eq!(assert_prepared_finite_taps(&route).right_reads, 0);
    if let Some(stems) = &stems {
        assert_selected(&route, stems);
    }
    assert_eq!(route.starts, [2, 1]);
    assert_rate_receipt(&route, &accepted);
    assert!(route.scheduler.is_empty());
    eprintln!(
        "finite_piano_glide_success {}",
        json!({
            "case":case,"mask":selected.then_some(SELECTED_MASK),"independent_seam_taps":seam_taps,
            "left_reads":taps.left_reads,"right_reads":taps.right_reads,"min":taps.min_frame,"max":taps.max_frame,
            "missing":taps.missing_reads,"PCM_max_error":route.maximum,"compared_frames":route.compared,
            "scope":"own real pre-birth finite WindowACK, unchanged physical pins, independent full-original PCM/scalar reset epochs/direct Native/FIFO/EQ and audible peer/master; no broad finite-handover/continuous-O/acoustic/P6/deadline/Human acceptance"
        })
    );
    route.route.close();
    // Material stays on disk through child exit, including deferred Python/native
    // teardown. Root cleanup waits for those child jobs to become terminal.
    drop(stems);
    drop(window);
    Python::attach(|_| drop(route));
    drop(directory);
}

fn finite_selected_expanded_history() {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (directory, mut route) = cold_route(true);
    let window = FiniteWindow::prepare(&mut route);
    let (originals, stems) = initialize_finite(&mut route, &window, true);
    let stems = stems.unwrap();
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    window.current(&route);
    assert_selected(&route, &stems);
    // This existing oracle case requires cold p>2, H>endpoint-H, several actual
    // p profiles across silent warm-up and genuine continuation, and a new E-lattice
    // origin distinct from the previous callback birth.
    // Positive-time source reads use the actual acknowledged finite selected views.
    apply_wet_attack(
        &mut route,
        &originals,
        &accepted,
        INITIAL_SPEED,
        12,
        ("wet-on-plus12-cold-above2", false),
        None,
    );
    window.current(&route);
    assert_selected(&route, &stems);
    assert_eq!(assert_prepared_finite_taps(&route).right_reads, 0);
    assert!(route.voice(0).stretch.native_primed_output_frames() > HISTORY);
    assert_eq!(route.starts, [2, 1]);
    assert_rate_receipt(&route, &accepted);
    eprintln!(
        "finite_piano_glide_success {}",
        json!({
            "case":"finite-selected-on-plus12-expanded-H-cold-above2",
            "mask":SELECTED_MASK,"primed_output_frames":route.voice(0).stretch.native_primed_output_frames(),
            "window_revision":window.sample.window_revision(),"PCM_max_error":route.maximum,
            "compared_frames":route.compared,
            "scope":"existing independent actual-epoch H fixed point/cold-profile oracle with genuine pre-birth finite ACK and retained selected component owners; no full B5/acoustic/resource/Human claim"
        })
    );
    route.route.close();
    drop(stems);
    drop(window);
    Python::attach(|_| drop(route));
    drop(directory);
}

fn finite_actual_window_change_refuses_original_then_fresh_retry() {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (directory, mut route) = cold_route(false);
    let window = FiniteWindow::prepare(&mut route);
    let (originals, stems) = initialize_finite(&mut route, &window, false);
    assert!(stems.is_none());
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    let starts = route.starts;
    let peer_before = VoiceWitness::capture(&route, 1);
    let mut refused = route.admit(0, 7, true);
    assert_eq!(refused.e, refused.t + 8192);
    route.wait_action(&mut refused);
    let ready_native = refused.prepared_native.unwrap();
    assert_ne!(ready_native, route.voice(0).stretch.native_state_address());
    window.current(&route);
    let ready_frame = route.route.transport.output_frame();

    // Real storage-only WindowWork and its actual callback ACK replace the
    // physical view after the original piano action is genuinely READY. No
    // mutation/ACK injection, journal editing or restored epoch is involved.
    // The loaded loop and accepted source timing stay unchanged.
    let expanded = FiniteWindow::prepare_range(&mut route, LOOP_START - 17, ENDS[0] + 23, None);
    assert_eq!(route.route.transport.output_frame(), ready_frame);
    assert!(ready_frame < refused.e);
    assert!(!expanded.sample.same_window(&window.sample));
    assert!(expanded.sample.same_source(&window.sample));
    assert_ne!(
        expanded.sample.samples.as_ptr(),
        window.sample.samples.as_ptr()
    );
    assert!(!window.ticket.is_current());
    expanded.bank_current(&route);
    peer_before.unchanged(&route, 1);
    assert_eq!(route.starts, starts);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(originals[0].note, 0).unwrap())
    );

    // The window transaction is independently permitted. Observe its resulting
    // Native/generation labels as the ownership anchor, without using any live
    // PCM, cursor, FIFO or filter state to rebuild the independent audio oracle.
    let after_ack = VoiceWitness::capture(&route, 0);
    assert_ne!(after_ack.native, ready_native);
    let reference = route.references[0].as_mut().unwrap();
    reference.native = after_ack.native;
    reference.generation = after_ack.generation;
    route.check_voices();
    route.render_to(refused.e);
    assert_eq!(
        route.voice(0).stretch.native_state_address(),
        after_ack.native
    );
    assert_eq!(route.voice(0).generation, after_ack.generation);
    let at_e = VoiceWitness::capture(&route, 0);
    let peer_at_e = VoiceWitness::capture(&route, 1);
    route.callback(0);
    at_e.unchanged(&route, 0);
    peer_at_e.unchanged(&route, 1);
    route.assert_no_attacks();
    // The existing WindowPending policy defers this original event once at E.
    // Its frozen E is never refreshed. Render E's first old-note sample through
    // the independent Raw/EQ/scalar oracle before the next callback retries it.
    assert_eq!(refused.action.state(), "prepared");
    assert!(!refused.action.effective());
    assert_eq!(
        route.scheduler.deferred_due().unwrap().target_frame,
        refused.e
    );
    route.callback(1);
    route.assert_no_attacks();
    let first = route.last_callback_first.unwrap();
    assert_eq!(first.0, refused.e);
    assert_eq!(
        route.voice(0).stretch.native_state_address(),
        after_ack.native
    );
    assert_eq!(route.voice(0).generation, after_ack.generation);
    let after_first = VoiceWitness::capture(&route, 0);
    let peer_after_first = VoiceWitness::capture(&route, 1);
    assert_eq!(route.route.transport.output_frame(), refused.e + 1);
    route.callback(0);
    after_first.unchanged(&route, 0);
    peer_after_first.unchanged(&route, 1);
    route.assert_no_attacks();
    assert_eq!(refused.action.state(), "rejected");
    assert_eq!(refused.action.reason(), Some("prepared_mismatch"));
    assert!(!refused.action.effective());
    assert_eq!(route.starts, starts);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(originals[0].note, 0).unwrap())
    );
    refused.unchanged();
    expanded.bank_current(&route);
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
    // Let genuine reads continue until the original public glide has settled.
    // This fresh request obtains its own current-window Native; the rejected
    // original is neither refreshed nor used as readiness for the retry.
    route.render_to(accepted.execution.unwrap() + 20_073);
    route.wait_claims();
    assert_eq!(route.voice(0).source_playback.tempo_ratio(), GLIDE_SPEED);
    assert_eq!(
        route.voice(0).stretch.native_state_address(),
        after_ack.native
    );
    assert_eq!(route.voice(0).generation, after_ack.generation);
    assert_eq!(route.starts, starts);
    expanded.bank_current(&route);
    emit_case(
        &route,
        "finite-actual-new-WindowACK-refused-old-ready-action",
        &refused,
        &accepted,
    );
    let retry_t = route.route.transport.output_frame();
    let retry_e = retry_t + 8192;
    let retry_timeline = original_timeline(
        &originals[0],
        INITIAL_SPEED,
        accepted.execution.unwrap(),
        retry_t,
    );
    let (_, retry_h_t, _) = reset_bound(&retry_timeline, 7, route.lock)
        .expect("settled fresh retry has genuine pre-T callback history");
    assert_eq!(retry_h_t, HISTORY);
    assert_eq!(retry_timeline.last().unwrap().rate, GLIDE_SPEED);
    let retry_timeline_e = original_timeline(
        &originals[0],
        INITIAL_SPEED,
        accepted.execution.unwrap(),
        retry_e,
    );
    let (_, retry_h_e, _) = reset_bound(&retry_timeline_e, 7, route.lock)
        .expect("fresh retry proves its accepted rate endpoint at E");
    assert_eq!(retry_h_e, HISTORY);
    let retry_prefeed_frames = retry_h_e.div_ceil(512) * 512;
    // The shared helper retains exact T/E, primes independent silent Native input
    // ending at E and emits the complete physical beginning with fresh EQ.
    apply_wet_attack(
        &mut route,
        &originals,
        &accepted,
        INITIAL_SPEED,
        7,
        ("finite-new-WindowACK-fresh-gui-retry", false),
        None,
    );
    expanded.current(&route);
    // The E-anchored Native prefix reads no source PCM, including at settled r=2.
    // No unheard T-to-E beginning is consumed. The unchanged coverage contract
    // still proves that both positive-time seam taps are held.
    let taps = assert_prepared_finite_taps(&route);
    assert_eq!(
        route.voice(0).stretch.native_primed_output_frames(),
        retry_prefeed_frames
    );
    assert_eq!(taps.left_reads, 0);
    assert_eq!(taps.right_reads, 0);
    assert_eq!(route.starts, [starts[0] + 1, starts[1]]);
    assert_eq!(
        route.voice(1).stretch.native_state_address(),
        peer_before.native
    );
    assert_eq!(route.voice(1).generation, peer_before.generation);
    assert_ne!(
        route.voice(0).stretch.native_state_address(),
        after_ack.native
    );
    assert!(!refused.action.effective());
    refused.unchanged();
    assert!(route.scheduler.is_empty());
    eprintln!(
        "finite_piano_glide_success {}",
        json!({
            "case":"actual-storage-WindowACK-after-READY-old-refusal-fresh-retry",
            "old_T":refused.t,"old_E":refused.e,"old_reason":refused.action.reason(),
            "fresh_T":retry_t,"fresh_E":retry_e,"fresh_H_T":retry_h_t,"fresh_H_E":retry_h_e,
            "fresh_prefeed_frames":retry_prefeed_frames,"fresh_unheard_advance_frames":0,
            "fresh_left_reads":taps.left_reads,"fresh_right_reads":taps.right_reads,
        "first_old_E_sample":first,"refusal_observed_frame":refused.e + 1,
            "old_window_revision":window.sample.window_revision(),"new_window_revision":expanded.sample.window_revision(),
            "new_window_start":expanded.sample.resident_start(),"new_window_end":expanded.sample.resident_end(),
            "old_ready_native":ready_native,"post_ACK_native":after_ack.native,"post_ACK_generation":after_ack.generation,
            "PCM_max_error":route.maximum,"compared_frames":route.compared,
            "scope":"actual storage-only worker/WindowACK, original refusal anchored after that independently permitted effect; unchanged complete-original raw/EQ/scalar oracle, own fresh current-window retry; no selected cache replacement/public Generate or broad finite-handover claim"
        })
    );
    route.route.close();
    drop(expanded);
    drop(window);
    Python::attach(|_| drop(route));
    drop(directory);
}

#[test]
fn finite_window_piano_retrigger_keeps_original_glide() {
    if let Ok(child) = std::env::var(FINITE_CHILD) {
        match child.as_str() {
            "FullMix-off" => finite_attack(false, false),
            "FullMix-on" => finite_attack(true, false),
            "selected-off" => finite_attack(false, true),
            "selected-on" => finite_attack(true, true),
            "selected-on-plus12-expanded-H" => finite_selected_expanded_history(),
            "actual-window-change-off-refusal-retry" => {
                finite_actual_window_change_refuses_original_then_fresh_retry()
            }
            _ => panic!("unknown finite piano/glide profile"),
        }
        return;
    }
    for child in [
        "FullMix-off",
        "FullMix-on",
        "selected-off",
        "selected-on",
        "selected-on-plus12-expanded-H",
        "actual-window-change-off-refusal-retry",
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", FINITE_TEST, "--nocapture", "--test-threads=1"])
            .env(FINITE_CHILD, child)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "finite piano/glide {child} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains("finite_piano_glide_success"));
        print!("{stdout}");
        eprint!("{stderr}");
    }
}
