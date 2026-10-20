"""Real App component owners for a device-free Cold/Hold lifetime fixture.

The enclosing Rust child owns the engine, physical files and callback. This
envelope uses the real public Loader and inherited App event/shutdown paths;
it never supplies a source publication, Native acknowledgement or audio effect.
"""

import hashlib
import json
from collections import deque
from contextlib import ExitStack, chdir, contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
from imgui_bundle import imgui

from flitzis_looper.control_diagnostics import ConsoleDiagnostics
from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.app import AppController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.material_migration import MaterialMigrationController
from flitzis_looper.controller.material_migration_recovery import resolve_config_reference
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.pair_arrangement_journal import (
    PairJournalOwner,
    validated_pair_history,
)
from flitzis_looper.controller.pair_arrangement_recovery import inspect_pair_recovery
from flitzis_looper.controller.performance_confirmation import PerformanceConfirmationController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping import InputMappingController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.ui import run as ui_run
from flitzis_looper.ui.context import UiContext
from flitzis_looper.ui.render import performance_view

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from contextlib import AbstractContextManager

    from flitzis_looper.controller.pad_arrangement import _Copy
    from flitzis_looper.controller.pair_arrangement import _Pair
    from flitzis_looper.controller.transport.waveform import _HoldOwner
    from flitzis_looper_audio import (
        AudioEngine,
        HoldActionTicket,
        InputRuntimePadBinding,
        ProjectAssetLease,
        WaveFormRenderData,
    )


@dataclass(frozen=True, slots=True)
class LoadedSource:
    """Detached evidence from one real Loader success and its Native owner."""

    request: int
    content_id: str
    reference: str
    sha256: str
    native: tuple[int, str, int, int]
    lifetime: int


@dataclass(frozen=True, slots=True)
class _JoinBefore:
    project: ProjectState
    session: SessionState
    config: bytes
    revision: int
    dirty: bool
    hold: tuple[object, ...]
    sources: tuple[tuple[int, str, int, int] | None, ...]
    digests: tuple[str | None, ...]
    bindings: tuple[InputRuntimePadBinding, ...]
    binding_metadata: tuple[dict[str, object], ...]
    assignments: dict[tuple[str, int], tuple[Path, ProjectAssetLease]]
    lease_objects: dict[tuple[str, int], int]
    reserved: int
    selection: tuple[int, int | None]


