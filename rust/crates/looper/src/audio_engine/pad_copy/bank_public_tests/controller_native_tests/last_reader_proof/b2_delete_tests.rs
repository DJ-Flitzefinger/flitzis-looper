//! B2 warning -> ordinary deletion -> real FullMix callback -> actual last reader.
//! Reuse the existing cold source/Copy/persistence/reader fixture and artifact oracle.
//! Only the stream_handle lookup is adapted to its existing headless producer;
//! no CPAL, App.run, inference, manufactured ACK or hearing acceptance is implied.
use super::*;
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use pyo3::PyResult;

#[pyclass]
struct DeleteFullMixAudio {
    engine: Py<AudioEngine>,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
}

#[pymethods]
impl DeleteFullMixAudio {
    fn set_stem_pair_full_mix(&self, py: Python<'_>, id: usize) -> PyResult<()> {
        resident_relocation::set_stem_pair_full_mix_with_producer(
            &self.engine.borrow(py),
            id,
            &self.producer,
        )
    }
}

// This observer adds no source, worker, selection or cleanup implementation.
// The same real BankControllerProbe owns the actual StemController and Config.
const OBSERVER: &str = r#"
import hashlib
import json
from dataclasses import asdict
from pathlib import Path
from types import SimpleNamespace

from flitzis_looper.control_diagnostics import ConsoleDiagnostics
from flitzis_looper.controller.app import AppController, _copied_components_available
from flitzis_looper.controller.copy_arrangement_journal import (
    CopyJournalOwner,
    validated_copy_history,
    written_copy_image_matches,
)
from flitzis_looper.controller.performance_confirmation import (
    PerformanceConfirmationController,
    StemDeleteConfirmation,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_batch import StemBatchController
from flitzis_looper.controller.stem_delete import StemDeleteController
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    copy_after_snapshot,
    musical_content_data,
    musical_snapshot_projection,
)


def _changed_values(expected, actual):
    changes = []

    def visit(before, after, path):
        if before == after or len(changes) == 12:
            return
        if isinstance(before, dict) and isinstance(after, dict):
            for key in sorted(before.keys() | after.keys()):
                visit(before.get(key, "<missing>"), after.get(key, "<missing>"), f"{path}.{key}")
        elif isinstance(before, list) and isinstance(after, list) and len(before) == len(after):
            for index, (left, right) in enumerate(zip(before, after, strict=True)):
                visit(left, right, f"{path}[{index}]")
        else:
            changes.append({"path": path, "expected": repr(before)[:300], "actual": repr(after)[:300]})

    visit(expected, actual, "$other")
    return changes


class _StreamlessFullMix:
    def __init__(self, native, full_mix):
        self.native = native
        self.full_mix = full_mix

    def __getattr__(self, name):
        owner = self.full_mix if name == "set_stem_pair_full_mix" else self.native
        return getattr(owner, name)


