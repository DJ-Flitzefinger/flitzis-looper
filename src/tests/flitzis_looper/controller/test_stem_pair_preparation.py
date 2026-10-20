"""Ordinary control races; native PCM, queue and callback proofs live in Rust."""

import json
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stem_generation import AudioShape, StemGenerationRequest
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.controller.stem_pair_preparation import (
    StemPairIntentBinding,
    StemPairPreparationQueue,
)
from flitzis_looper.key_intent import MAX_KEY_EPOCH, PadKeyIntent
from flitzis_looper.models import PadContentIdentity, ProjectState, StemCacheEntry
from flitzis_looper.stem_pair_selection import STEM_FINGERPRINT_SCHEMA, StemPairSelection
from tests.flitzis_looper.conftest import FakePreparedSourceTicket

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper_audio import AudioEngine, PreparedSourceTicket, PreparedStemPair


def _selection() -> dict[str, object]:
    root = f"samples/materials/M{'a' * 32}"
    return {
        "schema_version": 1,
        "descriptor_reference": f"{root}/.pcm-cache/stems/v1/.pairs/{'b' * 32}.json",
        "stem_set_identity": "b" * 64,
        "wav_generation": f"{root}/stems/.ready-{'c' * 32}",
        "pcm_generation": f"{root}/.pcm-cache/stems/v1/.ready-{'d' * 32}",
    }


class _PreparedPair:
    """Only the worker/control contract is replaced; no fake native ACK claim."""

    def __init__(
        self, *, components: bool = True, selection: dict[str, object] | None = None
    ) -> None:
        self.components = components
        self.selected = False
        self.discarded = False
        self.fail_discard = False
        self.selection = _selection() if selection is None else selection

    def selection_json(self) -> str:
        return json.dumps(self.selection)

    def has_components(self) -> bool:
        return self.components

    def select(self) -> None:
        self.selected = True

    def discard(self) -> None:
        if not self.selected and self.fail_discard:
            message = "native retirement queue full"
            raise RuntimeError(message)
        self.discarded = not self.selected


def _queued(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    components: bool = True,
) -> tuple[list[Callable[[], None]], _PreparedPair, FakePreparedSourceTicket, StemCacheEntry]:
    legacy = "samples/stems/#1/.ready-" + "e" * 32
    (tmp_path / legacy).mkdir(parents=True)
    (tmp_path / str(_selection()["wav_generation"])).mkdir(parents=True)
    controller.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    entry = StemCacheEntry(
        source_version="samples/old.wav|sha256-v1:" + "1" * 64,
        cache_dir=legacy,
        stems=expected_stem_files(legacy),
    )
    controller.project.stem_cache[0] = entry
    controller.project.pad_stem_mix_mode[0] = "all_stems"
    jobs: list[Callable[[], None]] = []
    controller.stems._pair_preparations._runner = jobs.append
    prepared = _PreparedPair(components=components)
    monkeypatch.setattr(audio, "prepare_stem_pair", Mock(return_value=prepared), raising=False)
    monkeypatch.setattr(audio, "publish_stem_pair", Mock(), raising=False)
    monkeypatch.setattr(audio, "set_stem_pair_full_mix", Mock(), raising=False)
    ticket = FakePreparedSourceTicket("pending")
    audio.capture_prepared_source.return_value = ticket
    assert (
        controller.stems._prepare_pair(
            0, entry, cast("PreparedSourceTicket", ticket), components=components
        )
        is True
    )
    assert controller.project.stem_cache[0] is entry
    return jobs, prepared, ticket, entry


