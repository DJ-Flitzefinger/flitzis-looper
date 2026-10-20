"""Append genuine last-reader retirement checks to the completed recovery receiver.

No new engine, source/pair fixture, Native permission, retirement request or
cleanup path is created. Rust owns callbacks and observes actual artifact paths.
"""

import json
from pathlib import Path
from typing import TYPE_CHECKING

from tests.flitzis_looper.controller.bank_native_probe import BankControllerProbe

if TYPE_CHECKING:
    from tests.flitzis_looper.controller.bank_recovery_native_probe import RecoveryProbe


def _completed_startup(probe: RecoveryProbe) -> tuple[int, int]:
    startup = probe.controller._startup
    assert startup is not None
    assert not startup.active
    assert startup.status == "completed"
    assert startup._after is not None
    assert startup._after.acceptance is None
    return id(startup), 0


def begin(probe: RecoveryProbe) -> str:
    """Hold a real reader after the caller's recovery and continuity assertions."""
    assert not probe.controller.active
    assert probe.persistence._migration_owner is None
    assert probe._release_observed
    startup, acceptance = _completed_startup(probe)
    # The completed receipt-release observer describes the old recovery owner.
    # End that test-only patch before new real Bank owners release their writers.
    # Keep the actual completed startup alive; it must retain no Native reader.
    probe._release_patch.stop()
    assert probe.app.stems._stem_worker_pool is not None
    probe.hold_reader(36)
    probe.assert_reader()
    entry = probe.project.stem_cache[36]
    assert entry is not None
    pair = entry.pair
    assert pair is not None
    return json.dumps({
        "wav": str(Path(pair.wav_generation).absolute()),
        "stem_pcm": str(Path(pair.pcm_generation).absolute()),
        "descriptor": str(Path(pair.descriptor_reference).absolute()),
        "config": str(probe.persistence.config_path.absolute()),
        "startup_id": startup,
        "acceptance_id": acceptance,
        "reader_geometry": probe._reader_geometry,
        "reader_sha256": probe._reader_digest,
    })


def capture_clear(probe: RecoveryProbe, target_bank: int) -> None:
    """Use the existing capture and live Bank owner with its normal async pool."""
    assert target_bank in {0, 1, 2}
    _completed_startup(probe)
    probe.assert_reader()
    probe.capture(None, target_bank)
    probe.begin()
    assert probe.bank is not None
    assert probe.bank.request.bank.projected().source_bank is None
    assert probe.bank.request.bank.projected().target_bank == target_bank


def poll_publication(probe: RecoveryProbe) -> bool:
    """Collect actual preparation until its original publication awaits the callback."""
    bank = probe.bank
    assert bank is not None
    assert probe.controller._bank is bank
    probe.controller.poll()
    probe.app._sync_arrangement_observers()
    probe.app.loader.poll_loader_events()
    probe.app.stems.on_frame_render()
    assert probe.controller._bank is bank, probe.controller.error
    assert not probe.session.sample_load_errors, probe.session.sample_load_errors
    assert not probe.tasks
    if bank.ticket is None:
        return False
    assert bank.worker_done
    assert bank.published
    probe.ticket = bank.ticket
    assert probe.ticket.publication_status() == "pending", probe.controller.error
    assert probe.ticket.acknowledgement() is None
    assert bank.journal is not None
    assert bank.journal.journal is not None
    assert bank.journal.journal.phase == "publication_pending"
    probe._assert_unchanged()
    return True


def settle_clear(probe: RecoveryProbe) -> str:
    """Settle the same actual ACK through the complete existing mirror/receipt checks."""
    BankControllerProbe.settle(probe)
    probe.assert_reader()
    return probe.settlement_json()


def forget_clear(probe: RecoveryProbe) -> None:
    """Release finished test ticket/mirror holders after Rust dropped its own wrappers."""
    assert probe.bank is not None
    assert probe.bank.native_finished
    assert probe.bank.writer_released
    assert not probe.controller.active
    probe.forget_settled()
    assert probe.bank is None
    assert probe.request is None
    assert probe.ticket is None
    probe.poll_retirements()
    probe.assert_reader()


def assert_held(probe: RecoveryProbe, startup_id: int, acceptance_id: int) -> str:
    """Preserve completed recovery owners while an independently held reader still reads."""
    assert _completed_startup(probe) == (startup_id, acceptance_id)
    assert all(path is None for path in probe.project.sample_paths)
    assert all(content is None for content in probe.project.pad_content)
    assert not probe.controller.active
    assert probe.persistence._migration_owner is None
    assert probe.reader is not None
    probe.assert_reader()
    return json.dumps({"geometry": probe._reader_geometry, "sha256": probe._reader_digest})


def release_reader(probe: RecoveryProbe) -> None:
    """Drop only the explicitly held reader and use ordinary retirement collection."""
    probe.release_reader()
    assert probe.reader is None


def poll_retirements(probe: RecoveryProbe) -> None:
    """Poll normal Loader/Stem/asset collectors; no manual retire or deletion."""
    probe.poll_retirements()
    probe.app.stems.on_frame_render()


def diagnostics(probe: RecoveryProbe) -> str:
    """Expose actual ownership and cleanup diagnostics without claiming retirement."""
    startup, acceptance = _completed_startup(probe)
    return json.dumps({
        "startup_id": startup,
        "acceptance_id": acceptance,
        "bank_status": probe.controller.status,
        "bank_error": probe.controller.error,
        "held_reader": probe.reader is not None,
        "assignments": len(probe.assets._assignments),
        "load_errors": probe.session.sample_load_errors,
        "cleanup": probe.audio.project_asset_cleanup_status(),
        "cleanup_admission": probe.audio.project_asset_cleanup_admission_status(),
    })