class DeleteProbe:
    def __init__(self, bank, full_mix):
        self.bank = bank
        self.stems = bank.app.stems
        self.project = bank.project
        self.session = bank.session
        self.persistence = bank.persistence
        self.native = bank.audio
        self.stems._audio = _StreamlessFullMix(self.native, full_mix)
        self.stems._on_project_changed = self.persistence.mark_dirty
        self.batch = StemBatchController(self.stems)
        self.stems.bind_delete_logical_interests(self.batch.capture_unadmitted_interests)
        self.delete = StemDeleteController(
            self.stems, withdraw_unadmitted=self.batch.withdraw_unadmitted_interests
        )
        self.envelope = SimpleNamespace(
            project=self.project,
            stems=self.stems,
            stem_batch=self.batch,
            stem_delete=self.delete,
            _close_requested=False,
            banks=bank.app.banks,
            pairs=bank.app.pairs,
            arrangement=bank.app.arrangement,
            diagnostics=ConsoleDiagnostics(),
        )
        self.warning = PerformanceConfirmationController(self.envelope)
        self.target_removed = bank.seed.arrangement._on_target_removed
        bank.seed.arrangement._on_target_removed = self._target_removed
        self.capture = None
        self.old72 = None
        self.before = None
        self.failure = None

    def _target_removed(self, slot, predecessor):
        AppController._on_copy_target_removed(self.bank.app, slot, predecessor)

    def persistence_markers(self):
        return {
            "intent_revision": self.persistence.revision,
            "saved_revision": self.project.config_revision,
            "consent_revision": getattr(self, "intent_revision_at_accept", None),
            "migration_owner": self.persistence._migration_owner,
        }

    def config_diagnostics(self, expected, actual):
        before, after = json.loads(expected), json.loads(actual)
        offset = None if expected == actual else next(
            (index for index, (left, right) in enumerate(zip(expected, actual)) if left != right),
            min(len(expected), len(actual)),
        )
        return {
            "persistence": self.persistence_markers(),
            "raw_equal": expected == actual,
            "expected_sha256": hashlib.sha256(expected).hexdigest(),
            "actual_sha256": hashlib.sha256(actual).hexdigest(),
            "expected_bytes": len(expected),
            "actual_bytes": len(actual),
            "first_raw_difference": offset,
            "changed_keys": [key for key in sorted(before.keys() | after.keys()) if before.get(key) != after.get(key)],
            "changed_values": _changed_values(before, after),
        }

    def adopt_source(self, selection):
        assert selection.publication_status() == "accepted"
        assert selection.is_current()
        self.stems.project_copied_native_state(
            0, components_available=True, selection=selection
        )

    def request_all(self, slot):
        assert self.stems.set_stem_enabled_mask(slot, 15)
        assert self.stems.set_stem_mix_mode(slot, "all_stems")

    def all_ready(self, slot):
        self.stems.on_frame_render()
        assert not self.session.stem_generation_errors, self.diagnostics()
        status = self.stems.stem_residency_status(slot)
        if status is None or status.effective != "effective":
            return False
        selection = self.stems._committed_residency._selections[slot]
        receipt = selection.receipt
        assert selection.ticket.publication_status() == "accepted"
        assert selection.ticket.is_current()
        assert receipt is not None
        assert receipt.is_current()
        assert receipt.residency_status() == "accepted"
        assert receipt.effective_mode_status() == "effective"
        assert selection.receipt_intent == ("all_stems", 15)
        return True

    def capture_warning(self):
        self.warning.request_all_loaded_stem_deletion()
        captured = self.warning.pending
        assert isinstance(captured, StemDeleteConfirmation), self.diagnostics()
        assert len(captured.inventory.items) == 216
        assert all(
            item is None or item.unavailable_reason is None
            for item in captured.inventory.items
        )
        self.capture = captured
        return json.dumps([
            item.pad_id for item in captured.inventory.items if item is not None
        ])

    def hold_reader(self):
        self.bank.hold_reader(36)
        self.bank.assert_reader()
        entry = self.project.stem_cache[36]
        assert entry is not None and entry.pair is not None
        return json.dumps({
            "wav": str(Path(entry.pair.wav_generation).absolute()),
            "stem_pcm": str(Path(entry.pair.pcm_generation).absolute()),
            "descriptor": str(Path(entry.pair.descriptor_reference).absolute()),
            "config": str(self.persistence.config_path.absolute()),
        })

    def accept(self):
        captured = self.capture
        assert captured is not None
        self.before = self.project.model_dump(mode="json", exclude_computed_fields=True)
        self.session_before = self.session.model_dump(mode="python")
        self.intent_revision_at_accept = self.persistence.revision
        self.config_at_accept = self.persistence.config_path.read_bytes()
        self.warning.accept(captured)
        original_rows = self.delete.items
        self.warning.accept(captured)
        assert self.delete.items == original_rows
        assert self.warning.pending is None
        assert self.delete.active
        assert not self.batch.active

    def overwrite72(self):
        # A genuine occupied Copy changes the original content after consent.
        # Its real observer fence also exercises partial/deferred deletion.
        self.old72 = self.project.pad_content[72].instance_id
        self.overwrite_receipt_start = len(self.bank.receipts)
        seed = self.bank.seed
        seed.before = self.persistence.config_path.read_bytes()
        seed.before_callbacks = list(seed.callbacks)
        assert seed.arrangement.copy_overwrite(215, 72), seed.arrangement.error
        seed.arrangement.poll()
        copy = seed.arrangement._copy
        assert copy is not None and copy.ticket is not None
        self.bank._seed_copy = copy
        self.bank.app._sync_arrangement_observers()
        assert self.stems._pair_deferred_sample_ids == frozenset({72})
        assert copy.ticket.publication_status() in {"pending", "adopting"}
        return copy.ticket

    def finish_overwrite72(self):
        # seed_complete() is the existing empty-target fixture oracle. This
        # genuine occupied Copy instead owns a predecessor, journal and receipt.
        seed = self.bank.seed
        copy = self.bank._seed_copy
        assert copy is not None and copy.ticket is not None
        assert (copy.source_id, copy.target_id) == (215, 72)
        assert copy.predecessor is not None
        assert copy.predecessor.projected().source_identity.instance_id == self.old72
        assert copy.ticket.publication_status() == "accepted"
        seed.arrangement.poll()
        assert seed.arrangement.status == "completed", seed.arrangement.error
        assert not seed.arrangement.active
        assert seed.callbacks == [*seed.before_callbacks, 72]
        for field in (
            "worker_done", "published", "acknowledged", "predecessor_retired",
            "config_committed", "model_committed", "assets_published", "callback_done",
            "native_finished", "writer_released",
        ):
            assert getattr(copy, field), field
        assert self.persistence._migration_owner is None
        receipt = copy.receipt
        assert receipt is not None
        assert self.bank.receipts[self.overwrite_receipt_start:] == [receipt]
        assert self.bank.receipt_owners[self.overwrite_receipt_start:] == [copy.owner]
        assert copy.initial_journal is not None
        assert copy.initial_journal.captured_revision == self.intent_revision_at_accept
        # The two changed deletion rows each mark one new intent. Occupied Copy
        # commits that current intent revision; its receipt creates no extra one.
        assert receipt.revision == self.intent_revision_at_accept + 2
        assert copy.committed_revision == receipt.revision == self.persistence.revision
        assert self.project.config_revision == receipt.revision
        written = self.persistence.config_path.read_bytes()
        assert written == receipt.written_text.encode("utf-8"), self.config_diagnostics(
            receipt.written_text.encode("utf-8"), written
        )
        assert hashlib.sha256(written).hexdigest() == receipt.sha256
        assert json.loads(written)["config_revision"] == receipt.revision
        self.overwrite_config_bytes = written
        # Copy's sealed durable image deliberately claims no target availability.
        # The real own ACK projects availability only after that receipt is written.
        saved_image = json.loads(written)
        assert saved_image["stem_cache"][72]["available"] is False
        components = _copied_components_available(72, copy.ticket)
        assert self.project.stem_cache[72].available is components
        saved_image["stem_cache"][72]["available"] = components
        self.expected_refresh_bytes = (
            json.dumps(saved_image, indent=2, ensure_ascii=False) + "\n"
        ).encode("utf-8")
        after = copy_after_snapshot(copy.snapshot, copy.identity)
        assert musical_content_data(self.project, 72) == musical_snapshot_projection(
            after.projected(), mode="json"
        )
        assert written_copy_image_matches(receipt.projected(), after, 72)
        assert copy.journal is not None
        reopened = CopyJournalOwner.open(Path.cwd() / "samples", copy.owner)
        assert reopened.encoded == copy.journal.encoded
        records = validated_copy_history(reopened.encoded, copy.owner)
        assert {record.phase for record in records} == {
            "captured", "publication_pending", "native_ack_observed",
            "config_committed", "settled",
        }
        assert records[-1].phase == "settled"
        assert records[-1].committed_revision == receipt.revision
        assert records[-1].committed_config_sha256 == receipt.sha256
        binding = self.native.current_input_runtime_pad_binding(72)
        assert binding is not None and binding.is_current()
        selection = self.bank._seed_registration
        self.bank._assert_selection(72, selection)
        assert self.stems._committed_residency._selections[72].ticket is selection
        self.bank._assert_assignments()
        self.bank._seed_copy = None
        self.bank._seed_registration = None
        self.bank.app._sync_arrangement_observers()
        assert not self.stems._pair_deferred_sample_ids
        assert self.project.pad_content[72].instance_id != self.old72
        assert self.capture.inventory.items[72].content.instance_id == self.old72
        assert self.capture.inventory.items[215] is None
        current = self.project.model_dump(mode="json", exclude_computed_fields=True)
        for _name, field in CONTENT_FIELDS:
            self.before[field][72] = current[field][72]
        self.before["config_revision"] = self.intent_revision_at_accept + 2

    def poll_delete(self):
        self.delete.poll()
        if self.bank.seed.arrangement.active:
            assert self.persistence.revision == self.intent_revision_at_accept + 2
            assert self.project.config_revision == self.before["config_revision"]
            written = self.persistence.config_path.read_bytes()
            assert written == self.config_at_accept, self.config_diagnostics(
                self.config_at_accept, written
            )

    def summary(self):
        summary = self.delete.summary
        assert summary is not None
        return json.dumps(asdict(summary))

    def assert_first_complete(self):
        assert {item.pad_id: item.state for item in self.delete.items} == {
            0: "removed", 36: "removed", 72: "skipped"
        }
        assert not self.delete.active
        assert self.project.stem_cache[0] is None
        assert self.project.stem_cache[36] is None
        assert self.project.stem_cache[72] is not None
        assert self.project.stem_cache[215] is not None
        assert self.project.pad_stem_mix_mode[215] == "all_stems"
        assert self.stems._committed_residency.contains(215)
        assert not self.stems._committed_residency.contains(36)
        assert self.persistence.revision == self.intent_revision_at_accept + 2
        current = self.project.model_dump(mode="json", exclude_computed_fields=True)
        for name, value in self.before.items():
            if name not in {"stem_cache", "pad_stem_mix_mode"}:
                assert current[name] == value, {
                    "field": name, "changes": _changed_values(value, current[name]),
                    "persistence": self.persistence_markers(),
                }
        session = self.session.model_dump(mode="python")
        assert session == self.session_before, {
            "session_changes": _changed_values(self.session_before, session),
            "persistence": self.persistence_markers(),
        }
        self.bank.assert_reader()
        written = self.persistence.config_path.read_bytes()
        assert written == self.overwrite_config_bytes, self.config_diagnostics(
            self.overwrite_config_bytes, written
        )
        self.flush()
        assert self.persistence.revision == self.intent_revision_at_accept + 2, self.persistence_markers()
        refreshed = self.persistence.config_path.read_bytes()
        receipt_difference = self.config_diagnostics(self.overwrite_config_bytes, refreshed)
        print("B2_CONFIG_OWN_ACK_REFRESH " + json.dumps(receipt_difference, sort_keys=True), flush=True)
        assert refreshed == self.expected_refresh_bytes, {
            "receipt_difference": receipt_difference,
            "unexpected_refresh": self.config_diagnostics(self.expected_refresh_bytes, refreshed),
        }
        self.bank.assert_reader()

    def delete_remaining(self):
        self.capture_warning()
        self.accept()
        self.delete.poll()
        assert {item.pad_id: item.state for item in self.delete.items} == {
            0: "noop", 36: "noop", 72: "removed", 215: "removed"
        }
        assert not self.delete.active
        assert all(self.project.stem_cache[slot] is None for slot in (0, 36, 72, 215))
        assert not self.stems._committed_residency._selections
        self.bank.assert_reader()
        self.flush()

    def flush(self):
        self.persistence.flush()
        saved = ProjectPersistence.from_config_path(self.persistence.config_path)
        assert saved.load_error is None, {"load_error": saved.load_error, "persistence": self.persistence_markers()}
        expected = self.project.model_dump(mode="json", exclude_computed_fields=True)
        actual = saved.project.model_dump(mode="json", exclude_computed_fields=True)
        for field in ("stem_cache", "pad_stem_mix_mode", "pad_content", "sample_paths"):
            assert actual[field] == expected[field], {
                "reopened_field": field, "changes": _changed_values(expected[field], actual[field]),
                "persistence": self.persistence_markers(),
            }

    def poll_retirements(self):
        self.stems.on_frame_render()
        self.bank.poll_retirements()

    def assert_reader(self):
        self.bank.assert_reader()

    def release_reader(self):
        self.bank.release_reader()
        assert self.bank.reader is None

    def pending_mask(self):
        assert self.stems.set_stem_enabled_mask(36, 5)
        selection = self.stems._committed_residency._selections[36]
        receipt = selection.receipt
        assert receipt is not None and receipt.is_current()
        assert receipt.residency_status() != "accepted"
        assert selection.receipt_intent == ("all_stems", 5)
        self.flush()
        self.failure = (
            self.project.model_dump(mode="json", exclude_computed_fields=True),
            self.persistence.config_path.read_bytes(),
            self.project.stem_cache[36], selection, receipt, self.stems._demands[36],
            receipt.residency_status(), receipt.effective_mode_status(),
        )

    def assert_refused(self):
        before, config, entry, selection, receipt, demand, residency, effect = self.failure
        assert {item.pad_id: item.state for item in self.delete.items} == {
            0: "failed", 36: "failed"
        }
        assert all("stem full-mix queue is full" in item.detail for item in self.delete.items)
        assert self.project.model_dump(mode="json", exclude_computed_fields=True) == before
        assert self.persistence.config_path.read_bytes() == config
        assert self.project.stem_cache[36] is entry
        assert self.stems._committed_residency._selections[36] is selection
        assert selection.ticket.is_current()
        assert selection.receipt is receipt
        assert receipt.is_current()
        assert receipt.residency_status() == residency
        assert receipt.effective_mode_status() == effect
        assert self.stems._demands[36] is demand
        assert self.project.pad_stem_mix_mode[36] == "all_stems"
        assert self.project.pad_stem_intent[36].enabled_mask == 5
        assert not self.delete.active

    def diagnostics(self):
        return json.dumps({
            "errors": self.session.stem_generation_errors,
            "delete": asdict(self.delete.summary) if self.delete.summary else None,
            "status36": repr(self.stems.stem_residency_status(36)),
            "status215": repr(self.stems.stem_residency_status(215)),
            "cleanup": self.native.project_asset_cleanup_status(),
        })

    def detach(self):
        self.failure = None
        self.bank.seed.arrangement._on_target_removed = self.target_removed
        self.stems.bind_delete_logical_interests(lambda: {})
        self.stems._audio = self.native
