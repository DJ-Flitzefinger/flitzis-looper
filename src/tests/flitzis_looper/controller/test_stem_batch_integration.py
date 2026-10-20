"""Real App/batch/pool/config joins; audio and separator fixtures are substitutes."""

from typing import TYPE_CHECKING

from flitzis_looper.controller.performance_confirmation import StemBatchConfirmation
from flitzis_looper.controller.stem_job import DIRECT_STEM_INTEREST, StemGenerationInterests
from flitzis_looper.models import PadContentIdentity
from tests.flitzis_looper.controller import test_stem_disk_commit as disk_fixtures
from tests.flitzis_looper.controller.test_stem_admission import _held_pool
from tests.flitzis_looper.controller.test_stem_pending_admission import _production_pool

disk_commit = disk_fixtures.disk_commit

if TYPE_CHECKING:
    from pathlib import Path
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController
    from tests.flitzis_looper.controller.test_stem_disk_commit import _DiskCommit


def _loaded_metadata(controller: AppController, audio: Mock, pads: tuple[int, ...]) -> None:
    identities = {}
    for sample_id in pads:
        version = controller.stems._loaded_source_version_for_pad(sample_id)
        assert version is not None
        source = audio.capture_offline_stem_source(sample_id, version)
        identities[sample_id] = (
            source.source_generation,
            source.digest,
            source.target_shape[2],
            source.target_shape[0],
        )
    # Cached no-I/O envelopes are prepared before the observed warning boundary.
    audio.waveform_source_identity.side_effect = identities.get
    audio.capture_offline_stem_source.reset_mock()


def test_real_hidden_settings_batch_keeps_snapshot_and_shared_direct_interest(
    controller: AppController, audio_engine_mock: Mock, tmp_path: Path
) -> None:
    with _held_pool() as held:
        held.fill(32)
        _production_pool(controller, audio_engine_mock, tmp_path, held.pool)
        _loaded_metadata(controller, audio_engine_mock, (0, 215))
        controller.project.selected_pad = 215
        controller.project.selected_bank = 5
        controller.session.settings_open = False
        stems, batch, warning = (
            controller.stems,
            controller.stem_batch,
            controller.performance_confirmation,
        )
        reserved = stems._assets._reserved
        warning.request_all_loaded_stems()
        intent = warning.pending
        assert isinstance(intent, StemBatchConfirmation)
        assert intent.inventory.items[0] is not None
        assert intent.inventory.items[215] is not None
        assert not stems._jobs
        assert not stems._requested_generations
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        controller.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
        assert warning.is_current(intent)  # Still-matching originals can proceed.
        warning.accept(intent)
        warning.accept(intent)  # Exact warning is consumed once.
        controller.on_frame_render()
        summary = batch.summary
        assert summary is not None
        assert (summary.total, summary.skipped, summary.queued) == (2, 1, 1)
        assert summary.items[0].pad_id == 0
        assert summary.items[1].pad_id == 215
        requested = stems._requested_generations[215]
        assert requested.capture is intent.inventory.items[215]
        assert stems.generate_stems_async(215)
        assert DIRECT_STEM_INTEREST in requested.owner.current
        batch.cancel()
        summary = batch.summary
        assert summary is not None
        assert (summary.skipped, summary.cancelled, summary.active) == (1, 1, False)
        assert requested.owner.current == frozenset({DIRECT_STEM_INTEREST})
        assert stems._requested_generations[215] is requested
        controller.on_frame_render()
        assert (controller.project.selected_pad, controller.project.selected_bank) == (215, 5)
        assert not controller.session.settings_open
        assert stems._assets._reserved == reserved
        assert not stems._jobs
        audio_engine_mock.capture_offline_stem_source.assert_not_called()
        assert not stems.delete_stems(215)


def test_real_batch_done_waits_for_config_replace_and_late_cancel_keeps_result(
    disk_commit: _DiskCommit,
) -> None:
    rig = disk_commit
    app, stems = rig.controller, rig.controller.stems
    _loaded_metadata(app, rig.audio, (0,))
    owner = StemGenerationInterests()  # Existing direct Pair work stays independent.
    rig.request.interest_owner = owner
    app.session.settings_open = False
    selected = app.project.selected_pad, app.project.selected_bank
    app.performance_confirmation.request_all_loaded_stems()
    intent = app.performance_confirmation.pending
    assert isinstance(intent, StemBatchConfirmation)
    app.performance_confirmation.accept(intent)
    app.on_frame_render()
    summary = app.stem_batch.summary
    assert summary is not None
    assert (summary.total, summary.queued, summary.skipped) == (2, 1, 1)
    assert len(owner.current) == 2
    assert rig.request.interest_owner is owner
    assert not rig.request.started.is_set()
    assert not owner.committed
    rig.jobs.pop()()
    app.stem_batch.poll()
    summary = app.stem_batch.summary
    assert summary is not None
    assert summary.running == 1
    assert summary.done == 0
    assert app._persistence.config_path.read_bytes() == rig.encoded
    stems.on_frame_render()
    app.stem_batch.poll()
    summary = app.stem_batch.summary
    assert summary is not None
    assert (summary.done, summary.skipped, summary.active) == (1, 1, False)
    assert summary.items[0].detail == "Stems saved"
    assert owner.committed
    saved = app._persistence.config_path.read_bytes()
    assert saved != rig.encoded
    assert rig.prepared.selected
    app.stem_batch.cancel()
    assert app.stem_batch.summary == summary
    assert app._persistence.config_path.read_bytes() == saved
    assert (app.project.selected_pad, app.project.selected_bank) == selected
