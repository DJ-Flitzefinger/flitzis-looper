"""Recognize occupied Copy intent from actual Config bytes without restoring authority."""

import hashlib
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.copy_arrangement_journal import (
    CopyArrangementJournal,
    project_with_journal_copy,
    written_copy_image_matches,
)
from flitzis_looper.controller.pair_arrangement_journal import (
    validated_project_image,
    written_project_intent,
)
from flitzis_looper.controller.persistence import ConfigCommitReceipt
from flitzis_looper.copy_snapshot import CONTENT_FIELDS

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState


def copy_before_project(project: ProjectState, journal: CopyArrangementJournal) -> ProjectState:
    """Retain latest A/unrelated intent while placing sealed original B before fixed C."""
    image = project.model_copy(deep=True)
    captured = validated_project_image(journal.captured_project_image_json)
    for _, name in CONTENT_FIELDS:
        getattr(image, name)[journal.target_id] = getattr(captured, name)[journal.target_id]
    return image


@dataclass(frozen=True, slots=True)
class CopyRecoveryDecision:
    """Historical disposition; every after image still needs fresh Native acceptance."""

    disposition: Literal["original_pending", "recognized_after", "conflict", "settled", "cancelled"]
    project_image_json: str | None = None
    receipt: ConfigCommitReceipt | None = None
    error: str | None = None

    def projected(self) -> ProjectState | None:
        """Return independent historical intent without runtime ownership."""
        return (
            None
            if self.project_image_json is None
            else validated_project_image(self.project_image_json)
        )


def inspect_copy_recovery(
    journal: CopyArrangementJournal,
    config_bytes: bytes | None,
    *,
    history: list[CopyArrangementJournal] | None = None,
) -> CopyRecoveryDecision:
    """Recognize exact pending or already written fixed C after whole-history validation."""
    journal = CopyArrangementJournal.model_validate(journal)
    if journal.phase == "settled":
        return CopyRecoveryDecision("settled")
    if journal.phase == "cancelled":
        return CopyRecoveryDecision("cancelled")
    digest = hashlib.sha256(config_bytes).hexdigest() if config_bytes is not None else None
    if journal.image_role == "before" and digest == journal.captured_config_sha256:
        return CopyRecoveryDecision("original_pending", journal.project_image_json)
    if config_bytes is None:
        return CopyRecoveryDecision("conflict", error="Copy Config evidence is missing or changed")
    assert digest is not None
    try:
        text = config_bytes.decode("utf-8")
        current = validated_project_image(text)
    except (UnicodeDecodeError, TypeError, ValueError) as error:
        return CopyRecoveryDecision("conflict", error=f"Unrecognized Copy Config: {error}")
    return _inspect_after(journal, current, text, digest, history)


def _inspect_after(
    journal: CopyArrangementJournal,
    current: ProjectState,
    text: str,
    digest: str,
    history: list[CopyArrangementJournal] | None,
) -> CopyRecoveryDecision:
    recognized = False
    if journal.image_role == "after":
        recognized = (
            digest == journal.committed_config_sha256
            and text == journal.project_image_json
            and current.config_revision == journal.committed_revision
        )
    elif (
        journal.phase == "native_ack_observed"
        and current.config_revision == journal.intent_revision
    ):
        before = validated_project_image(journal.project_image_json)
        after = project_with_journal_copy(before, journal)
        after.config_revision = journal.intent_revision
        recognized = written_copy_image_matches(current, journal.after, journal.target_id) and (
            written_project_intent(current) == written_project_intent(after)
        )
    if recognized:
        return CopyRecoveryDecision(
            "recognized_after", text, ConfigCommitReceipt(current.config_revision, digest, text)
        )
    if journal.phase == "native_ack_observed" and history is not None:
        for prior in reversed(history[:-1]):
            if (
                prior.phase == "config_committed"
                and prior.intent_revision < journal.intent_revision
                and prior.committed_config_sha256 == digest
                and prior.project_image_json == text
                and written_copy_image_matches(current, journal.after, journal.target_id)
            ):
                # Crash after recording newer A/unrelated intent, before replacing
                # the old complete C Config. Whole history proves both boundaries.
                latest_before = validated_project_image(journal.project_image_json)
                latest = project_with_journal_copy(latest_before, journal)
                latest.config_revision = journal.intent_revision
                return CopyRecoveryDecision("recognized_after", latest.model_dump_json())
    return CopyRecoveryDecision(
        "conflict", error="Newer or different Copy Config remains current; recovery stays fenced"
    )
