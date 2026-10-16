"""Python observer routing through real Pair mirrors and scripted opaque deliveries.

The delivery double follows the fixture's original voice objects. It does not
certify Native receipt guards, fixed work bounds, audio DSP or device behavior.
"""

from collections import deque
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.copy_snapshot import CONTENT_FIELDS
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis, SessionState
from tests.flitzis_looper.controller import test_pair_arrangement_controller as pair_facade

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.loader import LoaderController
    from flitzis_looper.pair_snapshot import PairOperation
    from flitzis_looper_audio import CapturedPadPair


@dataclass(slots=True)
class _OriginalOwner:
    voice: object
    pending: bool = False
    retired: bool = False
    timing_stale: bool = False


@dataclass(frozen=True, slots=True)
class _Delivery:
    owner: _OriginalOwner

    def same_owner(self, other: _Delivery) -> bool:
        return self.owner is other.owner


@dataclass(slots=True)
class _DeliveryStream:
    native: pair_facade._Native
    events: deque[dict[str, object]] = field(default_factory=deque)
    resolutions: list[_Delivery] = field(default_factory=list)

    def poll(self) -> dict[str, object] | None:
        return self.events.popleft() if self.events else None

    def resolve(self, delivery: _Delivery) -> tuple[int, bool] | None:
        self.resolutions.append(delivery)
        owner = delivery.owner
        if owner.pending:
            message = "loader event content placement pending"
            raise RuntimeError(message)
        if owner.retired:
            return None
        for sample_id, voice in self.native.voices.items():
            if voice is owner.voice:
                return sample_id, owner.timing_stale
        return None


@dataclass(slots=True)
class _ObserverHarness:
    pair: pair_facade._Harness
    stream: _DeliveryStream
    source: _OriginalOwner
    before: dict[int, SampleAnalysis]

    @property
    def loader(self) -> LoaderController:
        return self.pair.app.loader

    @property
    def project(self) -> ProjectState:
        return self.pair.app.project

    @property
    def session(self) -> SessionState:
        return self.pair.app.session


def _analysis(bpm: float) -> SampleAnalysis:
    return SampleAnalysis(
        bpm=bpm,
        key="Am",
        beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
    )


def _seed_analysis(rig: _ObserverHarness, sample_id: int, request_id: int, bpm: float) -> None:
    previous = _analysis(bpm)
    rig.before[sample_id] = previous
    rig.project.sample_analysis[sample_id] = previous
    rig.loader._analysis_request_ids[sample_id] = request_id
    rig.session.analyzing_sample_ids.add(sample_id)


def _empty_target(pair: pair_facade._Harness) -> None:
    defaults = ProjectState()
    for _, name in CONTENT_FIELDS:
        getattr(pair.app.project, name)[215] = getattr(defaults, name)[215]
    pair.app.session.active_sample_ids.discard(215)
    pair.app.session.paused_sample_ids.discard(215)
    pair.native.voices[215] = None
    pair.assets.sync_assignments()


def _harness(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    operation: PairOperation = "swap",
    equal_request_ids: bool = False,
) -> _ObserverHarness:
    pair = pair_facade._harness(tmp_path, monkeypatch)
    if operation == "move":
        _empty_target(pair)
    stream = _DeliveryStream(pair.native)
    rig = _ObserverHarness(pair, stream, _OriginalOwner(pair.native.voices[0]), {})
    monkeypatch.setattr(pair.native, "poll_loader_events", stream.poll, raising=False)
    monkeypatch.setattr(pair.native, "_resolve_loader_event", stream.resolve, raising=False)
    monkeypatch.setattr(pair.native, "pad_timing_intent", lambda _: "legacy", raising=False)
    _seed_analysis(rig, 0, 7, 120.0)
    if operation == "swap":
        _seed_analysis(rig, 215, 7 if equal_request_ids else 19, 90.0)
    pair.app.persistence.mark_dirty()
    pair.app.persistence.flush()
    return rig


def _success(
    sample_id: int,
    request_id: int,
    *,
    owner: _OriginalOwner | None,
    bpm: float = 143.0,
) -> dict[str, object]:
    event: dict[str, object] = {
        "type": "task_success",
        "id": sample_id,
        "task": "analysis",
        "request_id": request_id,
        "analysis": _analysis(bpm).model_dump(mode="python"),
    }
    if owner is not None:
        event["_native_event_owner"] = _Delivery(owner)
    return event


