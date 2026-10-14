"""Own the Python projection of one acknowledged surviving Native Pair.

This module exchanges existing control rows. It never captures a replacement
Native permit, cancels work, calls a callback or grants permission from metadata.
The enclosing arrangement owner retains its reservation and observer fence until
this exact plan has applied and the separate Stem/custody/durable lanes settle.
"""

import threading
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, cast

from pydantic import BaseModel

from flitzis_looper.controller.pad_selection import PreparedPadSelection
from flitzis_looper.copy_snapshot import CONTENT_FIELDS, canonical_snapshot_json
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.pair_snapshot import PairSnapshot, capture_pair_snapshot

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.transport.global_playback import (
        _DeferredStart,
        _PendingGlobalBatch,
    )
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper.ui.context import WaveformEditorActions


_SESSION_CONTAINERS = (
    "active_sample_ids",
    "paused_sample_ids",
    "global_stop_restore_sample_ids",
    "pressed_pads",
    "pad_peak",
    "pad_peak_updated_at",
    "pad_clip_hold_until",
    "pad_playhead_s",
    "pad_playhead_updated_at",
    "loading_sample_ids",
    "pending_sample_paths",
    "sample_load_progress",
    "sample_load_stage",
    "sample_load_errors",
    "analyzing_sample_ids",
    "sample_analysis_progress",
    "sample_analysis_stage",
    "sample_analysis_errors",
)
_SESSION_PLACEMENTS = (
    "file_dialog_pad_id",
    "waveform_editor_pad_id",
    "waveform_pause_hold_pad_id",
    "tap_bpm_pad_id",
    "bpm_lock_anchor_pad_id",
)
_RESIDENCY_CONTAINERS = (
    "_pending",
    "_launched",
    "_effective",
    "_errors",
    "_error_sources",
    "_launch_errors",
    "_confirmed_modes",
)
_TIMING_CONTAINERS = ("_preparations", "_publications", "_waiting_refresh", "_refreshes")
_LOADER_CONTAINERS = (
    "_load_request_ids",
    "_analysis_request_ids",
    "_new_load_sample_ids",
    "_incoming_loads",
    "_deferred_restores",
    "_deferred_restore_content",
    "_material_creation_pending",
)
_RENDER_FIELDS = (
    "_last_pad_id",
    "_last_width_px",
    "_last_start_s",
    "_last_end_s",
    "_last_source_identity",
    "_last_waveform_value",
)


def _store(owner: object, name: str, value: object) -> None:
    """Store a fully prepared image without invoking model validation or hooks."""
    if isinstance(owner, BaseModel):
        # These are independently valid original rows/defaults. Assignment
        # validation would copy owner containers and can fail during exchange.
        owner.__dict__[name] = value
    else:
        setattr(owner, name, value)


@dataclass(frozen=True, slots=True)
class _Placement:
    source: int
    target: int
    swap: bool

    def project(self, sample_id: object) -> object:
        if type(sample_id) is int and sample_id == self.source:
            return self.target
        if type(sample_id) is int and sample_id == self.target:
            return self.source if self.swap else None
        return sample_id

    def contains(self, key: object, *, compound: bool = False) -> bool:
        sample_id = key[0] if compound and isinstance(key, tuple) else key
        return type(sample_id) is int and sample_id in {self.source, self.target}

    def project_slot(self, sample_id: int) -> int | None:
        """Project a strict original control slot without casting opaque owners."""
        if type(sample_id) is not int:
            message = "Pair control placement requires an original integer slot"
            raise TypeError(message)
        return cast("int | None", self.project(sample_id))

    def key(self, key: object, *, compound: bool = False) -> object:
        if compound:
            if not isinstance(key, tuple) or len(key) != 2:
                message = "Pair loader retirement key is not a supported original request key"
                raise TypeError(message)
            projected = self.project(key[0])
            return (projected, key[1]) if projected is not None else None
        return self.project(key)


