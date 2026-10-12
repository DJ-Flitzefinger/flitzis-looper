"""Declared own receipts test control state; they do not prove rendered Native output."""

from collections import deque
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

import pytest

from flitzis_looper.controller.committed_stem_residency import CommittedStemResidency
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection

if TYPE_CHECKING:
    from flitzis_looper.models import StemMixMode
    from flitzis_looper_audio import AudioEngine, PreparedSourceTicket, PreparedStemPair


@dataclass(slots=True)
class _Registration:
    identity: str
    state: str = "accepted"
    current: bool = True
    message: str | None = None
    cancellations: int = 0
    reader_active: bool = True

    def publication_status(self) -> str:
        return self.state

    def is_current(self) -> bool:
        return self.current

    def error(self) -> str | None:
        return self.message

    def cancel(self) -> None:
        self.cancellations += 1
        self.current = False


@dataclass(slots=True)
class _OwnReceipt:
    residency: str = "pending"
    effective: str = "waiting"
    current: bool = True
    owned_claim: bool = False
    message: str | None = None
    failure_reason: str | None = None
    withdrawals: int = 0
    cancellations: int = 0
    reader_active: bool = True

    def residency_status(self) -> str:
        return self.residency

    def effective_mode_status(self) -> str:
        # A later ordinary Play loses the old receipt's launch permit. Its own
        # accepted component ACK and callback-held readers remain independent.
        return self.effective if self.current else "rejected"

    def effective_failure_reason(self) -> str | None:
        return self.failure_reason

    def is_current(self) -> bool:
        return self.current

    def error(self) -> str | None:
        return self.message

    def claimed(self) -> bool:
        return self.owned_claim

    def withdraw(self) -> bool:
        self.withdrawals += 1
        if self.owned_claim:
            return False
        self.current = False
        return True

    def cancel(self) -> None:
        self.cancellations += 1
        if not self.owned_claim:
            self.current = False


@dataclass(frozen=True, slots=True)
class _PreparedPair:
    identity: str


@dataclass(slots=True)
class _Audio:
    registrations: list[_Registration] = field(default_factory=list)
    candidates: list[_Registration] = field(default_factory=list)
    candidate_admissions: list[str] = field(default_factory=list)
    admissions: list[tuple[_Registration, str, int]] = field(default_factory=list)
    receipts: deque[_OwnReceipt] = field(default_factory=deque)
    request_error: RuntimeError | None = None
    candidate_error: RuntimeError | None = None

    def register_committed_stem_pair(
        self, prepared: _PreparedPair, _source_ticket: object
    ) -> _Registration:
        ticket = _Registration(prepared.identity)
        self.registrations.append(ticket)
        return ticket

    def register_pending_committed_stem_pair(
        self, prepared: _PreparedPair, _source_ticket: object
    ) -> _Registration:
        self.candidate_admissions.append(prepared.identity)
        if self.candidate_error is not None:
            raise self.candidate_error
        ticket = _Registration(prepared.identity)
        self.candidates.append(ticket)
        return ticket

    def request_committed_stem_residency(
        self, ticket: _Registration, *, mode: str, enabled_mask: int
    ) -> _OwnReceipt:
        self.admissions.append((ticket, mode, enabled_mask))
        if self.request_error is not None:
            raise self.request_error
        return self.receipts.popleft() if self.receipts else _OwnReceipt()


def _entry(generation: str, *, source: str = "1") -> StemCacheEntry:
    root = f"samples/materials/M{'a' * 32}"
    pair = StemPairSelection(
        descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{generation * 32}.json",
        stem_set_identity=generation * 64,
        wav_generation=f"{root}/stems/.ready-{generation * 32}",
        pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{generation * 32}",
    )
    return StemCacheEntry(
        source_version=f"{root}/original/source.wav|sha256-v1:{source * 64}",
        cache_dir=pair.wav_generation,
        stems=expected_stem_files(pair.wav_generation),
        pair=pair,
        available=True,
    )


