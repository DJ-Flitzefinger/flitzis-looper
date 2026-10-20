"""Interrupted startup initialization retains and resumes its exact Pair recovery."""

import hashlib
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import pair_arrangement
from flitzis_looper.controller.pair_arrangement import PairArrangementController
from flitzis_looper.models import PadContentIdentity, ProjectState
from tests.flitzis_looper.controller.test_pair_arrangement import _Audio, _Mirror, _StemMirror
from tests.flitzis_looper.controller.test_pair_arrangement_journal import _TRANSACTION
from tests.flitzis_looper.controller.test_pair_arrangement_startup import (
    _app,
    _journal,
    _prior,
    _startup,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller import AppController
    from flitzis_looper.pair_snapshot import PairSnapshot


PRESSURE = "startup initialization capacity temporarily unavailable"


def _pressure(app: SimpleNamespace, monkeypatch: pytest.MonkeyPatch, boundary: str) -> Mock:
    owner, name = {
        "reserve": (app._assets, "reserve"),
        "restore": (app.persistence, "recover_migration_intent"),
        "assignments": (app._assets, "sync_assignments"),
    }[boundary]
    refusal = Mock(wraps=getattr(owner, name), side_effect=RuntimeError(PRESSURE))
    monkeypatch.setattr(owner, name, refusal)
    return refusal


def _original_controller(
    app: SimpleNamespace, monkeypatch: pytest.MonkeyPatch
) -> PairArrangementController:
    app._audio = _Audio()
    app.arrangement = SimpleNamespace(active=False)
    app.stems = SimpleNamespace(_stem_task_runner=lambda run: run())
    app._pair_waveform_views = []

    def prepare_control(_app: AppController, snapshot: PairSnapshot, **_options: object) -> _Mirror:
        return _Mirror(app, snapshot)

    def prepare_stems(*_args: object, **_options: object) -> _StemMirror:
        return _StemMirror()

    monkeypatch.setattr(pair_arrangement, "prepare_pair_control_mirror", prepare_control)
    monkeypatch.setattr(pair_arrangement, "prepare_pair_stem_mirror", prepare_stems)
    return PairArrangementController(cast("AppController", app))


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
def test_after_recovery_retries_initial_reservation_then_uses_one_fresh_acceptance(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, error_type: type[Exception]
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=True)
    app = _app(config)
    refusal = _pressure(app, monkeypatch, "reserve")
    refusal.side_effect = error_type(PRESSURE)
    capture_writer = Mock(wraps=app.persistence.capture_migration)
    monkeypatch.setattr(app.persistence, "capture_migration", capture_writer)
    startup = _startup(app)
    assert startup.status == "recovery_pending"
    assert startup.error == PRESSURE
    assert startup._retirement is None
    (history,) = startup.histories
    encoded = history.store.history()
    before = config.read_bytes()
    refusal.side_effect = None

    assert startup.poll() is None

    assert refusal.call_count == 2
    capture_writer.assert_called_once_with(_TRANSACTION)
    assert startup.histories == [history]
    assert startup._after is not None
    assert startup._retirement is app._assets.retirement
    assert app._audio.captures == []
    assert app.persistence._migration_owner == _TRANSACTION
    assert history.store.history() == encoded
    assert config.read_bytes() == before
    app._audio.ready = True

    assert startup.poll() is None

    assert not startup.active
    assert startup.status == "completed"
    assert len(app._audio.captures) == 1
    acceptance = app._audio.captures[0][2]
    assert app._audio.releases == [acceptance]
    assert acceptance.released
    assert app.persistence._migration_owner is None
    assert app._assets.retirement is not None
    assert app._assets.retirement.closed == 1
    settled = _journal(tmp_path / "samples", _TRANSACTION)
    assert settled.phase == "settled"
    assert settled.committed_config_sha256 == hashlib.sha256(config.read_bytes()).hexdigest()


@pytest.mark.parametrize("error_type", [RuntimeError, ValueError])
@pytest.mark.parametrize("boundary", ["reserve", "restore", "assignments"])
def test_original_initialization_retries_before_handing_one_actual_pair_to_coordinator(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
    boundary: str,
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, unsaved=True)
    app = _app(config)
    refusal = _pressure(app, monkeypatch, boundary)
    refusal.side_effect = error_type(PRESSURE)
    reserve = refusal if boundary == "reserve" else Mock(wraps=app._assets.reserve)
    if boundary != "reserve":
        monkeypatch.setattr(app._assets, "reserve", reserve)
    controller = _original_controller(app, monkeypatch)
    startup = controller._startup
    assert startup.status == "recovery_pending"
    assert startup.error == PRESSURE
    (history,) = startup.histories
    encoded = history.store.history()
    before = config.read_bytes()
    app.session.loading_sample_ids.add(0)
    refusal.side_effect = None

    controller.poll()

    assert refusal.call_count == 2
    assert startup.histories == [history]
    assert startup.status == "restore_original"
    assert app.project.volume == 0.25
    assert app.project.pad_gain_db[36] == -24.0
    assert app._assets.synced
    assert app._audio.captures == []
    assert history.store.history() == encoded
    assert config.read_bytes() == before
    assert app.persistence._migration_owner == _TRANSACTION
    assert reserve.call_count == (2 if boundary == "reserve" else 1)
    app.session.loading_sample_ids.clear()
    for _ in range(8):
        controller.poll()
        if not controller.active:
            break

    assert not controller.active, controller.error
    assert controller.status == "completed"
    assert len(app._audio.captures) == len(app._audio.tickets) == 1
    assert len(app._audio.publish_calls) == len(app._audio.finish_calls) == 1
    assert app._audio.cancel_calls == []
    assert app.persistence._migration_owner is None
    assert app._assets.retirement.closed == 1
    saved = ProjectState.model_validate_json(config.read_bytes())
    assert saved.volume == 0.25
    assert saved.pad_gain_db[36] == -24.0
    assert saved.pad_content[0] is not None
    assert saved.pad_content[215] is not None
    assert saved.pad_content[0].instance_id == "2" * 32
    assert saved.pad_content[215].instance_id == "1" * 32
    handoff = startup._handoff
    assert handoff is not None
    assert handoff.completed
    assert _journal(tmp_path / "samples", handoff.owner).phase == "settled"


@pytest.mark.parametrize("boundary", ["reserve", "restore", "assignments"])
def test_persistent_initialization_refusal_retains_same_history_without_native_capture(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, boundary: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, unsaved=True)
    app = _app(config)
    refusal = _pressure(app, monkeypatch, boundary)
    reserve = refusal if boundary == "reserve" else Mock(wraps=app._assets.reserve)
    if boundary != "reserve":
        monkeypatch.setattr(app._assets, "reserve", reserve)
    controller = _original_controller(app, monkeypatch)
    startup = controller._startup
    (history,) = startup.histories
    encoded = history.store.history()
    before = config.read_bytes()

    for attempt in range(1, 4):
        controller.poll()

        assert refusal.call_count == attempt + 1
        assert controller.active
        assert controller._pair is None
        assert startup.histories == [history]
        assert startup.error == PRESSURE
        assert app.persistence._migration_owner == _TRANSACTION
        assert app._audio.captures == []
        assert history.store.history() == encoded
        assert config.read_bytes() == before
        assert reserve.call_count == (attempt + 1 if boundary == "reserve" else 1)
        assert app._assets.retirement.closed == 0


@pytest.mark.parametrize("layout", ["original", "after"])
def test_retry_keeps_replaced_participant_and_does_not_capture_new_content(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, layout: str
) -> None:
    config, _ = _prior(tmp_path, monkeypatch, after=layout == "after")
    app = _app(config)
    refusal = _pressure(app, monkeypatch, "reserve")
    startup = _startup(app)
    (history,) = startup.histories
    encoded = history.store.history()
    before = config.read_bytes()
    app.project.sample_paths[0] = "samples/replacement.wav"
    app.project.pad_content[0] = PadContentIdentity(instance_id="f" * 32)
    app.persistence.mark_dirty()
    replacement = app.project.model_dump_json()
    app._audio.ready = True
    refusal.side_effect = None

    assert startup.poll() is None

    assert startup.active
    assert startup.error is not None
    assert "changed" in startup.error or "untouched" in startup.error
    assert app.project.model_dump_json() == replacement
    assert app._audio.captures == []
    assert app._audio.releases == []
    assert app.persistence._migration_owner == _TRANSACTION
    assert history.store.history() == encoded
    assert config.read_bytes() == before
