"""Coordinator oracles with real config/journal files and explicit fake audio phases.

The fakes exercise control settlement; they do not certify native ACKs, readers,
WAV decoding, realtime behavior, or audible output.
"""

import hashlib
import json
import subprocess
import sys
from concurrent.futures import Future
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.accepted_timing import PersistedAcceptedTiming
from flitzis_looper.controller import material_migration as migration_module
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.material_migration_recovery import inspect_migration_recovery
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.key_intent import MAX_KEY_EPOCH, PadKeyIntent, SourceKeyVersion
from flitzis_looper.material_migration_model import MaterialMigrationJournal
from flitzis_looper.models import (
    STEM_KINDS,
    BeatGrid,
    PadContentIdentity,
    SampleAnalysis,
    SessionState,
    StemCacheEntry,
)
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.conftest import FakeProjectAssetLease, write_test_stem_marker

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import ConstantTimingTicket


_OLD = "samples/old.wav"
_MATERIAL = "a" * 32
_NEW = f"samples/materials/M{_MATERIAL}/original/old.wav"
_IDS = (0, 215)


def _old_stem_set(slot: int = 1) -> StemCacheEntry:
    cache = Path(f"samples/stems/#{slot}")
    cache.mkdir(parents=True)
    for kind in STEM_KINDS:
        write_mono_pcm16_wav(cache / f"{kind}.wav", 44_100)
    version = f"{_OLD}|sha256-v1:{hashlib.sha256(Path(_OLD).read_bytes()).hexdigest()}"
    write_test_stem_marker(cache, version)
    return StemCacheEntry(
        source_version=version,
        cache_dir=cache.as_posix(),
        stems=expected_stem_files(cache.as_posix()),
        available=True,
    )


class _Preparation:
    def __init__(self) -> None:
        self.state = "ready"
        self.abort_count = 0
        self.cancel_count = 0
        self.release_count = 0

    def status(self) -> str:
        return self.state

    def error(self) -> str:
        return "decoder preparation failed"

    def metadata_json(self) -> str:
        return json.dumps({
            "old_reference": _OLD,
            "new_reference": _NEW,
            "material_id": _MATERIAL,
            "original": {
                "sha256": hashlib.sha256(Path(_OLD).read_bytes()).hexdigest(),
                "bytes": Path(_OLD).stat().st_size,
            },
            "decoder_identity": "b" * 64,
            "playback_identity": "c" * 64,
            "cache_path": f"samples/materials/M{_MATERIAL}/.pcm-cache/v1/.ready-{'d' * 32}",
        })

    def abort_unpublished(self) -> None:
        self.abort_count += 1

    def cancel(self) -> None:
        self.cancel_count += 1

    def abort_rejected(self) -> None:
        self.abort_count += 1

    def release_preparation(self) -> None:
        self.release_count += 1


class _Hold:
    def __init__(self) -> None:
        self.release_count = 0

    def release(self) -> None:
        self.release_count += 1


class _SourceTicket:
    def __init__(self, sample_id: int) -> None:
        self.request_id = 1000 + sample_id
        self.state = "pending"
        self.current = True

    def phase(self) -> str:
        return self.state

    def is_current(self) -> bool:
        return self.state == "acknowledged" and self.current

    def cancel_unclaimed(self) -> bool:
        if self.state in {"pending", "rejected"}:
            self.state = "rejected"
            return True
        return False


class _StemWork:
    def __init__(self, *, state: str = "preparing", created: bool = False) -> None:
        self.state = state
        self.was_created = created
        self.cancel_count = 0

    def status(self) -> str:
        return self.state

    def cancel(self) -> None:
        self.cancel_count += 1

    def error(self) -> str:
        return "stem copy cancelled"

    def created(self) -> bool:
        return self.was_created

    def cache_reference(self) -> str:
        return f"samples/materials/M{_MATERIAL}/stems/.ready-{'e' * 32}"


class _DeliveredLease(FakeProjectAssetLease):
    def __init__(self, path: str) -> None:
        super().__init__(path)
        self.acknowledged: list[str] = []

    def acknowledge(self, path: str) -> None:
        assert Path(path) == Path(self.path)
        assert not self.released
        self.acknowledged.append(path)


class _TimingTicket:
    def __init__(self) -> None:
        self.state = "pending"

    def publication_status(self) -> str:
        return self.state

    def metadata(self) -> dict[str, object]:
        return {"request_id": 77}

    def accepted_metadata(self) -> dict[str, object] | None:
        return {"revision": "fresh-timing"} if self.state == "accepted" else None