class _HeldLifecycleJoin:
    """One real UI/Pair rejection join, independent of the old lifecycle profiles."""

    def _init_join_state(self, probe: HeldLifecycleProbe) -> None:
        self._join_probe = probe
        self.join_context: UiContext | None = None
        self.join_pair: _Pair | None = None
        self._join_before: _JoinBefore | None = None
        self._join_view: WaveFormRenderData | None = None
        self._join_native: Callable[[], AbstractContextManager[None]] | None = None

    def setup_join_ui(self) -> None:
        """Create only an isolated native ImGui context and the real UI facades."""
        assert self.join_context is None
        app = self._join_probe
        previous = imgui.get_current_context()
        owned = imgui.create_context()

        @contextmanager
        def native_scope() -> Iterator[None]:
            current = imgui.get_current_context()
            imgui.set_current_context(owned)
            try:
                yield
            finally:
                if current is not None:
                    imgui.set_current_context(current)

        def destroy_owned() -> None:
            current = imgui.get_current_context()
            imgui.destroy_context(owned)
            if current is not None and current is not owned:
                imgui.set_current_context(current)

        self._join_native = native_scope
        app._storage.callback(destroy_owned)
        try:
            io = imgui.get_io()
            io.set_ini_filename(None)
            io.display_size = imgui.ImVec2(1000, 700)
            io.delta_time = 1 / 60
            io.backend_flags |= imgui.BackendFlags_.renderer_has_textures
            io.fonts.add_font_default()
            self.join_context = UiContext(app)
            assert app.input_mapping._refresh_rust_runtime_state()
            self.join_context.ui.select_pad(0)
            self.join_context.ui.waveform.open(0)
            assert app.project.selected_pad == 0
            assert app.session.waveform_editor_pad_id == 0
            assert app._audio.input_selected_projection()[1] == 0
            assert self.join_context.ui.waveform in app._pair_waveform_views
        finally:
            if previous is not None:
                imgui.set_current_context(previous)

    def prime_join_view(self) -> bool:
        """Obtain actual worker-produced A data before testing its Native cancellation."""
        assert self.join_context is not None
        app = self._join_probe
        duration = app.project.sample_durations[0]
        assert duration is not None
        assert duration * app._audio.output_sample_rate() > 4096
        data = self.join_context.ui.waveform.get_render_data(0, 128, 0.0, duration)
        if data is None:
            status, error = app._audio.waveform_readiness(0)
            # The real worker may finish after the request returned None and
            # before this status read; fetch its data on the next control poll.
            assert status in {"idle", "pending", "ready"}, (status, error)
            return False
        assert app._audio.waveform_readiness(0) == ("ready", None)
        assert self.join_context.ui.waveform._last_waveform_value is data
        self._join_view = data
        return True

    def press_join(self) -> HoldActionTicket:
        """Use actual ImGui button state and the public waveform UI press route."""
        assert self._join_native is not None
        assert self.join_context is not None
        app = self._join_probe
        waveform = app.transport.waveform
        assert not waveform.has_pending_holds
        assert app.session.waveform_editor_pad_id == 0
        with self._join_native():
            down = True
            imgui.get_io().add_mouse_button_event(imgui.MouseButton_.right, down)
            imgui.new_frame()
            try:
                assert imgui.is_mouse_down(imgui.MouseButton_.right)
                self.join_context.ui.waveform.pause_selected_pad_hold_on_press()
            finally:
                imgui.end_frame()
        assert waveform._physical_hold_press is not None, waveform.hold_status()
        assert len(waveform._hold_owners) == 1
        app.owner = next(iter(waveform._hold_owners.values()))
        app.hold = app.owner.ticket
        app.hold_slot = 0
        assert app.owner.physical_press_id == waveform._physical_hold_press
        return app.hold

    def remember_join_before(self) -> None:
        """Capture the saved before image and exact current Python/Native owners."""
        app = self._join_probe
        assert app.hold is not None
        assert app.hold.publication_status() == "paused_owned"
        assert app.session.active_sample_ids == {0, 1}
        assert app.session.paused_sample_ids == {1}
        app.save()
        bindings = []
        for slot in (0, 1):
            binding = app._audio.current_input_runtime_pad_binding(slot)
            assert binding is not None
            assert binding.is_current()
            bindings.append(binding)
        self._join_before = _JoinBefore(
            app.project.model_copy(deep=True),
            app.session.model_copy(deep=True),
            app.persistence.config_path.read_bytes(),
            app.persistence.revision,
            app.persistence._dirty,
            app._hold_image(),
            tuple(app._audio.waveform_source_identity(slot) for slot in (0, 1)),
            tuple(app._audio.loaded_source_digest(slot) for slot in (0, 1)),
            tuple(bindings),
            tuple(binding.metadata() for binding in bindings),
            app._assets._assignments.copy(),
            {key: id(lease) for key, (_, lease) in app._assets._assignments.items()},
            app._assets._reserved,
            app._audio.input_selected_projection(),
        )

    def begin_rejected_join(self) -> None:
        """Capture a real complete Pair while H remains owned; Rust supplies queue pressure."""
        assert self._join_before is not None
        app = self._join_probe
        assert app.pairs.begin(0, 1, operation="swap"), app.pairs.error
        self.join_pair = app.pairs._pair
        assert self.join_pair is not None
        assert self.join_pair.acknowledgement is None
        assert self.join_pair.receipt is None

    def poll_rejected_join(self) -> bool:
        """Observe genuine publication pressure without fabricating a rejected ticket."""
        app = self._join_probe
        pair = self.join_pair
        assert pair is not None
        app.pairs.poll()
        assert app.pairs._pair is pair
        if not pair.worker_done or pair.ticket is None:
            return False
        assert app.pairs.error == "pair publication queue is full", app.pairs.error
        assert app.pairs.status == "settlement_pending"
        assert pair.captured is not None
        assert not pair.published
        assert pair.acknowledgement is None
        assert pair.ticket.own_acknowledgement() is None
        metadata = json.loads(pair.ticket.metadata_json())
        assert metadata["native_phase"] == 0
        assert metadata["own_native_ack"] is False
        assert pair.ticket.publication_status() != "rejected"
        self.assert_join_before()
        return True

    def assert_join_before(self) -> None:
        """Pair preparation/publication rejection cannot mutate the admitted before image."""
        app = self._join_probe
        before = self._join_before
        assert before is not None
        assert app.project == before.project
        assert app.session == before.session
        assert app.persistence.config_path.read_bytes() == before.config
        assert app.persistence.revision == before.revision
        assert app.persistence._dirty == before.dirty
        assert app._hold_image() == before.hold
        selection = app._audio.input_selected_projection()
        assert selection[0] == before.selection[0]
        # Reserved participants intentionally cannot admit fresh selected actions.
        assert selection[1] == (None if app.pairs.active else before.selection[1])
        assert tuple(app._audio.waveform_source_identity(slot) for slot in (0, 1)) == before.sources
        assert tuple(app._audio.loaded_source_digest(slot) for slot in (0, 1)) == before.digests
        assert app._assets._assignments == before.assignments
        lease_objects = {key: id(lease) for key, (_, lease) in app._assets._assignments.items()}
        assert lease_objects == before.lease_objects
        assert all(not lease.released for _, lease in before.assignments.values())
        for slot, binding in enumerate(before.bindings):
            assert binding.metadata() == before.binding_metadata[slot]
            if not app.pairs.active:
                current = app._audio.current_input_runtime_pad_binding(slot)
                assert current is not None
                assert current.is_current()
                assert current.metadata() == before.binding_metadata[slot]

    def cancel_rejected_join(self) -> None:
        """Cancel only the actual preclaim Pair and verify complete durable cancellation."""
        app = self._join_probe
        pair = self.join_pair
        before = self._join_before
        assert pair is not None
        assert before is not None
        assert pair.ticket is not None
        assert app.pairs.prepare_shutdown()
        assert app.pairs.status == "cancelled"
        assert not app.pairs.active
        assert pair.cancelled
        assert pair.writer_released
        assert not pair.published
        assert pair.ticket.own_acknowledgement() is None
        assert json.loads(pair.ticket.metadata_json())["native_phase"] == 3
        assert pair.ticket.publication_status() == "rejected"
        assert pair.acknowledgement is None
        assert pair.receipt is None
        assert pair.mirror is None
        assert pair.stems is None
        assert not pair.assets_published
        assert not pair.native_finished
        assert pair.retirement.remaining == 0
        assert app._assets._reserved == before.reserved
        assert app.persistence._migration_owner is None
        for component in (app.transport, app.loader, app.accepted_timing, app.stems):
            assert component._pair_deferred_sample_ids == frozenset()
        samples = Path.cwd() / "samples"
        journal = PairJournalOwner.open(samples, pair.owner)
        records = validated_pair_history(journal.encoded, pair.owner)
        assert records[-1].phase == "cancelled"
        assert all(record.phase != "native_ack_observed" for record in records)
        resolved = resolve_config_reference(samples, app.persistence.config_reference)
        assert resolved == app.persistence.config_reference
        assert inspect_pair_recovery(records[-1], before.config).disposition == "cancelled"
        self.assert_join_before()

    def retarget_join(self) -> None:
        """Retarget actual selection/editor and cancel A's genuine Native view request."""
        assert self.join_context is not None
        assert self._join_view is not None
        app = self._join_probe
        ui = self.join_context.ui
        waveform = app.transport.waveform
        revision = waveform.view_revision
        assert app._audio.waveform_readiness(0) == ("ready", None)
        assert ui.waveform._last_waveform_value is self._join_view
        ui.select_pad(1)
        ui.waveform.open(1)
        assert app.project.selected_pad == 1
        assert app._audio.input_selected_projection()[1] == 1
        assert app.session.waveform_editor_pad_id == 1
        assert waveform.view_revision == revision + 1
        assert app._audio.waveform_readiness(0) == ("idle", None)
        assert waveform._physical_hold_press is None
        assert app.owner is not None
        assert app.owner.release_requested
        duration = app.project.sample_durations[1]
        assert duration is not None
        end = min(duration, 4096 / app._audio.output_sample_rate())
        data = ui.waveform.get_render_data(1, 128, 0.0, end)
        assert data is not None
        assert data is not self._join_view
        assert ui.waveform._last_waveform_value is data
        assert ui.waveform._last_pad_id == 1
        assert ui.waveform._last_view_revision == waveform.view_revision
        assert ui.waveform._last_source_identity is not None
        assert ui.waveform._last_source_identity[3] == app._audio.waveform_source_identity(1)

    def outside_join_tick(self) -> None:
        """Run real global button-up and App polling without toolbar rendering or hover."""
        assert self._join_native is not None
        assert self.join_context is not None
        app = self._join_probe
        with self._join_native():
            io = imgui.get_io()
            io.add_mouse_pos_event(-10_000.0, -10_000.0)
            down = False
            io.add_mouse_button_event(imgui.MouseButton_.right, down)
            imgui.new_frame()
            try:
                assert not io.app_focus_lost
                assert not imgui.is_mouse_down(imgui.MouseButton_.right)
                assert not app.session.settings_open
                assert app.session.waveform_editor_open
                ui_run._poll_runtime_tick(self.join_context)
            finally:
                imgui.end_frame()

    def assert_join_released(self) -> None:
        """Require the actual global tick to observe and remove the original terminal H."""
        app = self._join_probe
        waveform = app.transport.waveform
        assert app.hold is not None
        assert app.owner is not None
        assert app.hold.publication_status() == "resumed"
        assert app.owner.ticket is app.hold
        assert app.owner.release_requested
        assert app.owner.release_admitted
        assert app.owner.status == "resumed"
        assert not waveform.has_pending_holds
        assert waveform._physical_hold_press is None
        assert waveform.hold_status() == (None, None)
        assert app.session.waveform_pause_hold_pad_id is None
        assert app.session.active_sample_ids == {0, 1}
        assert app.session.paused_sample_ids == {1}
        assert app.session.waveform_editor_pad_id == 1