@dataclass(slots=True)
class _Harness:
    audio: _Audio
    helper: CommittedStemResidency
    image: dict[int, tuple[StemCacheEntry, str, str]]
    errors: list[tuple[int, str | None]]

    def bind(
        self,
        generation: str,
        *,
        active: bool = False,
        sample_id: int = 0,
        source: str = "1",
        content: str = "content-1",
        path: str = "samples/source.wav",
    ) -> StemCacheEntry:
        entry = _entry(generation, source=source)
        self.image[sample_id] = entry, content, path
        self.helper.register(
            sample_id,
            cast("PreparedStemPair", _PreparedPair(generation * 64)),
            cast("PreparedSourceTicket", object()),
            entry,
            content,
            path,
            active=active,
        )
        return entry

    def effective_v1(self, *, sample_id: int = 0) -> _Registration:
        receipt = _OwnReceipt(residency="accepted", effective="effective", owned_claim=True)
        self.audio.receipts.append(receipt)
        self.bind("b", sample_id=sample_id)
        assert self.helper.request(sample_id, "all_stems", 15)
        self.helper.poll()
        self.errors.clear()
        return self.audio.registrations[-1]


@pytest.fixture
def adoption() -> _Harness:
    audio = _Audio()
    image: dict[int, tuple[StemCacheEntry, str, str]] = {}
    errors: list[tuple[int, str | None]] = []

    def source_current(
        sample_id: int, entry: StemCacheEntry, content: str | None, path: str | None
    ) -> bool:
        selected = image.get(sample_id)
        return (
            selected is not None
            and selected[0].source_version == entry.source_version
            and selected[1:] == (content, path)
        )

    def exact_current(
        sample_id: int, entry: StemCacheEntry, content: str | None, path: str | None
    ) -> bool:
        selected = image.get(sample_id)
        return (
            source_current(sample_id, entry, content, path)
            and selected is not None
            and selected[0].pair == entry.pair
        )

    helper = CommittedStemResidency(
        cast("AudioEngine", audio),
        exact_current,
        lambda sample_id, message: errors.append((sample_id, message)),
        source_current=source_current,
    )
    return _Harness(audio, helper, image, errors)


def test_candidate_registration_does_not_revoke_effective_v1(adoption: _Harness) -> None:
    predecessor = adoption.effective_v1()
    adoption.bind("c", active=True)
    candidate = adoption.audio.candidates[-1]
    candidate.state = "captured"
    receipt = _OwnReceipt()
    adoption.audio.receipts.append(receipt)

    assert adoption.helper.request(0, "all_stems", 5)
    adoption.helper.poll()

    assert predecessor.cancellations == 0
    assert predecessor.current
    assert len(adoption.audio.admissions) == 1
    status = adoption.helper.status(0)
    assert status is not None
    assert status.registration == "captured"
    assert status.disk_stem_set_identity == "c" * 64
    assert status.control_stem_set_identity == status.effective_stem_set_identity == "b" * 64
    candidate.state = "accepted"
    adoption.helper.poll()
    assert adoption.audio.admissions[-1] == (candidate, "all_stems", 5)
    status = adoption.helper.status(0)
    assert status is not None
    assert status.effective == "waiting"
    assert status.effective_stem_set_identity == "b" * 64


@pytest.mark.parametrize("effective", ["waiting", "transitioning"])
def test_own_acceptance_updates_control_without_claiming_wet_v2(
    adoption: _Harness, effective: str
) -> None:
    adoption.effective_v1()
    receipt = _OwnReceipt()
    adoption.audio.receipts.append(receipt)
    adoption.bind("c", active=True)
    assert adoption.helper.request(0, "all_stems", 15)
    receipt.residency = "accepted"
    receipt.owned_claim = True
    receipt.effective = effective

    adoption.helper.poll()

    status = adoption.helper.status(0)
    assert status is not None
    assert status.control_stem_set_identity == "c" * 64
    assert status.effective_stem_set_identity == "b" * 64
    assert status.effective == effective
    assert not adoption.errors
    receipt.effective = "effective"
    adoption.helper.poll()
    status = adoption.helper.status(0)
    assert status is not None
    assert status.effective_stem_set_identity == "c" * 64


