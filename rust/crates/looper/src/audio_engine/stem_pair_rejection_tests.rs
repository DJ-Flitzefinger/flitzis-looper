//! Device-free publication guards with genuine source and callback authority.
//! Tiny synthetic 7/11-frame material and unity-tempo lock commands isolate
//! control ordering. They are not the reported tracks, a four-bar DSP oracle,
//! a hearing test, or proof of which race occurred in the human session.
//! Source material preparation uses the direct native test harness. The exact
//! child runs the productive ordinary StemController/default pool with real
//! pair kernels and external callback ACKs; it does not open an audio device.

use super::*;
use crate::audio_engine::constant_timing;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::resident_relocation::{
    WindowRequest, prepare_window_with_producer, reconcile,
};
use crate::audio_engine::timing::InputClock;
use crate::messages::{STEM_MASK_MELODY, StemMixMode};

struct PlayingFirstPad {
    harness: Harness,
    runtime: InputRuntime,
    second_version: String,
    second_wav_reference: String,
    second_frames: usize,
    first_ticket: PreparedSourceTicket,
    first_selection: Value,
    first_voice: SampleBuffer,
    first_binding: InputRuntimePadBinding,
    _first_saved: crate::audio_engine::project_assets::ProjectAssetLease,
    _second_original: crate::audio_engine::project_assets::ProjectAssetLease,
}

