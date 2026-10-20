"""Fair control admission through the actual shared two-worker/32-queued pool."""

import threading
from contextlib import contextmanager
from dataclasses import dataclass, field
from functools import partial
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.stem_admission import (
    STEM_ADMISSION_GRANTS_PER_POLL,
    STEM_ADMISSION_LANES,
    StemAdmissionArbiter,
    StemAdmissionCandidate,
)
from flitzis_looper.controller.stem_workers import (
    STEM_QUEUED_JOBS,
    STEM_WORKERS,
    StemWorkerPool,
)

if TYPE_CHECKING:
    from collections.abc import Iterator

    from flitzis_looper.controller.stem_admission import (
        StemAdmissionClass,
        StemAdmissionLane,
        StemAdmissionPhase,
        StemAdmissionResult,
    )
    from flitzis_looper.controller.stem_workers import StemWorkerReservation


@dataclass(slots=True, eq=False)
class _Owner:
    name: str
    submitted: int = 0
    ran: threading.Event = field(default_factory=threading.Event)


@dataclass(slots=True)
class _HeldPool:
    pool: StemWorkerPool
    release: threading.Event
    queued: list[threading.Event] = field(default_factory=list)

    def fill(self, count: int) -> None:
        for _ in range(count):
            completed = threading.Event()
            self.queued.append(completed)
            self.pool(completed.set)


@contextmanager
def _held_pool() -> Iterator[_HeldPool]:
    pool = StemWorkerPool()
    release = threading.Event()
    started = tuple(threading.Event() for _ in range(STEM_WORKERS))
    finished = tuple(threading.Event() for _ in range(STEM_WORKERS))
    held = _HeldPool(pool, release)

    def hold(index: int) -> None:
        started[index].set()
        try:
            assert release.wait(10)
        finally:
            finished[index].set()

    try:
        for index in range(STEM_WORKERS):
            pool(partial(hold, index))
        assert all(event.wait(5) for event in started)
        yield held
    finally:
        release.set()
        pool.shutdown()
        assert all(event.wait(5) for event in finished)
        assert all(event.wait(5) for event in held.queued)


def _candidate(
    owner: _Owner,
    ready_sequence: int,
    lane: StemAdmissionLane = ("direct", "resolve"),
) -> StemAdmissionCandidate[_Owner]:
    return StemAdmissionCandidate(owner, ready_sequence, *lane)


def _submit(
    candidate: StemAdmissionCandidate[_Owner], reservation: StemWorkerReservation
) -> StemAdmissionResult:
    reservation.submit(candidate.owner.ran.set)
    candidate.owner.submitted += 1
    return "submitted"


def _assert_only_one_slot_available(pool: StemWorkerPool) -> None:
    reservation = pool.try_reserve()
    assert reservation is not None
    try:
        assert pool.try_reserve() is None
    finally:
        reservation.close()


