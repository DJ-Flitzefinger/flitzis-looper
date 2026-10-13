import json
import math
from dataclasses import dataclass, field, replace
from itertools import islice
from pathlib import Path
from typing import TYPE_CHECKING, Literal, NotRequired, TypedDict, TypeVar
from uuid import uuid4

from pydantic import ValidationError

from flitzis_looper.constants import PAD_LOOP_BARS_DEFAULT
from flitzis_looper.controller.accepted_restore import AcceptedTimingRestore
from flitzis_looper.controller.asset_lifecycle import (
    AssetRetirementReservation,
    ProjectAssetLifecycle,
)
from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.key_metadata import KeyMetadataAnalysis
from flitzis_looper.controller.saved_residency import saved_resident_loop
from flitzis_looper.controller.validation import normalize_bpm
from flitzis_looper.key_intent import PadKeyIntent
from flitzis_looper.material_migration_model import MaterialCreationReceipt
from flitzis_looper.models import (
    PadContentIdentity,
    ProjectState,
    SampleAnalysis,
    SessionState,
    TimingIntent,
    validate_sample_id,
)
from flitzis_looper.project_materials import original_asset
from flitzis_looper_audio import ProjectAssetLease

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper_audio import AudioEngine


_PadValue = TypeVar("_PadValue")


@dataclass(frozen=True, slots=True)
class _IncomingIntent:
    request_id: int
    revision: int
    override: bool | None
    key_lock: bool
    start_s: float | None = None
    end_s: float | None = None
    auto: bool = True
    bars: float = PAD_LOOP_BARS_DEFAULT
    timing_override: bool = False
    period_seconds: float | None = None
    grid_origin_s: float | None = None
    key_lock_revision: int = 0
    loop_revision: int = 0
    timing_revision: int = 0


@dataclass(frozen=True, slots=True)
class _IncomingLoad:
    intent: _IncomingIntent
    current_source_revisions: dict[Literal["key_lock", "loop", "timing"], int] = field(
        default_factory=dict
    )


@dataclass(frozen=True, slots=True)
class _IncomingDefaults:
    revision: int
    key_lock: bool
    start_s: float
    end_s: float | None
    period_seconds: float | None
    grid_origin_s: float
    auto: bool
    bars: float


def _incoming_scalar(value: object, *, nullable: bool = False) -> float | None:
    if value is None and nullable:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        message = "Incoming source receipt has an invalid scalar"
        raise ValueError(message)
    return float(value)


def _incoming_defaults(value: object) -> _IncomingDefaults:
    """Validate the complete native receipt before mutating project projections."""
    if not isinstance(value, dict):
        message = "Incoming source has no adopted defaults receipt"
        raise TypeError(message)
    revision = value.get("revision")
    if isinstance(revision, bool) or not isinstance(revision, int) or not 0 <= revision < 2**64:
        message = "Incoming source receipt has an invalid revision"
        raise ValueError(message)
    key_lock, auto = value.get("key_lock"), value.get("loop_auto")
    if not isinstance(key_lock, bool) or not isinstance(auto, bool):
        message = "Incoming source receipt has an invalid mode"
        raise TypeError(message)
    start = _incoming_scalar(value.get("start_s"))
    end = _incoming_scalar(value.get("end_s"), nullable=True)
    period = _incoming_scalar(value.get("period_seconds"), nullable=True)
    origin = _incoming_scalar(value.get("grid_origin_s"))
    bars = _incoming_scalar(value.get("loop_bars"))
    if start is None or bars is None or origin is None:
        message = "Incoming source receipt has missing geometry or timing"
        raise ValueError(message)
    if start < 0.0 or (end is not None and end <= start):
        message = "Incoming source receipt has invalid geometry"
        raise ValueError(message)
    if (period is not None and period <= 0.0) or bars <= 0.0:
        message = "Incoming source receipt has invalid geometry or timing"
        raise ValueError(message)
    return _IncomingDefaults(revision, key_lock, start, end, period, origin, auto, bars)


def _parse_sample_analysis(analysis: object) -> SampleAnalysis | None:
    if not isinstance(analysis, dict):
        return None
    try:
        return SampleAnalysis.model_validate(analysis)
    except ValidationError:
        return None


class _RestoredLoadOptions(TypedDict):
    run_analysis: bool
    replace_assignment: bool
    source_intent: Literal["restore"]
    restore_automatic: NotRequired[bool]
    resident_loop_start_s: NotRequired[float]
    resident_loop_end_s: NotRequired[float]
    resident_key_lock: NotRequired[bool]


def _reset_pad_value(values: list[_PadValue], sample_id: int, default: _PadValue) -> bool:
    if values[sample_id] == default:
        return False

    values[sample_id] = default
    return True


class _MaterialCreationOwnership(BaseController):
    """Poll exact material birth metadata without changing ordinary loader intent."""

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)
        self._material_creation_pending: dict[int, tuple[str, str]] = {}
        self._material_creation_restore: Callable[[int, PadContentIdentity], None] | None = None

    def bind_material_creation_restore(
        self, restore: Callable[[int, PadContentIdentity], None]
    ) -> None:
        """Route saved birth evidence to the existing guarded background owner."""
        self._material_creation_restore = restore

    def _poll_material_creation(self) -> None:
        getter = getattr(self._audio, "cold_source_manifest", None)
        if not callable(getter):
            self._material_creation_pending.clear()
            return
        for sample_id, (instance_id, reference) in list(self._material_creation_pending.items())[
            :8
        ]:
            content = self._project.pad_content[sample_id]
            if (
                content is None
                or content.instance_id != instance_id
                or self._project.sample_paths[sample_id] != reference
            ):
                self._material_creation_pending.pop(sample_id)
                continue
            try:
                encoded = getter(sample_id)
                if not isinstance(encoded, str):
                    continue
                self._apply_material_creation_manifest(sample_id, content, reference, encoded)
                self._material_creation_pending.pop(sample_id)
            except (OSError, RuntimeError, ValueError, KeyError, TypeError) as error:
                self._material_creation_pending.pop(sample_id)
                self._session.sample_load_errors[sample_id] = (
                    f"Material birth ownership preserved: {error}"
                )

    def _apply_material_creation_manifest(
        self, sample_id: int, content: PadContentIdentity, reference: str, encoded: str
    ) -> None:
        manifest = json.loads(encoded)
        if (
            original_asset(manifest["original_path"]).path != original_asset(reference).path
            or manifest["material_id"] != content.material_id
        ):
            message = "newer native source wins over material birth metadata"
            raise ValueError(message)
        birth = manifest.get("material_creation")
        if birth is not None:
            creation = MaterialCreationReceipt.model_validate(birth)
            self._check_material_creation(content, reference, creation)
            if content.material_creation != creation:
                self._project.pad_content[sample_id] = content.model_copy(
                    update={"material_creation": creation}
                )
                self._mark_project_changed()
        elif content.material_creation is not None and self._material_creation_restore is not None:
            self._material_creation_restore(sample_id, content)

    @staticmethod
    def _check_material_creation(
        content: PadContentIdentity, reference: str, creation: MaterialCreationReceipt
    ) -> None:
        asset = original_asset(reference)
        if (
            asset.layout != "readable"
            or asset.material_root is None
            or asset.material_root.relative_to(Path.cwd()).as_posix() != creation.root_reference
        ):
            message = "material birth metadata belongs to another canonical source root"
            raise ValueError(message)
        if content.material_creation is not None and content.material_creation != creation:
            message = "material birth metadata changed for the same content instance"
            raise ValueError(message)


