//! Productive Python recovery across a real process boundary, with native ACKs.
//! The facade routes the project root and directly invokes real native preparation
//! for the device-free harness. Public background-preparation admission and complete
//! physical garbage collection are covered separately, not certified by this test.
//! Pending and claimed parent sources use the actual callback CAS. Historical
//! acknowledged-error text remains journal policy, without a parent ACK claim.
use super::*;
use pyo3::types::{PyDict, PyDictMethods};

const RESUME_ROOT: &str = "FLITZI_MATERIAL_MIGRATION_RESUME_ROOT";
const RESUME_CHILD: &str = "audio_engine::cold_load::tests::material_migration_control_tests::material_migration_resume_tests::material_migration_resume_new_process_child";
const PARENT_TRANSACTION: &str = "88888888888888888888888888888888";
const CLAIM_FAULT: &str = "migration resume test stops after callback source claim";

/// Fault the existing retirement seam after the real callback's claim CAS but
/// before its adoption ACK. Other retirement methods retain their normal harness
/// behavior. No source phase is injected and no productive callback is changed.
struct ClaimRetirement<'a> {
    delegate: &'a mut Retirement,
    expected_pcm: std::sync::Weak<[f32]>,
    hits: usize,
}

impl AudioBufferRetirement for ClaimRetirement<'_> {
    fn available_retirement_slots(&mut self) -> usize {
        self.delegate.available_retirement_slots()
    }
    fn retire_sample(&mut self, sample: SampleBuffer) {
        assert!(
            self.expected_pcm.ptr_eq(&Arc::downgrade(&sample.samples)),
            "claim fault must retire a genuinely loaded previous bank source"
        );
        self.delegate.retire_sample(sample);
        self.hits += 1;
        panic!("{CLAIM_FAULT}");
    }
    fn retire_cold_adoption(&mut self, value: Arc<std::sync::atomic::AtomicU8>) {
        self.delegate.retire_cold_adoption(value);
    }
    fn retire_resident_capture(
        &mut self,
        value: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        self.delegate.retire_resident_capture(value);
    }
    fn retire_resident_cancellation(&mut self, value: Arc<std::sync::atomic::AtomicBool>) {
        self.delegate.retire_resident_cancellation(value);
    }
    fn retire_resident_transaction(&mut self, value: Box<crate::messages::ResidentTransaction>) {
        self.delegate.retire_resident_transaction(value);
    }
    fn retire_prepared_stems(&mut self, value: crate::messages::PreparedStemSet) {
        self.delegate.retire_prepared_stems(value);
    }
    fn retire_constant_timing(
        &mut self,
        value: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
        self.delegate.retire_constant_timing(value);
    }
    fn retire_global_playback_batch(
        &mut self,
        value: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.delegate.retire_global_playback_batch(value);
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        value: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.delegate.retire_accepted_timing_refresh(value);
    }
}

#[pyclass]
struct ResumeAudio {
    engine: Arc<AudioEngine>,
    samples_root: PathBuf,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
}

impl ResumeAudio {
    fn original(&self, reference: &str) -> PyResult<PathBuf> {
        crate::audio_engine::material_paths::resolve(&self.samples_root, Path::new(reference))
            .map(|asset| asset.path)
            .map_err(|error| pyo3::exceptions::PyValueError::new_err(error.to_string()))
    }
}

#[pymethods]
impl ResumeAudio {
    fn migration_artifact_ledger_supported(&self) -> bool {
        true
    }

    fn hold_material_migration(&self, ids: Vec<usize>) -> PyResult<control::MaterialMigrationHold> {
        control::hold_for_producer(&self.engine, ids, &self.producer)
    }

