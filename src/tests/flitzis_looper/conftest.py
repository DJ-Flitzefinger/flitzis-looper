import hashlib
import json
import struct
import wave
from dataclasses import dataclass, field
from pathlib import Path
from threading import get_ident
from typing import TYPE_CHECKING, cast
from unittest.mock import DEFAULT, Mock, patch

import pytest

from flitzis_looper.controller import AppController
from flitzis_looper.controller.stem_generation import (
    StemGenerationRequest,
    StemGenerationResult,
)
from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.models import ProjectState, SessionState


class FakePreparedSourceTicket:
    def __init__(self, status: str = "accepted") -> None:
        self.status = status
        self.reason: str | None = None
        self.source_request = 1

    def publication_status(self) -> str:
        return self.status

    def rejection_reason(self) -> str | None:
        return self.reason

    def same_source_request(self, other: FakePreparedSourceTicket) -> bool:
        return self.source_request == other.source_request


class FakeProjectAssetLease:
    """Control-flow substitute; actual filesystem/readers are native-tested."""

    def __init__(self, path: str) -> None:
        self.path = path
        self.released = False

    def release(self) -> None:
        self.released = True

    def reclaim_stems(self, expected_path: str) -> None:
        assert expected_path == self.path
        assert not self.released


@dataclass(frozen=True)
class FakeOfflineStemSourceLease:
    """Immutable control fixture; it claims no actual Native cold/file custody."""

    source_version: str
    source_path: str
    original_bytes: int
    target_shape: tuple[int, int, int]
    source_generation: int
    source_request: int
    sample_id: int
    digest: str
    file_identity: tuple[int, int]
    verification_threads: list[int] = field(default_factory=list, compare=False)

    def verify(self) -> None:
        """Verify fixture path/full bytes on the worker; Native held-file proof is separate."""
        self.verification_threads.append(get_ident())
        path = Path(self.source_path)
        before = path.stat()
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        after = path.stat()
        if (
            (before.st_dev, before.st_ino) != self.file_identity
            or (after.st_dev, after.st_ino) != self.file_identity
            or (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns)
            or after.st_size != self.original_bytes
            or digest != self.digest
        ):
            message = "Fixture offline original changed after admission"
            raise ValueError(message)


@dataclass(frozen=True)
class _LoadedSource:
    path: Path
    digest: str
    original_bytes: int
    shape: tuple[int, int, int]
    generation: int
    file_identity: tuple[int, int]


class FakeLoadedSourceDigests:
    """Fixture admission substitute, without claiming actual Native source custody."""

    def __init__(self) -> None:
        self._sources: dict[int, _LoadedSource] = {}
        self._generation = 0

    def load(self, sample_id: int, path: str, *_args: object, **_kwargs: object) -> object:
        """Capture once at explicit fake admission, preserving the mock's return value."""
        self._sources.pop(sample_id, None)
        try:
            admitted = Path(path).absolute()
            with admitted.open("rb") as source:
                digest = hashlib.file_digest(source, "sha256").hexdigest()
            metadata = admitted.stat()
        except OSError:
            return DEFAULT
        try:
            with wave.open(str(admitted), "rb") as wav:
                shape = (wav.getframerate(), wav.getnchannels(), wav.getnframes())
        except OSError, EOFError, wave.Error:
            shape = (44_100, 1, 128)
        self._generation += 1
        self._sources[sample_id] = _LoadedSource(
            admitted,
            digest,
            metadata.st_size,
            shape,
            self._generation,
            (metadata.st_dev, metadata.st_ino),
        )
        return DEFAULT

    def unload(self, sample_id: int) -> object:
        """Forget this fixture source only on the default successful fake unload."""
        self._sources.pop(sample_id, None)
        return DEFAULT

    def get(self, sample_id: int) -> str | None:
        """Read immutable admitted metadata without touching source files."""
        source = self._sources.get(sample_id)
        return source.digest if source is not None else None

    def capture(self, sample_id: int, source_version: str) -> FakeOfflineStemSourceLease:
        """Copy only cached admitted metadata, with no path reads or publication rights."""
        source = self._sources.get(sample_id)
        if source is None or not source_version.endswith(f"|sha256-v1:{source.digest}"):
            message = "Fixture pad has no matching admitted original"
            raise ValueError(message)
        return FakeOfflineStemSourceLease(
            source_version,
            str(source.path),
            source.original_bytes,
            source.shape,
            source.generation,
            source.generation,
            sample_id,
            source.digest,
            source.file_identity,
        )

    def is_current(self, sample_id: int, source: FakeOfflineStemSourceLease | None) -> bool:
        current = self._sources.get(sample_id)
        return (
            source is not None
            and current is not None
            and source.sample_id == sample_id
            and source.source_generation == current.generation
            and source.source_request == current.generation
        )