impl PlayingFirstPad {
    fn new() -> Self {
        let mut harness = Harness::new();
        let source = harness.root.join("second.wav");
        fs::write(&source, stereo_wav(11)).unwrap();
        let material = prepare_material(&harness.root, &source, RATE, 2, &|| false).unwrap();
        assert_eq!(material.sample.frame_count(), 11);
        assert_ne!(
            material.metadata()["material_id"],
            harness.material.metadata()["material_id"]
        );
        let second_version = canonical_version(&material);
        let second_wav_reference = format!(
            "samples/materials/M{}/stems/.ready-{GENERATION}",
            material.metadata()["material_id"].as_str().unwrap()
        );
        let second_wav_path = harness.root.parent().unwrap().join(&second_wav_reference);
        let second_frames = material.sample.frame_count();
        write_complete_wavs(
            &second_wav_path,
            &stereo_wav(second_frames),
            &second_version,
        );
        let second_original = harness
            .engine
            .project_assets
            .acquire(&harness.root, &material.lease.original_path)
            .unwrap();
        let preparation =
            control::prepared_for_test(&harness.engine, material, &harness.root).unwrap();
        let source_ticket = control::adopt_for_format(
            &harness.engine,
            1,
            &preparation,
            harness.producer.clone(),
            (2, RATE, harness.root.clone()),
        )
        .unwrap();
        wait_until(|| harness.consumer.peek().is_ok());
        assert_eq!(source_ticket.phase().unwrap(), "pending");
        assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
        assert!(matches!(
            terminal(&harness.engine, source_ticket.request_id()),
            LoaderEvent::Success { id: 1, .. }
        ));
        wait_until(|| harness.engine.cold_loading[1].load(Ordering::Acquire) == 0);
        assert_eq!(source_ticket.phase().unwrap(), "acknowledged");
        assert!(source_ticket.is_current().unwrap());
        preparation.release_preparation().unwrap();

        // Genuine legacy intent admission and its normal parameter drain use
        // the same unity period on both pads. Lock DSP at a non-unity tempo is
        // deliberately outside this small publication-ordering fixture.
        let (parameters, mut parameter_consumer) = rtrb::RingBuffer::new(4);
        let parameters = Arc::new(Mutex::new(parameters));
        for id in [0, 1] {
            constant_timing::publish_legacy_bpm(&harness.engine, &parameters, id, Some(120.0))
                .unwrap();
        }
        parameters
            .lock()
            .unwrap()
            .push(ControlParameterMessage::SetMasterPeriod(0.5))
            .unwrap();
        crate::audio_engine::audio_stream::drain_parameter_messages(
            &mut parameter_consumer,
            &mut harness.callback.mixer,
            &mut harness.callback.transport,
        );
        assert!(parameter_consumer.is_empty());

        let first_ticket = harness.ticket(0);
        let first_pair = harness.prepare(0, &first_ticket, true, None);
        let first_selection = selection(&first_pair);
        let first_saved = harness.save(&first_pair);
        harness
            .engine
            .publish_stem_pair_with_producer(&first_pair, &first_ticket, &harness.producer)
            .unwrap();
        let first_stems = queued_stems(&mut harness.consumer);
        assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
        assert_eq!(first_ticket.publication_status(), "accepted");
        reconcile(&harness.engine).unwrap();
        {
            let mut producer = harness.producer.lock().unwrap();
            producer.push(ControlMessage::SetBpmLock(true)).unwrap();
            producer.push(ControlMessage::SetKeyLock(true)).unwrap();
            producer
                .push(ControlMessage::SetStemMixMode {
                    id: 0,
                    mode: StemMixMode::AllStems,
                    source_version_hash: first_stems.source_version_hash,
                })
                .unwrap();
            producer
                .push(ControlMessage::SetStemEnabledMask {
                    id: 0,
                    enabled_stem_mask: STEM_MASK_MELODY,
                    source_version_hash: first_stems.source_version_hash,
                })
                .unwrap();
        }
        assert_eq!(harness.callback.drain(&mut harness.consumer), 4);
        assert!(harness.callback.mixer.key_lock_for_measurement(0));
        assert!(harness.callback.mixer.key_lock_for_measurement(1));
        assert_eq!(
            harness.callback.mixer.stem_demand_for_measurement(0),
            (
                StemMixMode::AllStems,
                STEM_MASK_MELODY,
                first_stems.source_version_hash
            )
        );

        // MULTILOOP is actual input-runtime policy, not a fabricated callback
        // message. Its admitted trigger must carry non-exclusive ownership.
        let bindings = [0, 1].map(|id| {
            input_runtime_binding::capture(&harness.engine, id)
                .unwrap()
                .unwrap()
        });
        let runtime = InputRuntime::new_with_ownership(
            harness.producer.clone(),
            InputClock::new(),
            harness.engine.input_runtime_ownership.clone(),
        );
        let mut loaded = vec![false; NUM_SAMPLES];
        let mut native_bindings = vec![None; NUM_SAMPLES];
        let mut starts = vec![0.0; NUM_SAMPLES];
        let mut ends = vec![None; NUM_SAMPLES];
        for id in [0, 1] {
            loaded[id] = true;
            native_bindings[id] = Some(&bindings[id]);
        }
        starts[0] = 1.0 / f64::from(RATE);
        ends[0] = Some(6.0 / f64::from(RATE));
        ends[1] = Some(second_frames as f64 / f64::from(RATE));
        runtime
            .set_runtime_state(true, loaded, starts, ends, native_bindings)
            .unwrap();
        assert!(runtime.trigger_pad(0, 123));
        assert!(matches!(
            harness.consumer.peek().unwrap(),
            ControlMessage::TriggerInputPad {
                id: 0,
                exclusive: false,
                ..
            }
        ));
        assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
        let first_voice = harness
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .expect("actual MULTILOOP trigger did not start PAD1")
            .sample
            .clone()
            .unwrap();
        assert!(
            harness
                .callback
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(1))
        );
        let first_binding = input_runtime_binding::capture(&harness.engine, 0)
            .unwrap()
            .unwrap();
        assert!(first_binding.current());
        harness.callback.feedback.events.clear();
        Self {
            harness,
            runtime,
            second_version,
            second_wav_reference,
            second_frames,
            first_ticket,
            first_selection,
            first_voice,
            first_binding,
            _first_saved: first_saved,
            _second_original: second_original,
        }
    }

    fn second_pair(&self) -> (PreparedSourceTicket, PreparedStemPair) {
        let ticket = self
            .harness
            .engine
            .capture_prepared_source(1, self.second_version.clone())
            .unwrap();
        let pair = self
            .harness
            .engine
            .prepare_stem_pair_at_root(
                &self.harness.root,
                1,
                &self.second_version,
                &self.second_wav_reference,
                &ticket,
                true,
                None,
            )
            .unwrap();
        (ticket, pair)
    }

    fn reprepare_after_settlement(&mut self, rejected: &PreparedSourceTicket, selected: &Value) {
        assert_eq!(rejected.publication_status(), "rejected");
        let original_reason = rejected.rejection_reason();
        assert!(original_reason.is_some());
        assert!(
            self.harness
                .callback
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(1))
        );
        reconcile(&self.harness.engine).unwrap();
        // Re-capture and invoke the real preparation kernel after settlement.
        // A rejected permit is never reset, cloned, or promoted to accepted.
        let fresh = self
            .harness
            .engine
            .capture_prepared_source(1, self.second_version.clone())
            .unwrap();
        assert_eq!(fresh.publication_status(), "captured");
        assert_eq!(fresh.rejection_reason(), None);
        let prepared = self
            .harness
            .engine
            .prepare_stem_pair_at_root(
                &self.harness.root,
                1,
                &self.second_version,
                &self.second_wav_reference,
                &fresh,
                true,
                Some(selected["descriptor_reference"].as_str().unwrap()),
            )
            .unwrap();
        assert_eq!(selection(&prepared), *selected);
        self.harness
            .engine
            .publish_stem_pair_with_producer(&prepared, &fresh, &self.harness.producer)
            .unwrap();
        assert_eq!(fresh.publication_status(), "pending");
        assert_eq!(fresh.rejection_reason(), None);
        assert_eq!(self.harness.callback.drain(&mut self.harness.consumer), 1);
        assert_eq!(fresh.publication_status(), "accepted");
        assert_eq!(fresh.rejection_reason(), None);
        assert_eq!(rejected.publication_status(), "rejected");
        assert_eq!(rejected.rejection_reason(), original_reason);
        assert_eq!(
            self.harness.callback.mixer.stems_for_measurement()[1]
                .as_ref()
                .unwrap()
                .publication
                .status(),
            "accepted"
        );
        assert_complete_selection(&self.harness.root, selected);
        self.assert_first_preserved();
    }

    fn assert_first_preserved(&mut self) {
        assert_eq!(self.first_ticket.publication_status(), "accepted");
        assert!(self.first_binding.current());
        let voice = self
            .harness
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .expect("independent PAD1 voice was lost");
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &self.first_voice.samples
        ));
        assert_eq!(
            self.harness.callback.mixer.stem_demand_for_measurement(0).1,
            STEM_MASK_MELODY
        );
        assert!(self.harness.callback.mixer.key_lock_for_measurement(0));
        let mut output = [0.0; 4];
        let mut peaks = [0.0; NUM_SAMPLES];
        self.harness.callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut peaks,
            0,
            &mut self.harness.callback.retirement,
        );
        assert!(output.iter().all(|value| value.is_finite()));
        assert!(
            peaks[0] > 0.0,
            "PAD1 M-only voice lost actual render activity"
        );
        assert_complete_selection(&self.harness.root, &self.first_selection);
    }
}