class _HeldRenderedArrangement:
    """Real rendered pad input, with detached observations of its original owner."""

    def _init_rendered_state(self, probe: HeldLifecycleProbe) -> None:
        self._render_probe = probe
        self.render_binding: InputRuntimePadBinding | None = None
        self.render_owner: _Pair | _Copy | None = None
        self.render_copy: _Copy | None = None
        self.render_project: ProjectState | None = None
        self.render_config: bytes | None = None
        self._render_centres: dict[int, tuple[float, float]] = {}

    def setup_arrangement_ui(self) -> None:
        """Reuse the actual UI context and enable its transient arrangement owner."""
        app = self._render_probe
        app.setup_join_ui()
        context = app.join_context
        assert context is not None
        assert not context.ui.arrangement.enabled
        context.ui.arrangement.toggle()
        assert context.ui.arrangement.enabled
        self.render_arrangement_frame(-1)
        app.save()
        self.render_project = app.project.model_copy(deep=True)
        self.render_config = app.saved_bytes

    def render_arrangement_frame(
        self, hover: int, *, left: bool = False, right: bool = False, middle: bool = False
    ) -> None:
        """Feed real IO, render production pad buttons, and run the global runtime tick."""
        app = self._render_probe
        context = app.join_context
        assert context is not None
        assert app._join_native is not None
        with app._join_native():
            io = imgui.get_io()
            point = self._render_centres.get(hover, (-10_000.0, -10_000.0))
            io.add_mouse_pos_event(*point)
            io.add_mouse_button_event(imgui.MouseButton_.left, left)
            io.add_mouse_button_event(imgui.MouseButton_.right, right)
            io.add_mouse_button_event(imgui.MouseButton_.middle, middle)
            imgui.new_frame()
            try:
                assert not io.app_focus_lost
                assert imgui.is_mouse_down(imgui.MouseButton_.left) == left
                assert imgui.is_mouse_down(imgui.MouseButton_.right) == right
                assert imgui.is_mouse_down(imgui.MouseButton_.middle) == middle
                imgui.set_next_window_pos((20.0, 20.0), imgui.Cond_.always)
                imgui.set_next_window_size((850.0, 220.0), imgui.Cond_.always)
                imgui.begin(
                    "Owned rendered arrangement fixture",
                    flags=(
                        imgui.WindowFlags_.no_title_bar
                        | imgui.WindowFlags_.no_resize
                        | imgui.WindowFlags_.no_move
                        | imgui.WindowFlags_.no_saved_settings
                    ),
                )
                try:
                    for slot in range(3):
                        imgui.set_cursor_pos((30.0 + slot * 250.0, 40.0))
                        corner = imgui.get_cursor_screen_pos()
                        self._render_centres[slot] = (corner.x + 110.0, corner.y + 60.0)
                        performance_view._pad_button(context, slot, (220.0, 120.0))
                    self._retain_rendered_owner()
                finally:
                    imgui.end()
                ui_run._poll_runtime_tick(context)
            finally:
                imgui.end_frame()

    def press_arrangement_hold(self) -> HoldActionTicket:
        """Start H over the rendered window so ImGui owns the held right press."""
        app = self._render_probe
        assert app._join_native is not None
        with app._join_native():
            imgui.get_io().add_mouse_pos_event(*self._render_centres[0])
            return app.press_join()

    def _retain_rendered_owner(self) -> None:
        app = self._render_probe
        assert app.join_context is not None
        press = app.join_context.ui.arrangement._press
        if self.render_binding is None and press is not None:
            assert press.source == 0
            assert press.binding.is_current()
            self.render_binding = press.binding
        owner = app.pairs._pair if app.pairs._pair is not None else app.arrangement._copy
        if owner is not None:
            if app.arrangement._copy is not None:
                self.render_copy = app.arrangement._copy
            assert self.render_binding is not None
            assert owner.source_binding is self.render_binding
            if self.render_owner is None:
                self.render_owner = owner
            assert self.render_owner is owner

    def assert_arrangement_preview(self, target: int, operation: str) -> None:
        """Read the production target feedback for the held rendered gesture."""
        context = self._render_probe.join_context
        assert context is not None
        preview = context.ui.arrangement.preview(target)
        assert preview is not None
        assert preview.source == 0
        assert preview.target == target
        assert preview.operation == operation
        assert preview.label == f"{operation}: PAD 1 → PAD {target + 1}"

    def poll_arrangement_published(self) -> bool:
        """Prepare/publish through actual App polling, without running the callback."""
        app = self._render_probe
        app.poll_runtime_events()
        owner = self.render_owner
        assert owner is not None
        assert owner.source_binding is self.render_binding
        assert app.pairs.error is None, app.pairs.error
        assert app.arrangement.error is None, app.arrangement.error
        assert owner.receipt is None
        assert not owner.native_finished
        assert not owner.writer_released
        assert app.project == self.render_project
        assert app.persistence.config_path.read_bytes() == self.render_config
        pair = app.pairs._pair
        if pair is not None:
            assert owner is pair
            assert pair.acknowledgement is None
            if pair.ticket is not None:
                assert pair.ticket.own_acknowledgement() is None
        else:
            copy = app.arrangement._copy
            assert copy is not None
            assert owner is copy
            assert not copy.acknowledged
        return owner.published

    def poll_arrangement_finished(self) -> bool:
        """Observe the original real owner through its own ACK, config and finish."""
        app = self._render_probe
        app.poll_runtime_events()
        assert app.pairs.error is None, app.pairs.error
        assert app.arrangement.error is None, app.arrangement.error
        if app.pairs.active or app.arrangement.active:
            return False
        owner = self.render_owner
        assert owner is not None
        assert owner.source_binding is self.render_binding
        assert owner.published
        assert owner.native_finished
        assert owner.writer_released
        assert owner.ticket is not None
        assert owner.ticket.publication_status() == "accepted"
        saved = app.persistence.config_path.read_bytes()
        assert ProjectState.model_validate(json.loads(saved)) == app.project
        if owner.receipt is None:
            # Empty Copy uses its existing atomic commit without retaining a receipt/journal.
            copy = self.render_copy
            assert copy is owner
            assert copy is not None
            assert copy.predecessor is None
            assert copy.initial_journal is None
            assert copy.journal is None
            assert copy.config_committed
            assert copy.model_committed
            assert copy.assets_published
            assert copy.committed_revision == app.persistence.revision
            assert json.loads(saved)["config_revision"] == copy.committed_revision
        else:
            assert saved == owner.receipt.written_text.encode("utf-8")
            assert app.project == owner.receipt.projected()
            assert owner.journal is not None
            assert owner.journal.journal is not None
            assert owner.journal.journal.phase == "settled"
        assert app.pairs.status == "completed" or app.arrangement.status == "completed"
        app.save()
        return True

    def toggle_multi_loop_after_pair_ack(self) -> None:
        """Publish the real global runtime change before Python observes Pair's own ACK."""
        app = self._render_probe
        context = app.join_context
        pair = app.pairs._pair
        assert context is not None
        assert pair is not None
        assert pair is self.render_owner
        assert pair.source_binding is self.render_binding
        assert pair.ticket is not None
        assert pair.ticket.publication_status() == "accepted"
        assert pair.ticket.own_acknowledgement() is not None
        assert pair.acknowledgement is None
        assert pair.receipt is None
        assert not pair.native_finished
        assert not pair.writer_released
        assert app.project == self.render_project
        assert app.persistence.config_path.read_bytes() == self.render_config
        app.join_pair = pair
        data = pair.snapshot.projected()
        participants = (data.source_id, data.target_id)
        before = app.input_mapping._last_input_runtime_state_signature
        assert before is not None
        assert before[0] is True
        assert set(participants) <= app.transport._pair_deferred_sample_ids
        revision = app.persistence.revision

        context.audio.global_.toggle_multi_loop()
        assert context.audio.global_.playback_error is None
        assert app.project.multi_loop is False
        assert app.persistence.revision == revision + 1
        # App.on_frame_render polls Pair first. Use the actual InputMapping frame
        # callback here so its global publication precedes that first ACK poll.
        app.input_mapping.on_frame_render()
        assert app.input_mapping._last_input_runtime_state_error is None
        after = app.input_mapping._last_input_runtime_state_signature
        assert after is not None
        assert after[0] is False
        for slot in participants:
            assert after[2][slot] is before[2][slot]
        assert app.pairs._pair is pair
        assert pair.acknowledgement is None
        assert pair.receipt is None
        assert not pair.native_finished
        assert not pair.writer_released
        assert app.persistence.config_path.read_bytes() == self.render_config

    def _assert_pair_app_durable(self) -> _Pair:
        """Verify that the original own ACK owns both mirrors and the durable receipt."""
        app = self._render_probe
        pair = app.join_pair
        assert pair is not None
        assert pair is self.render_owner
        assert pair.source_binding is self.render_binding
        assert pair.acknowledgement is not None
        assert pair.mirror is not None
        assert pair.mirror.applied
        assert pair.mirror._acknowledgement is pair.acknowledgement
        assert pair.stems is not None
        assert pair.stems.applied
        assert pair.stems._acknowledgement is pair.acknowledgement
        assert pair.native_finished
        assert pair.writer_released
        assert pair.assignment_publication is not None
        assert pair.assignment_publication.settled
        assert pair.retirement.remaining == 0
        assert app.persistence._migration_owner is None
        assert not app.pairs.active
        assert app.pairs.status == "completed"
        assert app.pairs.error is None
        assert pair.receipt is not None
        saved = app.persistence.config_path.read_bytes()
        assert saved == pair.receipt.written_text.encode("utf-8")
        assert hashlib.sha256(saved).hexdigest() == pair.receipt.sha256
        assert pair.receipt.revision == app.persistence.revision
        assert pair.receipt.projected() == app.project
        assert not app.project.multi_loop
        history = PairJournalOwner.open(Path.cwd() / "samples", pair.owner)
        records = validated_pair_history(history.encoded, pair.owner)
        assert records[-1].phase == "settled"
        assert records[-1].committed_revision == pair.receipt.revision
        assert records[-1].committed_config_sha256 == pair.receipt.sha256
        return pair

    def assert_pair_app_close_ready(self) -> None:
        """Verify durable Pair custody, fresh participant selection and actual App Close."""
        app = self._render_probe
        context = app.join_context
        assert context is not None
        pair = self._assert_pair_app_durable()
        app.input_mapping.on_frame_render()
        assert app.input_mapping._last_input_runtime_state_error is None
        signature = app.input_mapping._last_input_runtime_state_signature
        assert signature is not None
        data = pair.snapshot.projected()
        for slot in (data.source_id, data.target_id):
            for component in (app.transport, app.loader, app.accepted_timing, app.stems):
                assert slot not in component._pair_deferred_sample_ids
            loaded = app.project.sample_paths[slot] is not None
            assert signature[2][slot].ready is loaded
            if loaded:
                binding = app._audio.current_input_runtime_pad_binding(slot)
                assert binding is not None
                assert binding.is_current()
                before_selection = app._audio.input_selected_projection()[0]
                context.ui.select_pad(slot)
                selection = app._audio.input_selected_projection()
                assert selection == (before_selection + 1, slot)
                assert app.project.selected_pad == slot
            else:
                assert signature[2][slot].binding is None
                assert app._audio.current_input_runtime_pad_binding(slot) is None
        assert app.project.selected_pad == data.target_id
        assert app._audio.input_selected_projection()[1] == data.target_id
        app.save()
        assert app.prepare_close()
        assert app.pending_shutdown_error is None
        assert app._close_requested
        assert not app._shutdown_preparing
        assert not app._shutdown_running

    def assert_cancelled_rendered_drop(self) -> None:
        """A stale physical drag must not acquire a fresh source or transaction."""
        app = self._render_probe
        context = app.join_context
        assert context is not None
        assert self.render_binding is not None
        assert not self.render_binding.is_current()
        assert self.render_owner is None
        assert not app.pairs.active
        assert not app.arrangement.active
        assert context.ui.arrangement.source_id is None
        assert not context.ui.arrangement.owns_pointer
        assert app.project.pad_content[2] is None
        assert app.project.sample_paths[2] is None