class FakeGlobalPlaybackBatchTicket:
    """Controller-only feedback substitute for a native global transaction."""

    def __init__(self, status: str = "accepted") -> None:
        self.status = status

    def publication_status(self) -> str:
        return self.status


def current_timing_metadata(
    *,
    sample_id: int = 0,
    period: float = 0.500000000123,
    origin: float = -0.125,
    revision: str = "accepted-test-revision",
    rate: int = 48_000,
) -> dict[str, object]:
    """Complete native current-record contract for control consumer tests."""
    return {
        "pad_id": sample_id,
        "source_id": "native-loaded-source",
        "source_generation": 7,
        "accepted_request_id": 9,
        "publication_epoch": 11,
        "source_sha256": "a" * 64,
        "source_provenance": "observed association; copy-first not proved",
        "pcm_sha256": "b" * 64,
        "sample_rate_hz": rate,
        "frame_count": rate * 600,
        "source_zero_seconds": 0.0,
        "mono_revision": "channel-mean-v1",
        "raw_revision": "raw-test-revision",
        "revision": revision,
        "period_seconds_per_quarter": period,
        "origin_seconds": origin,
        "origin_provenance": "independent test origin",
        "acceptance_policy_version": "test-policy-v1",
        "acceptance_provenance": "explicit test assessment",
    }


class FakeInputRuntimePadBinding:
    """Native-owned runtime capture substitute for controller-only tests."""

    def __init__(
        self,
        sample_id: int = 0,
        *,
        accepted_timing: dict[str, object] | None = None,
        intent: str = "legacy",
        authority_revision: int = 1,
    ) -> None:
        source = accepted_timing or {}
        self._metadata: dict[str, object] = {
            "pad_id": sample_id,
            "source_id": source.get("source_id", f"loaded-{sample_id}-1"),
            "source_generation": source.get("source_generation", 1),
            "source_sha256": source.get("source_sha256", "a" * 64),
            "sample_rate_hz": source.get("sample_rate_hz", 44_100),
            "frame_count": source.get("frame_count", 44_100 * 600),
            "channels": 1,
            "intent": intent,
            "authority_revision": authority_revision,
            "accepted_timing": accepted_timing,
        }

    def is_current(self) -> bool:
        return True

    def metadata(self) -> dict[str, object]:
        return dict(self._metadata)


class FakeStemGenerationBackend:
    def __init__(self) -> None:
        self.requests: list[StemGenerationRequest] = []
        self.result = StemGenerationResult(
            backend_name="fake",
            model_name="fake-model",
            device="cpu",
            cpu_fallback=False,
            artifact_count=len(STEM_KINDS),
        )
        self.error: RuntimeError | None = None
        self.sample_value = 1024

    def generate(
        self,
        request: StemGenerationRequest,
        progress: Callable[[float, str], None],
    ) -> StemGenerationResult:
        self.requests.append(request)
        progress(0.5, "Generating fake stems")
        if self.error is not None:
            raise self.error

        request.cache_dir.mkdir(parents=True, exist_ok=True)
        for kind in STEM_KINDS:
            _write_test_wav(
                request.cache_dir / f"{kind}.wav",
                request.target_shape.sample_rate_hz,
                request.target_shape.channels,
                request.target_shape.frame_count,
                self.sample_value,
            )
        return self.result


def write_test_stem_marker(cache_dir: Path, source_version: str) -> None:
    digests = {
        kind: hashlib.sha256((cache_dir / f"{kind}.wav").read_bytes()).hexdigest()
        for kind in STEM_KINDS
    }
    (cache_dir / ".complete.json").write_text(
        json.dumps({
            "schema": "stem-set-sha256-v1",
            "source_version": source_version,
            "stems": digests,
        }),
        encoding="utf-8",
    )


def _write_test_wav(
    path: Path, sample_rate_hz: int, channels: int, frames: int, sample_value: int
) -> None:
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(channels)
        wav.setsampwidth(2)
        wav.setframerate(sample_rate_hz)
        frame = b"".join(struct.pack("<h", sample_value) for _ in range(channels))
        wav.writeframes(frame * frames)


@pytest.fixture
def stem_backend() -> FakeStemGenerationBackend:
    return FakeStemGenerationBackend()


