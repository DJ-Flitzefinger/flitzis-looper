from typing import TYPE_CHECKING, Literal, TypedDict

from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.current_binding import capture_current_pad_binding
from flitzis_looper.controller.transport.bpm import BpmController
from flitzis_looper.controller.transport.global_params import GlobalParametersController
from flitzis_looper.controller.transport.loop import PadLoopController
from flitzis_looper.controller.transport.pad import PadController
from flitzis_looper.controller.transport.pitch import PitchController
from flitzis_looper.controller.transport.playback import PadPlaybackController
from flitzis_looper.controller.transport.residency import ResidencyController
from flitzis_looper.controller.transport.state import ApplyProjectState
from flitzis_looper.controller.transport.waveform import WaveformController

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine, InputRuntimePadBinding, ResidentWindowTicket


class _SourceGenerationOptions(TypedDict, total=False):
    expected_source_generation: int


class TransportController(BaseController):
    def capture_global_start_entry(
        self, sample_id: int
    ) -> tuple[InputRuntimePadBinding, float, float | None, ResidentWindowTicket | None] | None:
        """Capture one original source and its genuine existing readiness dependency."""
        dependency = self.residency._global_start_dependency(sample_id)
        timing = self.bpm.current_timing(sample_id)
        snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
        if snapshot is None:
            return None
        if dependency is None:
            start, end = self.loop.requested_region(sample_id, timing=timing)
            ticket = None
        else:
            start, end, ticket = dependency
        return snapshot.binding, start, end, ticket

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)
        self._on_global_key_lock_changed: Callable[[], None] | None = None
        self._incoming_load_request: Callable[[int], int | None] | None = None
        self._pad_edit_guard: Callable[[int], None] | None = None
        self._global_key_lock_edit_guard: Callable[[], None] | None = None
        self._on_current_pad_edit: (
            Callable[[int, int | None, Literal["key_lock", "loop", "timing"]], None] | None
        ) = None

        self.residency = ResidencyController(self)
        self.bpm = BpmController(self)
        self.global_params = GlobalParametersController(self)
        self.loop = PadLoopController(self)
        self.playback = PadPlaybackController(self)
        self.pad = PadController(self)
        self.pitch = PitchController(self)
        self.waveform = WaveformController(self)

    def apply_project_state_to_audio(self) -> None:
        ApplyProjectState(self).apply_project_state_to_audio()

    def bind_incoming_global_key_lock(self, callback: Callable[[], None]) -> None:
        """Publish the new global default to pending incoming assignments immediately."""
        self._on_global_key_lock_changed = callback

    def bind_pad_edit_guard(self, guard: Callable[[int], None]) -> None:
        """Check transaction ownership before a current-pad gesture changes intent."""
        self._pad_edit_guard = guard

    def bind_global_key_lock_edit_guard(self, guard: Callable[[], None]) -> None:
        """Check inherited pending assignments before changing the global default."""
        self._global_key_lock_edit_guard = guard

    def ensure_global_key_lock_edit_allowed(self) -> None:
        """Reject a conflicting global gesture before capturing or mutating pad state."""
        if self._global_key_lock_edit_guard is not None:
            self._global_key_lock_edit_guard()

    def bind_current_pad_edit(
        self,
        callback: Callable[[int, int | None, Literal["key_lock", "loop", "timing"]], None],
        *,
        incoming_request: Callable[[int], int | None],
    ) -> None:
        """Observe generic gestures and identify ordinary pending source ownership."""
        self._on_current_pad_edit = callback
        self._incoming_load_request = incoming_request

    def capture_current_pad_edit_generation(self, sample_id: int) -> int | None:
        """Capture ownership before mutating a current-source control domain."""
        self.ensure_current_source_assignment_ready(sample_id)
        try:
            binding = self._audio.current_input_runtime_pad_binding(sample_id)
            value = (
                binding.metadata().get("source_generation")
                if binding is not None and binding.is_current() is True
                else None
            )
            if isinstance(value, int) and not isinstance(value, bool):
                return value
        except RuntimeError, ValueError:
            # Unavailable ownership cannot transfer a predecessor gesture to new content.
            pass
        return None

    def ensure_pad_edit_allowed(self, sample_id: int) -> None:
        """Check a reserved arrangement target before changing its musical metadata."""
        if self._pad_edit_guard is not None:
            self._pad_edit_guard(sample_id)

    def ensure_current_source_assignment_ready(self, sample_id: int) -> None:
        """Reject reads derived from the predecessor until the own receipt is projected."""
        self.ensure_pad_edit_allowed(sample_id)
        if (
            self._incoming_load_request is not None
            and self._incoming_load_request(sample_id) is not None
        ):
            message = (
                "Ordinary incoming assignment receipt is pending; current-source gesture rejected"
            )
            raise ValueError(message)

    @staticmethod
    def source_generation_options(generation: int | None) -> _SourceGenerationOptions:
        """Keep unfenced saved/maintenance calls distinct from captured current-source gestures."""
        return {} if generation is None else {"expected_source_generation": generation}

    def record_current_pad_edit(
        self,
        sample_id: int,
        generation: int | None,
        domain: Literal["key_lock", "loop", "timing"],
    ) -> None:
        if self._on_current_pad_edit is not None:
            self._on_current_pad_edit(sample_id, generation, domain)
