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


@dataclass(frozen=True, slots=True)
class StemResidencyStatus:
    """Separate disk registration, residency and effective-mode projections."""

    registration: str
    residency: str | None
    effective: str | None
    requested_mode: StemMixMode | None


@dataclass(slots=True)
class _CommittedSelection:
    entry: StemCacheEntry
    content_id: str | None
    source_path: str | None
    ticket: CommittedStemSetTicket
    desired: tuple[StemMixMode, int] | None = None
    receipt: ResidentStemTicket | None = None
    receipt_intent: tuple[StemMixMode, int] | None = None


class CommittedStemResidency:
    """Coalesce control intent while requiring each selection's own Native receipts."""

    def __init__(
        self,
        audio: AudioEngine,
        current: Callable[[int, StemCacheEntry, str | None, str | None], bool],
        report_error: Callable[[int, str | None], None],
    ) -> None:
        self._audio = audio
        self._current = current
        self._report_error = report_error
        self._selections: dict[int, _CommittedSelection] = {}

    @property
    def supported(self) -> bool:
        """Return whether this connected Native runtime supplies both sealed APIs."""
        return callable(getattr(self._audio, "register_committed_stem_pair", None)) and callable(
            getattr(self._audio, "request_committed_stem_residency", None)
        )

    def contains(self, sample_id: int) -> bool:
        """Return whether the pad has a tracked current disk selection receipt."""
        return sample_id in self._selections

    def register(
        self,
        sample_id: int,
        prepared: PreparedStemPair,
        source_ticket: PreparedSourceTicket,
        entry: StemCacheEntry,
        content_id: str | None,
        source_path: str | None,
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
        ticket = self._audio.register_committed_stem_pair(prepared, source_ticket)
        if previous is not None:
            self._cancel(previous)
        self._selections[sample_id] = _CommittedSelection(entry, content_id, source_path, ticket)

    def request(self, sample_id: int, mode: StemMixMode, enabled_mask: int) -> bool:
        """Request the latest mode, waiting for registration before Native admission."""
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
            selection.desired = None
            self._report_error(sample_id, f"Stem residency admission failed: {error}")
            return False

    def poll(self) -> None:
        """Observe bounded own receipts; polling never advances an audio timeline."""
        for sample_id, selection in tuple(self._selections.items()):
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
                if self._withdraw_accepted_receipt(selection):
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
                selection.desired = None
                self._report_error(sample_id, f"Stem residency status failed: {error}")

    def status(self, sample_id: int) -> StemResidencyStatus | None:
        """Return actual registration/residency/effective status without inferring ACK."""
        selection = self._selections.get(sample_id)
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
        )

    def cancel(self, sample_id: int) -> None:
        """Revoke future adoption without releasing actual callback or worker readers."""
        selection = self._selections.pop(sample_id, None)
        if selection is not None:
            self._cancel(selection)

    def shut_down(self) -> None:
        """Withdraw all tracked requests; Native owners remain until actual last use."""
        for sample_id in tuple(self._selections):
            self.cancel(sample_id)

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
        selection.ticket.cancel()
