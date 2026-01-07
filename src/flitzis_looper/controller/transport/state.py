from typing import TYPE_CHECKING

from flitzis_looper.controller.validation import normalize_bpm
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from flitzis_looper.controller.transport import TransportController


class ApplyProjectState:
    """Apply project state to audio engine and session state."""

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._bpm = transport.bpm
        self._global_modes = transport.global_params
        self._audio = transport._audio

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

        if self._project.key_lock != defaults.key_lock:
            self._audio.set_key_lock(enabled=self._project.key_lock)

        if self._project.bpm_lock != defaults.bpm_lock:
            self._audio.set_bpm_lock(enabled=self._project.bpm_lock)

    def _apply_per_pad_mixing(self, defaults: ProjectState) -> None:
        for sample_id, gain in enumerate(self._project.pad_gain):
            if gain != defaults.pad_gain[sample_id]:
                self._audio.set_pad_gain(sample_id, gain)

        for sample_id, low_db in enumerate(self._project.pad_eq_low_db):
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
            if self._project.sample_paths[sample_id] is None:
                continue

            start_s = self._project.pad_loop_start_s[sample_id]
            end_s = self._project.pad_loop_end_s[sample_id]
            if (
                start_s == defaults.pad_loop_start_s[sample_id]
                and end_s == defaults.pad_loop_end_s[sample_id]
                and not self._project.pad_loop_auto[sample_id]
            ):
                continue

            self._transport.loop._apply_effective_pad_loop_region_to_audio(sample_id)

    def _apply_pad_bpm_settings(self) -> None:
        for sample_id in range(len(self._project.sample_paths)):
            if (
                self._project.manual_bpm[sample_id] is None
                and self._project.sample_analysis[sample_id] is None
            ):
                continue
            self._bpm.on_pad_bpm_changed(sample_id)

    def _apply_bpm_lock_settings(self) -> None:
        if self._project.bpm_lock:
            anchor_pad_id = self._project.selected_pad
            anchor_bpm = normalize_bpm(self._bpm.effective_bpm(anchor_pad_id))
            self._session.bpm_lock_anchor_pad_id = anchor_pad_id
            self._session.bpm_lock_anchor_bpm = anchor_bpm
        else:
            self._session.bpm_lock_anchor_pad_id = None
            self._session.bpm_lock_anchor_bpm = None

        self._bpm.recompute_master_bpm()
