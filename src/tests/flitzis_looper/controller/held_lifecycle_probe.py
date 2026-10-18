"""Real App component owners for a device-free Cold/Hold lifetime fixture.

The enclosing Rust child owns the engine, physical files and callback. This
envelope uses the real public Loader and inherited App event/shutdown paths;
it never supplies a source publication, Native acknowledgement or audio effect.
"""

import hashlib
import json
from collections import deque
from contextlib import ExitStack, chdir
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.app import AppController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.material_migration import MaterialMigrationController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.performance_confirmation import PerformanceConfirmationController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping import InputMappingController
from flitzis_looper.models import ProjectState, SessionState

if TYPE_CHECKING:
    from flitzis_looper.controller.transport.waveform import _HoldOwner
    from flitzis_looper_audio import AudioEngine, HoldActionTicket


@dataclass(frozen=True, slots=True)
class LoadedSource:
    """Detached evidence from one real Loader success and its Native owner."""

    request: int
    content_id: str
    reference: str
    sha256: str
    native: tuple[int, str, int, int]
    lifetime: int


class HeldLifecycleProbe(AppController):
    """Inject a headless engine while retaining actual App component behavior."""

    def __init__(self, audio: AudioEngine, root: str, *, restore: bool = False) -> None:
        self._storage = ExitStack()
        self._closed = False
        self.app = self
        self.hold: HoldActionTicket | None = None
        self.owner: _HoldOwner | None = None
        self.hold_slot: int | None = None
        self.source_history: dict[int, list[LoadedSource]] = {}
        self.pending_requests: dict[int, int] = {}
        self.input_paths: dict[int, Path] = {}
        self.saved_bytes: bytes | None = None
        with ExitStack() as setup:
            directory = Path(root).resolve(strict=True)
            setup.enter_context(chdir(directory))
            samples = directory / "samples"
            samples.mkdir(exist_ok=True)
            config = samples / "held-lifecycle.config.json"
            if restore:
                self._persistence = ProjectPersistence.from_config_path(config)
                assert self._persistence.load_error is None, self._persistence.load_error
            else:
                self._persistence = ProjectPersistence(
                    ProjectState(
                        multi_loop=True,
                        bpm_lock=False,
                        key_lock=False,
                        trigger_quantization_enabled=False,
                        input_mapping_enabled=False,
                        selected_pad=0,
                    )
                )
            self._persistence.config_path = config
            self._project = self._persistence.project
            self._session = SessionState()
            self._audio = audio
            self._close_requested = False
            self._shutdown_preparing = False
            self._shutdown_running = False
            self._shutdown_complete = False
            self._gui_native_selection = None
            self._pair_waveform_views = []
            self._pending_content_feedback = deque()
            self._audio.set_resident_pcm_budget_mib(self._project.resident_pcm_budget_mib)
            self._assets = ProjectAssetLifecycle(self._project, audio)
            self._assets.sync_assignments()
            self._persistence.bind_audio(audio, self._report_timing_save_error)
            self._wire_components()
            # The real InputMapping component reads only this owned child's
            # relative config/input files. No MIDI ports are opened here.
            self._audio.set_input_mapping_enabled(False)
            self._storage = setup.pop_all()

    def _wire_components(self) -> None:
        self.settings = SettingsController(
            self.project, self.session, self._audio, self.persistence.mark_dirty
        )
        self.transport = TransportController(
            self.project, self.session, self._audio, self.persistence.mark_dirty
        )
        self.accepted_timing = AcceptedTimingController(self.transport)
        self.transport.bpm.set_accepted_refresh_callback(self.accepted_timing.refresh_current)
        self.stems = StemController(
            self.project,
            self.session,
            self._audio,
            on_project_changed=self.persistence.mark_dirty,
            asset_lifecycle=self._assets,
        )
        self.stems.fence_pair_metadata(self.persistence.invalid_stem_pair_ids)
        self.stems.bind_disk_selection_commit(
            self.persistence.commit_stem_selection, self.persistence.flush
        )
        self.arrangement = PadArrangementController(
            self.persistence,
            self.session,
            self._audio,
            self._assets,
            stem_task_runner=self.stems._stem_task_runner,
            on_copy_committed=self._on_copy_committed,
            on_target_removed=self._on_copy_target_removed,
            can_admit=lambda: not self.pairs.active,
        )
        self.transport.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.stems.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.transport.bind_global_key_lock_edit_guard(self._ensure_global_key_lock_edit_allowed)
        self.settings.bind_residency_policy(self.stems.refresh_residency_policy)
        self.loader = LoaderController(
            self.project,
            self.session,
            self._audio,
            on_pad_bpm_changed=self.transport.bpm.on_pad_bpm_changed,
            on_project_changed=self.persistence.mark_dirty,
            on_stem_generation_started=self.stems._handle_stem_generation_started,
            on_stem_generation_progress=self.stems._handle_stem_generation_progress,
            on_stem_generation_success=self.stems._handle_stem_generation_success,
            on_stem_generation_error=self.stems._handle_stem_generation_error,
            on_stems_deleted=self.stems.delete_stems,
        )
        self.loader.bind_asset_lifecycle(self._assets)
        self.transport.bind_incoming_global_key_lock(self.loader._refresh_incoming_global_intent)
        self.transport.bind_current_pad_edit(
            self.loader.record_current_pad_edit,
            incoming_request=self.loader.incoming_load_request_id,
        )
        self.loader.set_stems_invalidated_callback(self.stems.invalidate_stem_cache)
        self.loader.set_restored_sample_loaded_callback(self._on_restored_sample_loaded)
        self.loader.set_new_sample_loaded_callback(
            self._on_new_sample_loaded,
            key_lock_callback=self._refresh_loaded_key_lock,
            incoming_callback=self._on_incoming_sample_loaded,
        )
        self.loader.set_sample_unloaded_callback(self._on_sample_unloaded)
        self.loader.set_accepted_timing_refresh_callback(self._refresh_restored_accepted_timing)
        self.material_migration = MaterialMigrationController(
            self.persistence,
            self.session,
            self._audio,
            self._assets,
            self.loader,
            stem_task_runner=self.stems._stem_task_runner,
        )
        self.metering = MeteringController(self.project, self.session, self._audio)
        self.performance_confirmation = PerformanceConfirmationController(self)
        self.input_mapping = InputMappingController(self, self.persistence.mark_dirty)
        self._prepare_arrangement_recovery()
        self._controllers = {
            self.transport,
            self.loader,
            self.metering,
            self.stems,
            self.input_mapping,
        }
        self.transport.playback.set_global_playback_feedback_poll(self._poll_audio_messages)

    def load(self, slot: int, path: str) -> int:
        """Admit one ordinary import through the real public Loader."""
        self.input_paths[slot] = Path(path).resolve(strict=True)
        self.loader.load_sample_async(slot, path)
        assert slot not in self.session.sample_load_errors, self.session.sample_load_errors
        request = self.loader.incoming_load_request_id(slot)
        assert type(request) is int
        assert request > 0
        self.pending_requests[slot] = request
        assert slot in self.session.loading_sample_ids
        return request

    def restore_sources(self) -> None:
        """Start saved restoration only on the enclosing fixture's explicit request."""
        assert not self.source_history
        self.loader.restore_samples_from_project_state()
        assert not self.session.sample_load_errors, self.session.sample_load_errors

    def poll_load(self, slot: int) -> bool:
        """Project only the original Native success into the real Project model."""
        self.loader.poll_loader_events()
        assert slot not in self.session.sample_load_errors, self.session.sample_load_errors
        if slot in self.session.loading_sample_ids:
            return False
        reference = self.project.sample_paths[slot]
        content = self.project.pad_content[slot]
        if reference is None or content is None:
            return False
        native = self._audio.waveform_source_identity(slot)
        assert native is not None
        binding = self._audio.current_input_runtime_pad_binding(slot)
        assert binding is not None
        assert binding.is_current()
        metadata = binding.metadata()
        assert metadata["source_generation"] == native[0]
        request = self.pending_requests.get(slot, native[0])
        assert request == native[0]
        sha256 = hashlib.sha256(Path(reference).read_bytes()).hexdigest()
        assert self._audio.loaded_source_digest(slot) == sha256
        if slot in self.input_paths:
            assert hashlib.sha256(self.input_paths[slot].read_bytes()).hexdigest() == sha256
        history = self.source_history.setdefault(slot, [])
        if not history or history[-1].request != request:
            lifetime = metadata["content_lifetime"]
            assert type(lifetime) is int
            source = LoadedSource(
                request,
                content.instance_id,
                reference,
                sha256,
                native,
                lifetime,
            )
            if history:
                previous = history[-1]
                assert source.content_id != previous.content_id
                assert source.native[0] != previous.native[0]
                assert source.lifetime != previous.lifetime
            history.append(source)
            self.assert_stopped(slot)
        return True

    def poll_feedback(self) -> None:
        """Deliver the callback's actual packets through inherited App handling."""
        self._poll_audio_messages()

    def start(self, slot: int = 0) -> None:
        """Request a real public ordinary start; Rust owns readiness and effect."""
        self.transport.playback.trigger_pad_keep_others(slot)

    def press(self, slot: int = 0) -> HoldActionTicket:
        """Retain one real physical-press owner before its pause acknowledgement."""
        waveform = self.transport.waveform
        assert not waveform.has_pending_holds
        waveform.open_editor(slot)
        waveform.press_pause_hold(slot)
        assert waveform._physical_hold_press is not None, waveform.hold_status()
        assert len(waveform._hold_owners) == 1, waveform.hold_status()
        self.owner = next(iter(waveform._hold_owners.values()))
        self.hold = self.owner.ticket
        self.hold_slot = slot
        assert self.owner.physical_press_id == waveform._physical_hold_press
        return self.hold

    def observe_owned(self) -> None:
        """Observe the real Native pause acknowledgement without inventing it."""
        waveform = self.transport.waveform
        waveform.poll_holds()
        assert waveform.hold_status() == ("paused_owned", None), waveform.hold_status()
        assert self.owner is not None
        assert self.hold is not None
        assert waveform._physical_hold_press == self.owner.physical_press_id
        assert waveform._hold_owners[self.owner.physical_press_id] is self.owner
        assert self.session.waveform_pause_hold_pad_id == self.hold_slot

    def release(self) -> None:
        """Request release of the same original owner through its real controller."""
        assert self.hold is not None
        assert self.owner is not None
        waveform = self.transport.waveform
        waveform.request_hold_release("button_up")
        waveform.poll_holds()
        assert waveform._physical_hold_press is None
        assert self.owner.ticket is self.hold
        assert self.owner.release_requested

    def _hold_image(self) -> tuple[object, ...]:
        waveform = self.transport.waveform
        return (
            waveform._physical_hold_press,
            self.session.waveform_pause_hold_pad_id,
            tuple(
                (
                    press,
                    id(owner),
                    owner.release_requested,
                    owner.release_admitted,
                    owner.status,
                    owner.error,
                    owner.ticket.publication_status(),
                )
                for press, owner in waveform._hold_owners.items()
            ),
        )

    def reject_full_unload(self, slot: int = 0) -> None:
        """A real queue-full unload must leave source, model and physical Hold intact."""
        project = self.project.model_dump(mode="json")
        session = self.session.model_dump(mode="json")
        config = self.persistence.config_path
        persisted = config.read_bytes() if config.exists() else None
        persistence = (self.persistence.revision, self.persistence._dirty)
        source = self._audio.waveform_source_identity(slot)
        digest = self._audio.loaded_source_digest(slot)
        binding = self._audio.current_input_runtime_pad_binding(slot)
        assert binding is not None
        assert binding.is_current()
        hold = self._hold_image()
        assignments = self._assets._assignments.copy()
        with pytest.raises(RuntimeError, match="Failed to send UnloadSample - buffer may be full"):
            self.loader.unload_sample(slot)
        assert self.project.model_dump(mode="json") == project
        assert self.session.model_dump(mode="json") == session
        assert (config.read_bytes() if config.exists() else None) == persisted
        assert (self.persistence.revision, self.persistence._dirty) == persistence
        assert self._audio.waveform_source_identity(slot) == source
        assert self._audio.loaded_source_digest(slot) == digest
        assert binding.is_current()
        assert self._hold_image() == hold
        assert self._assets._assignments == assignments
        assert all(not lease.released for _, lease in assignments.values())

    def unload(self, slot: int = 0) -> None:
        """Use the actual Loader/App unload callbacks and asset retirement."""
        self.loader.unload_sample(slot)
        assert self.project.sample_paths[slot] is None
        assert self.project.pad_content[slot] is None
        self.assert_stopped(slot)

    def assert_stopped(self, slot: int = 0) -> None:
        """Check the fresh/retired source has no stale playback/pause projection."""
        assert slot not in self.session.active_sample_ids
        assert slot not in self.session.paused_sample_ids
        assert not self.session.pressed_pads[slot]
        assert self.session.pad_playhead_s[slot] is None
        assert self.session.pad_peak[slot] == 0.0

    def poll_terminal(self) -> None:
        """Remove an owner only after the real Native terminal acknowledgement."""
        waveform = self.transport.waveform
        waveform.poll_holds()
        assert not waveform.has_pending_holds, waveform.hold_status()
        assert waveform._physical_hold_press is None
        assert waveform.hold_status() == (None, None)
        assert self.session.waveform_pause_hold_pad_id is None

    def save(self) -> None:
        """Persist only the real Project image and retain its actual saved bytes."""
        self.persistence.flush()
        self.saved_bytes = self.persistence.config_path.read_bytes()
        saved = json.loads(self.saved_bytes)
        assert "waveform_pause_hold_pad_id" not in saved
        assert "paused_sample_ids" not in saved
        assert "_physical_hold_press" not in saved
        assert ProjectState.model_validate(saved) == self.project

    def prepare_close(self) -> bool:
        """Run actual App Close preparation with the original Hold owner alive."""
        return self.prepare_shutdown()

    def finish_close(self) -> None:
        """Run actual App shutdown; callback destruction remains Rust's obligation."""
        assignments = tuple(self._assets._assignments.values())
        self.shut_down()
        assert self._shutdown_complete
        assert not self._shutdown_running
        assert not self._shutdown_preparing
        assert not self._assets._assignments
        assert all(lease.released for _, lease in assignments)
        self.poll_terminal()
        saved = json.loads(self.persistence.config_path.read_bytes())
        assert "waveform_pause_hold_pad_id" not in saved
        assert "paused_sample_ids" not in saved
        assert "_physical_hold_press" not in saved

    def close(self) -> None:
        """Release owned test assignment leases and cwd without deleting evidence."""
        if self._closed:
            return
        try:
            self._assets.release_saved_assignments()
        finally:
            self._storage.close()
            self._closed = True
