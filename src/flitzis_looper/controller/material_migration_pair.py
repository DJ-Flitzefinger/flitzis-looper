"""Prepare migrated WAVs or a selected pair on the bounded stem worker lane."""

from concurrent.futures import Future
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.controller.stems import StemTaskRunner
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import AudioEngine, PreparedSourceTicket, PreparedStemPair


def prepare_migrated_pair(
    audio: AudioEngine,
    runner: StemTaskRunner | None,
    sample_id: int,
    source_version: str,
    entry: StemCacheEntry,
    ticket: PreparedSourceTicket,
    *,
    components: bool = True,
) -> Future[PreparedStemPair]:
    """Return only after actual verified preparation; no saved evidence grants an ACK."""
    if runner is None:
        message = "Migrated stem pair requires the ordinary bounded worker lane"
        raise RuntimeError(message)
    result: Future[PreparedStemPair] = Future()
    descriptor_reference = entry.pair.descriptor_reference if entry.pair is not None else None

    def run() -> None:
        try:
            prepared = audio.prepare_stem_pair(
                sample_id,
                source_version,
                entry.cache_dir,
                ticket,
                components=components,
                descriptor_reference=descriptor_reference,
            )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            result.set_exception(error)
        else:
            result.set_result(prepared)

    runner(run)
    return result
