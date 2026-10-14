"""Fresh-process Pair decisions from complete Config evidence, never saved ACKs."""

import hashlib
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementJournal,
    validated_project_image,
    written_pair_image_matches,
    written_project_intent,
)
from flitzis_looper.controller.persistence import ConfigCommitReceipt
from flitzis_looper.pair_snapshot import project_with_pair_snapshot

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState

type PairRecoveryDisposition = Literal[
    "original_pending",
    "recognized_after",
    "conflict",
    "settled",
    "cancelled",
]


@dataclass(frozen=True)
class PairRecoveryDecision:
    """Detached intent and historical outcome; the caller must acquire fresh Native custody."""

    disposition: PairRecoveryDisposition
    project_image_json: str | None = None
    receipt: ConfigCommitReceipt | None = None
    error: str | None = None

    def projected(self) -> ProjectState | None:
        """Return independent intent without voices, Hold owners or publication permissions."""
        if self.project_image_json is None:
            return None
        return validated_project_image(self.project_image_json)


def inspect_pair_recovery(
    journal: PairArrangementJournal,
    config_bytes: bytes | None,
) -> PairRecoveryDecision:
    """Recognize exact pending intent or a complete actual after image without another Swap.

    The caller MUST first validate the complete Native-opened history and actual
    selected Config location through Native config custody. This function performs
    no writes, Native actions, owner reconstruction or recovery from a scan tail.
    Different/newer Config remains untouched and conflicts remain fenced.
    """
    journal = PairArrangementJournal.model_validate(journal)
    if journal.phase == "settled":
        return PairRecoveryDecision("settled")
    if journal.phase == "cancelled":
        return PairRecoveryDecision("cancelled")
    digest = hashlib.sha256(config_bytes).hexdigest() if config_bytes is not None else None
    if journal.image_role == "before" and digest == journal.captured_config_sha256:
        return PairRecoveryDecision("original_pending", journal.project_image_json)
    if config_bytes is None:
        return PairRecoveryDecision("conflict", error="Pair Config evidence is missing or changed")
    return _inspect_after_config(journal, config_bytes)


def _inspect_after_config(
    journal: PairArrangementJournal, config_bytes: bytes
) -> PairRecoveryDecision:
    try:
        text = config_bytes.decode("utf-8")
        current = validated_project_image(text)
    except (UnicodeDecodeError, TypeError, ValueError) as error:
        return PairRecoveryDecision("conflict", error=f"Unrecognized Pair Config: {error}")
    digest = hashlib.sha256(config_bytes).hexdigest()
    receipt = _recognized_after_receipt(journal, current, text, digest)
    if receipt is not None:
        return PairRecoveryDecision("recognized_after", text, receipt)
    return PairRecoveryDecision(
        "conflict",
        error="Newer or different Pair Config remains current; recovery stays fenced",
    )


def _recognized_after_receipt(
    journal: PairArrangementJournal,
    current: ProjectState,
    text: str,
    digest: str,
) -> ConfigCommitReceipt | None:
    if journal.image_role == "after":
        if (
            digest == journal.committed_config_sha256
            and text == journal.project_image_json
            and current.config_revision == journal.committed_revision
        ):
            return ConfigCommitReceipt(current.config_revision, digest, text)
    elif (
        journal.phase == "native_ack_observed"
        and current.config_revision == journal.intent_revision
    ):
        before = validated_project_image(journal.project_image_json)
        projected = project_with_pair_snapshot(before, journal.before)
        projected.config_revision = journal.intent_revision
        if written_pair_image_matches(current, journal.after) and written_project_intent(
            current
        ) == written_project_intent(projected):
            # Crash after actual replacement but before the successful journal receipt.
            # Complete two-slot/full-project evidence recognizes the already applied Swap.
            return ConfigCommitReceipt(current.config_revision, digest, text)
    return None