"#;

fn observer(fixture: &Fixture, controller: &Py<PyAny>) -> Py<PyAny> {
    Python::attach(|py| {
        let module = PyModule::from_code(
            py,
            &CString::new(OBSERVER).unwrap(),
            c"b2_delete_native_observer.py",
            c"b2_delete_native_observer",
        )
        .unwrap();
        let full_mix = Py::new(
            py,
            DeleteFullMixAudio {
                engine: fixture.engine.clone_ref(py),
                producer: fixture.producer.clone(),
            },
        )
        .unwrap();
        module
            .getattr("DeleteProbe")
            .unwrap()
            .call1((controller.bind(py), full_mix))
            .unwrap()
            .unbind()
    })
}

fn observe_json(observer: &Py<PyAny>, method: &str) -> Value {
    Python::attach(|py| {
        serde_json::from_str(
            &observer
                .call_method0(py, method)
                .unwrap()
                .extract::<String>(py)
                .unwrap(),
        )
        .unwrap()
    })
}

fn adopt_source(fixture: &mut Fixture, observer: &Py<PyAny>) {
    // Project this same PAD's genuinely current fixture registration into the
    // ordinary control owner. Identical registration reuses its current Native
    // SelectionOwner and returns accepted_fixed. ALL adoption below separately
    // requires the real callback's own residency acknowledgement.
    let selection = fixture.with_engine(|engine| {
        let source = engine
            .capture_prepared_source(0, fixture.version.clone())
            .unwrap();
        engine
            .register_committed_stem_pair_with_producer(
                fixture.pair.as_ref().unwrap(),
                &source,
                &fixture.producer,
            )
            .unwrap()
    });
    assert_eq!(selection.publication_status(), "accepted");
    fixture.callback.drain();
    assert_eq!(selection.publication_status(), "accepted");
    Python::attach(|py| {
        observer
            .call_method1(py, "adopt_source", (Py::new(py, selection).unwrap(),))
            .unwrap();
    });
}

