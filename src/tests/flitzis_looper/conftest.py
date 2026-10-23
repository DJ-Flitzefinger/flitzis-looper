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

from flitzis_looper.controller import AppController, stem_provenance
from flitzis_looper.controller.stem_generation import (
    StemGenerationRequest,
    StemGenerationResult,
)
from flitzis_looper.controller.stem_provenance import (
    processing_paths as _default_processing_paths,
)
from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import ResidentWindowTicket


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
        self.source_beatmap_receipt: dict[str, object] | None = None
        self.source_beatmap_acknowledgements = 0
        self.source_beatmap_retirements = 0

    def release(self) -> None:
        self.released = True

    @property
    def source_beatmap_bound(self) -> bool:
        """Expose cached fixture binding, never a current file/source qualification."""
        return not self.released and self.source_beatmap_receipt is not None

    def reclaim_stems(self, expected_path: str) -> None:
        assert expected_path == self.path
        assert not self.released

    def bind_source_beatmap(self, receipt_json: str) -> None:
        """Track receipt identity only; no filesystem or source proof is simulated."""
        assert not self.released
        receipt = json.loads(receipt_json)
        assert set(receipt) == {
            "schema_version",
            "kind",
            "reference",
            "bytes",
            "sha256",
            "map_revision",
            "decoder_identity",
        }
        if self.source_beatmap_receipt is not None and self.source_beatmap_receipt != receipt:
            message = "map lease already has a different immutable receipt"
            raise ValueError(message)
        self.source_beatmap_receipt = receipt

    def acknowledge_source_beatmap(self) -> None:
        assert not self.released
        assert self.source_beatmap_receipt is not None
        self.source_beatmap_acknowledgements += 1

    def retire_source_beatmap(self) -> None:
        assert not self.released
        assert self.source_beatmap_receipt is not None
        self.source_beatmap_retirements += 1


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


@dataclass(frozen=True, slots=True)
class _CapturedPadStartIntent:
    sample_id: int
    start_s: float
    end_s: float | None
    volume: float
    exclusive: bool
    received_at_ns: int | None
    expected_source_generation: int | None


class FakeCapturedPadStart:
    """Original controller receipt; actual Native preparation/audio is tested separately."""

    def __init__(
        self,
        sample_id: int,
        start_s: float,
        end_s: float | None,
        volume: float,
        *,
        exclusive: bool = False,
        received_at_ns: int | None = None,
        expected_source_generation: int | None = None,
    ) -> None:
        self._intent = _CapturedPadStartIntent(
            sample_id,
            start_s,
            end_s,
            volume,
            exclusive,
            received_at_ns,
            expected_source_generation,
        )
        self.diagnostic_slot: int | None = sample_id
        self.status = "captured"
        self.failure: str | None = None
        self.cancelled = False
        self.published = False
        self.window: ResidentWindowTicket | None = None

    @property
    def original_id(self) -> int:
        return self._intent.sample_id

    @property
    def loop_region(self) -> tuple[float, float | None]:
        return self._intent.start_s, self._intent.end_s

    @property
    def volume(self) -> float:
        return self._intent.volume

    @property
    def exclusive(self) -> bool:
        return self._intent.exclusive

    @property
    def received_at_ns(self) -> int | None:
        return self._intent.received_at_ns

    @property
    def expected_source_generation(self) -> int | None:
        return self._intent.expected_source_generation

    def state(self) -> str:
        return self.status

    def reason(self) -> str | None:
        return self.failure

    def original_owner_status(self) -> str:
        if self.status == "rejected":
            return "superseded"
        return "published" if self.published else "current"

    def cancel(self) -> bool:
        if self.status in {"applying", "applied", "rejected"}:
            return False
        self.cancelled = True
        self.status = "rejected"
        self.failure = "launch_cancelled"
        return True

    def publish(self, window: ResidentWindowTicket | None = None) -> bool:
        """Complete this explicit controller double without invoking legacy Play."""
        if self.status == "rejected":
            return False
        if window is not self.window:
            return False
        if window is not None and (
            window.publication_status() != "accepted" or not window.is_current()
        ):
            return False
        self.published = True
        self.status = "applied"
        return True


