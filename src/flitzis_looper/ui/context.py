from pathlib import Path, PureWindowsPath
from typing import TYPE_CHECKING, Any, cast

from flitzis_looper.constants import SPEED_STEP

if TYPE_CHECKING:
    from pydantic import BaseModel

    from flitzis_looper.controller import LooperController
    from flitzis_looper.models import ProjectState, SessionState


class ReadOnlyStateProxy:
    """Wraps a Pydantic model and prevents attribute assignment."""

    def __init__(self, model: BaseModel):
        self._model = model

    def __getattr__(self, name: str) -> Any:
        return getattr(self._model, name)

    def __setattr__(self, name: str, value: Any) -> None:
        if name == "_model":
            super().__setattr__(name, value)
        else:
            msg = f"State is read-only. Use controller actions to mutate '{name}'."
            raise AttributeError(msg)


class UiState:
    """Read-only proxy of app state for UI rendering."""

    def __init__(self, controller: LooperController):
        self._controller = controller
        self._project_proxy = ReadOnlyStateProxy(controller.project)
        self._session_proxy = ReadOnlyStateProxy(controller.session)

    @property
    def project(self) -> ProjectState:
        # Type hinting says ProjectState, but functionality is ReadOnly
        return cast("ProjectState", self._project_proxy)

    @property
    def session(self) -> SessionState:
        return cast("SessionState", self._session_proxy)

    # --- Computed Properties ---

    def pad_label(self, pad_id: int) -> str:
        """Derive the UI label for a performance pad.

        Args:
            pad_id: Pad number

        Returns:
            The basename of the loaded audio file, otherwise the pad number.
        """
        path = self.project.sample_paths[pad_id]
        if path is None:
            pending = self._controller.pending_sample_path(pad_id)
            if pending is None:
                return ""
            path = pending

        if "\\" in path:
            return PureWindowsPath(path).name

        return Path(path).name

    def is_pad_loaded(self, pad_id: int) -> bool:
        """Return whether a pad has audio loaded.

        Args:
            pad_id: Pad ID.

        Returns:
            True when the pad has a sample loaded.
        """
        return self._controller.is_sample_loaded(pad_id)

    def is_pad_loading(self, pad_id: int) -> bool:
        """Return whether a pad is currently loading."""
        return self._controller.is_sample_loading(pad_id)

    def pad_load_error(self, pad_id: int) -> str | None:
        """Return the last async load error message for a pad."""
        return self._controller.sample_load_error(pad_id)

    def pad_load_progress(self, pad_id: int) -> float | None:
        """Return best-effort async load progress for a pad."""
        return self._controller.sample_load_progress(pad_id)

    def is_pad_active(self, pad_id: int) -> bool:
        """Return whether a pad is currently playing audio.

        Args:
            pad_id: Pad ID.

        Returns:
            True when the pad is playing audio.
        """
        return self._controller.is_sample_active(pad_id)

    def is_pad_pressed(self, pad_id: int) -> bool:
        """Return whether a pad is currently pressed.

        Args:
            pad_id: Pad ID.

        Returns:
            True when the pad is currently pressed.
        """
        return self._controller.session.pressed_pads[pad_id]

    def is_bank_selected(self, bank_id: int) -> bool:
        """Return whether a bank is currently selected.

        Args:
            bank_id: Bank ID.

        Returns:
            True when the bank is currently selected.
        """
        return self._controller.project.selected_bank == bank_id


class AudioActions:
    """Audio-related UI actions."""

    def __init__(self, controller: LooperController):
        self._controller = controller

    def trigger_pad(self, pad_id: int) -> None:
        """Trigger a pad."""
        self._controller.trigger_pad(pad_id)

    def stop_pad(self, pad_id: int) -> None:
        """Stop audio playback for a pad."""
        self._controller.stop_pad(pad_id)

    def load_sample_async(self, pad_id: int, path: str) -> None:
        """Load an audio file to a pad asynchronously."""
        self._controller.load_sample_async(pad_id, path)

    def poll_loader_events(self) -> None:
        """Apply pending loader events from Rust."""
        self._controller.poll_loader_events()

    def unload_sample(self, pad_id: int) -> None:
        """Unload an audio file from a pad."""
        self._controller.unload_sample(pad_id)

    def set_volume(self, volume: float) -> None:
        """Set master volume."""
        self._controller.set_volume(volume)

    def set_speed(self, speed: float) -> None:
        """Set master speed."""
        self._controller.set_speed(speed)

    def reset_speed(self) -> None:
        """Reset master speed."""
        self._controller.set_speed(1.0)

    def increase_speed(self) -> None:
        """Increase master speed by increment."""
        self._controller.set_speed(self._controller.project.speed + SPEED_STEP)

    def decrease_speed(self) -> None:
        """Decrease master speed by increment."""
        self._controller.set_speed(self._controller.project.speed - SPEED_STEP)

    def toggle_multi_loop(self) -> None:
        """Toggle multi loop mode."""
        self._controller.set_multi_loop(enabled=not self._controller.project.multi_loop)

    def toggle_key_lock(self) -> None:
        """Toggle key lock mode."""
        self._controller.set_key_lock(enabled=not self._controller.project.key_lock)

    def toggle_bpm_lock(self) -> None:
        """Toggle BPM lock mode."""
        self._controller.set_bpm_lock(enabled=not self._controller.project.bpm_lock)


class UiActions:
    """UI-related actions."""

    def __init__(self, controller: LooperController):
        self._controller = controller

    def toggle_left_sidebar(self) -> None:
        """Toggle left sidebar collapsed state."""
        new_val = not self._controller.project.sidebar_left_expanded
        self._controller.project.sidebar_left_expanded = new_val

    def toggle_right_sidebar(self) -> None:
        """Toggle right sidebar collapsed state."""
        new_val = not self._controller.project.sidebar_right_expanded
        self._controller.project.sidebar_right_expanded = new_val

    def open_file_dialog(self, pad_id: int) -> None:
        """Open file dialog for pad."""
        self._controller.session.file_dialog_pad_id = pad_id

    def close_file_dialog(self) -> None:
        """Close a pending file dialog."""
        self._controller.session.file_dialog_pad_id = None

    def select_pad(self, pad_id: int) -> None:
        """Select a pad by ID."""
        self._controller.project.selected_pad = pad_id

    def select_bank(self, bank_id: int) -> None:
        """Select the active performance bank."""
        self._controller.project.selected_bank = bank_id

    def store_pressed_pad_state(self, pad_id: int, *, pressed: bool) -> None:
        """Record pad pressed pad state."""
        self._controller.session.pressed_pads[pad_id] = pressed


class UiContext:
    """The public interface for the UI layer."""

    def __init__(self, controller: LooperController):
        self._controller = controller
        self.state = UiState(controller)
        self.audio = AudioActions(controller)
        self.ui = UiActions(controller)