class _Harness:
    def __init__(self, controller: AppController, audio: Mock) -> None:
        self.controller = controller
        self.audio = audio
        self.preparation = _Preparation()
        self.hold = _Hold()
        self.sources: dict[int, _SourceTicket] = {}
        self.owners: list[FakeProjectAssetLease] = []
        self.delivered: dict[int, _DeliveredLease] = {}
        self.transaction: str | None = None
        Path(_OLD).parent.mkdir(exist_ok=True)
        write_mono_pcm16_wav(Path(_OLD), 44_100)
        Path(_NEW).parent.mkdir(parents=True)
        Path(_NEW).write_bytes(Path(_OLD).read_bytes())
        for sample_id in _IDS:
            self.seed(sample_id)
        project = controller.project
        project.selected_pad = 215
        project.selected_bank = 5
        project.volume = 0.7
        project.stem_separator = "bs-roformer:musdb18hq"
        project.pad_key_intent[0] = PadKeyIntent(
            source=SourceKeyVersion(version=MAX_KEY_EPOCH, raw_key="Em"),
            correction="Cm",
            analysis_epoch=MAX_KEY_EPOCH,
            correction_epoch=MAX_KEY_EPOCH,
            base_shift=-4,
            extra_shift=12,
            retrigger=True,
        )
        project.pad_key_intent[215] = PadKeyIntent(
            source=SourceKeyVersion(version=19, raw_key="Bb"),
            correction="arbitrary saved key ♭",
            analysis_epoch=7,
            correction_epoch=13,
            base_shift=6,
            extra_shift=-18,
            retrigger=False,
        )

        def acquire(path: str) -> FakeProjectAssetLease:
            lease = FakeProjectAssetLease(path)
            self.owners.append(lease)
            return lease

        def adopt(sample_id: int, preparation: object) -> _SourceTicket:
            assert preparation is self.preparation
            assert sample_id not in self.sources
            ticket = _SourceTicket(sample_id)
            self.sources[sample_id] = ticket
            return ticket

        audio.acquire_project_asset_lease.side_effect = acquire
        audio.hold_material_migration = Mock(return_value=self.hold)
        audio.prepare_material_migration = Mock(return_value=self.preparation)
        audio.adopt_material_migration = Mock(side_effect=adopt)
        audio.prepare_material_migration_stems = Mock()
        audio.pad_timing_intent.side_effect = lambda sample_id: project.pad_timing_intent[sample_id]
        audio.export_current_constant_timing.return_value = None
        controller._assets.sync_assignments()
        controller.persistence.mark_dirty()
        controller.persistence.flush(now=0.0)
        self.initial_config = controller.persistence.config_path.read_bytes()
        self.migration = controller.material_migration

    def seed(self, sample_id: int) -> None:
        project = self.controller.project
        project.sample_paths[sample_id] = _OLD
        project.pad_content[sample_id] = PadContentIdentity(instance_id=f"{sample_id + 1:032x}")
        project.sample_durations[sample_id] = 128 / 44_100
        project.pad_gain_db[sample_id] = -3.0
        project.pad_eq_low_db[sample_id] = 2.0
        project.pad_loop_start_s[sample_id] = 0.0002
        project.pad_loop_end_s[sample_id] = 0.002
        project.manual_bpm[sample_id] = 123.0
        project.pad_timing_intent[sample_id] = "manual"
        project.sample_analysis[sample_id] = SampleAnalysis(
            bpm=119.0,
            key="Em",
            beat_grid=BeatGrid(beats=[0.0], downbeats=[0.0], bars=[0.0]),
        )

    def begin(self) -> None:
        self.transaction = self.migration.begin(0)

    def records(self) -> list[MaterialMigrationJournal]:
        assert self.transaction is not None
        root = Path("samples/.material-migrations") / self.transaction
        return [
            MaterialMigrationJournal.model_validate_json(path.read_bytes())
            for path in sorted(root.glob("[0-9][0-9].json"))
        ]

    def acknowledge(self, *sample_ids: int) -> None:
        events: list[dict[str, object]] = []
        for sample_id in sample_ids or tuple(self.sources):
            source = self.sources[sample_id]
            source.state = "acknowledged"
            owner = _DeliveredLease(str(Path(_NEW).absolute()))
            self.delivered[sample_id] = owner
            events.append({
                "type": "success",
                "id": sample_id,
                "request_id": source.request_id,
                "path": _NEW,
                "original_lease": owner,
                "duration": 999.0,
                "analysis": {"bpm": 80.0, "key": "Bm", "beat_grid": {}},
            })
        self.audio.poll_loader_events.side_effect = [*events, None]
        self.controller.loader.poll_loader_events()

    def assert_retained(self) -> None:
        assert self.hold.release_count == 0
        assert self.preparation.abort_count == self.preparation.release_count == 0
        assert Path(_OLD).is_file()
        assert Path(_NEW).is_file()
        assert any(
            not owner.released and Path(owner.path) == Path(_OLD).absolute()
            for owner in self.owners
        )
        assert not self.controller.persistence.flush_if_dirty()
        with pytest.raises(PersistenceFenceError):
            self.controller.persistence.flush()


@pytest.fixture
def migration(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> _Harness:
    # Preserve the production type check while substituting the explicit control fake.
    monkeypatch.setattr(migration_module, "ProjectAssetLease", _DeliveredLease)
    return _Harness(controller, audio_engine_mock)


def _assert_saved_preservation(
    migration: _Harness, expected: ProjectState, before_ids: list[str | None]
) -> None:
    project = migration.controller.project
    assert [value.instance_id if value else None for value in project.pad_content] == before_ids
    saved = ProjectPersistence.from_config_path(
        migration.controller.persistence.config_path
    ).project
    assert saved.pad_key_intent == expected.pad_key_intent == project.pad_key_intent
    assert saved.pad_gain_db == expected.pad_gain_db
    assert saved.pad_loop_start_s == expected.pad_loop_start_s
    assert saved.pad_loop_end_s == expected.pad_loop_end_s
    assert saved.manual_bpm == expected.manual_bpm
    assert saved.pad_timing_intent == expected.pad_timing_intent
    assert saved.sample_analysis == expected.sample_analysis
    assert (saved.selected_pad, saved.selected_bank, saved.volume, saved.stem_separator) == (
        215,
        5,
        0.4,
        expected.stem_separator,
    )
    for sample_id in _IDS:
        content = saved.pad_content[sample_id]
        assert content is not None
        assert content.instance_id == before_ids[sample_id]
        assert content.material_id == _MATERIAL
        assert saved.sample_paths[sample_id] == _NEW
        assert migration.delivered[sample_id].acknowledged == [str(Path(_NEW).absolute())]
        assert migration.delivered[sample_id].released
    assert migration.transaction in saved.material_migrations


def test_real_json_and_journal_commit_preserves_content_and_current_performer_intent(
    migration: _Harness, monkeypatch: pytest.MonkeyPatch
) -> None:
    controller = migration.controller
    project = controller.project
    before_ids = [value.instance_id if value else None for value in project.pad_content]
    migration.begin()
    capture = migration.records()[0]
    assert capture.phase == "captured"
    assert [item.sample_id for item in capture.assignments] == list(_IDS)
    assert capture.config_sha256 == hashlib.sha256(migration.initial_config).hexdigest()
    assert json.loads(capture.snapshot_json)["pad_key_intent"][0]["analysis_epoch"] == MAX_KEY_EPOCH
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=2, retrigger=False)
    project.pad_key_intent[215] = project.pad_key_intent[215].corrected("G#m")
    project.pad_gain_db[215] = -7.0
    project.volume = 0.4
    controller.persistence.mark_dirty()
    expected = project.model_copy(deep=True)
    ordinary_success = Mock(wraps=controller.loader._handle_loader_success)
    monkeypatch.setattr(controller.loader, "_handle_loader_success", ordinary_success)

    migration.migration.poll()
    assert migration.migration.status == "adoption_pending"
    assert set(migration.sources) == set(_IDS)
    assert len({source.request_id for source in migration.sources.values()}) == 2
    migration.acknowledge()
    ordinary_success.assert_not_called()
    assert project.sample_paths == expected.sample_paths
    assert project.pad_key_intent == expected.pad_key_intent
    assert project.sample_analysis == expected.sample_analysis
    assert project.sample_durations == expected.sample_durations
    migration.assert_retained()
    migration.migration.poll()

    assert migration.migration.status == "config_committed"
    assert migration.hold.release_count == migration.preparation.release_count == 1
    assert migration.preparation.abort_count == 0
    assert not controller.session.loading_sample_ids
    assert controller._assets._reserved == 0
    assert controller.persistence._migration_owner is None
    _assert_saved_preservation(migration, expected, before_ids)
    records = migration.records()
    assert [record.phase for record in records] == [
        "captured",
        "material_verified",
        "references_prepared",
        "adoption_pending",
        "ack_confirmed",
        "config_committed",
    ]
    assert (
        records[-1].committed_config_sha256
        == hashlib.sha256(controller.persistence.config_path.read_bytes()).hexdigest()
    )
    assert records[-1].committed_revision == controller.persistence.revision
    assert Path(_OLD).read_bytes() == Path(_NEW).read_bytes()


