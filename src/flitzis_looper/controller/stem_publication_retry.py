"""Bounded reprepare ownership after a native callback rejects a complete pair."""

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.controller.stems import _PendingStemPublication


@dataclass(slots=True)
class StemPublicationRetry:
    """Retain the original rollback owner until a fresh admission replaces it."""

    pending: _PendingStemPublication
    content_id: str | None
    source_path: str | None
    attempts: int = 0


class StemPublicationRetries:
    """At most one held rejected publication per pad, with eight total attempts."""

    MAX_ATTEMPTS = 8

    def __init__(self) -> None:
        self.pending: dict[int, StemPublicationRetry] = {}

    def retain(
        self,
        publication: _PendingStemPublication,
        content_id: str | None,
        source_path: str | None,
    ) -> StemPublicationRetry:
        """Keep the attempt count across replacement workers and native rejections."""
        retry = self.pending.get(publication.sample_id)
        if retry is None:
            retry = StemPublicationRetry(publication, content_id, source_path)
            self.pending[publication.sample_id] = retry
        return retry

    def forget(self, publication: _PendingStemPublication) -> None:
        """Only release bookkeeping for the exact current retained owner."""
        retry = self.pending.get(publication.sample_id)
        if retry is not None and retry.pending is publication:
            self.pending.pop(publication.sample_id)

    def replace(
        self, previous: _PendingStemPublication, replacement: _PendingStemPublication
    ) -> bool:
        """Transfer bookkeeping after the new publication has reserved native capacity."""
        retry = self.pending.get(previous.sample_id)
        if retry is None or retry.pending is not previous:
            return False
        retry.pending = replacement
        return True