def _install_captured_start_fakes(audio: Mock) -> None:
    def prepare_captured_start(
        start: FakeCapturedPadStart, window: ResidentWindowTicket | None = None
    ) -> ResidentWindowTicket:
        if window is not None:
            start.window = window
        if start.window is None:
            start_s, end_s = start.loop_region
            options = (
                {"expected_source_generation": start.expected_source_generation}
                if start.expected_source_generation is not None
                else {}
            )
            start.window = cast(
                "ResidentWindowTicket",
                audio.prepare_resident_control(
                    start.original_id,
                    start_s=start_s,
                    end_s=end_s,
                    position_s=None,
                    key_lock=None,
                    **options,
                ),
            )
        return start.window

    def publish_captured_start(
        start: FakeCapturedPadStart, window: ResidentWindowTicket | None = None
    ) -> bool:
        return start.publish(window)

    # Missing callback feedback remains unknown. The fake receipt cannot confirm
    # processing mode or claim actual Native PCM/preparation correctness.
    audio.pad_key_lock_status = Mock(return_value=None)
    # Install explicitly even when autospec uses an older Native extension.
    # Each capture gets a fresh owner; publication never calls legacy Play.
    audio.capture_pad_start = Mock(side_effect=FakeCapturedPadStart)
    audio.prepare_captured_pad_start = Mock(side_effect=prepare_captured_start)
    audio.publish_pad_start = Mock(side_effect=publish_captured_start)


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
        source_identity: str | None = None,
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
            "source_identity": source_identity,
        }

    def is_current(self) -> bool:
        return True

    def metadata(self) -> dict[str, object]:
        return dict(self._metadata)


@dataclass(slots=True, kw_only=True)
class FakeOrdinaryTransportAdmission:
    """Explicit routing-only admission; it invents no callback completion."""

    slot: int | None
    requested_paused: bool
    transport_revision: int = 1
    content_lifetime: int = 1
    observation: object | None = None
    error: str | None = None
    slot_queries: int = 0
    observation_queries: int = 0

    def current_control_slot(self) -> int | None:
        self.slot_queries += 1
        if self.error is not None:
            raise RuntimeError(self.error)
        return self.slot

    def current_observation(self) -> object | None:
        self.observation_queries += 1
        if self.error is not None:
            raise RuntimeError(self.error)
        follow = getattr(self.observation, "follow_current_source", None)
        if callable(follow):
            follow(self)
        return self.observation


def _install_ordinary_transport_fakes(audio: Mock) -> None:
    audio.receive_msg.return_value = None
    audio._resolve_ordinary_transport_feedback = Mock(return_value=None)
    audio._current_ordinary_transport_observation = Mock(return_value=None)
    audio.pause_sample.side_effect = lambda sample_id: FakeOrdinaryTransportAdmission(
        slot=sample_id, requested_paused=True
    )
    audio.resume_sample.side_effect = lambda sample_id: FakeOrdinaryTransportAdmission(
        slot=sample_id, requested_paused=False
    )


FakeGlobalStartEntry = tuple[FakeInputRuntimePadBinding, float, float | None, object | None]


@dataclass(frozen=True, slots=True)
class _CapturedGlobalStartIntent:
    entries: tuple[FakeGlobalStartEntry, ...]
    received_at_ns: int | None