@pytest.mark.parametrize("phase", ["pending", "claimed"])
def test_physical_source_phase_without_success_never_optimistically_commits_or_rolls_back(
    migration: _Harness, phase: str
) -> None:
    migration.begin()
    migration.migration.poll()
    for source in migration.sources.values():
        source.state = phase
    migration.migration.poll()
    assert migration.migration.status == "adoption_pending"
    assert migration.controller.project.sample_paths[0] == _OLD
    assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config
    migration.assert_retained()
    migration.migration.shut_down()
    assert migration.migration.status == "unresolved"
    assert migration.records()[-1].phase == "unresolved"
    migration.assert_retained()


@pytest.mark.parametrize("current", [False, True])
def test_acknowledged_error_uses_actual_current_phase_and_fresh_owner_admission(
    migration: _Harness, *, current: bool
) -> None:
    migration.begin()
    migration.migration.poll()
    for source in migration.sources.values():
        source.state = "acknowledged"
    source = migration.sources[0]
    source.current = current
    migration.audio.poll_loader_events.side_effect = [
        {
            "type": "error",
            "id": 0,
            "request_id": source.request_id,
            "error": "delivery failed after physical ACK",
        },
        None,
    ]
    migration.controller.loader.poll_loader_events()
    migration.migration.poll()
    assert migration.preparation.abort_count == 0
    if current:
        assert migration.migration.status == "config_committed"
        assert migration.hold.release_count == migration.preparation.release_count == 1
        assert migration.controller.project.sample_paths[0] == _NEW
        assert any(
            not owner.released and Path(owner.path) == Path(_NEW).absolute()
            for owner in migration.owners
        )
    else:
        assert migration.migration.status == "unresolved"
        assert migration.controller.project.sample_paths[0] == _OLD
        assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config
        migration.assert_retained()


def test_actual_ticket_ack_without_success_feedback_acquires_both_images_before_write(
    migration: _Harness, monkeypatch: pytest.MonkeyPatch
) -> None:
    migration.begin()
    migration.migration.poll()
    for source in migration.sources.values():
        source.state = "acknowledged"
    actual_write = migration.controller.persistence._atomic_write_text

    def write(content: str) -> None:
        assert all(source.is_current() for source in migration.sources.values())
        assert (
            sum(
                not owner.released and Path(owner.path) == Path(_NEW).absolute()
                for owner in migration.owners
            )
            >= 2
        )
        assert migration.controller.project.sample_paths[0] == _OLD
        migration.assert_retained()
        actual_write(content)

    monkeypatch.setattr(migration.controller.persistence, "_atomic_write_text", write)
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    assert migration.hold.release_count == migration.preparation.release_count == 1
    assert migration.preparation.abort_count == 0
    assert not migration.delivered
    assert not migration.controller.session.loading_sample_ids


