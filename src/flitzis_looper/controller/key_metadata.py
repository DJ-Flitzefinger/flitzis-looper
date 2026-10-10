"""Bind neutral key metadata to actual loaded-source analysis admission."""

from dataclasses import dataclass
from typing import TYPE_CHECKING, TypeGuard

from flitzis_looper.key_intent import SourceKeyVersion, next_key_epoch

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import AudioEngine


@dataclass(frozen=True)
class KeyAnalysisAdmission:
    """Control-only source/epoch snapshot; never a copied native timing permit."""

    content_instance: str | None
    path: str | None
    source_identity: tuple[int, str, int, int]
    analysis_epoch: int
    correction_epoch: int
    next_source_version: int


@dataclass(frozen=True)
class _AdmittedKeyAnalysis:
    capture: KeyAnalysisAdmission
    request_id: int
    admitted_epoch: int


class KeyMetadataAnalysis:
    """Use the existing analyzer's real request and source guards for key metadata."""

    def __init__(self, project: ProjectState, audio: AudioEngine) -> None:
        self._project = project
        self._audio = audio
        self._requests: dict[int, _AdmittedKeyAnalysis] = {}
        self._errors: dict[int, tuple[int, str]] = {}

    def prepare(self, sample_id: int) -> KeyAnalysisAdmission | None:
        """Capture actual source identity and reject exhausted epochs before admission."""
        identity = self._source_identity(sample_id)
        if identity is None:
            return None
        state = self._project.pad_key_intent[sample_id]
        next_key_epoch(state.analysis_epoch)
        if state.correction is not None:
            next_key_epoch(state.correction_epoch)
        content = self._project.pad_content[sample_id]
        return KeyAnalysisAdmission(
            content.instance_id if content is not None else None,
            self._project.sample_paths[sample_id],
            identity,
            state.analysis_epoch,
            state.correction_epoch,
            next_key_epoch(state.source.version if state.source is not None else 0),
        )

    def admit(
        self, sample_id: int, capture: KeyAnalysisAdmission | None, request_id: object
    ) -> bool:
        """Remove the preexisting correction only after real matching native admission."""
        if capture is None or not self._valid_request_id(request_id):
            return False
        if not self._matches_source(sample_id, capture):
            return False
        current = self._project.pad_key_intent[sample_id]
        if current.analysis_epoch != capture.analysis_epoch:
            return False
        if current.correction_epoch == capture.correction_epoch:
            current = current.corrected(None)
        updated = current.changed(analysis_epoch=next_key_epoch(capture.analysis_epoch))
        self._project.pad_key_intent[sample_id] = updated
        self._errors.pop(sample_id, None)
        self._requests[sample_id] = _AdmittedKeyAnalysis(
            capture, request_id, updated.analysis_epoch
        )
        return True

    def complete(self, sample_id: int, request_id: object, raw_key: str) -> bool:
        """Publish one current version while preserving a correction made after admission."""
        record = self._requests.get(sample_id)
        if (
            record is None
            or not self._valid_request_id(request_id)
            or request_id != record.request_id
        ):
            return False
        current = self._project.pad_key_intent[sample_id]
        if current.analysis_epoch != record.admitted_epoch or not self._matches_source(
            sample_id, record.capture
        ):
            return False
        updated = current.changed(
            source=SourceKeyVersion(version=record.capture.next_source_version, raw_key=raw_key)
        )
        self._project.pad_key_intent[sample_id] = updated
        self._requests.pop(sample_id, None)
        return True

    def loaded(self, sample_id: int, request_id: object, raw_key: str) -> bool:
        """Record initial analysis only for an already matched new-source success event."""
        if not self._valid_request_id(request_id) or self._source_identity(sample_id) is None:
            return False
        current = self._project.pad_key_intent[sample_id]
        version = next_key_epoch(current.source.version if current.source is not None else 0)
        self._project.pad_key_intent[sample_id] = current.changed(
            source=SourceKeyVersion(version=version, raw_key=raw_key)
        )
        return True

    def cancel(self, sample_id: int) -> None:
        """Forget a terminal request without restoring any earlier correction or permit."""
        self._requests.pop(sample_id, None)
        self._errors.pop(sample_id, None)

    def record_error(self, sample_id: int, request_id: object, error: str) -> None:
        """Retain a visible key failure until the genuinely admitted analysis settles."""
        if self._valid_request_id(request_id):
            self._requests.pop(sample_id, None)
            self._errors[sample_id] = (request_id, error)

    def error_for_request(self, sample_id: int, request_id: object) -> str | None:
        """Return only the error belonging to this strict native request ID."""
        failure = self._errors.get(sample_id)
        if failure is None or not self._valid_request_id(request_id) or failure[0] != request_id:
            return None
        return failure[1]

    def has_request(self, sample_id: int) -> bool:
        """Fence unbound legacy feedback while a genuinely admitted key request exists."""
        return sample_id in self._requests or sample_id in self._errors

    def _matches_source(self, sample_id: int, capture: KeyAnalysisAdmission) -> bool:
        content = self._project.pad_content[sample_id]
        return (
            (content.instance_id if content is not None else None) == capture.content_instance
            and self._project.sample_paths[sample_id] == capture.path
            and self._source_identity(sample_id) == capture.source_identity
        )

    def _source_identity(self, sample_id: int) -> tuple[int, str, int, int] | None:
        identity = self._audio.waveform_source_identity(sample_id)
        if not isinstance(identity, tuple) or len(identity) != 4:
            return None
        generation, digest, frames, rate = identity
        if not self._valid_request_id(generation) or not isinstance(digest, str):
            return None
        if any(
            isinstance(value, bool) or not isinstance(value, int) or value <= 0
            for value in (frames, rate)
        ):
            return None
        return generation, digest, frames, rate

    @staticmethod
    def _valid_request_id(request_id: object) -> TypeGuard[int]:
        return isinstance(request_id, int) and not isinstance(request_id, bool) and request_id >= 0
