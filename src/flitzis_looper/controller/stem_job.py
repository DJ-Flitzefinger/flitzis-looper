import os
import threading
from dataclasses import dataclass, field, replace
from typing import TYPE_CHECKING, Literal

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.stem_identity import request_fingerprint, request_signature

if TYPE_CHECKING:
    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper_audio import (
        OfflineStemSourceLease,
        ProjectAssetLease,
        SealedProcessingFiles,
    )

type StemJobPhase = Literal["queued", "resolving", "resolved", "running", "settled", "disposed"]


@dataclass(eq=False, frozen=True, slots=True)
class StemGenerationInterest:
    """One caller's identity, independent of its optional diagnostic owner label."""

    owner: str | None = None


DIRECT_STEM_INTEREST = StemGenerationInterest("direct")


@dataclass(eq=False, slots=True)
class StemGenerationInterests:
    """Stable caller ownership through worker, preparation and durable settlement.

    Membership and outcome metadata grant no source or publication permission.
    The holder owns no readers; actual job and preparation owners settle those.
    """

    _current: frozenset[StemGenerationInterest] = frozenset({DIRECT_STEM_INTEREST})
    _committed: bool = field(default=False, init=False, repr=False)
    _already_present: bool = field(default=False, init=False, repr=False)
    _failure: str | None = field(default=None, init=False, repr=False)
    _lock: threading.Lock = field(default_factory=threading.Lock, init=False, repr=False)

    @property
    def current(self) -> frozenset[StemGenerationInterest]:
        """Return an immutable membership snapshot without exposing mutable state."""
        with self._lock:
            return self._current

    @property
    def committed(self) -> bool:
        """Report the durable result independently of later cancellation or adoption."""
        with self._lock:
            return self._committed

    @property
    def allows_preparation(self) -> bool:
        """Keep unfinished callers or an already committed result's normal continuation."""
        with self._lock:
            return bool(self._current) or self._committed

    @property
    def failure(self) -> str | None:
        """Return the first precommit failure, never an adoption or settlement error."""
        with self._lock:
            return self._failure

    @property
    def already_present(self) -> bool:
        """Report a current verified selection without inventing a new durable commit."""
        with self._lock:
            return self._already_present

    def mark_already_present(self) -> None:
        """Latch the authoritative existing-set no-op independently of publication."""
        with self._lock:
            self._already_present = True

    def attach(self, interest: StemGenerationInterest) -> bool:
        """Attach one caller after its existing controller route has admitted it."""
        with self._lock:
            if interest in self._current:
                return False
            self._current |= {interest}
            return True

    def detach(
        self, interest: StemGenerationInterest
    ) -> Literal["absent", "remaining", "detached"]:
        """Remove only one caller; the controller handles final-owner cancellation."""
        with self._lock:
            if interest not in self._current:
                return "absent"
            self._current -= {interest}
            return "remaining" if self._current else "detached"

    def invalidate_all(self) -> None:
        """Withdraw obsolete content's callers without changing its durable outcome."""
        with self._lock:
            self._current = frozenset()

    def mark_committed(self) -> None:
        """Latch successful durable replacement before any later owner settlement."""
        with self._lock:
            self._committed = True
            self._failure = None

    def mark_failed(self, message: str) -> None:
        """Record the first precommit failure without undoing a committed result."""
        with self._lock:
            if not self._committed and not self._already_present and self._failure is None:
                self._failure = message


@dataclass(frozen=True, slots=True)
class StemSubscriber:
    """One independently admitted content, never a copied native permit."""

    sample_id: int
    source: OfflineStemSourceLease
    content_id: str | None
    retirement: AssetRetirementReservation
    request: StemGenerationRequest | None = None
    intent: StemGenerationIntent | None = None
    source_lease: ProjectAssetLease | None = None
    _interests: StemGenerationInterests = field(
        default_factory=StemGenerationInterests, repr=False, compare=False
    )

    @property
    def interests(self) -> frozenset[StemGenerationInterest]:
        """Immutable caller snapshot without changing this PAD's publication row."""
        return self._interests.current

    @property
    def interest_owner(self) -> StemGenerationInterests:
        """Retain the same caller identity when actual publication custody changes."""
        return self._interests

    def same_interest_owner(self, other: StemSubscriber) -> bool:
        """Match local cancellation ownership, never publication or Native permission."""
        return self._interests is other._interests

    def close(self) -> None:
        """Release this PAD row's source custody and reserved publication capacity."""
        if self.source_lease is not None:
            self.source_lease.release()
        self.retirement.close()