    fn prepare_material_migration(
        &self,
        py: Python<'_>,
        source: String,
    ) -> PyResult<control::MaterialMigrationPreparation> {
        let source = self.original(&source)?;
        py.detach(|| {
            let material = material_migration::prepare_material(
                &self.samples_root,
                &source,
                8_000,
                1,
                &|| false,
            )
            .map_err(pyo3::exceptions::PyValueError::new_err)?;
            control::prepared_for_test(&self.engine, material, &self.samples_root)
        })
    }

    fn adopt_material_migration(
        &self,
        id: usize,
        preparation: &control::MaterialMigrationPreparation,
    ) -> PyResult<control::MaterialMigrationSourceTicket> {
        control::adopt_for_format(
            &self.engine,
            id,
            preparation,
            self.producer.clone(),
            (1, 8_000, self.samples_root.clone()),
        )
    }

    fn acquire_project_asset_lease(
        &self,
        py: Python<'_>,
        path: String,
    ) -> PyResult<crate::audio_engine::project_assets::ProjectAssetLease> {
        let path = self.original(&path)?;
        py.detach(|| {
            self.engine
                .project_assets
                .acquire(&self.samples_root, &path)
        })
        .map_err(|error| pyo3::exceptions::PyValueError::new_err(error.to_string()))
    }

    fn pad_timing_intent(&self, id: usize) -> PyResult<&'static str> {
        self.engine.pad_timing_intent(id)
    }

    fn capture_saved_constant_timing(
        &self,
        id: usize,
        json: String,
        source_path: String,
    ) -> PyResult<constant_timing::SavedConstantTimingTicket> {
        let path = self.original(&source_path)?;
        constant_timing::capture_saved(&self.engine, id, &json, path.to_string_lossy().into())
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    fn restore_constant_timing(
        &self,
        py: Python<'_>,
        saved: &constant_timing::SavedConstantTimingTicket,
    ) -> PyResult<constant_timing::ConstantTimingTicket> {
        py.detach(|| constant_timing::restore_saved(&self.engine, &self.producer, saved))
    }

    fn current_constant_timing(&self, py: Python<'_>, id: usize) -> PyResult<Option<Py<PyAny>>> {
        constant_timing::current_metadata(&self.engine, py, id)
    }

    fn export_current_constant_timing(
        &self,
        py: Python<'_>,
        id: usize,
        source_path: String,
    ) -> PyResult<Option<String>> {
        let path = self.original(&source_path)?;
        py.detach(|| {
            constant_timing::export_current(&self.engine, id, path.to_string_lossy().into())
        })
        .map_err(pyo3::exceptions::PyValueError::new_err)
    }
}

fn resume_python(py: Python<'_>, locals: &Bound<'_, PyDict>, code: &str) {
    py.run(&std::ffi::CString::new(code).unwrap(), Some(locals), None)
        .unwrap_or_else(|error| panic!("migration script failed: {error:?}\n{code}"));
}

