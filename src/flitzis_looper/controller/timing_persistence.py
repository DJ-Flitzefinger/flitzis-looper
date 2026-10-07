"""Verified export from the acknowledged native owner for actual project saves."""

from typing import TYPE_CHECKING

from flitzis_looper.accepted_timing import PersistedAcceptedTiming
from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import AudioEngine


class TimingPersistenceError(OSError):
    """An expected native evidence rejection that leaves the atomic file intact."""

    def __init__(self, sample_id: int, message: str) -> None:
        super().__init__(message)
        self.sample_id = sample_id


def verified_project_timing(project: ProjectState, audio: AudioEngine | None) -> ProjectState:
    """Return a save snapshot containing only freshly verified current evidence.

    Saved envelopes are historical inputs, never authorization to export accepted
    state. Without the native owner a standalone save retains ordinary intent but
    omits accepted records. Failed verification aborts the atomic write.
    """
    snapshot = project.model_copy(deep=True)
    for sample_id, path in enumerate(snapshot.sample_paths):
        analysis = snapshot.sample_analysis[sample_id]
        if analysis is not None:
            analysis.accepted_timing = None
        if path is None:
            snapshot.pad_timing_intent[sample_id] = "legacy"
            continue
        if audio is None:
            continue
        intent = audio.pad_timing_intent(sample_id)
        if intent not in {"automatic", "manual", "tap", "legacy"}:
            msg = "Native timing intent is invalid"
            raise RuntimeError(msg)
        snapshot.pad_timing_intent[sample_id] = intent
        if intent != "automatic" or snapshot.manual_bpm[sample_id] is not None:
            continue
        try:
            exported = audio.export_current_constant_timing(sample_id, path)
            record = (
                PersistedAcceptedTiming.model_validate_json(exported)
                if exported is not None
                else None
            )
        except (RuntimeError, ValueError) as error:
            raise TimingPersistenceError(sample_id, str(error)) from error
        if record is None:
            continue
        if analysis is None:
            analysis = SampleAnalysis(
                bpm=0.0, key="", beat_grid=BeatGrid(beats=[], downbeats=[], bars=[])
            )
            snapshot.sample_analysis[sample_id] = analysis
        analysis.accepted_timing = record
    return snapshot