@dataclass(frozen=True, slots=True)
class _PreparedStemSubscribers:
    """Captured participant interests; the enclosing mirror owns ACK validation."""

    job: StemGenerationJob
    source_id: int
    target_id: int
    source: StemSubscriber | None
    target: StemSubscriber | None
    moved_source: StemSubscriber | None
    moved_target: StemSubscriber | None

    def merged_locked(self) -> dict[int, StemSubscriber]:
        """Validate under the job lock and prepare a merge preserving unrelated interests."""
        current = self.job._subscribers
        if (
            self.job._cancelled
            or self.job._disposed
            or current.get(self.source_id) is not self.source
            or current.get(self.target_id) is not self.target
        ):
            message = "Stem pair subscriber ownership changed after mirror preparation"
            raise RuntimeError(message)
        merged = current.copy()
        merged.pop(self.source_id, None)
        merged.pop(self.target_id, None)
        if self.moved_source is not None:
            merged[self.target_id] = self.moved_source
        if self.moved_target is not None:
            merged[self.source_id] = self.moved_target
        return merged


@dataclass(slots=True)
class PreparedStemSubscriberDetach:
    """Exact removed interests, retained until their own cleanup completes.

    The enclosing aggregate mirror owns Native ACK validation and holds the job
    lock while validating and exchanging the returned subscriber image. This
    object neither publishes that image nor grants runtime permission.
    """

    job: StemGenerationJob
    before: tuple[tuple[int, StemSubscriber | None], ...]
    closed: int = 0
    settled: bool = False

    def merged_locked(self, *, allow_own_after: bool = False) -> dict[int, StemSubscriber]:
        """Prepare removal under the actual job lock, preserving every other reader."""
        current = self.job._subscribers
        original = all(current.get(slot) is row for slot, row in self.before)
        own_after = allow_own_after and all(slot not in current for slot, _ in self.before)
        if not original and not own_after:
            message = "Bank stem subscriber changed outside its original removal plan"
            raise RuntimeError(message)
        if original and (self.job._cancelled or self.job._disposed):
            message = "Bank stem job retired before its original interests were detached"
            raise RuntimeError(message)
        targets = {slot for slot, _ in self.before}
        return {slot: row for slot, row in current.items() if slot not in targets}

    def settle(self) -> bool:
        """Close detached interests once; running physical readers keep their own leases."""
        if self.settled:
            return True
        with self.job._lock:
            if any(slot in self.job._subscribers for slot, _ in self.before):
                message = "Bank stem cleanup cannot release a current subscriber"
                raise RuntimeError(message)
            while self.closed < len(self.before):
                subscriber = self.before[self.closed][1]
                if subscriber is not None:
                    subscriber.interest_owner.invalidate_all()
                    subscriber.close()
                self.closed += 1
            if not self.job._subscribers:
                self.job._cancelled = True
                self.job.cancel_event.set()
                if not self.job._running:
                    self.job._dispose_locked()
            self.settled = True
        return True


