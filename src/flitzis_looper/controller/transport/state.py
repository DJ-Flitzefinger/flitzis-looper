from typing import TYPE_CHECKING

from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


class ApplyProjectState:
    """Apply project state to audio engine and session state."""

    def __init__(
        self, transport: TransportController, *, restored_sample_ids: frozenset[int] | None = None
    ) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._bpm = transport.bpm
        self._global_modes = transport.global_params
        self._audio = transport._audio
        self._restored_sample_ids = restored_sample_ids

    def _has_assignment(self, sample_id: int) -> bool:
        return self._project.sample_paths[sample_id] is not None and (
            self._restored_sample_ids is None or sample_id in self._restored_sample_ids
        )

    def apply_project_state_to_audio(self) -> None:
        defaults = ProjectState()

        self._apply_global_audio_settings(defaults)
        self._apply_per_pad_mixing(defaults)
        self._apply_pad_loop_regions(defaults)
        self._apply_pad_bpm_settings()
        self._apply_bpm_lock_settings()

    def _apply_global_audio_settings(self, defaults: ProjectState) -> None:
        if self._project.volume != defaults.volume:
            self._audio.set_volume(self._project.volume)

        if self._project.speed != defaults.speed:
            self._audio.set_speed(self._project.speed)

        self._apply_key_lock_settings(defaults)

        if self._project.trigger_quantization_enabled != defaults.trigger_quantization_enabled:
            mode = (
                self._project.trigger_quantization_step
                if self._project.trigger_quantization_enabled
                else "immediate"
            )
            self._audio.set_trigger_quantization(mode)

    def _apply_key_lock_settings(self, defaults: ProjectState) -> None:
        for sample_id in range(len(self._project.pad_key_lock)):
            if not self._has_assignment(sample_id):
                continue

            enabled = self._project.resolve_pad_key_lock(sample_id)
            if enabled:
                self._audio.set_pad_key_lock(sample_id, enabled)

    def _apply_per_pad_mixing(self, defaults: ProjectState) -> None:
        for sample_id, gain_db in enumerate(self._project.pad_gain_db):
            if not self._has_assignment(sample_id):
                continue

            if gain_db != defaults.pad_gain_db[sample_id]:
                self._audio.set_pad_gain(sample_id, gain_db)

        for sample_id, low_db in enumerate(self._project.pad_eq_low_db):
            if not self._has_assignment(sample_id):
                continue

            mid_db = self._project.pad_eq_mid_db[sample_id]
            high_db = self._project.pad_eq_high_db[sample_id]

            if (
                low_db == defaults.pad_eq_low_db[sample_id]
                and mid_db == defaults.pad_eq_mid_db[sample_id]
                and high_db == defaults.pad_eq_high_db[sample_id]
            ):
                continue

            self._audio.set_pad_eq(sample_id, low_db, mid_db, high_db)

    def _apply_pad_loop_regions(self, defaults: ProjectState) -> None:
        for sample_id in range(len(self._project.sample_paths)):
            if not self._has_assignment(sample_id):
                continue

            start_s = self._project.pad_loop_start_s[sample_id]
            end_s = self._project.pad_loop_end_s[sample_id]
            if (
                start_s == defaults.pad_loop_start_s[sample_id]
                and end_s == defaults.pad_loop_end_s[sample_id]
                and not self._project.pad_loop_auto[sample_id]
            ):
                continue

            timing = self._bpm.current_timing(sample_id)
            if (
                timing is None
                and self._project.manual_bpm[sample_id] is None
                and self._bpm.awaiting_automatic_timing(sample_id)
            ):
                continue
            self._transport.loop._apply_effective_pad_loop_region_to_audio(sample_id, timing=timing)

    def _apply_pad_bpm_settings(self) -> None:
        for sample_id in range(len(self._project.sample_paths)):
            if not self._has_assignment(sample_id):
                continue

            if (
                self._project.manual_bpm[sample_id] is None
                and self._project.sample_analysis[sample_id] is None
                and self._project.pad_grid_offset_samples[sample_id] == 0
                and self._project.pad_grid_anchor_s[sample_id] is None
                and self._project.pad_timing_intent[sample_id] not in {"manual", "tap"}
            ):
                continue
            self._bpm.on_pad_bpm_changed(sample_id)

    def _apply_bpm_lock_settings(self) -> None:
        self._global_modes._begin_bpm_lock_application(
            capture_anchor=self._project.resolve_bpm_lock()
        )
        self._global_modes.poll_bpm_lock_application()
