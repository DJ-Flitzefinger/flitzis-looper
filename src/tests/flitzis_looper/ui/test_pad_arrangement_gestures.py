"""Real gesture intent with Native binding and mutation-admission boundaries doubled.

These control tests establish mouse ownership and exact dispatch. Native effects,
durable settlement and rendered pointer delivery have their separate fixtures.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING
from unittest.mock import Mock, PropertyMock

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.models import PadContentIdentity
from flitzis_looper.ui.context import UiContext
from tests.flitzis_looper.conftest import FakeInputRuntimePadBinding

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.ui.pad_arrangement import ArrangementButton, PadArrangementGestures


class _Binding(FakeInputRuntimePadBinding):
    """Mutable currentness at the original opaque Native-binding boundary."""

    def __init__(self) -> None:
        super().__init__(0)
        self.current = True

    def is_current(self) -> bool:
        return self.current


@dataclass(slots=True)
class _Rig:
    app: AppController
    gestures: PadArrangementGestures
    binding: _Binding
    lookup: Mock
    dispatches: dict[str, Mock]

    def assert_no_dispatch(self) -> None:
        for dispatch in self.dispatches.values():
            dispatch.assert_not_called()


@pytest.fixture
def rig(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> _Rig:
    controller.project.sample_paths[0] = "samples/original.wav"
    controller.project.pad_content[0] = PadContentIdentity(instance_id="1" * 32)
    binding = _Binding()
    lookup = Mock(return_value=binding)
    monkeypatch.setattr(audio_engine_mock, "current_input_runtime_pad_binding", lookup)
    dispatches = {
        "pair": Mock(return_value=True),
        "copy": Mock(return_value=True),
        "overwrite": Mock(return_value=True),
    }
    monkeypatch.setattr(controller.pairs, "begin", dispatches["pair"])
    monkeypatch.setattr(controller.arrangement, "copy_to_empty", dispatches["copy"])
    monkeypatch.setattr(controller.arrangement, "copy_overwrite", dispatches["overwrite"])
    return _Rig(controller, UiContext(controller).ui.arrangement, binding, lookup, dispatches)


def test_mode_is_transient_off_and_does_not_change_project_persistence_or_midi(
    rig: _Rig, audio_engine_mock: Mock
) -> None:
    project = rig.app.project.model_dump(mode="json")
    session = rig.app.session.model_dump(mode="json")
    persistence = (rig.app.persistence.revision, rig.app.persistence._dirty)
    audio_engine_mock.reset_mock()

    assert not rig.gestures.enabled
    rig.gestures.press(0, "left")
    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "left")
    assert not rig.gestures.owns_pointer
    assert rig.gestures.source_id is None
    rig.gestures.toggle()
    assert rig.gestures.enabled
    assert not UiContext(rig.app).ui.arrangement.enabled
    rig.gestures.toggle()

    assert rig.app.project.model_dump(mode="json") == project
    assert rig.app.session.model_dump(mode="json") == session
    assert (rig.app.persistence.revision, rig.app.persistence._dirty) == persistence
    audio_engine_mock.set_input_mapping_enabled.assert_not_called()
    audio_engine_mock.set_input_mapping_snapshot.assert_not_called()
    audio_engine_mock.set_input_runtime_state.assert_not_called()
    audio_engine_mock.start_midi_input.assert_not_called()
    audio_engine_mock.stop_midi_input.assert_not_called()
    rig.lookup.assert_not_called()
    rig.assert_no_dispatch()


@pytest.mark.parametrize(
    ("button", "occupied", "label", "dispatch", "pair_operation"),
    [
        ("left", False, "Move: PAD 1 → PAD 2", "pair", "move"),
        ("left", True, "Swap: PAD 1 → PAD 2", "pair", "swap"),
        ("right", False, "Copy: PAD 1 → PAD 2", "copy", None),
        ("right", True, "Copy overwrite: PAD 1 → PAD 2", "overwrite", None),
    ],
)
def test_valid_drop_identifies_operation_and_dispatches_original_binding_once(
    rig: _Rig,
    button: ArrangementButton,
    label: str,
    dispatch: str,
    pair_operation: str | None,
    *,
    occupied: bool,
) -> None:
    if occupied:
        rig.app.project.sample_paths[1] = "samples/target.wav"
        rig.app.project.pad_content[1] = PadContentIdentity(instance_id="2" * 32)
    rig.gestures.toggle()
    rig.gestures.press(0, button)
    assert rig.gestures.source_id == 0
    assert rig.gestures.owns_pointer
    rig.assert_no_dispatch()
    preview = rig.gestures.preview(1)
    assert preview is not None
    assert (preview.source, preview.target, preview.label) == (0, 1, label)

    # A fresh lookup would return another object. Only the actual original
    # press-time binding may cross the final existing controller boundary.
    rig.lookup.return_value = _Binding()
    assert rig.gestures.release(1, button)
    options: dict[str, object] = {"source_binding": rig.binding}
    if pair_operation is not None:
        options["operation"] = pair_operation
    rig.dispatches[dispatch].assert_called_once_with(0, 1, **options)
    for name, other in rig.dispatches.items():
        if name != dispatch:
            other.assert_not_called()
    rig.lookup.assert_called_once_with(0)
    assert rig.gestures.source_id is None
    assert rig.gestures.owns_pointer
    assert not rig.gestures.release(1, button)
    rig.dispatches[dispatch].assert_called_once()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer


@pytest.mark.parametrize("target", [-1, NUM_SAMPLES, True, 0])
def test_invalid_or_self_target_is_noop_and_consumes_the_original_release(
    rig: _Rig, target: int
) -> None:
    rig.gestures.toggle()
    rig.gestures.press(0, "left")
    assert rig.gestures.preview(target) is None
    assert not rig.gestures.release(target, "left")
    assert rig.gestures.source_id is None
    assert rig.gestures.owns_pointer
    rig.assert_no_dispatch()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer


@pytest.mark.parametrize("source", [-1, NUM_SAMPLES, True, 2])
def test_invalid_or_empty_source_consumes_pointer_without_native_admission(
    rig: _Rig, source: int
) -> None:
    rig.gestures.toggle()
    rig.gestures.press(source, "right")
    assert rig.gestures.source_id is None
    assert rig.gestures.owns_pointer
    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "right")
    rig.lookup.assert_not_called()
    rig.assert_no_dispatch()
    rig.gestures.poll(left_down=False, right_down=True)
    assert rig.gestures.owns_pointer
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer


@pytest.mark.parametrize("binding_state", ["missing", "stale"])
def test_unavailable_native_source_cannot_start_a_gesture(rig: _Rig, binding_state: str) -> None:
    if binding_state == "missing":
        rig.lookup.return_value = None
    else:
        rig.binding.current = False
    rig.gestures.toggle()
    rig.gestures.press(0, "right")

    assert rig.gestures.source_id is None
    assert rig.gestures.owns_pointer
    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "right")
    rig.lookup.assert_called_once_with(0)
    rig.assert_no_dispatch()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer


@pytest.mark.parametrize("cancellation", ["explicit", "focus", "mode_off"])
def test_cancellation_keeps_consumption_until_matching_button_up(
    rig: _Rig, cancellation: str
) -> None:
    rig.gestures.toggle()
    rig.gestures.press(0, "left")
    if cancellation == "explicit":
        rig.gestures.cancel()
    elif cancellation == "focus":
        rig.gestures.poll(left_down=True, right_down=False, cancelled=True)
    else:
        rig.gestures.toggle()
        assert not rig.gestures.enabled

    assert rig.gestures.source_id is None
    assert rig.gestures.owns_pointer
    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "left")
    rig.gestures.poll(left_down=True, right_down=False)
    assert rig.gestures.owns_pointer
    rig.assert_no_dispatch()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer
    if not rig.gestures.enabled:
        rig.gestures.toggle()
    rig.gestures.press(0, "left")
    assert rig.gestures.release(1, "left")
    rig.dispatches["pair"].assert_called_once()


def test_wrong_button_release_keeps_original_source_and_cannot_dispatch(rig: _Rig) -> None:
    rig.gestures.toggle()
    rig.gestures.press(0, "left")
    assert not rig.gestures.release(1, "right")
    rig.gestures.poll(left_down=True, right_down=False)
    assert rig.gestures.source_id == 0
    assert rig.gestures.owns_pointer
    rig.assert_no_dispatch()
    assert rig.gestures.release(1, "left")
    rig.dispatches["pair"].assert_called_once_with(
        0, 1, operation="move", source_binding=rig.binding
    )


@pytest.mark.parametrize("change", ["same_path_content", "path", "stale_binding", "source_aba"])
def test_changed_original_source_never_recaptures_or_mutates_a_new_occupant(
    rig: _Rig, change: str
) -> None:
    original_content = rig.app.project.pad_content[0]
    original_path = rig.app.project.sample_paths[0]
    rig.gestures.toggle()
    rig.gestures.press(0, "right")
    if change == "same_path_content":
        rig.app.project.pad_content[0] = PadContentIdentity(instance_id="3" * 32)
        assert rig.app.project.sample_paths[0] == original_path
    elif change == "path":
        rig.app.project.sample_paths[0] = "samples/rebound.wav"
    elif change == "source_aba":
        rig.app.project.pad_content[0] = PadContentIdentity(instance_id="3" * 32)
        rig.app.project.pad_content[0] = original_content
        rig.app.project.sample_paths[0] = original_path
        rig.binding.current = False
    else:
        rig.binding.current = False
    rig.lookup.return_value = _Binding()

    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "right")
    assert rig.gestures.owns_pointer
    rig.lookup.assert_called_once_with(0)
    rig.assert_no_dispatch()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer


@pytest.mark.parametrize("busy_owner", ["pair", "copy"])
@pytest.mark.parametrize("became_busy_after_press", [False, True])
def test_busy_mutation_owner_fences_new_and_already_pressed_gestures(
    rig: _Rig,
    monkeypatch: pytest.MonkeyPatch,
    busy_owner: str,
    *,
    became_busy_after_press: bool,
) -> None:
    rig.gestures.toggle()
    if became_busy_after_press:
        rig.gestures.press(0, "left")
    owner = rig.app.pairs if busy_owner == "pair" else rig.app.arrangement
    monkeypatch.setattr(type(owner), "active", PropertyMock(return_value=True))
    if not became_busy_after_press:
        rig.gestures.press(0, "left")

    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "left")
    assert rig.gestures.owns_pointer
    rig.assert_no_dispatch()
    if not became_busy_after_press:
        rig.lookup.assert_not_called()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer


@pytest.mark.parametrize("loading_slot", [0, 1])
@pytest.mark.parametrize("started_after_press", [False, True])
def test_pending_source_or_target_load_cannot_admit_arrangement(
    rig: _Rig, loading_slot: int, *, started_after_press: bool
) -> None:
    rig.gestures.toggle()
    if started_after_press:
        rig.gestures.press(0, "right")
    rig.app.session.loading_sample_ids.add(loading_slot)
    if not started_after_press:
        rig.gestures.press(0, "right")

    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "right")
    assert rig.gestures.owns_pointer
    rig.assert_no_dispatch()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer


@pytest.mark.parametrize("partial_target", ["path_only", "content_only"])
def test_incomplete_target_projection_is_noop(rig: _Rig, partial_target: str) -> None:
    if partial_target == "path_only":
        rig.app.project.sample_paths[1] = "samples/settling.wav"
    else:
        rig.app.project.pad_content[1] = PadContentIdentity(instance_id="2" * 32)
    rig.gestures.toggle()
    rig.gestures.press(0, "left")

    assert rig.gestures.preview(1) is None
    assert not rig.gestures.release(1, "left")
    rig.assert_no_dispatch()
    rig.gestures.poll(left_down=False, right_down=False)
    assert not rig.gestures.owns_pointer
