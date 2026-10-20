"""Async Bank rejection feedback with real Controller settlement and scripted Native.

The existing harness supplies opaque Native doubles. These checks prove truthful
control feedback and original-owner cleanup, not a Native rejection cause, audio
callback behavior, resource admission or Human listening acceptance.
"""

from threading import Lock
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.control_diagnostics import ConsoleDiagnostics, poll_control_diagnostics
from tests.flitzis_looper.controller.test_bank_arrangement_controller import _harness, _records

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.bank_arrangement import _Bank
    from flitzis_looper_audio import BankParticipants
    from tests.flitzis_looper.controller.test_bank_arrangement_controller import _Harness

_REJECTION = "Native Bank mutation was rejected before acceptance; retry the Bank action."


def _published(harness: _Harness, *, clear: bool = False) -> _Bank:
    """Publish one original request, retaining its pending opaque Native ticket."""
    harness.native.auto_ack = False
    request = harness.controller.capture_request(None if clear else 0, 1)
    assert harness.controller.begin(request)
    bank = harness.current()
    harness.run_worker()
    harness.controller.poll()
    assert bank.request is request
    assert bank.published
    assert harness.native.bank_captures[0].phase == "adopting"
    assert bank.acknowledgement is None
    return bank


def _diagnostic_app(harness: _Harness) -> AppController:
    """Observe the actual Bank error while neighboring cached projections stay healthy."""
    return cast(
        "AppController",
        SimpleNamespace(
            diagnostics=ConsoleDiagnostics(),
            session=harness.app.session,
            persistence=harness.app.persistence,
            banks=harness.controller,
            pairs=SimpleNamespace(error=None),
            arrangement=SimpleNamespace(error=None),
            stem_batch=SimpleNamespace(summary=None, _rows=None),
            stem_delete=SimpleNamespace(summary=None, _rows=None),
            _assets=SimpleNamespace(_retirement_lock=Lock(), retirement_errors={}),
            _audio=SimpleNamespace(project_asset_cleanup_status=lambda: (0, 0, 0, [])),
            transport=SimpleNamespace(
                global_params=SimpleNamespace(
                    global_rate_status=lambda: SimpleNamespace(error=None, request_revision=None)
                ),
                waveform=SimpleNamespace(hold_status=lambda: (None, None)),
            ),
        ),
    )


@pytest.mark.parametrize("clear", [False, True])
def test_unsolicited_published_rejection_reports_error_and_settles_exact_original_without_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, clear: bool
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    before = harness.app.project.model_dump(), harness.app.session.model_dump()
    config = harness.app.persistence.config_path.read_bytes()
    bank = _published(harness, clear=clear)
    original = bank.request
    captured = harness.native.bank_captures[0]
    assert cast("BankParticipants", captured.participants) is original.participants
    captured.phase = "rejected"

    harness.controller.poll()

    assert harness.controller.error == _REJECTION
    assert bank.failure == _REJECTION
    assert harness.controller.status == "cancelled"
    assert not harness.controller.active
    assert bank.request is original
    assert bank.acknowledgement is None
    assert bank.receipt is None
    assert not bank.native_finished
    assert bank.cancellation_proven
    assert bank.writer_released
    assert not bank.images
    assert not bank.custody.readers
    assert harness.app.persistence._migration_owner is None
    assert harness.app.persistence.config_path.read_bytes() == config
    assert (harness.app.project.model_dump(), harness.app.session.model_dump()) == before
    assert len(harness.native.participants) == len(harness.native.bank_captures) == 1
    assert len(harness.native.bank_publications) == 1
    assert all(owner is captured for owner in harness.native.bank_cancellations)
    assert not harness.native.bank_finishes
    assert not harness.tasks
    records = _records(bank)
    assert [record.phase for record in records] == ["captured", "publication_pending", "cancelled"]
    assert records[-1].error == _REJECTION
    assert records[-1].image_role == "before"
    assert records[-1].committed_revision is None


def test_async_rejection_reaches_console_once_and_next_attempt_clears_deduplication(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = _diagnostic_app(harness)
    _published(harness)
    poll_control_diagnostics(app)
    assert not capsys.readouterr().err
    harness.native.bank_captures[0].phase = "rejected"
    harness.controller.poll()

    for _ in range(5):
        harness.controller.poll()
        poll_control_diagnostics(app)

    lines = capsys.readouterr().err.splitlines()
    assert len(lines) == 1
    assert f"operation=bank_arrangement reason={_REJECTION}" in lines[0]
    request = harness.controller.capture_request(0, 1)
    assert harness.controller.begin(request)
    assert harness.controller.error is None
    poll_control_diagnostics(app)
    assert not capsys.readouterr().err
    harness.run_worker()
    harness.controller.poll()
    harness.native.bank_captures[1].phase = "rejected"
    harness.controller.poll()
    poll_control_diagnostics(app)
    poll_control_diagnostics(app)
    second = capsys.readouterr().err.splitlines()
    assert len(second) == 1
    assert f"operation=bank_arrangement reason={_REJECTION}" in second[0]
    assert len(harness.native.participants) == len(harness.native.bank_captures) == 2


def test_deliberate_cancel_then_native_rejection_is_not_reported_as_a_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    app = _diagnostic_app(harness)
    bank = _published(harness)
    captured = harness.native.bank_captures[0]
    assert not harness.controller.cancel()
    assert bank.cancel_requested
    assert harness.controller.status == "cancelling"
    captured.phase = "rejected"

    harness.controller.poll()
    poll_control_diagnostics(app)

    assert harness.controller.status == "cancelled"
    assert harness.controller.error is None
    assert bank.failure is None
    assert _records(bank)[-1].error is None
    assert not capsys.readouterr().err
    assert not harness.native.bank_finishes
    assert harness.app.persistence._migration_owner is None


def test_rejected_ticket_preserves_a_more_specific_original_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    bank = _published(harness)
    specific = "original Bank image verification refused"
    bank.failure = harness.controller.error = specific
    harness.native.bank_captures[0].phase = "rejected"

    harness.controller.poll()

    assert harness.controller.status == "cancelled"
    assert harness.controller.error == specific
    assert bank.failure == specific
    assert _records(bank)[-1].error == specific


def test_fresh_attempt_clears_rejection_and_uses_new_explicit_request_only(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    harness = _harness(tmp_path, monkeypatch)
    first = _published(harness)
    harness.native.bank_captures[0].phase = "rejected"
    harness.controller.poll()
    assert harness.controller.error == _REJECTION
    assert len(harness.native.participants) == 1

    harness.native.auto_ack = True
    request = harness.controller.capture_request(0, 1)
    assert request is not first.request
    assert harness.controller.begin(request)
    current = harness.current()
    assert current.request is request
    assert harness.controller.error is None
    assert current.failure is None
    harness.complete()

    assert harness.controller.status == "completed"
    assert harness.controller.error is None
    assert current.acknowledgement is not None
    assert len(harness.native.participants) == len(harness.native.bank_captures) == 2
    assert len(harness.native.bank_finishes) == 1