def test_first_active_candidate_uses_its_own_pending_route(adoption: _Harness) -> None:
    adoption.bind("c", active=True)
    receipt = _OwnReceipt(residency="accepted", owned_claim=True)
    adoption.audio.receipts.append(receipt)
    assert adoption.helper.request(0, "all_stems", 15)

    adoption.helper.poll()

    assert not adoption.audio.registrations
    status = adoption.helper.status(0)
    assert status is not None
    assert status.residency == "accepted"
    assert status.control_stem_set_identity == "c" * 64
    assert status.effective == "waiting"
    assert status.effective_stem_set_identity is None


def test_claimed_v2_remains_observed_while_v3_and_v4_coalesce(adoption: _Harness) -> None:
    predecessor = adoption.effective_v1()
    second = _OwnReceipt(owned_claim=True)
    latest = _OwnReceipt()
    adoption.audio.receipts.extend((second, latest))
    adoption.bind("c", active=True)
    second_ticket = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.bind("d", active=True)
    assert adoption.helper.request(0, "all_stems", 2)
    adoption.bind("e", active=True)
    assert adoption.helper.request(0, "all_stems", 5)
    adoption.helper.poll()

    assert len(adoption.audio.candidates) == 1
    assert len(adoption.audio.admissions) == 2
    assert predecessor.cancellations == second_ticket.cancellations == 0
    status = adoption.helper.status(0)
    assert status is not None
    assert status.disk_stem_set_identity == "e" * 64
    assert status.attempt_stem_set_identity == "c" * 64
    second.residency = "accepted"
    second.effective = "transitioning"
    adoption.helper.poll()
    assert len(adoption.audio.candidates) == 1
    second.effective = "effective"
    adoption.helper.poll()

    assert [ticket.identity for ticket in adoption.audio.candidates] == ["c" * 64, "e" * 64]
    assert adoption.audio.admissions[-1] == (adoption.audio.candidates[-1], "all_stems", 5)
    status = adoption.helper.status(0)
    assert status is not None
    assert status.effective_stem_set_identity == "c" * 64
    assert status.disk_stem_set_identity == status.attempt_stem_set_identity == "e" * 64
    assert status.effective == "waiting"


def test_unclaimed_newer_disk_withdraws_only_its_predecessor_candidate(adoption: _Harness) -> None:
    predecessor = adoption.effective_v1()
    second = _OwnReceipt()
    third = _OwnReceipt()
    adoption.audio.receipts.extend((second, third))
    adoption.bind("c", active=True)
    second_ticket = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)

    adoption.bind("d", active=True)
    assert adoption.helper.request(0, "all_stems", 2)
    adoption.helper.poll()

    assert second.withdrawals == second_ticket.cancellations == 1
    assert predecessor.cancellations == 0
    assert predecessor.current
    assert second.reader_active
    assert adoption.audio.admissions[-1] == (adoption.audio.candidates[-1], "all_stems", 2)


def test_full_mix_withdraws_unclaimed_all_and_uses_retained_v1(adoption: _Harness) -> None:
    predecessor = adoption.effective_v1()
    candidate_receipt = _OwnReceipt()
    full_mix = _OwnReceipt()
    adoption.audio.receipts.extend((candidate_receipt, full_mix))
    adoption.bind("c", active=True)
    candidate = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)

    assert adoption.helper.request(0, "full_mix", 15)
    assert adoption.helper.request(0, "full_mix", 15)
    adoption.helper.poll()

    assert candidate_receipt.withdrawals == candidate.cancellations == 1
    assert predecessor.current
    assert predecessor.cancellations == 0
    assert adoption.audio.admissions[-1] == (predecessor, "full_mix", 15)
    assert len(adoption.audio.admissions) == 3
    assert full_mix.withdrawals == 0
    candidate_receipt.residency = "accepted"
    candidate_receipt.effective = "effective"
    adoption.helper.poll()
    status = adoption.helper.status(0)
    assert status is not None
    assert status.attempt_stem_set_identity == "b" * 64
    assert status.requested_mode == "full_mix"
    assert status.effective == "waiting"
    assert status.effective_stem_set_identity == "b" * 64
    full_mix.residency = "accepted"
    full_mix.effective = "effective"
    full_mix.owned_claim = True
    adoption.helper.poll()
    status = adoption.helper.status(0)
    assert status is not None
    assert status.registration == "withdrawn"
    assert status.requested_mode == "full_mix"
    assert status.residency == "accepted"
    assert status.effective == "effective"
    assert status.effective_stem_set_identity is None
    assert not status.attempt_failed


