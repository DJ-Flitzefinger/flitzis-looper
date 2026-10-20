"""Exact Bank warning ownership around the existing complete transaction path.

The real Bank-controller fixture below scripts Native witnesses; it proves no
callback, device, DSP, resource or Human listening acceptance.
"""

from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.control_diagnostics import ConsoleDiagnostics
from flitzis_looper.controller.performance_confirmation import (
    BankConfirmation,
    PerformanceConfirmation,
    PerformanceConfirmationController,
)
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.controller.test_bank_arrangement_controller import _harness

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import BankParticipants


def _seed(
    controller: AppController,
    audio: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    occupied: bool = True,
) -> tuple[UiContext, Mock, Mock]:
    project = controller.project
    project.sample_paths[0] = "samples/source.wav"
    project.pad_content[0] = PadContentIdentity(instance_id="1" * 32, material_id="a" * 32)
    if occupied:
        # The final slot matters: an empty first target slot does not make the bank empty.
        project.sample_paths[71] = "samples/target.wav"
        project.pad_content[71] = PadContentIdentity(instance_id="2" * 32)
    audio.capture_bank_participants = Mock(return_value=object())
    audio.waveform_source_identity.return_value = (7, "b" * 64, 128, 44_100)
    begin = Mock(return_value=True)
    discard = Mock(wraps=controller.banks.discard_request)
    monkeypatch.setattr(controller.banks, "begin", begin)
    monkeypatch.setattr(controller.banks, "discard_request", discard)
    return UiContext(controller), begin, discard


def _pending(ctx: UiContext) -> BankConfirmation:
    intent = ctx.ui.confirmation.pending
    assert isinstance(intent, BankConfirmation)
    return intent


