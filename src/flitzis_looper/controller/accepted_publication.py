"""Explicit native acceptance and acknowledged derived-control completion."""

import struct
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.accepted_restore import _matches_adopted_request
from flitzis_looper.controller.current_binding import (
    _same_accepted_timing,
    capture_current_pad_binding,
)
from flitzis_looper.controller.validation import ensure_finite
from flitzis_looper.models import validate_sample_id

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.current_timing import CurrentPadTiming
    from flitzis_looper.controller.transport import TransportController
    from flitzis_looper_audio import AcceptedTimingRefreshTicket, ConstantTimingTicket


@dataclass(frozen=True)
class ExplicitTimingAssessment:
    """Independent caller assertions, never inferred from a detector or fit."""

    hypotheses_json: str
    origin_seconds: float
    origin_provenance: str
    acceptance_policy_version: str
    acceptance_provenance: str


@dataclass
class _Publication:
    future: Future[None]
    ticket: ConstantTimingTicket
    path: str
    request_metadata: dict[str, object]


@dataclass
class _RefreshRequest:
    timing: CurrentPadTiming
    control_signature: tuple[object, ...]
    callbacks: tuple[Callable[[int], None], ...]
    admission_failures: int = 0


@dataclass(frozen=True)
class _Refresh:
    request: _RefreshRequest
    ticket: AcceptedTimingRefreshTicket
    grid_offset_samples: int
    master_period_seconds: float | None


def _scalar_signature(value: object) -> object:
    return struct.pack("!d", value) if isinstance(value, float) else value