def test_full_mix_after_claim_waits_then_uses_adopted_v2(adoption: _Harness) -> None:
    predecessor = adoption.effective_v1()
    candidate_receipt = _OwnReceipt(
        residency="accepted", effective="transitioning", owned_claim=True
    )
    full_mix = _OwnReceipt()
    adoption.audio.receipts.extend((candidate_receipt, full_mix))
    adoption.bind("c", active=True)
    candidate = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.helper.poll()
    predecessor.current = False

    assert adoption.helper.request(0, "full_mix", 15)
    adoption.helper.poll()

    assert len(adoption.audio.admissions) == 2
    status = adoption.helper.status(0)
    assert status is not None
    assert status.requested_mode == "all_stems"
    assert status.desired_mode == "full_mix"
    assert status.effective == "transitioning"
    candidate_receipt.effective = "effective"
    adoption.helper.poll()
    assert adoption.audio.admissions[-1] == (candidate, "full_mix", 15)
    assert candidate.cancellations == 0
    full_mix.residency = "accepted"
    full_mix.effective = "effective"
    adoption.helper.poll()
    status = adoption.helper.status(0)
    assert status is not None
    assert status.effective_stem_set_identity is None


def test_first_full_mix_withdraws_candidate_without_inventing_an_ack(adoption: _Harness) -> None:
    receipt = _OwnReceipt()
    adoption.audio.receipts.append(receipt)
    adoption.bind("c", active=True)
    ticket = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)

    assert adoption.helper.request(0, "full_mix", 15)
    adoption.helper.poll()

    assert receipt.withdrawals == ticket.cancellations == 1
    assert adoption.helper.contains(0)
    assert len(adoption.audio.admissions) == 1
    status = adoption.helper.status(0)
    assert status is not None
    assert status.registration == "withdrawn"
    assert status.effective is None
    assert status.effective_stem_set_identity is None


@pytest.mark.parametrize("failure", ["queue", "residency", "effective", "stale"])
def test_candidate_failure_preserves_v1_and_requires_explicit_retry(
    adoption: _Harness, failure: str
) -> None:
    predecessor = adoption.effective_v1()
    failed = _OwnReceipt(message="candidate history unavailable")
    adoption.audio.receipts.append(failed)
    adoption.bind("c", active=True)
    if failure == "queue":
        adoption.audio.request_error = RuntimeError("native queue full")
        assert not adoption.helper.request(0, "all_stems", 15)
    else:
        assert adoption.helper.request(0, "all_stems", 15)
        if failure == "stale":
            failed.current = False
        else:
            setattr(failed, failure, "rejected")
    for _ in range(3):
        adoption.helper.poll()

    assert len(adoption.audio.admissions) == 2
    assert adoption.errors[-1][1] is not None
    assert predecessor.current
    assert predecessor.cancellations == 0
    status = adoption.helper.status(0)
    assert status is not None
    assert status.effective_stem_set_identity == status.control_stem_set_identity == "b" * 64
    assert status.attempt_failed
    adoption.audio.request_error = None
    adoption.audio.receipts.clear()
    adoption.audio.receipts.append(_OwnReceipt())
    assert adoption.helper.request(0, "all_stems", 15)
    assert len(adoption.audio.admissions) == 3
    status = adoption.helper.status(0)
    assert status is not None
    assert not status.attempt_failed


def test_stopped_ready_v2_supplies_full_mix_control_without_wet_success(adoption: _Harness) -> None:
    adoption.effective_v1()
    ready = _OwnReceipt(residency="accepted", effective="waiting", owned_claim=True)
    adoption.audio.receipts.append(ready)
    adoption.bind("c", active=True)
    candidate = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.helper.poll()

    assert adoption.helper.request(0, "full_mix", 15)

    assert adoption.audio.admissions[-1] == (candidate, "full_mix", 15)
    status = adoption.helper.status(0)
    assert status is not None
    assert status.effective_stem_set_identity == "b" * 64
    assert status.control_stem_set_identity == "c" * 64