class _IncomingIntentControl:
    """Request-addressed incoming control; current-source transport stays independent."""

    _project: ProjectState
    _session: SessionState
    _audio: AudioEngine
    _incoming_loads: dict[int, _IncomingLoad]

    def record_current_pad_edit(
        self,
        sample_id: int,
        source_generation: int | None,
        domain: Literal["key_lock", "loop", "timing"],
    ) -> None:
        """Retain only generic gestures proven to address the adopted incoming source."""
        incoming = self._incoming_loads.get(sample_id)
        if (
            incoming is None
            or isinstance(source_generation, bool)
            or source_generation != incoming.intent.request_id
        ):
            return
        revision = incoming.intent.revision + 1
        intent = replace(incoming.intent, revision=revision)
        if domain == "key_lock":
            override = self._project.pad_key_lock[sample_id]
            intent = replace(
                intent,
                override=override,
                key_lock=self._project.resolve_pad_key_lock(
                    sample_id, assignment_override=override
                ),
            )
        self._incoming_loads[sample_id] = replace(
            incoming,
            intent=intent,
            current_source_revisions={**incoming.current_source_revisions, domain: revision},
        )

    def incoming_load_request_id(self, sample_id: int) -> int | None:
        """Return the request address required for explicit incoming-source edits."""
        validate_sample_id(sample_id)
        incoming = self._incoming_loads.get(sample_id)
        return incoming.intent.request_id if incoming is not None else None

    def set_incoming_key_lock_override(
        self, sample_id: int, request_id: int, *, override: bool | None
    ) -> None:
        """Address ON/OFF/inheritance to the incoming source without editing its predecessor."""
        if override is not None and not isinstance(override, bool):
            message = "Incoming Key Lock override must be ON, OFF or inheritance"
            raise ValueError(message)
        intent = self._incoming_intent(sample_id, request_id)
        self._update_incoming_intent(
            sample_id,
            replace(
                intent,
                override=override,
                key_lock=self._project.resolve_pad_key_lock(
                    sample_id, assignment_override=override
                ),
            ),
            domain="key_lock",
        )

    def set_incoming_loop_intent(
        self,
        sample_id: int,
        request_id: int,
        *,
        start_s: float | None,
        end_s: float | None,
        auto: bool,
        bars: float,
    ) -> None:
        """Address loop demand to one incoming load; None start retains activity default."""
        intent = self._incoming_intent(sample_id, request_id)
        self._update_incoming_intent(
            sample_id,
            replace(intent, start_s=start_s, end_s=end_s, auto=auto, bars=bars),
            domain="loop",
        )

    def set_incoming_timing_intent(
        self,
        sample_id: int,
        request_id: int,
        *,
        period_seconds: float,
        grid_origin_s: float | None,
    ) -> None:
        """Address scalar timing demand to incoming material.

        Accepted predecessor timing never establishes incoming authority.
        An explicit absent period cannot be represented by durable Legacy intent.
        """
        if (
            isinstance(period_seconds, bool)
            or not isinstance(period_seconds, (int, float))
            or not math.isfinite(period_seconds)
            or period_seconds <= 0.0
        ):
            message = "Incoming timing period must be positive and finite"
            raise ValueError(message)
        intent = self._incoming_intent(sample_id, request_id)
        self._update_incoming_intent(
            sample_id,
            replace(
                intent,
                timing_override=True,
                period_seconds=period_seconds,
                grid_origin_s=grid_origin_s,
            ),
            domain="timing",
        )

    def _incoming_intent(self, sample_id: int, request_id: int) -> _IncomingIntent:
        validate_sample_id(sample_id)
        incoming = self._incoming_loads.get(sample_id)
        if (
            isinstance(request_id, bool)
            or incoming is None
            or incoming.intent.request_id != request_id
        ):
            message = "Incoming source request is no longer current"
            raise ValueError(message)
        return incoming.intent

    def _update_incoming_intent(
        self,
        sample_id: int,
        intent: _IncomingIntent,
        *,
        domain: Literal["key_lock", "loop", "timing"],
    ) -> None:
        incoming = self._incoming_loads[sample_id]
        updated = replace(intent, revision=incoming.intent.revision + 1)
        if domain == "key_lock":
            updated = replace(updated, key_lock_revision=updated.revision)
        elif domain == "loop":
            updated = replace(updated, loop_revision=updated.revision)
        else:
            updated = replace(updated, timing_revision=updated.revision)
        self._audio.update_incoming_load_intent(
            sample_id,
            updated.request_id,
            updated.revision,
            updated.key_lock,
            loop_start_s=updated.start_s,
            loop_end_s=updated.end_s,
            loop_auto=updated.auto,
            loop_bars=updated.bars,
            timing_override=updated.timing_override,
            period_seconds=updated.period_seconds,
            grid_origin_s=updated.grid_origin_s,
        )
        self._incoming_loads[sample_id] = replace(incoming, intent=updated)

    def _refresh_incoming_global_intent(self) -> None:
        for sample_id, incoming in tuple(self._incoming_loads.items()):
            intent = incoming.intent
            if intent.override is not None:
                continue
            enabled = self._project.resolve_pad_key_lock(sample_id, assignment_override=None)
            if enabled == intent.key_lock:
                continue
            try:
                self._update_incoming_intent(
                    sample_id, replace(intent, key_lock=enabled), domain="key_lock"
                )
            except (RuntimeError, ValueError) as error:
                self._session.sample_load_errors[sample_id] = str(error)

    def _incoming_loop_signature(self, sample_id: int) -> tuple[float, float | None, bool, float]:
        return (
            self._project.pad_loop_start_s[sample_id],
            self._project.pad_loop_end_s[sample_id],
            self._project.pad_loop_auto[sample_id],
            self._project.pad_loop_bars[sample_id],
        )

    def _incoming_timing_signature(
        self, sample_id: int
    ) -> tuple[float | None, TimingIntent, float | None, int]:
        return (
            self._project.manual_bpm[sample_id],
            self._project.pad_timing_intent[sample_id],
            self._project.pad_grid_anchor_s[sample_id],
            self._project.pad_grid_offset_samples[sample_id],
        )