@dataclass(frozen=True, slots=True)
class _Container:
    owner: object
    name: str
    placement: _Placement
    before: object
    after: object
    compound: bool = False
    keys: frozenset[object] | None = None

    @classmethod
    def capture(
        cls,
        owner: object,
        name: str,
        placement: _Placement,
        *,
        empty_default: object = None,
        compound: bool = False,
        keys: frozenset[object] | None = None,
    ) -> _Container:
        current = getattr(owner, name)
        if isinstance(current, dict):
            before_dict = {
                key: row
                for key, row in current.items()
                if (key in keys if keys is not None else placement.contains(key, compound=compound))
            }
            after_dict = {
                projected: row
                for key, row in before_dict.items()
                if (projected := placement.key(key, compound=compound)) is not None
            }
            return cls(owner, name, placement, before_dict, after_dict, compound, keys)
        if isinstance(current, set):
            before_set = current.intersection((placement.source, placement.target))
            after_set = {
                projected
                for sample_id in before_set
                if (projected := placement.project(sample_id)) is not None
            }
            return cls(owner, name, placement, before_set, after_set, compound, keys)
        if isinstance(current, list) and not compound:
            before_list = (current[placement.source], current[placement.target])
            after_list = (before_list[1] if placement.swap else empty_default, before_list[0])
            return cls(owner, name, placement, before_list, after_list, compound, keys)
        message = f"Pair control table {name} has an unsupported container"
        raise TypeError(message)

    def _contains(self, key: object) -> bool:
        if self.keys is None:
            return self.placement.contains(key, compound=self.compound)
        return key in self.keys or (isinstance(self.after, dict) and key in self.after)

    def _selected(self, current: object) -> object:
        if isinstance(current, dict) and isinstance(self.before, dict):
            return {key: row for key, row in current.items() if self._contains(key)}
        if isinstance(current, set) and isinstance(self.before, set):
            return current.intersection((self.placement.source, self.placement.target))
        if isinstance(current, list) and isinstance(self.before, tuple):
            return (current[self.placement.source], current[self.placement.target])
        message = f"Pair control table {self.name} changed its container type"
        raise RuntimeError(message)

    @staticmethod
    def _matches(actual: object, expected: object) -> bool:
        if isinstance(actual, dict) and isinstance(expected, dict):
            return actual.keys() == expected.keys() and all(
                actual[key] is row for key, row in expected.items()
            )
        if isinstance(actual, tuple) and isinstance(expected, tuple):
            return all(left is right for left, right in zip(actual, expected, strict=True))
        return actual == expected

    def merged(self) -> object:
        """Merge either own image with newest unrelated entries, without swapping twice."""
        current = getattr(self.owner, self.name)
        selected = self._selected(current)
        if not self._matches(selected, self.before) and not self._matches(selected, self.after):
            message = f"Pair control participants in {self.name} changed outside the owning mirror"
            raise RuntimeError(message)
        if isinstance(current, dict) and isinstance(self.after, dict):
            merged_dict = {key: row for key, row in current.items() if not self._contains(key)}
            merged_dict.update(self.after)
            return merged_dict
        if isinstance(current, set) and isinstance(self.after, set):
            return (current - {self.placement.source, self.placement.target}) | self.after
        if isinstance(current, list) and isinstance(self.after, tuple):
            merged_list = current.copy()
            merged_list[self.placement.source], merged_list[self.placement.target] = self.after
            return merged_list
        message = f"Pair control table {self.name} changed its container type"
        raise RuntimeError(message)


@dataclass(frozen=True, slots=True)
class _Attribute:
    owner: object
    name: str
    before: object
    after: object
    retain_newer: bool = False

    def merged(self) -> object:
        current = getattr(self.owner, self.name)
        if current is self.before or current is self.after:
            return self.after
        if self.retain_newer:
            return current
        message = f"Pair control field {self.name} changed outside the owning mirror"
        raise RuntimeError(message)