def test_registration_failure_has_no_poll_retry_and_retains_v1(adoption: _Harness) -> None:
    predecessor = adoption.effective_v1()
    adoption.audio.candidate_error = RuntimeError("candidate capacity full")
    with pytest.raises(RuntimeError, match="candidate capacity full"):
        adoption.bind("c", active=True)
    for _ in range(3):
        adoption.helper.poll()

    assert adoption.audio.candidate_admissions == ["c" * 64]
    assert predecessor.current
    assert predecessor.cancellations == 0
    assert adoption.helper.request(0, "full_mix", 15)
    assert adoption.audio.admissions[-1] == (predecessor, "full_mix", 15)


def test_adopted_ticket_remains_normal_full_mix_and_all_authority(adoption: _Harness) -> None:
    adoption.effective_v1()
    adopted = _OwnReceipt(residency="accepted", effective="effective", owned_claim=True)
    full_mix = _OwnReceipt(residency="accepted", effective="effective", owned_claim=True)
    again = _OwnReceipt()
    adoption.audio.receipts.extend((adopted, full_mix, again))
    adoption.bind("c", active=True)
    ticket = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.helper.poll()
    assert adoption.helper.request(0, "full_mix", 15)
    adoption.helper.poll()

    assert adoption.helper.request(0, "all_stems", 5)

    assert adoption.audio.admissions[-3:] == [
        (ticket, "all_stems", 15),
        (ticket, "full_mix", 15),
        (ticket, "all_stems", 5),
    ]
    assert len(adoption.audio.candidates) == 1
    assert ticket.cancellations == 0


@pytest.mark.parametrize("claimed", [False, True])
@pytest.mark.parametrize("changed", ["unload", "content", "source"])
def test_source_supersession_withdraws_control_without_claiming_reader_retirement(
    adoption: _Harness, changed: str, *, claimed: bool
) -> None:
    predecessor = adoption.effective_v1()
    receipt = _OwnReceipt(owned_claim=claimed)
    adoption.audio.receipts.append(receipt)
    adoption.bind("c", active=True)
    candidate = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    entry, content, path = adoption.image[0]
    if changed == "unload":
        adoption.image.pop(0)
    elif changed == "content":
        adoption.image[0] = entry, "new-content", path
    else:
        adoption.image[0] = entry, content, "samples/new-source.wav"

    adoption.helper.poll()
    adoption.helper.poll()

    assert not adoption.helper.contains(0)
    assert adoption.helper.status(0) is None
    assert predecessor.cancellations == candidate.cancellations == receipt.cancellations == 1
    assert predecessor.reader_active
    assert candidate.reader_active
    assert receipt.reader_active


def test_disk_currentness_never_revokes_retained_source_authority(adoption: _Harness) -> None:
    predecessor = adoption.effective_v1()
    receipt = _OwnReceipt()
    adoption.audio.receipts.append(receipt)
    adoption.bind("c", active=True)
    assert adoption.helper.request(0, "all_stems", 15)
    _, content, path = adoption.image[0]
    adoption.image[0] = _entry("d"), content, path

    adoption.helper.poll()

    assert predecessor.current
    assert predecessor.cancellations == 0
    assert receipt.withdrawals == 1
    assert adoption.helper.contains(0)
    assert adoption.helper.request(0, "full_mix", 15)
    assert adoption.audio.admissions[-1] == (predecessor, "full_mix", 15)


def test_inactive_source_binding_and_active_same_set_keep_existing_routes(
    adoption: _Harness,
) -> None:
    predecessor = adoption.effective_v1()
    same = adoption.bind("b", active=True)
    assert same.pair is not None
    assert len(adoption.audio.registrations) == 1
    assert not adoption.audio.candidates
    assert predecessor.cancellations == 0

    adoption.bind("c", active=False, source="2", path="samples/new-source.wav")

    assert len(adoption.audio.registrations) == 2
    assert not adoption.audio.candidates
    assert predecessor.cancellations == 1


