"""App orchestration uses the existing owners; Native acceptance is tested separately."""

from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import app as app_module
from flitzis_looper.controller import material_migration as migration_module
from flitzis_looper.controller.bank_arrangement import BankArrangementController
from flitzis_looper.controller.material_migration_recovery import MigrationRecovery

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper_audio import AudioMessage

SOURCE = 0
TARGET = 36


def test_bank_startup_owns_existing_app_components_before_ordinary_restore(
    audio_engine_mock: Mock, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    restored = Mock()

    def restore(app: AppController) -> None:
        assert isinstance(app.banks, BankArrangementController)
        assert app.banks._app is app
        assert app.banks._startup is not None
        assert app.banks._startup._app is app
        assert app.loader._pair_deferred_sample_ids == frozenset()
        assert app.stems._pair_deferred_sample_ids == frozenset()
        restored()

    monkeypatch.setattr(app_module.AppController, "_restore_initial_runtime_state", restore)
    app = app_module.AppController(stem_task_runner=lambda target: target())

    restored.assert_called_once()
    assert not app.banks.active
    assert not app._arrangement_recovery_pending
    audio_engine_mock.capture_bank_participants.assert_not_called()


@pytest.mark.parametrize("owner", ["arrangement", "pairs"])
def test_active_bank_rejects_copy_and_pair_before_native_capture(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch, owner: str
) -> None:
    monkeypatch.setattr(BankArrangementController, "active", property(lambda _self: True))
    before = controller.project.model_dump()
    audio_engine_mock.reset_mock()

    if owner == "arrangement":
        assert not controller.arrangement.copy_to_empty(SOURCE, TARGET)
    else:
        assert not controller.pairs.begin(SOURCE, TARGET, operation="swap")

    assert controller.project.model_dump() == before
    assert controller.persistence._migration_owner is None
    assert audio_engine_mock.mock_calls == []


@pytest.mark.parametrize("owner", ["arrangement", "pairs"])
def test_other_arrangement_rejects_bank_before_original_capture(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch, owner: str
) -> None:
    monkeypatch.setattr(type(getattr(controller, owner)), "active", property(lambda _self: True))
    audio_engine_mock.reset_mock()

    with pytest.raises(RuntimeError, match="Another complete arrangement owns admission"):
        controller.banks.capture_request(0, 1)

    assert controller.persistence._migration_owner is None
    assert audio_engine_mock.mock_calls == []


@pytest.mark.parametrize("action", ["gain", "eq", "mask", "mode", "delete", "generate", "capture"])
def test_bank_participant_guard_precedes_project_and_native_edits(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
    action: str,
) -> None:
    monkeypatch.setattr(
        BankArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    edits: dict[str, Callable[[], object]] = {
        "gain": lambda: controller.transport.pad.set_pad_gain(TARGET, -6.0),
        "eq": lambda: controller.transport.pad.set_pad_eq(TARGET, -3.0, 2.0, 1.0),
        "mask": lambda: controller.stems.set_stem_enabled_mask(TARGET, 0),
        "mode": lambda: controller.stems.set_stem_mix_mode(TARGET, "full_mix"),
        "delete": lambda: controller.stems.delete_stems(TARGET),
        "generate": lambda: controller.stems.generate_stems_async(TARGET),
        "capture": lambda: controller.transport.capture_current_pad_edit_generation(TARGET),
    }
    before = controller.project.model_dump()
    session = controller.session.model_dump()
    revision = controller.persistence.revision
    audio_engine_mock.reset_mock()

    with pytest.raises(ValueError, match="Complete Bank receipt is pending"):
        edits[action]()

    assert controller.project.model_dump() == before
    assert controller.session.model_dump() == session
    assert controller.persistence.revision == revision
    assert audio_engine_mock.mock_calls == []
    controller.ensure_not_copy_target(SOURCE)


def test_bank_waiting_restore_keeps_assignment_and_gesture_fences_without_deferring_observers(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    observer_fence: set[int] = set()
    monkeypatch.setattr(
        BankArrangementController, "fenced_sample_ids", property(lambda _self: frozenset({TARGET}))
    )
    monkeypatch.setattr(
        BankArrangementController,
        "observer_fenced_sample_ids",
        property(lambda _self: frozenset(observer_fence)),
    )
    observers = (
        controller.transport,
        controller.loader,
        controller.accepted_timing,
        controller.stems,
    )

    controller._sync_arrangement_observers()

    assert controller._restored_assignment_held(TARGET)
    assert all(component._pair_deferred_sample_ids == frozenset() for component in observers)
    with pytest.raises(ValueError, match="Complete Bank receipt is pending"):
        controller.ensure_not_copy_target(TARGET)

    observer_fence.add(TARGET)
    controller._sync_arrangement_observers()
    assert all(
        component._pair_deferred_sample_ids == frozenset({TARGET}) for component in observers
    )
    observer_fence.clear()
    controller._sync_arrangement_observers()
    assert controller._restored_assignment_held(TARGET)
    assert all(component._pair_deferred_sample_ids == frozenset() for component in observers)


def test_bank_feedback_retains_original_message_until_projection_while_outside_continues(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    observer_fence = {TARGET}
    monkeypatch.setattr(
        BankArrangementController,
        "observer_fenced_sample_ids",
        property(lambda _self: frozenset(observer_fence)),
    )
    original = Mock()
    unrelated = Mock()
    original.feedback_kind.return_value = unrelated.feedback_kind.return_value = "started"
    handler = Mock()
    monkeypatch.setattr(controller.transport.playback, "handle_sample_started_message", handler)
    monkeypatch.setattr(
        controller._audio, "_resolve_pad_feedback", Mock(side_effect=[TARGET, SOURCE, TARGET])
    )
    monkeypatch.setattr(controller._audio, "receive_msg", Mock(return_value=None))

    controller._handle_audio_message(cast("AudioMessage", original))
    controller._handle_audio_message(cast("AudioMessage", unrelated))

    assert list(controller._pending_content_feedback) == [original]
    handler.assert_called_once_with(unrelated, sample_id=SOURCE)
    observer_fence.clear()
    controller._poll_audio_messages()
    assert handler.call_count == 2
    assert handler.call_args.args[0] is original
    assert handler.call_args.kwargs == {"sample_id": TARGET}
    assert not controller._pending_content_feedback


@pytest.mark.parametrize("last_owner", ["arrangement", "pairs", "banks"])
def test_only_actual_all_owner_settlement_refreshes_material_recovery_once(
    controller: AppController, monkeypatch: pytest.MonkeyPatch, last_owner: str
) -> None:
    active = {"arrangement", "pairs", "banks"}
    names = {type(getattr(controller, name)): name for name in active}

    def current(owner: object) -> bool:
        return names[type(owner)] in active

    for owner_type in names:
        monkeypatch.setattr(owner_type, "active", property(current))
    controller._arrangement_recovery_pending = True
    refresh = Mock(return_value=True)
    monkeypatch.setattr(controller.material_migration, "refresh_arrangement_recovery", refresh)

    controller._sync_arrangement_observers()
    active.intersection_update({last_owner})
    controller._sync_arrangement_observers()
    refresh.assert_not_called()
    assert controller._arrangement_recovery_pending
    active.clear()
    controller._sync_arrangement_observers()
    controller._sync_arrangement_observers()

    refresh.assert_called_once_with()
    assert not controller._arrangement_recovery_pending


def test_failed_material_refresh_stays_visible_deferred_and_explicitly_retriable(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    service = controller.material_migration
    service._arrangement_recovery_deferred = True
    controller._arrangement_recovery_pending = True
    scan = Mock(side_effect=OSError("owned history temporarily unavailable"))
    monkeypatch.setattr(migration_module, "inspect_migration_recovery", scan)

    controller._sync_arrangement_observers()
    controller._sync_arrangement_observers()

    scan.assert_called_once()
    assert service._arrangement_recovery_deferred
    assert service.status == "unresolved"
    assert "owned history temporarily unavailable" in (controller.pending_pad_error or "")
    assert not controller._arrangement_recovery_pending
    scan.side_effect = None
    scan.return_value = MigrationRecovery()
    service.retry_recovery()
    assert scan.call_count == 2
    assert not service._arrangement_recovery_deferred
    assert service.error is None
    assert controller.persistence._migration_owner is None


def test_explicit_material_retry_cannot_bypass_retained_bank_with_terminal_disk_history(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    service = controller.material_migration
    service._arrangement_recovery_deferred = True
    bank_active = [True]
    monkeypatch.setattr(BankArrangementController, "active", property(lambda _self: bank_active[0]))
    # A strict terminal-only scan has no unfinished kind. The actual Bank may
    # still own exact ancestor compaction and readers after its terminal append.
    terminal_scan = Mock(return_value=MigrationRecovery())
    monkeypatch.setattr(migration_module, "inspect_migration_recovery", terminal_scan)

    with pytest.raises(ValueError, match="Arrangement recovery retains its actual owners"):
        service.retry_recovery()

    terminal_scan.assert_not_called()
    assert service._arrangement_recovery_deferred
    assert service.status == "unresolved"
    with pytest.raises(RuntimeError, match="arrangement recovery"):
        service.begin(SOURCE)
    bank_active[0] = False
    service.retry_recovery()
    terminal_scan.assert_called_once()
    assert not service._arrangement_recovery_deferred
    assert service.error is None


def test_bank_shutdown_retains_other_owners_workers_and_native_until_actual_settlement(
    controller: AppController, monkeypatch: pytest.MonkeyPatch
) -> None:
    ready = Mock(return_value=False)
    monkeypatch.setattr(controller.banks, "prepare_shutdown", ready)
    hooks = [
        (controller.pairs, "prepare_shutdown"),
        (controller.arrangement, "prepare_shutdown"),
        (controller.material_migration, "shut_down"),
        (controller.transport.pitch, "shut_down"),
        (controller.transport.residency, "shut_down"),
        (controller.accepted_timing, "shut_down"),
        (controller.loader, "shut_down"),
        (controller.stems, "shut_down"),
        (controller._assets, "release_saved_assignments"),
        (controller._audio, "shut_down"),
    ]
    pending = []
    for owner, method in hooks:
        hook = Mock(return_value=True)
        monkeypatch.setattr(owner, method, hook)
        pending.append(hook)
    controller.banks.error = "Bank retains its original receipt and readers"

    assert not controller.prepare_shutdown()
    with pytest.raises(RuntimeError, match="waiting for recording or arrangement settlement"):
        controller.shut_down()

    assert all(hook.call_count == 0 for hook in pending)
    assert controller.pending_pad_error == controller.banks.error
    assert not controller._shutdown_complete
    ready.return_value = True
    controller.shut_down()
    assert all(hook.call_count == 1 for hook in pending)
    assert controller._shutdown_complete
