"""Actual stem worker/controller probe with a native producer replacing device startup."""
# Compiled into an isolated Rust test, never imported as a Python package.
# ruff: noqa: INP001

import hashlib
import json
import shutil
import struct
import time
import wave
from pathlib import Path
from typing import TYPE_CHECKING, Protocol, cast

from imgui_bundle import imgui

from flitzis_looper.controller import AppController
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_cache import (
    cache_dir_for_sample_id,
    expected_stem_files,
    verified_stem_cache_available,
)
from flitzis_looper.controller.stem_generation import StemGenerationResult
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping import InputMappingController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.stem_pair_selection import StemPairSelection
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemProgressCallback,
    )
    from flitzis_looper_audio import (
        AudioEngine,
        PreparedSourceTicket,
        PreparedStemPair,
        ResidentWindowTicket,
    )


class NativeProducerBridge(Protocol):
    engine: AudioEngine

    def prepare_stem_pair(
        self,
        sample_id: int,
        source_version: str,
        cache_dir: str,
        source_ticket: PreparedSourceTicket,
        components: bool,
        descriptor_reference: str | None = None,
    ) -> PreparedStemPair: ...

    def publish_stem_pair(
        self, prepared: PreparedStemPair, source_ticket: PreparedSourceTicket
    ) -> None: ...

    def set_stem_mix_mode(
        self, sample_id: int, mode: str, source_version: str | None = None
    ) -> None: ...

    def set_stem_enabled_mask(
        self, sample_id: int, enabled_stem_mask: int, source_version: str
    ) -> None: ...

    def set_stem_pair_full_mix(self, sample_id: int) -> None: ...

    def publish_prepared_stems(
        self, sample_id: int, version: str, cache_dir: str, ticket: PreparedSourceTicket
    ) -> None: ...

    def loaded_sample_shape(self, sample_id: int) -> tuple[int, int, int]: ...

    def prepare_resident_control(
        self,
        sample_id: int,
        *,
        start_s: float | None,
        end_s: float | None,
        position_s: float | None,
        key_lock: bool | None,
    ) -> ResidentWindowTicket: ...

    def play_resident_control(
        self,
        ticket: ResidentWindowTicket,
        *,
        exclusive: bool,
        received_at_ns: int | None,
    ) -> bool: ...

    def cancel_pad_launches(self, sample_id: int) -> bool: ...

    def stop_sample(self, sample_id: int) -> None: ...


class Audio:
    def __init__(self, bridge: NativeProducerBridge) -> None:
        self.bridge = bridge
        self.timestamps: list[int] = []

    def __getattr__(self, name: str) -> object:
        return getattr(self.bridge.engine, name)

    def prepare_stem_pair(
        self,
        sample_id: int,
        source_version: str,
        cache_dir: str,
        source_ticket: PreparedSourceTicket,
        components: bool,
        descriptor_reference: str | None = None,
    ) -> PreparedStemPair:
        return self.bridge.prepare_stem_pair(
            sample_id, source_version, cache_dir, source_ticket, components, descriptor_reference
        )

    def publish_stem_pair(
        self, prepared: PreparedStemPair, source_ticket: PreparedSourceTicket
    ) -> None:
        self.bridge.publish_stem_pair(prepared, source_ticket)

    def set_stem_mix_mode(
        self, sample_id: int, mode: str, source_version: str | None = None
    ) -> None:
        self.bridge.set_stem_mix_mode(sample_id, mode, source_version)

    def set_stem_enabled_mask(
        self, sample_id: int, enabled_stem_mask: int, source_version: str
    ) -> None:
        self.bridge.set_stem_enabled_mask(sample_id, enabled_stem_mask, source_version)

    def set_stem_pair_full_mix(self, sample_id: int) -> None:
        self.bridge.set_stem_pair_full_mix(sample_id)

    def publish_prepared_stems(
        self, sample_id: int, version: str, cache_dir: str, ticket: PreparedSourceTicket
    ) -> None:
        self.bridge.publish_prepared_stems(sample_id, version, cache_dir, ticket)

    def loaded_sample_shape(self, sample_id: int) -> tuple[int, int, int]:
        return self.bridge.loaded_sample_shape(sample_id)

    def output_sample_rate(self) -> int:
        return self.loaded_sample_shape(0)[0]

    def capture_input_timestamp_ns(self) -> int:
        timestamp = self.bridge.engine.capture_input_timestamp_ns()
        self.timestamps.append(timestamp)
        return timestamp

    def prepare_resident_control(
        self,
        sample_id: int,
        *,
        start_s: float | None,
        end_s: float | None,
        position_s: float | None,
        key_lock: bool | None,
    ) -> ResidentWindowTicket:
        return self.bridge.prepare_resident_control(
            sample_id,
            start_s=start_s,
            end_s=end_s,
            position_s=position_s,
            key_lock=key_lock,
        )

    def play_resident_control(
        self,
        ticket: ResidentWindowTicket,
        *,
        exclusive: bool,
        received_at_ns: int | None,
    ) -> bool:
        return self.bridge.play_resident_control(
            ticket,
            exclusive=exclusive,
            received_at_ns=received_at_ns,
        )

    def cancel_pad_launches(self, sample_id: int) -> bool:
        return self.bridge.cancel_pad_launches(sample_id)

    def stop_sample(self, sample_id: int) -> None:
        self.bridge.stop_sample(sample_id)


