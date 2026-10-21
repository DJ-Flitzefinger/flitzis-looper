"""Loader-owned compact map selection through the existing Native job and asset paths."""

import json
import math
import struct
from dataclasses import dataclass
from time import monotonic
from typing import TYPE_CHECKING, Literal, cast

from flitzis_looper.models import validate_sample_id
from flitzis_looper.source_beatmap_intent import (
    PadSourceBeatmapIntent,
    SourceBeatmapReference,
    next_source_beatmap_intent,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        AssignmentImage,
        AssignmentPublicationPlan,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import (
        AudioEngine,
        LoaderEventDelivery,
        OfflineAnalysisJob,
        SourceBeatmap,
        SourceBeatmapArtifact,
        SourceBeatmapRawCandidate,
        SourceBeatmapRawInfo,
        SourceBeatmapRawInspection,
    )

    type SelectionCommit = Callable[
        [int, PadSourceBeatmapIntent | None, PadSourceBeatmapIntent, Callable[[], bool]], None
    ]


@dataclass
class _MapJob:
    content: str
    intent: PadSourceBeatmapIntent
    job: OfflineAnalysisJob
    restoring: bool
    source_context: object
    reservation: AssetRetirementReservation | None = None


@dataclass
class _EffectiveMap:
    reference: SourceBeatmapReference
    artifact: SourceBeatmapArtifact
    delivery: LoaderEventDelivery


@dataclass
class _Publication:
    content: str
    image: AssignmentImage | None
    plan: AssignmentPublicationPlan | None
    reservation: AssetRetirementReservation


@dataclass(frozen=True, slots=True)
class RawSourceBeatmapState:
    """Display-only state for the single transient, unverified review attempt."""

    attempt: int
    status: Literal["none", "queued", "running", "cancel_retiring", "failed", "unreviewed"]
    occupied: bool
    revision: str | None = None
    info: SourceBeatmapRawInfo | None = None
    error: str | None = None


@dataclass(slots=True)
class _RawReview:
    content: str
    source_generation: int
    attempt: int
    job: OfflineAnalysisJob
    status: Literal["queued", "running", "cancel_retiring", "failed", "unreviewed"] = "queued"
    candidate: SourceBeatmapRawCandidate | None = None
    delivery: LoaderEventDelivery | None = None
    revision: str | None = None
    failure: str | None = None


class SourceBeatmapSelection:
    """Control intent only; genuine Native completions qualify every effective map.

    There is no Python worker or geometry cache. Pending jobs retain their original
    Native cancellation/completion owner. Saved references are ordinary compact
    historical data until fresh source verification succeeds.
    """

    def __init__(
        self,
        project: ProjectState,
        audio: AudioEngine,
        assets: ProjectAssetLifecycle,
        changed: Callable[[], None],
    ) -> None:
        self.project = project
        self.audio = audio
        self.assets = assets
        self.changed = changed
        self._commit: SelectionCommit | None = None
        self._blocked: Callable[[], bool] = lambda: False
        self._edit_guard: Callable[[int], None] = lambda _sample_id: None
        self._source_generation: Callable[[int], int | None] = lambda _sample_id: None
        self._pending: dict[str, _MapJob] = {}
        self._effective: dict[str, _EffectiveMap] = {}
        self._publications: list[_Publication] = []
        self._failed: dict[str, tuple[PadSourceBeatmapIntent, object]] = {}
        self._retry_after: dict[str, float] = {}
        self._errors: dict[str, str] = {}
        self._raw: _RawReview | None = None
        self._raw_attempt = 0

    def analyze_original(self, sample_id: int, *, expected_attempt: int) -> None:
        """Acquire complete original detections without entering accepted-map admission."""
        content = self._guard(sample_id)
        if type(expected_attempt) is not int or expected_attempt != self._raw_attempt:
            message = "The raw analysis attempt changed. Reopen Source Map before analyzing."
            raise ValueError(message)
        if content in self._pending:
            message = "Accepted source map preparation is pending for this content."
            raise RuntimeError(message)
        if self._raw is not None and self._raw.status != "failed":
            message = "The transient raw review slot is occupied or retiring."
            raise RuntimeError(message)
        generation = self._source_generation(sample_id)
        if type(generation) is not int or not 0 < generation < 1 << 64:
            message = "Raw analysis requires a current Native source generation."
            raise ValueError(message)
        job = self.audio.begin_source_beatmap_raw_async(
            sample_id, expected_source_generation=generation
        )
        self._raw_attempt += 1
        self._raw = _RawReview(content, generation, self._raw_attempt, job)

    def raw_state(self, sample_id: int) -> RawSourceBeatmapState:
        """Read raw readiness separately from accepted selection and playback authority."""
        validate_sample_id(sample_id)
        raw = self._raw
        if raw is None or self._content(sample_id) != raw.content:
            return RawSourceBeatmapState(
                self._raw_attempt, "none", raw is not None and raw.status != "failed"
            )
        info = self._raw_info(raw)
        return RawSourceBeatmapState(
            raw.attempt, raw.status, raw.status != "failed", raw.revision, info, raw.failure
        )

    def _raw_info(self, raw: _RawReview) -> SourceBeatmapRawInfo | None:
        if raw.candidate is None:
            return None
        try:
            info = raw.candidate.info()
        except (OSError, RuntimeError, ValueError) as error:
            self._cancel_raw(str(error))
            return None
        if info["status"] in {"stale", "cancelled"}:
            self._cancel_raw("The original source for raw analysis is no longer current.")
            return None
        if info["status"] == "ready":
            raw.revision = info["raw_revision"]
        return info

    def _raw_target(
        self, sample_id: int, expected_attempt: int, expected_revision: str | None
    ) -> _RawReview:
        validate_sample_id(sample_id)
        self._edit_guard(sample_id)
        raw = self._raw
        if raw is None:
            message = "The raw analysis source or attempt changed. Reopen Source Map."
            raise ValueError(message)
        if (
            type(expected_attempt) is not int
            or raw.attempt != expected_attempt
            or self._content(sample_id) != raw.content
            or self._source_generation(sample_id) != raw.source_generation
            or raw.revision != expected_revision
        ):
            message = "The raw analysis source or attempt changed. Reopen Source Map."
            raise ValueError(message)
        if raw.job.is_cancelled():
            self._cancel_raw()
            message = "The original Native raw analysis was cancelled."
            raise ValueError(message)
        return raw

    def inspect_raw(
        self,
        sample_id: int,
        *,
        expected_attempt: int,
        expected_revision: str,
        start_index: int,
        max_events: int = 128,
    ) -> SourceBeatmapRawInspection:
        """Borrow one bounded current Native page, never musical map geometry."""
        raw = self._raw_target(sample_id, expected_attempt, expected_revision)
        info = self._raw_info(raw)
        if raw.candidate is None or info is None or raw.status != "unreviewed":
            message = "The unreviewed raw candidate is unavailable."
            raise ValueError(message)
        result = raw.candidate.inspect(start_index, max_events)
        self._raw_target(sample_id, expected_attempt, expected_revision)
        if result["status"] in {"stale", "cancelled"}:
            self._cancel_raw("The original source for raw analysis is no longer current.")
        return result

    def discard_raw(
        self, sample_id: int, *, expected_attempt: int, expected_revision: str | None
    ) -> None:
        """Cancel only this original attempt and retain its actual retirement receipt."""
        raw = self._raw_target(sample_id, expected_attempt, expected_revision)
        if raw.candidate is not None:
            self._raw_info(raw)
        self._cancel_raw()

    def review_raw(
        self,
        sample_id: int,
        review_intent: list[tuple[int, float]],
        *,
        expected_attempt: int,
        expected_revision: str,
    ) -> None:
        """Submit explicit quarter labels to the original-candidate Native builder.

        These pairs are control intent only. Native resolves every source position
        from the original capture and creates the sole accepted geometry.
        """
        raw = self._raw_target(sample_id, expected_attempt, expected_revision)
        content = self._guard(sample_id)
        info = self._raw_info(raw)
        if (
            info is None
            or info["status"] != "ready"
            or raw.candidate is None
            or raw.status != "unreviewed"
            or info["raw_revision"] != expected_revision
        ):
            message = "The original raw candidate is not ready for interpretation."
            raise ValueError(message)
        encoded = self._encode_raw_review(review_intent, expected_revision, info["event_count"])
        self._raw_target(sample_id, expected_attempt, expected_revision)
        candidate = raw.candidate
        self._admit(
            sample_id,
            content,
            lambda: self.audio.begin_source_beatmap_review_async(
                sample_id,
                candidate,
                encoded,
                expected_source_generation=raw.source_generation,
            ),
        )

    @staticmethod
    def _encode_raw_review(
        review_intent: list[tuple[int, float]], revision: str, event_count: int
    ) -> str:
        if type(review_intent) is not list or not 2 <= len(review_intent) <= 64:
            message = "Enter between 2 and 64 explicit raw event/quarter-coordinate labels."
            raise ValueError(message)
        anchors = []
        previous_index, previous_quarter = -1, -math.inf
        for label in review_intent:
            if type(label) is not tuple or len(label) != 2:
                message = "Each review label needs one raw index and one finite quarter coordinate."
                raise ValueError(message)
            raw_index, quarter = label
            if (
                type(raw_index) is not int
                or not previous_index < raw_index < event_count
                or type(quarter) is not float
                or not math.isfinite(quarter)
                or quarter <= previous_quarter
            ):
                message = "Raw indices and finite quarter coordinates must be strictly increasing."
                raise ValueError(message)
            anchors.append({
                "raw_index": raw_index,
                "quarter_bits": struct.pack(">d", quarter).hex(),
            })
            previous_index, previous_quarter = raw_index, quarter
        encoded = json.dumps(
            {"schema_version": 1, "raw_revision": revision, "anchors": anchors},
            allow_nan=False,
            separators=(",", ":"),
        )
        if len(encoded.encode("utf-8")) > 8192:
            message = "Raw review intent exceeds the 8 KiB limit."
            raise ValueError(message)
        return encoded

    def raw_completed(self, sample_id: int, event: dict[str, object]) -> None:
        """Resolve raw evidence only through this job's original private Native delivery."""
        raw, delivery = self._raw, event.get("_native_event_owner")
        if (
            raw is None
            or raw.status in {"cancel_retiring", "failed"}
            or self._content(sample_id) != raw.content
            or delivery is None
        ):
            return
        candidate = raw.job.resolve_source_beatmap_raw_completion(
            cast("LoaderEventDelivery", delivery)
        )
        if candidate is None:
            return
        generation = self._source_generation(sample_id)
        if generation is None:
            # A current-pad capture may be unreadable during a global placement
            # publication. Absence alone does not prove a different source.
            message = "loader event content placement pending"
            raise RuntimeError(message)
        if generation != raw.source_generation:
            self._cancel_raw("The original source for raw analysis is no longer current.")
            return
        raw.candidate, raw.delivery = candidate, cast("LoaderEventDelivery", delivery)
        raw.status = "unreviewed"
        self._raw_info(raw)

    def _cancel_raw(self, failure: str | None = None) -> None:
        raw = self._raw
        if raw is None:
            return
        raw.candidate, raw.delivery = None, None
        raw.failure = failure
        if raw.status != "cancel_retiring":
            raw.status = "cancel_retiring"
            raw.job.cancel()

    def _poll_raw(self, deferred: frozenset[int]) -> None:
        raw = self._raw
        if raw is None or raw.status == "failed":
            return
        sample_id = self._slot(raw.content)
        if sample_id in deferred:
            return
        if sample_id is None or raw.job.is_cancelled():
            self._cancel_raw(raw.failure)
        status = raw.job.source_beatmap_async_status()
        if status is None:
            return
        if self._settle_raw_status(raw, status, sample_id):
            return
        if raw.status != "unreviewed":
            raw.status = "queued" if status.get("stage") == "queued" else "running"
        elif raw.candidate is not None:
            self._raw_info(raw)

    def _settle_raw_status(
        self, raw: _RawReview, status: dict[str, object], sample_id: int | None
    ) -> bool:
        if status.get("stage") in {"failed", "cancelled"} and raw.status != "cancel_retiring":
            detail = str(status.get("detail") or status["stage"])
            self._cancel_raw(detail if status["stage"] == "failed" else None)
        if raw.status == "cancel_retiring":
            if status.get("terminal") is True and status.get("raw_review_retired") is True:
                if raw.failure is not None and sample_id is not None:
                    raw.status = "failed"
                else:
                    self._raw = None
            return True
        return False

    @property
    def errors(self) -> dict[int, str]:
        """Project content-owned failures onto their current PAD placements."""
        return {
            index: self._errors[row.instance_id]
            for index, row in enumerate(self.project.pad_content)
            if row is not None and row.instance_id in self._errors
        }

    def bind(
        self,
        commit: SelectionCommit,
        *,
        blocked: Callable[[], bool],
        edit_guard: Callable[[int], None],
        source_generation: Callable[[int], int | None] | None = None,
    ) -> None:
        self._commit = commit
        self._blocked = blocked
        self._edit_guard = edit_guard
        if source_generation is not None:
            self._source_generation = source_generation

    def _content(self, sample_id: int) -> str | None:
        content = self.project.pad_content[sample_id]
        return None if content is None else content.instance_id

    def _slot(self, content: str) -> int | None:
        return next(
            (
                index
                for index, row in enumerate(self.project.pad_content)
                if row is not None and row.instance_id == content
            ),
            None,
        )

    def _guard(self, sample_id: int) -> str:
        validate_sample_id(sample_id)
        self._edit_guard(sample_id)
        if self._commit is None or self._blocked() or self._publications:
            message = "source beatmap selection writer is unavailable or settling"
            raise RuntimeError(message)
        content = self._content(sample_id)
        if content is None or self.project.sample_paths[sample_id] is None:
            message = "source beatmap selection requires an assigned source"
            raise ValueError(message)
        return content

    def select(self, sample_id: int, record_json: str) -> None:
        """Admit one real source-qualified record, preserving the previous selection."""
        content = self._guard(sample_id)
        self._admit(
            sample_id,
            content,
            lambda: self.audio.begin_source_beatmap_store_async(sample_id, record_json),
        )

    def create_manual(self, sample_id: int, origin_seconds: float, period_seconds: float) -> None:
        """Explicitly construct manual 4/4 geometry on the original source worker."""
        content = self._guard(sample_id)
        generation = self._source_generation(sample_id)
        if type(generation) is not int or not 0 < generation < 1 << 64:
            message = "manual source beatmap requires a current Native source generation"
            raise ValueError(message)
        self._admit(
            sample_id,
            content,
            lambda: self.audio.begin_source_beatmap_create_async(
                sample_id,
                origin_seconds,
                period_seconds,
                expected_source_generation=generation,
            ),
        )

    def edit(self, sample_id: int, operation: dict[str, object], *, expected_revision: str) -> None:
        """Send small explicit correction intent against one current qualified revision."""
        content = self._guard(sample_id)
        geometry = self.effective(sample_id)
        intent = self.project.pad_source_beatmap_intent[sample_id]
        if (
            geometry is None
            or geometry.revision() != expected_revision
            or intent is None
            or intent.selected is None
        ):
            message = "source beatmap correction target is no longer current"
            raise ValueError(message)
        encoded = json.dumps(operation, ensure_ascii=False, allow_nan=False, separators=(",", ":"))
        if len(encoded.encode("utf-8")) > 8192:
            message = "source beatmap correction intent exceeds the 8 KiB limit"
            raise ValueError(message)
        reference = intent.selected
        self._admit(
            sample_id,
            content,
            lambda: self.audio.begin_source_beatmap_edit_async(
                sample_id, reference.model_dump_json(), encoded
            ),
        )

    def _admit(self, sample_id: int, content: str, begin: Callable[[], OfflineAnalysisJob]) -> None:
        """Retain the same selection/publication owner for every map preparation purpose."""
        if content in self._pending:
            message = "source beatmap preparation is already pending for this content"
            raise RuntimeError(message)
        previous = self.project.pad_source_beatmap_intent[sample_id]
        requested = next_source_beatmap_intent(
            previous, None if previous is None else previous.selected
        )
        context = self._source_context(sample_id)
        reservation = self.assets.reserve(8)
        try:
            job = begin()
        except BaseException:
            reservation.close()
            raise
        self.project.pad_source_beatmap_intent[sample_id] = requested
        self._pending[content] = _MapJob(
            content,
            requested,
            job,
            restoring=False,
            source_context=context,
            reservation=reservation,
        )
        self._errors.pop(content, None)
        self.changed()

    def is_selected(self, sample_id: int) -> bool:
        """Identify selected or explicitly requested map mode without granting geometry."""
        validate_sample_id(sample_id)
        intent = self.project.pad_source_beatmap_intent[sample_id]
        return (intent is not None and intent.selected is not None) or (
            self._content(sample_id) in self._pending
        )

    def clear(self, sample_id: int) -> None:
        """Durably clear only map intent, leaving source and timing controls intact."""
        content = self._guard(sample_id)
        previous = self.project.pad_source_beatmap_intent[sample_id]
        selected = next_source_beatmap_intent(previous, None)
        reservation = self.assets.reserve(8)
        try:
            if not self._select_durable(sample_id, content, previous, selected, reservation):
                return
        except BaseException:
            reservation.close()
            raise
        self._cancel(content)
        self._effective.pop(content, None)
        self._failed.pop(content, None)
        self._errors.pop(content, None)

    def _select_durable(
        self,
        sample_id: int,
        content: str,
        previous: PadSourceBeatmapIntent | None,
        selected: PadSourceBeatmapIntent,
        reservation: AssetRetirementReservation,
    ) -> bool:
        candidate = self.project.model_copy(deep=True)
        candidate.pad_source_beatmap_intent[sample_id] = selected
        image = self.assets.prepare_assignment_image(candidate)
        try:
            plan = self.assets.prepare_assignment_publication(image)
            assert self._commit is not None
            self._commit(
                sample_id,
                previous,
                selected,
                lambda: (
                    self._content(sample_id) == content
                    and self.project.pad_source_beatmap_intent[sample_id] is previous
                ),
            )
        except BaseException:
            self.assets.release_assignment_image(image)
            raise
        # Reentrant newer intent after replacement stays authoritative in memory.
        # The actual saved Config independently protects its historical reference.
        if (
            self._content(sample_id) != content
            or self.project.pad_source_beatmap_intent[sample_id] is not previous
        ):
            self.assets.release_assignment_image(image)
            reservation.close()
            return False
        self.project.pad_source_beatmap_intent[sample_id] = selected
        publication = _Publication(content, image, plan, reservation)
        self._publications.append(publication)
        self._settle_publications()
        return True

    def _settle_publications(self) -> None:
        for publication in list(self._publications):
            if publication.plan is None:
                try:
                    image = self.assets.prepare_assignment_image(self.project)
                    try:
                        plan = self.assets.prepare_assignment_publication(image)
                    except BaseException:
                        self.assets.release_assignment_image(image)
                        raise
                except OSError, RuntimeError, ValueError:
                    continue
                publication.image, publication.plan = image, plan
            try:
                with publication.reservation.activate():
                    publication.plan.apply()
            except OSError, RuntimeError, ValueError:
                if publication.plan.owns_unpublished_image:
                    # An unrelated assignment publication may have changed the
                    # table after commit. Rebuild from the current full image.
                    assert publication.image is not None
                    self.assets.release_assignment_image(publication.image)
                    publication.image, publication.plan = None, None
                continue
            publication.reservation.close()
            self._publications.remove(publication)

    def completed(self, sample_id: int, event: dict[str, object]) -> None:
        """Consume only the original job's private delivery, never receipt dictionaries."""
        content = self._content(sample_id)
        pending = None if content is None else self._pending.get(content)
        delivery = event.get("_native_event_owner")
        if pending is None or delivery is None:
            return
        artifact = pending.job.resolve_source_beatmap_artifact_completion(
            cast("LoaderEventDelivery", delivery)
        )
        if artifact is None:
            return
        self._pending.pop(pending.content, None)
        reference = SourceBeatmapReference.model_validate_json(artifact.receipt_json())
        try:
            current = self.project.pad_source_beatmap_intent[sample_id]
            if current != pending.intent:
                artifact.retire_unselected()
                return
            if pending.restoring:
                if current.selected != reference:
                    return
                lease = self.assets._acquire_map(reference)
                try:
                    lease.acknowledge_source_beatmap()
                finally:
                    lease.release()
            else:
                selected = PadSourceBeatmapIntent(
                    schema_version=1, revision=pending.intent.revision, selected=reference
                )
                assert pending.reservation is not None
                self._select_durable(
                    sample_id, pending.content, current, selected, pending.reservation
                )
                pending.reservation = None
            selected_intent = self.project.pad_source_beatmap_intent[sample_id]
            if selected_intent is not None and selected_intent.selected == reference:
                self._effective[pending.content] = _EffectiveMap(
                    reference, artifact, cast("LoaderEventDelivery", delivery)
                )
                self._errors.pop(pending.content, None)
        except (OSError, RuntimeError, ValueError) as error:
            self._errors[pending.content] = str(error)
            artifact.retire_unselected()
        finally:
            if pending.reservation is not None:
                pending.reservation.close()

    def effective(self, sample_id: int) -> SourceBeatmap | None:
        """Return fresh Native-qualified geometry only for the still selected content."""
        validate_sample_id(sample_id)
        content = self._content(sample_id)
        effective = None if content is None else self._effective.get(content)
        intent = self.project.pad_source_beatmap_intent[sample_id]
        if effective is None or intent is None or intent.selected != effective.reference:
            return None
        try:
            owner = self.audio._resolve_loader_event(effective.delivery)
        except RuntimeError:
            return None
        if owner is None or owner[0] != sample_id:
            if content is not None:
                self._effective.pop(content, None)
            return None
        return effective.artifact.map()

    def playback_delivery(self, sample_id: int) -> LoaderEventDelivery | None:
        """Borrow only the original current qualification after assignment publication."""
        if self.effective(sample_id) is None:
            return None
        content = self._content(sample_id)
        if any(publication.content == content for publication in self._publications):
            return None
        effective = None if content is None else self._effective.get(content)
        return None if effective is None else effective.delivery

    def status(
        self,
        sample_id: int,
    ) -> Literal["none", "pending", "restoring", "settling", "effective", "failed"]:
        validate_sample_id(sample_id)
        content = self._content(sample_id)
        if any(publication.content == content for publication in self._publications):
            return "settling"
        pending = None if content is None else self._pending.get(content)
        if pending is not None:
            return "restoring" if pending.restoring else "pending"
        if self.effective(sample_id) is not None:
            return "effective"
        return "failed" if sample_id in self.errors else "none"

    def _source_context(self, sample_id: int) -> object:
        getter = getattr(self.audio, "waveform_source_identity", None)
        return None if getter is None else getter(sample_id)

    @staticmethod
    def _transient(message: str) -> bool:
        return any(
            word in message.lower()
            for word in (
                "busy",
                "queue full",
                "registry full",
                "admission",
                "sharing violation",
                "os error 32",
                "os error 33",
            )
        )

    def poll(self, deferred: frozenset[int], *, admit_restore: bool = True) -> None:
        self._poll_raw(deferred)
        self._settle_publications()
        self._poll_jobs(deferred)
        if admit_restore and not self._blocked():
            self._discover_restore(deferred)

    def _poll_jobs(self, deferred: frozenset[int]) -> None:
        for content, pending in list(self._pending.items()):
            sample_id = self._slot(content)
            if sample_id in deferred:
                continue
            if (
                sample_id is None
                or self.project.pad_source_beatmap_intent[sample_id] != pending.intent
                or pending.job.is_cancelled()
            ):
                self._cancel(content)
                continue
            status = pending.job.source_beatmap_async_status()
            if (
                status is not None
                and status.get("terminal") is True
                and status.get("stage")
                in {
                    "failed",
                    "cancelled",
                }
            ):
                detail = str(status.get("detail") or status["stage"])
                self._errors[content] = detail
                if self._transient(detail):
                    self._retry_after[content] = monotonic() + 1.0
                else:
                    self._failed[content] = pending.intent, pending.source_context
                self._cancel(content)
        current_contents = {row.instance_id for row in self.project.pad_content if row is not None}
        for cache in (self._effective, self._failed, self._errors, self._retry_after):
            for content in cache.keys() - current_contents:
                cache.pop(content, None)

    def _discover_restore(self, deferred: frozenset[int]) -> None:
        # Discovery also covers Copy/Move/Bank publications without a loader event.
        # At most one admission per tick uses the existing bounded Native cold pool.
        for sample_id, intent in enumerate(self.project.pad_source_beatmap_intent):
            if intent is None or intent.selected is None or sample_id in deferred:
                continue
            content = self._content(sample_id)
            if content is None or content in self._pending or self.effective(sample_id) is not None:
                continue
            context = self._source_context(sample_id)
            if context is None or self._failed.get(content) == (intent, context):
                continue
            if monotonic() < self._retry_after.get(content, 0.0):
                continue
            try:
                job = self.audio.begin_source_beatmap_restore_async(
                    sample_id, intent.selected.model_dump_json()
                )
            except (OSError, RuntimeError, ValueError) as error:
                message = str(error)
                if not self._transient(message):
                    self._errors[content] = message
                    self._failed[content] = intent, context
                else:
                    self._retry_after[content] = monotonic() + 1.0
                return
            self._pending[content] = _MapJob(
                content, intent, job, restoring=True, source_context=context
            )
            return

    def _cancel(self, content: str) -> None:
        pending = self._pending.pop(content, None)
        if pending is not None:
            pending.job.cancel()
            if pending.reservation is not None:
                pending.reservation.close()

    def shut_down(self) -> None:
        """Release control polling; Native retains its own bounded retirement owners."""
        self._cancel_raw()
        for content in list(self._pending):
            self._cancel(content)
        self._settle_publications()
        self._effective.clear()
