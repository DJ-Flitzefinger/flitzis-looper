//! Device-free ordinary pair preparation and actual callback publication.
//! Direct material preparation/adopt_for_format supplies the source harness;
//! the explicit worker child additionally runs the productive ordinary Python
//! StemController/default pool. Neither harness opens an audio device or hears output.
#![cfg(windows)]

use super::*;
use crate::audio_engine::material_migration::{PreparedMigrationMaterial, prepare_material};
use crate::audio_engine::material_migration_control as control;
use crate::audio_engine::material_paths;
use crate::audio_engine::prepared_source::{PreparedSourceTicket, push_preparation_epoch_message};
use crate::audio_engine::stem_cache::STEM_FILE_NAMES;
use crate::audio_engine::stem_pair::{admitted_component_view_bytes, prepare_complete_pair};
use crate::audio_engine::stem_pair_preparation::PreparedStemPair;
use crate::messages::{ControlParameterMessage, PreparedStemSet, ResidentContext};
use pyo3::prelude::PyAnyMethods;
use pyo3::types::PyBytesMethods;
use pyo3::types::{PyDict, PyDictMethods};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "stem_pair_borrow_tests.rs"]
mod stem_pair_borrow_tests;

#[path = "resident_range_worker_tests.rs"]
mod resident_range_worker_tests;

#[path = "stem_pair_rejection_tests.rs"]
mod stem_pair_rejection_tests;

const RATE: u32 = 48_000;
const GENERATION: &str = "0123456789abcdef0123456789abcdef";

fn stereo_wav(frames: usize) -> Vec<u8> {
    let values = [0_i16, 8192, -16384, 32767, 0, -4096, 16384];
    let data_bytes = u32::try_from(frames * 4).unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        for _ in 0..2 {
            bytes.extend_from_slice(&values[frame % values.len()].to_le_bytes());
        }
    }
    bytes
}

