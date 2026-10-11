"""Control receipts only; real Native readers and DSP continuity are tested in Rust."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock, call

import pytest

from flitzis_looper.controller.committed_stem_residency import CommittedStemResidency
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.models import StemCacheEntry
from flitzis_looper.stem_pair_selection import StemPairSelection

if TYPE_CHECKING:
    from flitzis_looper_audio import AudioEngine, PreparedSourceTicket, PreparedStemPair


@dataclass(slots=True)
class _RegistrationTicket:
    """Replace sealed receipt observation, without simulating Native adoption."""

    state: str = "captured"
    current: bool = True
    message: str | None = None
    status_error: RuntimeError | None = None
    cancellations: int = 0
    reader_active: bool = True

    def publication_status(self) -> str:
        if self.status_error is not None:
            raise self.status_error
        return self.state

    def is_current(self) -> bool:
        return self.current

    def error(self) -> str | None:
        return self.message

    def cancel(self) -> None:
        self.cancellations += 1
        self.current = False


@dataclass(slots=True)
class _ResidencyReceipt:
    """Cancellation withdraws intent; actual read settlement remains independent."""

    residency: str = "pending"
    effective: str = "waiting"
    current: bool = True
    message: str | None = None
    failure_reason: str | None = None
    status_error: RuntimeError | None = None
    cancellations: int = 0
    reader_active: bool = True

    def residency_status(self) -> str:
        if self.status_error is not None:
            raise self.status_error
        return self.residency

    def effective_mode_status(self) -> str:
        if self.status_error is not None:
            raise self.status_error
        return self.effective

    def effective_failure_reason(self) -> str | None:
        return self.failure_reason

    def is_current(self) -> bool:
        return self.current

    def error(self) -> str | None:
        return self.message

    def cancel(self) -> None:
        self.cancellations += 1
        self.current = False


def _entry(*, source: str = "1", generation: str = "b") -> StemCacheEntry:
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
    audio: Mock
    helper: CommittedStemResidency
    image: dict[int, tuple[StemCacheEntry, str | None, str | None]]
    errors: list[tuple[int, str | None]]
    prepared: PreparedStemPair = field(default_factory=lambda: cast("PreparedStemPair", Mock()))
    source_ticket: PreparedSourceTicket = field(
        default_factory=lambda: cast("PreparedSourceTicket", Mock())
    )

    def bind(
        self,
        ticket: _RegistrationTicket,
        *,
        sample_id: int = 0,
        entry: StemCacheEntry | None = None,
        content_id: str = "f" * 32,
        source_path: str = "samples/source.wav",
    ) -> StemCacheEntry:
        entry = _entry() if entry is None else entry
        self.image[sample_id] = entry, content_id, source_path
        self.audio.register_committed_stem_pair.return_value = ticket
        self.helper.register(
            sample_id, self.prepared, self.source_ticket, entry, content_id, source_path
        )
        return entry


@pytest.fixture
def residency() -> _Harness:
    audio = Mock(spec=["register_committed_stem_pair", "request_committed_stem_residency"])
    audio.request_committed_stem_residency.return_value = _ResidencyReceipt()
    image: dict[int, tuple[StemCacheEntry, str | None, str | None]] = {}
    errors: list[tuple[int, str | None]] = []

    def current(
        sample_id: int, entry: StemCacheEntry, content_id: str | None, source_path: str | None
    ) -> bool:
        selected = image.get(sample_id)
        return (
            selected is not None
            and selected[0] is entry
            and selected[1:]
            == (
                content_id,
                source_path,
            )
        )

    helper = CommittedStemResidency(
        cast("AudioEngine", audio),
        current,
        lambda sample_id, message: errors.append((sample_id, message)),
    )
    return _Harness(audio, helper, image, errors)


def test_each_pad_waits_for_its_own_disk_registration(residency: _Harness) -> None:
    first = _RegistrationTicket(state="accepted")
    second = _RegistrationTicket()
    first_receipt = _ResidencyReceipt(residency="accepted", effective="effective")
    second_receipt = _ResidencyReceipt()
    residency.audio.request_committed_stem_residency.side_effect = [first_receipt, second_receipt]
    residency.bind(first)
    residency.bind(second, sample_id=37)

    assert residency.helper.request(0, "all_stems", 15)
    assert residency.helper.request(37, "all_stems", 15)
    residency.helper.poll()
    residency.audio.request_committed_stem_residency.assert_called_once_with(
        first, mode="all_stems", enabled_mask=15
    )
    first_status, second_status = residency.helper.status(0), residency.helper.status(37)
    assert first_status is not None
    assert first_status.effective == "effective"
    assert second_status is not None
    assert second_status.effective is None
    second.state = "accepted"
    residency.helper.poll()
    assert residency.audio.request_committed_stem_residency.call_args_list == [
        call(first, mode="all_stems", enabled_mask=15),
        call(second, mode="all_stems", enabled_mask=15),
    ]
    second_status = residency.helper.status(37)
    assert second_status is not None
    assert second_status.effective == "waiting"


def test_residency_ack_does_not_imply_effective_mode_ack(residency: _Harness) -> None:
    registration = _RegistrationTicket(state="accepted")
    receipt = _ResidencyReceipt(residency="accepted")
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.return_value = receipt
    assert residency.helper.request(0, "all_stems", 15)

    residency.helper.poll()

    status = residency.helper.status(0)
    assert status is not None
    assert (status.registration, status.residency, status.effective, status.requested_mode) == (
        "accepted",
        "accepted",
        "waiting",
        "all_stems",
    )
    receipt.effective = "transitioning"
    residency.helper.poll()
    status = residency.helper.status(0)
    assert status is not None
    assert status.effective == "transitioning"
    assert not residency.errors
    receipt.effective = "effective"
    residency.helper.poll()
    status = residency.helper.status(0)
    assert status is not None
    assert status.effective == "effective"
    assert residency.audio.request_committed_stem_residency.call_count == 1
    assert residency.errors == [(0, None)]


@pytest.mark.parametrize("accepted", [False, True])
def test_full_mix_supersedes_pending_all_intent(residency: _Harness, *, accepted: bool) -> None:
    registration = _RegistrationTicket(state="accepted" if accepted else "captured")
    first, latest = _ResidencyReceipt(), _ResidencyReceipt()
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.side_effect = [first, latest]
    assert residency.helper.request(0, "all_stems", 15)
    assert residency.helper.request(0, "all_stems", 15)
    assert residency.helper.request(0, "full_mix", 15)
    assert residency.helper.request(0, "full_mix", 15)
    registration.state = "accepted"
    residency.helper.poll()
    residency.helper.poll()

    expected = [call(registration, mode="full_mix", enabled_mask=15)]
    if accepted:
        expected.insert(0, call(registration, mode="all_stems", enabled_mask=15))
    assert residency.audio.request_committed_stem_residency.call_args_list == expected
    status = residency.helper.status(0)
    assert status is not None
    assert status.requested_mode == "full_mix"
    if accepted:
        first.residency = "accepted"
        first.effective = "effective"
        residency.helper.poll()
        status = residency.helper.status(0)
        assert status is not None
        assert status.effective == "waiting"


def test_latest_mask_is_coalesced_before_and_after_registration(residency: _Harness) -> None:
    registration = _RegistrationTicket()
    residency.audio.request_committed_stem_residency.side_effect = [
        _ResidencyReceipt(),
        _ResidencyReceipt(),
    ]
    residency.bind(registration)
    assert residency.helper.request(0, "all_stems", 15)
    assert residency.helper.request(0, "all_stems", 2)
    residency.audio.request_committed_stem_residency.assert_not_called()
    registration.state = "accepted"
    residency.helper.poll()
    assert residency.helper.request(0, "all_stems", 2)
    assert residency.helper.request(0, "all_stems", 5)
    assert residency.helper.request(0, "all_stems", 5)
    residency.helper.poll()
    assert residency.audio.request_committed_stem_residency.call_args_list == [
        call(registration, mode="all_stems", enabled_mask=2),
        call(registration, mode="all_stems", enabled_mask=5),
    ]


@pytest.mark.parametrize("changed", ["source", "content", "selection", "native-source"])
@pytest.mark.parametrize("via_request", [False, True])
def test_stale_selection_cannot_admit_or_keep_future_adoption(
    residency: _Harness, changed: str, *, via_request: bool
) -> None:
    registration = _RegistrationTicket(state="accepted")
    receipt = _ResidencyReceipt()
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.return_value = receipt
    assert residency.helper.request(0, "all_stems", 15)
    entry, content_id, source_path = residency.image[0]
    if changed == "source":
        residency.image[0] = entry, content_id, "samples/new-source.wav"
    elif changed == "content":
        residency.image[0] = entry, "e" * 32, source_path
    elif changed == "selection":
        residency.image[0] = _entry(generation="c"), content_id, source_path
    else:
        registration.current = False
    if via_request:
        assert residency.helper.request(0, "all_stems", 2) is False
    else:
        residency.helper.poll()

    assert not residency.helper.contains(0)
    assert residency.helper.status(0) is None
    assert registration.cancellations == receipt.cancellations == 1
    assert registration.reader_active
    assert receipt.reader_active
    assert residency.audio.request_committed_stem_residency.call_count == 1


@pytest.mark.parametrize("shutdown", [False, True])
def test_withdrawal_does_not_claim_native_worker_read_end(
    residency: _Harness, *, shutdown: bool
) -> None:
    registration = _RegistrationTicket(state="accepted")
    receipt = _ResidencyReceipt()
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.return_value = receipt
    assert residency.helper.request(0, "all_stems", 15)

    if shutdown:
        residency.helper.shut_down()
        residency.helper.shut_down()
    else:
        residency.helper.cancel(0)
        residency.helper.cancel(0)
    residency.helper.poll()

    assert not residency.helper.contains(0)
    assert registration.cancellations == receipt.cancellations == 1
    assert registration.reader_active
    assert receipt.reader_active
    assert residency.audio.request_committed_stem_residency.call_count == 1


def test_queue_failure_is_visible_and_needs_explicit_retry(residency: _Harness) -> None:
    registration = _RegistrationTicket(state="accepted")
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.side_effect = RuntimeError("native queue full")

    assert residency.helper.request(0, "all_stems", 15) is False
    error = residency.errors[-1][1]
    assert error is not None
    assert "native queue full" in error
    for _ in range(3):
        residency.helper.poll()
    assert residency.audio.request_committed_stem_residency.call_count == 1
    status = residency.helper.status(0)
    assert status is not None
    assert status.residency is None
    assert status.effective is None
    residency.audio.request_committed_stem_residency.side_effect = None
    assert residency.helper.request(0, "all_stems", 15)
    assert residency.audio.request_committed_stem_residency.call_count == 2


def test_rejected_registration_never_admits_residency(residency: _Harness) -> None:
    registration = _RegistrationTicket(
        state="rejected", current=False, message="source request changed"
    )
    residency.bind(registration)

    residency.helper.poll()

    residency.audio.request_committed_stem_residency.assert_not_called()
    error = residency.errors[-1][1]
    assert error is not None
    assert "source request changed" in error
    status = residency.helper.status(0)
    assert status is not None
    assert status.registration == "rejected"
    assert status.residency is None
    assert status.effective is None
    assert residency.helper.request(0, "all_stems", 15) is False
    assert not residency.helper.contains(0)
    residency.audio.request_committed_stem_residency.assert_not_called()


@pytest.mark.parametrize("rejected_phase", ["residency", "effective"])
def test_terminal_rejection_stays_truthful_until_explicit_same_intent_retry(
    residency: _Harness, rejected_phase: str
) -> None:
    registration = _RegistrationTicket(state="accepted")
    first = _ResidencyReceipt(message="history permit changed")
    second = _ResidencyReceipt()
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.side_effect = [first, second]
    assert residency.helper.request(0, "all_stems", 15)
    setattr(first, rejected_phase, "rejected")

    for _ in range(3):
        residency.helper.poll()
    assert residency.audio.request_committed_stem_residency.call_count == 1
    error = residency.errors[-1][1]
    assert error is not None
    assert "history permit changed" in error
    status = residency.helper.status(0)
    assert status is not None
    assert (status.residency if rejected_phase == "residency" else status.effective) == "rejected"

    assert residency.helper.request(0, "all_stems", 15)
    assert residency.audio.request_committed_stem_residency.call_count == 2
    status = residency.helper.status(0)
    assert status is not None
    assert status.residency == "pending"
    assert status.effective == "waiting"
    assert residency.errors[-1][1] == error
    second.residency = "accepted"
    second.effective = "transitioning"
    residency.helper.poll()
    assert residency.errors[-1][1] == error
    second.effective = "effective"
    residency.helper.poll()
    assert residency.errors[-1] == (0, None)


@pytest.mark.parametrize("failed_status", ["registration", "residency"])
def test_status_exception_is_reported_without_inventing_acceptance(
    residency: _Harness, failed_status: str
) -> None:
    registration = _RegistrationTicket(state="accepted")
    receipt = _ResidencyReceipt()
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.return_value = receipt
    assert residency.helper.request(0, "all_stems", 15)
    target = registration if failed_status == "registration" else receipt
    target.status_error = RuntimeError("native status unavailable")

    residency.helper.poll()

    error = residency.errors[-1][1]
    assert error is not None
    assert "native status unavailable" in error
    assert residency.audio.request_committed_stem_residency.call_count == 1
    assert registration.reader_active
    assert receipt.reader_active
    target.status_error = None
    residency.helper.poll()
    status = residency.helper.status(0)
    assert status is not None
    assert status.effective == "waiting"
    assert residency.audio.request_committed_stem_residency.call_count == 1


@pytest.mark.parametrize("effective", ["waiting", "transitioning", "effective"])
def test_obsolete_accepted_receipt_becomes_unconfirmed_without_automatic_retry(
    residency: _Harness, effective: str
) -> None:
    registration = _RegistrationTicket(state="accepted")
    obsolete = _ResidencyReceipt(
        residency="accepted", effective=effective, message="mode request superseded"
    )
    fresh = _ResidencyReceipt()
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.side_effect = [obsolete, fresh]
    assert residency.helper.request(0, "all_stems", 15)
    obsolete.current = False

    for _ in range(3):
        residency.helper.poll()

    status = residency.helper.status(0)
    assert status is not None
    assert status.residency is None
    assert status.effective is None
    assert status.requested_mode is None
    assert not residency.errors
    assert residency.audio.request_committed_stem_residency.call_count == 1
    assert obsolete.reader_active
    assert registration.cancellations == obsolete.cancellations == 0

    assert residency.helper.request(0, "all_stems", 15)
    status = residency.helper.status(0)
    assert status is not None
    assert status.residency == "pending"
    assert status.effective == "waiting"
    assert residency.audio.request_committed_stem_residency.call_count == 2


def test_known_native_effective_failure_survives_later_context_invalidation(
    residency: _Harness,
) -> None:
    registration = _RegistrationTicket(state="accepted")
    failed = _ResidencyReceipt(
        residency="accepted",
        effective="rejected",
        current=False,
        message="Native preparation failed",
        failure_reason="native-preparation-failed",
    )
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.return_value = failed
    assert residency.helper.request(0, "all_stems", 15)

    for _ in range(3):
        residency.helper.poll()

    status = residency.helper.status(0)
    assert status is not None
    assert status.residency == "accepted"
    assert status.effective == "rejected"
    assert status.requested_mode == "all_stems"
    assert residency.errors[-1] == (0, "Native preparation failed")
    assert residency.audio.request_committed_stem_residency.call_count == 1
    assert failed.reader_active
    assert registration.cancellations == failed.cancellations == 0


def test_stale_pending_receipt_still_reports_failed_admission(residency: _Harness) -> None:
    registration = _RegistrationTicket(state="accepted")
    pending = _ResidencyReceipt(current=False, message="history changed before adoption")
    residency.bind(registration)
    residency.audio.request_committed_stem_residency.return_value = pending
    assert residency.helper.request(0, "all_stems", 15)

    for _ in range(3):
        residency.helper.poll()

    status = residency.helper.status(0)
    assert status is not None
    assert status.residency == "pending"
    assert status.effective is None
    assert status.requested_mode == "all_stems"
    assert residency.errors[-1] == (0, "history changed before adoption")
    assert residency.audio.request_committed_stem_residency.call_count == 1


def test_same_set_binding_is_idempotent_across_equivalent_metadata(residency: _Harness) -> None:
    registration = _RegistrationTicket(state="accepted")
    entry = residency.bind(registration)
    assert residency.helper.request(0, "all_stems", 15)

    residency.bind(_RegistrationTicket(), entry=entry.model_copy(deep=True))
    residency.helper.poll()

    assert residency.helper.contains(0)
    assert residency.audio.register_committed_stem_pair.call_count == 1
    assert residency.audio.request_committed_stem_residency.call_count == 1
    assert registration.cancellations == 0


def test_new_source_rebind_cancels_old_intent_and_requires_new_own_registration(
    residency: _Harness,
) -> None:
    first = _RegistrationTicket(state="accepted")
    receipt = _ResidencyReceipt()
    residency.bind(first)
    residency.audio.request_committed_stem_residency.return_value = receipt
    assert residency.helper.request(0, "all_stems", 15)
    second = _RegistrationTicket()

    residency.bind(second, entry=_entry(source="2"), source_path="samples/new-source.wav")
    assert first.cancellations == receipt.cancellations == 1
    assert first.reader_active
    assert receipt.reader_active
    assert residency.helper.request(0, "all_stems", 15)
    residency.helper.poll()
    assert residency.audio.request_committed_stem_residency.call_count == 1
    second.state = "accepted"
    residency.helper.poll()
    assert residency.audio.register_committed_stem_pair.call_count == 2
    assert residency.audio.request_committed_stem_residency.call_count == 2
    residency.audio.request_committed_stem_residency.assert_called_with(
        second, mode="all_stems", enabled_mask=15
    )
