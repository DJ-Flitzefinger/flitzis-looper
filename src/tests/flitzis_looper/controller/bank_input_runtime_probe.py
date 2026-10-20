"""Actual InputMapping frame publication during one existing complete Bank owner.

The delegate supplies the real App participant guard and the existing component
owners. No source, runtime publication, preparation, ACK or persistence behavior
is mocked. Rust supplies the device-free input runtime and advances its callback.
"""

from typing import TYPE_CHECKING, cast

from flitzis_looper.controller import app as app_module
from flitzis_looper.input_mapping.actions import LooperAction
from flitzis_looper.input_mapping.controller import InputMappingController

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from tests.flitzis_looper.controller.bank_native_probe import BankControllerProbe


class _InputAppDelegate:
    """Reuse App's actual participant checks around the original Bank envelope."""

    def __init__(self, probe: BankControllerProbe) -> None:
        self._probe = probe

    def __getattr__(self, name: str) -> object:
        return getattr(self._probe.app, name)

    def _restored_assignment_held(self, sample_id: int) -> bool:
        return app_module.AppController._restored_assignment_held(
            cast("AppController", self), sample_id
        )

    def ensure_not_copy_target(self, sample_id: int) -> None:
        app_module.AppController.ensure_not_copy_target(cast("AppController", self), sample_id)


class BankInputRuntimeProbe:
    """Join actual InputMapping publication to the same captured Bank and Config."""

    def __init__(self, probe: object) -> None:
        self.probe = cast("BankControllerProbe", probe)
        self.app = _InputAppDelegate(self.probe)
        self._edit_guard = self.probe.app.transport._pad_edit_guard
        self._pitch_refresh = self.probe.app.transport.pitch._refresh_runtime
        self.probe.app.transport.bind_pad_edit_guard(self.app.ensure_not_copy_target)
        self.mapping = InputMappingController(cast("AppController", self.app))
        self.mapping.on_frame_render()
        self._assert_refreshed()
        signature = self.mapping._last_input_runtime_state_signature
        assert signature is not None
        assert signature[2][0].ready
        assert signature[2][72].ready

    def _assert_refreshed(self) -> None:
        assert self.mapping._last_input_runtime_state_error is None

    def begin(self) -> None:
        """Admit the Bank before publishing a pending outside pitch-row update."""
        # Persist the independently valid outside intent before taking the Bank
        # baseline, but let its real InputMapping publication occur while Bank owns
        # participants. This needs no artificial edits to the baseline assertions.
        project = self.probe.project
        project.pad_key_intent[72] = project.pad_key_intent[72].changed(extra_shift=2)
        self.probe.persistence.mark_dirty()
        self.probe.persistence.flush()
        self.probe.capture(0, 1)
        self.probe.begin()
        assert self.probe.bank is not None
        assert self.probe.bank.request is self.probe.request
        assert 0 in self.probe.app.transport._pair_deferred_sample_ids
        assert 36 in self.probe.app.transport._pair_deferred_sample_ids
        assert 72 not in self.probe.app.transport._pair_deferred_sample_ids

    def refresh(self) -> None:
        """Run the actual frame callbacks, including the original runtime publisher."""
        self.mapping.on_frame_render()
        self._assert_refreshed()

    def outside_controls(self) -> None:
        """Publish a real mapping and verify outside pitch sees the updated row."""
        self.mapping.save_mapping("midi", "midi:note:1:60", LooperAction.stop_pad(72))
        capture = self.probe.audio.capture_pad_pitch_action(72, "pitch.extra.step:1:set")
        assert capture.extra_shift == 3
        assert capture.state == "captured"
        assert self.probe.audio.cancel_pitch_action(capture)
        # Cancellation returns the Native requested shadow to the same outside
        # projection; it does not publish an audio effect or change Bank authority.
        assert self.probe.project.pad_key_intent[72].extra_shift == 2
        if self.probe.controller.active:
            self.probe._assert_unchanged()
        else:
            assert self.probe.bank is not None
            self.probe._assert_preserved(self.probe.bank)

    def assert_restored_runtime(self) -> None:
        """After the own ACK/Config receipt, fresh copied bindings become visible."""
        assert not self.probe.controller.active
        self.refresh()
        signature = self.mapping._last_input_runtime_state_signature
        assert signature is not None
        assert all(signature[2][slot].ready for slot in (0, 36, 72))
        assert self.probe.project.selected_bank == 0

    def supersede_multi_loop(self) -> None:
        """Publish a deliberate global mode change against the pending original Bank."""
        bank = self.probe.bank
        assert bank is not None
        assert bank.ticket is not None
        assert bank.ticket.publication_status() == "pending"
        enabled = not self.probe.project.multi_loop
        self.probe.app.transport.global_params.set_multi_loop(enabled=enabled)
        self.refresh()
        assert self.probe.project.multi_loop is enabled
        assert self.probe.persistence.revision == self.probe._before_revision + 1
        assert self.probe.persistence.config_path.read_bytes() == self.probe._before_config

    def settle_superseded(self) -> None:
        """Retain changed global intent while the unacknowledged Bank cancels unchanged."""
        probe = self.probe
        bank, ticket = probe.bank, probe.ticket
        assert bank is not None
        assert ticket is not None
        assert ticket.publication_status() == "rejected"
        assert ticket.acknowledgement() is None
        probe.controller.poll()
        assert not probe.controller.active
        assert probe.controller.status == "cancelled"
        assert bank.cancellation_proven
        assert bank.writer_released
        assert not bank.native_finished
        assert bank.acknowledgement is None
        assert bank.receipt is None
        assert bank.mirror is None
        assert probe.persistence._migration_owner is None
        assert len(probe.receipts) == probe._receipt_start
        assert probe.persistence.config_path.read_bytes() == probe._before_config
        expected = probe._before_project | {"multi_loop": probe.project.multi_loop}
        assert probe.project.model_dump(mode="json", exclude_computed_fields=True) == expected
        assert probe.session.model_dump(mode="python") == probe._before_session
        probe._assert_assignments()
        probe.persistence.flush()
        self.assert_restored_runtime()

    def begin_clear(self) -> None:
        """Capture a fresh complete Clear after the earlier original owner is terminal."""
        assert not self.probe.controller.active
        self.probe.capture(None, 1)
        self.probe.begin()
        assert self.probe.bank is not None
        assert self.probe.bank.request is self.probe.request
        assert 36 in self.probe.app.transport._pair_deferred_sample_ids
        assert 0 not in self.probe.app.transport._pair_deferred_sample_ids
        assert 72 not in self.probe.app.transport._pair_deferred_sample_ids

    def assert_cleared_runtime(self) -> None:
        """A terminal Clear releases its cached participant row instead of restoring it."""
        assert not self.probe.controller.active
        self.refresh()
        signature = self.mapping._last_input_runtime_state_signature
        assert signature is not None
        assert all(signature[2][slot].ready for slot in (0, 72))
        assert signature[2][36].sample_path is None
        assert not signature[2][36].ready
        assert signature[2][36].binding is None
        assert self.probe.project.sample_paths[36] is None
        assert self.probe.project.pad_content[36] is None
        assert self.probe.project.selected_bank == 0

    def close(self) -> None:
        """Restore only fixture callback wiring after the actual Bank settles."""
        assert not self.probe.controller.active
        self.probe.app.transport._pad_edit_guard = self._edit_guard
        self.probe.app.transport.pitch._refresh_runtime = self._pitch_refresh