fn write_complete_wavs(path: &Path, encoded: &[u8], version: &str) {
    fs::create_dir_all(path).unwrap();
    let mut hashes = serde_json::Map::new();
    for name in STEM_FILE_NAMES {
        fs::write(path.join(format!("{name}.wav")), encoded).unwrap();
        hashes.insert(
            name.to_owned(),
            json!(format!("{:x}", Sha256::digest(encoded))),
        );
    }
    fs::write(
        path.join(".complete.json"),
        serde_json::to_vec(&json!({
            "schema":"stem-set-sha256-v1", "source_version":version, "stems":hashes,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn canonical_version(material: &PreparedMigrationMaterial) -> String {
    let metadata = material.metadata();
    format!(
        "{}|sha256-v1:{}",
        metadata["new_reference"].as_str().unwrap(),
        metadata["original"]["sha256"].as_str().unwrap()
    )
}

fn source_successes(engine: &AudioEngine, requests: &[u64]) {
    let mut seen = Vec::new();
    wait_until(|| {
        while let Ok(event) = engine.loader_rx.lock().unwrap().try_recv() {
            if let LoaderEvent::Success { request_id, .. } = event {
                if requests.contains(&request_id) {
                    seen.push(request_id);
                }
            } else if let LoaderEvent::Error {
                request_id, error, ..
            } = event
            {
                assert!(
                    !requests.contains(&request_id),
                    "source adoption failed: {error:?}"
                );
            }
        }
        seen.len() == requests.len()
    });
    seen.sort();
    let mut expected = requests.to_vec();
    expected.sort();
    assert_eq!(seen, expected);
    wait_until(|| {
        [0, 215]
            .into_iter()
            .all(|id| engine.cold_loading[id].load(Ordering::Acquire) == 0)
    });
}

struct Harness {
    _temp: tempfile::TempDir,
    root: PathBuf,
    engine: AudioEngine,
    // Keep the fixed mixer arrays out of nested Windows debug fixture returns.
    // The genuine callback and retirement worker retain their exact ownership.
    callback: Box<Callback>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    material: PreparedMigrationMaterial,
    version: String,
    wav_reference: String,
    wav_path: PathBuf,
    _original_owner: crate::audio_engine::project_assets::ProjectAssetLease,
}

impl Harness {
    fn new() -> Self {
        pyo3::Python::initialize();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("samples");
        fs::create_dir(&root).unwrap();
        let original = root.join("old.wav");
        wav(&original, RATE);
        let material = prepare_material(&root, &original, RATE, 2, &|| false).unwrap();
        let version = canonical_version(&material);
        let metadata = material.metadata();
        let wav_reference = format!(
            "samples/materials/M{}/stems/.ready-{GENERATION}",
            metadata["material_id"].as_str().unwrap()
        );
        let wav_path = root.parent().unwrap().join(&wav_reference);
        write_complete_wavs(
            &wav_path,
            &stereo_wav(material.sample.frame_count()),
            &version,
        );
        let engine = AudioEngine::new().unwrap();
        let previous = old(&engine);
        let mut callback = Box::new(Callback::new(&engine, &previous));
        let (producer, mut consumer) = rtrb::RingBuffer::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let preparation = control::prepared_for_test(
            &engine,
            PreparedMigrationMaterial::from_current(
                &root,
                material.sample.clone(),
                material.lease.clone(),
            )
            .unwrap(),
            &root,
        )
        .unwrap();
        let first = control::adopt_for_format(
            &engine,
            0,
            &preparation,
            producer.clone(),
            (2, RATE, root.clone()),
        )
        .unwrap();
        let last = control::adopt_for_format(
            &engine,
            215,
            &preparation,
            producer.clone(),
            (2, RATE, root.clone()),
        )
        .unwrap();
        assert_ne!(first.request_id(), last.request_id());
        wait_until(|| consumer.slots() >= 2);
        assert_eq!(first.phase().unwrap(), "pending");
        assert_eq!(last.phase().unwrap(), "pending");
        assert_eq!(callback.drain(&mut consumer), 2);
        source_successes(&engine, &[first.request_id(), last.request_id()]);
        assert!(first.is_current().unwrap() && last.is_current().unwrap());
        callback.assert_old_voice(&previous);
        // Separation/publication is restricted to stopped pads. Source adoption
        // above still established the old voice's independent immutable reader.
        callback.mixer.stop_sample(0);
        let original_owner = engine
            .project_assets
            .acquire(&root, &material.lease.original_path)
            .unwrap();
        preparation.release_preparation().unwrap();
        Self {
            _temp: temp,
            root,
            engine,
            callback,
            producer,
            consumer,
            material,
            version,
            wav_reference,
            wav_path,
            _original_owner: original_owner,
        }
    }

    fn ticket(&self, id: usize) -> PreparedSourceTicket {
        self.engine
            .capture_prepared_source(id, self.version.clone())
            .unwrap()
    }

    fn prepare(
        &self,
        id: usize,
        ticket: &PreparedSourceTicket,
        components: bool,
        selection: Option<&Value>,
    ) -> PreparedStemPair {
        self.engine
            .prepare_stem_pair_at_root(
                &self.root,
                id,
                &self.version,
                &self.wav_reference,
                ticket,
                components,
                selection.map(|value| value["descriptor_reference"].as_str().unwrap()),
            )
            .unwrap()
    }

    fn save(
        &self,
        pair: &PreparedStemPair,
    ) -> crate::audio_engine::project_assets::ProjectAssetLease {
        let selection = selection(pair);
        let wav_path = selected_path(&self.root, &selection, "wav_generation");
        let owner = self
            .engine
            .project_assets
            .acquire(&self.root, &wav_path)
            .unwrap();
        pair.select();
        owner
    }
}

fn selection(pair: &PreparedStemPair) -> Value {
    serde_json::from_str(pair.selection_json()).unwrap()
}
fn selected_path(root: &Path, value: &Value, field: &str) -> PathBuf {
    material_paths::resolve(root, Path::new(value[field].as_str().unwrap()))
        .unwrap()
        .path
}
fn assert_complete_selection(root: &Path, value: &Value) {
    let wav = selected_path(root, value, "wav_generation");
    let pcm = selected_path(root, value, "pcm_generation");
    for name in STEM_FILE_NAMES {
        assert!(wav.join(format!("{name}.wav")).is_file());
        assert!(pcm.join(format!("{name}.f32le")).is_file());
    }
    assert!(wav.join(".complete.json").is_file());
    assert!(pcm.join("manifest.json").is_file());
    assert!(selected_path(root, value, "descriptor_reference").is_file());
}

fn queued_stems(consumer: &mut rtrb::Consumer<ControlMessage>) -> PreparedStemSet {
    match consumer.peek().unwrap() {
        ControlMessage::PublishPreparedStems { stems, .. } => stems.clone(),
        _ => panic!("expected actual stem publication"),
    }
}

#[test]
fn ordinary_full_mix_builds_complete_disk_pair_without_live_component_publication() {
    let harness = Harness::new();
    let ticket = harness.ticket(0);
    let pair = harness.prepare(0, &ticket, false, None);
    assert!(!pair.has_components());
    let selected = selection(&pair);
    assert_eq!(selected.as_object().unwrap().len(), 5);
    assert_complete_selection(&harness.root, &selected);
    assert!(harness.consumer.peek().is_err());
    assert_eq!(ticket.publication_status(), "captured");
    assert!(
        harness
            .engine
            .publish_stem_pair_with_producer(&pair, &ticket, &harness.producer)
            .is_err()
    );
    assert_eq!(ticket.publication_status(), "captured");
    assert!(harness.consumer.peek().is_err());
    let _saved = harness.save(&pair);
    pair.discard().unwrap();
    assert_complete_selection(&harness.root, &selected);
}

#[test]
fn first_and_last_pad_share_four_pcm_arcs_with_independent_actual_callback_ack() {
    let mut harness = Harness::new();
    let first_ticket = harness.ticket(0);
    let first = harness.prepare(0, &first_ticket, true, None);
    let _saved = harness.save(&first);
    let selected = selection(&first);
    let last_ticket = harness.ticket(215);
    let last = harness.prepare(215, &last_ticket, true, Some(&selected));
    assert_eq!(selection(&last), selected);
    assert_eq!(
        (
            first_ticket.publication_status(),
            last_ticket.publication_status()
        ),
        ("captured", "captured")
    );
    harness
        .engine
        .publish_stem_pair_with_producer(&first, &first_ticket, &harness.producer)
        .unwrap();
    let first_set = queued_stems(&mut harness.consumer);
    assert_eq!(first_set.stems.len(), 4);
    assert_eq!(first_set.available_mask, 0b1111);
    assert_eq!(
        (
            first_ticket.publication_status(),
            last_ticket.publication_status()
        ),
        ("pending", "captured")
    );
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(
        (
            first_ticket.publication_status(),
            last_ticket.publication_status()
        ),
        ("accepted", "captured")
    );
    harness
        .engine
        .publish_stem_pair_with_producer(&last, &last_ticket, &harness.producer)
        .unwrap();
    let last_set = queued_stems(&mut harness.consumer);
    assert!(Arc::ptr_eq(
        &first_set.complete_set_identity,
        &last_set.complete_set_identity
    ));
    for index in 0..4 {
        assert!(Arc::ptr_eq(
            &first_set.stems[index].samples,
            &last_set.stems[index].samples
        ));
    }
    assert_eq!(last_ticket.publication_status(), "pending");
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(last_ticket.publication_status(), "accepted");
    assert_eq!(first_ticket.publication_status(), "accepted");
    assert!(
        harness
            .engine
            .publish_stem_pair_with_producer(&first, &first_ticket, &harness.producer)
            .is_err()
    );
    assert!(harness.consumer.peek().is_err());
    assert_complete_selection(&harness.root, &selected);
}

#[test]
fn full_control_queue_rejects_before_ticket_mutation_and_preserves_saved_reused_pair() {
    let harness = Harness::new();
    let first_ticket = harness.ticket(0);
    let first = harness.prepare(0, &first_ticket, false, None);
    let _saved = harness.save(&first);
    let selected = selection(&first);
    let ticket = harness.ticket(215);
    let reused = harness.prepare(215, &ticket, true, Some(&selected));
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(1);
    producer.push(ControlMessage::Ping()).unwrap();
    let producer = Arc::new(Mutex::new(producer));
    assert!(
        harness
            .engine
            .publish_stem_pair_with_producer(&reused, &ticket, &producer)
            .is_err()
    );
    assert_eq!(ticket.publication_status(), "captured");
    assert!(matches!(consumer.pop().unwrap(), ControlMessage::Ping()));
    assert!(consumer.pop().is_err());
    reused.discard().unwrap();
    drop(reused);
    assert_complete_selection(&harness.root, &selected);
    let current = harness.engine.sample_cache.lock().unwrap()[215]
        .clone()
        .unwrap();
    assert!(current.same_source(&ticket.sample));
}

#[test]
fn stale_source_and_other_pad_ticket_cannot_publish_an_existing_complete_pair() {
    let mut harness = Harness::new();
    let ticket = harness.ticket(0);
    let pair = harness.prepare(0, &ticket, true, None);
    let _saved = harness.save(&pair);
    let selected = selection(&pair);
    let foreign = harness.ticket(215);
    // Equal source bytes do not turn another subscriber's permit into this pad's.
    assert!(
        harness
            .engine
            .prepare_stem_pair_at_root(
                &harness.root,
                0,
                &harness.version,
                &harness.wav_reference,
                &foreign,
                true,
                None
            )
            .is_err()
    );
    harness.engine.input_runtime_ownership.revoke_source(0);
    assert!(
        harness
            .engine
            .publish_stem_pair_with_producer(&pair, &ticket, &harness.producer)
            .is_err()
    );
    assert!(harness.consumer.peek().is_err());
    assert_eq!(ticket.publication_status(), "captured");
    let last = harness.prepare(215, &foreign, true, Some(&selected));
    harness
        .engine
        .publish_stem_pair_with_producer(&last, &foreign, &harness.producer)
        .unwrap();
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(foreign.publication_status(), "accepted");
    assert_complete_selection(&harness.root, &selected);
}

#[test]
fn late_timing_change_rejects_real_queued_stems_and_keeps_other_subscriber_current() {
    let mut harness = Harness::new();
    let ticket = harness.ticket(0);
    let pair = harness.prepare(0, &ticket, true, None);
    let _saved = harness.save(&pair);
    let selected = selection(&pair);
    harness
        .engine
        .publish_stem_pair_with_producer(&pair, &ticket, &harness.producer)
        .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    let (mut parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
    push_preparation_epoch_message(
        &mut parameters,
        ControlParameterMessage::SetPadBpm {
            id: 0,
            bpm: Some(120.0),
        },
        &harness.engine.prepared_source_epochs[0],
        "SetPadBpm",
    )
    .unwrap();
    assert!(parameter_consumer.pop().is_ok());
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(ticket.publication_status(), "rejected");
    let last_ticket = harness.ticket(215);
    let last = harness.prepare(215, &last_ticket, true, Some(&selected));
    harness
        .engine
        .publish_stem_pair_with_producer(&last, &last_ticket, &harness.producer)
        .unwrap();
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(last_ticket.publication_status(), "accepted");
    assert_complete_selection(&harness.root, &selected);
}

#[test]
fn legitimate_different_window_cannot_be_bound_to_previous_prepared_pair() {
    let harness = Harness::new();
    let ticket = harness.ticket(0);
    let pair = harness.prepare(0, &ticket, true, None);
    let _saved = harness.save(&pair);
    let mut different = harness.ticket(0);
    let last = different.sample.resident_end() - 1;
    different.sample = different
        .sample
        .window(
            1,
            last,
            different.sample.window_revision() + 1,
            ResidentContext::FiniteLoop,
        )
        .unwrap();
    assert!(
        harness
            .engine
            .publish_stem_pair_with_producer(&pair, &different, &harness.producer)
            .is_err()
    );
    assert_eq!(different.publication_status(), "captured");
    assert!(harness.consumer.peek().is_err());
    assert_complete_selection(&harness.root, &selection(&pair));
}

#[test]
fn legacy_subscriber_version_is_preserved_while_native_pair_binds_canonical_source() {
    let mut harness = Harness::new();
    let digest = harness.material.metadata()["original"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let legacy_version = format!("samples/old.wav|sha256-v1:{digest}");
    let legacy_cache = harness
        .root
        .join("stems/#1/.ready-fedcba9876543210fedcba9876543210");
    write_complete_wavs(
        &legacy_cache,
        &stereo_wav(harness.material.sample.frame_count()),
        &legacy_version,
    );
    let ticket = harness
        .engine
        .capture_prepared_source(0, legacy_version.clone())
        .unwrap();
    let pair = harness
        .engine
        .prepare_stem_pair_at_root(
            &harness.root,
            0,
            &legacy_version,
            legacy_cache.to_str().unwrap(),
            &ticket,
            true,
            None,
        )
        .unwrap();
    let selected = selection(&pair);
    let descriptor: Value = serde_json::from_slice(
        &fs::read(selected_path(
            &harness.root,
            &selected,
            "descriptor_reference",
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(descriptor["content"]["source_version"], harness.version);
    let _saved = harness.save(&pair);
    harness
        .engine
        .publish_stem_pair_with_producer(&pair, &ticket, &harness.producer)
        .unwrap();
    let set = queued_stems(&mut harness.consumer);
    assert_eq!(
        set.source_version_hash,
        crate::audio_engine::stem_cache::source_version_hash(&legacy_version)
    );
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(legacy_cache.join("instrumental.wav").is_file());
    assert_complete_selection(&harness.root, &selected);
}

#[test]
fn explicit_instrumental_reader_holds_both_areas_without_creating_fifth_live_component() {
    pyo3::Python::initialize();
    let harness = Harness::new();
    let ticket = harness.ticket(0);
    let pair = harness.prepare(0, &ticket, false, None);
    let selected = selection(&pair);
    let reader = pyo3::Python::attach(|py| pair.instrumental_reader(py).unwrap());
    assert_eq!(
        reader.geometry(),
        (0, harness.material.sample.frame_count(), RATE, 2)
    );
    let bytes =
        pyo3::Python::attach(|py| reader.read_f32le(py).unwrap().bind(py).as_bytes().to_vec());
    let pcm_path = selected_path(&harness.root, &selected, "pcm_generation");
    assert_eq!(
        bytes,
        fs::read(pcm_path.join("instrumental.f32le")).unwrap()
    );
    let wav_path = selected_path(&harness.root, &selected, "wav_generation");
    let descriptor_path = selected_path(&harness.root, &selected, "descriptor_reference");
    harness
        .engine
        .project_assets
        .retire(&harness.root, &wav_path, true)
        .unwrap();
    pair.discard().unwrap();
    drop(pair);
    harness.engine.project_assets.collect_for_test();
    assert_complete_selection(&harness.root, &selected);
    assert!(fs::remove_file(wav_path.join("instrumental.wav")).is_err());
    assert!(fs::remove_file(pcm_path.join("instrumental.f32le")).is_err());
    drop(reader);
    wait_until(|| {
        harness.engine.project_assets.collect_for_test();
        !wav_path.exists() && !pcm_path.exists() && !descriptor_path.exists()
    });
}

#[test]
fn component_peak_accounts_for_final_arcs_and_scratch_and_rejects_checked_overflow() {
    const SCRATCH: usize = 64 * 1024;
    assert_eq!(admitted_component_view_bytes(0).unwrap(), SCRATCH);
    assert_eq!(admitted_component_view_bytes(1).unwrap(), SCRATCH + 20);
    let limit = crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES;
    let bound = (limit - SCRATCH) / 20;
    assert!(admitted_component_view_bytes(bound).unwrap() <= limit);
    assert!(admitted_component_view_bytes(bound + 1).unwrap() > limit);
    assert!(admitted_component_view_bytes(usize::MAX / 20 + 1).is_err());
    assert!(admitted_component_view_bytes(usize::MAX).is_err());
}

#[test]
fn cancellation_during_common_marker_reopen_rolls_back_only_owned_new_eligibility() {
    let harness = Harness::new();
    // The producer preparation assignment was released after source adoption.
    // This fault uses the actual current subscriber lease, never revives its ID.
    let material = PreparedMigrationMaterial::from_current(
        &harness.root,
        harness.engine.sample_cache.lock().unwrap()[0]
            .clone()
            .unwrap(),
        harness.engine.cold_leases.lock().unwrap()[0]
            .clone()
            .unwrap(),
    )
    .unwrap();
    let material_root = material
        .lease
        .original_path
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let pair_base = material_root.join(".pcm-cache/stems/v1/.pairs");
    let common_seen = AtomicBool::new(false);
    let cancelled = || {
        let exists = fs::read_dir(&pair_base).ok().is_some_and(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        });
        if exists {
            common_seen.store(true, Ordering::Release);
        }
        exists
    };
    let failed = prepare_complete_pair(
        &harness.root,
        &material,
        &harness.version,
        &harness.wav_reference,
        &cancelled,
    )
    .err()
    .expect("common-marker cancellation must reject eligibility");
    assert!(
        common_seen.load(Ordering::Acquire),
        "the fault must reach actual common publication/reopen: {failed}"
    );
    assert!(fs::read_dir(&pair_base).unwrap().next().is_none());
    assert!(harness.wav_path.join("instrumental.wav").is_file());
    assert!(harness.wav_path.join(".complete.json").is_file());
    let pair = prepare_complete_pair(
        &harness.root,
        &material,
        &harness.version,
        &harness.wav_reference,
        &|| false,
    )
    .unwrap();
    let value = json!({"wav_generation":pair.descriptor.wav_generation,"pcm_generation":pair.descriptor.pcm_generation,"descriptor_reference":pair.descriptor_reference});
    assert_complete_selection(&harness.root, &value);
}

#[test]
fn real_discard_queue_saturation_preserves_creations_and_same_pair_can_retry() {
    let harness = Harness::new();
    let ticket = harness.ticket(0);
    let pair = harness.prepare(0, &ticket, false, None);
    let selected = selection(&pair);
    let pcm = selected_path(&harness.root, &selected, "pcm_generation");
    let common = selected_path(&harness.root, &selected, "descriptor_reference");
    let wav = selected_path(&harness.root, &selected, "wav_generation");
    let wav_bytes = fs::read(wav.join("instrumental.wav")).unwrap();
    // Fill the real shared queue through normal owned, typed leaf admissions.
    // Held job owners keep every queued fixture leaf pending until we release it.
    let mut jobs = Vec::new();
    let mut queued = Vec::new();
    let mut capacity_seen = false;
    for index in 0..1_025 {
        let path = harness.root.join(format!("held-job-{index}.wav"));
        fs::write(&path, b"owned test job").unwrap();
        let job = harness
            .engine
            .project_assets
            .acquire_pin(&harness.root, &path)
            .unwrap();
        match harness
            .engine
            .project_assets
            .retire(&harness.root, &path, false)
        {
            Ok(()) => {
                jobs.push(job);
                queued.push(path);
            }
            Err(error) => {
                assert!(error.to_string().contains("queue full"), "{error}");
                capacity_seen = true;
                break;
            }
        }
    }
    assert!(capacity_seen && !queued.is_empty());
    let failure = pair.discard().unwrap_err();
    assert!(failure.to_string().starts_with("RuntimeError:"));
    assert!(failure.to_string().contains("queue full"));
    assert_complete_selection(&harness.root, &selected);
    assert_eq!(ticket.publication_status(), "captured");
    drop(jobs);
    wait_until(|| {
        harness.engine.project_assets.collect_for_test();
        queued.iter().take(8).all(|path| !path.exists())
    });
    // The failed attempt must not mark this held producer permanently discarded.
    pair.discard().unwrap();
    assert_complete_selection(&harness.root, &selected);
    drop(pair);
    wait_until(|| {
        harness.engine.project_assets.collect_for_test();
        !pcm.exists() && !common.exists()
    });
    assert_eq!(fs::read(wav.join("instrumental.wav")).unwrap(), wav_bytes);
    assert!(wav.join(".complete.json").is_file());
    wait_until(|| {
        harness.engine.project_assets.collect_for_test();
        queued.iter().all(|path| !path.exists())
    });
}

const PAIR_CHILD_ROOT: &str = "FLITZI_STEM_PAIR_REOPEN_ROOT";
const PAIR_CHILD_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::ordinary_pair_new_process_child_reopens_selection_with_own_source_timing_and_stem_ack";

fn validate_actual_durable_selection(encoded: &str) {
    // Use the actual frozen durable DTO; no native runtime ticket is imported.
    pyo3::Python::attach(|py| {
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
        let dto = py
            .import("flitzis_looper.stem_pair_selection")
            .unwrap()
            .getattr("StemPairSelection")
            .unwrap()
            .call_method1("model_validate_json", (encoded,))
            .unwrap();
        let reopened = dto
            .call_method0("model_dump_json")
            .unwrap()
            .extract::<String>()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&reopened).unwrap(),
            serde_json::from_str::<Value>(encoded).unwrap()
        );
    });
}

#[test]
fn ordinary_pair_new_process_reopens_durable_selection_with_fresh_source_timing_and_stem_ack() {
    use super::material_migration_control_tests::{
        migration_callback_8000, restore_migration_timing_with_actual_ack,
    };
    use crate::audio_engine::constant_timing;

    pyo3::Python::initialize();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let (fixture_original, envelope) = constant_timing::migration_test_fixture();
    let original_bytes = fs::read(fixture_original).unwrap();
    let legacy = root.join("old.wav");
    fs::write(&legacy, &original_bytes).unwrap();
    let payload;
    let parent_pcm;
    let parent_loading;
    let parent_leases;
    let parent_readers;
    let parent_stem_pcm: [std::sync::Weak<[f32]>; 4];
    let parent_stem_identity;
    {
        let engine = AudioEngine::new().unwrap();
        assert!(engine.sample_cache.lock().unwrap()[0].is_none());
        let material = prepare_material(&root, &legacy, 8_000, 1, &|| false).unwrap();
        let metadata = material.metadata();
        parent_pcm = Arc::downgrade(&material.sample.samples);
        parent_loading = engine.cold_loading.clone();
        parent_leases = Arc::downgrade(&engine.cold_leases);
        parent_readers = material.lease.reader_lifetime_probe_for_test();
        let version = canonical_version(&material);
        let wav_reference = format!(
            "samples/materials/M{}/stems/.ready-{GENERATION}",
            metadata["material_id"].as_str().unwrap()
        );
        write_complete_wavs(
            &root.parent().unwrap().join(&wav_reference),
            &original_bytes,
            &version,
        );
        let preparation = control::prepared_for_test(&engine, material, &root).unwrap();
        let (producer, mut consumer) = rtrb::RingBuffer::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let mut callback = migration_callback_8000(&engine);
        let source = control::adopt_for_format(
            &engine,
            0,
            &preparation,
            producer.clone(),
            (1, 8_000, root.clone()),
        )
        .unwrap();
        wait_until(|| consumer.peek().is_ok());
        assert_eq!(source.phase().unwrap(), "pending");
        assert!(!source.is_current().unwrap());
        assert_eq!(callback.drain(&mut consumer), 1);
        source_successes(&engine, &[source.request_id()]);
        assert!(source.is_current().unwrap());
        let original = material_paths::resolve(
            &root,
            Path::new(metadata["new_reference"].as_str().unwrap()),
        )
        .unwrap()
        .path;
        restore_migration_timing_with_actual_ack(
            &engine,
            &mut callback,
            &producer,
            &mut consumer,
            &original,
            &envelope,
            source.request_id(),
        );
        let ticket = engine.capture_prepared_source(0, version.clone()).unwrap();
        let pair = engine
            .prepare_stem_pair_at_root(&root, 0, &version, &wav_reference, &ticket, true, None)
            .unwrap();
        let selected = selection(&pair);
        validate_actual_durable_selection(pair.selection_json());
        let _saved_wav = engine
            .project_assets
            .acquire(&root, &selected_path(&root, &selected, "wav_generation"))
            .unwrap();
        pair.select();
        assert_eq!(ticket.publication_status(), "captured");
        engine
            .publish_stem_pair_with_producer(&pair, &ticket, &producer)
            .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        let stem_set = queued_stems(&mut consumer);
        parent_stem_pcm =
            std::array::from_fn(|index| Arc::downgrade(&stem_set.stems[index].samples));
        parent_stem_identity = Arc::downgrade(&stem_set.complete_set_identity);
        assert_eq!(callback.drain(&mut consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        payload = json!({"original_reference":metadata["new_reference"], "source_version":version,
            "selection":selected, "timing_envelope":serde_json::from_str::<Value>(&envelope).unwrap()});
        preparation.release_preparation().unwrap();
    }
    // Actual worker and weak file/PCM endpoints must end before child spawn.
    // The child imports durable bytes only; request/epoch/permit are absent.
    wait_until(|| {
        parent_loading[0].load(Ordering::Acquire) == 0
            && parent_pcm.strong_count() == 0
            && parent_leases.strong_count() == 0
            && parent_readers()
            && parent_stem_pcm.iter().all(|pcm| pcm.strong_count() == 0)
            && parent_stem_identity.strong_count() == 0
    });
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    let payload_path = directory.path().join("durable-selection.json");
    fs::write(&payload_path, &payload_bytes).unwrap();
    let output_path = directory.path().join("pair-child-output.txt");
    let output = fs::File::create(&output_path).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", PAIR_CHILD_TEST, "--nocapture"])
        .env(PAIR_CHILD_ROOT, directory.path())
        .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(output))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned pair child timed out: {}",
                fs::read_to_string(&output_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let output = fs::read_to_string(&output_path).unwrap();
    assert!(
        status.success() && output.contains("1 passed"),
        "fresh pair child failed: {output}"
    );
    let receipt: Value = serde_json::from_slice(
        &fs::read(directory.path().join("pair-child-verified.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["process_id"].as_u64(), Some(u64::from(child.id())));
    assert_ne!(
        receipt["process_id"].as_u64(),
        Some(u64::from(std::process::id()))
    );
    for field in [
        "source_callback_verified",
        "automatic_timing_callback_verified",
        "stem_callback_verified",
        "durable_selection_verified",
    ] {
        assert_eq!(receipt[field], true);
    }
    assert_eq!(receipt["selection"], payload["selection"]);
    assert_eq!(fs::read(&payload_path).unwrap(), payload_bytes);
    assert_complete_selection(&root, &payload["selection"]);
    assert_eq!(fs::read(&legacy).unwrap(), original_bytes);
}

#[test]
fn ordinary_pair_new_process_child_reopens_selection_with_own_source_timing_and_stem_ack() {
    use super::material_migration_control_tests::{
        migration_callback_8000, restore_migration_timing_with_actual_ack,
    };
    use crate::audio_engine::constant_timing;

    let Some(directory) = std::env::var_os(PAIR_CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    pyo3::Python::initialize();
    let root = directory.join("samples");
    let encoded = fs::read(directory.join("durable-selection.json")).unwrap();
    let payload: Value = serde_json::from_slice(&encoded).unwrap();
    let selected = &payload["selection"];
    let durable = serde_json::to_string(selected).unwrap();
    validate_actual_durable_selection(&durable);
    let descriptor_path = selected_path(&root, selected, "descriptor_reference");
    let descriptor_before = fs::read(&descriptor_path).unwrap();
    let pcm_base = selected_path(&root, selected, "pcm_generation")
        .parent()
        .unwrap()
        .to_owned();
    let generations_before = fs::read_dir(&pcm_base)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    let original = material_paths::resolve(
        &root,
        Path::new(payload["original_reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    let version = payload["source_version"].as_str().unwrap();
    let engine = AudioEngine::new().unwrap();
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    assert!(engine.cold_leases.lock().unwrap()[0].is_none());
    assert_eq!(engine.loaded_source_generations.lock().unwrap()[0], (0, 0));
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
    let material = prepare_material(&root, &original, 8_000, 1, &|| false).unwrap();
    assert_eq!(canonical_version(&material), version);
    let preparation = control::prepared_for_test(&engine, material, &root).unwrap();
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    let producer = Arc::new(Mutex::new(producer));
    let mut callback = migration_callback_8000(&engine);
    let source = control::adopt_for_format(
        &engine,
        0,
        &preparation,
        producer.clone(),
        (1, 8_000, root.clone()),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(source.phase().unwrap(), "pending");
    assert!(!source.is_current().unwrap());
    assert_eq!(callback.drain(&mut consumer), 1);
    source_successes(&engine, &[source.request_id()]);
    assert!(source.is_current().unwrap());
    let envelope = serde_json::to_string(&payload["timing_envelope"]).unwrap();
    restore_migration_timing_with_actual_ack(
        &engine,
        &mut callback,
        &producer,
        &mut consumer,
        &original,
        &envelope,
        source.request_id(),
    );
    let ticket = engine.capture_prepared_source(0, version.into()).unwrap();
    let pair = engine
        .prepare_stem_pair_at_root(
            &root,
            0,
            version,
            selected["wav_generation"].as_str().unwrap(),
            &ticket,
            true,
            Some(selected["descriptor_reference"].as_str().unwrap()),
        )
        .unwrap();
    assert_eq!(selection(&pair), *selected);
    assert!(pair.has_components());
    let _saved = engine
        .project_assets
        .acquire(&root, &selected_path(&root, selected, "wav_generation"))
        .unwrap();
    pair.select();
    assert_eq!(ticket.publication_status(), "captured");
    engine
        .publish_stem_pair_with_producer(&pair, &ticket, &producer)
        .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    let stems = queued_stems(&mut consumer);
    assert_eq!(stems.stems.len(), 4);
    assert!(stems.accepted_timing.is_some());
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(
        constant_timing::export_current(&engine, 0, original.to_string_lossy().into())
            .unwrap()
            .map(|value| serde_json::from_str::<Value>(&value).unwrap()),
        Some(payload["timing_envelope"].clone())
    );
    assert_eq!(fs::read(&descriptor_path).unwrap(), descriptor_before);
    assert_eq!(
        fs::read_dir(&pcm_base)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>(),
        generations_before
    );
    assert_eq!(
        fs::read(directory.join("durable-selection.json")).unwrap(),
        encoded
    );
    preparation.release_preparation().unwrap();
    fs::write(
        directory.join("pair-child-verified.json"),
        serde_json::to_vec(&json!({
            "process_id":std::process::id(), "source_callback_verified":true,
            "automatic_timing_callback_verified":true, "stem_callback_verified":true,
            "durable_selection_verified":true, "selection":selected,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn full_mix_releases_actual_component_owners_after_sink_job_and_voice_then_fresh_ack() {
    use crate::audio_engine::resident_relocation;
    use crate::messages::StemMixMode;

    for active_voice in [false, true] {
        let mut harness = Harness::new();
        let ticket = harness.ticket(0);
        let pair = harness.prepare(0, &ticket, true, None);
        let selected = selection(&pair);
        let _saved_wav = harness.save(&pair);
        let pair_reader = pair.reader_lifetime_for_test();
        harness
            .engine
            .publish_stem_pair_with_producer(&pair, &ticket, &harness.producer)
            .unwrap();
        let job = queued_stems(&mut harness.consumer);
        let weak: [_; 4] = std::array::from_fn(|index| Arc::downgrade(&job.stems[index].samples));
        assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        resident_relocation::reconcile(&harness.engine).unwrap();
        if active_voice {
            assert!(harness.callback.mixer.set_stem_mix_mode(
                0,
                StemMixMode::AllStems,
                job.source_version_hash
            ));
            assert!(harness.callback.mixer.play_sample(0, 1.0));
        }
        resident_relocation::set_stem_pair_full_mix_with_producer(
            &harness.engine,
            0,
            &harness.producer,
        )
        .unwrap();
        resident_relocation::reconcile(&harness.engine).unwrap();
        assert_eq!(
            resident_relocation::history_count_for_test(&harness.engine, 0),
            1
        );
        match harness.consumer.peek().unwrap() {
            ControlMessage::SetStemPairFullMix { id, retired } => {
                assert_eq!(*id, 0);
                assert_eq!(retired.len(), 2);
                assert!(retired.iter().flatten().any(|set| Arc::ptr_eq(
                    &set.complete_set_identity,
                    &job.complete_set_identity
                )));
            }
            _ => panic!("expected the actual coupled FullMix control command"),
        }
        drop(pair);
        // Queue/bank owners must survive a callback that cannot retire them.
        harness.callback.retirement.slots = 0;
        assert_eq!(harness.callback.drain(&mut harness.consumer), 0);
        assert!(weak.iter().all(|reader| reader.strong_count() > 0));
        harness.callback.retirement.slots = 3;
        assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
        let immediate_retired = harness
            .callback
            .retirement
            .retired
            .iter()
            .filter(|buffer| match buffer {
                RetiredAudioBuffer::PreparedStems(set) => {
                    Arc::ptr_eq(&set.complete_set_identity, &job.complete_set_identity)
                }
                _ => false,
            })
            .count();
        assert_eq!(
            immediate_retired,
            if active_voice { 1 } else { 2 },
            "an active transition retains its actual bank owner; a stopped bank can retire immediately"
        );
        harness.callback.retirement.slots = usize::MAX;
        let mut output = [0.0_f32; 512];
        let mut peaks = [0.0_f32; crate::audio_engine::constants::NUM_SAMPLES];
        // Render the real transition and retire voices through the existing sink.
        // No sleep or artificial permit acceptance stands in for callback work.
        let callback = harness.callback.as_mut();
        for _ in 0..64 {
            callback
                .mixer
                .render_rt(&mut output, &mut peaks, &mut callback.retirement);
        }
        callback.mixer.stop_sample_rt(0, &mut callback.retirement);
        for _ in 0..64 {
            callback
                .mixer
                .render_rt(&mut output, &mut peaks, &mut callback.retirement);
        }
        let retired_handles = harness
            .callback
            .retirement
            .retired
            .iter()
            .filter(|buffer| match buffer {
                RetiredAudioBuffer::PreparedStems(set) => {
                    Arc::ptr_eq(&set.complete_set_identity, &job.complete_set_identity)
                }
                _ => false,
            })
            .count();
        assert!(
            retired_handles >= 2,
            "both control and bank component owners reach the real retirement sink"
        );
        drop(job);
        assert!(
            weak.iter().all(|reader| reader.strong_count() > 0),
            "the retirement sink remains a real owner"
        );
        harness.callback.retirement.retired.clear();
        assert!(
            weak.iter().all(|reader| reader.strong_count() == 0),
            "FullMix must leave no control/bank/voice component owner"
        );
        assert_eq!(
            resident_relocation::history_count_for_test(&harness.engine, 0),
            1
        );
        assert!(pair_reader.strong_count() > 0);
        resident_relocation::reconcile(&harness.engine).unwrap();
        assert_eq!(
            resident_relocation::history_count_for_test(&harness.engine, 0),
            0
        );
        assert_eq!(
            pair_reader.strong_count(),
            0,
            "dead logical history must release its actual retained verified pair reader"
        );
        harness.engine.project_assets.collect_for_test();
        assert_complete_selection(&harness.root, &selected);
        let next_ticket = harness.ticket(0);
        assert_eq!(next_ticket.publication_status(), "captured");
        let next = harness.prepare(0, &next_ticket, true, Some(&selected));
        assert_eq!(selection(&next), selected);
        assert!(next.has_components());
        harness
            .engine
            .publish_stem_pair_with_producer(&next, &next_ticket, &harness.producer)
            .unwrap();
        let next_set = queued_stems(&mut harness.consumer);
        for index in 0..4 {
            assert!(
                !weak[index].ptr_eq(&Arc::downgrade(&next_set.stems[index].samples)),
                "the ended resident allocation cannot be reused as a live owner"
            );
        }
        assert_eq!(next_ticket.publication_status(), "pending");
        assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
        assert_eq!(next_ticket.publication_status(), "accepted");
        assert_eq!(ticket.publication_status(), "accepted");
        assert_complete_selection(&harness.root, &selected);
    }
}

const WORKER_CHILD_ROOT: &str = "FLITZI_STEM_PAIR_WORKER_ROOT";
const WORKER_CHILD_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::ordinary_python_worker_child_requires_own_callback_ack_and_reprepares_after_full_mix";

/// Test-only device/root routing. Preparation, tickets, publication, saved asset
/// owners and scalar messages are the actual native implementations. The callback
/// remains outside Python so a controller poll cannot accidentally acknowledge it.
#[pyclass]
struct PairWorkerAudio {
    engine: Arc<AudioEngine>,
    root: PathBuf,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    control_thread: std::thread::ThreadId,
    worker_calls: AtomicUsize,
    only_pool_threads: AtomicBool,
}

#[pymethods]
impl PairWorkerAudio {
    fn capture_prepared_source(
        &self,
        id: usize,
        source_version: String,
    ) -> PyResult<PreparedSourceTicket> {
        self.engine.capture_prepared_source(id, source_version)
    }

    #[pyo3(signature=(id,source_version,cache_dir,source_ticket,components,descriptor_reference=None))]
    fn prepare_stem_pair(
        &self,
        py: Python<'_>,
        id: usize,
        source_version: String,
        cache_dir: String,
        source_ticket: &PreparedSourceTicket,
        components: bool,
        descriptor_reference: Option<String>,
    ) -> PyResult<PreparedStemPair> {
        let worker_name: String = py
            .import("threading")?
            .call_method0("current_thread")?
            .getattr("name")?
            .extract()?;
        if std::thread::current().id() == self.control_thread || !worker_name.starts_with("stems_")
        {
            self.only_pool_threads.store(false, Ordering::Release);
        }
        self.worker_calls.fetch_add(1, Ordering::AcqRel);
        py.detach(|| {
            self.engine.prepare_stem_pair_at_root(
                &self.root,
                id,
                &source_version,
                &cache_dir,
                source_ticket,
                components,
                descriptor_reference.as_deref(),
            )
        })
        .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    fn publish_stem_pair(
        &self,
        prepared: &PreparedStemPair,
        source_ticket: &PreparedSourceTicket,
    ) -> PyResult<()> {
        self.engine
            .publish_stem_pair_with_producer(prepared, source_ticket, &self.producer)
    }

    fn acquire_project_asset_lease(
        &self,
        py: Python<'_>,
        path: String,
    ) -> PyResult<crate::audio_engine::project_assets::ProjectAssetLease> {
        py.detach(|| {
            self.engine
                .project_assets
                .acquire(&self.root, Path::new(&path))
        })
        .map_err(|error| pyo3::exceptions::PyRuntimeError::new_err(error.to_string()))
    }

    fn retire_project_asset(&self, path: String, recursive: bool) -> PyResult<()> {
        self.engine
            .project_assets
            .retire(&self.root, Path::new(&path), recursive)
            .map_err(|error| pyo3::exceptions::PyRuntimeError::new_err(error.to_string()))
    }

    fn output_sample_rate(&self) -> u32 {
        RATE
    }

    #[pyo3(signature=(id,mode,source_version=None))]
    fn set_stem_mix_mode(
        &self,
        id: usize,
        mode: &str,
        source_version: Option<String>,
    ) -> PyResult<()> {
        use crate::messages::StemMixMode;
        let (mode, hash) = match mode {
            "full_mix" => (StemMixMode::FullMix, 0),
            "all_stems" => (
                StemMixMode::AllStems,
                crate::audio_engine::stem_cache::source_version_hash(
                    source_version.as_deref().unwrap(),
                ),
            ),
            _ => return Err(pyo3::exceptions::PyValueError::new_err("invalid stem mode")),
        };
        self.producer
            .lock()
            .unwrap()
            .push(ControlMessage::SetStemMixMode {
                id,
                mode,
                source_version_hash: hash,
            })
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("scalar queue full"))
    }

    fn set_stem_enabled_mask(
        &self,
        id: usize,
        enabled_stem_mask: u8,
        source_version: String,
    ) -> PyResult<()> {
        self.producer
            .lock()
            .unwrap()
            .push(ControlMessage::SetStemEnabledMask {
                id,
                enabled_stem_mask,
                source_version_hash: crate::audio_engine::stem_cache::source_version_hash(
                    &source_version,
                ),
            })
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("scalar queue full"))
    }

    fn set_stem_pair_full_mix(&self, id: usize) -> PyResult<()> {
        crate::audio_engine::resident_relocation::set_stem_pair_full_mix_with_producer(
            &self.engine,
            id,
            &self.producer,
        )
    }

    fn worker_call_count(&self) -> usize {
        self.worker_calls.load(Ordering::Acquire)
    }
    fn prepared_only_on_pool_threads(&self) -> bool {
        self.only_pool_threads.load(Ordering::Acquire)
    }
}

fn worker_python(py: Python<'_>, locals: &Bound<'_, PyDict>, code: &str) {
    py.run(&std::ffi::CString::new(code).unwrap(), Some(locals), None)
        .unwrap_or_else(|error| panic!("ordinary worker script failed: {error:?}\n{code}"));
}

#[test]
fn ordinary_python_worker_runs_native_pair_preparation_and_persists_current_selection() {
    pyo3::Python::initialize();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("samples");
    fs::create_dir(&root).unwrap();
    let legacy = root.join("old.wav");
    wav(&legacy, RATE);
    let payload = {
        let material = prepare_material(&root, &legacy, RATE, 2, &|| false).unwrap();
        let metadata = material.metadata();
        let version = canonical_version(&material);
        let reference = format!(
            "samples/materials/M{}/stems/.ready-{GENERATION}",
            metadata["material_id"].as_str().unwrap()
        );
        write_complete_wavs(
            &directory.path().join(&reference),
            &stereo_wav(material.sample.frame_count()),
            &version,
        );
        let payload = json!({"original_reference":metadata["new_reference"], "source_version":version,
            "wav_generation":reference, "material_id":metadata["material_id"]});
        material.lease.release_preparation_assignment();
        payload
    };
    fs::write(
        directory.path().join("worker-input.json"),
        serde_json::to_vec(&payload).unwrap(),
    )
    .unwrap();
    let output_path = directory.path().join("worker-child-output.txt");
    let output = fs::File::create(&output_path).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", WORKER_CHILD_TEST, "--nocapture"])
        // Only this new process changes its own cwd; the Cargo runner is untouched.
        .current_dir(directory.path())
        .env(WORKER_CHILD_ROOT, directory.path())
        .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
        .stderr(std::process::Stdio::from(output))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned worker child timed out: {}",
                fs::read_to_string(&output_path).unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let output = fs::read_to_string(output_path).unwrap();
    assert!(
        status.success() && output.contains("1 passed"),
        "ordinary worker child failed: {output}"
    );
    let receipt: Value = serde_json::from_slice(
        &fs::read(directory.path().join("worker-child-verified.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["process_id"], child.id());
    assert_eq!(receipt["worker_calls"], 2);
    for field in [
        "only_pool_threads",
        "pending_before_callback",
        "accepted_after_callback",
        "full_mix_components_released",
        "current_json_reopened",
    ] {
        assert_eq!(receipt[field], true);
    }
    assert_complete_selection(&root, &receipt["selection"]);
}

#[test]
fn ordinary_python_worker_child_requires_own_callback_ack_and_reprepares_after_full_mix() {
    let Some(directory) = std::env::var_os(WORKER_CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    pyo3::Python::initialize();
    let root = directory.join("samples");
    let encoded = fs::read_to_string(directory.join("worker-input.json")).unwrap();
    let input: Value = serde_json::from_str(&encoded).unwrap();
    let original = material_paths::resolve(
        &root,
        Path::new(input["original_reference"].as_str().unwrap()),
    )
    .unwrap()
    .path;
    let engine = Arc::new(AudioEngine::new().unwrap());
    assert!(engine.sample_cache.lock().unwrap()[0].is_none());
    let previous = old(&engine);
    let mut callback = Callback::new(&engine, &previous);
    let (producer, mut consumer) = rtrb::RingBuffer::new(16);
    let producer = Arc::new(Mutex::new(producer));
    let material = prepare_material(&root, &original, RATE, 2, &|| false).unwrap();
    let preparation = control::prepared_for_test(&engine, material, &root).unwrap();
    let source = control::adopt_for_format(
        &engine,
        0,
        &preparation,
        producer.clone(),
        (2, RATE, root.clone()),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok());
    assert_eq!(source.phase().unwrap(), "pending");
    assert_eq!(callback.drain(&mut consumer), 1);
    source_successes(&engine, &[source.request_id()]);
    assert!(source.is_current().unwrap());
    callback.mixer.stop_sample_rt(0, &mut callback.retirement);
    callback.retirement.retired.clear();
    preparation.release_preparation().unwrap();
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
        locals.set_item("input_json", &encoded).unwrap();
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
from pathlib import Path
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, PadContentIdentity
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.stem_workers import StemWorkerPool, STEM_WORKERS, STEM_QUEUED_JOBS
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.stem_pair_selection import StemPairSelection
fixture = json.loads(input_json)
project = ProjectState()
project.sample_paths[0] = fixture['original_reference']
project.pad_content[0] = PadContentIdentity(instance_id='d' * 32, material_id=fixture['material_id'])
project.stem_cache[0] = StemCacheEntry(source_version=fixture['source_version'], cache_dir=fixture['wav_generation'],
    stems=expected_stem_files(fixture['wav_generation']), available=True)
project.pad_stem_mix_mode[0] = 'all_stems'
persistence = ProjectPersistence(project)
assets = ProjectAssetLifecycle(project, audio)
controller = StemController(project, SessionState(), audio, persistence.mark_dirty, asset_lifecycle=assets)
assert isinstance(controller._stem_worker_pool, StemWorkerPool)
assert STEM_WORKERS == 2 and STEM_QUEUED_JOBS == 32
assert controller._stem_worker_pool._executor._max_workers == 2
controller.restore_stem_cache_from_project_state()
assert not controller.stems_available(0)
assert controller.publish_restored_stem_cache_if_available(0)
"#,
        );
        locals.unbind()
    });
    wait_until(|| {
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                "controller.on_frame_render()\nassert not controller._session.stem_generation_errors, controller._session.stem_generation_errors",
            );
            locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .getattr("_pending_stem_publications")
                .unwrap()
                .contains(0)
                .unwrap()
        })
    });
    let first_set = queued_stems(&mut consumer);
    let first_weak: [_; 4] =
        std::array::from_fn(|index| Arc::downgrade(&first_set.stems[index].samples));
    drop(first_set);
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
pending = controller._pending_stem_publications[0]
first_ticket = pending.source_ticket
assert first_ticket.publication_status() == 'pending'
assert not controller.stems_available(0)
assert project.stem_cache[0].pair is not None
selection = project.stem_cache[0].pair
assert isinstance(selection, StemPairSelection)
assert selection.wav_generation == fixture['wav_generation']
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
persistence.flush()
pending_reopen = ProjectPersistence.from_config_path(persistence.config_path)
assert pending_reopen.load_error is None
assert pending_reopen.project.stem_cache[0].pair == selection
assert not pending_reopen.project.stem_cache[0].available
del pending
"#,
        )
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
assert first_ticket.publication_status() == 'accepted'
controller.on_frame_render()
assert controller.stems_available(0) and 0 in controller._resident_pairs
assert not controller._pending_stem_publications
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None
assert reopened.project.stem_cache[0].pair == selection
assert reopened.project.stem_cache[0].available
assert reopened.project.pad_content[0] == project.pad_content[0]
assert reopened.project.pad_stem_mix_mode[0] == 'all_stems'
"#,
        )
    });
    assert_eq!(callback.drain(&mut consumer), 2); // Real mode and component-mask messages.
    crate::audio_engine::resident_relocation::reconcile(&engine).unwrap();
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
assert controller.set_stem_mix_mode(0, 'full_mix')
assert 0 not in controller._resident_pairs
assert project.stem_cache[0].pair == selection
assert project.pad_stem_mix_mode[0] == 'full_mix'
"#,
        )
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    let mut output = [0.0_f32; 512];
    let mut peaks = [0.0_f32; crate::audio_engine::constants::NUM_SAMPLES];
    callback
        .mixer
        .render_rt(&mut output, &mut peaks, &mut callback.retirement);
    callback.retirement.retired.clear();
    assert!(
        first_weak.iter().all(|pcm| pcm.strong_count() == 0),
        "actual control, bank, queue and worker owners ended"
    );
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
persistence.flush()
full_mix_reopen = ProjectPersistence.from_config_path(persistence.config_path)
assert full_mix_reopen.project.stem_cache[0].pair == selection
assert full_mix_reopen.project.pad_stem_mix_mode[0] == 'full_mix'
assert controller.set_stem_mix_mode(0, 'all_stems')
"#,
        )
    });
    wait_until(|| {
        Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                "controller.on_frame_render()\nassert not controller._session.stem_generation_errors, controller._session.stem_generation_errors",
            );
            locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .getattr("_pending_stem_publications")
                .unwrap()
                .contains(0)
                .unwrap()
        })
    });
    let second_set = queued_stems(&mut consumer);
    for index in 0..4 {
        assert!(!first_weak[index].ptr_eq(&Arc::downgrade(&second_set.stems[index].samples)));
    }
    drop(second_set);
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
second_ticket = controller._pending_stem_publications[0].source_ticket
assert second_ticket is not first_ticket
assert second_ticket.publication_status() == 'pending'
assert not controller.stems_available(0)
assert project.stem_cache[0].pair == selection
assert audio.worker_call_count() == 2 and audio.prepared_only_on_pool_threads()
"#,
        )
    });
    assert_eq!(callback.drain(&mut consumer), 1);
    Python::attach(|py| {
        worker_python(
            py,
            locals.bind(py),
            r#"
assert second_ticket.publication_status() == 'accepted'
controller.on_frame_render()
assert controller.stems_available(0) and 0 in controller._resident_pairs
persistence.flush()
final_reopen = ProjectPersistence.from_config_path(persistence.config_path)
assert final_reopen.load_error is None
assert final_reopen.project.stem_cache[0].pair == selection
assert final_reopen.project.pad_content[0] == project.pad_content[0]
Path('worker-child-verified.json').write_text(json.dumps(dict(process_id=__import__('os').getpid(),
    worker_calls=audio.worker_call_count(), only_pool_threads=audio.prepared_only_on_pool_threads(),
    pending_before_callback=True, accepted_after_callback=True, full_mix_components_released=True,
    current_json_reopened=True, selection=selection.model_dump())), encoding='utf-8')
controller.shut_down()
assets.release_saved_assignments()
"#,
        )
    });
    assert_eq!(callback.drain(&mut consumer), 2);
}

#[test]
fn accepted_pair_reprepares_finite_window_and_relocation_with_own_callback_acks() {
    use crate::audio_engine::resident_relocation::{
        WindowRequest, prepare_window_with_producer, reconcile, relocate_with_producer,
    };

    let mut harness = Harness::new();
    let source_ticket = harness.ticket(0);
    let pair = harness.prepare(0, &source_ticket, true, None);
    let selected = selection(&pair);
    let _saved = harness.save(&pair);
    harness
        .engine
        .publish_stem_pair_with_producer(&pair, &source_ticket, &harness.producer)
        .unwrap();
    let initial_set = queued_stems(&mut harness.consumer);
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(source_ticket.publication_status(), "accepted");
    reconcile(&harness.engine).unwrap();
    let frames = initial_set.frame_count;
    assert!(frames > 3);
    let first = prepare_window_with_producer(
        &harness.engine,
        0,
        WindowRequest {
            loop_region: Some((
                1.0 / f64::from(RATE),
                Some((frames - 1) as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        harness.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        harness.consumer.peek().is_ok()
            || matches!(first.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        first.publication_status(),
        "pending",
        "{:?}",
        first.error().unwrap()
    );
    let finite = match harness.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert_eq!(transaction.sample.resident_start(), 1);
            assert_eq!(transaction.sample.resident_end(), frames - 1);
            let stems = transaction.stems.as_ref().unwrap();
            assert!(Arc::ptr_eq(
                &stems.complete_set_identity,
                &initial_set.complete_set_identity
            ));
            assert_eq!(stems.stems.len(), 4);
            assert!(
                stems
                    .stems
                    .iter()
                    .all(|stem| stem.same_window(&transaction.sample))
            );
            assert!(Arc::ptr_eq(
                &stems.reference_samples,
                &transaction.sample.samples
            ));
            stems.clone()
        }
        _ => panic!("expected actual paired finite-window command"),
    };
    let observed = first.read_observation_for_test().unwrap();
    let bytes = (frames - 2) * initial_set.channels * 4;
    assert_eq!(observed.source.read_bytes, bytes as u64);
    assert_eq!(observed.source.allocated_bytes, bytes);
    assert_eq!(
        (observed.source.start_frame, observed.source.end_frame),
        (1, frames - 1)
    );
    assert_eq!(
        observed.stems.read_bytes,
        [bytes as u64, bytes as u64, bytes as u64, bytes as u64, 0]
    );
    assert_eq!(observed.stems.allocated_bytes, bytes * 4);
    assert_eq!(observed.stems.fresh_opens, 0);
    assert_eq!(observed.stems.integrity_bytes, 0);
    let begin = initial_set.channels * 4;
    for range in &observed.stems.byte_ranges[..4] {
        assert_eq!(*range, Some((begin, begin + bytes)));
    }
    assert!(observed.admitted_peak_bytes < crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES);
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(first.publication_status(), "accepted");
    assert!(first.is_current());
    reconcile(&harness.engine).unwrap();
    let second = relocate_with_producer(
        &harness.engine,
        0,
        0.0,
        frames as f64 / f64::from(RATE),
        harness.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        harness.consumer.peek().is_ok()
            || matches!(second.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        second.publication_status(),
        "pending",
        "{:?}",
        second.error().unwrap()
    );
    match harness.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert_eq!(transaction.sample.resident_start(), 0);
            assert_eq!(transaction.sample.resident_end(), frames);
            let stems = transaction.stems.as_ref().unwrap();
            assert!(Arc::ptr_eq(
                &stems.complete_set_identity,
                &finite.complete_set_identity
            ));
            assert_eq!(stems.stems.len(), 4);
            assert!(
                stems
                    .stems
                    .iter()
                    .all(|stem| stem.same_window(&transaction.sample))
            );
            assert!(Arc::ptr_eq(
                &stems.reference_samples,
                &transaction.sample.samples
            ));
        }
        _ => panic!("expected actual paired storage relocation"),
    }
    assert_eq!(harness.callback.drain(&mut harness.consumer), 1);
    assert_eq!(second.publication_status(), "accepted");
    assert!(second.is_current());
    assert!(!first.is_current());
    reconcile(&harness.engine).unwrap();
    assert_complete_selection(&harness.root, &selected);
}