class LoaderController(_IncomingIntentControl, _MaterialCreationOwnership):
    _COLD_QUEUE_FULL = "cold source queue full (2 workers, 32 queued jobs)"
    _RESTORE_ADMISSIONS_PER_POLL = 8

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_pad_bpm_changed: Callable[[int], None],
        on_project_changed: Callable[[], None] | None = None,
        on_stem_generation_started: Callable[[int], None] | None = None,
        on_stem_generation_progress: Callable[[int, float | None, str | None], None] | None = None,
        on_stem_generation_success: Callable[[int], None] | None = None,
        on_stem_generation_error: Callable[[int, str], None] | None = None,
        on_stems_deleted: Callable[[int], bool] | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)

        self._on_pad_bpm_changed = on_pad_bpm_changed
        self._assets = ProjectAssetLifecycle(project, audio)
        self._on_stem_generation_started = on_stem_generation_started
        self._on_stem_generation_progress = on_stem_generation_progress
        self._on_stem_generation_success = on_stem_generation_success
        self._on_stem_generation_error = on_stem_generation_error
        self._on_stems_deleted = on_stems_deleted
        self._on_stems_invalidated: Callable[[int], None] | None = None
        self._on_restored_sample_loaded: Callable[[int], bool] | None = None
        self._on_new_sample_loaded: Callable[[int, float | None], None] | None = None
        self._on_incoming_sample_loaded: Callable[[int], None] | None = None
        self._on_loaded_key_lock: Callable[[int], None] | None = None
        self._on_sample_unloaded: Callable[[int], None] | None = None
        self._on_accepted_timing_refresh: Callable[[int], None] | None = None
        self._load_request_ids: dict[int, int] = {}
        self._migration_event_handler: Callable[[dict[str, object]], bool] | None = None
        self._load_retirements: dict[tuple[int, int | None], AssetRetirementReservation] = {}
        self._new_load_sample_ids: set[int] = set()
        self._incoming_loads: dict[int, _IncomingLoad] = {}
        self._deferred_restores: dict[int, Path] = {}
        self._analysis_request_ids: dict[int, int] = {}
        self._key_analysis = KeyMetadataAnalysis(project, audio)
        self._accepted_restore = AcceptedTimingRestore(
            project, session, audio, self._finish_accepted_restore
        )

    def _finish_accepted_restore(self, sample_id: int) -> None:
        if self._on_accepted_timing_refresh is not None:
            self._on_accepted_timing_refresh(sample_id)
            return
        self._on_pad_bpm_changed(sample_id)
        self.finish_accepted_timing_refresh(sample_id)
        self._mark_project_changed()

    def finish_accepted_timing_refresh(self, sample_id: int) -> None:
        """Refresh restored stem intent after acknowledged derived timing completion."""
        if self._on_restored_sample_loaded is not None:
            self._on_restored_sample_loaded(sample_id)

    def set_accepted_timing_refresh_callback(self, callback: Callable[[int], None]) -> None:
        """Route fresh accepted restore through the application's guarded completion."""
        self._on_accepted_timing_refresh = callback

    def shut_down(self) -> None:
        """Drain owned timing restoration before native stream teardown."""
        for sample_id in list(self._deferred_restores):
            self._cancel_deferred_restore(sample_id)
        self._accepted_restore.shut_down()
        for reservation in self._load_retirements.values():
            reservation.close()
        self._load_retirements.clear()
        self._incoming_loads.clear()

    def set_new_sample_loaded_callback(
        self,
        callback: Callable[[int, float | None], None],
        *,
        key_lock_callback: Callable[[int], None] | None = None,
        incoming_callback: Callable[[int], None] | None = None,
    ) -> None:
        """Register behavior that runs after a newly assigned sample finishes loading."""
        self._on_new_sample_loaded = callback
        self._on_loaded_key_lock = key_lock_callback
        self._on_incoming_sample_loaded = incoming_callback

    def bind_asset_lifecycle(self, lifecycle: ProjectAssetLifecycle) -> None:
        """Share assignment and separator owners with the application controller."""
        self._assets = lifecycle

    def set_restored_sample_loaded_callback(self, callback: Callable[[int], bool]) -> None:
        """Register behavior that runs after a restored sample finishes loading."""
        self._on_restored_sample_loaded = callback

    def set_sample_unloaded_callback(self, callback: Callable[[int], None]) -> None:
        """Register control-intent cleanup after a native unload is admitted."""
        self._on_sample_unloaded = callback

    def set_stems_invalidated_callback(self, callback: Callable[[int], None]) -> None:
        """Revoke retired stem eligibility without deleting readers' artifacts."""
        self._on_stems_invalidated = callback

    def restore_samples_from_project_state(self) -> None:
        """Schedule async loads for cached samples referenced by `ProjectState`.

        Invalid/missing cached files are ignored by clearing the pad assignment.
        Native queue saturation defers remaining restores to bounded UI polling.
        """
        output_sample_rate = self._output_sample_rate_hz()
        if output_sample_rate is None:
            return

        changed = False
        defer_remaining = False
        for sample_id, path in enumerate(self._project.sample_paths):
            if path is None:
                continue

            rel = self._parse_cached_sample_path(path)
            if rel is None:
                self._clear_restored_pad(sample_id)
                changed = True
                continue

            abs_path = Path.cwd() / rel
            if not abs_path.is_file():
                self._clear_restored_pad(sample_id)
                changed = True
                continue

            if defer_remaining:
                self._defer_restored_load(sample_id, rel)
            elif not self._schedule_restored_load(sample_id, rel, run_analysis=False):
                defer_remaining = sample_id in self._deferred_restores

        if changed:
            self._mark_project_changed()

    def load_sample_async(self, sample_id: int, path: str) -> None:
        """Load an audio file into a sample slot asynchronously.

        Native admission and preparation retain the previous assignment. UI code
        should call `poll_loader_events()` to apply an acknowledged replacement
        or a failure that leaves the previous source and settings intact.

        Args:
            sample_id: Sample slot identifier.
            path: Path to an audio file on disk.
        """
        validate_sample_id(sample_id)
        reservation = None
        try:
            reservation = self._assets.reserve()
            key_lock = self._project.resolve_pad_key_lock(sample_id, assignment_override=None)
            request_id = self._audio.load_incoming_sample_async(sample_id, path, key_lock)
        except (RuntimeError, ValueError) as error:
            if reservation is not None:
                reservation.close()
            self._session.sample_load_errors[sample_id] = str(error)
            return

        self._deferred_restores.pop(sample_id, None)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

        self._session.pending_sample_paths[sample_id] = path
        self._session.loading_sample_ids.add(sample_id)
        self._load_request_ids.pop(sample_id, None)
        self._new_load_sample_ids.add(sample_id)
        self._record_load_request_id(sample_id, request_id)
        self._record_load_retirement(sample_id, reservation)
        if isinstance(request_id, int) and not isinstance(request_id, bool):
            self._incoming_loads[sample_id] = _IncomingLoad(
                _IncomingIntent(request_id, 0, None, key_lock),
            )

    def unload_sample(self, sample_id: int) -> None:
        """Stop playback and unload a sample slot."""
        validate_sample_id(sample_id)
        with self._assets.admission():
            self._unload_sample(sample_id)

    def _unload_sample(self, sample_id: int) -> None:
        self._assets.sync_assignments()
        self._audio.unload_sample(sample_id)
        self._cancel_deferred_restore(sample_id)
        self._accepted_restore.cancel(sample_id)
        if self._on_sample_unloaded is not None:
            self._on_sample_unloaded(sample_id)
        self._clear_source_session(sample_id)
        self._session.loading_sample_ids.discard(sample_id)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._load_request_ids.pop(sample_id, None)
        self._new_load_sample_ids.discard(sample_id)
        self._incoming_loads.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.sample_load_errors.pop(sample_id, None)
        if self._on_stems_deleted is not None:
            self._on_stems_deleted(sample_id)
        else:
            self._clear_stem_cache(sample_id)

        self._reset_unloaded_pad_defaults(sample_id)
        self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()
        self._assets.sync_assignments()

    def analyze_sample_async(self, sample_id: int) -> None:
        """Analyze a previously loaded sample asynchronously."""
        validate_sample_id(sample_id)
        if self.is_sample_loading(sample_id):
            return

        if not self.is_sample_loaded(sample_id):
            self._clear_analysis_task_state(sample_id)
            self._session.sample_analysis_errors[sample_id] = "sample is not loaded"
            return

        try:
            key_admission = self._key_analysis.prepare(sample_id)
            request_id = self._audio.analyze_sample_async(sample_id)
        except (RuntimeError, ValueError) as err:
            self._session.sample_analysis_errors[sample_id] = str(err)
        else:
            self._clear_analysis_task_messages(sample_id)
            self._session.analyzing_sample_ids.add(sample_id)
            self._analysis_request_ids.pop(sample_id, None)
            self._record_analysis_request_id(sample_id, request_id)
            try:
                if self._key_analysis.admit(sample_id, key_admission, request_id):
                    self._mark_project_changed()
            except (RuntimeError, ValueError) as error:
                message = f"Analysis key metadata admission failed: {error}"
                self._key_analysis.record_error(sample_id, request_id, message)
                self._session.sample_analysis_errors[sample_id] = message

    def poll_loader_events(self) -> None:
        """Drain pending loader events from the Rust audio engine."""
        self._refresh_incoming_global_intent()
        self._accepted_restore.poll()
        self._poll_material_creation()
        self._assets.retry_retirements()
        handlers = {
            "started": self._handle_loader_started,
            "progress": self._handle_loader_progress,
            "success": self._handle_loader_success,
            "error": self._handle_loader_error,
            "task_started": self._handle_task_started,
            "task_progress": self._handle_task_progress,
            "task_success": self._handle_task_success,
            "task_error": self._handle_task_error,
        }

        while True:
            event = self._audio.poll_loader_events()
            if event is None:
                self._poll_material_creation()
                self._retry_deferred_restores()
                return

            if self._migration_event_handler is not None and self._migration_event_handler(event):
                continue

            event_type = event.get("type")
            sample_id = event.get("id")
            if not isinstance(event_type, str) or not isinstance(sample_id, int):
                continue

            handler = handlers.get(event_type)
            if handler is None:
                continue

            handler(sample_id, event)

    def _set_migration_event_handler(self, handler: Callable[[dict[str, object]], bool]) -> None:
        """Route only exact migration request events before ordinary assignment resets."""
        self._migration_event_handler = handler

    def is_sample_loaded(self, sample_id: int) -> bool:
        """Return whether a sample slot has audio loaded."""
        validate_sample_id(sample_id)
        return self._project.sample_paths[sample_id] is not None

    def is_sample_loading(self, sample_id: int) -> bool:
        """Return whether a sample slot is currently being loaded."""
        validate_sample_id(sample_id)
        return sample_id in self._session.loading_sample_ids

    def pending_sample_path(self, sample_id: int) -> str | None:
        """Return the pending path for an in-flight async load."""
        validate_sample_id(sample_id)
        return self._session.pending_sample_paths.get(sample_id)

    def sample_load_error(self, sample_id: int) -> str | None:
        """Return the last async load error message for a pad."""
        validate_sample_id(sample_id)
        return self._session.sample_load_errors.get(sample_id)

    def sample_load_progress(self, sample_id: int) -> float | None:
        """Return best-effort async load progress for a pad."""
        validate_sample_id(sample_id)
        value = self._session.sample_load_progress.get(sample_id)
        return float(value) if value is not None else None

    def sample_load_stage(self, sample_id: int) -> str | None:
        """Return the last reported async load stage for a pad."""
        validate_sample_id(sample_id)
        return self._session.sample_load_stage.get(sample_id)

    def _clear_analysis_task_state(self, sample_id: int) -> None:
        self._session.analyzing_sample_ids.discard(sample_id)
        self._analysis_request_ids.pop(sample_id, None)
        self._key_analysis.cancel(sample_id)
        self._clear_analysis_task_messages(sample_id)

    def _clear_analysis_task_messages(self, sample_id: int) -> None:
        self._session.sample_analysis_errors.pop(sample_id, None)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

    def _clear_stem_generation_state(self, sample_id: int) -> None:
        self._session.stem_generating_sample_ids.discard(sample_id)
        self._session.stem_generation_source_versions.pop(sample_id, None)
        self._clear_stem_generation_messages(sample_id)

    def _clear_stem_generation_messages(self, sample_id: int) -> None:
        self._session.stem_generation_errors.pop(sample_id, None)
        self._session.stem_generation_diagnostics.pop(sample_id, None)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)

    def _clear_stem_cache(self, sample_id: int) -> None:
        self._project.stem_cache[sample_id] = None

    def _reset_unloaded_pad_defaults(self, sample_id: int) -> None:
        defaults = ProjectState()
        self._reset_unloaded_pad_project_defaults(sample_id, defaults)
        self._publish_unloaded_pad_audio_defaults(sample_id, defaults)

    def _reset_unloaded_pad_project_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        source_changed = self._reset_unloaded_pad_source_defaults(sample_id, defaults)
        mixing_changed = self._reset_unloaded_pad_mixing_defaults(sample_id, defaults)
        loop_changed = self._reset_unloaded_pad_loop_defaults(sample_id, defaults)
        return source_changed or mixing_changed or loop_changed

    def _reset_unloaded_pad_source_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        return any((
            _reset_pad_value(
                self._project.sample_paths, sample_id, defaults.sample_paths[sample_id]
            ),
            _reset_pad_value(self._project.pad_content, sample_id, defaults.pad_content[sample_id]),
            _reset_pad_value(
                self._project.sample_durations,
                sample_id,
                defaults.sample_durations[sample_id],
            ),
            _reset_pad_value(
                self._project.sample_analysis,
                sample_id,
                defaults.sample_analysis[sample_id],
            ),
            _reset_pad_value(self._project.stem_cache, sample_id, defaults.stem_cache[sample_id]),
            _reset_pad_value(
                self._project.pad_stem_mix_mode,
                sample_id,
                defaults.pad_stem_mix_mode[sample_id],
            ),
            _reset_pad_value(
                self._project.pad_key_lock,
                sample_id,
                defaults.pad_key_lock[sample_id],
            ),
            _reset_pad_value(self._project.manual_bpm, sample_id, defaults.manual_bpm[sample_id]),
            _reset_pad_value(
                self._project.pad_key_intent, sample_id, defaults.pad_key_intent[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_timing_intent, sample_id, defaults.pad_timing_intent[sample_id]
            ),
        ))

    def _reset_unloaded_pad_mixing_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        return any((
            _reset_pad_value(self._project.pad_gain_db, sample_id, defaults.pad_gain_db[sample_id]),
            _reset_pad_value(
                self._project.pad_eq_low_db, sample_id, defaults.pad_eq_low_db[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_eq_mid_db, sample_id, defaults.pad_eq_mid_db[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_eq_high_db, sample_id, defaults.pad_eq_high_db[sample_id]
            ),
        ))

    def _reset_unloaded_pad_loop_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        return any((
            _reset_pad_value(
                self._project.pad_loop_start_s,
                sample_id,
                defaults.pad_loop_start_s[sample_id],
            ),
            _reset_pad_value(
                self._project.pad_loop_end_s, sample_id, defaults.pad_loop_end_s[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_loop_auto, sample_id, defaults.pad_loop_auto[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_loop_bars, sample_id, defaults.pad_loop_bars[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_grid_offset_samples,
                sample_id,
                defaults.pad_grid_offset_samples[sample_id],
            ),
            _reset_pad_value(
                self._project.pad_grid_anchor_s,
                sample_id,
                defaults.pad_grid_anchor_s[sample_id],
            ),
        ))

    def _publish_unloaded_pad_audio_defaults(self, sample_id: int, defaults: ProjectState) -> None:
        self._restore_legacy_timing_authority(sample_id, None)
        self._audio.set_pad_gain(sample_id, defaults.pad_gain_db[sample_id])
        self._audio.set_pad_eq(
            sample_id,
            defaults.pad_eq_low_db[sample_id],
            defaults.pad_eq_mid_db[sample_id],
            defaults.pad_eq_high_db[sample_id],
        )
        self._audio.set_pad_loop_region(
            sample_id,
            defaults.pad_loop_start_s[sample_id],
            defaults.pad_loop_end_s[sample_id],
        )
        disabled = False
        self._audio.set_pad_key_lock(sample_id, disabled)

    def _handle_loader_started(self, sample_id: int, _event: dict[str, object]) -> None:
        if not self._matches_load_request(sample_id, _event):
            return

        self._session.loading_sample_ids.add(sample_id)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

    def _handle_loader_progress(self, sample_id: int, event: dict[str, object]) -> None:
        if not self._matches_load_request(sample_id, event):
            return

        stage = event.get("stage")
        if isinstance(stage, str):
            self._session.sample_load_stage[sample_id] = stage

        percent = event.get("percent")
        if isinstance(percent, (int, float)):
            self._session.sample_load_progress[sample_id] = float(percent)

    def _handle_loader_success(self, sample_id: int, event: dict[str, object]) -> None:
        reservation = self._take_load_retirement(sample_id, event)
        if reservation is None:
            reservation = self._assets.reserve()
        try:
            with reservation.activate():
                self._apply_loader_success(sample_id, event)
        finally:
            reservation.close()

    def _apply_loader_success(self, sample_id: int, event: dict[str, object]) -> None:
        if not self._matches_load_request(sample_id, event):
            cached_path = event.get("cached_path")
            if isinstance(cached_path, str):
                self._assets.retire_unassigned_original(cached_path)
            return

        incoming = self._incoming_loads.get(sample_id)

        pending = self._session.pending_sample_paths.get(sample_id)
        cached_path = event.get("cached_path")
        target_path = cached_path if isinstance(cached_path, str) else pending
        if target_path is None:
            self._settle_load_request(sample_id)
            self._session.sample_load_errors[sample_id] = "Loaded source has no original reference"
            return
        try:
            adopted = self._validate_incoming_receipt(incoming, event)
            self._assets.sync_assignments()
            target_path, previous_path, new_assignment, content, prepared = (
                self._prepare_completed_assignment(sample_id, target_path, event)
            )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._settle_load_request(sample_id)
            self._session.sample_load_errors[sample_id] = (
                f"Loaded source assignment failed: {error}"
            )
            return

        self._settle_load_request(sample_id)
        self._session.sample_load_errors.pop(sample_id, None)
        restored_assignment = not new_assignment
        timing_stale = event.get("timing_stale") is True
        incoming_refresh = (False, False)
        if new_assignment:
            incoming_refresh = self._reset_completed_source(
                sample_id, event, incoming, adopted, timing_stale=timing_stale
            )
            self._project.pad_key_intent[sample_id] = PadKeyIntent()
            self._key_analysis.cancel(sample_id)
            self._project.sample_paths[sample_id] = target_path
            self._clear_stem_cache(sample_id)
        elif restored_assignment and previous_path != target_path:
            self._project.sample_paths[sample_id] = target_path

        self._record_completed_identity(
            sample_id, content, prepared, changed=new_assignment or previous_path != target_path
        )

        self._record_completed_duration(sample_id, event)

        try:
            self._assets.sync_assignments()
            if incoming is not None and adopted is not None:
                self._finish_incoming_load(sample_id, event, incoming_refresh)
                return
            if new_assignment and not timing_stale:
                self._publish_unloaded_pad_audio_defaults(sample_id, ProjectState())
            self._finish_loaded_timing(
                sample_id,
                event,
                new_assignment=new_assignment,
                restored_assignment=restored_assignment,
                timing_stale=timing_stale,
            )
        except (RuntimeError, ValueError) as error:
            self._session.sample_load_errors[sample_id] = (
                f"Loaded source control refresh failed: {error}"
            )

    @staticmethod
    def _validate_incoming_receipt(
        incoming: _IncomingLoad | None, event: dict[str, object]
    ) -> _IncomingDefaults | None:
        if incoming is None:
            return None
        adopted = _incoming_defaults(event.get("incoming_defaults"))
        if adopted.revision > incoming.intent.revision:
            message = "Incoming source receipt refers to an unknown intent revision"
            raise ValueError(message)
        return adopted

    def _record_completed_duration(self, sample_id: int, event: dict[str, object]) -> None:
        duration_s = event.get("duration_s")
        if isinstance(duration_s, float):
            self._project.sample_durations[sample_id] = duration_s

    def _reset_completed_source(
        self,
        sample_id: int,
        event: dict[str, object],
        incoming: _IncomingLoad | None,
        adopted: _IncomingDefaults | None,
        *,
        timing_stale: bool,
    ) -> tuple[bool, bool]:
        if incoming is not None and adopted is not None:
            return self._project_incoming_defaults(
                sample_id, event, incoming, adopted, timing_stale=timing_stale
            )
        self._project.pad_key_lock[sample_id] = None
        self._reset_completed_assignment(sample_id, timing_stale=timing_stale)
        return False, False

    def _settle_load_request(self, sample_id: int) -> None:
        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._load_request_ids.pop(sample_id, None)
        self._new_load_sample_ids.discard(sample_id)
        self._incoming_loads.pop(sample_id, None)

    def _project_incoming_defaults(
        self,
        sample_id: int,
        event: dict[str, object],
        incoming: _IncomingLoad,
        adopted: _IncomingDefaults,
        *,
        timing_stale: bool,
    ) -> tuple[bool, bool]:
        """Project own receipt and preserve subsequent, separately addressed control domains."""
        loop = self._incoming_loop_signature(sample_id)
        timing = self._incoming_timing_signature(sample_id)
        intent = incoming.intent
        revisions = incoming.current_source_revisions
        preserve_override = revisions.get("key_lock", -1) > intent.key_lock_revision
        preserve_loop = revisions.get("loop", -1) > intent.loop_revision
        override = self._project.pad_key_lock[sample_id]
        later_mode = intent.key_lock_revision > adopted.revision
        later_loop = intent.loop_revision > adopted.revision
        later_timing = intent.timing_revision > adopted.revision
        preserve_timing = (timing_stale and not later_timing) or (
            revisions.get("timing", -1) > intent.timing_revision
        )
        self._reset_completed_assignment(sample_id, timing_stale=timing_stale)
        self._project.pad_key_lock[sample_id] = override if preserve_override else intent.override
        if preserve_loop:
            self._set_incoming_loop_projection(sample_id, loop)
        else:
            self._project.pad_loop_start_s[sample_id] = (
                intent.start_s if later_loop and intent.start_s is not None else adopted.start_s
            )
            self._project.pad_loop_end_s[sample_id] = intent.end_s if later_loop else adopted.end_s
            self._project.pad_loop_auto[sample_id] = intent.auto if later_loop else adopted.auto
            self._project.pad_loop_bars[sample_id] = intent.bars if later_loop else adopted.bars
        if preserve_timing:
            self._set_incoming_timing_projection(sample_id, timing)
        else:
            period = intent.period_seconds if later_timing else adopted.period_seconds
            self._project.manual_bpm[sample_id] = (
                60.0 / period if intent.timing_override and period is not None else None
            )
            self._project.pad_timing_intent[sample_id] = (
                "manual" if self._project.manual_bpm[sample_id] is not None else "legacy"
            )
            origin = (
                intent.grid_origin_s
                if later_timing and intent.grid_origin_s is not None
                else adopted.grid_origin_s
            )
            self._set_incoming_grid_projection(sample_id, origin)
        parsed = _parse_sample_analysis(event.get("analysis"))
        self._project.sample_analysis[sample_id] = parsed
        # Later current-source edits have already obtained their own Native authority.
        # Only explicitly incoming edits newer than the receipt need post-ACK work.
        return preserve_override or later_mode, (later_loop and not preserve_loop) or (
            later_timing and not preserve_timing
        )

    def _set_incoming_grid_projection(self, sample_id: int, origin: float) -> None:
        """Preserve signed scalar origins within the durable nonnegative base/offset format."""
        rate = self._output_sample_rate_hz()
        self._project.pad_grid_anchor_s[sample_id] = max(0.0, origin)
        self._project.pad_grid_offset_samples[sample_id] = (
            round(origin * rate) if origin < 0.0 and rate is not None else 0
        )

    def _set_incoming_loop_projection(
        self, sample_id: int, values: tuple[float, float | None, bool, float]
    ) -> None:
        # The tuple contains the already validated ProjectState fields captured above.
        (
            self._project.pad_loop_start_s[sample_id],
            self._project.pad_loop_end_s[sample_id],
            self._project.pad_loop_auto[sample_id],
            self._project.pad_loop_bars[sample_id],
        ) = values

    def _set_incoming_timing_projection(
        self, sample_id: int, values: tuple[float | None, TimingIntent, float | None, int]
    ) -> None:
        (
            self._project.manual_bpm[sample_id],
            self._project.pad_timing_intent[sample_id],
            self._project.pad_grid_anchor_s[sample_id],
            self._project.pad_grid_offset_samples[sample_id],
        ) = values

    def _finish_incoming_load(
        self, sample_id: int, event: dict[str, object], refresh: tuple[bool, bool]
    ) -> None:
        """Finish source bookkeeping without republishing the already adopted defaults."""
        parsed = self._project.sample_analysis[sample_id]
        if parsed is not None and self._key_analysis.loaded(
            sample_id, event.get("request_id"), parsed.key
        ):
            self._mark_project_changed()
        self._clear_analysis_task_state(sample_id)
        analysis_error = event.get("analysis_error")
        if isinstance(analysis_error, str):
            self._session.sample_analysis_errors[sample_id] = analysis_error
        if self._on_incoming_sample_loaded is not None:
            self._on_incoming_sample_loaded(sample_id)
        if refresh[0] and self._on_loaded_key_lock is not None:
            self._on_loaded_key_lock(sample_id)
        if refresh[1]:
            self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()

    def _record_completed_identity(
        self,
        sample_id: int,
        content: PadContentIdentity,
        prepared: tuple[Path, ProjectAssetLease],
        *,
        changed: bool,
    ) -> None:
        changed = changed or self._project.pad_content[sample_id] != content
        self._project.pad_content[sample_id] = content
        reference = self._project.sample_paths[sample_id]
        if reference is not None:
            self._material_creation_pending[sample_id] = (content.instance_id, reference)
        self._assets.adopt_original(sample_id, prepared)
        if changed:
            self._mark_project_changed()

    def _prepare_completed_assignment(
        self, sample_id: int, target_path: str, event: dict[str, object]
    ) -> tuple[str, str | None, bool, PadContentIdentity, tuple[Path, ProjectAssetLease]]:
        """Reserve the complete original/identity tuple before resetting old intent."""
        asset = original_asset(target_path)
        target_path = asset.path.relative_to(Path.cwd()).as_posix()
        previous_path = self._project.sample_paths[sample_id]
        normalized_previous = (
            self._normalize_project_path(previous_path) if previous_path is not None else None
        )
        new_assignment = (
            sample_id in self._new_load_sample_ids or normalized_previous != target_path
        )
        content = self._project.pad_content[sample_id]
        if new_assignment or content is None:
            content = PadContentIdentity(instance_id=uuid4().hex, material_id=asset.material_id)
        else:
            self._validate_restored_material_identity(sample_id, asset.material_id)
        delivered = event.get("original_lease")
        prepared = self._assets.prepare_original(
            target_path, delivered if isinstance(delivered, ProjectAssetLease) else None
        )
        return target_path, previous_path, new_assignment, content, prepared

    def _finish_loaded_timing(
        self,
        sample_id: int,
        event: dict[str, object],
        *,
        new_assignment: bool,
        restored_assignment: bool,
        timing_stale: bool,
    ) -> None:
        """Refresh derived intent after the complete matching assignment is recorded."""
        if self._on_loaded_key_lock is not None:
            self._on_loaded_key_lock(sample_id)
        if self._begin_accepted_restore(
            sample_id, restored_assignment=restored_assignment, timing_stale=timing_stale
        ):
            return
        # A successful ordinary load owns its timing again. Passive refresh
        # cannot clear Automatic authority while acceptance is still pending.
        self._restore_legacy_timing_authority(sample_id, None, timing_stale=timing_stale)
        if new_assignment and not timing_stale:
            self._on_pad_bpm_changed(sample_id)
        # The source still belongs to this load, but a newer native request owns timing.
        # Settle source bookkeeping without replaying automatic or restored grid intent.
        if not timing_stale and new_assignment and self._on_new_sample_loaded is not None:
            detected_start = event.get("detected_loop_start_s")
            self._on_new_sample_loaded(
                sample_id, detected_start if isinstance(detected_start, float) else None
            )

        if not timing_stale:
            self._apply_loaded_analysis(
                sample_id,
                event.get("analysis"),
                key_request_id=event.get("request_id") if new_assignment else None,
            )
            self._clear_analysis_task_state(sample_id)
        analysis_error = event.get("analysis_error")
        if isinstance(analysis_error, str):
            self._session.sample_analysis_errors[sample_id] = analysis_error

        if restored_assignment and self._on_restored_sample_loaded is not None:
            self._on_restored_sample_loaded(sample_id)

    def _reset_completed_assignment(self, sample_id: int, *, timing_stale: bool = False) -> None:
        """Reset retired track intent after native replacement, without a second unload."""
        if timing_stale:
            # A newer timing intent does not validate the predecessor's analysis.
            self._project.sample_analysis[sample_id] = None
            self._clear_analysis_task_state(sample_id)
            if self._on_stems_invalidated is not None:
                self._on_stems_invalidated(sample_id)
            return
        self._accepted_restore.cancel(sample_id)
        if self._on_sample_unloaded is not None:
            self._on_sample_unloaded(sample_id)
        self._clear_source_session(sample_id)
        if self._on_stems_invalidated is not None:
            self._on_stems_invalidated(sample_id)
        self._reset_unloaded_pad_project_defaults(sample_id, ProjectState())

    def _clear_source_session(self, sample_id: int) -> None:
        """Clear source projections shared by explicit unload and completed replacement."""
        self._session.active_sample_ids.discard(sample_id)
        self._session.paused_sample_ids.discard(sample_id)
        self._session.global_stop_restore_sample_ids.discard(sample_id)
        self._session.pressed_pads[sample_id] = False
        self._session.pad_peak[sample_id] = 0.0
        self._session.pad_peak_updated_at[sample_id] = 0.0
        self._session.pad_clip_hold_until[sample_id] = 0.0
        self._session.pad_playhead_s[sample_id] = None
        self._session.pad_playhead_updated_at[sample_id] = 0.0
        if self._session.waveform_editor_pad_id == sample_id:
            self._session.waveform_editor_open = False
            self._session.waveform_editor_pad_id = None
        if self._session.waveform_pause_hold_pad_id == sample_id:
            self._session.waveform_pause_hold_pad_id = None
        if self._session.tap_bpm_pad_id == sample_id:
            self._session.tap_bpm_pad_id = None
            self._session.tap_bpm_timestamps.clear()
        self._clear_analysis_task_state(sample_id)
        self._clear_stem_generation_state(sample_id)

    def _apply_loaded_analysis(
        self, sample_id: int, analysis: object, *, key_request_id: object = None
    ) -> None:
        if analysis is not None:
            parsed = self._store_sample_analysis(sample_id, analysis)
            if parsed is not None and self._key_analysis.loaded(
                sample_id, key_request_id, parsed.key
            ):
                self._mark_project_changed()
        elif (
            self._project.sample_analysis[sample_id] is not None
            or self._project.manual_bpm[sample_id] is not None
            or self._project.pad_grid_offset_samples[sample_id] != 0
            or self._project.pad_grid_anchor_s[sample_id] is not None
            or self._project.pad_timing_intent[sample_id] in {"manual", "tap"}
        ):
            self._on_pad_bpm_changed(sample_id)

    def _handle_loader_error(self, sample_id: int, event: dict[str, object]) -> None:
        reservation = self._take_load_retirement(sample_id, event)
        if reservation is not None:
            reservation.close()
        if not self._matches_load_request(sample_id, event):
            return

        self._settle_load_request(sample_id)

        msg = event.get("msg")
        if isinstance(msg, str):
            self._session.sample_load_errors[sample_id] = msg

    def _handle_task_started(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_started is not None:
                self._on_stem_generation_started(sample_id)
                return

            self._session.stem_generating_sample_ids.add(sample_id)
            self._clear_stem_generation_messages(sample_id)
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        self._session.analyzing_sample_ids.add(sample_id)
        self._clear_analysis_task_messages(sample_id)
        key_error = self._key_analysis.error_for_request(sample_id, event.get("request_id"))
        if key_error is not None:
            self._session.sample_analysis_errors[sample_id] = key_error

    def _handle_task_progress(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_progress is not None:
                percent = event.get("percent")
                stage = event.get("stage")
                self._on_stem_generation_progress(
                    sample_id,
                    float(percent) if isinstance(percent, (int, float)) else None,
                    stage if isinstance(stage, str) else None,
                )
                return

            if sample_id not in self._session.stem_generating_sample_ids:
                return

            stage = event.get("stage")
            if isinstance(stage, str):
                self._session.stem_generation_stage[sample_id] = stage

            percent = event.get("percent")
            if isinstance(percent, (int, float)):
                self._session.stem_generation_progress[sample_id] = float(percent)
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        stage = event.get("stage")
        if isinstance(stage, str):
            self._session.sample_analysis_stage[sample_id] = stage

        percent = event.get("percent")
        if isinstance(percent, (int, float)):
            self._session.sample_analysis_progress[sample_id] = float(percent)

    def _handle_task_success(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_success is not None:
                self._on_stem_generation_success(sample_id)
                return

            self._handle_stem_generation_success(sample_id)
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        analysis = event.get("analysis")
        parsed = _parse_sample_analysis(analysis)
        key_error = self._key_analysis.error_for_request(sample_id, event.get("request_id"))
        try:
            if (
                parsed is not None
                and key_error is None
                and self._key_analysis.complete(sample_id, event.get("request_id"), parsed.key)
            ):
                self._mark_project_changed()
        except (RuntimeError, ValueError) as error:
            key_error = f"Analysis key metadata completion failed: {error}"
        if event.get("timing_stale") is not True:
            self._store_sample_analysis(sample_id, analysis)
        self._clear_analysis_task_state(sample_id)
        if key_error is not None:
            self._session.sample_analysis_errors[sample_id] = key_error

    def _handle_stem_generation_success(self, sample_id: int) -> None:
        if sample_id not in self._session.stem_generating_sample_ids:
            return

        # This legacy native event stream has no source ticket captured at admission.
        # Only the Python backend's ticket-bound completion can publish prepared stems.
        self._clear_stem_generation_state(sample_id)
        self._session.stem_generation_errors[sample_id] = (
            "Stem completion rejected because its admission ticket is missing"
        )

    def _handle_task_error(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_error is not None:
                msg = event.get("msg")
                if isinstance(msg, str):
                    self._on_stem_generation_error(sample_id, msg)
                return

            if sample_id not in self._session.stem_generating_sample_ids:
                return

            self._session.stem_generating_sample_ids.discard(sample_id)
            self._session.stem_generation_source_versions.pop(sample_id, None)
            self._session.stem_generation_progress.pop(sample_id, None)
            self._session.stem_generation_stage.pop(sample_id, None)

            msg = event.get("msg")
            if isinstance(msg, str):
                self._session.stem_generation_errors[sample_id] = msg
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        self._session.analyzing_sample_ids.discard(sample_id)
        self._analysis_request_ids.pop(sample_id, None)
        self._key_analysis.cancel(sample_id)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

        msg = event.get("msg")
        if isinstance(msg, str):
            self._session.sample_analysis_errors[sample_id] = msg

    def _store_sample_analysis(self, sample_id: int, analysis: object) -> SampleAnalysis | None:
        parsed = _parse_sample_analysis(analysis)
        if parsed is None:
            return None

        manual = self._project.manual_bpm[sample_id]
        self._restore_legacy_timing_authority(
            sample_id, normalize_bpm(manual if manual is not None else parsed.bpm)
        )
        self._project.sample_analysis[sample_id] = parsed
        self._project.pad_timing_intent[sample_id] = (
            "tap"
            if manual is not None and self._project.pad_timing_intent[sample_id] == "tap"
            else "manual"
            if manual is not None
            else "legacy"
        )
        self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()
        return parsed

    def _restore_legacy_timing_authority(
        self, sample_id: int, bpm: float | None, *, timing_stale: bool = False
    ) -> None:
        """Resume ordinary timing only at a successful or explicitly cleared lifecycle."""
        if not timing_stale and self._audio.pad_timing_intent(sample_id) == "automatic":
            self._audio.set_pad_bpm(sample_id, bpm)

    def _clear_restored_pad(self, sample_id: int) -> None:
        self._cancel_deferred_restore(sample_id)
        self._reset_unloaded_pad_defaults(sample_id)
        self._on_pad_bpm_changed(sample_id)

    def _wants_accepted_restore(self, sample_id: int) -> bool:
        return (
            self._project.pad_timing_intent[sample_id] == "automatic"
            and self._project.manual_bpm[sample_id] is None
        )

    def _begin_accepted_restore(
        self, sample_id: int, *, restored_assignment: bool, timing_stale: bool
    ) -> bool:
        if timing_stale or not restored_assignment or not self._wants_accepted_restore(sample_id):
            return False
        self._clear_analysis_task_state(sample_id)
        self._accepted_restore.begin(sample_id)
        return True

    @staticmethod
    def _normalize_project_path(value: str) -> str:
        cwd = Path.cwd().resolve()

        path = Path(value)
        try:
            abs_path = path if path.is_absolute() else (cwd / path)
            rel = abs_path.resolve().relative_to(cwd)
        except OSError:
            return value
        except ValueError:
            return value

        return rel.as_posix()

    def _parse_cached_sample_path(self, path: str) -> Path | None:
        try:
            return original_asset(path).path.relative_to(Path.cwd())
        except OSError, ValueError:
            return None

    def _schedule_restored_load(self, sample_id: int, rel: Path, *, run_analysis: bool) -> bool:
        reservation = None
        try:
            asset = original_asset(rel)
            self._validate_restored_material_identity(sample_id, asset.material_id)
            reservation = self._assets.reserve()
            options: _RestoredLoadOptions = {
                "run_analysis": run_analysis,
                "replace_assignment": True,
                "source_intent": "restore",
            }
            if self._wants_accepted_restore(sample_id):
                options["restore_automatic"] = True
            sample_rate_hz = self._output_sample_rate_hz()
            if sample_rate_hz is not None:
                resident = saved_resident_loop(
                    self._project, sample_id, sample_rate_hz=sample_rate_hz
                )
                if resident is not None:
                    options["resident_loop_start_s"] = resident.start_seconds
                    options["resident_loop_end_s"] = resident.end_seconds
                    options["resident_key_lock"] = resident.key_lock
            request_id = self._audio.load_sample_async(sample_id, rel.as_posix(), **options)
        except (RuntimeError, ValueError) as error:
            if reservation is not None:
                reservation.close()
            if isinstance(error, RuntimeError) and str(error) == self._COLD_QUEUE_FULL:
                self._defer_restored_load(sample_id, rel)
                return False
            self._cancel_deferred_restore(sample_id)
            self._session.sample_load_errors[sample_id] = str(error)
            return False
        self._deferred_restores.pop(sample_id, None)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.pending_sample_paths[sample_id] = rel.as_posix()
        self._session.loading_sample_ids.add(sample_id)
        self._load_request_ids.pop(sample_id, None)
        self._new_load_sample_ids.discard(sample_id)
        self._record_load_request_id(sample_id, request_id)
        self._record_load_retirement(sample_id, reservation)
        return True

    def _validate_restored_material_identity(self, sample_id: int, material_id: str | None) -> None:
        content = self._project.pad_content[sample_id]
        if content is not None and content.material_id != material_id:
            message = "Saved material identity does not match its original"
            raise ValueError(message)

    def _defer_restored_load(self, sample_id: int, rel: Path) -> None:
        self._deferred_restores[sample_id] = rel
        self._session.pending_sample_paths[sample_id] = rel.as_posix()
        self._session.loading_sample_ids.add(sample_id)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage[sample_id] = "Waiting for cold source admission"
        self._load_request_ids.pop(sample_id, None)
        self._new_load_sample_ids.discard(sample_id)

    def _cancel_deferred_restore(self, sample_id: int) -> None:
        if self._deferred_restores.pop(sample_id, None) is None:
            return
        self._session.pending_sample_paths.pop(sample_id, None)
        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

    def _retry_deferred_restores(self) -> None:
        pending = list(islice(self._deferred_restores.items(), self._RESTORE_ADMISSIONS_PER_POLL))
        for sample_id, rel in pending:
            current_path = self._project.sample_paths[sample_id]
            if current_path is None or self._parse_cached_sample_path(current_path) != rel:
                self._cancel_deferred_restore(sample_id)
                continue
            admitted = self._schedule_restored_load(sample_id, rel, run_analysis=False)
            if not admitted and sample_id in self._deferred_restores:
                break

    def _record_load_request_id(self, sample_id: int, request_id: object) -> None:
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return
        self._load_request_ids[sample_id] = request_id

    def _record_load_retirement(
        self, sample_id: int, reservation: AssetRetirementReservation
    ) -> None:
        key = (sample_id, self._load_request_ids.get(sample_id))
        previous = self._load_retirements.pop(key, None)
        if previous is not None:
            previous.close()
        self._load_retirements[key] = reservation

    def _take_load_retirement(
        self, sample_id: int, event: dict[str, object]
    ) -> AssetRetirementReservation | None:
        value = event.get("request_id")
        request = value if isinstance(value, int) and not isinstance(value, bool) else None
        return self._load_retirements.pop((sample_id, request), None)

    def _record_analysis_request_id(self, sample_id: int, request_id: object) -> None:
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return
        self._analysis_request_ids[sample_id] = request_id

    def _matches_load_request(self, sample_id: int, event: dict[str, object]) -> bool:
        request_id = event.get("request_id")
        incoming = self._incoming_loads.get(sample_id)
        if incoming is not None:
            return (
                isinstance(request_id, int)
                and not isinstance(request_id, bool)
                and request_id == incoming.intent.request_id
            )
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return True
        return self._load_request_ids.get(sample_id) == request_id

    def _matches_analysis_request(self, sample_id: int, event: dict[str, object]) -> bool:
        request_id = event.get("request_id")
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return not self._key_analysis.has_request(sample_id)
        return self._analysis_request_ids.get(sample_id) == request_id
