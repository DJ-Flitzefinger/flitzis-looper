"""Ordinary control races; native PCM, queue and callback proofs live in Rust."""

import json
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.key_intent import MAX_KEY_EPOCH, PadKeyIntent
from flitzis_looper.models import PadContentIdentity, ProjectState, StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection
from tests.flitzis_looper.conftest import FakePreparedSourceTicket

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import PreparedSourceTicket, PreparedStemPair


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

    def __init__(self, *, components: bool = True) -> None:
        self.components = components
        self.selected = False
        self.discarded = False
        self.fail_discard = False

    def selection_json(self) -> str:
        return json.dumps(_selection())

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
    assert controller.stems._prepare_pair(0, entry, cast("PreparedSourceTicket", ticket)) is True
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
