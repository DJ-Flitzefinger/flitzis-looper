"""Real controller source custody and durable cancellation around scripted Native boundaries.

The adapters inject capture results and rejection; these tests do not prove Native
source guards, callback execution or listening acceptance.
"""

from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.pair_arrangement_journal import (
    PairJournalOwner,
    validated_pair_history,
)
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding
from tests.flitzis_looper.controller.test_pad_arrangement import (
    _harness as _copy_harness,
)
from tests.flitzis_looper.controller.test_pad_arrangement import (
    _occupy_target,
)
from tests.flitzis_looper.controller.test_pair_arrangement_controller import (
    _harness as _pair_harness,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.pair_arrangement_journal import PairArrangementJournal
    from flitzis_looper_audio import CapturedPadCopy, CapturedPadPair, InputRuntimePadBinding


def _copy_sources(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    (samples / "source.wav").write_bytes(b"scripted source custody")
    (samples / "target.wav").write_bytes(b"scripted predecessor custody")


@pytest.mark.parametrize("occupied", [False, True], ids=["empty", "overwrite"])
def test_copy_passes_original_gesture_binding_without_substituting_fresh_source(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, occupied: bool
) -> None:
    _copy_sources(tmp_path, monkeypatch)
    harness = _copy_harness(tmp_path)
    if occupied:
        _occupy_target(harness)
    original = cast("InputRuntimePadBinding", FakeInputRuntimePadBinding(0))
    fresh = cast("InputRuntimePadBinding", FakeInputRuntimePadBinding(0, authority_revision=2))
    harness.native.bindings[0] = original
    target = harness.native.bindings.get(215)
    getter = Mock(side_effect=lambda slot: fresh if slot == 0 else target)
    monkeypatch.setattr(harness.native, "current_input_runtime_pad_binding", getter)
    capture = harness.native.capture_pad_copy
    received: list[InputRuntimePadBinding] = []

    def capture_original(
        source_binding: InputRuntimePadBinding,
        target_id: int,
        intent_json: str,
        *,
        target_binding: InputRuntimePadBinding | None = None,
    ) -> CapturedPadCopy:
        received.append(source_binding)
        return capture(source_binding, target_id, intent_json, target_binding=target_binding)

    monkeypatch.setattr(harness.native, "capture_pad_copy", capture_original)
    admit = harness.controller.copy_overwrite if occupied else harness.controller.copy_to_empty
    assert admit(0, 215, source_binding=original)
    pending = harness.controller._copy
    assert pending is not None
    assert pending.source_binding is original
    assert received == [original]
    assert received[0] is original
    assert fresh is not original
    assert [call.args for call in getter.call_args_list] == [(215,)]
    assert harness.native.target_binding is target
    assert harness.native.publish_count == 0
    assert not harness.controller.cancel()
    harness.jobs.run()
    harness.controller.poll()
    assert not harness.controller.active
    assert harness.persistence._migration_owner is None
    assert harness.assets.retirement.closed
    assert harness.native.publish_count == 0


def test_pair_passes_exact_original_source_without_calling_fresh_binding_getter(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _pair_harness(tmp_path, monkeypatch)
    original = cast("InputRuntimePadBinding", FakeInputRuntimePadBinding(0))
    fresh = cast("InputRuntimePadBinding", FakeInputRuntimePadBinding(0, authority_revision=2))
    getter = Mock(return_value=fresh)
    monkeypatch.setattr(harness.native, "current_input_runtime_pad_binding", getter, raising=False)
    capture = harness.native._capture_pad_pair
    received: list[InputRuntimePadBinding | None] = []

    def capture_original(
        source: int, target: int, *, source_binding: InputRuntimePadBinding | None = None
    ) -> CapturedPadPair:
        received.append(source_binding)
        return cast("CapturedPadPair", capture(source, target))

    monkeypatch.setattr(harness.native, "_capture_pad_pair", capture_original)
    assert harness.controller.begin(0, 215, operation="swap", source_binding=original)
    pair = harness.original_pair()
    assert pair.source_binding is original
    assert received == [original]
    assert received[0] is original
    getter.assert_not_called()
    assert len(harness.native.captures) == 1
    assert harness.controller.prepare_shutdown()
    assert not harness.controller.active
    assert harness.app.persistence._migration_owner is None
    assert pair.retirement.remaining == 0
    assert harness.native.publish_calls == []


def _fail_first_journal_write(monkeypatch: pytest.MonkeyPatch) -> None:
    """Inject one real journal-write retry before any Native source capture."""
    append = PairJournalOwner.append
    fail_initial = True

    def append_after_retry(
        owner: PairJournalOwner, record: PairArrangementJournal
    ) -> PairArrangementJournal:
        nonlocal fail_initial
        if record.phase == "captured" and fail_initial:
            fail_initial = False
            message = "initial gesture journal temporarily unavailable"
            raise OSError(message)
        return append(owner, record)

    monkeypatch.setattr(PairJournalOwner, "append", append_after_retry)


def _reject_original_source(
    monkeypatch: pytest.MonkeyPatch, native: object
) -> list[InputRuntimePadBinding | None]:
    """Observe original binding identity at the scripted Native rejection boundary."""
    received: list[InputRuntimePadBinding | None] = []

    def reject_original(
        source: int, target: int, *, source_binding: InputRuntimePadBinding | None = None
    ) -> CapturedPadPair:
        assert (source, target) == (0, 215)
        received.append(source_binding)
        message = "stale or foreign pair source binding"
        raise ValueError(message)

    monkeypatch.setattr(native, "_capture_pad_pair", reject_original)
    return received


def test_pair_journal_retry_retains_original_then_durably_cancels_guard_rejection(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _pair_harness(tmp_path, monkeypatch)
    original = cast("InputRuntimePadBinding", FakeInputRuntimePadBinding(0))
    fresh = cast("InputRuntimePadBinding", FakeInputRuntimePadBinding(0, authority_revision=2))
    getter = Mock(return_value=fresh)
    monkeypatch.setattr(harness.native, "current_input_runtime_pad_binding", getter, raising=False)
    before_config = harness.app.persistence.config_path.read_bytes()
    before_project = harness.app.project.model_dump_json()
    before_assignments = harness.assets._assignments.copy()
    _fail_first_journal_write(monkeypatch)
    received = _reject_original_source(monkeypatch, harness.native)
    assert harness.controller.begin(0, 215, operation="swap", source_binding=original)
    pair = harness.original_pair()
    assert harness.controller.status == "settlement_pending"
    assert pair.source_binding is original
    assert pair.captured is None
    assert received == []
    assert harness.app.persistence._migration_owner == pair.owner
    assert pair.retirement.remaining > 0

    harness.controller.poll()

    assert received == [original]
    assert received[0] is original
    getter.assert_not_called()
    assert pair.cancel_requested
    assert pair.cancelled
    assert pair.writer_released
    assert pair.captured is None
    assert not harness.controller.active
    assert harness.controller._pair is None
    assert harness.controller.status == "cancelled"
    assert harness.app.persistence._migration_owner is None
    assert pair.retirement.remaining == 0
    assert harness.assets._assignments == before_assignments
    assert harness.images == []
    assert all(lease.release_count == 0 for lease in harness.native.leases)
    assert harness.native.captures == []
    assert harness.native.publish_calls == []
    assert harness.native.cancel_calls == []
    assert harness.native.finish_calls == []
    assert harness.app.project.model_dump_json() == before_project
    assert harness.app.persistence.config_path.read_bytes() == before_config
    for component in (
        harness.app.transport,
        harness.app.loader,
        harness.app.accepted_timing,
        harness.app.stems,
    ):
        assert component._pair_deferred_sample_ids == frozenset()
    reopened = PairJournalOwner.open(tmp_path / "samples", pair.owner)
    history = validated_pair_history(reopened.store.history(), pair.owner)
    assert [record.phase for record in history] == ["captured", "cancelled"]
    harness.controller.poll()
    harness.controller.poll()
    assert len(received) == 1
    assert harness.controller.status == "cancelled"


def test_legacy_pair_admission_keeps_two_argument_native_capture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _pair_harness(tmp_path, monkeypatch)
    capture = harness.native._capture_pad_pair
    calls: list[tuple[int, int]] = []

    def capture_without_keywords(source: int, target: int) -> CapturedPadPair:
        calls.append((source, target))
        return cast("CapturedPadPair", capture(source, target))

    monkeypatch.setattr(harness.native, "_capture_pad_pair", capture_without_keywords)
    assert harness.controller.begin(0, 215, operation="swap")
    assert calls == [(0, 215)]
    assert harness.original_pair().source_binding is None
    assert harness.controller.prepare_shutdown()
    assert harness.app.persistence._migration_owner is None


@pytest.mark.parametrize("occupied", [False, True], ids=["empty", "overwrite"])
def test_legacy_copy_admission_still_resolves_its_source_at_admission(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, occupied: bool
) -> None:
    _copy_sources(tmp_path, monkeypatch)
    harness = _copy_harness(tmp_path)
    if occupied:
        _occupy_target(harness)
    original = harness.native.bindings[0]
    getter = Mock(side_effect=harness.native.current_input_runtime_pad_binding)
    monkeypatch.setattr(harness.native, "current_input_runtime_pad_binding", getter)
    admit = harness.controller.copy_overwrite if occupied else harness.controller.copy_to_empty
    assert admit(0, 215)
    pending = harness.controller._copy
    assert pending is not None
    assert pending.source_binding is original
    assert [call.args for call in getter.call_args_list] == [(0,), (215,)]
    assert harness.native.capture_count == 1
    assert not harness.controller.cancel()
    harness.jobs.run()
    harness.controller.poll()
    assert not harness.controller.active
    assert harness.persistence._migration_owner is None