class FakeCapturedGlobalStart:
    """One immutable controller group; this double supplies no Native ACK/PCM proof."""

    def __init__(
        self, entries: list[FakeGlobalStartEntry], *, received_at_ns: int | None = None
    ) -> None:
        self._intent = _CapturedGlobalStartIntent(tuple(entries), received_at_ns)
        self.slots = [cast("int", binding.metadata()["pad_id"]) for binding, *_ in entries]
        self.status = "captured"
        self.failure: str | None = None
        self.cancelled = False
        self.frame: int | None = None
        self.ticket = FakeGlobalPlaybackBatchTicket()

    @property
    def entries(self) -> tuple[FakeGlobalStartEntry, ...]:
        return self._intent.entries

    @property
    def received_at_ns(self) -> int | None:
        return self._intent.received_at_ns

    def state(self) -> str:
        return self.status

    def reason(self) -> str | None:
        return self.failure

    def diagnostic_slots(self) -> list[int]:
        return list(self.slots)

    def original_frame(self) -> int | None:
        return self.frame

    def cancel(self) -> bool:
        if self.status in {"applied", "rejected"}:
            return False
        self.cancelled = True
        self.status = "rejected"
        self.failure = "launch_cancelled"
        return True

    def publish(
        self, ticket: FakeGlobalPlaybackBatchTicket | None = None
    ) -> FakeGlobalPlaybackBatchTicket | None:
        """Wait on the exact retained dependency; never invoke legacy GLOBAL START."""
        if self.status == "rejected":
            raise ValueError(self.failure or "captured global start rejected")
        for _, _, _, dependency in self.entries:
            if dependency is None:
                continue
            window = cast("ResidentWindowTicket", dependency)
            status = window.publication_status()
            if status in {"failed", "cancelled", "rejected"} or not window.is_current():
                self.status = "rejected"
                self.failure = window.error() or "original global resident dependency rejected"
                raise ValueError(self.failure)
            if status != "accepted":
                return None
        receipt = self.ticket if ticket is None else ticket
        self.status = "applied" if receipt.publication_status() == "accepted" else "published"
        return receipt


def _install_captured_global_start_fakes(audio: Mock) -> None:
    # Explicit installation also works with an older installed Native ABI.
    audio.capture_global_start = Mock(side_effect=FakeCapturedGlobalStart)
    audio.publish_captured_global_start = Mock(side_effect=lambda captured: captured.publish())


class FakeInputSelection:
    """Control-only selection projection; opaque Native lifetime proof remains separate."""

    def __init__(self) -> None:
        self._revision = 0
        self._ready_bindings: dict[int, object] = {}
        self._ready_sources: dict[int, tuple[object, ...]] = {}
        self._selected_source: tuple[object, ...] | None = None

    @staticmethod
    def _source_identity(binding: object) -> tuple[object, ...] | None:
        current = getattr(binding, "is_current", None)
        provider = getattr(binding, "metadata", None)
        if not callable(current) or current() is not True or not callable(provider):
            return None
        metadata = provider()
        if not isinstance(metadata, dict):
            return None
        source_id = metadata.get("source_id")
        generation = metadata.get("source_generation")
        if not isinstance(source_id, str) or type(generation) is not int:
            return None
        return (
            metadata.get("content_lifetime"),
            source_id,
            generation,
            metadata.get("source_sha256"),
        )

    def record_runtime_state(
        self,
        multi_loop: object,
        loaded: list[bool],
        loop_starts: list[float],
        loop_ends: list[float | None],
        bindings: list[object] | None = None,
        pitch_rows: list[dict[str, object]] | None = None,
        source_map_rows: list[dict[str, object]] | None = None,
        *,
        timing_policy_json: str | None = None,
    ) -> object:
        """Record only explicit ready bindings; preserve the caller's mock return value."""
        rows = bindings if bindings is not None else [None] * len(loaded)
        self._ready_bindings = {
            sample_id: binding
            for sample_id, (ready, binding) in enumerate(zip(loaded, rows, strict=True))
            if ready
        }
        self._ready_sources = {
            sample_id: identity
            for sample_id, binding in self._ready_bindings.items()
            if (identity := self._source_identity(binding)) is not None
        }
        if self._selected_source not in self._ready_sources.values():
            self._selected_source = None
        return DEFAULT

    def select(self, sample_id: int) -> int:
        """Advance one GUI selection revision, including an explicit empty-pad choice."""
        self._revision += 1
        identity = self._source_identity(self._ready_bindings.get(sample_id))
        self._selected_source = identity if identity == self._ready_sources.get(sample_id) else None
        return self._revision

    def projection(self) -> tuple[int, int | None]:
        """Follow a unique published source through placement without binding a later refill."""
        matches = [
            sample_id
            for sample_id, source in self._ready_sources.items()
            if source == self._selected_source
            and self._source_identity(self._ready_bindings[sample_id]) == source
        ]
        return self._revision, matches[0] if len(matches) == 1 else None


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