def _pending_global_image(
    current: _PendingGlobalBatch | None, placement: _Placement
) -> _PendingGlobalBatch | None:
    if current is None:
        return None
    return replace(
        current,
        restore_ids=frozenset(
            projected
            for sample_id in current.restore_ids
            if (projected := placement.project_slot(sample_id)) is not None
        ),
        start_ids=frozenset(
            projected
            for sample_id in current.start_ids
            if (projected := placement.project_slot(sample_id)) is not None
        ),
    )


def _deferred_global_image(
    current: _DeferredStart | None, placement: _Placement
) -> _DeferredStart | None:
    if current is None:
        return None
    sources = tuple(
        (
            projected,
            (placement.project(signature[0]), *signature[1:]),
            accepted,
        )
        for sample_id, signature, accepted in current.sources
        if (projected := placement.project_slot(sample_id)) is not None
    )
    return replace(current, sources=sources)


def _references(
    controller: AppController, waveform_views: tuple[WaveformEditorActions, ...]
) -> tuple[tuple[object, str, object], ...]:
    """Capture the actual component/model owners, never their equal replacements."""
    references: list[tuple[object, str, object]] = []
    components: list[object] = [controller]
    for name in ("transport", "accepted_timing", "loader", "metering"):
        component = getattr(controller, name)
        references.append((controller, name, component))
        components.append(component)
    transport = controller.transport
    for name in ("residency", "playback", "waveform", "bpm", "loop", "pad", "global_params"):
        component = getattr(transport, name)
        references.append((transport, name, component))
        components.append(component)
    for owner, name in (
        (transport.playback, "_global_playback"),
        (controller.loader, "_accepted_restore"),
        (controller.loader, "_key_analysis"),
    ):
        component = getattr(owner, name)
        references.append((owner, name, component))
        components.append(component)
    for component in components:
        references.extend(
            (component, name, getattr(component, name))
            for name in ("_project", "_session", "_audio", "_transport", "_loop", "_bpm")
            if hasattr(component, name)
        )
    references.extend((view, "_controller", controller) for view in waveform_views)
    return tuple(references)