class OfflineFixtureBackend:
    """Separation is synthetic; the production worker and publication are unchanged."""

    def __init__(self) -> None:
        self.private_path: Path | None = None

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        self.private_path = request.cache_dir
        request.cache_dir.mkdir(parents=True)
        progress(0.9, "Writing fixture stems")
        shape = request.target_shape
        for name, integer in (
            ("vocals", 2048),
            ("melody", 3072),
            ("bass", 4096),
            ("drums", 5120),
            ("instrumental", 12288),
        ):
            with wave.open(str(request.cache_dir / f"{name}.wav"), "wb") as artifact:
                artifact.setnchannels(shape.channels)
                artifact.setsampwidth(2)
                artifact.setframerate(shape.sample_rate_hz)
                artifact.writeframes(
                    struct.pack("<h", integer) * shape.frame_count * shape.channels
                )
        return StemGenerationResult(
            backend_name="synthetic-fixture",
            model_name="no-model",
            device="cpu",
            cpu_fallback=False,
            artifact_count=5,
        )


def wait_for_pending(controller: StemController, sample_id: int) -> PreparedSourceTicket:
    """Collect the real Pair worker result before checking its native queued ticket."""
    deadline = time.monotonic() + 10
    while sample_id not in controller._pending_stem_publications:
        controller.on_frame_render()
        assert not controller._session.stem_generation_errors, (
            controller._session.stem_generation_errors
        )
        assert time.monotonic() < deadline, "actual Pair worker did not publish a pending ticket"
        if sample_id not in controller._pending_stem_publications:
            time.sleep(0.001)
    ticket = controller._pending_stem_publications[sample_id].source_ticket
    assert ticket.publication_status() == "pending"
    return ticket


