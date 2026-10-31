"""Queued control proofs with real Native processing-file seals.

Owned checkpoint/config/code bytes declare a non-neural producer. Only source
custody, publication tickets, pair preparation and selection ACKs are substitutes;
actual cross-root Native source authority and PCM proof live in the Rust suite.
"""

import hashlib
import json
import os
import shutil
from contextlib import contextmanager
from dataclasses import replace
from threading import Event, Thread
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import stem_provenance
from flitzis_looper.controller import stems as stems_module
from flitzis_looper.controller.bs_roformer_assets import MODEL_NAME
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import (
    STEM_KINDS,
    STEM_MASK_BASS,
    STEM_MASK_DRUMS,
    STEM_MASK_VOCALS,
    PadContentIdentity,
)
from flitzis_looper.stem_pair_selection import (
    STEM_FINGERPRINT_SCHEMA,
    StemGenerationProvenance,
    StemPairSelection,
)
from flitzis_looper.ui.render import sidebar_left
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import (
    FakeOfflineStemSourceLease,
    FakePreparedSourceTicket,
    FakeProjectAssetLease,
)

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from pathlib import Path
    from typing import Literal, Never

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemGenerationResult,
    )
    from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper.ui.context import UiContext
    from tests.flitzis_looper.conftest import FakeStemGenerationBackend


class _ControllerPairAck:
    """Explicit controller-only complete-pair/selection ACK substitution."""

    def __init__(self, selection: StemPairSelection, *, components: bool = False) -> None:
        self.selection = selection
        self.components = components
        self.selected = 0
        self.discarded = 0

    def selection_json(self) -> str:
        return self.selection.model_dump_json()

    def has_components(self) -> bool:
        return self.components

    def select(self) -> None:
        self.selected += 1

    def discard(self) -> None:
        self.discarded += 1


class _OwnSourceTicket(FakePreparedSourceTicket):
    """Controller-only Native ticket with explicit own pad and source authority."""

    def __init__(self, sample_id: int, source_version: str) -> None:
        super().__init__()
        self.source_request = sample_id + 1
        self.sample_id = sample_id
        self.source_version = source_version
        self.timing_revision = 0

    def same_source_request(self, other: FakePreparedSourceTicket) -> bool:
        return (
            isinstance(other, _OwnSourceTicket)
            and self.source_request == other.source_request
            and self.source_version == other.source_version
        )


def _released(lease: object) -> bool:
    assert isinstance(lease, FakeProjectAssetLease)
    return lease.released


