import json
from collections import deque
from contextlib import suppress
from pathlib import Path
from time import monotonic
from typing import TYPE_CHECKING

from flitzis_looper.control_diagnostics import ConsoleDiagnostics, poll_control_diagnostics
from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.bank_arrangement import BankArrangementController
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.material_migration import MaterialMigrationController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.pad_selection import record_pad_selection
from flitzis_looper.controller.pair_arrangement import PairArrangementController
from flitzis_looper.controller.performance_confirmation import PerformanceConfirmationController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.recording import RecordingController
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.controller.stem_batch import StemBatchController
from flitzis_looper.controller.stem_delete import StemDeleteController
from flitzis_looper.controller.stems import StemController, StemTaskRunner
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping import InputMappingController
from flitzis_looper.models import ProjectState, SessionState, validate_sample_id
from flitzis_looper.recording_paths import hold_live_recording_spool
from flitzis_looper_audio import AudioEngine, AudioMessage

if TYPE_CHECKING:
    from flitzis_looper.controller.base import BaseController
    from flitzis_looper.controller.stem_generation import StemGenerationBackend
    from flitzis_looper.copy_snapshot import CopySnapshot
    from flitzis_looper.ui.context import WaveformEditorActions
    from flitzis_looper_audio import PadCopyTicket


def _copied_components_available(sample_id: int, ticket: PadCopyTicket) -> bool:
    try:
        metadata = json.loads(ticket.metadata_json())
    except (AttributeError, TypeError, ValueError) as error:
        message = "Complete Copy target acknowledgement metadata is unavailable"
        raise RuntimeError(message) from error
    if not isinstance(metadata, dict):
        message = "Complete Copy target acknowledgement metadata is invalid"
        raise TypeError(message)
    target = metadata.get("target_id")
    generation = metadata.get("source_generation")
    lifetime = metadata.get("content_lifetime")
    components = metadata.get("components_available")
    target_matches = type(target) is int and target == sample_id
    counters_are_current = all(
        type(counter) is int and counter > 0 for counter in (generation, lifetime)
    )
    if (
        not target_matches
        or not counters_are_current
        or metadata.get("status") != "accepted"
        or metadata.get("control_mirrored") is not True
        or not isinstance(components, bool)
    ):
        message = "Complete Copy target has no matching acknowledged Native image"
        raise RuntimeError(message)
    return components