def assert_selected_pair(project: ProjectState, sample_id: int) -> None:
    """Check durable canonical selection and actual five-artifact disk lineage.

    Full native integrity/geometry was established by the real preparation kernel.
    The subscriber may still name the verified legacy original; the pair's WAV
    marker and descriptor name the canonical original with the same complete SHA.
    """
    entry = project.stem_cache[sample_id]
    assert entry is not None and entry.pair is not None
    selection = StemPairSelection.model_validate_json(entry.pair.model_dump_json())
    assert selection == entry.pair
    assert entry.cache_dir == selection.wav_generation
    assert entry.stems == expected_stem_files(selection.wav_generation)
    common_bytes = Path(selection.descriptor_reference).read_bytes()
    common = json.loads(common_bytes)
    pcm_bytes = (Path(selection.pcm_generation) / "manifest.json").read_bytes()
    pcm_manifest = json.loads(pcm_bytes)
    marker_bytes = (Path(selection.wav_generation) / ".complete.json").read_bytes()
    marker = json.loads(marker_bytes)
    assert common["schema_version"] == pcm_manifest["schema_version"] == 1
    assert common["encoding"] == "aligned-stem-pair-v1"
    assert pcm_manifest["encoding"] == "aligned-stem-pcm-v1"
    assert (
        common["stem_set_identity"]
        == pcm_manifest["stem_set_identity"]
        == selection.stem_set_identity
    )
    assert (
        common["wav_generation"]
        == pcm_manifest["wav_generation"]
        == selection.wav_generation
    )
    assert common["pcm_generation"] == selection.pcm_generation
    assert common["content"] == pcm_manifest["content"]
    assert common["pcm_manifest_sha256"] == hashlib.sha256(pcm_bytes).hexdigest()
    assert common["wav_manifest_sha256"] == hashlib.sha256(marker_bytes).hexdigest()
    assert common["content"]["source_version"] == marker["source_version"]
    assert (
        common["content"]["source"]["original_sha256"]
        == entry.source_version.rsplit(":", 1)[1]
    )
    assert Path(selection.wav_generation).parts[2] == f"M{common['content']['material_id']}"
    # Preserve the old complete-five-WAV SHA oracle with the actual canonical
    # marker's source version, without changing the runtime subscriber ticket.
    assert verified_stem_cache_available(
        entry.model_copy(update={"source_version": marker["source_version"]})
    )
    assert [artifact["name"] for artifact in common["content"]["artifacts"]] == [
        "vocals", "melody", "bass", "drums", "instrumental"
    ]
    for artifact in common["content"]["artifacts"]:
        data = (Path(selection.pcm_generation) / f"{artifact['name']}.f32le").read_bytes()
        assert len(data) == artifact["pcm_bytes"]
        assert hashlib.sha256(data).hexdigest() == artifact["pcm_sha256"]