fn request_all(fixture: &mut Fixture, observer: &Py<PyAny>, id: usize) {
    fixture.start(id);
    Python::attach(|py| observer.call_method1(py, "request_all", (id,)).unwrap());
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        fixture.callback.drain();
        let pcm = fixture.callback.render(257);
        assert!(pcm.iter().all(|sample| sample.is_finite()));
        let ready = Python::attach(|py| {
            observer
                .call_method1(py, "all_ready", (id,))
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        });
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "PAD{id} lacked its own real ALL receipt: {}",
            observe_json(observer, "diagnostics")
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let demand = fixture.callback.mixer.stem_demand_for_measurement(id);
    assert_eq!((demand.0, demand.1), (StemMixMode::AllStems, 15));
    assert!(fixture.callback.mixer.stems_for_measurement()[id].is_some());
    let pcm = render_fixed(fixture, WARMUP);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 1e-4));
}

fn paths(fixture: &Fixture, held: &Value) -> [PathBuf; 5] {
    let (original, source_pcm) = fixture.with_engine(|engine| {
        let leases = engine.cold_leases.lock().unwrap();
        let source = leases[36].as_ref().unwrap();
        (source.original_path.clone(), source.cache_path.clone())
    });
    [
        original,
        source_pcm,
        PathBuf::from(held["wav"].as_str().unwrap()),
        PathBuf::from(held["stem_pcm"].as_str().unwrap()),
        PathBuf::from(held["descriptor"].as_str().unwrap()),
    ]
}