class PreparedPairControlMirror:
    """Retain one before/own-after plan through all interrupted Python exchanges.

    Preparation and apply belong to the original Controller thread. The trusted
    enclosing owner supplies an opaque actual-own-ACK validator, defers unresolved
    participant observers, and owns separately settled Stem and asset custody.
    """

    def __init__(
        self,
        controller: AppController,
        snapshot: PairSnapshot,
        validate_acknowledgement: Callable[[object], bool],
        waveform_views: tuple[WaveformEditorActions, ...],
    ) -> None:
        captured = snapshot.projected()
        if (
            capture_pair_snapshot(
                controller._project,
                captured.source_id,
                captured.target_id,
                operation=captured.operation,
            )
            != snapshot
        ):
            message = "Pair control preparation differs from the complete original musical seal"
            raise RuntimeError(message)
        self._controller = controller
        self._placement = _Placement(
            captured.source_id, captured.target_id, captured.operation == "swap"
        )
        self._validate_acknowledgement = validate_acknowledgement
        self._control_thread = threading.get_ident()
        self._references = _references(controller, waveform_views)
        self._acknowledgement: object | None = None
        self._applied = False
        self._containers: list[_Container] = []
        self._attributes: list[_Attribute] = []
        self._refresh_signatures: dict[int, _Attribute] = {}
        self._render_invalidations: dict[int, tuple[_Attribute, ...]] = {}
        self._waveform_views = waveform_views
        self._selection = PreparedPadSelection(controller._project, controller._session, snapshot)
        self._prepare_models()
        self._prepare_controls()
        self._prepare_global()
        self._prepare_views()
        self._anchor_before = controller._session.bpm_lock_anchor_pad_id
        self._anchor_after = self._placement.project(self._anchor_before)
        self._prepare_refresh_signatures(project_anchor=True)

    @property
    def applied(self) -> bool:
        """Whether all stores accepted this exact owning acknowledgement once."""
        return self._applied

    @property
    def projected_entries(self) -> tuple[tuple[int, StemCacheEntry | None], ...]:
        """Return exact projected StemEntry owners without claiming availability."""
        table = next(table for table in self._containers if table.name == "stem_cache")
        after = cast("tuple[StemCacheEntry | None, StemCacheEntry | None]", table.after)
        return (
            (self._placement.source, after[0]),
            (self._placement.target, after[1]),
        )

    def projected_project(self) -> ProjectState:
        """Build a detached valid candidate from latest unrelated and own Pair rows.

        This musical image grants no Native authority. It also remains available
        after an interrupted mirror has exchanged only some Project containers;
        the durable coordinator must reuse this plan instead of applying Swap to
        a partly exchanged image. Exact Stem owners remain in projected_entries.
        """
        if threading.get_ident() != self._control_thread:
            message = "Pair control candidate belongs to its original Controller thread"
            raise RuntimeError(message)
        project = self._controller._project
        tables = tuple(table for table in self._containers if table.owner is project)
        if len(tables) != len(CONTENT_FIELDS):
            message = "Pair control Project owner changed after preparation"
            raise RuntimeError(message)
        images = tuple((table.name, table.merged()) for table in tables)
        candidate = project.model_copy(deep=True)
        for name, image in images:
            _store(candidate, name, image)
        _store(candidate, "selected_pad", self._selection.projected_pad())
        data = candidate.model_dump(mode="json", exclude_computed_fields=True, warnings="error")
        return ProjectState.model_validate_json(canonical_snapshot_json(data), strict=True)

    def _add(self, owner: object, names: Iterable[str], *, defaults: object | None = None) -> None:
        for name in names:
            empty = None
            if defaults is not None and isinstance(getattr(owner, name), list):
                empty = getattr(defaults, name)[self._placement.source]
            self._containers.append(
                _Container.capture(owner, name, self._placement, empty_default=empty)
            )

    def _prepare_models(self) -> None:
        controller = self._controller
        self._add(
            controller._project, (name for _, name in CONTENT_FIELDS), defaults=ProjectState()
        )
        self._add(controller._session, _SESSION_CONTAINERS, defaults=SessionState())
        for name in _SESSION_PLACEMENTS:
            before = getattr(controller._session, name)
            self._attributes.append(
                _Attribute(
                    controller._session,
                    name,
                    before,
                    self._placement.project(before),
                    retain_newer=True,
                )
            )

    def _prepare_controls(self) -> None:
        controller = self._controller
        self._add(controller.transport.residency, _RESIDENCY_CONTAINERS)
        self._add(controller.accepted_timing, _TIMING_CONTAINERS)
        self._add(controller.loader, _LOADER_CONTAINERS)
        self._containers.append(
            _Container.capture(
                controller.loader,
                "_load_retirements",
                self._placement,
                compound=True,
                keys=frozenset(
                    (sample_id, request_id)
                    for sample_id in (self._placement.source, self._placement.target)
                    if (self._placement.swap or sample_id == self._placement.source)
                    and (request_id := controller.loader._load_request_ids.get(sample_id))
                    is not None
                ),
            )
        )
        self._add(controller.loader._accepted_restore, ("_pending",))
        self._add(controller.loader._key_analysis, ("_requests", "_errors"))
        self._add(controller.transport.waveform, ("_readiness",))
        self._add(controller.metering, ("_active_peak_sample_ids",))

    def _prepare_global(self) -> None:
        owner = self._controller.transport.playback._global_playback
        for name, prepare in (
            ("_pending", _pending_global_image),
            ("_deferred", _deferred_global_image),
        ):
            before = getattr(owner, name)
            self._attributes.append(
                _Attribute(owner, name, before, prepare(before, self._placement), retain_newer=True)
            )

    def _prepare_views(self) -> None:
        for view in self._waveform_views:
            if view._controller is not self._controller:
                message = "Pair waveform view belongs to a different Controller owner"
                raise RuntimeError(message)
            self._add(view, ("_pad_view_ranges", "_pad_view_sources"))

    def _prepare_render_invalidation(self) -> list[_Attribute]:
        attributes: list[_Attribute] = []
        for view in self._waveform_views:
            retained = self._render_invalidations.get(id(view))
            if retained is None and self._placement.contains(view._last_pad_id):
                retained = tuple(
                    _Attribute(view, name, getattr(view, name), None, retain_newer=True)
                    for name in _RENDER_FIELDS
                )
                self._render_invalidations[id(view)] = retained
            if retained is not None and (
                self._placement.contains(view._last_pad_id) or view._last_pad_id is None
            ):
                attributes.extend(retained)
        return attributes

    def _prepare_refresh_signatures(self, *, project_anchor: bool) -> None:
        timing = self._controller.accepted_timing
        requests = tuple(timing._waiting_refresh.values()) + tuple(
            refresh.request for refresh in timing._refreshes.values()
        )
        for request in requests:
            if id(request) in self._refresh_signatures:
                continue
            before = request.control_signature
            if not isinstance(before, tuple) or len(before) != 12:
                message = "Pair accepted refresh has an unsupported control signature"
                raise TypeError(message)
            after = before
            if project_anchor and before[-1] == self._anchor_before:
                after = (*before[:-1], self._anchor_after)
            self._refresh_signatures[id(request)] = _Attribute(
                request, "control_signature", before, after
            )

    def apply(self, acknowledgement: object) -> None:
        """Validate and prepare all exchanges, then store one complete owning image.

        A retry reads the newest unrelated entries and recognizes every retained
        own-after store. Only the identical actual ACK can continue an interrupted
        attempt; applying an already completed plan is an identity-based no-op.
        """
        if threading.get_ident() != self._control_thread:
            message = "Pair control mirror must apply on its original Controller thread"
            raise RuntimeError(message)
        if self._acknowledgement is not None and acknowledgement is not self._acknowledgement:
            message = "Pair control mirror cannot accept another transaction acknowledgement"
            raise RuntimeError(message)
        if self._applied:
            return
        if acknowledgement is None or self._validate_acknowledgement(acknowledgement) is not True:
            message = "Pair control mirror has no matching opaque actual own Native ACK"
            raise RuntimeError(message)
        if any(getattr(owner, name) is not expected for owner, name, expected in self._references):
            message = "Pair control component or model ownership changed after preparation"
            raise RuntimeError(message)
        anchor = self._controller._session.bpm_lock_anchor_pad_id
        self._prepare_refresh_signatures(project_anchor=anchor == self._anchor_before)
        containers = tuple((table.owner, table.name, table.merged()) for table in self._containers)
        attributes = tuple(
            (attribute.owner, attribute.name, attribute.merged())
            for attribute in (
                *self._attributes,
                *self._refresh_signatures.values(),
                *self._prepare_render_invalidation(),
            )
        )
        exchanges = (
            *containers,
            *attributes,
            (self._controller._project, "selected_pad", self._selection.projected_pad()),
        )
        # No allocation, callback, validation, Native action or cancellation
        # occurs between these fully prepared exchanges. Retain this exact ACK
        # before the first store so every interrupted attempt keeps one owner.
        self._acknowledgement = acknowledgement
        for owner, name, image in exchanges:
            _store(owner, name, image)
        self._applied = True


def prepare_pair_control_mirror(
    controller: AppController,
    snapshot: PairSnapshot,
    *,
    validate_acknowledgement: Callable[[object], bool],
    waveform_views: Iterable[WaveformEditorActions] = (),
) -> PreparedPairControlMirror:
    """Prepare complete Python rows for one separately owned Native Move or Swap.

    The caller supplies registered UiContext.ui.waveform owners and the validator
    of its original actual Native acknowledgement. Historical JSON, slot ids and
    equal acknowledgement metadata never authorize this helper's exchanges.
    """
    views = tuple(waveform_views)
    if len({id(view) for view in views}) != len(views):
        message = "Pair waveform view registration contains duplicate owners"
        raise ValueError(message)
    return PreparedPairControlMirror(
        controller, PairSnapshot.model_validate(snapshot), validate_acknowledgement, views
    )
