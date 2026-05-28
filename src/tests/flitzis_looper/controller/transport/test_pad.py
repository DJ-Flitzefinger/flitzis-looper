from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.constants import PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_DB_MAX, PAD_GAIN_DB_MIN
from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


@pytest.mark.parametrize(
    ("gain_db", "exp"),
    [(2.5, 2.5), (-70.0, PAD_GAIN_DB_MIN), (20.0, PAD_GAIN_DB_MAX)],
)
def test_set_pad_gain_clamps_and_calls_engine(
    controller: AppController, audio_engine_mock: Mock, gain_db: float, exp: float
) -> None:
    controller.transport.pad.set_pad_gain(0, gain_db)
    audio_engine_mock.set_pad_gain.assert_called_with(0, exp)
    assert controller.project.pad_gain_db[0] == exp


def test_set_pad_eq_clamps_and_calls_engine(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0

    controller.transport.pad.set_pad_eq(sample_id, 1.0, -2.0, 3.0)
    audio_engine_mock.set_pad_eq.assert_called_with(sample_id, 1.0, -2.0, 3.0)
    assert controller.project.pad_eq_low_db[sample_id] == 1.0
    assert controller.project.pad_eq_mid_db[sample_id] == -2.0
    assert controller.project.pad_eq_high_db[sample_id] == 3.0

    controller.transport.pad.set_pad_eq(sample_id, 999.0, -999.0, 0.0)
    audio_engine_mock.set_pad_eq.assert_called_with(
        sample_id,
        PAD_EQ_DB_MAX,
        PAD_EQ_DB_MIN,
        0.0,
    )
    assert controller.project.pad_eq_low_db[sample_id] == PAD_EQ_DB_MAX
    assert controller.project.pad_eq_mid_db[sample_id] == PAD_EQ_DB_MIN


def test_set_pad_key_lock_updates_only_one_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.sample_paths[3] = "samples/foo.wav"
    controller.project.sample_paths[4] = "samples/bar.wav"
    controller.project.pad_key_lock[4] = True
    enabled = True

    controller.transport.pad.set_pad_key_lock(3, enabled=enabled)

    audio_engine_mock.set_pad_key_lock.assert_called_once_with(3, enabled)
    assert controller.project.pad_key_lock[3] is True
    assert controller.project.pad_key_lock[4] is True
    assert controller.project.pad_key_lock[2] is False


def test_toggle_pad_key_lock(controller: AppController, audio_engine_mock: Mock) -> None:
    controller.project.sample_paths[3] = "samples/foo.wav"
    enabled = True

    controller.transport.pad.toggle_pad_key_lock(3)

    audio_engine_mock.set_pad_key_lock.assert_called_once_with(3, enabled)
    assert controller.project.pad_key_lock[3] is True


def test_set_pad_key_lock_no_op(controller: AppController, audio_engine_mock: Mock) -> None:
    controller.project.sample_paths[3] = "samples/foo.wav"
    controller.transport.pad.set_pad_key_lock(3, enabled=False)

    audio_engine_mock.set_pad_key_lock.assert_not_called()


def test_set_pad_key_lock_ignores_unloaded_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.transport.pad.set_pad_key_lock(3, enabled=True)

    audio_engine_mock.set_pad_key_lock.assert_not_called()
    assert controller.project.pad_key_lock[3] is False


def test_set_pad_key_lock_resets_stale_unloaded_pad(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    controller.project.pad_key_lock[3] = True
    disabled = False

    controller.transport.pad.set_pad_key_lock(3, enabled=True)

    audio_engine_mock.set_pad_key_lock.assert_called_once_with(3, disabled)
    assert controller.project.pad_key_lock[3] is False


def test_poll_audio_messages_updates_session_peak(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    now = 123.0
    monkeypatch.setattr("flitzis_looper.controller.metering.monotonic", lambda: now)

    msg1 = Mock()
    msg1.sample_id.return_value = 0
    msg1.pad_peak.return_value = 0.8
    msg1.pad_playhead.return_value = 0.1
    msg2 = Mock()
    msg2.sample_id.return_value = 0
    msg2.pad_peak.return_value = 1.2
    msg2.pad_playhead.return_value = 0.2
    msg3 = Mock()
    msg3.sample_id.return_value = 0
    msg3.pad_peak.return_value = None
    msg3.pad_playhead.return_value = None

    controller.metering.handle_pad_peak_message(msg1)
    controller.metering.handle_pad_playhead_message(msg1)
    controller.metering.handle_pad_peak_message(msg2)
    controller.metering.handle_pad_playhead_message(msg2)
    controller.metering.handle_pad_peak_message(msg3)
    controller.metering.handle_pad_playhead_message(msg3)

    assert controller.session.pad_peak[0] == pytest.approx(1.0)
    assert controller.session.pad_peak_updated_at[0] == now
    assert controller.session.pad_playhead_s[0] == pytest.approx(0.2)
    assert controller.session.pad_playhead_updated_at[0] == now


def test_set_and_clear_manual_key(controller: AppController) -> None:
    sample_id = 0

    controller.transport.pad.set_manual_key(sample_id, "Gm")
    assert controller.project.manual_key[sample_id] == "Gm"

    controller.transport.pad.clear_manual_key(sample_id)
    assert controller.project.manual_key[sample_id] is None


def test_effective_key_prefers_manual(controller: AppController) -> None:
    sample_id = 0

    controller.project.sample_analysis[sample_id] = SampleAnalysis(
        bpm=123.4,
        key="C#m",
        beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0], bars=[0.0]),
    )
    assert controller.transport.pad.effective_key(sample_id) == "C#m"

    controller.transport.pad.set_manual_key(sample_id, "Gm")
    assert controller.transport.pad.effective_key(sample_id) == "Gm"


def test_set_manual_key_empty_raises(controller: AppController) -> None:
    sample_id = 0

    with pytest.raises(ValueError, match="key must be a non-empty string"):
        controller.transport.pad.set_manual_key(sample_id, "")


@pytest.mark.parametrize("gain", ["nan", "inf", "-inf"])
def test_set_pad_gain_non_finite_raises(controller: AppController, gain: str) -> None:
    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.pad.set_pad_gain(0, float(gain))


@pytest.mark.parametrize("db", ["nan", "inf", "-inf"])
def test_set_pad_eq_non_finite_raises(controller: AppController, db: str) -> None:
    sample_id = 0

    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.pad.set_pad_eq(sample_id, float(db), 0.0, 0.0)

    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.pad.set_pad_eq(sample_id, 0.0, float(db), 0.0)

    with pytest.raises(ValueError, match="value must be finite"):
        controller.transport.pad.set_pad_eq(sample_id, 0.0, 0.0, float(db))