fn transport(callback: &Callback) -> (u64, u32, Option<u64>, u64, Option<u64>) {
    (
        callback.transport.output_frame(),
        callback.transport.sample_rate_hz(),
        callback.transport.master_period_seconds().map(f64::to_bits),
        callback.transport.downbeat_frame(),
        callback.transport.beat_position().map(f64::to_bits),
    )
}

fn source_image(fixture: &Fixture) -> Vec<(u64, String, usize, u32)> {
    fixture.with_engine(|engine| {
        [0, 36, 72, 215]
            .into_iter()
            .map(|id| engine.waveform_source_identity(id).unwrap().unwrap())
            .collect()
    })
}

fn source_reads(fixture: &Fixture, id: usize) -> SourceFeedSnapshot {
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(
        actual_voice(fixture, id)
            .source_feed_history
            .capture_into(&mut snapshot)
    );
    snapshot
}

fn assert_retained_reads_unchanged(actual: &SourceFeedSnapshot, expected: &SourceFeedSnapshot) {
    assert_eq!(actual.active_origin(), expected.active_origin());
    assert_eq!(actual.active_frames(), expected.active_frames());
    assert_eq!(actual.retained_range(), expected.retained_range());
    assert_eq!(actual.spans().len(), expected.spans().len());
    for (actual, expected) in actual.spans().zip(expected.spans()) {
        assert_eq!(actual.active_start, expected.active_start);
        assert_eq!(actual.frames, expected.frames);
        assert!(actual.playback.matches_exact(&expected.playback));
        assert!(actual.plan.matches_exact(expected.plan));
        assert_eq!(actual.context, expected.context);
    }
}