def test_full_same_pool_refuses_before_admission_and_preserves_original_owner() -> None:
    owner = _Owner("original source/content/intent")
    original = _candidate(owner, 7)
    admissions: list[StemAdmissionCandidate[_Owner]] = []
    arbiter = StemAdmissionArbiter()

    def admit(
        candidate: StemAdmissionCandidate[_Owner], reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        admissions.append(candidate)
        return _submit(candidate, reservation)

    with _held_pool() as held:
        held.fill(STEM_QUEUED_JOBS)
        assert held.pool.try_reserve() is None
        assert arbiter.run(lambda: (original,), held.pool.try_reserve, admit) == 0
        assert admissions == []
        assert owner.submitted == 0
        assert not owner.ran.is_set()
        assert not any(event.is_set() for event in held.queued)

        held.release.set()
        # The queued tail is a real executor barrier, with no sleep/retry loop.
        assert held.queued[-1].wait(5)
        assert arbiter.run(lambda: (original,), held.pool.try_reserve, admit) == 1
        assert admissions == [original]
        assert admissions[0].owner is owner
        assert owner.ran.wait(5)
        assert owner.submitted == 1


@pytest.mark.parametrize("result", ["deferred", "settled"])
def test_unsubmitted_result_returns_the_only_permit_within_the_poll(
    result: StemAdmissionResult,
) -> None:
    owner = _Owner("guard still settling")
    original = _candidate(owner, 1)
    attempts: list[StemAdmissionCandidate[_Owner]] = []

    def admit(
        candidate: StemAdmissionCandidate[_Owner], _reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        attempts.append(candidate)
        return result

    with _held_pool() as held:
        held.fill(STEM_QUEUED_JOBS - 1)
        assert StemAdmissionArbiter().run(lambda: (original,), held.pool.try_reserve, admit) == 0
        assert attempts == [original]
        assert owner.submitted == 0
        assert not owner.ran.is_set()
        _assert_only_one_slot_available(held.pool)


def test_admission_exception_propagates_and_returns_the_only_unused_permit() -> None:
    owner = _Owner("capture failed")
    original = _candidate(owner, 1)

    def fail(
        _candidate: StemAdmissionCandidate[_Owner], _reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        message = "original source validation failed"
        raise ValueError(message)

    with _held_pool() as held:
        held.fill(STEM_QUEUED_JOBS - 1)
        with pytest.raises(ValueError, match="original source validation failed"):
            StemAdmissionArbiter().run(lambda: (original,), held.pool.try_reserve, fail)
        _assert_only_one_slot_available(held.pool)
        assert owner.submitted == 0
        assert not owner.ran.is_set()


def test_six_lanes_and_fifo_progress_despite_continuous_new_direct_arrivals() -> None:
    oldest = [_candidate(_Owner(f"oldest-{lane}"), 1, lane) for lane in STEM_ADMISSION_LANES]
    older = [_candidate(_Owner(f"older-{lane}"), 2, lane) for lane in STEM_ADMISSION_LANES]
    pending = oldest + older
    submitted: list[StemAdmissionCandidate[_Owner]] = []
    provider_calls = 0
    arbiter = StemAdmissionArbiter()

    def provider() -> tuple[StemAdmissionCandidate[_Owner], ...]:
        nonlocal provider_calls
        provider_calls += 1
        for lane in STEM_ADMISSION_LANES:
            if lane[0] == "direct":
                pending.append(
                    _candidate(
                        _Owner(f"arrival-{provider_calls}-{lane}"), 100 + provider_calls, lane
                    )
                )
        # Readiness order must not depend on enumeration order.
        return tuple(reversed(pending))

    def admit(
        candidate: StemAdmissionCandidate[_Owner], reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        result = _submit(candidate, reservation)
        submitted.append(candidate)
        pending.remove(candidate)
        return result

    with _held_pool() as held:
        assert arbiter.run(provider, held.pool.try_reserve, admit) == STEM_ADMISSION_GRANTS_PER_POLL
        assert submitted == oldest
        assert provider_calls == STEM_ADMISSION_GRANTS_PER_POLL
        assert all(not candidate.owner.ran.is_set() for candidate in submitted)
        assert arbiter.run(provider, held.pool.try_reserve, admit) == STEM_ADMISSION_GRANTS_PER_POLL
        assert submitted == oldest + older
        assert len(pending) > STEM_ADMISSION_GRANTS_PER_POLL
        held.release.set()
        assert all(candidate.owner.ran.wait(5) for candidate in submitted)
        assert all(candidate.owner.submitted == 1 for candidate in submitted)


def test_deferred_oldest_does_not_block_its_lane_or_advance_the_cursor() -> None:
    blocked = _candidate(_Owner("old blocked Pair guard"), 1)
    next_ready = _candidate(_Owner("same-lane continuation"), 2)
    other_lane = _candidate(_Owner("next lane"), 1, ("direct", "infer"))
    image = (other_lane, next_ready, blocked)
    attempts: list[StemAdmissionCandidate[_Owner]] = []

    def admit(
        candidate: StemAdmissionCandidate[_Owner], reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        attempts.append(candidate)
        if candidate is blocked:
            return "deferred"
        return _submit(candidate, reservation)

    with _held_pool() as held:
        assert StemAdmissionArbiter().run(lambda: image, held.pool.try_reserve, admit) == 2
        assert attempts == [blocked, next_ready, other_lane]
        assert blocked.owner.submitted == 0
        held.release.set()
        assert next_ready.owner.ran.wait(5)
        assert other_lane.owner.ran.wait(5)


def test_a_poll_with_only_deferred_work_keeps_the_next_poll_original_lane_priority() -> None:
    original = _candidate(_Owner("original deferred"), 1)
    competitor = _candidate(_Owner("other lane"), 1, ("direct", "infer"))
    attempts: list[StemAdmissionCandidate[_Owner]] = []
    arbiter = StemAdmissionArbiter()

    def defer(
        candidate: StemAdmissionCandidate[_Owner], _reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        attempts.append(candidate)
        return "deferred"

    def admit(
        candidate: StemAdmissionCandidate[_Owner], reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        attempts.append(candidate)
        return _submit(candidate, reservation)

    with _held_pool() as held:
        assert arbiter.run(lambda: (original,), held.pool.try_reserve, defer) == 0
        assert arbiter.run(lambda: (competitor, original), held.pool.try_reserve, admit) == 2
        assert attempts == [original, original, competitor]
        held.release.set()
        assert original.owner.ran.wait(5)
        assert competitor.owner.ran.wait(5)


@pytest.mark.parametrize("result", ["deferred", "settled"])
def test_unchanged_duplicate_stale_image_is_attempted_once_without_provider_spin(
    result: StemAdmissionResult,
) -> None:
    original = _candidate(_Owner("unchanged authoritative image"), 1)
    duplicate_view = _candidate(original.owner, original.ready_sequence)
    attempts: list[StemAdmissionCandidate[_Owner]] = []
    provider_calls = 0

    def provider() -> tuple[StemAdmissionCandidate[_Owner], ...]:
        nonlocal provider_calls
        provider_calls += 1
        return original, duplicate_view, original

    def admit(
        candidate: StemAdmissionCandidate[_Owner], _reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        attempts.append(candidate)
        return result

    with _held_pool() as held:
        assert StemAdmissionArbiter().run(provider, held.pool.try_reserve, admit) == 0
        assert attempts == [original]
        assert provider_calls == 1


def test_submitted_candidate_is_not_redelivered_when_refreshed_provider_is_unchanged() -> None:
    original = _candidate(_Owner("same old view after consumption"), 1)
    duplicate_view = _candidate(original.owner, original.ready_sequence)
    attempts: list[StemAdmissionCandidate[_Owner]] = []

    def admit(
        candidate: StemAdmissionCandidate[_Owner], reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        attempts.append(candidate)
        return _submit(candidate, reservation)

    with _held_pool() as held:
        image = (original, duplicate_view)
        assert StemAdmissionArbiter().run(lambda: image, held.pool.try_reserve, admit) == 1
        assert attempts == [original]
        held.release.set()
        assert original.owner.ran.wait(5)
        assert original.owner.submitted == 1


def test_successful_partial_poll_retains_rotation_when_capacity_returns() -> None:
    original = [_candidate(_Owner(str(lane)), 1, lane) for lane in STEM_ADMISSION_LANES]
    newcomer = _candidate(_Owner("new direct resolve"), 2)
    pending = list(original)
    submitted: list[StemAdmissionCandidate[_Owner]] = []
    arbiter = StemAdmissionArbiter()

    def admit(
        candidate: StemAdmissionCandidate[_Owner], reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        result = _submit(candidate, reservation)
        pending.remove(candidate)
        submitted.append(candidate)
        return result

    with _held_pool() as held:
        held.fill(STEM_QUEUED_JOBS - 1)
        assert arbiter.run(lambda: tuple(pending), held.pool.try_reserve, admit) == 1
        assert submitted == original[:1]
        pending.append(newcomer)
        held.release.set()
        assert original[0].owner.ran.wait(5)
        assert arbiter.run(lambda: tuple(pending), held.pool.try_reserve, admit) == 6
        assert submitted == [*original, newcomer]
        assert all(candidate.owner.ran.wait(5) for candidate in submitted)


@pytest.mark.parametrize("sequence", [True, 1.0, "1", None])
def test_readiness_sequence_rejects_coercion(sequence: object) -> None:
    with pytest.raises(TypeError, match="must be an integer"):
        _candidate(_Owner("bad sequence"), cast("int", sequence))


def test_readiness_sequence_rejects_negative_values() -> None:
    with pytest.raises(ValueError, match="must be nonnegative"):
        _candidate(_Owner("bad sequence"), -1)


@pytest.mark.parametrize(
    ("request_class", "phase"),
    [("interactive", "resolve"), ("direct", "inference"), (None, "prepare"), ("batch", 1)],
)
def test_candidate_rejects_unknown_lane_metadata(request_class: object, phase: object) -> None:
    with pytest.raises(ValueError, match="Unknown stem admission"):
        StemAdmissionCandidate(
            _Owner("bad lane"),
            0,
            cast("StemAdmissionClass", request_class),
            cast("StemAdmissionPhase", phase),
        )
