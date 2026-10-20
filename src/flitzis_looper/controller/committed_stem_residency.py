"""Track source-bound Native selection receipts independently from saved stem intent."""

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import StemCacheEntry, StemMixMode
    from flitzis_looper_audio import (
        AudioEngine,
        CommittedStemSetTicket,
        PreparedSourceTicket,
        PreparedStemPair,
        ResidentStemTicket,
    )


def resident_pcm_budget_pressure(message: object) -> bool:
    """Recognize the Native resource codec without treating other failures as retryable."""
    return isinstance(message, str) and message.startswith("[resident_pcm_budget_exceeded]")


def resident_stem_capacity_pressure(message: object) -> bool:
    """Recognize only declared worker capacity and same-pad interest-busy protocols."""
    return isinstance(message, str) and (
        message
        in {
            "cold source queue full (2 workers, 32 queued jobs)",
            "Stem queue full (2 workers, 32 queued jobs)",
        }
        or message.startswith("[resident_stem_interest_busy]")
    )


def resident_stem_admission_pressure(message: object) -> bool:
    """Keep recoverable admission separate from stale/data/history/terminal failures."""
    return (
        resident_pcm_budget_pressure(message)
        or resident_stem_capacity_pressure(message)
        or (isinstance(message, str) and message.startswith("[resident_stem_placement_pending]"))
    )


@dataclass(frozen=True, slots=True)
class StemResidencyStatus:
    """Separate disk registration, residency and effective-mode projections."""

    registration: str
    residency: str | None
    effective: str | None
    requested_mode: StemMixMode | None
    disk_stem_set_identity: str | None = None
    control_stem_set_identity: str | None = None
    effective_stem_set_identity: str | None = None
    attempt_stem_set_identity: str | None = None
    desired_mode: StemMixMode | None = None
    attempt_failed: bool = False
    demand: str | None = None
    demand_error: str | None = None


@dataclass(frozen=True, slots=True)
class WithdrawnStemIntent:
    """One accepted operation's retired implicit intent, without Native ownership."""

    sample_id: int
    content_id: str | None
    source_path: str | None
    source_version: str
    stem_set_identity: str | None
    mode: StemMixMode
    enabled_mask: int


@dataclass(slots=True)
class _CommittedSelection:
    entry: StemCacheEntry
    content_id: str | None
    source_path: str | None
    ticket: CommittedStemSetTicket
    desired: tuple[StemMixMode, int] | None = None
    receipt: ResidentStemTicket | None = None
    receipt_intent: tuple[StemMixMode, int] | None = None
    candidate: bool = False
    preload_receipt: ResidentStemTicket | None = None
    preload_enabled: bool | None = None


@dataclass(slots=True)
class _DiskCandidate:
    entry: StemCacheEntry
    content_id: str | None
    source_path: str | None
    prepared: PreparedStemPair
    source_ticket: PreparedSourceTicket
    selection: _CommittedSelection | None = None
    registration: str = "deferred"


@dataclass(slots=True)
class _LiveSelection:
    disk: _DiskCandidate
    control: _CommittedSelection | None = None
    effective_identity: str | None = None
    attempt: _CommittedSelection | None = None
    desired: tuple[StemMixMode, int] | None = None
    blocked: bool = False


