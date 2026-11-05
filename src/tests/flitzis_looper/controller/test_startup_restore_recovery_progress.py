"""Public journal/Config startup progress with a scripted Native admission boundary.

These checks prove Controller retry ordering and ownership, not source ACKs,
actual audio loading, callback effects or device acceptance.
"""

from typing import TYPE_CHECKING

import pytest

from flitzis_looper.bank_snapshot import capture_bank_snapshot
from flitzis_looper.controller.app import AppController
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import PadContentIdentity, ProjectState
from tests.flitzis_looper.conftest import _settle_output_control_mock

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator
    from pathlib import Path
    from unittest.mock import Mock


@pytest.fixture
def start_app() -> Iterator[Callable[[Path], AppController]]:
    """Join real output workers up to activation, retaining pending recovery."""
    apps: list[AppController] = []

    def start(config: Path) -> AppController:
        app = AppController(project_config_path=config)
        apps.append(app)
        _settle_output_control_mock(app, activation_only=True)
        return app

    try:
        yield start
    finally:
        for app in apps:
            operation = app.audio_output._operation
            if operation is not None:
                assert operation.thread is not None
                operation.thread.join(2)
                assert not operation.thread.is_alive()
            app.stems.shut_down()


def _captured_startup(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Persist one real interrupted Bank capture before starting the new App."""
    monkeypatch.chdir(tmp_path)
    samples = tmp_path / "samples"
    samples.mkdir()
    (samples / "outside.wav").write_bytes(b"bounded original source fixture")
    project = ProjectState(key_lock=False)
    for sample_id in (*range(8), *range(72, 89)):
        project.sample_paths[sample_id] = (
            f"samples/missing{sample_id}.wav" if sample_id < 8 else "samples/outside.wav"
        )
        project.pad_content[sample_id] = PadContentIdentity(
            instance_id=f"{sample_id + 1:032x}", material_id=None
        )
        project.pad_gain_db[sample_id] = -9.0
    persistence = ProjectPersistence(project)
    persistence.config_path = samples / "project.json"
    persistence.flush()
    transaction = "a" * 32
    revision, image, digest = persistence.capture_migration(transaction)
    journal = BankArrangementJournal(
        transaction_id=transaction,
        config_reference=persistence.config_reference,
        captured_revision=revision,
        intent_revision=revision,
        captured_config_sha256=digest,
        bank=capture_bank_snapshot(image, 0, 1),
        captured_project_image_json=image.model_dump_json(),
        project_image_json=image.model_dump_json(),
        image_role="before",
        phase="captured",
    )
    BankJournalOwner(samples, journal).append(journal)
    persistence.release_migration(transaction)
    return persistence.config_path


def test_real_recovery_missing_originals_allow_bounded_outside_restore_progress(
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    start_app: Callable[[Path], AppController],
) -> None:
    config = _captured_startup(tmp_path, monkeypatch)
    original_config = config.read_bytes()
    audio_engine_mock.load_sample_async.side_effect = lambda sample_id, *_args, **_kw: sample_id + 1
    audio_engine_mock.poll_loader_events.return_value = None
    app = start_app(config)

    assert app.banks.status == "recovery_pending"
    startup = app.banks._startup
    assert startup is not None
    assert not startup._unknown
    assert startup.histories[0].initial.phase == "captured"
    assert startup.error is not None
    assert "original custody is missing" in startup.error
    assert app.banks.fenced_sample_ids == frozenset(range(72))
    assert app.banks.observer_fenced_sample_ids == frozenset()
    assert [item.args[0] for item in audio_engine_mock.load_sample_async.call_args_list] == list(
        range(72, 80)
    )
    assert list(app.loader._deferred_restores) == [*range(8), *range(80, 89)]
    before = app.project.model_dump()
    held_content = app.loader._deferred_restore_content.copy()
    held_errors = {sample_id: app.session.sample_load_errors[sample_id] for sample_id in range(8)}

    app.loader.poll_loader_events()

    assert [item.args[0] for item in audio_engine_mock.load_sample_async.call_args_list] == list(
        range(72, 88)
    )
    assert list(app.loader._deferred_restores) == [*range(8), 88]
    app.loader.poll_loader_events()
    assert [item.args[0] for item in audio_engine_mock.load_sample_async.call_args_list] == list(
        range(72, 89)
    )
    assert list(app.loader._deferred_restores) == list(range(8))
    assert app.loader._deferred_restore_content == {
        sample_id: held_content[sample_id] for sample_id in range(8)
    }
    assert app.session.sample_load_errors == held_errors
    assert app.session.loading_sample_ids == {*range(8), *range(72, 89)}
    assert app.project.model_dump() == before
    assert config.read_bytes() == original_config

    # The same recovery-owned originals can resume after their actual files return.
    for sample_id in range(8):
        (tmp_path / "samples" / f"missing{sample_id}.wav").write_bytes(b"returned original")
    app.loader.poll_loader_events()
    assert [item.args[0] for item in audio_engine_mock.load_sample_async.call_args_list] == [
        *range(72, 89),
        *range(8),
    ]
    assert not app.loader._deferred_restores
    assert not app.loader._deferred_restore_content
    assert not app.session.sample_load_errors
    assert app.project.model_dump() == before
    assert config.read_bytes() == original_config
    assert app.banks.active
    audio_engine_mock.unload_sample.assert_not_called()


def test_recovery_restore_budget_counts_failed_native_admission_attempts(
    audio_engine_mock: Mock,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    start_app: Callable[[Path], AppController],
) -> None:
    config = _captured_startup(tmp_path, monkeypatch)
    audio_engine_mock.load_sample_async.side_effect = RuntimeError(
        "[resident_memory_pressure] insufficient host allocation headroom"
    )
    audio_engine_mock.poll_loader_events.return_value = None
    app = start_app(config)
    before = app.project.model_dump()
    assert audio_engine_mock.load_sample_async.call_count == 8
    audio_engine_mock.load_sample_async.reset_mock()

    app.loader.poll_loader_events()

    assert [item.args[0] for item in audio_engine_mock.load_sample_async.call_args_list] == list(
        range(72, 80)
    )
    assert len(app.loader._deferred_restores) == 25
    assert list(app.loader._deferred_restores) == [*range(8), *range(80, 89), *range(72, 80)]
    assert app.session.loading_sample_ids == {*range(8), *range(72, 89)}
    assert app.project.model_dump() == before
    assert app.banks.active