def _configure_recording_control_mock(audio: Mock) -> None:
    """Set routing-only format/lifecycle defaults without claiming quiescence."""
    audio.output_sample_rate.return_value = 44_100
    audio.audio_stream_failed = Mock(return_value=False)
    audio.quiesce_failed_audio_stream = Mock(return_value=False)


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
        # Routing-only defaults, also for an older loaded Native autospec.
        # Neither absence of failure nor False grants callback quiescence.
        _configure_recording_control_mock(audio_engine.return_value)
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
        _install_ordinary_transport_fakes(audio_engine.return_value)
        audio_engine.return_value.current_constant_timing.return_value = None
        audio_engine.return_value.pad_timing_intent.return_value = "legacy"
        audio_engine.return_value.cancel_pad_launches = Mock(return_value=False)
        audio_engine.return_value.cancel_all_launches = Mock(return_value=[])
        audio_engine.return_value.admitted_launch_ids = Mock(return_value=[])
        # Control-flow doubles only; real stopped/held/pending truth is Native-owned.
        audio_engine.return_value.key_lock_start_behavior = Mock(return_value="first_sound_at_grid")
        audio_engine.return_value.can_change_key_lock_start_behavior = Mock(return_value=True)
        audio_engine.return_value.set_key_lock_start_behavior = Mock(return_value=True)
        _install_captured_start_fakes(audio_engine.return_value)
        _install_captured_global_start_fakes(audio_engine.return_value)

        def runtime_binding(sample_id: int) -> FakeInputRuntimePadBinding:
            metadata = audio_engine.return_value.current_constant_timing.return_value
            accepted = metadata if isinstance(metadata, dict) else None
            return FakeInputRuntimePadBinding(
                sample_id,
                accepted_timing=accepted,
                intent=audio_engine.return_value.pad_timing_intent.return_value,
            )

        audio_engine.return_value.current_input_runtime_pad_binding.side_effect = runtime_binding
        selection = FakeInputSelection()
        audio_engine.return_value.select_input_pad = Mock(side_effect=selection.select)
        audio_engine.return_value.input_selected_projection = Mock(side_effect=selection.projection)
        if audio_engine.return_value.set_input_runtime_state.side_effect is None:
            audio_engine.return_value.set_input_runtime_state.side_effect = (
                selection.record_runtime_state
            )
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

    app = AppController(stem_backend=stem_backend, stem_task_runner=lambda target: target())
    configured_paths = stem_provenance.processing_paths
    if configured_paths is _default_processing_paths:
        # This backend writes tiny synthetic WAVs and executes no installed model.
        # Keep actual Native file sealing/hashing while owning its small inputs.
        # Explicit provenance fixtures and replacement real backends retain their
        # independently configured inventory and full producer checks.
        producer = tmp_path / "owned-fake-stem-producer"
        producer.mkdir()
        inputs = tuple(producer / name for name in ("checkpoint.bin", "recipe.json", "backend.py"))
        for index, path in enumerate(inputs):
            path.write_bytes(f"unattested fake separator input role {index}\n".encode())

        def processing_paths(request: StemGenerationRequest) -> tuple[Path, ...]:
            if app.stems._stem_backend is stem_backend and request.cache_dir in app.stems._jobs:
                return inputs
            return configured_paths(request)

        monkeypatch.setattr(stem_provenance, "processing_paths", processing_paths)
    return app


@pytest.fixture
def project_state(controller: AppController) -> ProjectState:
    return controller.project


@pytest.fixture
def session_state(controller: AppController) -> SessionState:
    return controller.session