class Probe:
    def __init__(
        self, bridge: NativeProducerBridge, source_path: str, frames: int, rate: int
    ) -> None:
        self.audio = cast("AudioEngine", Audio(bridge))
        self.project = ProjectState()
        self.project.sample_paths[0] = source_path
        self.project.sample_durations[0] = frames / rate
        self.project.pad_timing_intent[0] = "legacy"
        self.project.pad_stem_mix_mode[0] = "all_stems"
        self.session = SessionState()
        self.session.pad_stem_enabled_mask[0] = 1
        self.backend = OfflineFixtureBackend()
        self.controller = StemController(
            self.project,
            self.session,
            self.audio,
            stem_backend=self.backend,
            stem_task_runner=lambda work: work(),
        )

    def mouse_setup(self) -> None:
        """Compose the real controller/facade without app startup or a device."""
        self.app = AppController.__new__(AppController)
        self.app._project = self.project
        self.app._session = self.session
        self.app._audio = self.audio
        self.app._persistence = ProjectPersistence(self.project)
        self.app.transport = TransportController(self.project, self.session, self.audio)
        self.app.stems = self.controller
        self.app.loader = LoaderController(
            self.project, self.session, self.audio, lambda _pad: None
        )
        self.app.input_mapping = InputMappingController(self.app)
        self.project.pad_loop_auto[0] = False
        self.context = UiContext(self.app)
        self.mouse_context = imgui.create_context()
        imgui.set_current_context(self.mouse_context)
        io = imgui.get_io()
        io.set_ini_filename(None)
        io.display_size = imgui.ImVec2(400, 300)
        io.delta_time = 1 / 60
        io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
        self.inside = (60.0, 80.0)
        self.mouse_frame(inside=False, down=False)

    def mouse_frame(self, *, inside: bool, down: bool) -> int:
        """Feed actual ImGui mouse events; UI delta_time is not a timing proof."""
        imgui.set_current_context(self.mouse_context)
        io = imgui.get_io()
        io.add_mouse_pos_event(*(self.inside if inside else (350.0, 250.0)))
        io.add_mouse_button_event(imgui.MouseButton_.left, down)
        imgui.new_frame()
        imgui.set_next_window_pos((20, 20))
        imgui.set_next_window_size((200, 220))
        imgui.begin("native mouse trigger proof")
        performance_view._pad_button(self.context, 0, (100, 100))
        lo, hi = imgui.get_item_rect_min(), imgui.get_item_rect_max()
        self.inside = ((lo.x + hi.x) / 2, (lo.y + hi.y) / 2)
        imgui.end()
        imgui.render()
        timestamps = cast("Audio", self.audio).timestamps
        return timestamps[-1] if timestamps else 0

    def mouse_poll(self) -> None:
        self.app.transport.residency.poll()

    def mouse_capture_count(self) -> int:
        return len(cast("Audio", self.audio).timestamps)

    def mouse_status(self) -> str:
        return repr(self.app.transport.residency.status(0))

    def mouse_stop(self) -> None:
        self.app.transport.playback.stop_pad(0)

    def mouse_close(self) -> None:
        imgui.destroy_context(self.mouse_context)
        self.app.loader.shut_down()
        self.app.loader._assets.release_saved_assignments()

    def begin(self) -> str:
        assert self.controller.generate_stems_async(0)
        self.controller.on_frame_render()
        assert not self.session.stem_generation_errors, self.session.stem_generation_errors
        self.original_ticket = wait_for_pending(self.controller, 0)
        assert self.original_ticket.publication_status() == "pending"
        entry = self.project.stem_cache[0]
        assert entry is not None
        assert not entry.available
        assert not self.controller.stems_available(0)
        assert self.backend.private_path is not None
        assert not self.backend.private_path.exists()
        assert Path(entry.cache_dir).name.startswith(".ready-")
        assert_selected_pair(self.project, 0)
        self.saved_path = entry.cache_dir
        return entry.source_version

    def accepted(self) -> None:
        assert self.original_ticket.publication_status() == "accepted"
        self.controller.on_frame_render()
        assert not self.session.stem_generation_errors, self.session.stem_generation_errors
        assert self.controller.stems_available(0)
        assert self.controller.stem_grid_indicator_state(0) == "available"
        entry = self.project.stem_cache[0]
        assert entry is not None
        assert entry.available
        assert entry.cache_dir == self.saved_path
        assert_selected_pair(self.project, 0)

    def save_survivor(self) -> None:
        entry = self.project.stem_cache[0]
        assert entry is not None and entry.available
        self.project.sample_paths[215] = self.project.sample_paths[0]
        self.project.sample_durations[215] = self.project.sample_durations[0]
        self.project.pad_stem_mix_mode[215] = self.project.pad_stem_mix_mode[0]
        self.project.pad_content[215] = PadContentIdentity(instance_id="2" * 32)
        self.project.stem_cache[215] = entry.model_copy(deep=True)
        self.controller._assets.sync_assignments()
        self.controller.invalidate_stem_cache(0)
        self.project.sample_paths[0] = None
        self.project.sample_durations[0] = None
        self.project.pad_content[0] = None
        self.controller._assets.sync_assignments()
        Path("p1b-survivor.json").write_text(self.project.model_dump_json(), encoding="utf-8")
        assert Path(entry.cache_dir).is_dir()

    def restore(self) -> None:
        self.restored_project = ProjectState.model_validate_json(self.project.model_dump_json())
        self.restored_session = SessionState()
        self.restored_session.pad_stem_enabled_mask[0] = 1
        self.restored = StemController(self.restored_project, self.restored_session, self.audio)
        self.restored.restore_stem_cache_from_project_state()
        assert not self.restored.stems_available(0)
        assert self.restored.publish_restored_stem_cache_if_available(0)
        self.restored_ticket = wait_for_pending(self.restored, 0)
        assert self.restored_ticket is not self.original_ticket
        assert self.restored_ticket.publication_status() == "pending"
        assert self.original_ticket.publication_status() == "accepted"

    def restored_accepted(self) -> None:
        assert self.restored_ticket.publication_status() == "accepted"
        self.restored.on_frame_render()
        assert self.restored.stems_available(0)
        entry = self.restored_project.stem_cache[0]
        assert entry is not None
        assert entry.cache_dir == self.saved_path
        assert entry.available
        assert_selected_pair(self.restored_project, 0)

    def reject_tampered_restore(self) -> None:
        artifact = Path(self.saved_path) / "vocals.wav"
        original = artifact.read_bytes()
        corrupted = bytearray(original)
        corrupted[-1] ^= 1
        try:
            artifact.write_bytes(corrupted)
        except PermissionError:
            assert artifact.read_bytes() == original
        else:
            raise AssertionError("active immutable stem generation permitted a write")
        # Preserve the cold legacy-WAV SHA-reader oracle in an unregistered
        # candidate, independently of the active canonical complete pair.
        inactive = (
            Path(cache_dir_for_sample_id(0, self.project.sample_paths[0]))
            / f".ready-{'e' * 32}"
        )
        shutil.copytree(Path(self.saved_path), inactive)
        previous = self.restored_project.stem_cache[0]
        assert previous is not None
        marker_path = inactive / ".complete.json"
        marker = json.loads(marker_path.read_text(encoding="utf-8"))
        marker["source_version"] = previous.source_version
        marker_path.write_text(json.dumps(marker), encoding="utf-8")
        damaged = previous.model_copy(
            update={
                "cache_dir": inactive.as_posix(),
                "stems": expected_stem_files(inactive.as_posix()),
                "pair": None,
            }
        )
        assert verified_stem_cache_available(damaged)
        (inactive / "vocals.wav").write_bytes(corrupted)
        assert not verified_stem_cache_available(damaged)
        self.restored_project.stem_cache[0] = damaged
        try:
            self.restored.restore_stem_cache_from_project_state()
            assert not self.restored.stems_available(0)
            assert self.restored.publish_restored_stem_cache_if_available(0)
            assert not self.restored._pending_stem_publications
            assert self.restored_ticket.publication_status() == "accepted"
            assert artifact.is_file()
        finally:
            self.restored_project.stem_cache[0] = previous
        entry = self.restored_project.stem_cache[0]
        assert entry is not None
        assert_selected_pair(self.restored_project, 0)

    def begin_stale_restore(self) -> None:
        self.restored.restore_stem_cache_from_project_state()
        assert self.restored.publish_restored_stem_cache_if_available(0)
        self.stale_ticket = wait_for_pending(self.restored, 0)
        assert self.stale_ticket.publication_status() == "pending"

    def rejected(self) -> None:
        assert self.stale_ticket.publication_status() == "rejected"
        self.restored.on_frame_render()
        assert not self.restored.stems_available(0)
        assert (
            "native source/request/timing validation"
            in self.restored_session.stem_generation_errors[0]
        )
        assert Path(self.saved_path).is_dir()
        self.controller.shut_down()
        self.restored.shut_down()
        self.controller._assets.release_saved_assignments()
        self.restored._assets.release_saved_assignments()


class SavedMaterialProbe:
    """Fresh interpreter/project/engine restores the surviving far-bank assignment."""

    def __init__(self, bridge: NativeProducerBridge) -> None:
        self.project = ProjectState.model_validate_json(Path("p1b-survivor.json").read_text(encoding="utf-8"))
        assert self.project.sample_paths[0] is None
        assert self.project.stem_cache[0] is None
        assert self.project.pad_content[215].instance_id == "2" * 32
        self.controller = StemController(self.project, SessionState(), cast("AudioEngine", Audio(bridge)))

    def begin(self) -> None:
        self.controller.restore_stem_cache_from_project_state()
        assert not self.controller.stems_available(215)
        assert self.controller.publish_restored_stem_cache_if_available(215)
        self.ticket = wait_for_pending(self.controller, 215)
        assert self.ticket.publication_status() == "pending"

    def accepted(self) -> None:
        assert self.ticket.publication_status() == "accepted"
        self.controller.on_frame_render()
        assert self.controller.stems_available(215)
        assert_selected_pair(self.project, 215)