def test_occupied_bank_captures_once_and_accepts_exact_original_after_selection_change(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, begin, _discard = _seed(controller, audio_engine_mock, monkeypatch)
    before = controller.project.model_dump()
    ctx.ui.confirmation.request_bank_copy(0, 1)
    intent = _pending(ctx)
    original = intent.request
    assert (intent.action, intent.source_bank, intent.target_bank) == ("copy_bank", 0, 1)
    assert original is controller.banks._request
    assert original.participants is audio_engine_mock.capture_bank_participants.return_value
    assert controller.project.model_dump() == before
    assert controller.persistence._migration_owner is None
    assert not controller.banks.active
    begin.assert_not_called()
    controller.project.selected_bank = 4
    controller.project.selected_pad = 100
    ctx.ui.confirmation.request_bank_copy(4, 2)
    ctx.ui.confirmation.request_bank_clear(5)
    controller.performance_confirmation.request("unload", 0)
    assert ctx.ui.confirmation.pending is intent
    assert ctx.ui.confirmation.is_current(intent)
    assert ctx.ui.confirmation.take_open_request(intent)
    assert not ctx.ui.confirmation.take_open_request(intent)
    ctx.ui.confirmation.accept(intent)
    ctx.ui.confirmation.accept(intent)
    ctx.ui.confirmation.dismiss(intent)
    assert ctx.ui.confirmation.pending is None
    audio_engine_mock.capture_bank_participants.assert_called_once_with(0, 1)
    begin.assert_called_once_with(original)
    assert begin.call_args.args[0] is original
    assert controller.project.selected_bank == 4


@pytest.mark.parametrize("source_occupied", [False, True])
def test_empty_target_copy_dispatches_exact_capture_immediately(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    *,
    source_occupied: bool,
) -> None:
    ctx, begin, _discard = _seed(controller, audio_engine_mock, monkeypatch, occupied=False)
    if not source_occupied:
        controller.project.sample_paths[0] = None
        controller.project.pad_content[0] = None
    before = controller.project.model_dump()
    ctx.ui.confirmation.request_bank_copy(0, 1)
    assert ctx.ui.confirmation.pending is None
    audio_engine_mock.capture_bank_participants.assert_called_once_with(0, 1)
    original = controller.banks._request
    assert original is not None
    begin.assert_called_once_with(original)
    assert begin.call_args.args[0] is original
    assert controller.project.model_dump() == before


@pytest.mark.parametrize("target", [0, 1, 5])
def test_clear_always_requires_exact_target_warning_even_empty_or_current(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    target: int,
) -> None:
    ctx, begin, _discard = _seed(controller, audio_engine_mock, monkeypatch)
    controller.project.selected_bank = target
    ctx.ui.confirmation.request_bank_clear(target)
    intent = _pending(ctx)
    assert (intent.action, intent.source_bank, intent.target_bank) == ("clear_bank", None, target)
    begin.assert_not_called()
    audio_engine_mock.capture_bank_participants.assert_called_once_with(None, target)
    controller.project.selected_bank = (target + 1) % 6
    assert ctx.ui.confirmation.is_current(intent)
    ctx.ui.confirmation.accept(intent)
    ctx.ui.confirmation.accept(intent)
    begin.assert_called_once_with(intent.request)


def test_cancel_discards_exact_request_and_old_warning_cannot_touch_later_clear(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, begin, discard = _seed(controller, audio_engine_mock, monkeypatch)
    before = controller.project.model_dump(), controller.session.model_dump()
    ctx.ui.confirmation.request_bank_clear(5)
    old = _pending(ctx)
    ctx.ui.confirmation.dismiss(old)
    discard.assert_called_once_with(old.request)
    assert discard.call_args.args[0] is old.request
    assert controller.banks._request is None
    ctx.ui.confirmation.request_bank_clear(5)
    current = _pending(ctx)
    assert old is not current
    assert old.request is not current.request
    ctx.ui.confirmation.accept(old)
    ctx.ui.confirmation.dismiss(old)
    assert ctx.ui.confirmation.pending is current
    assert controller.banks._request is current.request
    assert (controller.project.model_dump(), controller.session.model_dump()) == before
    begin.assert_not_called()
    assert discard.call_count == 1


@pytest.mark.parametrize("change", ["source", "target", "empty_slot", "gain", "inheritance"])
def test_changed_participant_revokes_warning_without_recapture_or_dispatch(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    change: str,
) -> None:
    ctx, begin, discard = _seed(controller, audio_engine_mock, monkeypatch)
    ctx.ui.confirmation.request_bank_copy(0, 1)
    intent = _pending(ctx)
    if change in {"source", "target"}:
        slot = 0 if change == "source" else 71
        original = controller.project.pad_content[slot]
        assert original is not None
        controller.project.pad_content[slot] = original.model_copy(update={"instance_id": "3" * 32})
    elif change == "empty_slot":
        controller.project.sample_paths[36] = "samples/new-target.wav"
        controller.project.pad_content[36] = PadContentIdentity(instance_id="3" * 32)
    elif change == "gain":
        controller.project.pad_gain_db[0] = -12.0
    else:
        controller.project.key_lock = not controller.project.key_lock
    before = controller.project.model_dump(), controller.session.model_dump()
    assert not ctx.ui.confirmation.is_current(intent)
    ctx.ui.confirmation.accept(intent)
    assert ctx.ui.confirmation.pending is None
    begin.assert_not_called()
    discard.assert_called_once_with(intent.request)
    audio_engine_mock.capture_bank_participants.assert_called_once_with(0, 1)
    assert (controller.project.model_dump(), controller.session.model_dump()) == before


@pytest.mark.parametrize(
    "failure",
    [None, OSError("disk refused"), RuntimeError("native refused"), TypeError("bad witness")],
)
def test_failed_admission_stays_truthful_and_dismissible_without_second_dispatch(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    failure: Exception | None,
) -> None:
    ctx, begin, discard = _seed(controller, audio_engine_mock, monkeypatch)
    begin.return_value = False
    begin.side_effect = failure
    controller.banks.error = "unavailable captured request" if failure is None else None
    ctx.ui.confirmation.request_bank_copy(0, 1)
    intent = _pending(ctx)
    before = controller.project.model_dump(), controller.session.model_dump()
    ctx.ui.confirmation.accept(intent)
    failed = _pending(ctx)
    expected = str(failure) if failure is not None else "unavailable captured request"
    assert failed.error == expected
    assert failed.request is intent.request
    assert not ctx.ui.confirmation.is_current(failed)
    assert expected in capsys.readouterr().err
    assert ctx.ui.confirmation.take_open_request(failed)
    assert not ctx.ui.confirmation.take_open_request(failed)
    ctx.ui.confirmation.accept(intent)
    assert ctx.ui.confirmation.pending is failed
    begin.assert_called_once_with(intent.request)
    discard.assert_called_once_with(intent.request)
    ctx.ui.confirmation.dismiss(failed)
    assert ctx.ui.confirmation.pending is None
    assert discard.call_args.args[0] is intent.request
    assert (controller.project.model_dump(), controller.session.model_dump()) == before


def test_refused_admission_without_controller_reason_has_visible_diagnostic(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    ctx, begin, _discard = _seed(controller, audio_engine_mock, monkeypatch, occupied=False)
    begin.return_value = False
    ctx.ui.confirmation.request_bank_copy(0, 1)
    failed = _pending(ctx)
    assert failed.error == "Bank request is no longer available for admission"
    assert failed.error in capsys.readouterr().err


@pytest.mark.parametrize("error_type", [OSError, RuntimeError, TypeError, ValueError])
def test_capture_failure_reports_reason_without_warning_or_mutation(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    error_type: type[Exception],
) -> None:
    ctx, begin, discard = _seed(controller, audio_engine_mock, monkeypatch)
    audio_engine_mock.capture_bank_participants.side_effect = error_type("capture refused")
    before = controller.project.model_dump(), controller.session.model_dump()
    ctx.ui.confirmation.request_bank_copy(0, 1)
    assert ctx.ui.confirmation.pending is None
    assert "capture refused" in capsys.readouterr().err
    begin.assert_not_called()
    discard.assert_not_called()
    assert (controller.project.model_dump(), controller.session.model_dump()) == before


def test_pad_warning_prevents_a_second_bank_capture(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, begin, _discard = _seed(controller, audio_engine_mock, monkeypatch)
    controller.performance_confirmation.request("unload", 0)
    original = ctx.ui.confirmation.pending
    assert isinstance(original, PerformanceConfirmation)
    ctx.ui.confirmation.request_bank_copy(0, 1)
    ctx.ui.confirmation.request_bank_clear(0)
    assert ctx.ui.confirmation.pending is original
    audio_engine_mock.capture_bank_participants.assert_not_called()
    begin.assert_not_called()


def test_close_revokes_unreserved_bank_confirmation_without_starting_a_transaction(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctx, begin, discard = _seed(controller, audio_engine_mock, monkeypatch)
    ctx.ui.confirmation.request_bank_clear(0)
    intent = _pending(ctx)
    controller._close_requested = True
    controller.banks.prepare_shutdown()
    assert controller.banks._request is None
    assert not ctx.ui.confirmation.is_current(intent)
    ctx.ui.confirmation.accept(intent)
    begin.assert_not_called()
    discard.assert_called_once_with(intent.request)
    assert ctx.ui.confirmation.pending is None


def test_native_aba_is_rejected_by_original_bank_witness_without_recapture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = SimpleNamespace(
        project=harness.app.project,
        banks=harness.controller,
        arrangement=harness.app.arrangement,
        pairs=harness.app.pairs,
        diagnostics=ConsoleDiagnostics(),
        _close_requested=False,
    )
    confirmation = PerformanceConfirmationController(cast("AppController", app))
    before = harness.app.project.model_dump(), harness.app.session.model_dump()
    confirmation.request_bank_copy(0, 1)
    intent = confirmation.pending
    assert isinstance(intent, BankConfirmation)
    witness = harness.native.participants[0]
    assert intent.request.participants is cast("BankParticipants", witness)
    witness.current = False
    # Musical equality cannot authorize Native replacement: begin retains the original witness.
    assert confirmation.is_current(intent)
    confirmation.accept(intent)
    confirmation.accept(intent)
    for _ in range(3):
        harness.controller.poll()
    assert harness.controller.status == "cancelled"
    assert "original Native Bank participants are stale" in capsys.readouterr().err
    assert len(harness.native.participants) == 1
    assert not harness.native.bank_captures
    assert not harness.native.bank_publications
    assert not harness.tasks
    assert (harness.app.project.model_dump(), harness.app.session.model_dump()) == before
    assert harness.app.persistence._migration_owner is None
