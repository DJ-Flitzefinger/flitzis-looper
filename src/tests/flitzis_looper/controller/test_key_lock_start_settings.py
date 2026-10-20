"""Settings transactions only; Native playback admission and DSP need their own proofs."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, patch

import pytest
from pydantic import ValidationError

import flitzis_looper.controller.app as app_module
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.models import (
    KEY_LOCK_START_BEHAVIORS,
    ProjectState,
    SessionState,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from flitzis_looper.models import KeyLockStartBehavior
    from flitzis_looper_audio import AudioEngine


@dataclass
class _NativeStartPolicy:
    """Control-flow double, with no claim about real voices, silence or retained histories."""

    behavior: KeyLockStartBehavior = "first_sound_at_grid"
    stopped: bool = True
    requests: list[str] = field(default_factory=list)
    admission_hook: Callable[[], None] | None = None

    def key_lock_start_behavior(self) -> str:
        return self.behavior

    def can_change_key_lock_start_behavior(self) -> bool:
        return self.stopped

    def set_key_lock_start_behavior(self, behavior: str) -> bool:
        self.requests.append(behavior)
        if self.admission_hook is not None:
            self.admission_hook()
        if not self.stopped:
            return False
        assert behavior in KEY_LOCK_START_BEHAVIORS
        self.behavior = cast("KeyLockStartBehavior", behavior)
        return True


def _settings(
    project: ProjectState,
    native: _NativeStartPolicy,
    changed: Callable[[], None] | None = None,
    *,
    session: SessionState | None = None,
) -> SettingsController:
    return SettingsController(
        project,
        SessionState() if session is None else session,
        cast("AudioEngine", native),
        on_project_changed=changed,
    )


def _assert_policy(
    project: ProjectState, native: _NativeStartPolicy, expected: KeyLockStartBehavior
) -> None:
    """Observe current fields afresh after a settings transaction may mutate them."""
    assert project.key_lock_start_behavior == expected
    assert native.behavior == expected


def test_legacy_config_uses_first_sound_and_both_policies_round_trip() -> None:
    assert ProjectState.model_validate_json("{}").key_lock_start_behavior == "first_sound_at_grid"
    for behavior in KEY_LOCK_START_BEHAVIORS:
        project = ProjectState(key_lock_start_behavior=behavior)
        reopened = ProjectState.model_validate_json(project.model_dump_json())
        assert reopened.key_lock_start_behavior == behavior


@pytest.mark.parametrize("invalid", [None, True, 1, "attack", "", [], {}, b"attack_at_grid"])
def test_invalid_policy_is_rejected_on_load_and_assignment(invalid: object) -> None:
    with pytest.raises(ValidationError):
        ProjectState.model_validate({"key_lock_start_behavior": invalid})
    project = ProjectState()
    with pytest.raises(ValidationError):
        project.key_lock_start_behavior = cast("KeyLockStartBehavior", invalid)
    assert project.key_lock_start_behavior == "first_sound_at_grid"


@pytest.mark.parametrize("behavior", KEY_LOCK_START_BEHAVIORS)
def test_native_accepts_before_global_persistence_without_other_musical_edits(
    behavior: KeyLockStartBehavior,
) -> None:
    previous: KeyLockStartBehavior = (
        "attack_at_grid" if behavior == "first_sound_at_grid" else "first_sound_at_grid"
    )
    project = ProjectState(key_lock_start_behavior=previous, key_lock=True, speed=1.25)
    project.manual_bpm[0] = 127.5
    project.pad_loop_start_s[0] = 3.5
    project.pad_loop_end_s[0] = 9.0
    project.pad_key_intent[0] = project.pad_key_intent[0].changed(base_shift=12, extra_shift=2)
    # Deliberately stale UI telemetry cannot override Native's authoritative guard.
    session = SessionState(active_sample_ids={0})
    session.master_bpm = 112.0
    original_project = project.model_dump()
    original_session = session.model_dump()
    native = _NativeStartPolicy(behavior=previous)

    def before_admission() -> None:
        assert project.model_dump() == original_project

    def after_admission() -> None:
        assert native.behavior == project.key_lock_start_behavior == behavior

    native.admission_hook = before_admission
    changed = Mock(side_effect=after_admission)
    settings = _settings(project, native, changed, session=session)

    assert settings.can_change_key_lock_start_behavior()
    assert settings.set_key_lock_start_behavior(behavior)

    assert native.requests == [behavior]
    assert native.key_lock_start_behavior() == behavior
    assert project.key_lock_start_behavior == behavior
    expected = original_project | {"key_lock_start_behavior": behavior}
    assert project.model_dump() == expected
    assert session.model_dump() == original_session
    changed.assert_called_once_with()


def test_refused_policy_does_not_dirty_or_queue_a_future_choice() -> None:
    project = ProjectState()
    native = _NativeStartPolicy(stopped=False)
    changed = Mock()
    settings = _settings(project, native, changed)
    original = project.model_dump()

    assert not settings.can_change_key_lock_start_behavior()
    assert not settings.set_key_lock_start_behavior("attack_at_grid")
    assert project.model_dump() == original
    assert native.behavior == "first_sound_at_grid"
    changed.assert_not_called()

    native.stopped = True
    assert settings.can_change_key_lock_start_behavior()
    settings.on_frame_render()
    assert native.requests == ["attack_at_grid"]
    assert project.model_dump() == original
    assert native.behavior == "first_sound_at_grid"
    changed.assert_not_called()


def test_playback_admission_between_guard_and_set_preserves_the_accepted_policy() -> None:
    project = ProjectState()
    native = _NativeStartPolicy()
    changed = Mock()
    settings = _settings(project, native, changed)
    assert settings.can_change_key_lock_start_behavior()
    native.admission_hook = lambda: setattr(native, "stopped", False)

    assert not settings.set_key_lock_start_behavior("attack_at_grid")

    assert native.behavior == project.key_lock_start_behavior == "first_sound_at_grid"
    changed.assert_not_called()
    native.admission_hook = None
    native.stopped = True
    assert settings.can_change_key_lock_start_behavior()
    assert native.behavior == project.key_lock_start_behavior == "first_sound_at_grid"
    assert native.requests == ["attack_at_grid"]
    assert settings.set_key_lock_start_behavior("attack_at_grid")
    _assert_policy(project, native, "attack_at_grid")
    changed.assert_called_once_with()


def test_reselecting_the_accepted_policy_still_checks_native_without_dirtying() -> None:
    project = ProjectState()
    native = _NativeStartPolicy()
    changed = Mock()
    settings = _settings(project, native, changed)
    assert settings.set_key_lock_start_behavior("first_sound_at_grid")
    native.stopped = False
    assert not settings.set_key_lock_start_behavior("first_sound_at_grid")
    assert native.requests == ["first_sound_at_grid", "first_sound_at_grid"]
    changed.assert_not_called()


@pytest.mark.parametrize("invalid", [None, True, 1, "unknown", []])
def test_invalid_controller_request_cannot_reach_native_or_dirty(invalid: object) -> None:
    project = ProjectState()
    native, changed = _NativeStartPolicy(), Mock()
    with pytest.raises(ValueError, match="supported launch policy"):
        _settings(project, native, changed).set_key_lock_start_behavior(
            cast("KeyLockStartBehavior", invalid)
        )
    assert native.requests == []
    assert project.key_lock_start_behavior == "first_sound_at_grid"
    changed.assert_not_called()


def test_json_reopen_retains_only_an_accepted_policy(tmp_path: Path) -> None:
    persistence = ProjectPersistence()
    persistence.config_path = tmp_path / "project.json"
    persistence.mark_dirty()
    persistence.flush()
    native = _NativeStartPolicy(stopped=False)
    settings = _settings(persistence.project, native, persistence.mark_dirty)
    original_bytes = persistence.config_path.read_bytes()

    assert not settings.set_key_lock_start_behavior("attack_at_grid")
    assert not persistence.flush_if_dirty()
    assert persistence.config_path.read_bytes() == original_bytes
    native.stopped = True
    assert settings.set_key_lock_start_behavior("attack_at_grid")
    assert persistence.flush_if_dirty()
    reopened = ProjectPersistence.from_config_path(persistence.config_path).project
    assert reopened.key_lock_start_behavior == "attack_at_grid"
    assert reopened.pad_key_lock == persistence.project.pad_key_lock


class _StoppedAtMockRunError(RuntimeError):
    """Stop construction at a mocked run boundary; no real engine or device is started."""


@pytest.mark.parametrize("behavior", KEY_LOCK_START_BEHAVIORS)
def test_startup_restores_persisted_policy_before_mocked_audio_run(
    tmp_path: Path, behavior: KeyLockStartBehavior
) -> None:
    path = tmp_path / "project.json"
    path.write_text(
        ProjectState(key_lock_start_behavior=behavior).model_dump_json(), encoding="utf-8"
    )
    native = _NativeStartPolicy()
    engine = Mock()
    engine.set_key_lock_start_behavior.side_effect = native.set_key_lock_start_behavior

    def stop_at_mock_run() -> None:
        assert native.key_lock_start_behavior() == behavior
        raise _StoppedAtMockRunError

    engine.run.side_effect = stop_at_mock_run
    with (
        patch.object(app_module, "AudioEngine", return_value=engine),
        pytest.raises(_StoppedAtMockRunError),
    ):
        app_module.AppController(project_config_path=path)
    assert native.requests == [behavior]
    engine.run.assert_called_once_with()


def test_startup_refusal_never_starts_audio_with_a_different_policy(tmp_path: Path) -> None:
    path = tmp_path / "project.json"
    path.write_text(
        ProjectState(key_lock_start_behavior="attack_at_grid").model_dump_json(), encoding="utf-8"
    )
    engine = Mock()
    engine.set_key_lock_start_behavior.return_value = False
    with (
        patch.object(app_module, "AudioEngine", return_value=engine),
        pytest.raises(RuntimeError, match="before audio startup"),
    ):
        app_module.AppController(project_config_path=path)
    engine.set_key_lock_start_behavior.assert_called_once_with("attack_at_grid")
    engine.run.assert_not_called()
    reopened = ProjectState.model_validate_json(path.read_text(encoding="utf-8"))
    assert reopened.key_lock_start_behavior == "attack_at_grid"