class StemGenerationJob:
    """Keep original and private-generation owners through actual backend reads.

    Cancellation releases an unstarted job immediately. A running backend retains
    its leases until it returns; cancellation never removes files under that read.
    """

    def __init__(
        self,
        request: StemGenerationRequest,
        source: OfflineStemSourceLease,
        assets: ProjectAssetLifecycle,
        content_id: str | None = None,
        *,
        intent: StemGenerationIntent | None = None,
        interest: StemGenerationInterest = DIRECT_STEM_INTEREST,
        interest_owner: StemGenerationInterests | None = None,
    ) -> None:
        if interest_owner is not None and not interest_owner.current:
            message = "Stem generation requires a nonempty interest owner"
            raise RuntimeError(message)
        self.request = request
        self.source: OfflineStemSourceLease | None = source
        self.intent = intent
        self.resolved_request: StemGenerationRequest | None = None
        # Control-only FIFO readiness; never callback timing or worker authority.
        self.admission_ready_sequence = 0
        self._assets = assets
        self.retirement = assets.reserve(18)
        try:
            self._source_lease = assets.acquire(request.source_path)
        except RuntimeError, ValueError, OSError:
            self.retirement.close()
            raise
        try:
            self._generation_lease = assets.acquire(request.cache_dir)
        except RuntimeError, ValueError, OSError:
            self._source_lease.release()
            self.retirement.close()
            raise
        self._lock = threading.Lock()
        self.cancel_event = threading.Event()
        self._cancelled = False
        self._running = False
        self._disposed = False
        self._begun = False
        self._phase: StemJobPhase = "queued"
        self.generation_fingerprint: str | None = None
        self.producer_identity: str | None = None
        self.processing_files: SealedProcessingFiles | None = None
        self._subscribers: dict[int, StemSubscriber] = {}
        try:
            self.add_subscriber(
                request.sample_id,
                source,
                content_id,
                request=request,
                intent=intent,
                interest=interest,
                interest_owner=interest_owner,
            )
        except OSError, RuntimeError, ValueError:
            self.cancel()
            raise

    def matches(self, request: StemGenerationRequest, producer_identity: str | None = None) -> bool:
        """Share resolved full content fingerprints, never model or source locators."""
        fingerprint = request_fingerprint(request, producer_identity)
        with self._lock:
            return (
                not self._cancelled
                and not self._disposed
                and self.resolved_request is not None
                and fingerprint is not None
                and fingerprint == self.generation_fingerprint
            )

    def cohorts(self, request: StemGenerationRequest) -> bool:
        """Group identical queued locators only before byte capture; this is not identity."""
        own = self.request
        with self._lock:
            return (
                not self._cancelled
                and not self._disposed
                and self._phase == "queued"
                and own.source_version == request.source_version
                and os.path.normcase(str(own.source_path.absolute()))
                == os.path.normcase(str(request.source_path.absolute()))
                and os.path.normcase(str(own.model_cache_dir.absolute()))
                == os.path.normcase(str(request.model_cache_dir.absolute()))
                and request_signature(own) == request_signature(request)
            )

    def add_subscriber(
        self,
        sample_id: int,
        source: OfflineStemSourceLease,
        content_id: str | None,
        *,
        request: StemGenerationRequest | None = None,
        intent: StemGenerationIntent | None = None,
        interest: StemGenerationInterest = DIRECT_STEM_INTEREST,
        interest_owner: StemGenerationInterests | None = None,
    ) -> None:
        """Reserve independent publication/source custody before joining existing work."""
        own_request = request or replace(self.request, sample_id=sample_id)
        if own_request.sample_id != sample_id:
            message = "Stem subscriber request must belong to its own pad"
            raise ValueError(message)
        holder = (
            StemGenerationInterests(frozenset({interest}))
            if interest_owner is None
            else interest_owner
        )
        if not holder.current:
            message = "Stem subscriber requires a nonempty interest owner"
            raise RuntimeError(message)
        retirement = self._assets.reserve(16)
        try:
            source_lease = self._assets.acquire(own_request.source_path)
        except OSError, RuntimeError, ValueError:
            retirement.close()
            raise
        with self._lock:
            if self._cancelled or self._disposed or sample_id in self._subscribers:
                source_lease.release()
                retirement.close()
                message = "Stem subscriber is cancelled or already registered"
                raise RuntimeError(message)
            with holder._lock:
                if not holder._current:
                    source_lease.release()
                    retirement.close()
                    message = "Stem subscriber interest owner was withdrawn before admission"
                    raise RuntimeError(message)
                self._subscribers[sample_id] = StemSubscriber(
                    sample_id,
                    source,
                    content_id,
                    retirement,
                    own_request,
                    intent,
                    source_lease,
                    holder,
                )

    def add_interest(self, expected: StemSubscriber, interest: StemGenerationInterest) -> bool:
        """Join an exact live PAD row; return whether this caller was newly attached.

        A stale placement, resolved-intent row or terminal job cannot admit new
        callers. Token-only joins preserve Pair/Bank plans and held deliveries.
        """
        with self._lock:
            if (
                self._cancelled
                or self._disposed
                or self._phase not in {"queued", "resolving", "resolved", "running"}
                or self._subscribers.get(expected.sample_id) is not expected
            ):
                message = "Stem interest requires an exact live subscriber before settlement"
                raise RuntimeError(message)
            return expected.interest_owner.attach(interest)

    def remove_interest(
        self, expected: StemSubscriber, interest: StemGenerationInterest
    ) -> Literal["absent", "remaining", "detached"]:
        """Detach only this original caller, closing its PAD row only on the final token.

        Original holder identity follows surviving placements; an absent token
        cannot cancel a replacement row even when its content and source match.
        Running physical readers retain their existing leases until backend return.
        """
        with self._lock:
            subscriber = self._interest_subscriber_locked(expected)
            if subscriber is None:
                return "absent"
            result = subscriber.interest_owner.detach(interest)
            if result == "detached":
                self._remove_subscriber_locked(subscriber.sample_id)
            return result

    def _interest_subscriber_locked(self, expected: StemSubscriber) -> StemSubscriber | None:
        return next(
            (
                subscriber
                for subscriber in self._subscribers.values()
                if subscriber.same_interest_owner(expected)
            ),
            None,
        )

    def update_intent(self, sample_id: int, intent: StemGenerationIntent) -> None:
        """Install one subscriber's resolved revision without changing captured job context."""
        with self._lock:
            subscriber = self._subscribers.get(sample_id)
            if subscriber is None:
                message = "Stem subscriber is no longer registered"
                raise RuntimeError(message)
            if subscriber.request is not None and request_signature(subscriber.request) != (
                intent.signature
            ):
                message = "Resolved stem intent differs from its own captured request"
                raise ValueError(message)
            self._subscribers[sample_id] = replace(subscriber, intent=intent)

    def prepare_pair_subscribers(
        self,
        source_id: int,
        target_id: int,
        source_content_id: str,
        target_content_id: str | None,
    ) -> _PreparedStemSubscribers:
        """Prepare surviving placements without changing requests or releasing any owner.

        The source and target name controller placements, not Native permissions.
        The enclosing private Pair mirror must validate its own opaque Native ACK
        and hold every involved job lock before exchanging any subscriber table.
        """
        if (
            source_id == target_id
            or any(
                type(value) is not int or not 0 <= value < NUM_SAMPLES
                for value in (source_id, target_id)
            )
            or not source_content_id
            or source_content_id == target_content_id
        ):
            message = "Stem pair subscribers require distinct participants"
            raise ValueError(message)
        with self._lock:
            if self._cancelled or self._disposed:
                message = "Stem pair cannot prepare a cancelled or disposed job"
                raise RuntimeError(message)
            source = self._subscribers.get(source_id)
            target = self._subscribers.get(target_id)
            if (source is not None and source.content_id != source_content_id) or (
                target is not None
                and (target_content_id is None or target.content_id != target_content_id)
            ):
                message = "Stem pair subscriber belongs to another captured content"
                raise RuntimeError(message)
            if (source is not None and source.sample_id != source_id) or (
                target is not None and target.sample_id != target_id
            ):
                message = "Stem pair subscriber has a conflicting current placement"
                raise RuntimeError(message)
            return _PreparedStemSubscribers(
                self,
                source_id,
                target_id,
                source,
                target,
                replace(source, sample_id=target_id) if source is not None else None,
                replace(target, sample_id=source_id) if target is not None else None,
            )

    def prepare_subscriber_detach(
        self, targets: tuple[tuple[int, str | None], ...]
    ) -> PreparedStemSubscriberDetach:
        """Capture exact target interests without cancelling a shared physical job."""
        slots = tuple(slot for slot, _ in targets)
        if len(set(slots)) != len(slots) or any(
            type(slot) is not int or not 0 <= slot < NUM_SAMPLES for slot in slots
        ):
            message = "Bank subscriber removal requires distinct strict target slots"
            raise ValueError(message)
        with self._lock:
            before = tuple((slot, self._subscribers.get(slot)) for slot in slots)
            for (slot, content_id), (_, subscriber) in zip(targets, before, strict=True):
                if subscriber is not None and (
                    content_id is None
                    or subscriber.content_id != content_id
                    or subscriber.sample_id != slot
                ):
                    message = "Bank subscriber belongs to another original target content"
                    raise RuntimeError(message)
            plan = PreparedStemSubscriberDetach(self, before)
            plan.merged_locked()
            return plan

    def transfer_subscribers_to(
        self, target: StemGenerationJob, *, require_active: bool = False
    ) -> tuple[StemSubscriber, ...]:
        """Move exact interests after resolution without releasing their independent custody."""
        if target is self:
            return ()
        first, second = (self, target) if id(self) < id(target) else (target, self)
        with first._lock, second._lock:
            if self._assets is not target._assets:
                message = "Stem subscribers cannot transfer across project ownership"
                raise ValueError(message)
            if self._running or self._cancelled or self._disposed:
                message = "Stem candidate has unsettled reads or no current interests"
                raise RuntimeError(message)
            if require_active and (
                target._cancelled
                or target._disposed
                or target._phase not in {"resolved", "running"}
            ):
                # A worker may have returned between the control observation
                # and these locks. Its already captured terminal delivery cannot
                # acquire a new subscriber; keep the candidate intact for retry.
                return ()
            if target._cancelled or target._disposed:
                message = "Stem target job is no longer available"
                raise RuntimeError(message)
            if (
                self.resolved_request is None
                or target.resolved_request is None
                or self.generation_fingerprint is None
                or self.generation_fingerprint != target.generation_fingerprint
            ):
                message = "Stem subscriber transfer requires equal verified full fingerprints"
                raise ValueError(message)
            if self._subscribers.keys() & target._subscribers.keys():
                message = "Stem target already contains a subscriber for this pad"
                raise RuntimeError(message)
            transferred = tuple(self._subscribers.values())
            target._subscribers.update(self._subscribers)
            self._subscribers.clear()
            self._cancelled = True
            self.cancel_event.set()
            self._dispose_locked()
            return transferred

    @property
    def running(self) -> bool:
        """Whether resolution or inference currently holds an actual backend read."""
        with self._lock:
            return self._running

    @property
    def phase(self) -> StemJobPhase:
        """Current bounded job phase for control-owned inference serialization."""
        with self._lock:
            return self._phase

    def subscribers(self) -> tuple[StemSubscriber, ...]:
        """Snapshot registered PAD rows without borrowing mutable worker state."""
        with self._lock:
            return tuple(self._subscribers.values())

    def remove_subscriber(self, sample_id: int) -> bool:
        """Detach all callers of one PAD; only the final PAD cancels physical work."""
        with self._lock:
            return self._remove_subscriber_locked(sample_id)

    def _remove_subscriber_locked(self, sample_id: int) -> bool:
        subscriber = self._subscribers.pop(sample_id, None)
        if subscriber is not None:
            subscriber.interest_owner.invalidate_all()
            subscriber.close()
        if not self._subscribers:
            self._cancelled = True
            self.cancel_event.set()
            if not self._running:
                self._dispose_locked()
        return bool(self._subscribers)

    def begin_resolution(self) -> OfflineStemSourceLease | None:
        """Claim byte resolution once while preserving the job source through the phase gap."""
        with self._lock:
            if self._cancelled or self._disposed or self._phase != "queued":
                return None
            self._running = True
            self._phase = "resolving"
            return self.source

    def finish_resolution(self) -> bool:
        """Settle resolution reads without releasing source/model custody before inference."""
        with self._lock:
            self._running = False
            if self._cancelled:
                self._dispose_locked()
            else:
                self._phase = "resolved"
            return not self._cancelled

    def begin(self) -> OfflineStemSourceLease | None:
        """Claim inference once, or ignore a cancelled queued task."""
        with self._lock:
            if self._cancelled or self._disposed or self._begun or self._phase == "resolving":
                return None
            self._begun = True
            self._running = True
            self._phase = "running"
            return self.source

    def finish_read(self) -> bool:
        """Release completed backend reads and report whether completion is current."""
        with self._lock:
            self._running = False
            self._phase = "settled"
            self.source = None
            self._source_lease.release()
            if self._cancelled:
                self._dispose_locked()
            return not self._cancelled

    def cancel(self) -> None:
        """Retire idle work now and running work after its last backend read."""
        with self._lock:
            self._cancelled = True
            self.cancel_event.set()
            for subscriber in self._subscribers.values():
                subscriber.interest_owner.invalidate_all()
                subscriber.close()
            self._subscribers.clear()
            if not self._running:
                self._dispose_locked()

    def dispose(self) -> None:
        """Release a settled event's ownership and retire only its private directory."""
        with self._lock:
            if not self._running:
                self._dispose_locked()

    def _dispose_locked(self) -> None:
        if self._disposed:
            return
        self._disposed = True
        self._phase = "disposed"
        self.source = None
        # The guard outlives backend return and the queued event. Successful
        # settlement promotes the immutable WAVs/marker before disposing this job.
        self.processing_files = None
        for subscriber in self._subscribers.values():
            subscriber.close()
        self._subscribers.clear()
        self._source_lease.release()
        with self.retirement.activate():
            self._assets.retire(
                self.request.cache_dir, recursive=True, lease=self._generation_lease
            )
        self.retirement.close()