fn deletion_profile() {
    let mut fixture = bank_fixture_with_stems(true, false, "tap");
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(fixture.directory.path()).unwrap();
    let controller = controller(&mut fixture, false);
    let observer = observer(&fixture, &controller);
    adopt_source(&mut fixture, &observer);
    for id in [36, 72] {
        seed(&mut fixture, &controller, id);
    }
    request_all(&mut fixture, &observer, 36);
    session_effect(&controller, 36, false);
    assert_eq!(
        observe_json(&observer, "capture_warning"),
        json!([0, 36, 72])
    );

    // This independently current shared owner did not exist in the warning.
    seed(&mut fixture, &controller, 215);
    request_all(&mut fixture, &observer, 215);
    session_effect(&controller, 215, false);
    let held = observe_json(&observer, "hold_reader");
    let paths = paths(&fixture, &held);
    assert!(paths.iter().all(|path| path.is_absolute()));
    let artifacts = snapshot_all(&paths);
    let reader = fixture.pair.as_ref().unwrap().reader_lifetime_for_test();
    let owner = fixture.directory.path().parent().unwrap().join("OWNER.md");
    let owner_bytes = fs::read(&owner).unwrap();
    call(&observer, "accept");
    let overwrite: Py<PadCopyTicket> = Python::attach(|py| {
        observer
            .call_method0(py, "overwrite72")
            .unwrap()
            .extract(py)
            .unwrap()
    });
    assert!(matches!(
        fixture.callback.controls.peek().unwrap(),
        ControlMessage::PadCopy(_)
    ));
    let before = [
        proof(&fixture, 0),
        proof(&fixture, 36),
        proof(&fixture, 215),
    ];
    let reads_before = source_reads(&fixture, 36);
    assert!(reads_before.is_current());
    assert!(reads_before.retained_range().is_some());
    assert!(reads_before.active_frames() > reads_before.active_origin());
    assert_eq!(reads_before.captured_epoch(), before[1].feed_revision);
    let clock = transport(&fixture.callback);
    call(&observer, "poll_delete");
    let partial = observe_json(&observer, "summary");
    assert_eq!(partial["queued"], json!(1));
    assert_eq!(partial["removed"], json!(2));
    assert_eq!(partial["active"], json!(true));
    assert_eq!(fixture.callback.controls.slots(), 3);
    assert_eq!(snapshot_all(&paths), artifacts);
    assert_eq!(transport(&fixture.callback), clock);
    assert_eq!(
        fixture.callback.mixer.stem_demand_for_measurement(36).0,
        StemMixMode::AllStems,
        "logical Removed cannot stand in for the unconsumed FullMix callback"
    );
    assert!(reads_before.is_current());
    assert_eq!(fixture.callback.drain(), 3);
    assert_eq!(transport(&fixture.callback), clock);
    // ALL -> FullMix fences only this voice's captured future source plan.
    // Its actual retained reads, source trajectory and productive DSP stay intact.
    // PAD0 was already FullMix, and the independent PAD215 keeps its ALL owner.
    let mut expected = before;
    expected[1].feed_revision = expected[1].feed_revision.checked_add(1).unwrap();
    assert_eq!(
        [
            proof(&fixture, 0),
            proof(&fixture, 36),
            proof(&fixture, 215)
        ],
        expected
    );
    let reads_after = source_reads(&fixture, 36);
    assert!(!reads_before.is_current());
    assert!(reads_after.is_current());
    assert_eq!(
        reads_after.captured_epoch(),
        reads_before.captured_epoch().checked_add(1).unwrap()
    );
    assert_retained_reads_unchanged(&reads_after, &reads_before);
    Python::attach(|py| assert_eq!(overwrite.borrow(py).publication_status(), "accepted"));
    call(&observer, "finish_overwrite72");
    Python::attach(|_| drop(overwrite));
    let sources = source_image(&fixture);
    call(&observer, "poll_delete");
    let complete = observe_json(&observer, "summary");
    assert_eq!(complete["removed"], json!(2));
    assert_eq!(complete["skipped"], json!(1));
    assert_eq!(complete["queued"], json!(0));
    assert_eq!(complete["failed"], json!(0));
    assert_eq!(fixture.callback.drain(), 0);
    assert_eq!(transport(&fixture.callback), clock);
    assert_eq!(source_image(&fixture), sources);
    assert_eq!(
        [
            proof(&fixture, 0),
            proof(&fixture, 36),
            proof(&fixture, 215)
        ],
        expected
    );
    assert!(reads_after.is_current());
    let reads_settled = source_reads(&fixture, 36);
    assert_eq!(reads_settled.captured_epoch(), reads_after.captured_epoch());
    assert_retained_reads_unchanged(&reads_settled, &reads_after);
    drop((reads_before, reads_after, reads_settled));
    call(&observer, "assert_first_complete");
    let pcm = render_fixed(&mut fixture, WARMUP);
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 1e-4));
    for id in [0, 36] {
        assert_eq!(
            fixture.callback.mixer.stem_demand_for_measurement(id).0,
            StemMixMode::FullMix
        );
        assert!(fixture.callback.mixer.stems_for_measurement()[id].is_none());
    }
    assert!(fixture.callback.mixer.stems_for_measurement()[215].is_some());
    assert_eq!(
        fixture.callback.mixer.stem_demand_for_measurement(215).0,
        StemMixMode::AllStems
    );
    assert_eq!(snapshot_all(&paths), artifacts);

    // A second deliberate warning removes the two surviving current owners.
    // Nothing is manually retired: the normal assignment/registry owners decide.
    call(&observer, "delete_remaining");
    fixture.callback.drain();
    render_fixed(&mut fixture, WARMUP);
    fixture.pair = None;
    let saved = PathBuf::from(held["config"].as_str().unwrap());
    let config_bytes = fs::read(&saved).unwrap();
    for _ in 0..8 {
        fixture.callback.drain();
        fixture.callback.render(257);
        call(&observer, "poll_retirements");
        call(&observer, "assert_reader");
        assert_eq!(snapshot_all(&paths), artifacts);
        assert_eq!(fs::read(&saved).unwrap(), config_bytes);
        assert_eq!(fs::read(&owner).unwrap(), owner_bytes);
    }
    call(&observer, "release_reader");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        fixture.callback.drain();
        fixture.callback.render(257);
        call(&observer, "poll_retirements");
        assert_eq!(snapshot(&paths[0]), artifacts[0]);
        assert_eq!(snapshot(&paths[1]), artifacts[1]);
        assert_eq!(fs::read(&saved).unwrap(), config_bytes);
        assert_eq!(fs::read(&owner).unwrap(), owner_bytes);
        if reader.strong_count() == 0 && paths[2..].iter().all(|path| !path.exists()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "B2 real final reader did not release its three stem artifacts: readers={} paths={:?} runtime={}",
            reader.strong_count(),
            paths.each_ref().map(|path| (path.clone(), path.exists())),
            observe_json(&observer, "diagnostics")
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(source_image(&fixture), sources);
    for id in [0, 36, 72, 215] {
        assert!(fixture.callback.mixer.bank_for_measurement()[id].is_some());
        assert!(fixture.callback.mixer.stems_for_measurement()[id].is_none());
    }
    call(&observer, "detach");
    Python::attach(|_| drop(observer));
    close(controller);
    drop(previous);
}