#[test]
fn active_m_only_pad1_with_multi_key_and_bpm_lock_accepts_stopped_pad2_pair() {
    let mut rig = PlayingFirstPad::new();
    let (ticket, pair) = rig.second_pair();
    let selected = selection(&pair);
    let _saved = rig.harness.save(&pair);
    rig.harness
        .engine
        .publish_stem_pair_with_producer(&pair, &ticket, &rig.harness.producer)
        .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!(ticket.rejection_reason(), None);
    assert!(rig.harness.callback.mixer.stems_for_measurement()[1].is_none());
    assert_eq!(rig.harness.callback.drain(&mut rig.harness.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(ticket.rejection_reason(), None);
    assert_eq!(
        rig.harness.callback.mixer.stems_for_measurement()[1]
            .as_ref()
            .unwrap()
            .stems
            .len(),
        4
    );
    assert!(
        rig.harness
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(1))
    );
    assert_complete_selection(&rig.harness.root, &selected);
    rig.assert_first_preserved();
}

#[test]
fn genuine_pad2_start_before_publish_rejects_own_pair_without_ui_start_feedback() {
    let mut rig = PlayingFirstPad::new();
    let (ticket, pair) = rig.second_pair();
    let selected = selection(&pair);
    let _saved = rig.harness.save(&pair);
    assert!(rig.runtime.trigger_pad(1, 456));
    assert!(matches!(
        rig.harness.consumer.peek().unwrap(),
        ControlMessage::TriggerInputPad {
            id: 1,
            exclusive: false,
            ..
        }
    ));
    assert!(rig.harness.callback.feedback.events.is_empty());
    assert!(
        rig.harness
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(1))
    );
    rig.harness
        .engine
        .publish_stem_pair_with_producer(&pair, &ticket, &rig.harness.producer)
        .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!(rig.harness.callback.drain(&mut rig.harness.consumer), 2);
    assert!(
        rig.harness
            .callback
            .feedback
            .events
            .iter()
            .any(|event| { matches!(event, AudioMessage::SampleStarted { id: 1 }) })
    );
    assert!(
        rig.harness
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(1))
    );
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(ticket.rejection_reason(), Some("pad-playing"));
    assert!(rig.harness.callback.mixer.stems_for_measurement()[1].is_none());
    assert_complete_selection(&rig.harness.root, &selected);
    rig.assert_first_preserved();
    assert!(input_runtime_binding::enqueue_stop_with_producer(
        &rig.harness.engine.input_runtime_ownership,
        &mut rig.harness.producer.lock().unwrap(),
        Some(1),
    ));
    assert_eq!(rig.harness.callback.drain(&mut rig.harness.consumer), 1);
    assert!(
        rig.harness
            .callback
            .feedback
            .events
            .iter()
            .any(|event| { matches!(event, AudioMessage::SampleStopped { id: 1 }) })
    );
    rig.reprepare_after_settlement(&ticket, &selected);
}

#[test]
fn actual_legacy_bpm_or_origin_edit_before_own_ack_rejects_stopped_pad2_pair() {
    for ordered_origin in [false, true] {
        let mut rig = PlayingFirstPad::new();
        let (ticket, pair) = rig.second_pair();
        let selected = selection(&pair);
        let _saved = rig.harness.save(&pair);
        let source_before = rig.harness.engine.sample_cache.lock().unwrap()[1]
            .clone()
            .unwrap();
        let requests_before = rig.harness.engine.pad_request_ids.lock().unwrap()[1];
        rig.harness
            .engine
            .publish_stem_pair_with_producer(&pair, &ticket, &rig.harness.producer)
            .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        if ordered_origin {
            // The real admission revokes authority immediately, even though
            // its metadata command follows the already enqueued publication.
            constant_timing::publish_legacy_origin(
                &rig.harness.engine,
                &rig.harness.producer,
                1,
                1.0 / f64::from(RATE),
            )
            .unwrap();
        } else {
            let (parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
            let parameters = Arc::new(Mutex::new(parameters));
            constant_timing::publish_legacy_bpm(&rig.harness.engine, &parameters, 1, Some(121.0))
                .unwrap();
            crate::audio_engine::audio_stream::drain_parameter_messages(
                &mut parameter_consumer,
                &mut rig.harness.callback.mixer,
                &mut rig.harness.callback.transport,
            );
            assert!(parameter_consumer.is_empty());
        }
        assert_eq!(ticket.publication_status(), "pending");
        assert_eq!(
            rig.harness.callback.drain(&mut rig.harness.consumer),
            if ordered_origin { 2 } else { 1 }
        );
        assert_eq!(ticket.publication_status(), "rejected");
        assert_eq!(ticket.rejection_reason(), Some("timing-changed"));
        assert!(rig.harness.callback.mixer.stems_for_measurement()[1].is_none());
        assert!(Arc::ptr_eq(
            &rig.harness.engine.sample_cache.lock().unwrap()[1]
                .as_ref()
                .unwrap()
                .samples,
            &source_before.samples
        ));
        assert_eq!(
            rig.harness.engine.pad_request_ids.lock().unwrap()[1],
            requests_before
        );
        assert!(
            rig.harness
                .callback
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(1))
        );
        assert_complete_selection(&rig.harness.root, &selected);
        rig.assert_first_preserved();
        rig.reprepare_after_settlement(&ticket, &selected);
    }
}