fn pending_project(
    engine: Arc<AudioEngine>,
    root: &Path,
    material: &serde_json::Value,
    history: &str,
) -> String {
    Python::attach(|py| {
        let locals = migration_python_locals(py, root);
        locals
            .set_item("material_json", material.to_string())
            .unwrap();
        locals.set_item("history", history).unwrap();
        locals
            .set_item("parent_transaction", PARENT_TRANSACTION)
            .unwrap();
        locals
            .set_item(
                "native_owner",
                Py::new(
                    py,
                    MigrationNativeSaveOwner {
                        engine,
                        samples_root: root.into(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        resume_python(
            py,
            &locals,
            r#"
import sys, json, hashlib
from pathlib import Path
sys.path.insert(0, source_modules)
from flitzis_looper.models import ProjectState, PadContentIdentity
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion, MAX_KEY_EPOCH
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.material_migration_model import (
    MaterialMigrationAlias, MaterialMigrationJournal, MigrationAssignment,
    MigrationArtifactEvidence, MigrationArtifactRecord,
)
from flitzis_looper_audio import MigrationArtifactLease
material = json.loads(material_json)
digest = material['original']['sha256']
alias = MaterialMigrationAlias(
    transaction_id=parent_transaction, material_id=material['material_id'],
    old_reference=material['old_reference'], new_reference=material['new_reference'],
    original_sha256=digest, original_bytes=material['original']['bytes'],
    decoder_identity=material['decoder_identity'], playback_identity=material['playback_identity'],
    cache_path=material['cache_path'],
    old_source_version=f"{material['old_reference']}|sha256-v1:{digest}",
    new_source_version=f"{material['new_reference']}|sha256-v1:{digest}",
)
project = ProjectState(config_revision=17, selected_bank=5, selected_pad=215,
    multi_loop=True, key_lock=True, demucs_shifts=2, demucs_overlap=0.5)
for sample_id, uuid, raw, correction, base, extra, retrigger in (
    (0, '1'*32, 'Em', 'Cm', -4, 12, True),
    (215, 'f'*32, 'Bb', 'unknown retained correction', 6, -18, False),
):
    project.sample_paths[sample_id] = alias.new_reference if sample_id == 0 else alias.old_reference
    project.pad_content[sample_id] = PadContentIdentity(
        instance_id=uuid, material_id=alias.material_id if sample_id == 0 else None)
    project.pad_key_intent[sample_id] = PadKeyIntent(
        source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key=raw), correction=correction,
        analysis_epoch=MAX_KEY_EPOCH, correction_epoch=MAX_KEY_EPOCH,
        base_shift=base, extra_shift=extra, retrigger=retrigger)
    project.pad_gain_db[sample_id] = -7.0 if sample_id == 0 else -13.0
    project.pad_loop_start_s[sample_id] = 0.125
    project.pad_loop_end_s[sample_id] = 1.0
project.pad_timing_intent[0] = 'automatic'
persistence = ProjectPersistence(project)
persistence.config_path = Path(project_root) / 'samples/flitzis_looper.config.json'
persistence.bind_audio(native_owner)
persistence.flush()
saved_before = ProjectState.model_validate_json(persistence.config_path.read_bytes())
assert saved_before.sample_analysis[0].accepted_timing is not None
# A fresh application receives these actual exported records from disk. Keep
# that same validated evidence in the interrupted snapshot, without a fake ACK.
project.sample_analysis = saved_before.model_copy(deep=True).sample_analysis
revision, snapshot, config_digest = persistence.capture_migration(parent_transaction)
assert revision == 17 and config_digest == hashlib.sha256(persistence.config_path.read_bytes()).hexdigest()
# Unsaved performer edits are part of interrupted current intent. MAX key epochs
# cannot be advanced by an analysis/correction shortcut during recovery.
project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=2)
project.pad_gain_db[0] = -11.0
persistence.mark_dirty()
artifacts = []
for role, reference in (
    ('rollback', alias.old_reference), ('target', alias.new_reference), ('target', alias.cache_path),
):
    lease = MigrationArtifactLease.capture(str(Path(project_root)/'samples'), reference)
    artifacts.append(MigrationArtifactRecord(role=role, created=False,
        evidence=MigrationArtifactEvidence.model_validate_json(lease.receipt_json())))
    lease.release()
journal = MaterialMigrationJournal(
    transaction_id=parent_transaction,
    phase='adoption_pending' if history == 'pending' else 'unresolved',
    error=(None if history == 'pending' else
        'actual callback claim interrupted before config outcome' if history == 'claimed' else
        f'historical {history} interrupted before config outcome'),
    config_reference=persistence.config_reference, captured_revision=revision,
    intent_revision=persistence.revision, config_sha256=config_digest,
    snapshot_json=project.model_dump_json(), alias=alias, artifacts=tuple(artifacts),
    assignments=tuple(MigrationAssignment(sample_id=i, instance_id=project.pad_content[i].instance_id,
        old_reference=project.sample_paths[i]) for i in (0,215)),
)
assert MaterialMigrationJournal.model_validate_json(journal.model_dump_json()) == journal
(Path(project_root)/'expected-intent.json').write_text(project.model_dump_json(), encoding='utf-8')
persistence.release_migration(parent_transaction)
journal_json = journal.model_dump_json()
"#,
        );
        locals
            .get_item("journal_json")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    })
}

#[test]
fn material_migration_resume_pending_journal_in_new_process_requires_fresh_source_and_timing_ack() {
    Python::initialize();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let scratch = workspace.join("scratch/goal-p2b-material-reconciliation-20261010");
    fs::create_dir_all(&scratch).unwrap();
    let (fixture_original, envelope) = constant_timing::migration_test_fixture();
    let original_bytes = fs::read(&fixture_original).unwrap();
    // Pending and claimed are actual parent ticket phases. The final case proves
    // acknowledged-error history policy without claiming an actual parent ACK.
    for history in ["pending", "claimed", "acknowledged-error"] {
        let directory = tempfile::tempdir_in(&scratch).unwrap();
        let root = directory.path().join("samples");
        fs::create_dir(&root).unwrap();
        let legacy = root.join("old.wav");
        fs::write(&legacy, &original_bytes).unwrap();
        fs::write(directory.path().join("accepted-timing.json"), &envelope).unwrap();
        let record;
        let parent_loading;
        let parent_material;
        let parent_pcm;
        let parent_leases;
        let parent_engine;
        let parent_file_readers;
        {
            let engine = Arc::new(AudioEngine::new().unwrap());
            parent_engine = Arc::downgrade(&engine);
            parent_loading = engine.cold_loading.clone();
            parent_leases = Arc::downgrade(&engine.cold_leases);
            let material =
                material_migration::prepare_material(&root, &legacy, 8_000, 1, &|| false).unwrap();
            let metadata = material.metadata();
            let preparation = control::prepared_for_test(&engine, material, &root).unwrap();
            {
                let material = preparation.material().unwrap();
                parent_material = Arc::downgrade(&material);
                parent_pcm = Arc::downgrade(&material.sample.samples);
                parent_file_readers = material.lease.reader_lifetime_probe_for_test();
            }
            let (producer, mut consumer) = rtrb::RingBuffer::new(16);
            let producer = Arc::new(Mutex::new(producer));
            let mut callback = migration_callback_8000(&engine);
            let sources = [0, 215].map(|id| {
                control::adopt_for_format(
                    &engine,
                    id,
                    &preparation,
                    producer.clone(),
                    (1, 8_000, root.clone()),
                )
                .unwrap()
            });
            wait_until(|| consumer.slots() >= 2);
            assert_eq!(callback.drain(&mut consumer), 2);
            assert!(
                terminals(
                    &engine,
                    &sources
                        .iter()
                        .map(|source| source.request_id())
                        .collect::<Vec<_>>()
                )
                .iter()
                .all(|event| matches!(event, LoaderEvent::Success { .. }))
            );
            wait_until(|| {
                [0, 215]
                    .into_iter()
                    .all(|id| engine.cold_loading[id].load(Ordering::Acquire) == 0)
            });
            let original = crate::audio_engine::material_paths::resolve(
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
                sources[0].request_id(),
            );
            record = pending_project(engine.clone(), &root, &metadata, history);
            let expected_pcm = Arc::downgrade(
                &engine.sample_cache.lock().unwrap()[0]
                    .as_ref()
                    .unwrap()
                    .samples,
            );
            // Begin with genuinely admitted, undrained parent work. The claimed
            // branch below then interrupts the actual callback before its ACK.
            // All these runtime owners/tickets must end before child spawn.
            let pending = [0, 215].map(|id| {
                control::adopt_for_format(
                    &engine,
                    id,
                    &preparation,
                    producer.clone(),
                    (1, 8_000, root.clone()),
                )
                .unwrap()
            });
            wait_until(|| consumer.slots() >= 2);
            assert!(
                pending
                    .iter()
                    .all(|ticket| ticket.phase().unwrap() == "pending")
            );
            assert!(pending.iter().all(|ticket| !ticket.is_current().unwrap()));
            if history == "claimed" {
                // Both old banks were installed by the earlier real source ACKs.
                // Their shared immutable PCM is a valid previous-bank identity,
                // regardless of which of the two workers enqueued first.
                {
                    let mut retirement = ClaimRetirement {
                        delegate: &mut callback.retirement,
                        expected_pcm,
                        hits: 0,
                    };
                    let fault = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        drain_control_messages(
                            &mut consumer,
                            &mut callback.scheduler,
                            0,
                            &mut callback.quantization,
                            &mut callback.transport,
                            &mut callback.mixer,
                            &mut callback.feedback,
                            &mut retirement,
                        )
                    }))
                    .expect_err("existing retirement seam must interrupt the callback after claim");
                    let message = fault
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| fault.downcast_ref::<&str>().copied());
                    assert_eq!(message, Some(CLAIM_FAULT));
                    assert_eq!(retirement.hits, 1);
                }
                assert_eq!(consumer.slots(), 1);
                assert_eq!(
                    pending
                        .iter()
                        .filter(|ticket| ticket.phase().unwrap() == "claimed")
                        .count(),
                    1
                );
                assert_eq!(
                    pending
                        .iter()
                        .filter(|ticket| ticket.phase().unwrap() == "pending")
                        .count(),
                    1
                );
                let claimed = pending
                    .iter()
                    .find(|ticket| ticket.phase().unwrap() == "claimed")
                    .unwrap();
                assert!(!claimed.is_current().unwrap());
                assert!(!claimed.cancel_unclaimed());
                // Cancellation ends the asynchronous producers; it does not
                // turn this actual partial callback claim into a rejected ACK.
                engine.cold_cancelled.store(true, Ordering::Release);
                wait_until(|| {
                    [0, 215]
                        .into_iter()
                        .all(|id| engine.cold_loading[id].load(Ordering::Acquire) == 0)
                });
                assert_eq!(claimed.phase().unwrap(), "claimed");
            }
            let store = control::MaterialMigrationJournalStore::new(
                root.to_string_lossy().into(),
                PARENT_TRANSACTION.into(),
            )
            .unwrap();
            store.append(record.clone()).unwrap();
        }
        // AudioEngine::drop joins its ColdJobs. Observe the actual workers' loading
        // guard and both prepared/PCM ownership endpoints instead of inferring their
        // completion from the lexical scope. Weak references retain no authority.
        wait_until(|| {
            [0, 215]
                .into_iter()
                .all(|id| parent_loading[id].load(Ordering::Acquire) == 0)
                && parent_engine.strong_count() == 0
                && parent_material.strong_count() == 0
                && parent_pcm.strong_count() == 0
                && parent_leases.strong_count() == 0
                && parent_file_readers()
        });
        let config_before = fs::read(root.join("flitzis_looper.config.json")).unwrap();
        let output_path = directory.path().join("resume-child-output.txt");
        let output = fs::File::create(&output_path).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", RESUME_CHILD, "--nocapture"])
            .env(RESUME_ROOT, directory.path())
            .stdout(std::process::Stdio::from(output.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(output))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!(
                    "owned resume child timed out: {}",
                    fs::read_to_string(&output_path).unwrap()
                );
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let output = fs::read_to_string(output_path).unwrap();
        assert!(
            status.success(),
            "resume ({history}) child failed: {output}"
        );
        assert!(
            output.contains("1 passed"),
            "exact resume child not run: {output}"
        );
        let receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(directory.path().join("resume-verified.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["process_id"], child.id());
        assert_ne!(receipt["process_id"], std::process::id());
        assert_eq!(receipt["source_callback_verified"], true);
        assert_eq!(receipt["timing_callback_verified"], true);
        assert_eq!(receipt["productive_resume_verified"], true);
        assert_ne!(
            fs::read(root.join("flitzis_looper.config.json")).unwrap(),
            config_before
        );
        assert_eq!(
            fs::read(directory.path().join("accepted-timing.json")).unwrap(),
            envelope.as_bytes()
        );
    }
}

#[test]
fn material_migration_resume_new_process_child() {
    let Some(directory) = std::env::var_os(RESUME_ROOT).map(PathBuf::from) else {
        return;
    };
    // This exact child owns its entire process. Never change the parent harness
    // cwd, where unrelated Rust tests may be running concurrently.
    std::env::set_current_dir(&directory).unwrap();
    Python::initialize();
    let root = directory.join("samples");
    let engine = Arc::new(AudioEngine::new().unwrap());
    for id in [0, 215] {
        assert!(engine.sample_cache.lock().unwrap()[id].is_none());
        assert!(engine.cold_leases.lock().unwrap()[id].is_none());
        assert_eq!(engine.loaded_source_generations.lock().unwrap()[id], (0, 0));
        assert_eq!(engine.pad_request_ids.lock().unwrap()[id], 0);
        assert_eq!(engine.current_timing_acknowledgements.current_epoch(id), 0);
        assert_eq!(engine.input_runtime_ownership.migration_hold(id), 0);
    }
    let (producer, mut consumer) = rtrb::RingBuffer::new(32);
    let producer = Arc::new(Mutex::new(producer));
    let mut callback = migration_callback_8000(&engine);
    // Normal saved-intent startup through the actual native parameter queue.
    constant_timing::set_intent(&engine, &producer, 0, TimingIntent::Automatic).unwrap();
    assert_eq!(callback.drain(&mut consumer), 1);
    assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
    let locals = Python::attach(|py| {
        let locals = migration_python_locals(py, &root);
        locals
            .set_item("parent_transaction", PARENT_TRANSACTION)
            .unwrap();
        locals
            .set_item(
                "audio",
                Py::new(
                    py,
                    ResumeAudio {
                        engine: engine.clone(),
                        samples_root: root.clone(),
                        producer: producer.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        resume_python(
            py,
            &locals,
            r#"
import sys, json, hashlib
from pathlib import Path
sys.path.insert(0, source_modules)
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.key_intent import MAX_KEY_EPOCH
from flitzis_looper.material_migration_model import MaterialMigrationJournal
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.material_migration import MaterialMigrationController
from flitzis_looper_audio import MaterialMigrationJournalStore
samples = Path(project_root)/'samples'
config = samples/'flitzis_looper.config.json'
config_before = config.read_bytes()
scan = [json.loads(item) for item in MaterialMigrationJournalStore.scan(str(samples))]
assert len(scan) == 1 and scan[0]['error'] is None
parent = MaterialMigrationJournal.model_validate_json(scan[0]['record'])
assert parent.transaction_id == parent_transaction
assert parent.phase in {'adoption_pending','unresolved'}
assert parent.config_sha256 == hashlib.sha256(config_before).hexdigest()
for field in ('request_id','source_ticket','permit','ack','assignment_id'):
    assert field not in json.loads(parent.model_dump_json())
expected = ProjectState.model_validate_json((Path(project_root)/'expected-intent.json').read_bytes())
assert ProjectState.model_validate_json(parent.snapshot_json) == expected
persistence = ProjectPersistence.from_config_path(config)
assert persistence.load_error is None
project = persistence.project
session = SessionState()
assets = ProjectAssetLifecycle(project, audio)
assets.sync_assignments()
loader = LoaderController(project, session, audio, lambda _: None, persistence.mark_dirty)
loader.bind_asset_lifecycle(assets)
persistence.bind_audio(audio)
service = MaterialMigrationController(persistence, session, audio, assets, loader)
assert service._ledger_enabled and service.status == 'unresolved'
assert service._recovery_pending == [parent]
assert persistence._migration_owner is not None
assert service._history_future is None
for i in (0,215):
    assert project.pad_key_intent[i] == expected.pad_key_intent[i]
    assert project.pad_content[i] == expected.pad_content[i]
assert project.pad_key_intent[0].extra_shift == 2
assert project.pad_gain_db[0] == -11.0
assert project.sample_paths[0] == parent.alias.new_reference
assert project.sample_paths[215] == parent.alias.old_reference
# A further current performer edit occurs while real verification is outstanding.
project.pad_key_intent[215] = project.pad_key_intent[215].changed(extra_shift=-12)
project.pad_gain_db[215] = -8.0
persistence.mark_dirty()
expected_current = project.model_copy(deep=True)
"#,
        );
        locals.unbind()
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut source_gate_checked = false;
    let mut timing_gate_checked = false;
    loop {
        let (sources_ready, timing_ready, settled) = Python::attach(|py| {
            let locals = locals.bind(py);
            resume_python(
                py,
                locals,
                r#"
service.poll()
assert service._recovery_errors == [], service._recovery_errors
assert service.cleanup_error is None, service.cleanup_error
if service._journal is not None:
    assert service._journal.phase not in {'failed','unresolved'}, service.error
sources_ready = len(service._subscribers) == 2 and all(
    item.source is not None for item in service._subscribers.values())
timing_ready = any(item.timing_ticket is not None for item in service._subscribers.values())
settled = (service.status == 'config_committed' and service._history_future is None
    and not service._recovery_pending and persistence._migration_owner is None)
"#,
            );
            let flag = |name| {
                locals
                    .get_item(name)
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            };
            (flag("sources_ready"), flag("timing_ready"), flag("settled"))
        });
        if !source_gate_checked {
            if sources_ready && consumer.slots() >= 2 {
                Python::attach(|py| {
                    resume_python(
                        py,
                        locals.bind(py),
                        r#"
assert service._journal.resume_of == parent.transaction_id
assert service._journal.transaction_id != parent.transaction_id
assert config.read_bytes() == config_before
assert service._journal.phase == 'adoption_pending'
assert set(service._subscribers) == {0,215}
assert len({(s.source.sample_id, s.source.request_id)
    for s in service._subscribers.values()}) == 2
assert all(s.source.request_id > 0 for s in service._subscribers.values())
assert all(s.source.phase() == 'pending' and not s.source.is_current()
    for s in service._subscribers.values())
assert all(s.source.sample_id == i for i,s in service._subscribers.items())
assert audio.current_constant_timing(0) is None
"#,
                    )
                });
                assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
                assert_eq!(callback.drain(&mut consumer), 2);
                source_gate_checked = true;
            }
        } else if !timing_gate_checked {
            if timing_ready && consumer.slots() >= 1 {
                Python::attach(|py| {
                    resume_python(
                        py,
                        locals.bind(py),
                        r#"
assert all(s.source.phase() == 'acknowledged' and s.source.is_current()
    for s in service._subscribers.values())
assert service._subscribers[0].timing_ticket.publication_status() == 'pending'
assert audio.current_constant_timing(0) is None
assert config.read_bytes() == config_before
assert service._journal.phase == 'adoption_pending'
current_source_requests = [(i,s.source.request_id) for i,s in service._subscribers.items()]
"#,
                    )
                });
                let current_sources: Vec<(usize, u64)> = Python::attach(|py| {
                    locals
                        .bind(py)
                        .get_item("current_source_requests")
                        .unwrap()
                        .unwrap()
                        .extract()
                        .unwrap()
                });
                assert_eq!(current_sources.len(), 2);
                for (id, request) in current_sources {
                    assert_eq!(
                        engine.loaded_source_generations.lock().unwrap()[id],
                        (request, 8_000)
                    );
                }
                assert_eq!(engine.current_timing_acknowledgements.current_epoch(0), 0);
                assert_eq!(callback.drain(&mut consumer), 1);
                assert_ne!(engine.current_timing_acknowledgements.current_epoch(0), 0);
                timing_gate_checked = true;
            }
        } else {
            callback.drain(&mut consumer);
        }
        if settled {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "productive migration resume did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(source_gate_checked && timing_gate_checked);
    Python::attach(|py| {
        resume_python(
            py,
            locals.bind(py),
            r#"
saved = ProjectState.model_validate_json(config.read_bytes())
child = service._journal
assert child.phase == 'config_committed' and child.resume_of == parent.transaction_id
assert child.alias.resume_of == parent.transaction_id
assert all(s.source.phase() == 'acknowledged' and s.source.is_current()
    for s in service._subscribers.values())
assert child.committed_config_sha256 == hashlib.sha256(config.read_bytes()).hexdigest()
assert child.committed_revision == saved.config_revision == persistence.revision
assert child.transaction_id != parent.transaction_id
assert saved.material_migrations[child.transaction_id] == child.alias
assert parent.transaction_id not in saved.material_migrations
assert {item.evidence.reference for item in parent.artifacts}.issubset(
    {item.evidence.reference for item in child.artifacts})
assert {item.role for item in child.artifacts} == {'rollback','target'}
assert service._history_future is None
for i, uuid in ((0,'1'*32),(215,'f'*32)):
    assert saved.pad_content[i].instance_id == uuid == expected_current.pad_content[i].instance_id
    assert saved.pad_content[i].material_id == child.alias.material_id
    assert saved.sample_paths[i] == child.alias.new_reference
    assert saved.pad_key_intent[i] == expected_current.pad_key_intent[i]
    assert saved.pad_key_intent[i].source.version == MAX_KEY_EPOCH
    assert saved.pad_key_intent[i].analysis_epoch == saved.pad_key_intent[i].correction_epoch == MAX_KEY_EPOCH
    assert saved.pad_gain_db[i] == expected_current.pad_gain_db[i]
    assert saved.pad_loop_start_s[i] == expected_current.pad_loop_start_s[i]
    assert saved.pad_loop_end_s[i] == expected_current.pad_loop_end_s[i]
assert saved.selected_bank == 5 and saved.selected_pad == 215
assert saved.multi_loop and saved.key_lock and saved.demucs_shifts == 2 and saved.demucs_overlap == 0.5
ticket = service._subscribers[0].timing_ticket
current = audio.current_constant_timing(0)
assert ticket.publication_status() == 'accepted'
assert current['accepted_request_id'] == ticket.metadata()['request_id']
assert current['revision'] == ticket.accepted_metadata()['revision']
assert current['source_generation'] == service._subscribers[0].source.request_id
envelope = json.loads((Path(project_root)/'accepted-timing.json').read_text(encoding='utf-8'))
assert saved.sample_analysis[0].accepted_timing.model_dump() == envelope
assert json.loads(audio.export_current_constant_timing(0, saved.sample_paths[0])) == envelope
scan_after = [json.loads(item) for item in MaterialMigrationJournalStore.scan(str(samples))]
assert all(item['error'] is None for item in scan_after)
current_records = [MaterialMigrationJournal.model_validate_json(item['record']) for item in scan_after]
assert any(item.transaction_id == child.transaction_id and item.phase == 'config_committed'
    and item.resume_of == parent.transaction_id for item in current_records)
import os
# The receipt contains assertions/process identity, never transferable authority.
(Path(project_root)/'resume-verified.json').write_text(json.dumps({
    'process_id': os.getpid(), 'productive_resume_verified': True,
    'source_callback_verified': True, 'timing_callback_verified': True,
}), encoding='utf-8')
service.shut_down()
loader.shut_down()
assets.release_saved_assignments()
service._store = None
"#,
        );
    });
}