class CommittedStemResidency:
    """Coalesce control intent while requiring each selection's own Native receipts."""

    def __init__(
        self,
        audio: AudioEngine,
        current: Callable[[int, StemCacheEntry, str | None, str | None], bool],
        report_error: Callable[[int, str | None], None],
        *,
        source_current: Callable[[int, StemCacheEntry, str | None, str | None], bool] | None = None,
    ) -> None:
        self._audio = audio
        self._current = current
        self._source_current = source_current or current
        self._report_error = report_error
        self._selections: dict[int, _CommittedSelection] = {}
        self._live_selections: dict[int, _LiveSelection] = {}

    @property
    def supported(self) -> bool:
        """Return whether this connected Native runtime supplies both sealed APIs."""
        return callable(getattr(self._audio, "register_committed_stem_pair", None)) and callable(
            getattr(self._audio, "request_committed_stem_residency", None)
        )

    @property
    def pending_supported(self) -> bool:
        """Return whether Native can register a candidate without revoking its predecessor."""
        return self.supported and callable(
            getattr(self._audio, "register_pending_committed_stem_pair", None)
        )

    def contains(self, sample_id: int) -> bool:
        """Return whether the pad has a tracked current disk selection receipt."""
        return sample_id in self._selections or sample_id in self._live_selections

    def registered_selection_current(self, sample_id: int, entry: StemCacheEntry) -> bool:
        """Project verified disk eligibility from the exact current registration owner."""
        return self._current_registered_selection(sample_id, entry) is not None

    def _current_registered_selection(
        self, sample_id: int, entry: StemCacheEntry
    ) -> _CommittedSelection | None:
        live = self._live_selections.get(sample_id)
        selection = live.disk.selection if live is not None else self._selections.get(sample_id)
        if (
            selection is None
            or selection.entry.model_dump(exclude={"available"})
            != entry.model_dump(exclude={"available"})
            or not self._current(
                sample_id, selection.entry, selection.content_id, selection.source_path
            )
            or selection.ticket.publication_status() != "accepted"
            or not selection.ticket.is_current()
        ):
            return None
        return selection

    def recovered_selection_ready(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        *,
        mode: StemMixMode,
        enabled_mask: int,
        preload: bool,
    ) -> bool:
        """Require the exact restored disk owner and its own demanded current ACKs."""
        live = self._live_selections.get(sample_id)
        selection = self._current_registered_selection(sample_id, entry)
        if selection is None:
            return False
        if mode == "all_stems":
            observed = (live.attempt or live.control) if live is not None else selection
            if (
                observed is None
                or observed.ticket is not selection.ticket
                or observed.receipt is None
            ):
                return False
            receipt = observed.receipt
            if (
                observed.receipt_intent != (mode, enabled_mask)
                or not receipt.is_current()
                or receipt.residency_status() != "accepted"
                or receipt.effective_failure_reason() == "native-preparation-failed"
            ):
                return False
        if preload:
            preload_receipt = selection.preload_receipt
            if (
                selection.preload_enabled is not True
                or preload_receipt is None
                or not preload_receipt.is_current()
                or preload_receipt.residency_status() != "accepted"
            ):
                return False
        return True

    def _pair_control_rows(
        self, sample_id: int
    ) -> tuple[_CommittedSelection | _DiskCandidate | _LiveSelection, ...]:
        """Capture exact surviving rows without cancelling or registering Native owners."""
        rows: list[_CommittedSelection | _DiskCandidate | _LiveSelection] = []
        selected = self._selections.get(sample_id)
        if selected is not None:
            rows.append(selected)
        live = self._live_selections.get(sample_id)
        if live is not None:
            rows.extend((live, live.disk))
            for selection in (live.disk.selection, live.control, live.attempt):
                if selection is not None and all(selection is not row for row in rows):
                    rows.append(selection)
        return tuple(rows)

    def adopt_copied_selection(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        content_id: str | None,
        source_path: str | None,
        ticket: CommittedStemSetTicket,
    ) -> None:
        """Track the copied target's own accepted registration without resubmitting it."""
        binding_current = (
            entry.pair is not None
            and content_id is not None
            and source_path is not None
            and self._current(sample_id, entry, content_id, source_path)
        )
        if (
            not binding_current
            or ticket.publication_status() != "accepted"
            or ticket.is_current() is not True
        ):
            message = "Copied stem selection has no current accepted target registration"
            raise RuntimeError(message)
        previous = self._selections.get(sample_id)
        if sample_id in self._live_selections or (
            previous is not None and not self._matches(previous, entry, content_id, source_path)
        ):
            message = "Copied stem selection conflicts with an existing target control owner"
            raise RuntimeError(message)
        if previous is not None:
            previous.entry = entry
            return
        self._selections[sample_id] = _CommittedSelection(entry, content_id, source_path, ticket)

    @staticmethod
    def prepare_copied_selection(
        entry: StemCacheEntry,
        content_id: str | None,
        source_path: str | None,
        ticket: CommittedStemSetTicket,
    ) -> _CommittedSelection:
        """Prepare a fresh copied row without consulting or writing a live slot.

        The aggregate owner separately validates its opaque Native ACK, complete
        target image and the ticket obtained from that parent's ordered receipt.
        Registration does not establish resident component availability.
        """
        row = _CommittedSelection(entry, content_id, source_path, ticket)
        CommittedStemResidency.validate_copied_selection(row)
        return row

    @staticmethod
    def validate_copied_selection(row: _CommittedSelection) -> None:
        """Recheck the exact accepted registration before an aggregate mirror store."""
        if (
            row.entry.pair is None
            or not row.content_id
            or not row.source_path
            or row.ticket.publication_status() != "accepted"
            or row.ticket.is_current() is not True
        ):
            message = "Copied stem selection has no current accepted target registration"
            raise RuntimeError(message)

    def requires_current_preparation(
        self, sample_id: int, source_ticket: PreparedSourceTicket
    ) -> bool:
        """Allow explicit same-source refresh when sealed preparation context changed."""
        live = self._live_selections.get(sample_id)
        if live is None or self._busy(live.attempt):
            return False
        previous = live.disk.source_ticket
        compare = getattr(previous, "same_preparation_context", None)
        return (
            callable(compare)
            and previous.same_source_request(source_ticket)
            and not compare(source_ticket)
        )

    def register(
        self,
        sample_id: int,
        prepared: PreparedStemPair,
        source_ticket: PreparedSourceTicket,
        entry: StemCacheEntry,
        content_id: str | None,
        source_path: str | None,
        *,
        active: bool = False,
    ) -> None:
        """Bind a verified committed disk set; this alone grants no live selection."""
        previous = self._selections.get(sample_id)
        if (
            previous is not None
            and self._matches(previous, entry, content_id, source_path)
            and previous.ticket.is_current()
            and previous.ticket.publication_status() != "rejected"
        ):
            previous.entry = entry
            return
        disk = _DiskCandidate(entry, content_id, source_path, prepared, source_ticket)
        if self.pending_supported and (
            active
            or sample_id in self._live_selections
            or (previous is not None and self._same_source(previous, disk))
        ):
            try:
                self._register_live(sample_id, disk)
            except RuntimeError, TypeError, ValueError:
                live = self._live_selections.get(sample_id)
                if live is not None:
                    live.blocked = True
                raise
            return
        ticket = self._audio.register_committed_stem_pair(prepared, source_ticket)
        if previous is not None:
            self._cancel(previous)
        self._selections[sample_id] = _CommittedSelection(entry, content_id, source_path, ticket)

    def request(self, sample_id: int, mode: StemMixMode, enabled_mask: int) -> bool:
        """Request the latest mode, waiting for registration before Native admission."""
        live = self._live_selections.get(sample_id)
        if live is not None:
            return self._request_live(sample_id, live, (mode, enabled_mask))
        selection = self._selections[sample_id]
        if not self._current(
            sample_id, selection.entry, selection.content_id, selection.source_path
        ):
            self.cancel(sample_id)
            return False
        selection.desired = mode, enabled_mask
        try:
            if selection.ticket.publication_status() == "rejected":
                self._report_error(
                    sample_id, selection.ticket.error() or "Committed stem selection rejected"
                )
                self.cancel(sample_id)
                return False
            if not selection.ticket.is_current():
                self.cancel(sample_id)
                return False
            if selection.receipt is not None and (
                not selection.receipt.is_current()
                or selection.receipt.residency_status() == "rejected"
                or selection.receipt.effective_mode_status() == "rejected"
            ):
                selection.receipt_intent = None
            return self._submit(selection)
        except (RuntimeError, TypeError, ValueError) as error:
            self._selection_admission_error(sample_id, selection, error, context="admission")
            return False

    def request_preload(self, sample_id: int, *, enabled: bool) -> bool:
        """Change only Native policy interest, preserving explicit mode and actual readers."""
        live = self._live_selections.get(sample_id)
        if live is not None and live.disk.selection is None:
            self._register_candidate(live)
        selection = live.disk.selection if live is not None else self._selections.get(sample_id)
        if (
            selection is None
            or not self._current(
                sample_id, selection.entry, selection.content_id, selection.source_path
            )
            or not selection.ticket.is_current()
            or selection.ticket.publication_status() != "accepted"
        ):
            return False
        receipt = selection.preload_receipt
        if selection.preload_enabled == enabled and receipt is not None:
            if receipt.is_current() and receipt.residency_status() != "rejected":
                return True
            if not resident_stem_admission_pressure(receipt.error()):
                self._report_error(sample_id, receipt.error() or "Stem preload context changed")
                return False
        try:
            selection.preload_receipt = self._audio.request_committed_stem_residency(
                selection.ticket,
                mode="full_mix",
                interest="preload" if enabled else "withdraw_preload",
            )
            selection.preload_enabled = enabled
        except (RuntimeError, TypeError, ValueError) as error:
            self._report_error(sample_id, str(error))
            return False
        return True

    def preload_status(self, sample_id: int) -> tuple[str | None, str | None]:
        """Return the policy operation's own residency ACK independently of audio mode."""
        live = self._live_selections.get(sample_id)
        selection = live.disk.selection if live is not None else self._selections.get(sample_id)
        receipt = selection.preload_receipt if selection is not None else None
        if receipt is None:
            return None, None
        if not receipt.is_current():
            return "rejected", receipt.error() or "Stem preload context changed"
        return receipt.residency_status(), receipt.error()

    def component_residency_current(self, sample_id: int) -> bool:
        """Confirm the matching selection's own current ACK without lending an old receipt."""
        live = self._live_selections.get(sample_id)
        selection = (
            (live.attempt or live.control) if live is not None else self._selections.get(sample_id)
        )
        if selection is None or selection.receipt is None:
            return False
        return (
            self._current(sample_id, selection.entry, selection.content_id, selection.source_path)
            and selection.ticket.is_current()
            and selection.receipt.is_current()
            and selection.receipt_intent is not None
            and selection.receipt_intent[0] == "all_stems"
            and selection.receipt.residency_status() == "accepted"
            and selection.receipt.effective_failure_reason() != "native-preparation-failed"
        )

    def poll(
        self, *, deferred_sample_ids: frozenset[int] = frozenset()
    ) -> tuple[WithdrawnStemIntent, ...]:
        """Observe bounded own receipts; polling never advances an audio timeline."""
        withdrawn: list[WithdrawnStemIntent] = []
        self._poll_live_selections(withdrawn, deferred_sample_ids=deferred_sample_ids)
        ready = tuple(
            (sample_id, selection)
            for sample_id, selection in self._selections.items()
            if sample_id not in deferred_sample_ids
        )
        for sample_id, selection in ready:
            try:
                if not self._current(
                    sample_id, selection.entry, selection.content_id, selection.source_path
                ):
                    self.cancel(sample_id)
                    continue
                if selection.ticket.publication_status() == "rejected":
                    self._report_error(
                        sample_id,
                        selection.ticket.error() or "Committed stem selection rejected",
                    )
                    continue
                if not selection.ticket.is_current():
                    self.cancel(sample_id)
                    continue
                intent = selection.receipt_intent
                implicit_intent = intent is not None and selection.desired == intent
                if self._withdraw_accepted_receipt(selection):
                    if implicit_intent and intent is not None:
                        withdrawn.append(self._withdrawn_intent(sample_id, selection, intent))
                    continue
                self._submit(selection)
                receipt = selection.receipt
                if receipt is not None and (
                    not receipt.is_current()
                    or receipt.residency_status() == "rejected"
                    or receipt.effective_mode_status() == "rejected"
                ):
                    self._report_error(sample_id, receipt.error() or "Stem mode adoption rejected")
                elif receipt is not None and receipt.effective_mode_status() == "effective":
                    self._report_error(sample_id, None)
            except (RuntimeError, TypeError, ValueError) as error:
                self._selection_admission_error(sample_id, selection, error, context="status")
        return tuple(withdrawn)

    def _selection_admission_error(
        self,
        sample_id: int,
        selection: _CommittedSelection,
        error: RuntimeError | TypeError | ValueError,
        *,
        context: str,
    ) -> None:
        message = str(error)
        selection.desired = None
        self._report_error(
            sample_id,
            message
            if resident_stem_admission_pressure(message)
            else f"Stem residency {context} failed: {message}",
        )

    def status(self, sample_id: int) -> StemResidencyStatus | None:
        """Return actual registration/residency/effective status without inferring ACK."""
        selection = self._selections.get(sample_id)
        live = self._live_selections.get(sample_id)
        if live is not None:
            return self._live_status(live)
        if selection is None:
            return None
        receipt = selection.receipt
        effective = None
        if receipt is not None:
            if receipt.effective_failure_reason() == "native-preparation-failed":
                effective = "rejected"
            elif receipt.is_current():
                effective = receipt.effective_mode_status()
        return StemResidencyStatus(
            selection.ticket.publication_status(),
            receipt.residency_status() if receipt else None,
            effective,
            selection.receipt_intent[0] if selection.receipt_intent else None,
            self._identity(selection),
            self._identity(selection),
            self._effective_identity(selection),
            self._identity(selection) if receipt else None,
            selection.desired[0] if selection.desired else None,
        )

    def cancel(self, sample_id: int) -> None:
        """Revoke future adoption without releasing actual callback or worker readers."""
        selection = self._selections.pop(sample_id, None)
        if selection is not None:
            self._cancel(selection)
        live = self._live_selections.pop(sample_id, None)
        if live is not None:
            selections = (live.disk.selection, live.control, live.attempt)
            for index, owned in enumerate(selections):
                if owned is not None and all(owned is not other for other in selections[:index]):
                    self._cancel(owned)

    def shut_down(self) -> None:
        """Withdraw all tracked requests; Native owners remain until actual last use."""
        for sample_id in tuple(self._selections.keys() | self._live_selections.keys()):
            self.cancel(sample_id)

    def _register_live(self, sample_id: int, disk: _DiskCandidate) -> None:
        live = self._live_selections.get(sample_id)
        if live is not None and not self._same_source(live.disk, disk):
            self.cancel(sample_id)
            live = None
        if live is not None:
            self._observe_live(sample_id, live)
        if live is not None and self._reuse_live_disk(live, disk):
            return
        previous = self._selections.get(sample_id) if live is None else live.control
        if previous is not None and not self._same_source(previous, disk):
            self.cancel(sample_id)
            previous, live = None, None
        if live is None:
            self._selections.pop(sample_id, None)
            attempt = (
                previous
                if (
                    previous is not None
                    and previous.receipt is not None
                    and not self._failed(previous.receipt)
                    and (previous.receipt.residency_status() != "accepted" or self._busy(previous))
                )
                else None
            )
            live = _LiveSelection(
                disk,
                previous,
                self._effective_identity(previous) if previous else None,
                attempt=attempt,
                desired=attempt.desired if attempt is not None else None,
            )
            self._live_selections[sample_id] = live
        else:
            self._replace_disk(live, disk)
        live.blocked = False
        self._register_candidate(live)

    def _reuse_live_disk(self, live: _LiveSelection, disk: _DiskCandidate) -> bool:
        if not self._same_disk(live.disk, disk):
            return False
        live.disk.entry = disk.entry
        selection = live.disk.selection
        if selection is not None:
            selection.entry = disk.entry
            self._reset_failed_candidate(live)
            if live.disk.selection is None:
                live.disk.prepared = disk.prepared
                live.disk.source_ticket = disk.source_ticket
                live.blocked = False
                self._register_candidate(live)
        else:
            live.disk.prepared = disk.prepared
            live.disk.source_ticket = disk.source_ticket
        return True

    def _replace_disk(self, live: _LiveSelection, disk: _DiskCandidate) -> None:
        previous = live.disk.selection
        attempt = live.attempt
        if attempt is not None and attempt.candidate and self._withdraw(attempt):
            live.attempt = None
        if previous is not None and previous is not live.control and previous is not live.attempt:
            previous.ticket.cancel()
        live.disk = disk

    def _register_candidate(self, live: _LiveSelection) -> None:
        if live.disk.selection is not None or self._busy(live.attempt):
            return
        disk = live.disk
        ticket = self._audio.register_pending_committed_stem_pair(disk.prepared, disk.source_ticket)
        disk.selection = _CommittedSelection(
            disk.entry, disk.content_id, disk.source_path, ticket, candidate=True
        )

    def _request_live(
        self, sample_id: int, live: _LiveSelection, intent: tuple[StemMixMode, int]
    ) -> bool:
        if not self._source_current(
            sample_id, live.disk.entry, live.disk.content_id, live.disk.source_path
        ):
            self.cancel(sample_id)
            return False
        try:
            self._observe_live(sample_id, live)
            live.desired = intent
            live.blocked = False
            self._reset_failed_attempt(live)
            self._reset_failed_candidate(live)
            if not self._withdraw_superseded_attempt(live, intent):
                return True
            return self._submit_live(sample_id, live)
        except (RuntimeError, TypeError, ValueError) as error:
            live.blocked = True
            self._report_error(
                sample_id,
                str(error)
                if resident_stem_admission_pressure(str(error))
                else f"Stem residency admission failed: {error}",
            )
            return False

    @staticmethod
    def _superseded_attempt(
        live: _LiveSelection,
        attempt: _CommittedSelection,
        intent: tuple[StemMixMode, int],
    ) -> bool:
        target = live.disk.selection if intent[0] == "all_stems" else live.control
        return attempt.receipt_intent != intent or attempt is not target

    def _withdraw_superseded_attempt(
        self, live: _LiveSelection, intent: tuple[StemMixMode, int]
    ) -> bool:
        """Yield only this unclaimed operation; claimed readers stay with their receipt."""
        attempt = live.attempt
        if attempt is None or not self._superseded_attempt(live, attempt, intent):
            return True
        if not self._withdraw(attempt):
            return False
        live.attempt = None
        if attempt.candidate:
            attempt.ticket.cancel()
            if live.disk.selection is attempt:
                live.disk.selection = None
                live.disk.registration = "withdrawn"
        return True

    @staticmethod
    def _reset_failed_attempt(live: _LiveSelection) -> None:
        attempt = live.attempt
        if (
            attempt is not None
            and attempt.receipt is not None
            and (
                not attempt.receipt.is_current()
                or attempt.receipt.residency_status() == "rejected"
                or attempt.receipt.effective_mode_status() == "rejected"
            )
        ):
            attempt.receipt_intent = None
            live.attempt = None
            if attempt.candidate and not attempt.ticket.is_current():
                attempt.ticket.cancel()
                if live.disk.selection is attempt:
                    live.disk.selection = None

    @staticmethod
    def _reset_failed_candidate(live: _LiveSelection) -> None:
        selection = live.disk.selection
        if (
            selection is not None
            and selection.candidate
            and (
                not selection.ticket.is_current()
                or selection.ticket.publication_status() == "rejected"
            )
        ):
            selection.ticket.cancel()
            live.disk.selection = None

    def _poll_live_selections(
        self,
        withdrawn: list[WithdrawnStemIntent],
        *,
        deferred_sample_ids: frozenset[int] = frozenset(),
    ) -> None:
        for sample_id, live in tuple(self._live_selections.items()):
            if sample_id in deferred_sample_ids:
                continue
            try:
                if not self._source_current(
                    sample_id, live.disk.entry, live.disk.content_id, live.disk.source_path
                ):
                    self.cancel(sample_id)
                    continue
                if not self._current(
                    sample_id, live.disk.entry, live.disk.content_id, live.disk.source_path
                ):
                    live.desired = None
                    self._withdraw_disk(live)
                self._observe_live(sample_id, live, withdrawn=withdrawn)
                self._submit_live(sample_id, live)
            except (RuntimeError, TypeError, ValueError) as error:
                live.blocked = True
                self._report_error(
                    sample_id,
                    str(error)
                    if resident_stem_admission_pressure(str(error))
                    else f"Stem residency status failed: {error}",
                )

    def _observe_live(
        self,
        sample_id: int,
        live: _LiveSelection,
        *,
        withdrawn: list[WithdrawnStemIntent] | None = None,
    ) -> None:
        selection = live.attempt or live.control
        if selection is None or selection.receipt is None:
            return
        receipt = selection.receipt
        if self._withdraw_accepted_live_receipt(sample_id, live, selection, withdrawn=withdrawn):
            return
        if self._failed(receipt):
            live.blocked = True
            self._report_error(sample_id, receipt.error() or "Stem mode adoption rejected")
            return
        if self._effective_observed(live, selection, receipt):
            return
        if self._busy(selection):
            if receipt.residency_status() == "accepted":
                live.control = selection
                selection.candidate = False
            return
        if not receipt.is_current():
            live.blocked = True
            self._report_error(sample_id, receipt.error() or "Stem mode adoption rejected")
            return
        if receipt.residency_status() == "accepted":
            live.control = selection
            selection.candidate = False
            if receipt.effective_mode_status() == "effective":
                live.effective_identity = self._effective_identity(selection)
                self._report_error(sample_id, None)
            live.attempt = None

    @staticmethod
    def _withdraw_accepted_live_receipt(
        sample_id: int,
        live: _LiveSelection,
        selection: _CommittedSelection,
        *,
        withdrawn: list[WithdrawnStemIntent] | None = None,
    ) -> bool:
        intent = selection.receipt_intent
        target = live.disk.selection if intent and intent[0] == "all_stems" else live.control
        implicit_intent = live.desired == intent and target is selection
        if not CommittedStemResidency._withdraw_accepted_receipt(selection):
            return False
        # The callback installed this set; a later launch invalidates only its
        # output observation. Retain control, without lending the new voice ACK.
        live.control = selection
        selection.candidate = False
        if live.attempt is selection:
            live.attempt = None
        live.effective_identity = None
        if implicit_intent:
            live.desired = None
            if withdrawn is not None and intent is not None:
                withdrawn.append(
                    CommittedStemResidency._withdrawn_intent(sample_id, selection, intent)
                )
        return True

    @staticmethod
    def _withdrawn_intent(
        sample_id: int, selection: _CommittedSelection, intent: tuple[StemMixMode, int]
    ) -> WithdrawnStemIntent:
        return WithdrawnStemIntent(
            sample_id,
            selection.content_id,
            selection.source_path,
            selection.entry.source_version,
            CommittedStemResidency._identity(selection),
            *intent,
        )

    @staticmethod
    def _effective_observed(
        live: _LiveSelection, selection: _CommittedSelection, receipt: ResidentStemTicket
    ) -> bool:
        return (
            live.attempt is None
            and receipt.effective_mode_status() == "effective"
            and live.effective_identity == CommittedStemResidency._effective_identity(selection)
        )

    @staticmethod
    def _failed(receipt: ResidentStemTicket) -> bool:
        return (
            receipt.residency_status() == "rejected"
            or receipt.effective_mode_status() == "rejected"
            or receipt.effective_failure_reason() == "native-preparation-failed"
        )

    def _submit_live(self, sample_id: int, live: _LiveSelection) -> bool:
        # A first live registration can inherit an ordinary pending request.
        # Polling its carried desire must use the same own withdrawal boundary
        # as a later explicit gesture before admitting a superseding operation.
        if (
            live.blocked
            or live.desired is None
            or self._busy(live.attempt)
            or not self._withdraw_superseded_attempt(live, live.desired)
        ):
            return True
        mode, enabled_mask = live.desired
        if mode == "full_mix":
            self._withdraw_disk(live)
            selection = live.control
        else:
            if not self._current(
                sample_id, live.disk.entry, live.disk.content_id, live.disk.source_path
            ):
                return False
            self._register_candidate(live)
            selection = live.disk.selection
        if selection is None:
            return True
        if self._ticket_failed(selection):
            live.blocked = True
            self._report_error(
                sample_id, selection.ticket.error() or "Committed stem selection rejected"
            )
            return False
        if (
            selection.ticket.publication_status() != "accepted"
            or selection.receipt_intent == live.desired
        ):
            return True
        selection.receipt = self._audio.request_committed_stem_residency(
            selection.ticket, mode=mode, enabled_mask=enabled_mask
        )
        selection.receipt_intent = live.desired
        live.attempt = selection
        return True

    @staticmethod
    def _ticket_failed(selection: _CommittedSelection) -> bool:
        return (
            selection.ticket.publication_status() == "rejected" or not selection.ticket.is_current()
        )

    @staticmethod
    def _withdraw_disk(live: _LiveSelection) -> None:
        selection = live.disk.selection
        if selection is None or selection is live.control:
            return
        if selection is live.attempt:
            if not CommittedStemResidency._withdraw(selection):
                return
            live.attempt = None
        selection.ticket.cancel()
        live.disk.selection = None
        live.disk.registration = "withdrawn"

    @staticmethod
    def _withdraw(selection: _CommittedSelection) -> bool:
        receipt = selection.receipt
        if receipt is None:
            return True
        return receipt.withdraw()

    @staticmethod
    def _busy(selection: _CommittedSelection | None) -> bool:
        if selection is None or selection.receipt is None:
            return False
        receipt = selection.receipt
        if not receipt.claimed() or CommittedStemResidency._failed(receipt):
            return False
        return not (
            receipt.residency_status() == "accepted"
            and receipt.effective_mode_status() in {"waiting", "effective", "rejected"}
        )

    def _live_status(self, live: _LiveSelection) -> StemResidencyStatus:
        disk = live.disk.selection
        observed = live.attempt or live.control
        receipt = observed.receipt if observed else None
        effective = None
        if receipt is not None:
            if receipt.effective_failure_reason() == "native-preparation-failed":
                effective = "rejected"
            elif receipt.is_current() or self._busy(observed):
                effective = receipt.effective_mode_status()
        return StemResidencyStatus(
            disk.ticket.publication_status() if disk else live.disk.registration,
            receipt.residency_status() if receipt else None,
            effective,
            observed.receipt_intent[0] if observed and observed.receipt_intent else None,
            self._identity(live.disk),
            self._identity(live.control),
            live.effective_identity,
            self._identity(observed) if receipt else None,
            live.desired[0] if live.desired else None,
            live.blocked,
        )

    @staticmethod
    def _identity(selection: _CommittedSelection | _DiskCandidate | None) -> str | None:
        pair = selection.entry.pair if selection is not None else None
        return pair.stem_set_identity if pair is not None else None

    @staticmethod
    def _effective_identity(selection: _CommittedSelection) -> str | None:
        receipt = selection.receipt
        if (
            receipt is not None
            and receipt.is_current()
            and receipt.effective_mode_status() == "effective"
            and selection.receipt_intent is not None
            and selection.receipt_intent[0] == "all_stems"
        ):
            return CommittedStemResidency._identity(selection)
        return None

    @staticmethod
    def _same_source(selection: _CommittedSelection | _DiskCandidate, disk: _DiskCandidate) -> bool:
        return (
            selection.content_id == disk.content_id
            and selection.source_path == disk.source_path
            and selection.entry.source_version == disk.entry.source_version
        )

    @staticmethod
    def _same_disk(first: _DiskCandidate, second: _DiskCandidate) -> bool:
        return (
            first.content_id == second.content_id
            and first.source_path == second.source_path
            and first.entry.source_version == second.entry.source_version
            and first.entry.pair == second.entry.pair
        )

    def _submit(self, selection: _CommittedSelection) -> bool:
        status = selection.ticket.publication_status()
        if status == "rejected":
            raise RuntimeError(selection.ticket.error() or "Committed stem selection rejected")
        if selection.desired is None or status != "accepted":
            return True
        if selection.receipt_intent == selection.desired:
            return True
        mode, enabled_mask = selection.desired
        receipt = self._audio.request_committed_stem_residency(
            selection.ticket, mode=mode, enabled_mask=enabled_mask
        )
        selection.receipt = receipt
        selection.receipt_intent = selection.desired
        return True

    @staticmethod
    def _withdraw_accepted_receipt(selection: _CommittedSelection) -> bool:
        receipt = selection.receipt
        if (
            receipt is None
            or receipt.residency_status() != "accepted"
            or receipt.is_current()
            or receipt.effective_failure_reason() == "native-preparation-failed"
        ):
            return False
        # A later authorized context invalidates the old operation's observation,
        # not its already-installed readers. Only an explicit gesture requests a
        # fresh live receipt; polling cannot infer output or silently retry.
        selection.receipt = None
        selection.receipt_intent = None
        selection.desired = None
        return True

    @staticmethod
    def _matches(
        selection: _CommittedSelection,
        entry: StemCacheEntry,
        content_id: str | None,
        source_path: str | None,
    ) -> bool:
        return (
            selection.content_id == content_id
            and selection.source_path == source_path
            and selection.entry.source_version == entry.source_version
            and selection.entry.pair == entry.pair
        )

    @staticmethod
    def _cancel(selection: _CommittedSelection) -> None:
        if selection.receipt is not None:
            selection.receipt.cancel()
        if selection.preload_receipt is not None:
            selection.preload_receipt.cancel()
        selection.ticket.cancel()