def _progress(owner: _OriginalOwner, percent: float) -> dict[str, object]:
    return {
        "type": "task_progress",
        "id": 0,
        "task": "analysis",
        "request_id": 7,
        "percent": percent,
        "stage": "original analysis",
        "_native_event_owner": _Delivery(owner),
    }


def _hold_through_claim(
    rig: _ObserverHarness, event: dict[str, object], operation: PairOperation
) -> None:
    rig.pair.native.auto_ack = False
    rig.stream.events.append(event)
    assert rig.pair.controller.begin(0, 215, operation=operation)
    rig.loader.poll_loader_events()
    assert len(rig.loader._pair_held_loader_events) == 1
    assert rig.loader._pair_held_loader_events[0] is event
    rig.pair.controller.poll()
    pair = rig.pair.original_pair()
    captured = rig.pair.native.captures[-1]
    # This is the same scripted fake viewed at the production Native boundary.
    native_capture = cast("CapturedPadPair", captured)
    assert pair.captured is native_capture
    assert captured.claimed
    assert pair.acknowledgement is None
    rig.loader.poll_loader_events()
    assert rig.loader._pair_held_loader_events[0] is event
    assert rig.loader._analysis_request_ids[0] == 7


def _finish_pair(rig: _ObserverHarness) -> None:
    captured = rig.pair.native.captures[-1]
    # Retain the typed fake for its acknowledgement state alongside its Native view.
    native_capture = cast("CapturedPadPair", captured)
    assert rig.pair.original_pair().captured is native_capture
    captured.acknowledged = True
    rig.pair.complete()
    assert rig.loader._pair_deferred_sample_ids == frozenset()


@pytest.mark.parametrize(
    ("operation", "equal_request_ids"), [("move", False), ("swap", False), ("swap", True)]
)
def test_held_analysis_follows_original_source_after_pair_and_request_rekey(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    operation: PairOperation,
    *,
    equal_request_ids: bool,
) -> None:
    rig = _harness(tmp_path, monkeypatch, operation=operation, equal_request_ids=equal_request_ids)
    event = _success(0, 7, owner=rig.source)
    original = event.copy()
    _hold_through_claim(rig, event, operation)
    assert event["id"] == 215
    assert event["_native_event_owner"] is original["_native_event_owner"]
    _finish_pair(rig)
    assert rig.loader._analysis_request_ids[215] == 7
    assert rig.project.sample_analysis[215] is rig.before[0]
    rig.loader.poll_loader_events()
    assert rig.project.sample_analysis[215] == _analysis(143.0)
    assert 215 not in rig.loader._analysis_request_ids
    assert 215 not in rig.session.analyzing_sample_ids
    assert rig.loader._pair_held_loader_events == []
    assert event == {**original, "id": 215}
    assert all(token is original["_native_event_owner"] for token in rig.stream.resolutions)
    if operation == "swap":
        assert rig.project.sample_analysis[0] is rig.before[215]
        assert rig.loader._analysis_request_ids[0] == (7 if equal_request_ids else 19)
        assert 0 in rig.session.analyzing_sample_ids
    else:
        assert rig.project.sample_analysis[0] is None
        assert 0 not in rig.loader._analysis_request_ids
        assert 0 not in rig.session.analyzing_sample_ids


@pytest.mark.parametrize("retirement", ["retired", "replaced"])
def test_pending_retains_exact_event_and_token_then_none_cannot_change_later_occupant(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, retirement: str
) -> None:
    rig = _harness(tmp_path, monkeypatch)
    event = _success(0, 7, owner=rig.source)
    token = event["_native_event_owner"]
    rig.source.pending = True
    rig.stream.events.append(event)
    for _ in range(2):
        rig.loader.poll_loader_events()
        assert rig.loader._pair_held_loader_events == [event]
        assert rig.loader._pair_held_loader_events[0] is event
        assert event["_native_event_owner"] is token
        assert rig.project.sample_analysis[0] is rig.before[0]
    rig.source.pending = False
    rig.source.retired = retirement == "retired"
    rig.pair.native.voices[0] = object()
    _seed_analysis(rig, 0, 7, 99.0)
    rig.loader.poll_loader_events()
    assert rig.loader._pair_held_loader_events == []
    assert rig.project.sample_analysis[0] is rig.before[0]
    assert rig.loader._analysis_request_ids[0] == 7
    assert 0 in rig.session.analyzing_sample_ids
    assert len(rig.stream.resolutions) == 3
    assert all(resolved is token for resolved in rig.stream.resolutions)