class HeldLifecycleProbe(AppController, _HeldLifecycleJoin, _HeldRenderedArrangement):
    """Inject a headless engine while retaining actual App component behavior."""

    def __init__(self, audio: AudioEngine, root: str, *, restore: bool = False) -> None:
        self._storage = ExitStack()
        self._closed = False
        self.app = self
        self._init_join_state(self)
        self._init_rendered_state(self)
        self.hold: HoldActionTicket | None = None
        self.owner: _HoldOwner | None = None
        self.hold_slot: int | None = None
        self.source_history: dict[int, list[LoadedSource]] = {}
        self.pending_requests: dict[int, int] = {}
        self.input_paths: dict[int, Path] = {}
        self.saved_bytes: bytes | None = None
        with ExitStack() as setup:
            directory = Path(root).resolve(strict=True)
            setup.enter_context(chdir(directory))
            samples = directory / "samples"
            samples.mkdir(exist_ok=True)
            config = samples / "held-lifecycle.config.json"
            if restore:
                self._persistence = ProjectPersistence.from_config_path(config)
                assert self._persistence.load_error is None, self._persistence.load_error
            else:
                self._persistence = ProjectPersistence(
                    ProjectState(
                        multi_loop=True,
                        bpm_lock=False,
                        key_lock=False,
                        trigger_quantization_enabled=False,
                        input_mapping_enabled=False,
                        selected_pad=0,
                    )
                )
            self._persistence.config_path = config
            self._project = self._persistence.project
            self._session = SessionState()
            self._audio = audio
            self._close_requested = False
            self._shutdown_preparing = False
            self._shutdown_running = False
            self._shutdown_complete = False
            self._gui_native_selection = None
            self._pair_waveform_views = []
            self._pending_content_feedback = deque()
            self._audio.set_resident_pcm_budget_mib(self._project.resident_pcm_budget_mib)
            self._assets = ProjectAssetLifecycle(self._project, audio)
            self._assets.sync_assignments()
            self._persistence.bind_audio(audio, self._report_timing_save_error)
            self._wire_components()
            # The real InputMapping component reads only this owned child's
            # relative config/input files. No MIDI ports are opened here.
            self._audio.set_input_mapping_enabled(False)
            self._storage = setup.pop_all()

    def _wire_components(self) -> None:
        self.settings = SettingsController(
            self.project, self.session, self._audio, self.persistence.mark_dirty
        )
        self.transport = TransportController(
            self.project, self.session, self._audio, self.persistence.mark_dirty
        )
        self._init_recording_control()
        self.accepted_timing = AcceptedTimingController(self.transport)
        self.transport.bpm.set_accepted_refresh_callback(self.accepted_timing.refresh_current)
        self._init_stem_control(None, None)
        self.arrangement = PadArrangementController(
            self.persistence,
            self.session,
            self._audio,
            self._assets,
            stem_task_runner=self.stems._stem_task_runner,
            on_copy_committed=self._on_copy_committed,
            on_target_removed=self._on_copy_target_removed,
            can_admit=lambda: not (self.pairs.active or self.banks.active),
        )
        self.transport.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.stems.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.transport.bind_global_key_lock_edit_guard(self._ensure_global_key_lock_edit_allowed)
        self.settings.bind_residency_policy(self.stems.refresh_residency_policy)
        self.loader = LoaderController(
            self.project,
            self.session,
            self._audio,
            on_pad_bpm_changed=self.transport.bpm.on_pad_bpm_changed,
            on_project_changed=self.persistence.mark_dirty,
            on_stem_generation_started=self.stems._handle_stem_generation_started,
            on_stem_generation_progress=self.stems._handle_stem_generation_progress,
            on_stem_generation_success=self.stems._handle_stem_generation_success,
            on_stem_generation_error=self.stems._handle_stem_generation_error,
            on_stems_deleted=self.stems.delete_stems,
        )
        self.loader.bind_asset_lifecycle(self._assets)
        self.transport.bind_incoming_global_key_lock(self.loader._refresh_incoming_global_intent)
        self.transport.bind_current_pad_edit(
            self.loader.record_current_pad_edit,
            incoming_request=self.loader.incoming_load_request_id,
        )
        self.loader.set_stems_invalidated_callback(self.stems.invalidate_stem_cache)
        self.loader.set_restored_sample_loaded_callback(self._on_restored_sample_loaded)
        self.loader.set_new_sample_loaded_callback(
            self._on_new_sample_loaded,
            key_lock_callback=self._refresh_loaded_key_lock,
            incoming_callback=self._on_incoming_sample_loaded,
        )
        self.loader.set_sample_unloaded_callback(self._on_sample_unloaded)
        self.loader.set_accepted_timing_refresh_callback(self._refresh_restored_accepted_timing)
        self.material_migration = MaterialMigrationController(
            self.persistence,
            self.session,
            self._audio,
            self._assets,
            self.loader,
            stem_task_runner=self.stems._stem_task_runner,
        )
        self.metering = MeteringController(self.project, self.session, self._audio)
        self.performance_confirmation = PerformanceConfirmationController(self)
        self.input_mapping = InputMappingController(self, self.persistence.mark_dirty)
        self._prepare_arrangement_recovery()
        self._controllers = {
            self.transport,
            self.loader,
            self.metering,
            self.stems,
            self.input_mapping,
        }
        self.transport.playback.set_global_playback_feedback_poll(self._poll_audio_messages)
        self.diagnostics = ConsoleDiagnostics(
            lambda: (
                path
                for path in (
                    *self._project.sample_paths,
                    *self._session.pending_sample_paths.values(),
                )
                if path is not None
            )
        )
        self.transport.pitch.bind_diagnostics(self.diagnostics)
        self.transport.residency._bind_diagnostics(self.diagnostics)
        self.transport.waveform.bind_diagnostics(self.diagnostics)
        self.loader._bind_diagnostics(self.diagnostics)
        self.stems._bind_diagnostics(self.diagnostics)

    def load(self, slot: int, path: str) -> int:
        """Admit one ordinary import through the real public Loader."""
        self.input_paths[slot] = Path(path).resolve(strict=True)
        self.loader.load_sample_async(slot, path)
        assert slot not in self.session.sample_load_errors, self.session.sample_load_errors
        request = self.loader.incoming_load_request_id(slot)
        assert type(request) is int
        assert request > 0
        self.pending_requests[slot] = request
        assert slot in self.session.loading_sample_ids
        return request

    def restore_sources(self) -> None:
        """Start saved restoration only on the enclosing fixture's explicit request."""
        assert not self.source_history
        self.loader.restore_samples_from_project_state()
        assert not self.session.sample_load_errors, self.session.sample_load_errors

    def poll_load(self, slot: int) -> bool:
        """Project only the original Native success into the real Project model."""
        self.loader.poll_loader_events()
        assert slot not in self.session.sample_load_errors, self.session.sample_load_errors
        if slot in self.session.loading_sample_ids:
            return False
        reference = self.project.sample_paths[slot]
        content = self.project.pad_content[slot]
        if reference is None or content is None:
            return False
        native = self._audio.waveform_source_identity(slot)
        assert native is not None
        binding = self._audio.current_input_runtime_pad_binding(slot)
        assert binding is not None
        assert binding.is_current()
        metadata = binding.metadata()
        assert metadata["source_generation"] == native[0]
        request = self.pending_requests.get(slot, native[0])
        assert request == native[0]
        sha256 = hashlib.sha256(Path(reference).read_bytes()).hexdigest()
        assert self._audio.loaded_source_digest(slot) == sha256
        if slot in self.input_paths:
            assert hashlib.sha256(self.input_paths[slot].read_bytes()).hexdigest() == sha256
        history = self.source_history.setdefault(slot, [])
        if not history or history[-1].request != request:
            lifetime = metadata["content_lifetime"]
            assert type(lifetime) is int
            source = LoadedSource(
                request,
                content.instance_id,
                reference,
                sha256,
                native,
                lifetime,
            )
            if history:
                previous = history[-1]
                assert source.content_id != previous.content_id
                assert source.native[0] != previous.native[0]
                assert source.lifetime != previous.lifetime
            history.append(source)
            self.assert_stopped(slot)
        return True

    def poll_feedback(self) -> None:
        """Deliver the callback's actual packets through inherited App handling."""
        self._poll_audio_messages()

    def start(self, slot: int = 0) -> None:
        """Request a real public ordinary start; Rust owns readiness and effect."""
        self.transport.playback.trigger_pad_keep_others(slot)

    def press(self, slot: int = 0) -> HoldActionTicket:
        """Retain one real physical-press owner before its pause acknowledgement."""
        waveform = self.transport.waveform
        assert not waveform.has_pending_holds
        waveform.open_editor(slot)
        waveform.press_pause_hold(slot)
        assert waveform._physical_hold_press is not None, waveform.hold_status()
        assert len(waveform._hold_owners) == 1, waveform.hold_status()
        self.owner = next(iter(waveform._hold_owners.values()))
        self.hold = self.owner.ticket
        self.hold_slot = slot
        assert self.owner.physical_press_id == waveform._physical_hold_press
        return self.hold

    def observe_owned(self) -> None:
        """Observe the real Native pause acknowledgement without inventing it."""
        waveform = self.transport.waveform
        waveform.poll_holds()
        assert waveform.hold_status() == ("paused_owned", None), waveform.hold_status()
        assert self.owner is not None
        assert self.hold is not None
        assert waveform._physical_hold_press == self.owner.physical_press_id
        assert waveform._hold_owners[self.owner.physical_press_id] is self.owner
        assert self.session.waveform_pause_hold_pad_id == self.hold_slot

    def release(self) -> None:
        """Request release of the same original owner through its real controller."""
        assert self.hold is not None
        assert self.owner is not None
        waveform = self.transport.waveform
        waveform.request_hold_release("button_up")
        waveform.poll_holds()
        assert waveform._physical_hold_press is None
        assert self.owner.ticket is self.hold
        assert self.owner.release_requested

    def _hold_image(self) -> tuple[object, ...]:
        waveform = self.transport.waveform
        return (
            waveform._physical_hold_press,
            self.session.waveform_pause_hold_pad_id,
            tuple(
                (
                    press,
                    id(owner),
                    owner.release_requested,
                    owner.release_admitted,
                    owner.status,
                    owner.error,
                    owner.ticket.publication_status(),
                )
                for press, owner in waveform._hold_owners.items()
            ),
        )

    def reject_full_unload(self, slot: int = 0) -> None:
        """A real queue-full unload must leave source, model and physical Hold intact."""
        project = self.project.model_dump(mode="json")
        session = self.session.model_dump(mode="json")
        config = self.persistence.config_path
        persisted = config.read_bytes() if config.exists() else None
        persistence = (self.persistence.revision, self.persistence._dirty)
        source = self._audio.waveform_source_identity(slot)
        digest = self._audio.loaded_source_digest(slot)
        binding = self._audio.current_input_runtime_pad_binding(slot)
        assert binding is not None
        assert binding.is_current()
        hold = self._hold_image()
        assignments = self._assets._assignments.copy()
        with pytest.raises(RuntimeError, match="Failed to send UnloadSample - buffer may be full"):
            self.loader.unload_sample(slot)
        assert self.project.model_dump(mode="json") == project
        assert self.session.model_dump(mode="json") == session
        assert (config.read_bytes() if config.exists() else None) == persisted
        assert (self.persistence.revision, self.persistence._dirty) == persistence
        assert self._audio.waveform_source_identity(slot) == source
        assert self._audio.loaded_source_digest(slot) == digest
        assert binding.is_current()
        assert self._hold_image() == hold
        assert self._assets._assignments == assignments
        assert all(not lease.released for _, lease in assignments.values())

    def unload(self, slot: int = 0) -> None:
        """Use the actual Loader/App unload callbacks and asset retirement."""
        self.loader.unload_sample(slot)
        assert self.project.sample_paths[slot] is None
        assert self.project.pad_content[slot] is None
        self.assert_stopped(slot)

    def assert_stopped(self, slot: int = 0) -> None:
        """Check the fresh/retired source has no stale playback/pause projection."""
        assert slot not in self.session.active_sample_ids
        assert slot not in self.session.paused_sample_ids
        assert not self.session.pressed_pads[slot]
        assert self.session.pad_playhead_s[slot] is None
        assert self.session.pad_peak[slot] == 0.0

    def poll_terminal(self) -> None:
        """Remove an owner only after the real Native terminal acknowledgement."""
        waveform = self.transport.waveform
        waveform.poll_holds()
        assert not waveform.has_pending_holds, waveform.hold_status()
        assert waveform._physical_hold_press is None
        assert waveform.hold_status() == (None, None)
        assert self.session.waveform_pause_hold_pad_id is None

    def save(self) -> None:
        """Persist only the real Project image and retain its actual saved bytes."""
        self.persistence.flush()
        self.saved_bytes = self.persistence.config_path.read_bytes()
        saved = json.loads(self.saved_bytes)
        assert "waveform_pause_hold_pad_id" not in saved
        assert "paused_sample_ids" not in saved
        assert "_physical_hold_press" not in saved
        assert ProjectState.model_validate(saved) == self.project

    def prepare_close(self) -> bool:
        """Run actual App Close preparation with the original Hold owner alive."""
        return self.prepare_shutdown()

    def finish_close(self) -> None:
        """Run actual App shutdown; callback destruction remains Rust's obligation."""
        assignments = tuple(self._assets._assignments.values())
        self.shut_down()
        assert self._shutdown_complete
        assert not self._shutdown_running
        assert not self._shutdown_preparing
        assert not self._assets._assignments
        assert all(lease.released for _, lease in assignments)
        self.poll_terminal()
        saved = json.loads(self.persistence.config_path.read_bytes())
        assert "waveform_pause_hold_pad_id" not in saved
        assert "paused_sample_ids" not in saved
        assert "_physical_hold_press" not in saved

    def close(self) -> None:
        """Release owned test assignment leases and cwd without deleting evidence."""
        if self._closed:
            return
        try:
            self._assets.release_saved_assignments()
        finally:
            self._storage.close()
            self._closed = True