def test_superseded_actual_source_ack_cannot_commit_old_subscriber_or_release_owners(
    migration: _Harness,
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    migration.sources[215].current = False
    migration.migration.poll()
    assert migration.migration.status == "unresolved"
    assert migration.controller.project.sample_paths[215] == _OLD
    assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config
    migration.assert_retained()
    with pytest.raises(ValueError, match="acknowledgement"):
        migration.migration.retry_commit()


@pytest.mark.parametrize("request_id", [None, True, False, "1000", 1000.0, 999])
def test_only_exact_strict_owned_request_can_settle_source_feedback(
    migration: _Harness, request_id: object
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.sources[0].state = "claimed"
    before = migration.controller.project.model_dump()
    assert not migration.migration.handle_loader_event({
        "type": "success",
        "id": 0,
        "request_id": request_id,
        "path": _NEW,
    })
    migration.migration.poll()
    assert migration.migration.status == "adoption_pending"
    assert migration.controller.project.model_dump() == before
    assert 0 in migration.controller.session.loading_sample_ids
    migration.assert_retained()


def test_preclaim_preparation_failure_releases_fence_and_hold_without_mutating_project(
    migration: _Harness,
) -> None:
    before = migration.controller.project.model_dump()
    migration.preparation.state = "failed"
    migration.begin()
    migration.migration.poll()
    assert migration.migration.status == "failed"
    assert migration.preparation.abort_count == migration.hold.release_count == 1
    assert migration.preparation.release_count == 0
    migration.audio.adopt_material_migration.assert_not_called()
    assert migration.controller.project.model_dump() == before
    assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config
    assert migration.controller.persistence._migration_owner is None
    assert migration.controller._assets._reserved == 0
    assert migration.records()[-1].phase == "failed"


def test_complete_wav_worker_rejection_fails_before_any_source_admission(
    migration: _Harness,
) -> None:
    entry = _old_stem_set()
    cache = Path(entry.cache_dir)
    (cache / "bass.wav").unlink()
    migration.controller.project.stem_cache[0] = entry
    migration.controller.persistence.mark_dirty()
    migration.controller.persistence.flush(now=1.0)
    before = migration.controller.persistence.config_path.read_bytes()
    marker = (cache / ".complete.json").read_bytes()
    migration.audio.prepare_material_migration_stems.return_value = _StemWork(state="failed")
    migration.begin()
    migration.migration.poll()
    assert migration.migration.status == "failed"
    assert migration.controller.project.stem_cache[0] == entry
    migration.audio.prepare_material_migration_stems.assert_called_once()
    migration.audio.adopt_material_migration.assert_not_called()
    assert migration.controller.persistence.config_path.read_bytes() == before
    assert (cache / ".complete.json").read_bytes() == marker
    assert (cache / "vocals.wav").is_file()
    assert migration.preparation.abort_count == migration.hold.release_count == 1


def test_stem_preparation_poll_never_hashes_complete_wavs_on_the_host_thread(
    migration: _Harness, monkeypatch: pytest.MonkeyPatch
) -> None:
    entry = _old_stem_set()
    for sample_id in _IDS:
        migration.controller.project.stem_cache[sample_id] = entry.model_copy(deep=True)
    work = _StemWork(state="preparing")
    migration.audio.prepare_material_migration_stems.return_value = work
    host_hash = Mock(
        side_effect=AssertionError("complete WAV hashing must run in the native worker")
    )
    monkeypatch.setattr("flitzis_looper.controller.stem_cache._file_digest", host_hash)
    migration.begin()
    for _ in range(5):
        migration.migration.poll()
    host_hash.assert_not_called()
    migration.audio.prepare_material_migration_stems.assert_called_once()
    migration.audio.adopt_material_migration.assert_not_called()
    assert migration.migration.status == "material_verified"
    assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config
    work.state = "failed"
    migration.migration.poll()
    host_hash.assert_not_called()
    migration.audio.adopt_material_migration.assert_not_called()
    assert migration.migration.status == "failed"
    assert migration.controller.project.stem_cache[0] == entry
    assert migration.hold.release_count == 1


def test_preclaim_abort_waits_for_cancelled_stem_worker_to_finish_before_releasing_hold(
    migration: _Harness,
) -> None:
    entry = _old_stem_set()
    migration.controller.project.stem_cache[0] = entry
    work = _StemWork()
    migration.audio.prepare_material_migration_stems.return_value = work
    migration.begin()
    migration.migration.poll()
    assert migration.migration.status == "material_verified"
    migration.controller.project.stem_cache[0] = entry.model_copy(update={"available": False})
    migration.controller.persistence.mark_dirty()
    migration.migration.poll()
    assert work.cancel_count == migration.preparation.cancel_count == 1
    migration.audio.adopt_material_migration.assert_not_called()
    migration.assert_retained()
    migration.migration.poll()
    migration.assert_retained()
    work.state = "failed"
    migration.migration.poll()
    assert migration.migration.status == "failed"
    assert migration.preparation.abort_count == migration.hold.release_count == 1
    assert migration.controller.persistence._migration_owner is None
    assert (Path(entry.cache_dir) / "vocals.wav").is_file()


@pytest.mark.parametrize("created", [False, True])
def test_preclaim_abort_retires_only_the_stem_generation_created_by_this_transaction(
    migration: _Harness, *, created: bool
) -> None:
    entry = _old_stem_set()
    migration.controller.project.stem_cache[0] = entry
    work = _StemWork(state="ready", created=created)
    target = Path(work.cache_reference())
    target.mkdir(parents=True)
    for kind in STEM_KINDS:
        (target / f"{kind}.wav").write_bytes((Path(entry.cache_dir) / f"{kind}.wav").read_bytes())
    version = f"{_NEW}|sha256-v1:{hashlib.sha256(Path(_OLD).read_bytes()).hexdigest()}"
    write_test_stem_marker(target, version)
    marker = (target / ".complete.json").read_bytes()
    migration.audio.prepare_material_migration_stems.return_value = work
    migration.audio.adopt_material_migration.side_effect = RuntimeError("source queue full")
    migration.begin()
    migration.migration.poll()
    assert migration.migration.status == "failed"
    retirements = migration.audio.retire_project_asset.call_args_list
    if created:
        assert len(retirements) == 1
        assert Path(retirements[0].args[0]) == target.absolute()
        assert retirements[0].kwargs == {"recursive": True}
    else:
        assert retirements == []
    assert (target / ".complete.json").read_bytes() == marker
    assert (Path(entry.cache_dir) / "vocals.wav").is_file()
    assert migration.hold.release_count == migration.preparation.abort_count == 1
    assert migration.controller.project.stem_cache[0] == entry


@pytest.mark.parametrize("replacement", ["new_uuid_same_path", "new_source"])
def test_newer_content_instance_or_source_skips_captured_subscriber(
    migration: _Harness, replacement: str
) -> None:
    migration.begin()
    project = migration.controller.project
    project.pad_content[215] = PadContentIdentity(instance_id="f" * 32)
    if replacement == "new_source":
        write_mono_pcm16_wav(Path("samples/replacement.wav"), 48_000)
        project.sample_paths[215] = "samples/replacement.wav"
    project.pad_key_intent[215] = project.pad_key_intent[215].corrected("Fm")
    migration.controller.persistence.mark_dirty()
    replacement_content = project.pad_content[215]
    replacement_key = project.pad_key_intent[215]
    replacement_path = project.sample_paths[215]
    migration.migration.poll()
    assert set(migration.sources) == {0}
    migration.acknowledge(0)
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    assert project.sample_paths[0] == _NEW
    saved = ProjectPersistence.from_config_path(
        migration.controller.persistence.config_path
    ).project
    assert saved.pad_content[215] == project.pad_content[215] == replacement_content
    assert saved.pad_key_intent[215] == project.pad_key_intent[215] == replacement_key
    assert saved.sample_paths[215] == project.sample_paths[215] == replacement_path
    assert migration.hold.release_count == 1


@pytest.mark.parametrize("phase", ["pending", "claimed"])
def test_replaced_subscriber_already_admitted_requires_actual_phase_settlement(
    migration: _Harness, phase: str
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.sources[215].state = phase
    project = migration.controller.project
    write_mono_pcm16_wav(Path("samples/replacement.wav"), 48_000)
    project.sample_paths[215] = "samples/replacement.wav"
    project.pad_content[215] = PadContentIdentity(instance_id="f" * 32)
    migration.controller.persistence.mark_dirty()
    migration.acknowledge(0)
    migration.migration.poll()
    assert project.sample_paths[215] == "samples/replacement.wav"
    assert project.pad_content[215] == PadContentIdentity(instance_id="f" * 32)
    if phase == "pending":
        assert migration.sources[215].phase() == "rejected"
        assert migration.migration.status == "config_committed"
        assert 215 not in migration.controller.session.loading_sample_ids
        saved = ProjectPersistence.from_config_path(
            migration.controller.persistence.config_path
        ).project
        assert saved.sample_paths[215] == "samples/replacement.wav"
        assert saved.pad_content[215] == project.pad_content[215]
    else:
        assert migration.migration.status == "unresolved"
        assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config
        migration.assert_retained()


@pytest.mark.parametrize("failure", ["write_error", "newer_revision_after_write"])
def test_failed_or_superseded_atomic_commit_retains_both_images_until_current_retry_settles(
    migration: _Harness, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    controller = migration.controller
    persistence = controller.persistence
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    actual_write = persistence._atomic_write_text

    def write(content: str) -> None:
        if failure == "write_error":
            message = "injected atomic writer failure"
            raise OSError(message)
        actual_write(content)
        controller.project.pad_key_intent[0] = controller.project.pad_key_intent[0].changed(
            extra_shift=-2
        )
        controller.project.pad_gain_db[215] = -9.0
        persistence.mark_dirty()

    monkeypatch.setattr(persistence, "_atomic_write_text", write)
    migration.migration.poll()
    assert migration.migration.status == "unresolved"
    assert controller.project.sample_paths[0] == controller.project.sample_paths[215] == _OLD
    migration.assert_retained()
    assert all(not lease.released for lease in migration.delivered.values())
    if failure == "write_error":
        assert persistence.config_path.read_bytes() == migration.initial_config
    else:
        saved = ProjectPersistence.from_config_path(persistence.config_path).project
        assert saved.sample_paths[0] == _NEW
        assert saved.pad_key_intent[0].extra_shift == 12
        assert controller.project.pad_key_intent[0].extra_shift == -2
        assert persistence._dirty
        assert any(
            not owner.released and Path(owner.path) == Path(_NEW).absolute()
            for owner in migration.owners
        )
    monkeypatch.setattr(persistence, "_atomic_write_text", actual_write)
    current_keys = list(controller.project.pad_key_intent)
    migration.migration.retry_commit()
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    assert migration.hold.release_count == migration.preparation.release_count == 1
    assert migration.preparation.abort_count == 0
    saved = ProjectPersistence.from_config_path(persistence.config_path).project
    assert saved.pad_key_intent == current_keys == controller.project.pad_key_intent
    assert saved.pad_gain_db == controller.project.pad_gain_db
    assert persistence._migration_owner is None


@pytest.mark.parametrize("result", ["accepted", "rejected", "superseded"])
def test_source_success_waits_for_fresh_matching_timing_ack_before_commit(
    migration: _Harness, monkeypatch: pytest.MonkeyPatch, result: str
) -> None:
    controller = migration.controller
    analysis = controller.project.sample_analysis[0]
    assert analysis is not None
    analysis.accepted_timing = PersistedAcceptedTiming(
        schema_version=1,
        encoding="accepted-constant-timing-qm-raw-v1",
        record={"controller_test": "historical envelope; no native validation claimed"},
    )
    controller.project.pad_timing_intent[0] = "automatic"
    controller.project.manual_bpm[0] = None
    timing = _TimingTicket()
    future: Future[ConstantTimingTicket] = Future()
    prepare = Mock(return_value=future)
    monkeypatch.setattr(controller.loader._accepted_restore, "prepare_for_migration", prepare)
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    migration.migration.poll()
    prepare.assert_called_once_with(0, analysis.accepted_timing, _NEW)
    assert migration.migration.status == "adoption_pending"
    migration.assert_retained()
    future.set_result(cast("ConstantTimingTicket", timing))
    migration.migration.poll()
    assert migration.migration.status == "adoption_pending"
    migration.assert_retained()
    timing.state = "rejected" if result == "rejected" else "accepted"
    migration.audio.current_constant_timing.return_value = {
        "revision": "other-timing" if result == "superseded" else "fresh-timing",
        "accepted_request_id": 77,
    }
    migration.migration.poll()
    if result == "accepted":
        assert migration.migration.status == "config_committed"
        assert migration.hold.release_count == 1
        saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
        assert saved.pad_timing_intent[0] == "automatic"
        assert saved.pad_key_intent[0].analysis_epoch == MAX_KEY_EPOCH
    else:
        assert migration.migration.status == "unresolved"
        assert controller.project.sample_paths[0] == _OLD
        assert controller.persistence.config_path.read_bytes() == migration.initial_config
        migration.assert_retained()


def test_each_poll_admits_at_most_eight_independent_source_requests(migration: _Harness) -> None:
    for sample_id in range(1, 17):
        migration.seed(sample_id)
    migration.controller._assets.sync_assignments()
    migration.begin()
    counts = []
    for _ in range(3):
        migration.migration.poll()
        counts.append(len(migration.sources))
    assert counts == [8, 16, 18]
    assert set(migration.sources) == {*range(17), 215}
    assert len({ticket.request_id for ticket in migration.sources.values()}) == 18
    assert migration.migration.status == "adoption_pending"
    migration.assert_retained()


def test_new_assignment_owner_admission_failure_precedes_any_config_write(
    migration: _Harness, monkeypatch: pytest.MonkeyPatch
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    acquire = cast(
        "Callable[[str], FakeProjectAssetLease]",
        migration.audio.acquire_project_asset_lease.side_effect,
    )

    def saturated(path: str) -> FakeProjectAssetLease:
        if Path(path) == Path(_NEW).absolute():
            message = "native owner registry full"
            raise RuntimeError(message)
        return acquire(path)

    migration.audio.acquire_project_asset_lease.side_effect = saturated
    write = Mock(wraps=migration.controller.persistence._atomic_write_text)
    monkeypatch.setattr(migration.controller.persistence, "_atomic_write_text", write)
    migration.migration.poll()
    write.assert_not_called()
    assert migration.migration.status == "unresolved"
    assert migration.controller.project.sample_paths[0] == _OLD
    assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config
    migration.assert_retained()


def test_startup_reaches_one_legacy_material_after_restore_without_bulk_migration(
    migration: _Harness,
) -> None:
    service = migration.migration
    service.schedule_after_restore()
    migration.controller.session.loading_sample_ids.add(0)
    service.poll()
    migration.audio.prepare_material_migration.assert_not_called()
    migration.controller.session.loading_sample_ids.clear()
    service.poll()
    migration.audio.prepare_material_migration.assert_called_once_with(_OLD)
    migration.acknowledge()
    service.poll()
    assert service.status == "config_committed"
    service.poll()
    migration.audio.prepare_material_migration.assert_called_once()


def test_unavailable_stem_intent_migrates_without_copy_or_separation(
    migration: _Harness,
) -> None:
    entry = StemCacheEntry(
        source_version="old historical source",
        cache_dir="samples/stems/#1",
        stems=expected_stem_files("samples/stems/#1"),
        available=False,
    )
    project = migration.controller.project
    project.stem_cache[0] = entry
    project.pad_stem_mix_mode[0] = "all_stems"
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    migration.audio.prepare_material_migration_stems.assert_not_called()
    migrated = project.stem_cache[0]
    assert migrated is not None
    assert not migrated.available
    assert migrated.cache_dir == entry.cache_dir
    assert project.pad_stem_mix_mode[0] == "all_stems"
    assert Path(_OLD).is_file()


@pytest.mark.parametrize("stage", ["journal_constructor", "journal_append", "journal_dto"])
def test_early_begin_failure_releases_exact_new_fence_and_no_old_resources(
    migration: _Harness,
    monkeypatch: pytest.MonkeyPatch,
    stage: str,
) -> None:
    class FailingStore:
        def __init__(self, *_args: object) -> None:
            if stage == "journal_constructor":
                message = "journal constructor failure"
                raise ValueError(message)

        def append(self, _content: str) -> str:
            message = "journal append failure"
            raise ValueError(message)

    if stage == "journal_dto":

        def bad_journal(**_kwargs: object) -> None:
            message = "journal DTO failure"
            raise ValueError(message)

        monkeypatch.setattr(migration_module, "MaterialMigrationJournal", bad_journal)
    else:
        monkeypatch.setattr(migration_module, "MaterialMigrationJournalStore", FailingStore)
    with pytest.raises(ValueError, match="journal"):
        migration.migration.begin(0)
    assert migration.controller.persistence._migration_owner is None
    assert migration.controller._assets._reserved == 0
    assert migration.hold.release_count == 0
    assert migration.preparation.abort_count == 0
    assert migration.controller.persistence.config_path.read_bytes() == migration.initial_config


def test_interrupted_journal_reopens_latest_intent_with_fresh_fences_and_no_old_ack(
    migration: _Harness,
) -> None:
    migration.begin()
    migration.migration.poll()
    project = migration.controller.project
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=17)
    migration.controller.persistence.mark_dirty()
    migration.migration.shut_down()
    reopened = ProjectPersistence.from_config_path(migration.controller.persistence.config_path)
    reopened.config_path = migration.controller.persistence.config_path
    assets = ProjectAssetLifecycle(reopened.project, migration.audio)
    assets.sync_assignments()
    before_admissions = migration.audio.adopt_material_migration.call_count
    service = migration_module.MaterialMigrationController(
        reopened, migration.controller.session, migration.audio, assets, migration.controller.loader
    )
    assert service.status == "unresolved"
    assert reopened.project.pad_key_intent[0].extra_shift == 17
    assert reopened.project.pad_key_intent[0].source == project.pad_key_intent[0].source
    assert reopened.project.pad_content == project.pad_content
    assert reopened._migration_owner is not None
    assert not reopened.flush_if_dirty()
    assert migration.audio.adopt_material_migration.call_count == before_admissions
    assert not service._subscribers
    assert Path(_OLD).is_file()
    assert Path(_NEW).is_file()


def test_second_begin_store_failure_does_not_touch_the_completed_journal(
    migration: _Harness,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    migration.migration.poll()
    records = {
        path: path.read_bytes() for path in Path("samples/.material-migrations").rglob("*.json")
    }
    old_release_count = migration.preparation.release_count

    def fail_store(*_args: object) -> None:
        message = "new journal constructor failed"
        raise ValueError(message)

    monkeypatch.setattr(migration_module, "MaterialMigrationJournalStore", fail_store)
    with pytest.raises(ValueError, match="new journal"):
        migration.migration.begin(0)
    assert migration.controller.persistence._migration_owner is None
    assert migration.preparation.release_count == old_release_count
    assert migration.controller._assets._reserved == 0
    assert {path: path.read_bytes() for path in records} == records


def test_committed_journal_does_not_fence_a_later_saved_performer_edit(
    migration: _Harness,
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    project = migration.controller.project
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(extra_shift=16)
    migration.controller.persistence.mark_dirty()
    migration.controller.persistence.flush()
    result = inspect_migration_recovery(
        Path.cwd() / "samples", migration.controller.persistence.config_path
    )
    assert not result.errors
    assert not result.pending
    assert result.intent is None


def test_committed_config_and_journal_reopen_in_a_genuine_new_python_process(
    migration: _Harness,
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    migration.migration.poll()
    before = migration.controller.project
    code = """
import json
from pathlib import Path
from flitzis_looper.models import ProjectState
from flitzis_looper_audio import MaterialMigrationJournalStore
state = ProjectState.model_validate_json(Path('samples/flitzis_looper.config.json').read_bytes())
print(json.dumps({'content': [state.pad_content[i].model_dump() for i in (0,215)],
'keys': [state.pad_key_intent[i].model_dump() for i in (0,215)],
'aliases': list(state.material_migrations),
'journals': [json.loads(value) for value in
MaterialMigrationJournalStore.scan(str(Path('samples').resolve()))]}))
"""
    result = subprocess.run(
        [sys.executable, "-c", code],
        check=True,
        text=True,
        capture_output=True,
        encoding="utf-8",
        timeout=30,
    )
    reopened = json.loads(result.stdout)
    expected_content = []
    for index in _IDS:
        content = before.pad_content[index]
        assert content is not None
        expected_content.append(content.model_dump())
    assert reopened["content"] == expected_content
    assert reopened["keys"] == [before.pad_key_intent[i].model_dump() for i in _IDS]
    assert reopened["aliases"] == [migration.transaction]
    assert all(item["error"] is None for item in reopened["journals"])
    records = [json.loads(item["record"]) for item in reopened["journals"]]
    assert all(item["phase"] == "config_committed" for item in records)
    assert all("source_ticket" not in item for item in records)
    # Native lifetime/ACK remains absent: this child deliberately creates no AudioEngine.


@pytest.mark.parametrize("event_type", ["success", "error", "started", "progress"])
def test_old_migration_feedback_cannot_change_a_newer_regular_load(
    migration: _Harness,
    event_type: str,
) -> None:
    migration.begin()
    migration.migration.poll()
    source = migration.sources[0]
    loader = migration.controller.loader
    loader._load_request_ids[0] = source.request_id + 1000
    session = migration.controller.session
    session.loading_sample_ids.add(0)
    session.sample_load_stage[0] = "new regular load"
    session.sample_load_errors[0] = "new regular error"
    delivered = _DeliveredLease(str(Path(_NEW).absolute()))
    event = {
        "type": event_type,
        "id": 0,
        "request_id": source.request_id,
        "stage": "old progress",
        "error": "old error",
        "original_lease": delivered,
    }
    assert migration.migration.handle_loader_event(event)
    assert loader._load_request_ids[0] == source.request_id + 1000
    assert 0 in session.loading_sample_ids
    assert session.sample_load_stage[0] == "new regular load"
    assert session.sample_load_errors[0] == "new regular error"
    if event_type == "success":
        assert delivered.released
        assert not delivered.acknowledged


def test_new_regular_import_pending_before_new_uuid_is_not_committed_as_migrated(
    migration: _Harness,
) -> None:
    migration.begin()
    migration.migration.poll()
    migration.acknowledge()
    source = migration.sources[0]
    controller = migration.controller
    before = controller.project.pad_content[0]
    controller.loader._load_request_ids[0] = source.request_id + 1000
    controller.session.pending_sample_paths[0] = "performer-new-source.wav"
    controller.session.loading_sample_ids.add(0)
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    assert controller.project.sample_paths[0] == _OLD
    assert controller.project.pad_content[0] == before
    assert controller.project.sample_paths[215] == _NEW
    assert controller.loader._load_request_ids[0] == source.request_id + 1000
    assert controller.session.pending_sample_paths[0] == "performer-new-source.wav"
    assert 0 in controller.session.loading_sample_ids
    saved = ProjectPersistence.from_config_path(controller.persistence.config_path).project
    assert saved.sample_paths[0] == _OLD
    assert saved.pad_content[0] == before


def test_same_verified_stem_set_from_two_legacy_directories_commits_one_shared_target(
    migration: _Harness,
) -> None:
    project = migration.controller.project
    old_entries = [_old_stem_set(slot) for slot in (1, 216)]
    for sample_id, entry in zip(_IDS, old_entries, strict=True):
        project.stem_cache[sample_id] = entry
        project.pad_stem_mix_mode[sample_id] = "all_stems"
    works = [_StemWork(state="ready", created=True), _StemWork(state="ready", created=False)]
    target = Path(works[0].cache_reference())
    target.mkdir(parents=True)
    for kind in STEM_KINDS:
        (target / f"{kind}.wav").write_bytes(
            (Path(old_entries[0].cache_dir) / f"{kind}.wav").read_bytes()
        )
    version = f"{_NEW}|sha256-v1:{hashlib.sha256(Path(_NEW).read_bytes()).hexdigest()}"
    write_test_stem_marker(target, version)
    migration.audio.prepare_material_migration_stems.side_effect = works
    tickets: dict[int, Mock] = {}

    def capture(sample_id: int, _version: str) -> Mock:
        ticket = Mock()
        ticket.publication_status.return_value = "accepted"
        tickets[sample_id] = ticket
        return ticket

    migration.audio.capture_prepared_source.side_effect = capture
    migration.begin()
    migration.migration.poll()
    assert [project.stem_cache[i] for i in _IDS] == old_entries
    assert migration.audio.prepare_material_migration_stems.call_count == 2
    assert [
        call.args[1] for call in migration.audio.prepare_material_migration_stems.call_args_list
    ] == [entry.cache_dir for entry in old_entries]
    migration.acknowledge()
    migration.migration.poll()
    assert migration.migration.status == "config_committed"
    assert set(tickets) == set(_IDS)
    assert tickets[0] is not tickets[215]
    saved = ProjectPersistence.from_config_path(
        migration.controller.persistence.config_path
    ).project
    for sample_id in _IDS:
        saved_entry = saved.stem_cache[sample_id]
        assert saved_entry is not None
        assert saved_entry.cache_dir == target.as_posix()
        assert saved_entry.source_version == version
        assert saved.pad_stem_mix_mode[sample_id] == "all_stems"
    assert all(Path(entry.cache_dir).is_dir() for entry in old_entries)
    # Copy deduplication itself is proved by the real native WAV tests; these are fake tickets.


def test_identical_config_bytes_do_not_admit_another_projects_unsaved_journal_intent(
    migration: _Harness,
) -> None:
    persistence_a = migration.controller.persistence
    config_b = persistence_a.config_path.with_name("other-project.config.json")
    config_b.write_bytes(migration.initial_config)
    assert config_b.read_bytes() == persistence_a.config_path.read_bytes()
    migration.begin()
    migration.migration.poll()
    project_a = migration.controller.project
    project_a.pad_key_intent[0] = project_a.pad_key_intent[0].changed(extra_shift=17)
    persistence_a.mark_dirty()
    migration.migration.shut_down()
    assert migration.records()[-1].config_reference == persistence_a.config_reference

    persistence_b = ProjectPersistence.from_config_path(config_b)
    persistence_b.config_path = config_b
    persistence_b.project.pad_key_intent[0] = persistence_b.project.pad_key_intent[0].changed(
        extra_shift=-17
    )
    persistence_b.mark_dirty()
    before_b = persistence_b.project.model_copy(deep=True)
    assets_b = ProjectAssetLifecycle(persistence_b.project, migration.audio)
    assets_b.sync_assignments()
    holds_before = migration.audio.hold_material_migration.call_count
    service_b = migration_module.MaterialMigrationController(
        persistence_b, SessionState(), migration.audio, assets_b, migration.controller.loader
    )
    assert service_b.status == "idle"
    assert service_b.error is None
    assert persistence_b.project == before_b
    assert persistence_b._migration_owner is None
    assert migration.audio.hold_material_migration.call_count == holds_before
    assert config_b.read_bytes() == migration.initial_config

    reopened_a = ProjectPersistence.from_config_path(persistence_a.config_path)
    reopened_a.config_path = persistence_a.config_path
    assets_a = ProjectAssetLifecycle(reopened_a.project, migration.audio)
    assets_a.sync_assignments()
    service_a = migration_module.MaterialMigrationController(
        reopened_a, SessionState(), migration.audio, assets_a, migration.controller.loader
    )
    assert service_a.status == "unresolved"
    assert reopened_a.project.pad_key_intent[0].extra_shift == 17
    assert reopened_a._migration_owner is not None
    assert not service_a._subscribers


@pytest.mark.parametrize("binding", [None, "unrecognized-relative-config"])
def test_unknown_project_binding_keeps_journal_visible_and_acquires_fresh_fences(
    migration: _Harness, binding: str | None
) -> None:
    migration.begin()
    migration.migration.poll()
    journal = migration.records()[-1].changed(config_reference=binding)
    assert migration.migration._store is not None
    migration.migration._store.append(journal.model_dump_json())
    recovery = inspect_migration_recovery(
        Path.cwd() / "samples", migration.controller.persistence.config_path
    )
    assert recovery.errors
    assert recovery.intent is None
    reopened = ProjectPersistence.from_config_path(migration.controller.persistence.config_path)
    reopened.config_path = migration.controller.persistence.config_path
    assets = ProjectAssetLifecycle(reopened.project, migration.audio)
    assets.sync_assignments()
    service = migration_module.MaterialMigrationController(
        reopened, SessionState(), migration.audio, assets, migration.controller.loader
    )
    assert service.status == "unresolved"
    assert reopened._migration_owner is not None
    assert not service._subscribers
    assert migration.audio.hold_material_migration.call_args.args == (list(range(216)),)


@pytest.mark.parametrize("late_phase", ["acknowledged", "rejected"])
def test_later_genuine_native_phase_reconciles_a_retained_claim(
    migration: _Harness,
    late_phase: str,
) -> None:
    migration.begin()
    migration.migration.poll()
    for sample_id, source in migration.sources.items():
        source.state = "claimed"
        migration.migration.handle_loader_event({
            "type": "error",
            "id": sample_id,
            "request_id": source.request_id,
            "error": "claimed callback has no ACK yet",
        })
    migration.migration.poll()
    assert migration.migration.status == "unresolved"
    migration.assert_retained()
    for source in migration.sources.values():
        source.state = late_phase
    migration.migration.poll()
    if late_phase == "acknowledged":
        assert migration.migration.status == "config_committed"
        assert migration.controller.project.sample_paths[0] == _NEW
        assert migration.preparation.abort_count == 0
    else:
        assert migration.migration.status == "failed"
        assert migration.controller.project.sample_paths[0] == _OLD
        assert migration.preparation.abort_count == 1
    assert migration.hold.release_count == 1
    assert migration.controller.persistence._migration_owner is None
