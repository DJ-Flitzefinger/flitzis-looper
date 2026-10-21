"""Explicit raw boundary doubles; genuine Native custody has separate regressions."""

from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

if TYPE_CHECKING:
    from flitzis_looper_audio import SourceBeatmapRawInfo, SourceBeatmapRawInspection


class FakeRawCandidate:
    """Supply unverified detector DTOs without exposing accepted-map capabilities."""

    def __init__(self, event_count: int = 260) -> None:
        self.status = "ready"
        self.revision = "d" * 64
        self.event_count = event_count
        self.queries: list[tuple[int, int]] = []

    def info(self) -> SourceBeatmapRawInfo:
        """Return fresh metadata, including truthful unknown musical interpretation."""
        return cast(
            "SourceBeatmapRawInfo",
            {
                "status": self.status,
                "raw_revision": self.revision,
                "decoder_identity": "fixture-decoder",
                "source_sample_rate_hz": 48_000,
                "source_frame_count": 48_000 * 600,
                "source_zero_frame": 0,
                "mono_sha256": "1" * 64,
                "mono_rule": "fixture channel mean",
                "analyzer_sha256": "2" * 64,
                "analyzer_sample_rate_hz": 44_100,
                "analyzer_frame_count": 44_100 * 600,
                "conversion_revision": "fixture-conversion",
                "odf_hop_samples": 512,
                "detector_id": "fixture-QM",
                "code_revision": "fixture-code",
                "configuration": {"fixture": "configuration"},
                "event_count": self.event_count,
                "detected_downbeat_count": 0,
                "outside_source_event_count": 0,
                "interpretation": "unverified",
                "timing_uncertainty": "unknown",
            },
        )

    def inspect(self, start_index: int, max_events: int = 128) -> SourceBeatmapRawInspection:
        """Model only bounded query/routing, with no claim to real detector output."""
        if (
            type(start_index) is not int
            or not 0 <= start_index <= self.event_count
            or type(max_events) is not int
            or not 1 <= max_events <= 128
        ):
            message = "Raw detection page is outside the Native query bounds."
            raise ValueError(message)
        self.queries.append((start_index, max_events))
        end = min(start_index + max_events, self.event_count)
        events = (
            [
                {
                    "raw_index": index,
                    "odf_frame": index + 0.125,
                    "source_seconds": (index + 0.125) * 512 / 44_100,
                    "detected_downbeat": False,
                    "physical_domain": "inside",
                }
                for index in range(start_index, end)
            ]
            if self.status == "ready"
            else []
        )
        return cast(
            "SourceBeatmapRawInspection",
            {
                "status": self.status,
                "raw_revision": self.revision,
                "start_index": start_index,
                "returned_count": len(events),
                "total_event_count": self.event_count,
                "has_more": end < self.event_count,
                "events": events,
            },
        )


def raw_job(audio: Mock) -> Mock:
    """Install explicit neutral defaults even against an older loaded autospec."""
    job = Mock()
    job.is_cancelled.return_value = False
    job.source_beatmap_async_status.return_value = {
        "stage": "queued",
        "terminal": False,
        "raw_review_retired": False,
    }
    job.resolve_source_beatmap_raw_completion.return_value = None
    audio.begin_source_beatmap_raw_async = Mock(return_value=job)
    return job


def raw_event(job: Mock, candidate: FakeRawCandidate) -> dict[str, object]:
    """Resolve this exact opaque delivery; public DTO fields grant nothing."""
    delivery = object()
    job.resolve_source_beatmap_raw_completion.side_effect = lambda owner: (
        candidate if owner is delivery else None
    )
    return {"type": "source_beatmap_raw_completed", "id": 0, "_native_event_owner": delivery}


def review_job(audio: Mock) -> Mock:
    """Install the independent accepted admission seam against an older loaded Native."""
    job = Mock()
    job.is_cancelled.return_value = False
    job.source_beatmap_async_status.return_value = None
    job.resolve_source_beatmap_artifact_completion.return_value = None
    audio.begin_source_beatmap_review_async = Mock(return_value=job)
    return job