def test_stopped_same_source_replacement_retains_v1_and_uses_candidate_route(
    adoption: _Harness,
) -> None:
    predecessor = adoption.effective_v1()
    ready = _OwnReceipt(residency="accepted", effective="waiting", owned_claim=True)
    adoption.audio.receipts.append(ready)

    adoption.bind("c", active=False)
    candidate = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.helper.poll()

    assert len(adoption.audio.registrations) == 1
    assert predecessor.current
    assert predecessor.cancellations == 0
    assert adoption.audio.admissions[-1] == (candidate, "all_stems", 15)
    status = adoption.helper.status(0)
    assert status is not None
    assert status.disk_stem_set_identity == status.control_stem_set_identity == "c" * 64
    assert status.effective_stem_set_identity == "b" * 64
    assert status.effective == "waiting"


def test_another_pads_own_ack_never_promotes_this_pad(adoption: _Harness) -> None:
    adoption.effective_v1()
    adoption.effective_v1(sample_id=37)
    first = _OwnReceipt(residency="accepted", effective="effective", owned_claim=True)
    second = _OwnReceipt()
    adoption.audio.receipts.extend((first, second))
    adoption.bind("c", active=True)
    adoption.bind("c", active=True, sample_id=37)
    assert adoption.helper.request(0, "all_stems", 15)
    assert adoption.helper.request(37, "all_stems", 15)

    adoption.helper.poll()

    first_status, second_status = adoption.helper.status(0), adoption.helper.status(37)
    assert first_status is not None
    assert second_status is not None
    assert first_status.effective_stem_set_identity == "c" * 64
    assert second_status.effective_stem_set_identity == "b" * 64
    assert second_status.effective == "waiting"


@pytest.mark.parametrize("effective", ["waiting", "effective"])
def test_accepted_v2_launch_loss_needs_explicit_same_all_receipt(
    adoption: _Harness, effective: str
) -> None:
    predecessor = adoption.effective_v1()
    obsolete = _OwnReceipt(residency="accepted", effective=effective, owned_claim=True)
    fresh = _OwnReceipt()
    adoption.audio.receipts.extend((obsolete, fresh))
    adoption.bind("c", active=False)
    installed = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.helper.poll()
    status = adoption.helper.status(0)
    assert status is not None
    assert status.control_stem_set_identity == "c" * 64
    adoption.errors.clear()

    # Declared Native launch loss, rather than an actual transport/audio proof:
    # accepted remains true, but a new Play cannot borrow the old output receipt.
    obsolete.current = False
    assert obsolete.claimed()
    assert obsolete.residency_status() == "accepted"
    assert obsolete.effective_mode_status() == "rejected"
    assert obsolete.effective_failure_reason() is None
    for _ in range(3):
        adoption.helper.poll()

    status = adoption.helper.status(0)
    assert status is not None
    assert status.registration == "accepted"
    assert status.disk_stem_set_identity == status.control_stem_set_identity == "c" * 64
    assert status.residency is status.effective is status.requested_mode is None
    assert status.effective_stem_set_identity is status.attempt_stem_set_identity is None
    assert status.desired_mode is None
    assert not adoption.errors
    assert len(adoption.audio.admissions) == 2
    assert predecessor.cancellations == installed.cancellations == obsolete.cancellations == 0
    assert obsolete.withdrawals == 0
    assert obsolete.reader_active
    assert installed.reader_active
    assert installed.current

    assert adoption.helper.request(0, "all_stems", 15)
    assert adoption.audio.admissions[-1] == (installed, "all_stems", 15)
    assert len(adoption.audio.admissions) == 3
    assert not adoption.audio.receipts
    status = adoption.helper.status(0)
    assert status is not None
    assert status.residency == "pending"
    assert status.effective == "waiting"
    assert status.effective_stem_set_identity is None
    assert not fresh.claimed()
    assert not adoption.errors
    adoption.helper.poll()
    assert len(adoption.audio.admissions) == 3


