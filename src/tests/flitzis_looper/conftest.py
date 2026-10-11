import hashlib
import json
import struct
import wave
from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.controller import AppController
from flitzis_looper.controller.stem_generation import (
    StemGenerationRequest,
    StemGenerationResult,
)
from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from pathlib import Path

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
        audio_engine.return_value.output_sample_rate.return_value = 44_100
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