@pytest.mark.parametrize("operation", ["move", "swap"])
def test_delivery_rechecks_original_timing_after_pair_before_storing_analysis(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, operation: PairOperation
) -> None:
    rig = _harness(tmp_path, monkeypatch, operation=operation)
    event = _success(0, 7, owner=rig.source)
    _hold_through_claim(rig, event, operation)
    assert "timing_stale" not in event
    _finish_pair(rig)
    rig.project.manual_bpm[215] = 137.5
    rig.project.pad_timing_intent[215] = "tap"
    rig.source.timing_stale = True
    rig.loader.poll_loader_events()
    assert event["timing_stale"] is True
    assert rig.project.sample_analysis[215] is rig.before[0]
    assert rig.project.manual_bpm[215] == 137.5
    assert rig.project.pad_timing_intent[215] == "tap"
    assert 215 not in rig.loader._analysis_request_ids
    assert 215 not in rig.session.analyzing_sample_ids
    assert rig.loader._pair_held_loader_events == []


def test_progress_burst_coalesces_original_owner_and_does_not_block_unrelated_terminal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = _harness(tmp_path, monkeypatch)
    rig.loader._pair_deferred_sample_ids = frozenset({0, 215})
    rig.pair.native.voices[36] = object()
    _seed_analysis(rig, 36, 9, 70.0)
    progress = [_progress(rig.source, float(index)) for index in range(150)]
    terminal = _success(0, 7, owner=rig.source)
    unrelated = _success(36, 9, owner=_OriginalOwner(rig.pair.native.voices[36]), bpm=156.0)
    rig.stream.events.extend([*progress, terminal, unrelated])
    rig.loader.poll_loader_events()
    assert rig.stream.events == deque()
    assert rig.project.sample_analysis[36] == _analysis(156.0)
    assert 36 not in rig.loader._analysis_request_ids
    assert 36 not in rig.session.analyzing_sample_ids
    held = rig.loader._pair_held_loader_events
    assert len(held) == 2
    assert held[0] is progress[-1]
    assert held[1] is terminal
    assert rig.project.sample_analysis[0] is rig.before[0]
    assert 0 not in rig.session.sample_analysis_progress
    assert len(rig.stream.resolutions) == 152
    rig.loader._pair_deferred_sample_ids = frozenset()
    rig.loader.poll_loader_events()
    assert rig.project.sample_analysis[0] == _analysis(143.0)
    assert rig.project.sample_analysis[36] == _analysis(156.0)
    assert rig.loader._pair_held_loader_events == []
    assert 0 not in rig.loader._analysis_request_ids
    assert 0 not in rig.session.sample_analysis_progress


@pytest.mark.parametrize("difference", ["owner", "type", "task", "request_id"])
def test_progress_coalescing_retains_distinct_original_owner_and_task_guards(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, difference: str
) -> None:
    rig = _harness(tmp_path, monkeypatch)
    rig.loader._pair_deferred_sample_ids = frozenset({0})
    previous = _progress(rig.source, 1.0)
    latest = _progress(rig.source, 2.0)
    distinct = _progress(rig.source, 3.0)
    if difference == "owner":
        distinct["_native_event_owner"] = _Delivery(_OriginalOwner(rig.source.voice))
    else:
        distinct[difference] = {"type": "progress", "task": "stem_generation", "request_id": 8}[
            difference
        ]
    rig.stream.events.extend((previous, latest, distinct))
    rig.loader.poll_loader_events()
    held = rig.loader._pair_held_loader_events
    assert len(held) == 2
    assert held[0] is latest
    assert held[1] is distinct
    assert latest["_native_event_owner"] is not previous["_native_event_owner"]
    assert 0 not in rig.session.sample_analysis_progress


def test_untagged_held_numeric_terminal_cannot_settle_equal_request_id_after_swap(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    rig = _harness(tmp_path, monkeypatch, equal_request_ids=True)
    event = _success(0, 7, owner=None)
    _hold_through_claim(rig, event, "swap")
    assert event["_unowned_pair_event"] is True
    assert event["id"] == 0
    _finish_pair(rig)
    rig.loader.poll_loader_events()
    assert rig.loader._pair_held_loader_events == []
    assert rig.project.sample_analysis[0] is rig.before[215]
    assert rig.project.sample_analysis[215] is rig.before[0]
    assert rig.loader._analysis_request_ids == {0: 7, 215: 7}
    assert rig.session.analyzing_sample_ids == {0, 215}
    assert rig.stream.resolutions == []
