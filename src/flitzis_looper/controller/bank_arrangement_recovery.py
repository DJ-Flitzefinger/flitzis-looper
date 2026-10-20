"""Historical whole-bank Config recognition, never reconstructed Native permission."""

import hashlib
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    project_with_journal_bank,
    validated_bank_history,
    written_bank_image_matches,
    written_bank_project_intent,
)
from flitzis_looper.controller.pair_arrangement_journal import validated_project_image
from flitzis_looper.controller.persistence import ConfigCommitReceipt

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState


@dataclass(frozen=True, slots=True)
class BankRecoveryDecision:
    """Detached historical intent; every runtime effect still requires fresh Native custody."""

    disposition: Literal["original_pending", "recognized_after", "conflict", "settled", "cancelled"]
    project_image_json: str | None = None
    receipt: ConfigCommitReceipt | None = None
    error: str | None = None

    def projected(self) -> ProjectState | None:
        """Return independent musical intent without voices, Holds, ACKs or reservations."""
        return (
            None
            if self.project_image_json is None
            else validated_project_image(self.project_image_json)
        )


def inspect_bank_recovery(
    journal: BankArrangementJournal,
    config_bytes: bytes | None,
    *,
    history: list[str],
) -> BankRecoveryDecision:
    """Recognize only complete history and actual selected Config before/after evidence.

    The caller separately verifies selected Config location and retains all source,
    old-target and after-image leases. No routing, writes, Native actions or startup
    settlement happen here. An unchanged before/after Clear image conservatively
    remains original_pending until an actual committed ordinal exists.
    """
    journal = BankArrangementJournal.model_validate(journal)
    try:
        records = _matching_history(journal, history)
    except (TypeError, ValueError) as error:
        return BankRecoveryDecision("conflict", error=f"Unrecognized Bank history: {error}")
    if journal.phase == "settled":
        return BankRecoveryDecision("settled")
    if journal.phase == "cancelled":
        return BankRecoveryDecision("cancelled")
    return _inspect_config(journal, config_bytes, records)


def _matching_history(
    journal: BankArrangementJournal, history: list[str]
) -> list[BankArrangementJournal]:
    records = validated_bank_history(history, journal.transaction_id)
    if records[-1] != journal:
        message = "Bank scan tail differs from its complete opened history"
        raise ValueError(message)
    return records


def _inspect_config(
    journal: BankArrangementJournal,
    config_bytes: bytes | None,
    records: list[BankArrangementJournal],
) -> BankRecoveryDecision:
    digest = hashlib.sha256(config_bytes).hexdigest() if config_bytes is not None else None
    if journal.image_role == "before" and digest == journal.captured_config_sha256:
        return BankRecoveryDecision("original_pending", journal.project_image_json)
    if config_bytes is None:
        return BankRecoveryDecision("conflict", error="Bank Config evidence is missing or changed")
    assert digest is not None
    try:
        text = config_bytes.decode("utf-8")
        current = validated_project_image(text)
        return _inspect_after(journal, current, text, digest, records)
    except (UnicodeDecodeError, TypeError, ValueError) as error:
        return BankRecoveryDecision("conflict", error=f"Unrecognized Bank Config: {error}")


def _inspect_after(
    journal: BankArrangementJournal,
    current: ProjectState,
    text: str,
    digest: str,
    history: list[BankArrangementJournal],
) -> BankRecoveryDecision:
    if journal.image_role == "after" and (
        digest == journal.committed_config_sha256
        and text == journal.project_image_json
        and current.config_revision == journal.committed_revision
    ):
        return BankRecoveryDecision(
            "recognized_after", text, ConfigCommitReceipt(current.config_revision, digest, text)
        )
    if journal.phase == "native_ack_observed":
        before = validated_project_image(journal.project_image_json)
        after = project_with_journal_bank(before, journal)
        after.config_revision = journal.intent_revision
        if current.config_revision == journal.intent_revision and written_bank_project_intent(
            current, journal.bank
        ) == written_bank_project_intent(after, journal.bank):
            return BankRecoveryDecision(
                "recognized_after", text, ConfigCommitReceipt(current.config_revision, digest, text)
            )
        for prior in reversed(history[:-1]):
            if (
                prior.phase == "config_committed"
                and prior.intent_revision < journal.intent_revision
                and prior.committed_config_sha256 == digest
                and prior.project_image_json == text
                and written_bank_image_matches(current, journal.bank, image_role="after")
            ):
                # A recorded newer complete pre-bank image may follow an older
                # actual complete after receipt. Preserve that latest intent;
                # it has no current written receipt until the caller commits it.
                return BankRecoveryDecision("recognized_after", after.model_dump_json())
    return BankRecoveryDecision(
        "conflict", error="Newer, partial or different Bank Config remains current and fenced"
    )