class _QueuedScenario:
    def __init__(
        self,
        controller: AppController,
        audio: Mock,
        backend: FakeStemGenerationBackend,
        tmp: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        self.controller = controller
        self.audio = audio
        self.backend = backend
        self.tmp = tmp
        self.tasks: list[Callable[[], None]] = []
        self.pairs: list[_ControllerPairAck] = []
        self.leases: list[FakeProjectAssetLease] = []
        self.versions: dict[int, str] = {}
        self.models = (tmp / "owned-model-a", tmp / "owned-model-b")
        self.current_model = self.models[0]
        self.allow_components = False
        self._seed_owned_models_and_originals()
        # Capture still constructs actual Native SealedProcessingFiles. These
        # isolated inputs and declared lineage never execute a neural model.
        monkeypatch.setattr(
            stems_module, "separator_model_cache_dir", lambda _separator: self.current_model
        )
        monkeypatch.setattr(
            stem_provenance,
            "processing_paths",
            lambda request: tuple(
                request.model_cache_dir / leaf
                for leaf in ("checkpoint.bin", "config.yaml", "definition.py")
            ),
        )
        monkeypatch.setattr(
            stem_provenance,
            "processing_library_versions",
            lambda _request: (("owned-fixture", "1"),),
        )
        monkeypatch.setattr(
            stem_provenance, "loaded_code_identity", lambda _paths: "declared-controller-fixture"
        )
        monkeypatch.setattr(
            stem_provenance, "processing_source_roots", lambda _request, _guard: (tmp,)
        )
        monkeypatch.setattr(
            stems_module, "processing_source_roots", lambda _request, _guard: (tmp,)
        )
        self._write_outputs = backend.generate
        monkeypatch.setattr(backend, "generate", self.generate_receipt)
        audio.capture_prepared_source.side_effect = self.capture_ticket
        audio.loaded_source_digest = Mock(
            side_effect=lambda sample_id: self.versions[sample_id].rsplit("|sha256-v1:", 1)[1]
        )
        audio.acquire_project_asset_lease.side_effect = self.acquire_lease
        audio.prepare_stem_pair = Mock(side_effect=self.prepare_pair_ack)
        controller.stems._stem_task_runner = self.tasks.append
        controller.stems._pair_preparations._runner = self.tasks.append
        controller._assets.sync_assignments()

    def _seed_owned_models_and_originals(self) -> None:
        for model in self.models:
            model.mkdir()
            for index, leaf in enumerate(("checkpoint.bin", "config.yaml", "definition.py")):
                (model / leaf).write_bytes(
                    f"non-neural {model.name} producer input {index}".encode()
                )
        for slot, leaf in ((0, "First.wav"), (215, "Equal renamed.wav")):
            source = self.tmp / "samples" / leaf / leaf
            source.parent.mkdir(parents=True)
            write_mono_pcm16_wav(source, 44_100)
            digest = hashlib.sha256(source.read_bytes()).hexdigest()
            (source.parent / ".material.json").write_text(
                json.dumps({
                    "schema_version": 1,
                    "sha256": digest,
                    "bytes": source.stat().st_size,
                    "original_leaf": leaf,
                }),
                encoding="utf-8",
            )
            reference = source.relative_to(self.tmp).as_posix()
            self.controller.project.sample_paths[slot] = reference
            self.controller.project.sample_durations[slot] = 128 / 44_100
            self.controller.project.pad_content[slot] = PadContentIdentity(
                instance_id=f"{slot + 1:032x}", material_id=digest[:32]
            )
            self.versions[slot] = f"{reference}|sha256-v1:{digest}"
            self.audio.load_sample_async(slot, reference)

    @staticmethod
    def capture_ticket(sample_id: int, source_version: str) -> _OwnSourceTicket:
        # Rechecks get another object for the same own source request. No origin
        # pad's permit is transferable to the other equal-content pad.
        return _OwnSourceTicket(sample_id, source_version)

    def acquire_lease(self, path: str) -> FakeProjectAssetLease:
        lease = FakeProjectAssetLease(path)
        self.leases.append(lease)
        return lease

    def generate_receipt(
        self, request: StemGenerationRequest, progress: Callable[[float, str], None]
    ) -> StemGenerationResult:
        # Check a real Native file seal while the actual worker is using it.
        with pytest.raises(PermissionError):
            (request.model_cache_dir / "checkpoint.bin").write_bytes(b"rewrite during read")
        result = self._write_outputs(request, progress)
        if request.separator == "bs-roformer:musdb18hq":
            return replace(
                result, backend_name="bs-roformer", model_name=MODEL_NAME, decoder_verified=True
            )
        return replace(result, backend_name="demucs", model_name="htdemucs", encoder_verified=True)

    def prepare_pair_ack(
        self,
        sample_id: int,
        own_version: str,
        cache: str,
        ticket: _OwnSourceTicket,
        components: bool,  # noqa: FBT001 - Native positional callback signature
        _descriptor: str | None,
        artifact_version: str | None,
    ) -> _ControllerPairAck:
        if components:
            assert self.allow_components
        assert own_version == self.versions[sample_id]
        assert ticket.sample_id == sample_id
        assert ticket.source_version == own_version
        source = self.tmp / cache
        marker = json.loads((source / ".complete.json").read_bytes())
        assert marker["source_version"] == (artifact_version or own_version)
        assert marker["fingerprint_schema"] == STEM_FINGERPRINT_SCHEMA
        provenance = StemGenerationProvenance(
            fingerprint_schema=marker["fingerprint_schema"],
            generation_fingerprint=marker["generation_fingerprint"],
            producer_identity=marker["producer_identity"],
        )
        root = own_version.split("|sha256-v1:", 1)[0].rsplit("/", 1)[0]
        identity = hashlib.sha256(
            f"{own_version}|{provenance.generation_fingerprint}|{provenance.producer_identity}".encode()
        ).hexdigest()
        target = self.tmp / root / "stems" / f".ready-{identity[:32]}"
        target.mkdir(parents=True, exist_ok=True)
        for kind in STEM_KINDS:
            if source != target:
                shutil.copyfile(source / f"{kind}.wav", target / f"{kind}.wav")
        marker["source_version"] = own_version
        (target / ".complete.json").write_text(json.dumps(marker), encoding="utf-8")
        pair = _ControllerPairAck(
            StemPairSelection(
                descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{identity[:32]}.json",
                stem_set_identity=identity,
                wav_generation=target.relative_to(self.tmp).as_posix(),
                pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{identity[:32]}",
                provenance=provenance,
            ),
            components=components,
        )
        self.pairs.append(pair)
        return pair

    def poll(self) -> None:
        self.controller.stems.on_frame_render()

    def step(self) -> None:
        assert self.tasks, "expected one explicitly queued worker"
        self.tasks.pop(0)()
        self.poll()

    def pump(self) -> None:
        for _ in range(24):
            if not self.tasks:
                self.poll()
                if not self.tasks:
                    return
            self.step()
        pytest.fail("bounded controller worker queue did not settle")

    def settings_aba(self) -> None:
        before = self.controller.project.stem_processing_revision
        self.controller.settings.set_stem_separator("bs-roformer:musdb18hq")
        self.controller.settings.set_stem_separator("demucs:htdemucs")
        assert self.controller.project.stem_processing_revision >= before + 2

    def selected(self, slot: int = 0) -> StemCacheEntry:
        entry = self.controller.project.stem_cache[slot]
        assert entry is not None
        assert entry.available
        assert entry.pair is not None
        assert entry.pair.provenance is not None
        return entry


@pytest.fixture
def queued(
    controller: AppController,
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> Iterator[_QueuedScenario]:
    scenario = _QueuedScenario(controller, audio_engine_mock, stem_backend, tmp_path, monkeypatch)
    try:
        yield scenario
    finally:
        controller.stems.shut_down()


class _ActiveGenerationBaseline:
    """Snapshot declared controller activity/effective residency; no audio oracle."""

    def __init__(self, queued: _QueuedScenario) -> None:
        project, session = queued.controller.project, queued.controller.session
        self.residents = frozenset(queued.controller.stems._resident_pairs)
        self.activity = (
            frozenset(session.active_sample_ids),
            frozenset(session.loading_sample_ids),
            frozenset(session.analyzing_sample_ids),
        )
        self.controls = (
            tuple(project.pad_stem_mix_mode),
            tuple(intent.enabled_mask for intent in project.pad_stem_intent),
            tuple(intent.last_custom_mask for intent in project.pad_stem_intent),
            tuple(intent.display_mode for intent in project.pad_stem_intent),
        )
        self.other_path = project.sample_paths[215]
        self.other_content = project.pad_content[215]
        self.other_entry = queued.selected(215)
        self.other_owners = {
            key: owner
            for key, owner in queued.controller._assets._assignments.items()
            if key[1] == 215
        }

    def assert_unchanged(self, queued: _QueuedScenario) -> None:
        project, session = queued.controller.project, queued.controller.session
        assert frozenset(queued.controller.stems._resident_pairs) == self.residents
        assert (
            frozenset(session.active_sample_ids),
            frozenset(session.loading_sample_ids),
            frozenset(session.analyzing_sample_ids),
        ) == self.activity
        assert (
            tuple(project.pad_stem_mix_mode),
            tuple(intent.enabled_mask for intent in project.pad_stem_intent),
            tuple(intent.last_custom_mask for intent in project.pad_stem_intent),
            tuple(intent.display_mode for intent in project.pad_stem_intent),
        ) == self.controls
        self.assert_unrelated_unchanged(queued)

    def assert_unrelated_unchanged(self, queued: _QueuedScenario) -> None:
        project, session = queued.controller.project, queued.controller.session
        assert project.sample_paths[215] == self.other_path
        assert project.pad_content[215] is self.other_content
        assert queued.selected(215) is self.other_entry
        assert project.pad_stem_mix_mode[215] == self.controls[0][215]
        assert project.pad_stem_intent[215].enabled_mask == self.controls[1][215]
        assert project.pad_stem_intent[215].last_custom_mask == self.controls[2][215]
        assert project.pad_stem_intent[215].display_mode == self.controls[3][215]
        assert 215 in session.active_sample_ids
        for key, owner in self.other_owners.items():
            assert queued.controller._assets._assignments[key] is owner
            assert not _released(owner[1])


def _seed_controller_selected_pairs(queued: _QueuedScenario, mix_mode: str) -> None:
    """Reach preexisting residency through public requests and declared pair ACKs."""
    stems = queued.controller.stems
    queued.allow_components = True
    for slot in (0, 215):
        assert stems.generate_stems_async(slot)
        queued.pump()
        assert stems.set_stem_mix_mode(slot, "all_stems")
        queued.pump()
        assert slot in stems._resident_pairs
    assert stems.set_stem_enabled_mask(0, STEM_MASK_VOCALS | STEM_MASK_BASS)
    assert stems.set_stem_enabled_mask(215, STEM_MASK_DRUMS)
    if mix_mode == "full_mix":
        assert stems.set_stem_mix_mode(0, "full_mix")
        assert 0 not in stems._resident_pairs
    queued.allow_components = False


def _assert_active_generation_fresh_source(
    queued: _QueuedScenario,
    source: FakeOfflineStemSourceLease,
    earlier: tuple[_OwnSourceTicket, ...],
) -> None:
    pending = queued.controller.stems._pair_preparations.pending[0]
    assert isinstance(pending.ticket, _OwnSourceTicket)
    assert pending.ticket.sample_id == 0
    assert pending.ticket.source_version == queued.versions[0]
    assert all(pending.ticket is not old for old in earlier)
    assert pending.generation_intent is not None
    assert pending.own_source_version == source.source_version
    queued.audio.capture_prepared_stem_source.assert_called_once_with(0, source)
    assert queued.audio.capture_prepared_stem_source.call_args.args[1] is source
    assert len(source.verification_threads) == 2


def _assert_active_generated_disk_selection(
    queued: _QueuedScenario, previous: StemCacheEntry, initial_requests: int
) -> None:
    selected = queued.selected()
    assert previous.pair is not None
    assert selected.pair is not None
    assert selected.pair.stem_set_identity != previous.pair.stem_set_identity
    assert selected.producer_identity != previous.producer_identity
    assert len(queued.backend.requests) == initial_requests + 1
    assert queued.backend.requests[-1].model_cache_dir == queued.current_model
    assert queued.audio.prepare_stem_pair.call_count == 1
    assert queued.audio.prepare_stem_pair.call_args.args[4] is False
    assert queued.pairs[-1].has_components() is False
    assert queued.pairs[-1].selected == 1
    for operation in (
        queued.audio.publish_stem_pair,
        queued.audio.publish_prepared_stems,
        queued.audio.set_stem_mix_mode,
        queued.audio.set_stem_enabled_mask,
        queued.audio.set_stem_pair_full_mix,
    ):
        operation.assert_not_called()
    reopened = ProjectPersistence.from_config_path(queued.controller._persistence.config_path)
    assert reopened.load_error is None
    assert not reopened.invalid_stem_pair_ids
    assert reopened.project.stem_cache[0] == selected
    assert reopened.project.stem_cache[215] == queued.selected(215)
    assert reopened.project.pad_stem_mix_mode == queued.controller.project.pad_stem_mix_mode
    assert reopened.project.pad_stem_intent == queued.controller.project.pad_stem_intent
    assert not queued.tasks
    assert not queued.controller.stems._jobs
    assert not queued.controller.stems._pending_stem_publications
    assert not queued.controller.stems._disk_selection_settlements


@pytest.mark.parametrize("mix_mode", ["full_mix", "all_stems"])
def test_public_generate_while_active_commits_disk_only_and_retains_effective_pair(
    queued: _QueuedScenario,
    mix_mode: str,
) -> None:
    """Prove controller scheduling/ownership only; actual active rendering is Native-tested."""
    stems = queued.controller.stems
    captured: list[_OwnSourceTicket] = []

    def capture(sample_id: int, version: str) -> _OwnSourceTicket:
        ticket = queued.capture_ticket(sample_id, version)
        captured.append(ticket)
        return ticket

    queued.audio.capture_prepared_source.side_effect = capture
    _seed_controller_selected_pairs(queued, mix_mode)
    queued.controller.session.active_sample_ids.update((0, 215))
    baseline = _ActiveGenerationBaseline(queued)
    previous, earlier = queued.selected(), tuple(captured)
    initial_requests = len(queued.backend.requests)
    queued.current_model = queued.models[1]
    for operation in (
        queued.audio.capture_prepared_source,
        queued.audio.capture_prepared_stem_source,
        queued.audio.prepare_stem_pair,
        queued.audio.publish_stem_pair,
        queued.audio.publish_prepared_stems,
        queued.audio.set_stem_mix_mode,
        queued.audio.set_stem_enabled_mask,
        queued.audio.set_stem_pair_full_mix,
    ):
        operation.reset_mock()

    assert stems.stem_generation_block_reason(0) is None
    assert stems.generate_stems_async(0)
    source = stems._generation_sources[0]
    assert isinstance(source, FakeOfflineStemSourceLease)
    assert not source.verification_threads
    queued.audio.capture_prepared_source.assert_not_called()
    queued.step()  # actual existing identity resolution admits inference while active
    assert len(queued.backend.requests) == initial_requests
    baseline.assert_unchanged(queued)
    queued.step()  # the declared backend writes/verifies the complete generation
    baseline.assert_unchanged(queued)
    _assert_active_generation_fresh_source(queued, source, earlier)
    queued.step()  # components=False complete-pair preparation and actual config replacement
    baseline.assert_unchanged(queued)
    _assert_active_generated_disk_selection(queued, previous, initial_requests)
    selected = queued.selected()

    # An inactive later gesture follows ordinary fresh preparation and source ACK
    # checks. Public FullMix first withdraws any old effective selected components.
    queued.controller.session.active_sample_ids.discard(0)
    assert stems.set_stem_mix_mode(0, "full_mix")
    queued.allow_components = True
    assert stems.set_stem_mix_mode(0, "all_stems")
    queued.pump()
    assert queued.audio.prepare_stem_pair.call_args.args[4] is True
    assert queued.pairs[-1].has_components()
    queued.audio.publish_stem_pair.assert_called_once()
    assert 0 in stems._resident_pairs
    assert queued.selected().pair == selected.pair
    assert len(queued.backend.requests) == initial_requests + 1
    baseline.assert_unrelated_unchanged(queued)


def _assert_shared_own_subscribers(
    queued: _QueuedScenario, job: StemGenerationJob
) -> dict[int, StemSubscriber]:
    subscribers = {item.sample_id: item for item in job.subscribers()}
    assert set(subscribers) == {0, 215}
    assert subscribers[0].source is not subscribers[215].source
    assert subscribers[0].content_id != subscribers[215].content_id
    assert subscribers[0].intent is not None
    assert subscribers[215].intent is not None
    assert subscribers[0].intent.admission_sequence != subscribers[215].intent.admission_sequence
    assert subscribers[0].intent.material_revision == subscribers[215].intent.material_revision
    for slot, subscriber in subscribers.items():
        assert subscriber.request is not None
        assert subscriber.request.source_version == queued.versions[slot]
        assert isinstance(subscriber.source, FakeOfflineStemSourceLease)
        assert subscriber.source.sample_id == slot
        assert subscriber.source.source_version == queued.versions[slot]
        assert subscriber.source_lease is not None
        assert not _released(subscriber.source_lease)
    return subscribers


def _assert_own_preparation_fanout(
    queued: _QueuedScenario,
    subscribers: dict[int, StemSubscriber],
    expected: set[int],
    origin_version: str,
) -> None:
    pending_by_slot = queued.controller.stems._pair_preparations.pending
    assert set(pending_by_slot) == expected
    for slot, pending in pending_by_slot.items():
        subscriber = subscribers[slot]
        assert isinstance(pending.ticket, _OwnSourceTicket)
        assert pending.ticket.sample_id == slot
        assert pending.ticket.source_version == queued.versions[slot]
        queued.audio.capture_prepared_stem_source.assert_any_call(slot, subscriber.source)
        assert pending.content_id == subscriber.content_id
        assert pending.generation_intent == subscriber.intent
        assert pending.own_source_version == queued.versions[slot]
        assert pending.artifact_source_version == origin_version


def _assert_model_files_sealed(model: Path) -> None:
    for leaf in ("checkpoint.bin", "config.yaml", "definition.py"):
        with pytest.raises(PermissionError):
            (model / leaf).write_bytes(b"rewrite forbidden while actual producer seal is held")


class _BlockedRead:
    """One actual worker boundary, with deterministic progress and return gates."""

    def __init__(
        self,
        queued: _QueuedScenario,
        outcome: Literal["progress", "error", "success"],
        *,
        initial_progress: bool,
    ) -> None:
        self.queued = queued
        self.outcome = outcome
        self.initial_progress = initial_progress
        self.entered = Event()
        self.finish = Event()
        self.progress: list[Callable[[float, str], None]] = []
        self.worker = Thread(target=queued.tasks.pop(0))

    def generate(
        self, request: StemGenerationRequest, progress: Callable[[float, str], None]
    ) -> StemGenerationResult:
        if self.initial_progress:
            progress(0.25, "old reader entered")
        self.progress.append(progress)
        self.entered.set()
        assert self.finish.wait(5), "controller did not release owned blocked backend"
        if self.initial_progress:
            progress(0.9, "obsolete progress after ABA")
        if self.outcome == "error":
            message = "obsolete backend error after ABA"
            raise RuntimeError(message)
        return self.queued.generate_receipt(request, progress)

    def settle(self) -> None:
        self.finish.set()
        self.worker.join(5)
        assert not self.worker.is_alive()
        self.queued.poll()


@contextmanager
def _blocked_backend(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
    *,
    outcome: Literal["progress", "error", "success"] = "success",
    initial_progress: bool = False,
) -> Iterator[_BlockedRead]:
    read = _BlockedRead(queued, outcome, initial_progress=initial_progress)
    monkeypatch.setattr(queued.backend, "generate", read.generate)
    read.worker.start()
    try:
        assert read.entered.wait(5)
        yield read
    finally:
        read.finish.set()
        read.worker.join(5)
        assert not read.worker.is_alive()


def test_generation_admission_uses_cached_original_without_hashing_or_live_permit(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def forbidden_read(*_args: object, **_kwargs: object) -> Never:
        message = "Generate admission must only capture already loaded source metadata"
        raise AssertionError(message)

    with monkeypatch.context() as capture:
        capture.setattr(hashlib, "file_digest", forbidden_read)
        capture.setattr(stems_module, "source_version_for_sample_path", forbidden_read)
        assert queued.controller.stems.generate_stems_async(0)
        source = queued.controller.stems._generation_sources[0]
        assert isinstance(source, FakeOfflineStemSourceLease)
        assert not source.verification_threads
        assert source.source_version == queued.versions[0]
        assert source.original_bytes > 0
        assert source.target_shape == (44_100, 1, 128)
        queued.audio.capture_prepared_source.assert_not_called()
        queued.audio.capture_prepared_stem_source.assert_not_called()
    queued.pump()
    queued.selected()
    assert source.verification_threads


@pytest.mark.parametrize("replacement", ["bytes", "path"])
def test_worker_rejects_changed_original_before_inference_or_fresh_permit(
    queued: _QueuedScenario,
    replacement: str,
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    source = stems._generation_sources[0]
    assert isinstance(source, FakeOfflineStemSourceLease)
    reference = queued.controller.project.sample_paths[0]
    assert reference is not None
    original = queued.tmp / reference
    before = original.stat()
    data = original.read_bytes()
    if replacement == "bytes":
        original.write_bytes(data[:-2] + b"\x01\x02")
    else:
        renamed = original.with_suffix(".replacement")
        renamed.write_bytes(data)
        os.replace(renamed, original)
    os.utime(original, ns=(before.st_atime_ns, before.st_mtime_ns))
    assert original.stat().st_size == source.original_bytes
    assert original.stat().st_mtime_ns == before.st_mtime_ns

    queued.step()

    assert source.verification_threads
    assert not queued.backend.requests
    assert not queued.tasks
    assert not stems._jobs
    assert not stems._generation_sources
    assert "original changed" in (stems.stem_generation_error(0) or "").lower()
    assert not queued.pairs
    queued.audio.capture_prepared_source.assert_not_called()
    queued.audio.capture_prepared_stem_source.assert_not_called()


def test_timing_change_during_inference_receives_only_fresh_current_publication_permit(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    timing_revision = [1]

    def capture_current(sample_id: int, version: str) -> _OwnSourceTicket:
        ticket = queued.capture_ticket(sample_id, version)
        ticket.timing_revision = timing_revision[0]
        return ticket

    queued.audio.capture_prepared_source.side_effect = capture_current
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    queued.step()
    source = stems._generation_sources[0]
    with _blocked_backend(queued, monkeypatch) as read:
        queued.audio.capture_prepared_source.assert_not_called()
        queued.audio.capture_prepared_stem_source.assert_not_called()
        timing_revision[0] = 2
        queued.poll()
        queued.audio.capture_prepared_source.assert_not_called()
        read.settle()
        pending = stems._pair_preparations.pending[0]
        assert isinstance(pending.ticket, _OwnSourceTicket)
        assert pending.ticket.timing_revision == 2
        queued.audio.capture_prepared_stem_source.assert_called_once_with(0, source)
        queued.pump()
        queued.selected()
        assert len(queued.backend.requests) == 1
        assert sum(pair.selected for pair in queued.pairs) == 1


def test_origin_unload_during_shared_read_preserves_offline_source_and_survivor_fresh_permit(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    assert stems.generate_stems_async(215)
    queued.tasks.pop(0)()
    queued.tasks.pop(0)()
    queued.poll()
    job = next(iter(stems._jobs.values()))
    subscribers = _assert_shared_own_subscribers(queued, job)
    source = job.source
    assert isinstance(source, FakeOfflineStemSourceLease)
    with _blocked_backend(queued, monkeypatch) as read:
        queued.controller.loader.unload_sample(0)
        queued.poll()
        assert cast("object", job.source) is source
        assert not _released(job._source_lease)
        assert not queued.audio.offline_stem_source_is_current(0, source)
        source.verify()  # Origin assignment is gone; captured physical bytes still verify.
        queued.audio.capture_prepared_stem_source.assert_not_called()
        read.settle()
        assert job.source is None
        _assert_own_preparation_fanout(queued, subscribers, {215}, job.request.source_version)
        queued.audio.capture_prepared_stem_source.assert_called_once_with(
            215, subscribers[215].source
        )
        queued.pump()
        queued.selected(215)
        assert len(queued.backend.requests) == 1
        assert sum(pair.selected for pair in queued.pairs) == 1
        assert queued.controller.project.sample_paths[0] is None


def test_same_path_hash_reload_during_inference_cannot_launder_old_lease_into_fresh_permit(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    queued.step()
    old_job = next(iter(stems._jobs.values()))
    old_source = old_job.source
    assert isinstance(old_source, FakeOfflineStemSourceLease)
    with _blocked_backend(queued, monkeypatch) as read:
        path = queued.controller.project.sample_paths[0]
        content = queued.controller.project.pad_content[0]
        assert path is not None
        queued.controller.loader.unload_sample(0)
        queued.controller.project.sample_paths[0] = path
        queued.controller.project.sample_durations[0] = 128 / 44_100
        queued.controller.project.pad_content[0] = content
        queued.audio.load_sample_async(0, path)
        assert stems.generate_stems_async(0)
        queued.step()
        current = stems._generation_sources[0]
        assert current.source_version == old_source.source_version
        assert current.source_generation != old_source.source_generation
        assert not queued.audio.offline_stem_source_is_current(0, old_source)
        assert stems._waiting_jobs
        assert not queued.tasks
        read.settle()
        queued.audio.capture_prepared_stem_source.assert_not_called()
        assert stems._generation_sources[0] is current
        assert len(queued.tasks) == 1
        monkeypatch.setattr(queued.backend, "generate", queued.generate_receipt)
        queued.pump()
        queued.selected()
        queued.audio.capture_prepared_stem_source.assert_called_once_with(0, current)
        assert sum(pair.selected for pair in queued.pairs) == 1


@pytest.mark.parametrize("unload_origin", [False, True])
def test_equal_readable_roots_resolve_before_one_inference_and_keep_own_authority(
    queued: _QueuedScenario, *, unload_origin: bool
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    assert stems.generate_stems_async(215)
    assert len(queued.tasks) == 2
    # Both complete byte captures happen before the control equality decision;
    # equal filenames or a speculative in-flight model locator cannot dedup.
    queued.tasks.pop(0)()
    queued.tasks.pop(0)()
    assert not queued.backend.requests
    assert not queued.tasks
    queued.poll()
    assert len(stems._jobs) == 1
    assert len(queued.tasks) == 1
    job = next(iter(stems._jobs.values()))
    subscribers = _assert_shared_own_subscribers(queued, job)
    origin_version = job.request.source_version
    if unload_origin:
        queued.controller.loader.unload_sample(0)
        queued.poll()
        assert {item.sample_id for item in job.subscribers()} == {215}
        assert not _released(job._source_lease)
    queued.step()  # the single physical inference, then control-owned fanout
    expected = {215} if unload_origin else {0, 215}
    _assert_own_preparation_fanout(queued, subscribers, expected, origin_version)
    queued.pump()
    assert len(queued.backend.requests) == 1
    for slot in expected:
        entry = queued.selected(slot)
        assert entry.source_version == queued.versions[slot]
        assert entry.cache_dir.startswith(queued.versions[slot].split("|", 1)[0].rsplit("/", 1)[0])
    if not unload_origin:
        first, second = queued.selected(0), queued.selected(215)
        assert first.pair is not None
        assert second.pair is not None
        assert first.pair.provenance == second.pair.provenance
        assert first.pair.stem_set_identity != second.pair.stem_set_identity
        queued.controller.loader.unload_sample(0)
        assert queued.selected(215) is second
        assert (queued.tmp / second.cache_dir).is_dir()
    assert sum(pair.selected for pair in queued.pairs) == len(expected)


def test_equal_model_bytes_relocate_with_independent_candidate_and_selected_seals(
    queued: _QueuedScenario,
) -> None:
    queued.controller.settings.set_stem_separator("demucs:htdemucs")
    stems = queued.controller.stems
    first_model, relocated_model = queued.models
    for leaf in ("checkpoint.bin", "config.yaml", "definition.py"):
        shutil.copyfile(first_model / leaf, relocated_model / leaf)
    assert stems.generate_stems_async(0)
    queued.current_model = relocated_model
    assert stems.generate_stems_async(215)
    assert len(queued.tasks) == 2
    queued.tasks.pop(0)()
    queued.tasks.pop(0)()
    first = next(job for job in stems._jobs.values() if job.request.sample_id == 0)
    second = next(job for job in stems._jobs.values() if job.request.sample_id == 215)
    assert first.processing_files is not None
    assert second.processing_files is not None
    assert first.processing_files is not second.processing_files
    assert first.generation_fingerprint is not None
    assert first.generation_fingerprint == second.generation_fingerprint
    assert first.producer_identity == second.producer_identity
    assert not queued.backend.requests
    queued.poll()
    assert len(stems._jobs) == 1
    assert len(queued.tasks) == 1
    assert stems._verified_candidates[0][1].model_cache_dir == first_model
    assert stems._verified_candidates[215][1].model_cache_dir == relocated_model
    assert stems._verified_candidates[0][2] is not stems._verified_candidates[215][2]
    _assert_model_files_sealed(first_model)
    _assert_model_files_sealed(relocated_model)
    queued.pump()
    assert len(queued.backend.requests) == 1
    original, relocated = queued.selected(0), queued.selected(215)
    assert original.pair is not None
    assert relocated.pair is not None
    assert original.pair.provenance == relocated.pair.provenance
    assert stems._verified_sets[215].request.model_cache_dir == relocated_model
    # Neither these jobs nor test-held Native guards can mask snapshot release.
    assert first.processing_files is None
    assert second.processing_files is None
    _assert_model_files_sealed(relocated_model)
    assert stems.stem_generation_block_reason(0) is None
    assert (
        stems.stem_generation_block_reason(215) == "Stems for the current model are already present"
    )
    queued.settings_aba()
    queued.poll()
    assert not stems._verified_sets
    assert not stems._verified_candidates
    for model in queued.models:
        for leaf in ("checkpoint.bin", "config.yaml", "definition.py"):
            (model / leaf).write_bytes(b"owned fixture rewrite after legitimate ABA withdrawal")


def test_settings_aba_rejects_blocked_resolution_before_any_inference(
    queued: _QueuedScenario,
) -> None:
    queued.controller.settings.set_stem_separator("demucs:htdemucs")
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    old = stems._generation_intents[0]
    queued.settings_aba()
    queued.step()
    assert not queued.backend.requests
    assert not queued.pairs
    assert not stems._jobs
    assert stems.stem_generation_error(0) is None
    assert stems.generate_stems_async(0)
    assert stems._generation_intents[0].settings_revision > old.settings_revision
    assert stems._generation_intents[0].admission_sequence > old.admission_sequence
    queued.pump()
    queued.selected()
    assert len(queued.backend.requests) == 1


def test_initial_generation_and_control_fanout_never_hash_original_on_control(
    queued: _QueuedScenario, monkeypatch: pytest.MonkeyPatch
) -> None:
    original_verify = FakeOfflineStemSourceLease.verify
    in_worker = False
    verifications: list[tuple[str, int]] = []

    def verified_source(source: FakeOfflineStemSourceLease) -> None:
        assert in_worker, "full original hashing escaped the bounded worker"
        original_verify(source)  # Actual fixture full-byte, extent and FileID verification.
        verifications.append((source.source_path, len(queued.backend.requests)))

    def legacy_source_hash(*_args: object, **_kwargs: object) -> Never:
        assert in_worker, "legacy original hashing escaped the bounded worker"
        message = "Offline source lease must own full original verification"
        raise AssertionError(message)

    monkeypatch.setattr(FakeOfflineStemSourceLease, "verify", verified_source)
    monkeypatch.setattr(stems_module, "source_version_for_sample_path", legacy_source_hash)
    assert queued.controller.stems.generate_stems_async(0)
    assert not verifications
    for _ in range(8):
        if not queued.tasks:
            break
        in_worker = True
        try:
            queued.tasks.pop(0)()
        finally:
            in_worker = False
        queued.poll()
    assert not queued.tasks
    queued.selected()
    reference = queued.controller.project.sample_paths[0]
    assert reference is not None
    assert verifications == [(str((queued.tmp / reference).absolute()), phase) for phase in (0, 1)]
    assert len(queued.backend.requests) == 1


@pytest.mark.parametrize("stage", ["resolution", "inference"])
def test_unexpected_worker_failure_retires_admission_and_releases_serialization_slot(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
    stage: Literal["resolution", "inference"],
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    old_job = next(iter(stems._jobs.values()))
    if stage == "inference":
        queued.step()

    def unexpected(_request: StemGenerationRequest, *_args: object) -> Never:
        message = "owned unexpected worker failure"
        raise AssertionError(message)

    if stage == "resolution":
        monkeypatch.setattr(stems_module, "capture_processing_files", unexpected)
    else:
        monkeypatch.setattr(queued.backend, "generate", unexpected)
    with pytest.raises(AssertionError, match="owned unexpected worker failure"):
        queued.tasks.pop(0)()
    queued.poll()
    assert not stems._jobs
    assert not stems._inference_jobs
    assert not stems._waiting_jobs
    assert not stems._generation_sources
    assert not stems._generation_intents
    assert not stems.is_stem_generation_running(0)
    assert queued.controller._assets._reserved == 0
    assert _released(old_job._source_lease)
    assert _released(old_job._generation_lease)
    assert not queued.pairs
    monkeypatch.setattr(
        stems_module, "capture_processing_files", stem_provenance.capture_processing_files
    )
    monkeypatch.setattr(queued.backend, "generate", queued.generate_receipt)
    assert stems.generate_stems_async(0)
    queued.step()  # a new resolution must be able to admit actual inference
    assert len(queued.tasks) == 1
    queued.pump()
    queued.selected()
    assert len(queued.backend.requests) == 1


@pytest.mark.parametrize("old_outcome", ["progress", "error", "success"])
def test_settings_aba_running_reader_keeps_custody_and_stale_events_cannot_select(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
    old_outcome: Literal["progress", "error", "success"],
) -> None:
    queued.controller.settings.set_stem_separator("demucs:htdemucs")
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    queued.step()  # identity resolution admits a still-unstarted inference
    old_job = next(iter(stems._jobs.values()))
    old_source = old_job.source
    assert old_source is not None
    with _blocked_backend(queued, monkeypatch, outcome=old_outcome, initial_progress=True) as read:
        queued.poll()
        assert stems.stem_generation_stage(0) == "old reader entered"
        queued.settings_aba()
        queued.poll()
        assert not _released(old_job._source_lease)
        assert not _released(old_job._generation_lease)
        assert old_job.source is old_source
        assert stems.generate_stems_async(0)
        queued.step()  # replacement resolves but must wait for actual backend return
        current_source = stems._generation_sources[0]
        current_intent = stems._generation_intents[0]
        assert stems._waiting_jobs
        assert not queued.tasks
        assert not queued.pairs
        if old_outcome == "progress":
            # Exercise a queued obsolete progress event while the old reader is
            # still blocked, before its eventual terminal event can settle it.
            read.progress[0](0.77, "obsolete progress while blocked")
            queued.poll()
            assert stems.stem_generation_stage(0) != "obsolete progress while blocked"
            assert stems._generation_sources[0] is current_source
        read.settle()
        assert _released(old_job._source_lease)
        assert _released(old_job._generation_lease)
        assert old_job.source is None
        assert stems._generation_sources[0] is current_source
        assert stems._generation_intents[0] == current_intent
        assert stems.stem_generation_error(0) is None
        assert stems.stem_generation_stage(0) != "obsolete progress after ABA"
        assert not queued.pairs
        assert len(queued.tasks) == 1
        monkeypatch.setattr(queued.backend, "generate", queued.generate_receipt)
        queued.pump()
        queued.selected()
        assert sum(pair.selected for pair in queued.pairs) == 1


def test_settings_aba_discards_returned_old_preparation_without_selection(
    queued: _QueuedScenario,
) -> None:
    queued.controller.settings.set_stem_separator("demucs:htdemucs")
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    queued.step()
    queued.step()
    pending = stems._pair_preparations.pending[0]
    assert pending.generation_intent is not None
    queued.settings_aba()
    queued.step()  # complete pair returns under its old captured intent
    assert _released(pending.lease)
    assert pending.retirement.remaining == 0
    assert queued.pairs
    assert queued.pairs[0].selected == 0
    assert queued.pairs[0].discarded == 1
    assert stems.stem_generation_error(0) is None
    assert stems.generate_stems_async(0)
    queued.pump()
    queued.selected()
    assert sum(pair.selected for pair in queued.pairs) == 1


@pytest.mark.parametrize("replacement", ["producer", "model"])
def test_different_resolved_producer_or_model_serializes_until_actual_reader_return(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
    replacement: str,
) -> None:
    queued.controller.settings.set_stem_separator("demucs:htdemucs")
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    queued.step()
    first = next(iter(stems._jobs.values()))
    with _blocked_backend(queued, monkeypatch) as read:
        queued.current_model = queued.models[1]
        if replacement == "model":
            queued.controller.settings.set_stem_separator("bs-roformer:musdb18hq")
        assert stems.generate_stems_async(215)
        queued.step()
        second = next(job for job in stems._jobs.values() if job is not first)
        assert first.generation_fingerprint is not None
        assert first.producer_identity is not None
        assert second.generation_fingerprint is not None
        assert second.producer_identity is not None
        assert second.generation_fingerprint != first.generation_fingerprint
        assert second.producer_identity != first.producer_identity
        assert second.request.separator == (
            "bs-roformer:musdb18hq" if replacement == "model" else "demucs:htdemucs"
        )
        assert second.request.cache_dir in stems._waiting_jobs
        assert not queued.tasks
        assert first.running
        assert not _released(first._source_lease)
        assert not _released(first._generation_lease)
        queued.poll()  # producer binding expires the prior material intent
        assert not first.subscribers()
        read.settle()
        assert not queued.pairs
        assert len(queued.tasks) == 1
        monkeypatch.setattr(queued.backend, "generate", queued.generate_receipt)
        queued.pump()
        entry = queued.selected(215)
        assert entry.generation_fingerprint == second.generation_fingerprint
        assert entry.producer_identity == second.producer_identity
        assert sum(pair.selected for pair in queued.pairs) == 1
        assert _released(first._source_lease)
        assert _released(first._generation_lease)


def test_current_set_direct_and_ui_duplicate_is_diagnostic_and_render_does_no_hashing(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    queued.pump()
    entry = queued.selected()
    inference_count = len(queued.backend.requests)
    preparation_count = queued.audio.prepare_stem_pair.call_count
    forbidden = Mock(
        side_effect=AssertionError("render/duplicate must use verified intent metadata")
    )
    monkeypatch.setattr(stems, "source_version_for_pad", forbidden)
    monkeypatch.setattr(stems_module, "source_version_for_sample_path", forbidden)
    monkeypatch.setattr(stems_module, "capture_processing_files", forbidden)
    monkeypatch.setattr(stems_module, "request_fingerprint", forbidden)
    imgui = Mock()
    imgui.button.return_value = False
    monkeypatch.setattr(sidebar_left, "imgui", imgui)
    ctx = cast(
        "UiContext",
        SimpleNamespace(state=SimpleNamespace(stems=stems), audio=SimpleNamespace(stems=stems)),
    )
    for _ in range(3):
        sidebar_left._render_generate_stems(ctx, 0)
        queued.poll()
    assert imgui.begin_disabled.call_args_list[0].kwargs == {"disabled": True}
    assert stems.generate_stems_async(0)
    assert stems.stem_generation_diagnostic(0) == "Stems for the current model are already present"
    assert stems.stem_generation_error(0) is None
    assert queued.selected() is entry
    assert not queued.tasks
    assert len(queued.backend.requests) == inference_count
    assert queued.audio.prepare_stem_pair.call_count == preparation_count
    forbidden.assert_not_called()


def test_worker_capacity_failure_releases_only_failed_admission_ownership(
    queued: _QueuedScenario,
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    queued.pump()
    selected = queued.selected()
    assignment_owners = tuple(
        owner for _path, owner in queued.controller._assets._assignments.values()
    )
    baseline = len(queued.leases)

    def queue_full(_worker: Callable[[], None]) -> None:
        message = "owned worker queue full"
        raise RuntimeError(message)

    stems._stem_task_runner = queue_full
    assert not stems.generate_stems_async(215)
    error = stems.stem_generation_error(215)
    assert error is not None
    assert "worker admission failed" in error.lower()
    queued.poll()
    assert not stems._jobs
    assert not stems._generation_sources
    assert not stems._generation_intents
    assert not stems.is_stem_generation_running(215)
    assert queued.controller._assets._reserved == 0
    assert queued.leases[baseline:]
    assert all(owner.released for owner in queued.leases[baseline:])
    assert all(not _released(owner) for owner in assignment_owners)
    assert queued.selected() is selected
    assert len(queued.backend.requests) == 1
    stems._stem_task_runner = queued.tasks.append
    assert stems.generate_stems_async(215)
    queued.pump()
    queued.selected(215)
    assert (
        len(queued.backend.requests) == 1
    )  # verified completed-set reuse after admission recovers


def _assert_failed_shared_job_settled(
    queued: _QueuedScenario,
    job: StemGenerationJob,
    subscribers: dict[int, StemSubscriber],
) -> None:
    stems = queued.controller.stems
    assert not queued.backend.requests
    assert not queued.tasks
    assert not queued.pairs
    assert not stems._jobs
    assert not stems._inference_jobs
    assert not stems._waiting_jobs
    assert not stems._generation_sources
    assert not stems._generation_intents
    assert not stems._pair_preparations.pending
    assert not stems._pending_stem_publications
    assert not stems._verified_candidates
    assert queued.controller._assets._reserved == 0
    assert _released(job._source_lease)
    assert _released(job._generation_lease)
    for subscriber in subscribers.values():
        assert subscriber.source_lease is not None
        assert _released(subscriber.source_lease)
        assert not stems.is_stem_generation_running(subscriber.sample_id)


@pytest.mark.parametrize("unload_origin", [False, True])
def test_shared_inference_queue_rejection_settles_each_own_interest_and_recovers(
    queued: _QueuedScenario,
    monkeypatch: pytest.MonkeyPatch,
    *,
    unload_origin: bool,
) -> None:
    stems = queued.controller.stems
    assert stems.generate_stems_async(0)
    assert stems.generate_stems_async(215)
    assert len(queued.tasks) == 2
    queued.tasks.pop(0)()
    queued.tasks.pop(0)()
    # Gate only inference scheduling while control consumes both actual identity
    # results. Production binding/transfer forms one shared waiting job first.
    start_waiting = stems._start_waiting_generations
    monkeypatch.setattr(stems, "_start_waiting_generations", lambda: None)
    queued.poll()
    assert len(stems._jobs) == 1
    assert len(stems._waiting_jobs) == 1
    assert not stems._inference_jobs
    assert not queued.tasks
    job = next(iter(stems._jobs.values()))
    subscribers = _assert_shared_own_subscribers(queued, job)
    if unload_origin:
        queued.controller.loader.unload_sample(0)
        assert {subscriber.sample_id for subscriber in job.subscribers()} == {215}
    survivors = {215} if unload_origin else {0, 215}
    queued.controller._assets.sync_assignments()
    retained = {
        slot: (
            queued.controller.project.sample_paths[slot],
            queued.controller.project.pad_content[slot],
            queued.controller.project.stem_cache[slot],
            queued.controller._assets._assignments["original", slot],
        )
        for slot in survivors
    }

    def inference_queue_full(_worker: Callable[[], None]) -> None:
        message = "owned shared inference queue full"
        raise RuntimeError(message)

    monkeypatch.setattr(stems, "_start_waiting_generations", start_waiting)
    stems._stem_task_runner = inference_queue_full
    stems._start_waiting_generations()
    queued.poll()
    _assert_failed_shared_job_settled(queued, job, subscribers)
    for slot, (path, content, cache, owner) in retained.items():
        assert queued.controller.project.sample_paths[slot] == path
        assert queued.controller.project.pad_content[slot] is content
        assert queued.controller.project.stem_cache[slot] is cache
        assert queued.controller._assets._assignments["original", slot] is owner
        assert not _released(owner[1])
        error = stems.stem_generation_error(slot)
        assert error is not None
        assert "owned shared inference queue full" in error
    stems._stem_task_runner = queued.tasks.append
    assert stems.generate_stems_async(215)
    queued.pump()
    recovered = queued.selected(215)
    assert recovered.source_version == queued.versions[215]
    assert len(queued.backend.requests) == 1