def test_v2_launch_loss_before_first_poll_keeps_callback_installed_control(
    adoption: _Harness,
) -> None:
    predecessor = adoption.effective_v1()
    installed_receipt = _OwnReceipt()
    adoption.audio.receipts.append(installed_receipt)
    adoption.bind("c", active=False)
    installed = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)

    # Callback ACK and then Play occur before controller observation, so the
    # pending selection is still marked candidate in the controller.
    installed_receipt.residency = "accepted"
    installed_receipt.owned_claim = True
    installed_receipt.current = False
    adoption.helper.poll()
    adoption.helper.poll()

    status = adoption.helper.status(0)
    assert status is not None
    assert status.control_stem_set_identity == status.disk_stem_set_identity == "c" * 64
    assert status.effective_stem_set_identity is None
    assert status.residency is status.effective is status.requested_mode is None
    assert status.desired_mode is None
    assert len(adoption.audio.admissions) == 2
    assert not adoption.errors
    assert predecessor.cancellations == installed.cancellations == 0
    assert installed_receipt.cancellations == installed_receipt.withdrawals == 0
    assert installed.current
    assert installed.reader_active
    assert installed_receipt.reader_active

    # Retained normal control authority routes a fresh explicit FullMix through
    # V2, even though its earlier stopped output observation was withdrawn.
    assert adoption.helper.request(0, "full_mix", 15)
    assert adoption.audio.admissions[-1] == (installed, "full_mix", 15)
    assert len(adoption.audio.admissions) == 3


@pytest.mark.parametrize(("mode", "mask"), [("all_stems", 15), ("all_stems", 5), ("full_mix", 9)])
def test_obsolete_installed_v2_preserves_newer_desired_intent_and_target(
    adoption: _Harness, mode: str, mask: int
) -> None:
    predecessor = adoption.effective_v1()
    second = _OwnReceipt(owned_claim=True)
    latest = _OwnReceipt(owned_claim=True)
    adoption.audio.receipts.extend((second, latest))
    adoption.bind("c", active=True)
    installed = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.bind("d", active=True)
    assert adoption.helper.request(0, cast("StemMixMode", mode), mask)
    assert len(adoption.audio.admissions) == 2

    # V2's claim delayed V3 and the explicit new intent. A later callback ACK
    # followed by Play revokes only V2's observation, not that queued gesture.
    second.residency = "accepted"
    second.current = False
    adoption.helper.poll()
    expected = adoption.audio.candidates[-1] if mode == "all_stems" else installed
    assert adoption.audio.admissions[-1] == (expected, mode, mask)
    assert len(adoption.audio.admissions) == 3
    assert len(adoption.audio.candidates) == (2 if mode == "all_stems" else 1)
    assert latest.claimed()
    assert latest.current
    for _ in range(3):
        adoption.helper.poll()

    status = adoption.helper.status(0)
    assert status is not None
    assert status.control_stem_set_identity == "c" * 64
    assert status.disk_stem_set_identity == "d" * 64
    assert status.attempt_stem_set_identity == ("d" if mode == "all_stems" else "c") * 64
    assert status.requested_mode == status.desired_mode == mode
    assert status.residency == "pending"
    assert status.effective == "waiting"
    assert status.effective_stem_set_identity is None
    assert len(adoption.audio.admissions) == 3
    assert not adoption.errors
    assert predecessor.cancellations == installed.cancellations == 0
    assert second.cancellations == latest.cancellations == 0
    assert latest.withdrawals == 0


def test_native_preparation_failure_is_not_soft_cleared_after_launch_loss(
    adoption: _Harness,
) -> None:
    predecessor = adoption.effective_v1()
    failed = _OwnReceipt(residency="accepted", effective="transitioning", owned_claim=True)
    adoption.audio.receipts.append(failed)
    adoption.bind("c", active=True)
    installed = adoption.audio.candidates[-1]
    assert adoption.helper.request(0, "all_stems", 15)
    adoption.helper.poll()
    failed.current = False
    failed.failure_reason = "native-preparation-failed"
    failed.message = "Committed stem effective mode Native preparation failed"
    for _ in range(3):
        adoption.helper.poll()

    status = adoption.helper.status(0)
    assert status is not None
    assert status.control_stem_set_identity == "c" * 64
    assert status.residency == "accepted"
    assert status.effective == "rejected"
    assert status.requested_mode == status.desired_mode == "all_stems"
    assert adoption.errors[-1] == (0, failed.message)
    assert len(adoption.audio.admissions) == 2
    assert predecessor.cancellations == installed.cancellations == failed.cancellations == 0
    assert failed.withdrawals == 0
    assert failed.reader_active