class AcceptedTimingController:
    """Own only explicit pending operations; native audio remains timing authority.

    One worker prepares complete source evidence and explicit assessments. The
    control poll observes publication, then admits one guarded native loop/master
    effect. A second genuine acknowledgement permits saved/session projections.
    No normal analysis or UI polling initiates automatic acceptance.
    """

    _MAX_ADMISSION_FAILURES = 3

    def __init__(self, transport: TransportController) -> None:
        self._transport = transport
        self._project = transport._project
        self._session = transport._session
        self._audio = transport._audio
        self._worker: ThreadPoolExecutor | None = None
        self._preparations: dict[int, Future[ConstantTimingTicket]] = {}
        self._publications: dict[int, _Publication] = {}
        self._waiting_refresh: dict[int, _RefreshRequest] = {}
        self._refreshes: dict[int, _Refresh] = {}

    def _executor(self) -> ThreadPoolExecutor:
        if self._worker is None:
            self._worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix="timing-accept")
        return self._worker

    def prepare(
        self,
        sample_id: int,
        timing_error_halfwidth_seconds: float,
        timing_error_provenance: str,
        *,
        intent: Literal["automatic"],
    ) -> Future[ConstantTimingTicket]:
        """Explicitly choose Automatic and prepare actual captured source off-thread.

        The mandatory intent declares a performer choice. It does not accept a
        detector result: publication still needs a separate independent assessment.
        Native intent admission precedes durable intent changes and source capture.
        """
        validate_sample_id(sample_id)
        if intent != "automatic":
            msg = "explicit preparation requires Automatic intent"
            raise ValueError(msg)
        if self._project.sample_paths[sample_id] is None:
            msg = "explicit preparation requires a loaded pad"
            raise ValueError(msg)
        ensure_finite(timing_error_halfwidth_seconds)
        if timing_error_halfwidth_seconds < 0.0 or not timing_error_provenance.strip():
            msg = "explicit preparation requires a valid independent timing-error assertion"
            raise ValueError(msg)
        if not self._automatic(sample_id):
            self._audio.set_pad_timing_intent(sample_id, intent)
        changed = (
            self._project.manual_bpm[sample_id] is not None
            or self._project.pad_timing_intent[sample_id] != intent
        )
        self._project.manual_bpm[sample_id] = None
        self._project.pad_timing_intent[sample_id] = intent
        if changed:
            self._transport._mark_project_changed()
        binding = self._audio.current_input_runtime_pad_binding(sample_id)
        if binding is None:
            msg = "actual loaded source is unavailable for explicit preparation"
            raise RuntimeError(msg)
        captured = self._audio.capture_current_constant_timing(
            binding, timing_error_halfwidth_seconds, timing_error_provenance
        )
        future = self._executor().submit(self._audio.prepare_captured_constant_timing, captured)
        previous = self._preparations.pop(sample_id, None)
        if previous is not None:
            previous.cancel()
        # Preparation does not replace accepted timing. Keep genuine earlier
        # publication/derived completion observers until native guards settle them.
        self._preparations[sample_id] = future
        return future

    def publish(
        self, sample_id: int, ticket: ConstantTimingTicket, assessment: ExplicitTimingAssessment
    ) -> Future[None]:
        """Publish an explicit assessment and own its actual completion refresh."""
        validate_sample_id(sample_id)
        if (
            sample_id in self._publications
            or sample_id in self._waiting_refresh
            or sample_id in self._refreshes
        ):
            msg = "explicit timing completion is already pending for this pad"
            raise RuntimeError(msg)
        path = self._project.sample_paths[sample_id]
        captured = ticket.metadata()
        if path is None or captured.get("pad_id") != sample_id:
            msg = "explicit timing ticket does not identify this loaded pad"
            raise ValueError(msg)
        if not self._automatic(sample_id):
            msg = "explicit timing publication requires current Automatic intent"
            raise RuntimeError(msg)
        future = self._executor().submit(
            self._audio.publish_constant_timing,
            ticket,
            assessment.hypotheses_json,
            assessment.origin_seconds,
            assessment.origin_provenance,
            assessment.acceptance_policy_version,
            assessment.acceptance_provenance,
        )
        preparation = self._preparations.pop(sample_id, None)
        if preparation is not None:
            preparation.cancel()
        self._publications[sample_id] = _Publication(
            future, ticket, path, {"request_id": captured.get("request_id")}
        )
        return future

    def refresh_current(
        self, sample_id: int, *, on_refreshed: Callable[[int], None] | None = None
    ) -> None:
        """Request derived refresh from genuine current accepted native ownership."""
        validate_sample_id(sample_id)
        timing = self._transport.bpm.current_timing(sample_id)
        if not self._automatic(sample_id) or timing is None or timing.accepted_identity is None:
            return
        signature = self._control_signature(sample_id)
        active = self._refreshes.get(sample_id)
        existing = active.request if active is not None else self._waiting_refresh.get(sample_id)
        if (
            existing is not None
            and existing.control_signature == signature
            and _same_accepted_timing(existing.timing, timing)
        ):
            if on_refreshed is not None and on_refreshed not in existing.callbacks:
                existing.callbacks += (on_refreshed,)
            return
        request = _RefreshRequest(timing, signature, (on_refreshed,) if on_refreshed else ())
        self._refreshes.pop(sample_id, None)
        self._waiting_refresh[sample_id] = request

    def poll(self) -> None:
        """Observe owned operations and acknowledgements, never infer new acceptance."""
        self._poll_publications()
        self._poll_refreshes()
        for sample_id, request in list(self._waiting_refresh.items()):
            if not self._request_is_current(sample_id, request):
                self._waiting_refresh.pop(sample_id)
                continue
            try:
                self._admit_refresh(sample_id, request)
            except RuntimeError as error:
                # Retry only this still-current explicit request, with a finite bound.
                request.admission_failures += 1
                if request.admission_failures >= self._MAX_ADMISSION_FAILURES:
                    self._waiting_refresh.pop(sample_id)
                self._session.sample_analysis_errors[sample_id] = str(error)
            except ValueError as error:
                self._waiting_refresh.pop(sample_id)
                self._session.sample_analysis_errors[sample_id] = str(error)

    def _poll_publications(self) -> None:
        for sample_id, publication in list(self._publications.items()):
            if self._project.sample_paths[sample_id] != publication.path or not self._automatic(
                sample_id
            ):
                self.cancel(sample_id)
                continue
            if not publication.future.done():
                continue
            if publication.future.cancelled():
                self._publications.pop(sample_id)
                continue
            try:
                publication.future.result()
                status = publication.ticket.publication_status()
                if status == "pending":
                    continue
                self._publications.pop(sample_id)
                if status != "accepted":
                    self._session.sample_analysis_errors[sample_id] = (
                        "Explicit timing publication was rejected"
                    )
                    continue
                if _matches_adopted_request(
                    self._audio.current_constant_timing(sample_id),
                    publication.request_metadata,
                    publication.ticket.accepted_metadata(),
                ):
                    self.refresh_current(sample_id)
            except (RuntimeError, ValueError) as error:
                self._publications.pop(sample_id, None)
                self._session.sample_analysis_errors[sample_id] = str(error)

    def _admit_refresh(self, sample_id: int, request: _RefreshRequest) -> None:
        snapshot = capture_current_pad_binding(self._audio, sample_id, timing=request.timing)
        if snapshot is None or snapshot.accepted_timing is None:
            self._waiting_refresh.pop(sample_id)
            return
        start_s, end_s = self._transport.loop._effective_pad_loop_region(
            sample_id, timing=snapshot.accepted_timing
        )
        master_period = (
            snapshot.accepted_timing.period_seconds / self._project.speed
            if self._project.bpm_lock and self._session.bpm_lock_anchor_pad_id == sample_id
            else None
        )
        grid_offset = self._transport.loop._clamp_grid_offset_samples(
            sample_id,
            self._project.pad_grid_offset_samples[sample_id],
            timing=snapshot.accepted_timing,
        )
        ticket = self._audio.refresh_current_constant_timing(
            snapshot.binding, start_s, end_s, master_period
        )
        self._refreshes[sample_id] = _Refresh(request, ticket, grid_offset, master_period)
        self._waiting_refresh.pop(sample_id)

    def _poll_refreshes(self) -> None:
        for sample_id, refresh in list(self._refreshes.items()):
            if not self._request_is_current(sample_id, refresh.request):
                self._refreshes.pop(sample_id)
                continue
            status = refresh.ticket.publication_status()
            if status == "pending":
                continue
            self._refreshes.pop(sample_id)
            if status != "accepted":
                self._session.sample_analysis_errors[sample_id] = (
                    "Accepted timing derived refresh was rejected"
                )
                continue
            if not refresh.ticket.is_current():
                continue
            self._project.pad_grid_offset_samples[sample_id] = refresh.grid_offset_samples
            if refresh.master_period_seconds is not None:
                timing = refresh.request.timing
                self._session.master_period_seconds = refresh.master_period_seconds
                self._session.master_bpm = 60.0 / refresh.master_period_seconds
                self._session.bpm_lock_anchor_bpm = timing.bpm
                self._session.bpm_lock_anchor_revision = timing.accepted_revision
            self._session.sample_analysis_errors.pop(sample_id, None)
            self._transport._mark_project_changed()
            for callback in refresh.request.callbacks:
                callback(sample_id)

    def _automatic(self, sample_id: int) -> bool:
        return (
            self._project.sample_paths[sample_id] is not None
            and self._project.manual_bpm[sample_id] is None
            and self._project.pad_timing_intent[sample_id] == "automatic"
            and self._audio.pad_timing_intent(sample_id) == "automatic"
        )

    def _request_is_current(self, sample_id: int, request: _RefreshRequest) -> bool:
        if not self._automatic(sample_id):
            return False
        timing = self._transport.bpm.current_timing(sample_id)
        return (
            self._control_signature(sample_id) == request.control_signature
            and timing is not None
            and _same_accepted_timing(timing, request.timing)
        )

    def _control_signature(self, sample_id: int) -> tuple[object, ...]:
        return tuple(
            _scalar_signature(value)
            for value in (
                self._project.sample_paths[sample_id],
                self._project.manual_bpm[sample_id],
                self._project.pad_timing_intent[sample_id],
                self._project.pad_loop_start_s[sample_id],
                self._project.pad_loop_end_s[sample_id],
                self._project.pad_loop_auto[sample_id],
                self._project.pad_loop_bars[sample_id],
                self._project.pad_grid_anchor_s[sample_id],
                self._project.pad_grid_offset_samples[sample_id],
                self._project.speed,
                self._project.bpm_lock,
                self._session.bpm_lock_anchor_pad_id,
            )
        )

    def cancel(self, sample_id: int) -> None:
        """Retire control observers; actual native guards reject superseded work."""
        preparation = self._preparations.pop(sample_id, None)
        if preparation is not None:
            preparation.cancel()
        publication = self._publications.pop(sample_id, None)
        if publication is not None:
            publication.future.cancel()
        self._waiting_refresh.pop(sample_id, None)
        self._refreshes.pop(sample_id, None)

    def shut_down(self) -> None:
        """Drain evidence and assessment workers before native stream teardown."""
        for sample_id in set(self._preparations) | set(self._publications):
            self.cancel(sample_id)
        self._waiting_refresh.clear()
        self._refreshes.clear()
        if self._worker is not None:
            self._worker.shutdown(wait=True, cancel_futures=True)
            self._worker = None