fn refused_full_mix_profile() {
    let mut fixture = bank_fixture_with_stems(true, false, "tap");
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(fixture.directory.path()).unwrap();
    let controller = controller(&mut fixture, false);
    let observer = observer(&fixture, &controller);
    adopt_source(&mut fixture, &observer);
    seed(&mut fixture, &controller, 36);
    request_all(&mut fixture, &observer, 36);
    call(&observer, "pending_mask");
    assert_eq!(observe_json(&observer, "capture_warning"), json!([0, 36]));
    let held = observe_json(&observer, "hold_reader");
    let paths = paths(&fixture, &held);
    let artifacts = snapshot_all(&paths);
    let before = [proof(&fixture, 0), proof(&fixture, 36)];
    let clock = transport(&fixture.callback);
    let intent_epochs = fixture.with_engine(|engine| {
        let owners = engine.resident_stem_cache.lock().unwrap();
        [0, 36].map(|id| owners[id].intent_epoch.load(Ordering::Acquire))
    });
    {
        let mut producer = fixture.producer.lock().unwrap();
        while !producer.is_full() {
            producer.push(ControlMessage::Ping()).unwrap();
        }
    }
    assert_eq!(fixture.callback.controls.slots(), 128);
    call(&observer, "accept");
    call(&observer, "poll_delete");
    call(&observer, "assert_refused");
    assert_eq!(fixture.callback.controls.slots(), 128);
    assert_eq!(transport(&fixture.callback), clock);
    assert_eq!([proof(&fixture, 0), proof(&fixture, 36)], before);
    let demand = fixture.callback.mixer.stem_demand_for_measurement(36);
    assert_eq!(
        (demand.0, demand.1),
        (StemMixMode::AllStems, 15),
        "refusal preserves the effective old mask while the new mask is pending"
    );
    fixture.with_engine(|engine| {
        let owners = engine.resident_stem_cache.lock().unwrap();
        assert_eq!(
            [0, 36].map(|id| owners[id].intent_epoch.load(Ordering::Acquire)),
            intent_epochs
        );
    });
    assert_eq!(snapshot_all(&paths), artifacts);
    call(&observer, "assert_reader");
    fixture.callback.drain();
    call(&observer, "release_reader");
    call(&observer, "detach");
    Python::attach(|_| drop(observer));
    close(controller);
    drop(previous);
}

#[test]
fn actual_b2_delete_preserves_new_shared_owner_stale_content_and_last_reader() {
    isolated(
        "bank_public_tests::controller_native_tests::last_reader_proof::b2_delete_tests::actual_b2_delete_preserves_new_shared_owner_stale_content_and_last_reader",
        deletion_profile,
    );
}

#[test]
fn actual_b2_full_mix_queue_refusal_retains_selection_and_pending_receipt() {
    isolated(
        "bank_public_tests::controller_native_tests::last_reader_proof::b2_delete_tests::actual_b2_full_mix_queue_refusal_retains_selection_and_pending_receipt",
        refused_full_mix_profile,
    );
}
