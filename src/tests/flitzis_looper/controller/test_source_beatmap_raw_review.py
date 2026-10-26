"""Explicit interpretation intent over productive selection/publication boundaries."""

import json
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller.raw_source_beatmap_fakes import (
    FakeRawCandidate,
    raw_event,
    raw_job,
    review_job,
)
from tests.flitzis_looper.controller.test_source_beatmap_raw_selection import _raw_assigned
from tests.flitzis_looper.controller.test_source_beatmap_selection import _event, _job

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import AssetRetirementReservation
    from flitzis_looper.controller.source_beatmap_selection import SourceBeatmapSelection
    from flitzis_looper.source_beatmap_intent import PadSourceBeatmapIntent


def _candidate(
    controller: AppController, audio: Mock
) -> tuple[SourceBeatmapSelection, Mock, FakeRawCandidate]:
    maps = _raw_assigned(controller, audio)
    job, candidate = raw_job(audio), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    maps.raw_completed(0, raw_event(job, candidate))
    job.source_beatmap_async_status.return_value.update(
        stage="completed", terminal=True, raw_review_retired=False
    )
    return maps, job, candidate


def test_review_passes_only_exact_explicit_nonordinal_labels_and_original_candidate(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps, _raw, candidate = _candidate(controller, audio_engine_mock)
    review_job(audio_engine_mock)
    before = controller.project.model_dump()
    saved = controller._persistence.config_path.read_bytes()

    maps.review_raw(
        0, [(2, -0.0), (7, 0.1)], expected_attempt=1, expected_revision=candidate.revision
    )

    args, kwargs = audio_engine_mock.begin_source_beatmap_review_async.call_args
    assert args[:2] == (0, candidate)
    assert json.loads(args[2]) == {
        "schema_version": 1,
        "raw_revision": candidate.revision,
        "anchors": [
            {"raw_index": 2, "quarter_bits": "8000000000000000"},
            {"raw_index": 7, "quarter_bits": "3fb999999999999a"},
        ],
    }
    assert kwargs == {"expected_source_generation": 71}
    assert maps.status(0) == "pending"
    assert maps.raw_state(0).status == "unreviewed"
    assert controller.project.pad_source_beatmap_intent[0].selected is None
    before["pad_source_beatmap_intent"] = controller.project.model_dump()[
        "pad_source_beatmap_intent"
    ]
    assert controller.project.model_dump() == before
    assert controller._persistence.config_path.read_bytes() == saved
    audio_engine_mock.seek_sample.assert_not_called()


@pytest.mark.parametrize(
    "labels",
    [
        [],
        [(0, 0.0)],
        [(0, 0.0), (4, float("nan"))],
        [(0, 0.0), (4, float("inf"))],
        [(0, 0.0), (0, 1.0)],
        [(4, 0.0), (0, 1.0)],
        [(0, 1.0), (4, 1.0)],
        [(0, 1.0), (4, 0.0)],
        [(0, 0.0), (260, 1.0)],
        [(True, 0.0), (4, 1.0)],
        [(0, 0.0), (4, True)],
        [(0, 0), (4, 1.0)],
        [(index, float(index)) for index in range(65)],
        {"anchors": [], "unexpected": "x" * 8193},
    ],
)
def test_invalid_review_never_admits_or_changes_intent(
    controller: AppController, audio_engine_mock: Mock, labels: object
) -> None:
    maps, _raw, candidate = _candidate(controller, audio_engine_mock)
    review_job(audio_engine_mock)
    before = controller.project.model_dump()

    with pytest.raises(
        ValueError,
        match=(
            r"Enter between 2 and 64 explicit raw event/quarter-coordinate labels"
            r"|Raw indices and finite quarter coordinates must be strictly increasing"
        ),
    ):
        maps.review_raw(
            0,
            cast("list[tuple[int, float]]", labels),
            expected_attempt=1,
            expected_revision=candidate.revision,
        )

    audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()
    assert controller.project.model_dump() == before
    assert not maps._pending
    assert controller._assets._reserved == 0


def test_maximum_64_explicit_labels_stays_bounded_and_no_labels_are_filled(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps, _raw, candidate = _candidate(controller, audio_engine_mock)
    review_job(audio_engine_mock)
    maps.review_raw(
        0,
        [(index * 2, float(index) * 1.25) for index in range(64)],
        expected_attempt=1,
        expected_revision=candidate.revision,
    )
    encoded = audio_engine_mock.begin_source_beatmap_review_async.call_args.args[2]
    assert len(encoded.encode("utf-8")) <= 8192
    anchors = json.loads(encoded)["anchors"]
    assert len(anchors) == 64
    assert anchors[-1]["raw_index"] == 126


@pytest.mark.parametrize("change", ["attempt", "revision", "source", "pending", "native-pending"])
def test_stale_or_pending_review_never_reaches_accepted_admission(
    controller: AppController, audio_engine_mock: Mock, change: str
) -> None:
    maps, _raw, candidate = _candidate(controller, audio_engine_mock)
    review_job(audio_engine_mock)
    if change == "source":
        maps._source_generation = Mock(return_value=72)
    elif change == "pending":
        maps._source_generation = Mock(return_value=None)
    elif change == "native-pending":
        candidate.status = "placement_pending"
    before = controller.project.model_dump()
    with pytest.raises(
        ValueError,
        match=(
            r"The raw analysis source or attempt changed"
            r"|The original raw candidate is not ready for interpretation"
        ),
    ):
        maps.review_raw(
            0,
            [(0, 0.0), (4, 1.5)],
            expected_attempt=2 if change == "attempt" else 1,
            expected_revision="e" * 64 if change == "revision" else candidate.revision,
        )
    audio_engine_mock.begin_source_beatmap_review_async.assert_not_called()
    assert controller.project.model_dump() == before
    assert controller._assets._reserved == 0


def test_native_review_refusal_preserves_predecessor_selection_delivery_and_reservation(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    old = _job(audio_engine_mock)
    maps.select(0, "existing accepted map")
    maps.completed(0, _event(old))
    job, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    maps.raw_completed(0, raw_event(job, candidate))
    geometry, delivery = maps.effective(0), maps.playback_delivery(0)
    intent = controller.project.pad_source_beatmap_intent[0]
    before = controller._persistence.config_path.read_bytes()
    changed = Mock()
    monkeypatch.setattr(maps, "changed", changed)
    review_job(audio_engine_mock)
    audio_engine_mock.begin_source_beatmap_review_async.side_effect = ValueError("outside anchor")

    with pytest.raises(ValueError, match="outside anchor"):
        maps.review_raw(
            0, [(0, 0.0), (4, 1.5)], expected_attempt=1, expected_revision=candidate.revision
        )

    assert controller.project.pad_source_beatmap_intent[0] is intent
    assert maps.effective(0) is geometry
    assert maps.playback_delivery(0) is delivery
    assert controller._persistence.config_path.read_bytes() == before
    assert controller._assets._reserved == 0
    assert not maps._pending
    changed.assert_not_called()


def test_raw_discard_after_valid_review_admission_does_not_cancel_new_completion(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps, raw, candidate = _candidate(controller, audio_engine_mock)
    accepted = review_job(audio_engine_mock)
    maps.review_raw(
        0, [(0, 0.0), (4, 1.5)], expected_attempt=1, expected_revision=candidate.revision
    )
    maps.discard_raw(0, expected_attempt=1, expected_revision=candidate.revision)
    raw.cancel.assert_called_once()
    accepted.cancel.assert_not_called()
    assert maps.raw_state(0).status == "cancel_retiring"
    assert maps.status(0) == "pending"

    completion = _event(accepted, "b")
    maps.completed(0, completion)

    assert maps.status(0) == "effective"
    assert maps.playback_delivery(0) is completion["_native_event_owner"]
    assert controller.project.pad_source_beatmap_intent[0].selected is not None
    accepted.cancel.assert_not_called()
    raw.source_beatmap_async_status.return_value["raw_review_retired"] = True
    maps.poll(frozenset())
    assert maps.raw_state(0).status == "none"
    assert maps.status(0) == "effective"


def test_successful_review_retires_only_its_raw_capture_after_durable_selection(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps, raw, candidate = _candidate(controller, audio_engine_mock)
    accepted = review_job(audio_engine_mock)
    maps.review_raw(
        0, [(0, 0.0), (4, 1.5)], expected_attempt=1, expected_revision=candidate.revision
    )
    raw.cancel.assert_not_called()
    assert maps.raw_state(0).status == "unreviewed"
    maps.completed(0, _event(accepted, "b"))
    assert maps.status(0) == "effective"
    raw.cancel.assert_called_once()
    accepted.cancel.assert_not_called()
    assert maps.raw_state(0).status == "cancel_retiring"
    maps.poll(frozenset())
    assert maps.raw_state(0).occupied, "Terminal alone cannot release the original reader."
    raw.source_beatmap_async_status.return_value["raw_review_retired"] = True
    maps.poll(frozenset())
    assert not maps.raw_state(0).occupied
    assert maps.status(0) == "effective"


def test_old_review_completion_does_not_retire_another_pads_new_analysis(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    maps, raw, candidate = _candidate(controller, audio_engine_mock)
    accepted = review_job(audio_engine_mock)
    maps.review_raw(
        0, [(0, 0.0), (4, 1.5)], expected_attempt=1, expected_revision=candidate.revision
    )
    maps.discard_raw(0, expected_attempt=1, expected_revision=candidate.revision)
    raw.source_beatmap_async_status.return_value["raw_review_retired"] = True
    maps.poll(frozenset())
    controller.project.pad_content[1] = PadContentIdentity(instance_id="f" * 32)
    controller.project.sample_paths[1] = controller.project.sample_paths[0]
    newer = raw_job(audio_engine_mock)
    maps.analyze_original(1, expected_attempt=1)
    maps.completed(0, _event(accepted, "b"))
    assert maps.status(0) == "effective"
    assert maps.raw_state(1).attempt == 2
    assert maps.raw_state(1).status == "queued"
    newer.cancel.assert_not_called()


def test_reentrant_equal_reference_does_not_retire_raw_without_own_selection(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    maps, raw, candidate = _candidate(controller, audio_engine_mock)
    accepted = review_job(audio_engine_mock)
    maps.review_raw(
        0, [(0, 0.0), (4, 1.5)], expected_attempt=1, expected_revision=candidate.revision
    )

    def newer_selection(
        slot: int,
        _content: str,
        _previous: PadSourceBeatmapIntent | None,
        selected: PadSourceBeatmapIntent,
        reservation: AssetRetirementReservation,
    ) -> bool:
        controller.project.pad_source_beatmap_intent[slot] = selected.model_copy(
            update={"revision": selected.revision + 1}
        )
        reservation.close()
        return False

    monkeypatch.setattr(maps, "_select_durable", newer_selection)
    maps.completed(0, _event(accepted, "b"))
    assert maps.raw_state(0).status == "unreviewed"
    raw.cancel.assert_not_called()


def test_review_save_failure_retires_new_artifact_and_preserves_old_effective_map(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    maps = _raw_assigned(controller, audio_engine_mock)
    old = _job(audio_engine_mock)
    maps.select(0, "existing map")
    maps.completed(0, _event(old))
    geometry, delivery = maps.effective(0), maps.playback_delivery(0)
    selected = controller.project.pad_source_beatmap_intent[0].selected
    saved = controller._persistence.config_path.read_bytes()
    raw, candidate = raw_job(audio_engine_mock), FakeRawCandidate()
    maps.analyze_original(0, expected_attempt=0)
    maps.raw_completed(0, raw_event(raw, candidate))
    accepted = review_job(audio_engine_mock)
    maps.review_raw(
        0, [(0, 0.0), (4, 1.5)], expected_attempt=1, expected_revision=candidate.revision
    )

    def refuse(*_args: object, **_kwargs: object) -> None:
        message = "injected review Config failure"
        raise OSError(message)

    monkeypatch.setattr(controller._persistence, "_atomic_write_text", refuse)
    maps.completed(0, _event(accepted, "b"))

    assert controller.project.pad_source_beatmap_intent[0].selected == selected
    assert controller._persistence.config_path.read_bytes() == saved
    assert maps.effective(0) is geometry
    assert maps.playback_delivery(0) is delivery
    artifact = accepted.resolve_source_beatmap_artifact_completion.return_value
    artifact.retire_unselected.assert_called_once()
    assert controller._assets._reserved == 0
    assert maps.raw_state(0).status == "unreviewed"
    raw.cancel.assert_not_called()