@pytest.fixture
def audio_engine_mock() -> Iterator[Mock]:
    with patch("flitzis_looper.controller.app.AudioEngine", autospec=True) as audio_engine:
        loaded_sources = FakeLoadedSourceDigests()
        audio_engine.return_value.load_sample_async.side_effect = loaded_sources.load
        audio_engine.return_value.load_incoming_sample_async = Mock(side_effect=loaded_sources.load)
        audio_engine.return_value.update_incoming_load_intent = Mock(return_value=True)
        audio_engine.return_value.unload_sample.side_effect = loaded_sources.unload
        # Install explicitly even when the currently loaded extension predates
        # the metadata getter. Tests may still replace native admission behavior.
        audio_engine.return_value.loaded_source_digest = Mock(side_effect=loaded_sources.get)
        audio_engine.return_value.capture_offline_stem_source = Mock(
            side_effect=loaded_sources.capture
        )
        audio_engine.return_value.offline_stem_source_is_current = Mock(
            side_effect=loaded_sources.is_current
        )

        def capture_stem_ticket(
            sample_id: int, source: FakeOfflineStemSourceLease
        ) -> FakePreparedSourceTicket:
            if not loaded_sources.is_current(sample_id, source):
                message = "Fixture offline source was replaced before fresh publication"
                raise ValueError(message)
            return cast(
                "FakePreparedSourceTicket",
                audio_engine.return_value.capture_prepared_source(sample_id, source.source_version),
            )

        audio_engine.return_value.capture_prepared_stem_source = Mock(
            side_effect=capture_stem_ticket
        )
        audio_engine.return_value.output_sample_rate.return_value = 44_100
        # Controller-only accounting double; this does not claim actual Native PCM/RSS.
        audio_engine.return_value.set_resident_pcm_budget_mib = Mock()
        audio_engine.return_value.get_resident_pcm_budget_status = Mock(
            return_value={
                "budget_mib": 512,
                "budget_bytes": 512 * 1048576,
                "held_unique_pcm_bytes": 0,
                "pending_resident_bytes": 0,
                "admitted_total_bytes": 0,
                "unique_backing_count": 0,
                "pending_reservation_count": 0,
                "over_budget": False,
                "available_bytes": 512 * 1048576,
            }
        )
        audio_engine.return_value.poll_input_events.return_value = None
        audio_engine.return_value.receive_msg.return_value = None
        audio_engine.return_value.current_constant_timing.return_value = None
        audio_engine.return_value.pad_timing_intent.return_value = "legacy"
        audio_engine.return_value.cancel_pad_launches = Mock(return_value=False)
        audio_engine.return_value.cancel_all_launches = Mock(return_value=[])
        audio_engine.return_value.admitted_launch_ids = Mock(return_value=[])
        # Missing callback feedback deliberately remains unknown. Scalar enqueue
        # and project booleans do not prove an actual mode acknowledgement.
        audio_engine.return_value.pad_key_lock_status = Mock(return_value=None)

        def runtime_binding(sample_id: int) -> FakeInputRuntimePadBinding:
            metadata = audio_engine.return_value.current_constant_timing.return_value
            accepted = metadata if isinstance(metadata, dict) else None
            return FakeInputRuntimePadBinding(
                sample_id,
                accepted_timing=accepted,
                intent=audio_engine.return_value.pad_timing_intent.return_value,
            )

        audio_engine.return_value.current_input_runtime_pad_binding.side_effect = runtime_binding
        audio_engine.return_value.start_global_playback_batch.return_value = (
            FakeGlobalPlaybackBatchTicket()
        )
        audio_engine.return_value.stop_global_playback_batch.return_value = (
            FakeGlobalPlaybackBatchTicket()
        )
        audio_engine.return_value.capture_prepared_source.return_value = FakePreparedSourceTicket()
        # Historical WAV controller oracles exercise the explicit compatibility
        # route. New ordinary pair-worker tests install their own typed result.
        audio_engine.return_value.prepare_stem_pair = None
        # Sealed selection/residency tests install their own receipt contract;
        # these historical mocks provide no callback-owned selection receipts.
        audio_engine.return_value.register_committed_stem_pair = None
        audio_engine.return_value.request_committed_stem_residency = None
        audio_engine.return_value.acquire_project_asset_lease = Mock(
            side_effect=FakeProjectAssetLease
        )
        audio_engine.return_value.retire_project_asset = Mock()
        audio_engine.return_value.project_asset_cleanup_status = Mock(return_value=(0, 0, 0, []))
        if hasattr(audio_engine.return_value, "loaded_sample_shape"):
            audio_engine.return_value.loaded_sample_shape.return_value = (44_100, 1, 128)
        yield audio_engine.return_value


@pytest.fixture
def controller(
    audio_engine_mock: Mock,
    stem_backend: FakeStemGenerationBackend,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> AppController:
    monkeypatch.chdir(tmp_path)

    return AppController(stem_backend=stem_backend, stem_task_runner=lambda target: target())


@pytest.fixture
def project_state(controller: AppController) -> ProjectState:
    return controller.project


@pytest.fixture
def session_state(controller: AppController) -> SessionState:
    return controller.session