def test_queued_all_then_full_mix_selects_disk_pair_without_component_publication(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, prepared, _, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    assert controller.stems.set_stem_mix_mode(0, "full_mix") is True
    jobs[0]()
    controller.stems._poll_pair_preparations()

    audio_engine_mock.publish_stem_pair.assert_not_called()
    assert prepared.selected is True
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert 0 not in controller.stems._resident_pairs
    assert not controller.stems._pending_stem_publications
    assert controller._assets._reserved == 0
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.pair is not None
    assert entry.available


def test_current_all_uses_own_pending_then_accepted_feedback(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, prepared, ticket, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    jobs[0]()
    controller.stems._poll_pair_preparations()
    audio_engine_mock.publish_stem_pair.assert_called_once_with(prepared, ticket)
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.available is False
    assert prepared.selected is False
    assert 0 not in controller.stems._resident_pairs
    ticket.status = "accepted"
    controller.stems._poll_stem_publications()
    assert prepared.selected is True
    assert 0 in controller.stems._resident_pairs
    assert controller._assets._reserved == 0


def test_selected_disk_pair_uses_separate_registration_residency_and_effective_receipts(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, prepared, source_ticket, _ = _queued(
        controller, audio_engine_mock, monkeypatch, tmp_path, components=False
    )
    registration = Mock()
    registration.publication_status.return_value = "pending"
    registration.is_current.return_value = True
    receipt = Mock()
    receipt.residency_status.return_value = "pending"
    receipt.effective_mode_status.return_value = "waiting"
    receipt.is_current.return_value = True
    monkeypatch.setattr(
        audio_engine_mock,
        "register_committed_stem_pair",
        Mock(return_value=registration),
        raising=False,
    )
    monkeypatch.setattr(
        audio_engine_mock,
        "request_committed_stem_residency",
        Mock(return_value=receipt),
        raising=False,
    )
    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    jobs[0]()
    controller.stems._poll_pair_preparations()
    audio_engine_mock.register_committed_stem_pair.assert_called_once_with(prepared, source_ticket)
    assert prepared.selected
    entry = controller.project.stem_cache[0]
    assert entry is not None
    monkeypatch.setattr(
        controller.stems, "_current_prepared_source_version", lambda _: entry.source_version
    )
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    audio_engine_mock.request_committed_stem_residency.assert_not_called()
    assert len(jobs) == 1
    registration.publication_status.return_value = "accepted"
    controller.stems._committed_residency.poll()
    audio_engine_mock.request_committed_stem_residency.assert_called_once_with(
        registration, mode="all_stems", enabled_mask=15
    )
    audio_engine_mock.publish_stem_pair.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")
    receipt.residency_status.return_value = "accepted"
    receipt.effective_mode_status.return_value = "transitioning"
    controller.stems._committed_residency.poll()
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.residency == "accepted"
    assert status.effective == "transitioning"
    receipt.effective_mode_status.return_value = "effective"
    controller.stems._committed_residency.poll()
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.effective == "effective"
    assert entry.available


def _known_pair_entry() -> StemCacheEntry:
    selected = _selection()
    selected["provenance"] = {
        "schema": STEM_FINGERPRINT_SCHEMA,
        "generation_fingerprint": "4" * 64,
        "producer_identity": "5" * 64,
    }
    return StemCacheEntry(
        source_version="samples/origin.wav|sha256-v1:" + "1" * 64,
        cache_dir=str(selected["wav_generation"]),
        stems=expected_stem_files(str(selected["wav_generation"])),
        pair=StemPairSelection.model_validate(selected),
        fingerprint_schema=STEM_FINGERPRINT_SCHEMA,
        generation_fingerprint="4" * 64,
        producer_identity="5" * 64,
    )


def _metadata_queue(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    entry: StemCacheEntry,
    selected: dict[str, object],
    *,
    binding: StemPairIntentBinding | None = None,
    processing_request: StemGenerationRequest | None = None,
) -> tuple[StemPairPreparationQueue, list[Callable[[], None]], FakePreparedSourceTicket]:
    (tmp_path / entry.cache_dir).mkdir(parents=True, exist_ok=True)
    jobs: list[Callable[[], None]] = []
    queue = StemPairPreparationQueue(cast("AudioEngine", audio), controller._assets, jobs.append)
    prepared = _PreparedPair(components=False, selection=selected)
    monkeypatch.setattr(audio, "prepare_stem_pair", Mock(return_value=prepared), raising=False)
    ticket = FakePreparedSourceTicket("pending")
    queue.submit(
        0,
        entry,
        cast("PreparedSourceTicket", ticket),
        "f" * 32,
        None,
        components=False,
        binding=binding,
        processing_request=processing_request,
    )
    return queue, jobs, ticket


def test_cross_root_preparation_retains_own_ticket_and_native_verified_new_identity(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    entry = _known_pair_entry()
    assert entry.pair is not None
    selected = entry.pair.model_dump(mode="json")
    for field_name in ("descriptor_reference", "wav_generation", "pcm_generation"):
        selected[field_name] = selected[field_name].replace("M" + "a" * 32, "M" + "2" * 32)
    selected["stem_set_identity"] = "3" * 64
    own_version = "samples/copy.wav|sha256-v1:" + "1" * 64
    intent = cast("StemGenerationIntent", Mock())
    capture = Mock(side_effect=AssertionError("Pair preparation must not recapture a sealed job"))
    monkeypatch.setattr(
        "flitzis_looper.controller.stem_pair_preparation.capture_processing_files", capture
    )
    queue, jobs, ticket = _metadata_queue(
        controller,
        audio_engine_mock,
        monkeypatch,
        tmp_path,
        entry,
        selected,
        binding=StemPairIntentBinding(
            generation_intent=intent,
            own_source_version=own_version,
            artifact_source_version=entry.source_version,
            source_path="samples/copy.wav",
        ),
    )
    jobs[0]()
    (request,) = queue.collect()
    assert request.error is None
    assert request.generation_intent is intent
    assert request.source_path == "samples/copy.wav"
    assert audio_engine_mock.prepare_stem_pair.call_args.args == (
        0,
        own_version,
        entry.cache_dir,
        ticket,
        False,
        entry.pair.descriptor_reference,
        entry.source_version,
    )
    result = queue.selected_entry(request)
    assert result.source_version == own_version
    assert result.fingerprint_schema == STEM_FINGERPRINT_SCHEMA
    assert result.pair is not None
    assert result.pair.provenance == entry.pair.provenance
    assert result.pair.stem_set_identity != entry.pair.stem_set_identity
    capture.assert_not_called()
    request.close()


def test_cross_root_without_explicit_artifact_version_fails_before_reservation(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    queue = StemPairPreparationQueue(
        cast("AudioEngine", audio_engine_mock), controller._assets, Mock()
    )
    with pytest.raises(ValueError, match="explicit artifact source version"):
        queue.submit(
            0,
            _known_pair_entry(),
            cast("PreparedSourceTicket", FakePreparedSourceTicket("pending")),
            "f" * 32,
            None,
            components=False,
            binding=StemPairIntentBinding(
                own_source_version="samples/copy.wav|sha256-v1:" + "1" * 64
            ),
        )
    assert not queue.pending
    assert controller._assets._reserved == 0


@pytest.mark.parametrize("known", [False, True])
def test_new_pair_or_legacy_restore_uses_saved_metadata_without_model_io(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    known: bool,
) -> None:
    if known:
        entry = _known_pair_entry().model_copy(update={"pair": None})
        selected = _known_pair_entry().pair
        assert selected is not None
        data = selected.model_dump(mode="json")
    else:
        data = _selection()
        entry = StemCacheEntry(
            source_version="saved-version",
            cache_dir=str(data["wav_generation"]),
            stems=expected_stem_files(str(data["wav_generation"])),
            pair=StemPairSelection.model_validate(data),
        )
    capture = Mock(side_effect=AssertionError("Restore/new generation must not recapture producer"))
    monkeypatch.setattr(
        "flitzis_looper.controller.stem_pair_preparation.capture_processing_files", capture
    )
    queue, jobs, ticket = _metadata_queue(
        controller, audio_engine_mock, monkeypatch, tmp_path, entry, data
    )
    jobs[0]()
    (request,) = queue.collect()
    assert request.error is None
    assert request.own_source_version == entry.source_version
    assert request.artifact_source_version == entry.source_version
    assert audio_engine_mock.prepare_stem_pair.call_args.args[3] is ticket
    assert audio_engine_mock.prepare_stem_pair.call_args.args[-1] is None
    result = queue.selected_entry(request)
    assert result.fingerprint_schema == entry.fingerprint_schema
    assert result.generation_fingerprint == entry.generation_fingerprint
    assert result.producer_identity == entry.producer_identity
    assert result.pair is not None
    assert ("provenance" in result.pair.model_dump(mode="json")) is known
    capture.assert_not_called()
    request.close()


@pytest.mark.parametrize(
    "fault", ["fingerprint", "producer", "missing", "legacy_attestation", "same_root_rebind"]
)
def test_preparation_rejects_native_provenance_or_same_root_selection_change(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    fault: str,
) -> None:
    entry = _known_pair_entry()
    assert entry.pair is not None
    selected = entry.pair.model_dump(mode="json")
    if fault == "missing":
        selected.pop("provenance")
    elif fault == "legacy_attestation":
        entry = entry.model_copy(update={"fingerprint_schema": None, "pair": None})
    elif fault == "same_root_rebind":
        selected["stem_set_identity"] = "6" * 64
    else:
        selected["provenance"][
            "generation_fingerprint" if fault == "fingerprint" else "producer_identity"
        ] = "6" * 64
    queue, jobs, _ = _metadata_queue(
        controller, audio_engine_mock, monkeypatch, tmp_path, entry, selected
    )
    jobs[0]()
    (request,) = queue.collect()
    message = "selection changed" if fault == "same_root_rebind" else "provenance differs"
    with pytest.raises(RuntimeError, match=message):
        queue.selected_entry(request)
    request.close()


@pytest.mark.parametrize(
    "fault", ["fingerprint", "marker_schema", "producer", "legacy_schema", "none"]
)
def test_explicit_compatible_reuse_checks_full_request_and_marker_schema(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    fault: str,
) -> None:
    entry = _known_pair_entry()
    processing_request = StemGenerationRequest(
        sample_id=0,
        source_path=tmp_path / "source.wav",
        source_version=entry.source_version,
        cache_dir=tmp_path / entry.cache_dir,
        target_shape=AudioShape(44100, 2, 64),
        model_cache_dir=tmp_path / "models",
        original_bytes=256,
    )
    fingerprint = request_fingerprint(processing_request, "5" * 64)
    assert fingerprint is not None
    entry = entry.model_copy(update={"generation_fingerprint": fingerprint, "pair": None})
    selected = _selection()
    selected["provenance"] = {
        "schema": STEM_FINGERPRINT_SCHEMA,
        "generation_fingerprint": fingerprint,
        "producer_identity": "5" * 64,
    }
    if fault == "fingerprint":
        entry = entry.model_copy(update={"generation_fingerprint": "7" * 64})
    elif fault == "legacy_schema":
        entry = entry.model_copy(update={"fingerprint_schema": None})
    guard = Mock()
    capture = Mock(return_value=(guard, "6" * 64 if fault == "producer" else "5" * 64))
    monkeypatch.setattr(
        "flitzis_looper.controller.stem_pair_preparation.capture_processing_files", capture
    )
    artifact_lease = Mock()
    artifact = Mock()
    artifact.capture.return_value = artifact_lease
    monkeypatch.setattr(
        "flitzis_looper.controller.stem_pair_preparation.MigrationArtifactLease", artifact
    )
    queue, jobs, _ = _metadata_queue(
        controller,
        audio_engine_mock,
        monkeypatch,
        tmp_path,
        entry,
        selected,
        processing_request=processing_request,
    )
    marker = {
        "fingerprint_schema": STEM_FINGERPRINT_SCHEMA,
        "generation_fingerprint": fingerprint,
        "producer_identity": "5" * 64,
    }
    if fault == "marker_schema":
        marker.pop("fingerprint_schema")
    (tmp_path / entry.cache_dir / ".complete.json").write_text(json.dumps(marker), encoding="utf-8")
    jobs[0]()
    (request,) = queue.collect()
    if fault == "none":
        assert request.error is None
        assert queue.selected_entry(request).generation_fingerprint == fingerprint
        guard.verify.assert_called_once()
    else:
        assert request.error is not None
        assert request.compatibility_rejected
        audio_engine_mock.prepare_stem_pair.assert_not_called()
    capture.assert_called_once_with(processing_request)
    if fault in {"marker_schema", "none"}:
        artifact_lease.release.assert_called_once()
    request.close()


def test_failed_committed_registration_never_falls_back_to_unbound_mode_command(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, prepared, ticket, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    monkeypatch.setattr(
        audio_engine_mock,
        "register_committed_stem_pair",
        Mock(side_effect=RuntimeError("selection queue full")),
        raising=False,
    )
    monkeypatch.setattr(
        audio_engine_mock, "request_committed_stem_residency", Mock(), raising=False
    )
    jobs[0]()
    controller.stems._poll_pair_preparations()
    ticket.status = "accepted"
    controller.stems._poll_stem_publications()
    assert prepared.selected
    assert "selection queue full" in controller.session.stem_generation_errors[0]
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    audio_engine_mock.request_committed_stem_residency.assert_not_called()


@pytest.mark.parametrize("late_all", [False, True])
def test_ordered_full_mix_release_survives_late_ack_and_next_all_prepares_fresh(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    late_all: bool,
) -> None:
    jobs, prepared, ticket, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    jobs[0]()
    controller.stems._poll_pair_preparations()
    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    audio_engine_mock.set_stem_pair_full_mix.assert_called_once_with(0)
    audio_engine_mock.set_stem_pair_full_mix.side_effect = RuntimeError("queue full")
    if late_all:
        controller.project.pad_stem_mix_mode[0] = "all_stems"
    ticket.status = "accepted"
    controller.stems._poll_stem_publications()
    assert prepared.selected
    assert 0 not in controller.stems._resident_pairs
    audio_engine_mock.set_stem_pair_full_mix.assert_called_once_with(0)
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.pair is not None
    monkeypatch.setattr(
        controller.stems, "_current_prepared_source_version", lambda _: entry.source_version
    )
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    assert len(jobs) == 2
    assert controller.stems._pair_preparations.pending[0].entry is entry


@pytest.mark.parametrize("boundary", ["worker_result", "publication_rejection"])
def test_discard_queue_failure_is_visible_reachable_and_reservations_close(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    boundary: str,
) -> None:
    jobs, prepared, ticket, entry = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    jobs[0]()
    prepared.fail_discard = True
    if boundary == "worker_result":
        controller.project.pad_content[0] = PadContentIdentity(instance_id="2" * 32)
    controller.stems._poll_pair_preparations()
    if boundary == "publication_rejection":
        ticket.status = "rejected"
        ticket.reason = "invalid-geometry"
        controller.stems._poll_stem_publications()
    assert controller.project.stem_cache[0] is entry
    assert not controller.stems._pending_stem_publications
    assert controller._assets._reserved == 0
    retries = controller.stems._pair_preparations._discard_retries
    assert retries[id(prepared)] == (0, cast("PreparedStemPair", prepared))
    assert "native retirement queue full" in controller.session.stem_generation_errors[0]
    with pytest.raises(RuntimeError, match="cleanup admission"):
        controller.stems._prepare_pair(0, entry, cast("PreparedSourceTicket", ticket))
    assert controller._assets._reserved == 0
    prepared.fail_discard = False
    controller.stems._poll_pair_preparations()
    assert not retries
    assert prepared.discarded is True


def test_cancelled_worker_retains_readers_until_actual_return(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, prepared, _, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    request = controller.stems._pair_preparations.pending[0]
    controller.stems._pair_preparations.cancel(0)
    assert controller._assets._reserved == 16
    assert not request.lease.released
    jobs[0]()
    controller.stems._poll_pair_preparations()
    audio_engine_mock.prepare_stem_pair.assert_not_called()
    assert not prepared.selected
    assert request.lease.released
    assert controller._assets._reserved == 0


@pytest.mark.parametrize("fault", ["schema", "runtime_right", "traversal", "material", "type"])
def test_malformed_pair_recovery_preserves_all_other_saved_intent_and_fences_disk(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    fault: str,
) -> None:
    monkeypatch.chdir(tmp_path)
    project = ProjectState(selected_pad=215, selected_bank=5, volume=0.4)
    project.pad_content[0] = PadContentIdentity(instance_id="e" * 32)
    project.pad_content[215] = PadContentIdentity(instance_id="f" * 32)
    project.pad_key_intent[215] = PadKeyIntent(
        correction="independent surviving text",
        correction_epoch=MAX_KEY_EPOCH,
        analysis_epoch=MAX_KEY_EPOCH,
        base_shift=6,
        extra_shift=-18,
        retrigger=True,
    )
    project.pad_stem_mix_mode[0] = "all_stems"
    project.stem_cache[0] = StemCacheEntry(
        source_version="saved-version",
        cache_dir=str(_selection()["wav_generation"]),
        stems=expected_stem_files(str(_selection()["wav_generation"])),
        available=True,
        pair=StemPairSelection.model_validate(_selection()),
    )
    data = project.model_dump(mode="json")
    pair = data["stem_cache"][0]["pair"]
    if fault == "schema":
        pair["schema_version"] = 2
    elif fault == "runtime_right":
        pair["accepted_request_id"] = 1
    elif fault == "traversal":
        pair["descriptor_reference"] = "samples/../foreign.json"
    elif fault == "material":
        pair["pcm_generation"] = pair["pcm_generation"].replace("a" * 32, "3" * 32)
    else:
        pair["stem_set_identity"] = True
    config = tmp_path / "samples" / "recovery.json"
    config.parent.mkdir()
    original = json.dumps(data).encode()
    config.write_bytes(original)
    loaded = ProjectPersistence.from_config_path(config)
    loaded.config_path = config
    expected = project.model_dump(mode="json")
    expected["stem_cache"][0]["pair"] = None
    expected["stem_cache"][0]["available"] = False
    assert loaded.project.model_dump(mode="json") == expected
    assert loaded.invalid_stem_pair_ids == {0}
    assert loaded.load_error == "Unsupported stem pair metadata retained on disk"
    loaded.mark_dirty()
    with pytest.raises(PersistenceFenceError, match="stem pair metadata"):
        loaded.flush()
    assert loaded._dirty
    assert config.read_bytes() == original


def test_discard_selected_pair_retains_disk_selection(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, prepared, _, _ = _queued(
        controller, audio_engine_mock, monkeypatch, tmp_path, components=False
    )
    jobs[0]()
    controller.stems._poll_pair_preparations()
    prepared.fail_discard = True
    controller.stems._discard_pair(0, cast("PreparedStemPair", prepared))
    assert prepared.selected is True
    assert prepared.discarded is False
    assert not controller.stems._pair_preparations._discard_retries


@pytest.mark.parametrize("late_full_mix", [False, True])
def test_explicit_all_retries_rejected_registration_with_fresh_pair_preparation(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    late_full_mix: bool,
) -> None:
    jobs, _, source, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path)
    registration = Mock(**{
        "publication_status.return_value": "pending",
        "is_current.return_value": True,
        "error.return_value": "selection callback rejected",
    })
    register = Mock(return_value=registration)
    residency = Mock()
    monkeypatch.setattr(audio_engine_mock, "register_committed_stem_pair", register, raising=False)
    monkeypatch.setattr(
        audio_engine_mock, "request_committed_stem_residency", residency, raising=False
    )
    jobs[0]()
    controller.stems._poll_pair_preparations()
    source.status = "accepted"
    controller.stems._poll_stem_publications()
    assert 0 in controller.stems._resident_pairs

    entry = controller.project.stem_cache[0]
    assert entry is not None
    monkeypatch.setattr(
        controller.stems, "_current_prepared_source_version", lambda _: entry.source_version
    )

    registration.publication_status.return_value = "rejected"
    registration.is_current.return_value = False
    controller.stems._committed_residency.poll()
    assert controller.session.stem_generation_errors[0] == "selection callback rejected"
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    assert len(jobs) == 2
    assert 0 in controller.stems._pair_preparations.pending
    assert controller.stems._pair_preparations.pending[0].residency_only
    registration.cancel.assert_called_once()
    residency.assert_not_called()
    audio_engine_mock.set_stem_mix_mode.assert_not_called()
    if late_full_mix:
        assert controller.stems.set_stem_mix_mode(0, "full_mix")

    fresh = Mock()
    fresh.publication_status.return_value = "pending"
    fresh.is_current.return_value = True
    register.return_value = fresh
    jobs[1]()
    controller.stems._poll_pair_preparations()
    controller.stems._poll_stem_publications()
    assert register.call_count == 2
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.registration == "pending"
    residency.assert_not_called()
    assert audio_engine_mock.prepare_stem_pair.call_args.args[4] is False
    audio_engine_mock.publish_stem_pair.assert_called_once()
    fresh.publication_status.return_value = "accepted"
    controller.stems._committed_residency.poll()
    if late_full_mix:
        residency.assert_not_called()
        assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    else:
        residency.assert_called_once_with(fresh, mode="all_stems", enabled_mask=15)


@pytest.mark.parametrize("release_fails", [False, True])
def test_rejected_full_mix_retry_uses_ordered_release_despite_saved_full_mix(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    release_fails: bool,
) -> None:
    jobs, _, _, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path, components=False)
    registration = Mock()
    registration.publication_status.return_value = "accepted"
    registration.is_current.return_value = True
    all_receipt = Mock()
    all_receipt.residency_status.return_value = "accepted"
    all_receipt.effective_mode_status.return_value = "effective"
    all_receipt.is_current.return_value = True
    full_receipt = Mock()
    full_receipt.residency_status.return_value = "pending"
    full_receipt.effective_mode_status.return_value = "waiting"
    full_receipt.is_current.return_value = True
    request = Mock(side_effect=[all_receipt, full_receipt, RuntimeError("typed queue full")])
    monkeypatch.setattr(
        audio_engine_mock,
        "register_committed_stem_pair",
        Mock(return_value=registration),
        raising=False,
    )
    monkeypatch.setattr(
        audio_engine_mock, "request_committed_stem_residency", request, raising=False
    )
    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    jobs[0]()
    controller.stems._poll_pair_preparations()
    entry = controller.project.stem_cache[0]
    assert entry is not None
    monkeypatch.setattr(
        controller.stems, "_current_prepared_source_version", lambda _: entry.source_version
    )
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert 0 not in controller.stems._resident_pairs
    full_receipt.residency_status.return_value = "rejected"
    full_receipt.effective_mode_status.return_value = "rejected"
    full_receipt.is_current.return_value = False
    if release_fails:
        audio_engine_mock.set_stem_pair_full_mix.side_effect = RuntimeError("release queue full")

    assert controller.stems.set_stem_mix_mode(0, "full_mix") is not release_fails
    audio_engine_mock.set_stem_pair_full_mix.assert_called_once_with(0)
    assert not controller.stems._committed_residency.contains(0)
    if release_fails:
        assert "release queue full" in controller.session.stem_generation_errors[0]
        assert 0 in controller.stems._pending_full_mix_releases
        audio_engine_mock.set_stem_pair_full_mix.side_effect = None
        assert controller.stems.set_stem_mix_mode(0, "full_mix")
        assert audio_engine_mock.set_stem_pair_full_mix.call_count == 2
    assert 0 not in controller.stems._pending_full_mix_releases
    audio_engine_mock.set_stem_mix_mode.assert_called_once_with(0, "full_mix")


def _restored_pair_after_rejected_full_mix(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> tuple[list[Callable[[], None]], StemCacheEntry, Mock, Mock, Mock]:
    """Unit receipt doubles exercise fresh admission, without claiming Native ACK/audio."""
    source_path = tmp_path / "samples" / "old.wav"
    source_path.parent.mkdir(parents=True, exist_ok=True)
    source_path.write_bytes(b"declared controller source")
    controller.project.sample_paths[0] = "samples/old.wav"
    jobs, _, _, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path, components=False)
    controller.project.pad_stem_mix_mode[0] = "full_mix"
    jobs[0]()
    controller.stems._poll_pair_preparations()
    entry = controller.project.stem_cache[0]
    assert entry is not None
    assert entry.pair is not None
    saved = entry.model_copy(update={"available": False})
    controller.project.stem_cache[0] = saved
    controller.stems._restored_stem_candidates[0] = saved
    registration = Mock(**{
        "publication_status.return_value": "accepted",
        "is_current.return_value": True,
        "error.return_value": "selection callback rejected",
    })
    fresh_registration = Mock(**{
        "publication_status.return_value": "pending",
        "is_current.return_value": True,
    })
    register = Mock(side_effect=[registration, fresh_registration])
    receipt = Mock(**{
        "residency_status.return_value": "pending",
        "effective_mode_status.return_value": "waiting",
        "is_current.return_value": True,
        "error.return_value": None,
    })
    residency = Mock(return_value=receipt)
    monkeypatch.setattr(audio_engine_mock, "register_committed_stem_pair", register)
    monkeypatch.setattr(audio_engine_mock, "request_committed_stem_residency", residency)
    assert controller.stems.publish_restored_stem_cache_if_available(0)
    controller.stems._poll_residency_demands()
    assert len(jobs) == 2
    jobs[1]()
    controller.stems._poll_pair_preparations()
    assert controller.project.stem_cache[0] is saved
    assert not saved.available
    assert controller.stems.stems_available(0)
    register.assert_called_once()
    residency.assert_not_called()

    registration.publication_status.return_value = "rejected"
    registration.is_current.return_value = False
    assert controller.stems.set_stem_mix_mode(0, "full_mix")
    registration.cancel.assert_called_once()
    audio_engine_mock.set_stem_pair_full_mix.assert_called_once_with(0)
    assert not controller.stems._committed_residency.contains(0)
    assert not controller.stems.stems_available(0)
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"

    # The ordinary availability query stays real: no current registration means
    # this restored disk entry cannot grant readiness to the explicit retry.
    assert controller.stems._current_prepared_source_version(0) is None
    return jobs, saved, register, residency, fresh_registration


def test_unavailable_restored_pair_retries_after_public_full_mix_registration_rejection(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    jobs, saved, register, residency, fresh_registration = _restored_pair_after_rejected_full_mix(
        controller, audio_engine_mock, monkeypatch, tmp_path
    )
    source_ticket = FakePreparedSourceTicket()
    capture = audio_engine_mock.capture_prepared_source
    capture.reset_mock()
    capture.return_value = source_ticket
    fresh_prepared = _PreparedPair(components=False)
    audio_engine_mock.prepare_stem_pair.reset_mock()
    audio_engine_mock.prepare_stem_pair.return_value = fresh_prepared

    assert controller.stems.can_request_all_stems(0)
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    capture.assert_called_once_with(0, saved.source_version)
    assert len(jobs) == 3
    pending = controller.stems._pair_preparations.pending[0]
    assert pending.entry is saved
    assert cast("object", pending.ticket) is source_ticket
    assert pending.residency_only
    assert controller.project.pad_stem_mix_mode[0] == "all_stems"
    assert not controller.stems.stems_available(0)
    register.assert_called_once()
    residency.assert_not_called()
    jobs[2]()
    controller.stems._poll_pair_preparations()
    register.assert_called_with(fresh_prepared, source_ticket)
    assert register.call_count == 2
    assert audio_engine_mock.prepare_stem_pair.call_args.args[4] is False
    audio_engine_mock.publish_stem_pair.assert_not_called()
    controller.stems._committed_residency.poll()
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.registration == "pending"
    assert status.residency is None
    assert status.effective is None
    residency.assert_not_called()
    fresh_registration.publication_status.return_value = "accepted"
    controller.stems._committed_residency.poll()
    residency.assert_called_once_with(fresh_registration, mode="all_stems", enabled_mask=15)
    status = controller.stems.stem_residency_status(0)
    assert status is not None
    assert status.registration == "accepted"
    assert status.residency == "pending"
    assert status.effective == "waiting"


@pytest.mark.parametrize("source_state", ["stale", "missing"])
def test_unavailable_restored_pair_retry_requires_current_loaded_source(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    source_state: str,
) -> None:
    jobs, saved, register, residency, _ = _restored_pair_after_rejected_full_mix(
        controller, audio_engine_mock, monkeypatch, tmp_path
    )
    capture = audio_engine_mock.capture_prepared_source
    capture.reset_mock()
    if source_state == "stale":
        capture.side_effect = ValueError("loaded source changed")
    else:
        controller.project.sample_paths[0] = None
    audio_engine_mock.prepare_stem_pair.reset_mock()

    assert controller.stems.can_request_all_stems(0) is (source_state != "missing")
    assert not controller.stems.set_stem_mix_mode(0, "all_stems")
    assert len(jobs) == 2
    assert 0 not in controller.stems._pair_preparations.pending
    assert not controller.stems._committed_residency.contains(0)
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    register.assert_called_once()
    residency.assert_not_called()
    audio_engine_mock.prepare_stem_pair.assert_not_called()
    audio_engine_mock.publish_stem_pair.assert_not_called()
    if source_state == "stale":
        capture.assert_called_once_with(0, saved.source_version)
        assert controller.session.stem_generation_errors[0] == "loaded source changed"
    else:
        capture.assert_not_called()


@pytest.mark.parametrize("saved_all", [False, True])
def test_restore_verifies_complete_pair_without_component_demand_with_preload_off(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    saved_all: bool,
) -> None:
    source = tmp_path / "samples" / "old.wav"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_bytes(b"declared controller source")
    controller.project.sample_paths[0] = "samples/old.wav"
    jobs, prepared, _, _entry = _queued(
        controller, audio_engine_mock, monkeypatch, tmp_path, components=False
    )
    # The fixture's first worker represents complete disk preparation, not residency.
    jobs[0]()
    controller.stems._poll_pair_preparations()
    restored_entry = controller.project.stem_cache[0]
    assert restored_entry is not None
    saved = restored_entry.model_copy(update={"available": False})
    controller.project.stem_cache[0] = saved
    controller.project.pad_stem_mix_mode[0] = "all_stems" if saved_all else "full_mix"
    controller.stems._restored_stem_candidates[0] = saved
    registration = Mock(**{
        "publication_status.return_value": "accepted",
        "is_current.return_value": True,
    })
    monkeypatch.setattr(
        audio_engine_mock, "register_committed_stem_pair", Mock(return_value=registration)
    )
    residency = Mock()
    monkeypatch.setattr(audio_engine_mock, "request_committed_stem_residency", residency)
    audio_engine_mock.prepare_stem_pair.reset_mock()
    assert controller.stems.publish_restored_stem_cache_if_available(0)
    controller.stems._poll_residency_demands()
    assert len(jobs) == 2
    jobs[1]()
    controller.stems._poll_pair_preparations()
    controller.stems._poll_residency_demands()
    assert audio_engine_mock.prepare_stem_pair.call_args.args[4] is False
    assert prepared.selected
    audio_engine_mock.publish_stem_pair.assert_not_called()
    residency.assert_not_called()
    assert controller.project.pad_stem_mix_mode[0] == ("all_stems" if saved_all else "full_mix")
    assert controller.stems.stems_available(0)
    if saved_all:
        receipt = Mock(**{
            "residency_status.return_value": "pending",
            "effective_mode_status.return_value": "waiting",
            "is_current.return_value": True,
        })
        residency.return_value = receipt
        assert controller.stems.set_stem_mix_mode(0, "all_stems")
        residency.assert_called_once_with(registration, mode="all_stems", enabled_mask=15)


def test_policy_toggle_preserves_direct_interest_and_uses_only_policy_withdrawal(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    source = tmp_path / "samples" / "old.wav"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_bytes(b"declared controller source")
    controller.project.sample_paths[0] = "samples/old.wav"
    jobs, _, _, _ = _queued(controller, audio_engine_mock, monkeypatch, tmp_path, components=False)
    registration = Mock(**{
        "publication_status.return_value": "accepted",
        "is_current.return_value": True,
    })
    receipt = Mock(**{
        "residency_status.return_value": "accepted",
        "effective_mode_status.return_value": "not_requested",
        "is_current.return_value": True,
    })
    monkeypatch.setattr(
        audio_engine_mock, "register_committed_stem_pair", Mock(return_value=registration)
    )
    request = Mock(return_value=receipt)
    monkeypatch.setattr(audio_engine_mock, "request_committed_stem_residency", request)
    jobs[0]()
    controller.stems._poll_pair_preparations()
    controller.project.pad_stem_mix_mode[0] = "full_mix"
    controller.settings.set_preload_stem_loops_on_startup(enabled=True)
    controller.stems._poll_residency_demands()
    request.assert_called_once_with(registration, mode="full_mix", interest="preload")
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.stems.residency_summary().acked == 1
    demand = controller.stems._demands[0]
    demand.explicit = True
    controller.settings.set_preload_stem_loops_on_startup(enabled=False)
    assert demand.explicit
    assert not demand.policy
    assert demand.withdraw_policy
    controller.stems._poll_residency_demands()
    assert any(call.kwargs.get("interest") == "withdraw_preload" for call in request.call_args_list)


def test_all216_demand_scheduling_is_bounded_fair_and_direct_request_is_not_starved(
    controller: AppController,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Control scheduling only; no claim of actual 216 PCM/adoption/resource acceptance."""
    pair = StemPairSelection.model_validate(_selection())
    for sample_id in range(216):
        entry = StemCacheEntry(
            source_version="saved-own-source",
            cache_dir=pair.wav_generation,
            stems=expected_stem_files(pair.wav_generation),
            pair=pair,
            available=True,
        )
        controller.project.stem_cache[sample_id] = entry
        controller.project.sample_paths[sample_id] = "samples/declared.wav"
        demand = controller.stems._remember_demand(sample_id, entry)
        demand.policy = True
    controller.stems._demands[215].explicit = True
    visited: list[int] = []

    def blocked(sample_id: int, _demand: object) -> None:
        visited.append(sample_id)
        message = "[resident_pcm_budget_exceeded] declared pressure"
        raise RuntimeError(message)

    monkeypatch.setattr(controller.stems, "_advance_residency_demand", blocked)
    controller.stems._poll_residency_demands()
    assert len(visited) == 8
    assert visited[0] == 215
    for _ in range(54):
        previous = len(visited)
        controller.stems._poll_residency_demands()
        assert len(visited) - previous <= 8
    assert set(visited) == set(range(216))
    assert len(controller.stems._demands) == 216
    assert controller.stems._demands[215].explicit
    assert all(entry is not None for entry in controller.project.stem_cache)


@pytest.mark.parametrize(
    ("message", "recoverable"),
    [
        ("cold source queue full (2 workers, 32 queued jobs)", True),
        ("cold source lane stopped", False),
        ("corrupt marker contains cold source queue full (2 workers, 32 queued jobs)", False),
    ],
)
def test_async_metadata_native_capacity_retries_complete_verification_without_assignment_loss(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    message: str,
    *,
    recoverable: bool,
) -> None:
    jobs, prepared, _, previous = _queued(
        controller, audio_engine_mock, monkeypatch, tmp_path, components=False
    )
    demand = controller.stems._remember_demand(0, previous)
    demand.verify_disk = True
    audio_engine_mock.prepare_stem_pair.side_effect = RuntimeError(message)
    jobs[0]()
    controller.stems._poll_pair_preparations()
    assert controller.project.stem_cache[0] is previous
    assert controller.project.pad_stem_mix_mode[0] == "all_stems"
    assert not controller.stems._pair_preparations.pending
    assert controller._assets._reserved == 0
    assert demand.state == ("queued" if recoverable else "error")
    audio_engine_mock.prepare_stem_pair.side_effect = None
    controller.stems._poll_residency_demands()
    assert len(jobs) == (2 if recoverable else 1)
    if recoverable:
        jobs[1]()
        controller.stems._poll_pair_preparations()
        controller.stems._poll_residency_demands()
        assert prepared.selected
        assert controller.stems.stems_available(0)
        assert demand.state == "verified"
        assert audio_engine_mock.prepare_stem_pair.call_count == 2
        assert all(
            call.args[4] is False for call in audio_engine_mock.prepare_stem_pair.call_args_list
        )
    else:
        assert audio_engine_mock.prepare_stem_pair.call_count == 1
    audio_engine_mock.publish_stem_pair.assert_not_called()


def _registered_metadata_pair(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> Mock:
    source = tmp_path / "samples" / "old.wav"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_bytes(b"declared controller source")
    controller.project.sample_paths[0] = "samples/old.wav"
    jobs, _, _, _ = _queued(controller, audio, monkeypatch, tmp_path, components=False)
    registration = Mock(**{
        "publication_status.return_value": "accepted",
        "is_current.return_value": True,
    })
    monkeypatch.setattr(audio, "register_committed_stem_pair", Mock(return_value=registration))
    monkeypatch.setattr(audio, "request_committed_stem_residency", Mock())
    jobs[0]()
    controller.stems._poll_pair_preparations()
    return registration


@pytest.mark.parametrize(
    ("message", "recoverable"),
    [
        ("cold source queue full (2 workers, 32 queued jobs)", True),
        (
            (
                "[resident_stem_interest_busy] explicit stem intent is preparing "
                "or awaiting effective acknowledgement"
            ),
            True,
        ),
        ("committed preparation deadline exceeded", False),
        ("corrupt source contains [resident_stem_interest_busy]", False),
    ],
)
def test_policy_native_admission_refusal_recovers_only_exact_capacity_then_own_ack(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    message: str,
    *,
    recoverable: bool,
) -> None:
    registration = _registered_metadata_pair(controller, audio_engine_mock, monkeypatch, tmp_path)
    receipt = Mock(**{
        "residency_status.return_value": "pending",
        "effective_mode_status.return_value": "not_requested",
        "is_current.return_value": True,
        "error.return_value": None,
    })
    request = audio_engine_mock.request_committed_stem_residency
    request.side_effect = [RuntimeError(message), receipt]
    controller.project.pad_stem_mix_mode[0] = "full_mix"
    controller.settings.set_preload_stem_loops_on_startup(enabled=True)
    controller.stems._poll_residency_demands()
    demand = controller.stems._demands[0]
    assert demand.state == ("queued" if recoverable else "error")
    assert demand.policy
    assert not demand.explicit
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    assert controller.stems.residency_summary().acked == 0
    controller.stems._poll_residency_demands()
    assert request.call_count == (2 if recoverable else 1)
    if recoverable:
        assert demand.state == "preparing"
        assert controller.stems.residency_summary().acked == 0
        receipt.residency_status.return_value = "accepted"
        controller.stems._poll_residency_demands()
        assert demand.state == "acked"
        assert controller.stems.residency_summary().acked == 1
        request.assert_called_with(registration, mode="full_mix", interest="preload")
    assert controller.project.pad_stem_mix_mode[0] == "full_mix"
    audio_engine_mock.publish_stem_pair.assert_not_called()


def test_pending_explicit_all_keeps_authority_and_serializes_policy_until_own_effect(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    registration = _registered_metadata_pair(controller, audio_engine_mock, monkeypatch, tmp_path)
    explicit = Mock(**{
        "residency_status.return_value": "pending",
        "effective_mode_status.return_value": "waiting",
        "is_current.return_value": True,
        "error.return_value": None,
    })
    policy = Mock(**{
        "residency_status.return_value": "accepted",
        "effective_mode_status.return_value": "not_requested",
        "is_current.return_value": True,
        "error.return_value": None,
    })
    request = audio_engine_mock.request_committed_stem_residency
    request.side_effect = [explicit, policy]
    assert controller.stems.set_stem_mix_mode(0, "all_stems")
    controller.settings.set_preload_stem_loops_on_startup(enabled=True)
    controller.stems._poll_residency_demands()
    assert request.call_count == 1
    explicit.residency_status.return_value = "accepted"
    explicit.effective_mode_status.return_value = "transitioning"
    controller.stems._poll_residency_demands()
    assert request.call_count == 1
    explicit.effective_mode_status.return_value = "effective"
    controller.stems._poll_residency_demands()
    assert request.call_count == 2
    request.assert_called_with(registration, mode="full_mix", interest="preload")
    assert explicit.is_current()
    explicit.cancel.assert_not_called()
    assert controller.stems.residency_summary().acked == 1


@pytest.mark.parametrize("recoverable", [True, False])
def test_public_all_request_native_lane_refusal_retains_interest_only_for_exact_capacity(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    recoverable: bool,
) -> None:
    registration = _registered_metadata_pair(controller, audio_engine_mock, monkeypatch, tmp_path)
    controller.project.pad_stem_mix_mode[0] = "full_mix"
    previous = controller.project.stem_cache[0]
    message = (
        "cold source queue full (2 workers, 32 queued jobs)"
        if recoverable
        else ("corrupt source contains cold source queue full (2 workers, 32 queued jobs)")
    )
    receipt = Mock(**{
        "residency_status.return_value": "pending",
        "effective_mode_status.return_value": "waiting",
        "is_current.return_value": True,
        "error.return_value": None,
    })
    request = audio_engine_mock.request_committed_stem_residency
    request.side_effect = [RuntimeError(message), receipt]
    assert controller.stems.set_stem_mix_mode(0, "all_stems") is recoverable
    demand = controller.stems._demands[0]
    assert demand.state == ("queued" if recoverable else "error")
    assert controller.project.stem_cache[0] is previous
    assert controller.project.pad_stem_mix_mode[0] == ("all_stems" if recoverable else "full_mix")
    controller.stems._poll_residency_demands()
    assert request.call_count == (2 if recoverable else 1)
    if recoverable:
        assert controller.stems.residency_summary().acked == 0
        receipt.residency_status.return_value = "accepted"
        receipt.effective_mode_status.return_value = "effective"
        controller.stems._poll_residency_demands()
        assert controller.stems.residency_summary().acked == 1
        request.assert_called_with(registration, mode="all_stems", enabled_mask=15)
    assert audio_engine_mock.prepare_stem_pair.call_count == 1