#[test]
fn actual_window_admission_cannot_race_already_pending_pair_publication() {
    let mut rig = PlayingFirstPad::new();
    let (ticket, pair) = rig.second_pair();
    let selected = selection(&pair);
    let _saved = rig.harness.save(&pair);
    rig.harness
        .engine
        .publish_stem_pair_with_producer(&pair, &ticket, &rig.harness.producer)
        .unwrap();
    let error = prepare_window_with_producer(
        &rig.harness.engine,
        1,
        WindowRequest {
            loop_region: Some((
                1.0 / f64::from(RATE),
                Some((rig.second_frames - 1) as f64 / f64::from(RATE)),
            )),
            key_lock: Some(false),
            ..WindowRequest::default()
        },
        rig.harness.producer.clone(),
    )
    .err()
    .expect("native window admission must preserve pending stem ownership");
    assert!(
        error
            .to_string()
            .contains("resident stem publication is pending")
    );
    assert_eq!(ticket.publication_status(), "pending");
    assert_eq!(ticket.rejection_reason(), None);
    assert!(matches!(
        rig.harness.consumer.peek().unwrap(),
        ControlMessage::PublishPreparedStems { id: 1, .. }
    ));
    assert_eq!(rig.harness.callback.drain(&mut rig.harness.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(ticket.rejection_reason(), None);
    assert_complete_selection(&rig.harness.root, &selected);
    rig.assert_first_preserved();
}

#[test]
fn same_source_real_window_ack_rejects_old_captured_pair_before_enqueue() {
    let mut rig = PlayingFirstPad::new();
    let (ticket, pair) = rig.second_pair();
    let selected = selection(&pair);
    let _saved = rig.harness.save(&pair);
    let before = rig.harness.engine.sample_cache.lock().unwrap()[1]
        .clone()
        .unwrap();
    let request_before = rig.harness.engine.pad_request_ids.lock().unwrap()[1];
    let window = prepare_window_with_producer(
        &rig.harness.engine,
        1,
        WindowRequest {
            loop_region: Some((
                1.0 / f64::from(RATE),
                Some((rig.second_frames - 1) as f64 / f64::from(RATE)),
            )),
            key_lock: Some(false),
            ..WindowRequest::default()
        },
        rig.harness.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        rig.harness.consumer.peek().is_ok()
            || matches!(window.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        window.publication_status(),
        "pending",
        "{:?}",
        window.error().unwrap()
    );
    assert!(matches!(
        rig.harness.consumer.peek().unwrap(),
        ControlMessage::RelocateResident(transaction)
            if transaction.id == 1
                && transaction.sample.resident_start() == 1
                && transaction.sample.resident_end() == rig.second_frames - 1
    ));
    assert_eq!(rig.harness.callback.drain(&mut rig.harness.consumer), 1);
    assert_eq!(window.publication_status(), "accepted");
    assert!(window.is_current());
    // Public capture must reconcile the actual callback-ACKed view itself.
    let fresh = rig
        .harness
        .engine
        .capture_prepared_source(1, rig.second_version.clone())
        .unwrap();
    let current = rig.harness.engine.sample_cache.lock().unwrap()[1]
        .clone()
        .unwrap();
    assert!(before.same_source(&current));
    assert_eq!(
        rig.harness.engine.pad_request_ids.lock().unwrap()[1],
        request_before
    );
    assert_ne!(current.window_revision(), before.window_revision());
    assert!(!Arc::ptr_eq(&current.samples, &before.samples));
    assert!(
        rig.harness
            .engine
            .publish_stem_pair_with_producer(&pair, &ticket, &rig.harness.producer)
            .is_err()
    );
    assert_eq!(ticket.publication_status(), "captured");
    assert_eq!(ticket.rejection_reason(), None);
    assert!(rig.harness.consumer.is_empty());
    let current_pair = rig
        .harness
        .engine
        .prepare_stem_pair_at_root(
            &rig.harness.root,
            1,
            &rig.second_version,
            &rig.second_wav_reference,
            &fresh,
            true,
            Some(selected["descriptor_reference"].as_str().unwrap()),
        )
        .unwrap();
    assert_eq!(selection(&current_pair), selected);
    rig.harness
        .engine
        .publish_stem_pair_with_producer(&current_pair, &fresh, &rig.harness.producer)
        .unwrap();
    assert_eq!(fresh.publication_status(), "pending");
    assert_eq!(rig.harness.callback.drain(&mut rig.harness.consumer), 1);
    assert_eq!(fresh.publication_status(), "accepted");
    assert_complete_selection(&rig.harness.root, &selected);
    rig.assert_first_preserved();
}

const RETRY_CHILD_ROOT: &str = "FLITZI_STEM_PAIR_RETRY_ROOT";
const RETRY_FIRST_CREATED_CASE: &str = "FLITZI_STEM_PAIR_RETRY_FIRST_CREATED_CASE";
const RETRY_CHILD_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::stem_pair_rejection_tests::ordinary_python_retry_child_keeps_fresh_pending_until_own_ack";

struct PairPhysicalProof {
    directories: Vec<(PathBuf, crate::audio_engine::project_assets::FileIdentity)>,
    files: Vec<(
        PathBuf,
        crate::audio_engine::project_assets::FileIdentity,
        [u8; 32],
    )>,
}

fn pair_physical_proof(root: &Path, selected: &Value) -> PairPhysicalProof {
    use crate::audio_engine::project_assets::capture_identity;
    let wav = selected_path(root, selected, "wav_generation");
    let pcm = selected_path(root, selected, "pcm_generation");
    let directories = [&wav, &pcm]
        .into_iter()
        .map(|path| (path.clone(), capture_identity(path).unwrap().unwrap()))
        .collect();
    let mut leaves = vec![
        wav.join(".complete.json"),
        pcm.join("manifest.json"),
        selected_path(root, selected, "descriptor_reference"),
    ];
    for name in STEM_FILE_NAMES {
        leaves.push(wav.join(format!("{name}.wav")));
        leaves.push(pcm.join(format!("{name}.f32le")));
    }
    let files = leaves
        .into_iter()
        .map(|path| {
            let identity = capture_identity(&path).unwrap().unwrap();
            let sha: [u8; 32] = Sha256::digest(fs::read(&path).unwrap()).into();
            (path, identity, sha)
        })
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 13);
    PairPhysicalProof { directories, files }
}

fn assert_pair_physical_proof(proof: &PairPhysicalProof, exclusive: bool) {
    use crate::audio_engine::project_assets::capture_identity;
    use std::os::windows::fs::OpenOptionsExt;
    for (path, identity) in &proof.directories {
        assert_eq!(capture_identity(path).unwrap().as_ref(), Some(identity));
    }
    for (path, identity, sha) in &proof.files {
        assert_eq!(capture_identity(path).unwrap().as_ref(), Some(identity));
        let actual: [u8; 32] = Sha256::digest(fs::read(path).unwrap()).into();
        assert_eq!(actual, *sha);
        if exclusive {
            // An exclusive native open proves no surviving sealed pair handle
            // is invisibly protecting this leaf after the Weak/collector checks.
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(path)
                .unwrap();
        }
    }
}

/// Reopen the actual current JSON in an empty engine, with fresh real source
/// admission, callback-settled legacy timing and ordinary pool/own stem ACK.
/// The 11-frame fixture has no AutomaticTiming/QM envelope to restore.
fn reopen_first_created_selection(root: &Path, fixture: &Value, config: &Path) {
    let engine = Arc::new(AudioEngine::new().unwrap());
    assert!(engine.sample_cache.lock().unwrap()[1].is_none());
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    callback.mixer.stop_sample(0);
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let original = material_paths::resolve(
        root,
        Path::new(fixture["second"]["original_reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    let material = prepare_material(root, &original, RATE, 2, &|| false).unwrap();
    assert_eq!(
        material.metadata()["material_id"],
        fixture["second"]["material_id"]
    );
    let original_owner = engine
        .project_assets
        .acquire(root, &material.lease.original_path)
        .unwrap();
    let prepared = control::prepared_for_test(&engine, material, root).unwrap();
    let source = control::adopt_for_format(
        &engine,
        1,
        &prepared,
        producer.clone(),
        (2, RATE, root.to_path_buf()),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(source.phase().unwrap(), "pending");
    assert_eq!(callback.drain(&mut consumer), 1);
    assert!(matches!(
        terminal(&engine, source.request_id()),
        LoaderEvent::Success { id: 1, .. }
    ));
    wait_until(|| engine.cold_loading[1].load(Ordering::Acquire) == 0);
    assert_eq!(source.phase().unwrap(), "acknowledged");
    assert!(source.is_current().unwrap());
    prepared.release_preparation().unwrap();
    constant_timing::publish_legacy_origin(&engine, &producer, 1, 1.0 / f64::from(RATE)).unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    let locals = Python::attach(|py| {
        let locals = PyDict::new(py);
        locals.set_item("input_json", fixture.to_string()).unwrap();
        locals
            .set_item("config_path", config.to_str().unwrap())
            .unwrap();
        locals
            .set_item(
                "audio",
                Py::new(
                    py,
                    PairWorkerAudio {
                        engine: engine.clone(),
                        root: root.to_path_buf(),
                        producer: producer.clone(),
                        control_thread: std::thread::current().id(),
                        worker_calls: AtomicUsize::new(0),
                        only_pool_threads: AtomicBool::new(true),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        worker_python(
            py,
            &locals,
            r#"
import json
from pathlib import Path
from flitzis_looper.models import SessionState
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.stems import StemController
from flitzis_looper.stem_pair_selection import StemPairSelection
fixture = json.loads(input_json)
persistence = ProjectPersistence.from_config_path(Path(config_path))
assert persistence.load_error is None
project = persistence.project
selection = StemPairSelection.model_validate(fixture['second']['selection'])
assert project.stem_cache[1].pair == selection
assert project.pad_content[1].instance_id == '2' * 32
assert project.sample_paths[1] == fixture['second']['original_reference']
assets = ProjectAssetLifecycle(project, audio)
assets.sync_assignments()
session = SessionState()
controller = StemController(project, session, audio, persistence.mark_dirty, asset_lifecycle=assets)
controller.restore_stem_cache_from_project_state()
assert controller.publish_restored_stem_cache_if_available(1)
assert not controller.stems_available(1)
"#,
        );
        locals.unbind()
    });
    wait_until(|| {
        Python::attach(|py| {
            worker_python(py, locals.bind(py), "controller.on_frame_render()");
            locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .getattr("_pending_stem_publications")
                .unwrap()
                .contains(1)
                .unwrap()
        })
    });
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
ticket = controller._pending_stem_publications[1].source_ticket
assert ticket.publication_status() == 'pending'
assert not controller.stems_available(1)
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
assert project.stem_cache[1].pair == selection
"#,
        )
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
assert ticket.publication_status() == 'accepted'
controller.on_frame_render()
assert controller.stems_available(1) and project.stem_cache[1].available
assert project.stem_cache[1].pair == selection
assert project.pad_content[1].instance_id == '2' * 32
persistence.flush()
reopened = ProjectPersistence.from_config_path(Path(config_path))
assert reopened.load_error is None and reopened.project.stem_cache[1].pair == selection
assert reopened.project.stem_cache[1].available
controller.shut_down()
assets.release_saved_assignments()
"#,
        )
    });
    assert_eq!(callback.drain(&mut consumer), 2);
    assert!(source.is_current().unwrap());
    assert_complete_selection(root, &fixture["second"]["selection"]);
    Python::attach(|py| {
        locals.bind(py).clear();
        py.import("gc").unwrap().call_method0("collect").unwrap();
    });
    drop(locals);
    drop(original_owner);
}

fn run_python_retry_child(first_created_case: Option<&str>) -> Value {
    let directory = tempfile::tempdir().unwrap();
    let output_path = directory.path().join("retry-child-output.txt");
    let output = fs::File::create(&output_path).unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", RETRY_CHILD_TEST, "--nocapture"])
        .current_dir(directory.path())
        .env(RETRY_CHILD_ROOT, directory.path())
        .env_remove(RETRY_FIRST_CREATED_CASE)
        .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(output));
    if let Some(case) = first_created_case {
        command.env(RETRY_FIRST_CREATED_CASE, case);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned publication retry child timed out: {}",
                fs::read_to_string(&output_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(
        status.success(),
        "ordinary controller retry child failed: {}",
        fs::read_to_string(output_path).unwrap()
    );
    let receipt: Value = serde_json::from_slice(
        &fs::read(directory.path().join("retry-child-verified.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["process_id"], child.id());
    for field in [
        "only_pool_threads",
        "native_timing_rejection",
        "new_ticket_own_ack",
        "unchanged_selection",
        "pad1_voice_preserved",
        "current_json_reopened",
    ] {
        assert_eq!(receipt[field], true);
    }
    receipt
}

#[test]
fn ordinary_python_controller_reprepares_native_timing_rejection_and_waits_for_own_ack() {
    let receipt = run_python_retry_child(None);
    assert_eq!(receipt["worker_calls"], 2);
    assert_eq!(receipt["held_pending_polls"], 25);
}

#[test]
fn first_created_rejected_pair_survives_close_and_successful_retry_after_all_readers_end() {
    for case in ["close", "retry"] {
        let receipt = run_python_retry_child(Some(case));
        assert_eq!(receipt["first_created_pair"], true);
        assert_eq!(receipt["closed_before_retry"], case == "close");
        assert_eq!(receipt["worker_calls"], if case == "close" { 1 } else { 2 });
        assert_eq!(
            receipt["held_pending_polls"],
            if case == "close" { 0 } else { 25 }
        );
        for field in [
            "all_pair_owners_released",
            "collector_terminal",
            "same_file_ids_and_sha256",
            "exclusive_pair_files_opened",
            "fresh_engine_source_ack",
            "fresh_engine_stem_ack",
        ] {
            assert_eq!(receipt[field], true);
        }
    }
}

/// Child isolation is solely for Python's actual project-root/cwd convention.
/// PairWorkerAudio routes real native preparation/publication and exact leases;
/// controller polling cannot execute the external native callback or fake ACK.
#[test]
fn ordinary_python_retry_child_keeps_fresh_pending_until_own_ack() {
    let Some(child_directory) = std::env::var_os(RETRY_CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    let first_created_case = std::env::var(RETRY_FIRST_CREATED_CASE).ok();
    let first_created = first_created_case.is_some();
    let close_before_retry = first_created_case.as_deref() == Some("close");
    let rig = PlayingFirstPad::new();
    let second_wavs = rig
        .harness
        .root
        .parent()
        .unwrap()
        .join(&rig.second_wav_reference);
    assert!(
        second_wavs.join(".complete.json").is_file(),
        "{second_wavs:?}"
    );
    for name in STEM_FILE_NAMES {
        assert!(second_wavs.join(format!("{name}.wav")).is_file());
    }
    let second_original_reference = rig.second_version.rsplit_once("|sha256-v1:").unwrap().0;
    let second_material =
        match material_paths::classify(Path::new(second_original_reference)).unwrap() {
            material_paths::AssetKind::Original {
                material: Some(material),
            } => material,
            _ => panic!("fixture must have an actual canonical second material"),
        };
    let (second_selection, _second_saved) = if first_created {
        let source =
            material_paths::resolve(&rig.harness.root, Path::new(second_original_reference))
                .unwrap()
                .path;
        assert!(
            !source
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(".pcm-cache/stems")
                .exists()
        );
        (Value::Null, None)
    } else {
        let (disk_ticket, disk_pair) = rig.second_pair();
        assert_eq!(disk_ticket.publication_status(), "captured");
        (selection(&disk_pair), Some(rig.harness.save(&disk_pair)))
    };
    let mut fixture = json!({
        "first_created": first_created,
        "close_before_retry": close_before_retry,
        "first": {
            "original_reference": rig.harness.material.metadata()["new_reference"],
            "material_id": rig.harness.material.metadata()["material_id"],
            "source_version": rig.harness.version,
            "selection": rig.first_selection,
        },
        "second": {
            "original_reference": second_original_reference,
            "material_id": second_material,
            "source_version": rig.second_version,
            "wav_generation": rig.second_wav_reference,
            "selection": second_selection,
        },
    });
    let PlayingFirstPad {
        harness:
            Harness {
                _temp,
                root,
                engine,
                mut callback,
                producer,
                mut consumer,
                material: _material,
                version: _,
                wav_reference: _,
                wav_path: _,
                _original_owner,
            },
        runtime: _runtime,
        first_ticket,
        first_voice,
        first_binding,
        _first_saved,
        _second_original,
        ..
    } = rig;
    // This exact child is the only running test in its process. The parent
    // Cargo process never changes cwd or imports its source/timing authority.
    std::env::set_current_dir(root.parent().unwrap()).unwrap();
    let engine = Arc::new(engine);
    let locals = Python::attach(|py| {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap()
            .join("src");
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1("insert", (0, source.to_str().unwrap()))
            .unwrap();
        let locals = PyDict::new(py);
        locals.set_item("input_json", fixture.to_string()).unwrap();
        locals
            .set_item(
                "audio",
                Py::new(
                    py,
                    PairWorkerAudio {
                        engine: engine.clone(),
                        root: root.clone(),
                        producer: producer.clone(),
                        control_thread: std::thread::current().id(),
                        worker_calls: AtomicUsize::new(0),
                        only_pool_threads: AtomicBool::new(true),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        worker_python(
            py,
            &locals,
            r#"
import json
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, PadContentIdentity
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.stem_pair_selection import StemPairSelection
fixture = json.loads(input_json)
project = ProjectState()
for pad, name in enumerate(('first', 'second')):
    data = fixture[name]
    pair = StemPairSelection.model_validate(data['selection']) if data['selection'] is not None else None
    project.sample_paths[pad] = data['original_reference']
    project.pad_content[pad] = PadContentIdentity(instance_id=str(pad + 1) * 32, material_id=data['material_id'])
    cache_dir = pair.wav_generation if pair else data['wav_generation']
    project.stem_cache[pad] = StemCacheEntry(source_version=data['source_version'],
        cache_dir=cache_dir, stems=expected_stem_files(cache_dir), available=True, pair=pair)
    project.pad_stem_mix_mode[pad] = 'all_stems'
persistence = ProjectPersistence(project)
assets = ProjectAssetLifecycle(project, audio)
assets.sync_assignments()
session = SessionState(active_sample_ids={0})
session.pad_stem_enabled_mask[0] = 2
controller = StemController(project, session, audio, persistence.mark_dirty, asset_lifecycle=assets)
assert isinstance(controller._stem_worker_pool, StemWorkerPool)
assert controller._stem_worker_pool._executor._max_workers == 2
controller.restore_stem_cache_from_project_state()
first_entry_before = project.stem_cache[0].model_dump()
first_content_before = project.pad_content[0]
second_content_before = project.pad_content[1]
selection = project.stem_cache[1].pair
assert (selection is None) == fixture['first_created']
assert controller.source_version_for_pad(1) == fixture['second']['source_version']
assert controller._entry_files_available(project.stem_cache[1]), project.stem_cache[1]
assert controller.publish_restored_stem_cache_if_available(1)
assert 1 in controller._pair_preparations.pending
assert not controller.stems_available(1)
"#,
        );
        locals.unbind()
    });
    wait_until(|| {
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
            );
            locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .getattr("_pending_stem_publications")
                .unwrap()
                .contains(1)
                .unwrap()
        })
    });
    assert!(matches!(
        consumer.peek().unwrap(),
        ControlMessage::PublishPreparedStems { id: 1, .. }
    ));
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
original_pending = controller._pending_stem_publications[1]
first_attempt = original_pending.source_ticket
original_previous_entry = original_pending.previous_entry
original_previous_lease = original_pending.previous_lease
original_retirement = original_pending.retirement
assert first_attempt.publication_status() == 'pending'
assert first_attempt.rejection_reason() is None
assert original_retirement.remaining > 0
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
selection = project.stem_cache[1].pair
assert isinstance(selection, StemPairSelection)
assert json.loads(original_pending.prepared_pair.selection_json()) == selection.model_dump()
assert not controller.stems_available(1)
"#,
        )
    });
    let (pair_reader, pair_identity, pair_components) = {
        let stems = queued_stems(&mut consumer);
        let identity = Arc::downgrade(&stems.complete_set_identity);
        let components = stems
            .stems
            .clone()
            .map(|stem| Arc::downgrade(&stem.samples));
        let reader = Python::attach(|py| {
            let controller = locals.bind(py).get_item("controller").unwrap().unwrap();
            let pending = controller
                .getattr("_pending_stem_publications")
                .unwrap()
                .get_item(1)
                .unwrap();
            let prepared = pending.getattr("prepared_pair").unwrap();
            let prepared = prepared.extract::<Py<PreparedStemPair>>().unwrap();
            let weak = prepared.borrow(py).reader_lifetime_for_test();
            weak
        });
        (reader, identity, components)
    };
    if first_created {
        let created = pair_reader.upgrade().unwrap();
        assert!(
            created.created_pcm && created.created_descriptor,
            "the actual first pool worker must own both new targets"
        );
    }
    fixture["second"]["selection"] = Python::attach(|py| {
        let json = locals
            .bind(py)
            .get_item("selection")
            .unwrap()
            .unwrap()
            .call_method0("model_dump_json")
            .unwrap()
            .extract::<String>()
            .unwrap();
        serde_json::from_str(&json).unwrap()
    });
    let physical_proof = pair_physical_proof(&root, &fixture["second"]["selection"]);
    // Real timing admission revokes only PAD2 before its queued callback runs.
    // Native source/request identity and stopped state stay unchanged.
    let sample_before = engine.sample_cache.lock().unwrap()[1].clone().unwrap();
    let request_before = engine.pad_request_ids.lock().unwrap()[1];
    constant_timing::publish_legacy_origin(&engine, &producer, 1, 1.0 / f64::from(RATE)).unwrap();
    assert_eq!(callback.drain(&mut consumer), 2);
    assert!(callback.mixer.stems_for_measurement()[1].is_none());
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(1))
    );
    assert!(Arc::ptr_eq(
        &engine.sample_cache.lock().unwrap()[1]
            .as_ref()
            .unwrap()
            .samples,
        &sample_before.samples
    ));
    assert_eq!(engine.pad_request_ids.lock().unwrap()[1], request_before);
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
assert first_attempt.publication_status() == 'rejected'
assert first_attempt.rejection_reason() == 'timing-changed'
assert original_retirement.remaining > 0
if not fixture['close_before_retry']:
    controller.on_frame_render()
else:
    controller._poll_stem_publications()
assert original_retirement.remaining > 0
assert project.stem_cache[1].pair == selection
assert not controller.stems_available(1)
assert first_attempt.publication_status() == 'rejected'
assert 1 not in controller._resident_pairs
"#,
        )
    });
    if !close_before_retry {
        wait_until(|| {
            Python::attach(|py| {
                worker_python(py, locals.bind(py), "controller.on_frame_render()");
                locals
                    .bind(py)
                    .get_item("controller")
                    .unwrap()
                    .unwrap()
                    .getattr("_pending_stem_publications")
                    .unwrap()
                    .get_item(1)
                    .unwrap()
                    .getattr("source_ticket")
                    .unwrap()
                    .call_method0("publication_status")
                    .unwrap()
                    .extract::<String>()
                    .unwrap()
                    == "pending"
            })
        });
        assert!(matches!(
            consumer.peek().unwrap(),
            ControlMessage::PublishPreparedStems { id: 1, .. }
        ));
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                r#"
fresh_pending = controller._pending_stem_publications[1]
fresh_ticket = fresh_pending.source_ticket
assert fresh_ticket is not first_attempt
assert first_attempt.same_source_request(fresh_ticket)
assert fresh_ticket.publication_status() == 'pending'
assert fresh_ticket.rejection_reason() is None
assert fresh_pending.previous_entry is original_previous_entry
assert fresh_pending.previous_lease is original_previous_lease
assert original_pending.previous_lease is None
assert original_retirement.remaining == 0
assert fresh_pending.retirement.remaining > 0
assert controller._publication_retries.pending[1].attempts == 1
assert audio.worker_call_count() == 2 and audio.prepared_only_on_pool_threads()
for _ in range(25):
    controller.on_frame_render()
    assert controller._pending_stem_publications[1] is fresh_pending
    assert controller._publication_retries.pending[1].pending is fresh_pending
    assert controller._publication_retries.pending[1].attempts == 1
    assert fresh_ticket.publication_status() == 'pending'
    assert fresh_pending.retirement.remaining > 0
    assert audio.worker_call_count() == 2
    assert not controller.stems_available(1)
assert project.stem_cache[1].pair == selection
assert project.stem_cache[0].model_dump() == first_entry_before
"#,
            )
        });
        assert_eq!(callback.drain(&mut consumer), 1);
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                r#"
assert first_attempt.publication_status() == 'rejected'
assert first_attempt.rejection_reason() == 'timing-changed'
assert fresh_ticket.publication_status() == 'accepted'
assert fresh_ticket.rejection_reason() is None
controller.on_frame_render()
assert controller.stems_available(1)
assert 1 in controller._resident_pairs
assert not controller._pending_stem_publications
assert not controller._publication_retries.pending
assert not controller._pair_preparations.pending
assert 1 not in session.stem_generation_errors
assert fresh_pending.retirement.remaining == 0
for _ in range(25):
    controller.on_frame_render()
    assert audio.worker_call_count() == 2
assert project.stem_cache[1].pair == selection
assert project.stem_cache[0].model_dump() == first_entry_before
assert project.pad_content[0] == first_content_before
assert project.pad_content[1] == second_content_before
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None
assert reopened.project.stem_cache[1].pair == selection
assert reopened.project.stem_cache[1].available
assert reopened.project.stem_cache[0].model_dump() == first_entry_before
assert reopened.project.pad_content[0] == first_content_before
assert reopened.project.pad_content[1] == second_content_before
"#,
            )
        });
        assert_eq!(callback.drain(&mut consumer), 2); // Own accepted mode/mask commands.
    } else {
        assert!(consumer.is_empty());
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                r#"
assert audio.worker_call_count() == 1
assert original_retirement.remaining > 0
assert project.stem_cache[1].pair == selection
assert not project.stem_cache[1].available and not controller.stems_available(1)
assert first_attempt.publication_status() == 'rejected'
assert controller._publication_retries.pending[1].attempts == 0
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[1].pair == selection
assert not reopened.project.stem_cache[1].available
assert reopened.project.pad_content[1] == second_content_before
"#,
            )
        });
    }
    assert_eq!(first_ticket.publication_status(), "accepted");
    assert!(first_binding.current());
    let preserved_voice = callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert!(Arc::ptr_eq(
        &preserved_voice.sample.as_ref().unwrap().samples,
        &first_voice.samples
    ));
    assert_eq!(
        callback.mixer.stem_demand_for_measurement(0).1,
        STEM_MASK_MELODY
    );
    let mut output = [0.0; 4];
    let mut peaks = [0.0; NUM_SAMPLES];
    callback
        .mixer
        .render_rt_at_output_frame(&mut output, &mut peaks, 0, &mut callback.retirement);
    assert!(output.iter().all(|value| value.is_finite()));
    assert!(peaks[0] > 0.0);
    assert_complete_selection(&root, &fixture["first"]["selection"]);
    assert_complete_selection(&root, &fixture["second"]["selection"]);
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            "controller.shut_down()\nassets.release_saved_assignments()",
        );
    });
    let config_path = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("persistence")
            .unwrap()
            .unwrap()
            .getattr("config_path")
            .unwrap()
            .call_method0("__str__")
            .unwrap()
            .extract::<String>()
            .unwrap()
    });
    let config_path = PathBuf::from(config_path);
    let config_path = if config_path.is_absolute() {
        config_path
    } else {
        root.parent().unwrap().join(config_path)
    };
    if first_created {
        // Remove all Python pending/retry/job handles and the facade's engine
        // Arc, including bound-controller callback cycles. No saved test owner
        // of the second pair was ever manufactured in these two branches.
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                r#"
assert original_retirement.remaining == 0
assert first_attempt.publication_status() == 'rejected'
assert project.stem_cache[1].pair == selection
assert project.stem_cache[1].available == (not fixture['close_before_retry'])
assert not controller._pair_preparations.pending
"#,
            );
            locals.bind(py).clear();
            py.import("gc").unwrap().call_method0("collect").unwrap();
        });
        drop(locals);
        wait_until(|| {
            [0, 1, 215]
                .into_iter()
                .all(|id| engine.cold_loading[id].load(Ordering::Acquire) == 0)
        });
        let assets = engine.project_assets.clone();
        let weak_engine = Arc::downgrade(&engine);
        drop(sample_before);
        drop(callback);
        drop(consumer);
        drop(producer);
        drop(_runtime);
        drop(first_ticket);
        drop(first_voice);
        drop(first_binding);
        drop(_material);
        drop(_original_owner);
        drop(_first_saved);
        drop(_second_original);
        assert!(_second_saved.is_none());
        drop(_second_saved);
        drop(engine);
        wait_until(|| {
            assets.collect_for_test();
            weak_engine.strong_count() == 0
                && pair_reader.strong_count() == 0
                && pair_identity.strong_count() == 0
                && pair_components.iter().all(|pcm| pcm.strong_count() == 0)
        });
        assets.collect_for_test();
        let (retiring, _, _, errors) = assets.status().unwrap();
        assert_eq!(
            retiring, 0,
            "producer retirement must reach a real terminal outcome"
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_pair_physical_proof(&physical_proof, true);
        reopen_first_created_selection(&root, &fixture, &config_path);
        assert_pair_physical_proof(&physical_proof, false);
    }
    fs::write(
        child_directory.join("retry-child-verified.json"),
        serde_json::to_vec(&json!({
            "process_id": std::process::id(),
            "worker_calls": if close_before_retry {1} else {2},
            "held_pending_polls": if close_before_retry {0} else {25},
            "only_pool_threads": true, "native_timing_rejection": true,
            "new_ticket_own_ack": true, "unchanged_selection": true,
            "pad1_voice_preserved": true, "current_json_reopened": true,
            "first_created_pair": first_created, "closed_before_retry": close_before_retry,
            "all_pair_owners_released": first_created, "collector_terminal": first_created,
            "same_file_ids_and_sha256": first_created, "exclusive_pair_files_opened": first_created,
            "fresh_engine_source_ack": first_created, "fresh_engine_stem_ack": first_created,
        }))
        .unwrap(),
    )
    .unwrap();
    std::env::set_current_dir(child_directory).unwrap();
}