class AppController:
    def __init__(
        self,
        stem_backend: StemGenerationBackend | None = None,
        stem_task_runner: StemTaskRunner | None = None,
        *,
        project_config_path: Path | None = None,
    ) -> None:
        self._persistence = (
            ProjectPersistence.from_config_path()
            if project_config_path is None
            else ProjectPersistence.from_config_path(project_config_path)
        )
        if project_config_path is not None:
            self._persistence.config_path = project_config_path
        self._project = self._persistence.project
        self._session = SessionState()
        self._close_requested = False
        self._shutdown_preparing = False
        self._shutdown_running = False
        self._shutdown_complete = False
        self._gui_native_selection: tuple[int, int] | None = None
        self._pair_waveform_views: list[WaveformEditorActions] = []
        self._pending_content_feedback: deque[AudioMessage] = deque()

        self._audio = AudioEngine()
        self._audio.set_resident_pcm_budget_mib(self._project.resident_pcm_budget_mib)
        if not self._audio.set_key_lock_start_behavior(self._project.key_lock_start_behavior):
            message = "Key Lock start behavior could not be restored before audio startup"
            raise RuntimeError(message)
        self._audio.run()
        self._assets = ProjectAssetLifecycle(self._project, self._audio)
        self._assets.sync_assignments()
        self._persistence.bind_audio(self._audio, self._report_timing_save_error)

        self.settings = SettingsController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
        )
        self.transport = TransportController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
        )
        self._init_recording_control()
        self.accepted_timing = AcceptedTimingController(self.transport)
        self.transport.bpm.set_accepted_refresh_callback(self.accepted_timing.refresh_current)
        self._init_stem_control(stem_backend, stem_task_runner)
        self.arrangement = PadArrangementController(
            self._persistence,
            self._session,
            self._audio,
            self._assets,
            stem_task_runner=self.stems._stem_task_runner,
            on_copy_committed=self._on_copy_committed,
            on_target_removed=self._on_copy_target_removed,
            can_admit=lambda: not (self.pairs.active or self.banks.active),
        )
        self.transport.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.stems.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.transport.bind_global_key_lock_edit_guard(self._ensure_global_key_lock_edit_allowed)
        self.settings.bind_residency_policy(self.stems.refresh_residency_policy)
        self.loader = LoaderController(
            self._project,
            self._session,
            self._audio,
            on_pad_bpm_changed=self.transport.bpm.on_pad_bpm_changed,
            on_project_changed=self._persistence.mark_dirty,
            on_stem_generation_started=self.stems._handle_stem_generation_started,
            on_stem_generation_progress=self.stems._handle_stem_generation_progress,
            on_stem_generation_success=self.stems._handle_stem_generation_success,
            on_stem_generation_error=self.stems._handle_stem_generation_error,
            on_stems_deleted=self.stems.delete_stems,
        )
        self._bind_loader_assets()
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
            self._persistence,
            self._session,
            self._audio,
            self._assets,
            self.loader,
            stem_task_runner=self.stems._stem_task_runner,
        )
        self.metering = MeteringController(self._project, self._session, self._audio)
        self.performance_confirmation = PerformanceConfirmationController(self)
        self.input_mapping = InputMappingController(
            self,
            on_project_changed=self._persistence.mark_dirty,
        )
        self._prepare_arrangement_recovery()

        self._controllers: set[BaseController] = {
            self.transport,
            self.loader,
            self.metering,
            self.stems,
            self.input_mapping,
        }
        self.transport.playback.set_global_playback_feedback_poll(self._poll_audio_messages)

        self._restore_initial_runtime_state()
        self._discover_recordings()

    def _bind_loader_assets(self) -> None:
        self.loader.bind_asset_lifecycle(self._assets)
        self.loader.source_beatmaps.bind(
            self._persistence.commit_source_beatmap_selection,
            blocked=lambda: (
                self._persistence.stem_selection_write_blocked()
                or self._persistence.load_error is not None
            ),
            edit_guard=self.ensure_not_copy_target,
        )

    def _init_recording_control(self) -> None:
        """Bind one original Engine and project namespace before restore and UI work."""
        self._recording_shutdown_deadline: float | None = None
        self._record_root = Path.cwd().absolute() / "record"
        self.recording = RecordingController(
            self._project,
            self._audio,
            self._record_root,
            live_spool_witness=hold_live_recording_spool,
            stream_failed=self._audio.audio_stream_failed,
            quiesce_failed_stream=self._audio.quiesce_failed_audio_stream,
        )

    def _discover_recordings(self) -> None:
        """Observe an existing recording namespace without creating one at startup."""
        try:
            self._record_root.lstat()
        except FileNotFoundError:
            return
        self.recording.discover_recovery()

    def _init_stem_control(
        self, stem_backend: StemGenerationBackend | None, stem_task_runner: StemTaskRunner | None
    ) -> None:
        self.stems = StemController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
            stem_backend=stem_backend,
            stem_task_runner=stem_task_runner,
            asset_lifecycle=self._assets,
        )
        self.stems.fence_pair_metadata(self._persistence.invalid_stem_pair_ids)
        self.stem_batch = StemBatchController(self.stems)
        self.stems.bind_delete_logical_interests(self.stem_batch.capture_unadmitted_interests)
        self.stem_delete = StemDeleteController(
            self.stems, withdraw_unadmitted=self.stem_batch.withdraw_unadmitted_interests
        )
        self.stems.bind_disk_selection_commit(
            self._persistence.commit_stem_selection,
            self._persistence.flush,
            writer_blocked=self._persistence.stem_selection_write_blocked,
        )

    def _restore_initial_runtime_state(self) -> None:
        """Restore source/control state in order, then initialize one configured GUI selection."""
        self.diagnostics = ConsoleDiagnostics(
            lambda: (
                path
                for path in (
                    *self._project.sample_paths,
                    *self._session.pending_sample_paths.values(),
                )
                if path is not None
            )
        )
        self.transport.pitch.bind_diagnostics(self.diagnostics)
        self.transport.residency._bind_diagnostics(self.diagnostics)
        self.transport.waveform.bind_diagnostics(self.diagnostics)
        self.loader._bind_diagnostics(self.diagnostics)
        self.stems._bind_diagnostics(self.diagnostics)
        restored_paths = self.loader._validate_restored_sample_paths()
        # Initial timing/loop edits must finish before Cold captures their epoch
        # and residency guards. Mixing also travels with that original source ACK.
        self.transport.apply_project_state_to_audio(restored_sample_ids=frozenset(restored_paths))
        self.loader.restore_samples_from_project_state(validated_paths=restored_paths)
        self.stems.restore_stem_cache_from_project_state()
        self.material_migration.schedule_after_restore()
        self.input_mapping.apply_project_state_to_input_runtime()
        self._initialize_native_gui_selection()

    def _prepare_arrangement_recovery(self) -> None:
        """Acquire complete arrangement ownership before ordinary Loader restoration."""
        self.banks = BankArrangementController(self)
        self.arrangement.bind_startup(self)
        self.pairs = PairArrangementController(self, can_admit=lambda: not self.banks.active)
        self.banks.bind_startup()
        self._arrangement_recovery_pending = (
            self.arrangement.active or self.pairs.active or self.banks.active
        )
        self.material_migration.bind_arrangement_recovery_guard(
            lambda: not (self.arrangement.active or self.pairs.active or self.banks.active)
        )
        self.loader._bind_restored_assignment_guard(self._restored_assignment_held)
        self.stems.bind_restored_assignment_guard(self._restored_assignment_held)
        self._sync_arrangement_observers()

    def _restored_assignment_held(self, sample_id: int) -> bool:
        return sample_id in (
            self.arrangement.fenced_sample_ids
            | self.pairs.fenced_sample_ids
            | self.banks.fenced_sample_ids
        )

    def _finish_arrangement_recovery(self) -> None:
        """Refresh cleanup admission once all original startup owners actually settle."""
        if not self._arrangement_recovery_pending or (
            self.arrangement.active or self.pairs.active or self.banks.active
        ):
            return
        self._arrangement_recovery_pending = False
        try:
            self.material_migration.refresh_arrangement_recovery()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.material_migration.error = (
                f"Arrangement recovery cleanup remains deferred: {error}"
            )[:2048]

    def select_pad(
        self,
        sample_id: int,
        *,
        native_selection_already_applied: bool = False,
        expected_native_selection_revision: int | None = None,
    ) -> None:
        """Order GUI selection natively or project only an unchanged original MIDI receipt."""
        validate_sample_id(sample_id)
        if native_selection_already_applied:
            if not self._native_selection_receipt_current(
                sample_id, expected_native_selection_revision
            ):
                return
        else:
            setter = getattr(self._audio, "select_input_pad", None)
            if callable(setter):
                try:
                    revision = setter(sample_id)
                    self._validate_native_selection_revision(revision)
                except (RuntimeError, ValueError, TypeError) as error:
                    self._session.input_mapping_error = str(error)
                    return
                self._gui_native_selection = (revision, sample_id)
            else:
                self._session.input_mapping_error = (
                    "Pitch choices require the current audio runtime."
                )
        record_pad_selection(self._project, self._session, sample_id)
        self._persistence.mark_dirty()

    @staticmethod
    def _validate_native_selection_revision(revision: object) -> None:
        if type(revision) is not int or not 0 <= revision < 1 << 64:
            message = "Native selection acknowledgement is unavailable"
            raise TypeError(message)

    def _native_selection_receipt_current(self, sample_id: int, revision: int | None) -> bool:
        if type(revision) is not int or not 0 <= revision < 1 << 64:
            return False
        current = self._current_native_selection()
        if current is None or current[0] != revision:
            return False
        if current[1] is None:
            return (
                self._project.sample_paths[sample_id] is None
                and self._project.pad_content[sample_id] is None
            )
        return type(current[1]) is int and current[1] == sample_id

    def _current_native_selection(self) -> tuple[int, int | None] | None:
        provider = getattr(self._audio, "input_selected_projection", None)
        if not callable(provider):
            return None
        try:
            current: object = provider()
        except (RuntimeError, ValueError, TypeError) as error:
            self._session.input_mapping_error = str(error)
            return None
        if not isinstance(current, tuple) or len(current) != 2:
            return None
        revision: object = current[0]
        slot: object = current[1]
        if type(revision) is not int or not 0 <= revision < 1 << 64:
            return None
        if slot is not None and (
            type(slot) is not int or not 0 <= slot < len(self._project.pad_content)
        ):
            return None
        return revision, slot

    def _initialize_native_gui_selection(self) -> None:
        current = self._current_native_selection()
        if current is None or current[0] != 0:
            return
        sample_id = self._project.selected_pad
        self._gui_native_selection = (0, sample_id)
        self._refresh_native_gui_selection(sample_id)

    def _refresh_native_gui_selection(self, sample_id: int) -> None:
        """Resolve one startup/empty GUI choice after its own loaded runtime snapshot is ready."""
        previous = self._gui_native_selection
        if previous is None or previous[1] != sample_id or self._project.selected_pad != sample_id:
            return
        current = self._current_native_selection()
        if current is None or current[0] != previous[0] or current[1] == sample_id:
            return
        if not self.input_mapping._refresh_rust_runtime_state():
            return
        snapshot = self.input_mapping._last_input_runtime_state_signature
        if snapshot is None or not snapshot[2][sample_id].ready:
            return
        self.select_pad(sample_id)

    def prepare_shutdown(
        self, *, holds_polled: bool = False, deletion_polled: bool = False
    ) -> bool:
        """Settle recording and arrangement while their callback ownership remains alive."""
        if self._shutdown_complete:
            return True
        if self._shutdown_preparing:
            message = "Application shutdown preparation is already running"
            raise RuntimeError(message)
        self._shutdown_preparing = True
        try:
            if self._recording_shutdown_deadline is None:
                self._recording_shutdown_deadline = monotonic() + 30.0
            recording_ready = self.recording.prepare_shutdown(
                deadline=self._recording_shutdown_deadline
            )
            self.transport.waveform.request_hold_release("close")
            if not holds_polled:
                self.transport.waveform.poll_holds()
            if not self._close_requested:
                self._audio.set_input_mapping_enabled(False)
                self._close_requested = True
                self.stem_batch.cancel()
            if not deletion_polled:
                self.stem_delete.poll()
            other_owners_ready = (
                self.banks.prepare_shutdown()
                and self.pairs.prepare_shutdown()
                and self.arrangement.prepare_shutdown()
                and not self.transport.waveform.has_pending_holds
                and not self.stem_delete.active
            )
            return other_owners_ready and recording_ready
        finally:
            self._shutdown_preparing = False

    def shut_down(self) -> None:
        """Release dependent owners only after actual arrangement settlement permits exit."""
        if self._shutdown_complete:
            return
        if self._shutdown_running:
            message = "Application shutdown is already running"
            raise RuntimeError(message)
        self._shutdown_running = True
        try:
            if not self.prepare_shutdown():
                message = "Application close is waiting for recording or arrangement settlement"
                raise RuntimeError(message)
            self.recording.shut_down()
            self.material_migration.shut_down()
            self.transport.pitch.shut_down()
            self.transport.residency.shut_down()
            self.accepted_timing.shut_down()
            self.loader.shut_down()
            self.stems.shut_down()
            with suppress(OSError):
                self._persistence.flush()

            self._audio.stop_all()
            self._assets.release_saved_assignments()
            self._audio.shut_down()
            self._shutdown_complete = True
        finally:
            self._shutdown_running = False

    def _report_timing_save_error(self, sample_id: int, message: str) -> None:
        self._session.sample_analysis_errors[sample_id] = message

    def on_frame_render(self) -> None:
        self.pairs.poll()
        self._sync_arrangement_observers()
        self.banks.poll()
        self._sync_arrangement_observers()
        self.arrangement.poll()
        self._sync_arrangement_observers()
        for controller in self._controllers:
            controller.on_frame_render()
        # The controllers are a set. Offer original batch metadata explicitly
        # after their observations, independent of Settings or iteration order.
        self.stem_delete.poll()
        self.stem_batch.poll()
        poll_control_diagnostics(self)

    def poll_runtime_events(self) -> None:
        """Poll runtime event sources and update controller-owned state projections."""
        self.recording.poll()
        self.diagnostics.observe("recording", self.recording.status().error)
        self.transport.waveform.poll_holds()
        self.pairs.poll()
        self._sync_arrangement_observers()
        self.banks.poll()
        self._sync_arrangement_observers()
        self.arrangement.poll()
        self._sync_arrangement_observers()
        self.loader.poll_loader_events()
        self.material_migration.poll()
        self._poll_audio_messages()
        self.transport.global_params._poll_global_rate_status()
        self.accepted_timing.poll()
        self.transport.residency.poll()
        self.transport.pitch.poll()
        self.banks.poll()
        self._sync_arrangement_observers()
        self.arrangement.poll()
        self._sync_arrangement_observers()

        poll_control_diagnostics(self)

    def _sync_arrangement_observers(self) -> None:
        deferred = (
            self.pairs.observer_fenced_sample_ids
            | self.arrangement.observer_fenced_sample_ids
            | self.banks.observer_fenced_sample_ids
        )
        for component in (self.transport, self.loader, self.accepted_timing, self.stems):
            component._pair_deferred_sample_ids = deferred
        self._finish_arrangement_recovery()

    def ensure_not_copy_target(self, sample_id: int) -> None:
        """Reject participant gestures while a complete arrangement owns the slot."""
        validate_sample_id(sample_id)
        if self._restored_assignment_held(sample_id):
            kind = "Bank"
            if sample_id in self.arrangement.fenced_sample_ids:
                kind = "Copy target"
            elif sample_id in self.pairs.fenced_sample_ids:
                kind = "Pair"
            message = f"Complete {kind} receipt is pending; current-pad gesture rejected"
            raise ValueError(message)

    def _ensure_global_key_lock_edit_allowed(self) -> None:
        self.arrangement.ensure_global_key_lock_edit_allowed()
        self.pairs.ensure_global_key_lock_edit_allowed()
        self.banks.ensure_global_key_lock_edit_allowed()

    def _on_copy_target_removed(self, sample_id: int, predecessor: CopySnapshot) -> None:
        """Retire original target control owners only after its complete own Native ACK."""
        validate_sample_id(sample_id)
        original = predecessor.projected().source_identity.instance_id
        identity = self._project.pad_content[sample_id]
        if identity is None or identity.instance_id != original:
            message = "Acknowledged Copy lost its original target control projection"
            raise RuntimeError(message)
        self._sync_arrangement_observers()
        self.transport.residency.cancel(sample_id)
        self.accepted_timing.cancel(sample_id)
        self.loader._clear_analysis_task_state(sample_id)
        self.stems.retire_overwritten_native_state(sample_id, original_content_id=original)

    def _on_copy_committed(self, sample_id: int, ticket: PadCopyTicket) -> None:
        """Project the durable stopped target before its Native reservation is released."""
        validate_sample_id(sample_id)
        components = _copied_components_available(sample_id, ticket)
        try:
            selection = ticket.committed_stem_ticket()
        except AttributeError as error:
            message = "Complete Copy target registration metadata is unavailable"
            raise RuntimeError(message) from error
        self.stems.project_copied_native_state(
            sample_id, components_available=components, selection=selection
        )
        self._session.active_sample_ids.discard(sample_id)
        self._session.paused_sample_ids.discard(sample_id)
        self._session.pressed_pads[sample_id] = False
        self._session.pad_peak[sample_id] = 0.0
        self._session.pad_peak_updated_at[sample_id] = 0.0
        self._session.pad_clip_hold_until[sample_id] = 0.0
        self._session.pad_playhead_s[sample_id] = None
        self._session.pad_playhead_updated_at[sample_id] = 0.0
        self.metering._active_peak_sample_ids.discard(sample_id)
        self.transport.playback.discard_global_restore_for_unloaded_pad(sample_id)

    def _refresh_restored_accepted_timing(self, sample_id: int) -> None:
        self.accepted_timing.refresh_current(
            sample_id, on_refreshed=self.loader.finish_accepted_timing_refresh
        )

    def _on_sample_unloaded(self, sample_id: int) -> None:
        if self._session.waveform_editor_pad_id == sample_id:
            self.transport.waveform.close_editor()
        self.transport.residency.cancel(sample_id)
        self.accepted_timing.cancel(sample_id)
        self.transport.playback.discard_global_restore_for_unloaded_pad(sample_id)

    def _on_new_sample_loaded(self, sample_id: int, detected_start: float | None) -> None:
        self.transport.loop.initialize_loaded_pad_defaults(sample_id, detected_start)
        self.stems.reuse_compatible_stems_on_load(sample_id)
        self._refresh_native_gui_selection(sample_id)

    def _on_restored_sample_loaded(self, sample_id: int) -> bool:
        """Resolve configured selection after actual restore readiness, then refresh stems."""
        self._refresh_native_gui_selection(sample_id)
        return self.stems.publish_restored_stem_cache_if_available(sample_id)

    def _on_incoming_sample_loaded(self, sample_id: int) -> None:
        """Refresh derived master and stem work after Native has adopted incoming defaults."""
        self.transport.residency.cancel(sample_id)
        self.transport.residency.remember(sample_id)
        if self._session.bpm_lock_anchor_pad_id == sample_id:
            self.transport.bpm.recompute_master_bpm()
        self.stems.reuse_compatible_stems_on_load(sample_id)
        self._refresh_native_gui_selection(sample_id)

    def _refresh_loaded_key_lock(self, sample_id: int) -> None:
        """Retire prior-source control baselines before resolving the adopted source."""
        self.transport.residency.cancel(sample_id)
        # A settled snapshot cannot retire scalar defaults already queued by load/startup.
        self.transport.residency.apply_key_lock(sample_id, force=True)

    def _poll_audio_messages(self) -> None:
        for _ in range(len(self._pending_content_feedback)):
            msg = self._pending_content_feedback.popleft()
            self._handle_audio_message(msg)
        while True:
            incoming = self._audio.receive_msg()
            if incoming is None:
                return

            self._handle_audio_message(incoming)

    def _handle_audio_message(self, msg: AudioMessage) -> None:
        if self._handle_content_feedback(msg):
            return
        if isinstance(msg, AudioMessage.PadPeak):
            self.metering.handle_pad_peak_message(msg)
        if isinstance(msg, AudioMessage.MasterPeak):
            self.metering.handle_master_peak_message(msg)
        if isinstance(msg, AudioMessage.PadPlayhead):
            self.metering.handle_pad_playhead_message(msg)
        if isinstance(msg, AudioMessage.SampleStarted):
            self.transport.playback.handle_sample_started_message(msg)
        if isinstance(msg, AudioMessage.SampleStopped):
            self.transport.playback.handle_sample_stopped_message(msg)

    def _handle_content_feedback(self, msg: AudioMessage) -> bool:
        kind = getattr(msg, "feedback_kind", lambda: None)()
        if kind is None:
            return False
        try:
            sample_id = self._audio._resolve_pad_feedback(msg)
        except RuntimeError as error:
            if "content feedback placement pending" not in str(error):
                raise
            self._retain_content_feedback(msg)
            return True
        if sample_id is None:
            return True
        if sample_id in (
            self.pairs.observer_fenced_sample_ids
            | self.arrangement.fenced_sample_ids
            | self.banks.observer_fenced_sample_ids
        ):
            self._retain_content_feedback(msg)
            return True
        handlers = {
            "started": self.transport.playback.handle_sample_started_message,
            "stopped": self.transport.playback.handle_sample_stopped_message,
            "peak": self.metering.handle_pad_peak_message,
            "playhead": self.metering.handle_pad_playhead_message,
        }
        if (handler := handlers.get(kind)) is not None:
            handler(msg, sample_id=sample_id)
        return True

    def _retain_content_feedback(self, msg: AudioMessage) -> None:
        if len(self._pending_content_feedback) < 1024:
            self._pending_content_feedback.append(msg)

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def pending_pad_error(self) -> str | None:
        """Expose the existing owning settlement error while Close waits."""
        return (
            self.banks.error
            or self.pairs.error
            or self.transport.waveform.hold_status()[1]
            or self.arrangement.error
            or self.material_migration.error
        )

    @property
    def pending_shutdown_error(self) -> str | None:
        """Expose recording and existing settlement diagnostics while Close waits."""
        errors = (self.recording.status().error, self.pending_pad_error)
        return "\n".join(dict.fromkeys(error for error in errors if error)) or None

    @property
    def session(self) -> SessionState:
        return self._session

    @property
    def persistence(self) -> ProjectPersistence:
        return self._persistence
