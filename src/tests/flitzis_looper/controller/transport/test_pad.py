from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.constants import PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_MAX, PAD_GAIN_MIN
from flitzis_looper.models import BeatGrid, SampleAnalysis

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController


def test_set_pad_gain_clamps_and_calls_engine(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0

    controller.transport.pad.set_pad_gain(sample_id, 0.25)
    audio_engine_mock.return_value.set_pad_gain.assert_called_with(sample_id, 0.25)
    assert controller.project.pad_gain[sample_id] == 0.25

    controller.transport.pad.set_pad_gain(sample_id, -1.0)
    audio_engine_mock.return_value.set_pad_gain.assert_called_with(sample_id, PAD_GAIN_MIN)
    assert controller.project.pad_gain[sample_id] == PAD_GAIN_MIN

    controller.transport.pad.set_pad_gain(sample_id, 2.0)
    audio_engine_mock.return_value.set_pad_gain.assert_called_with(sample_id, PAD_GAIN_MAX)
    assert controller.project.pad_gain[sample_id] == PAD_GAIN_MAX


def test_set_pad_eq_clamps_and_calls_engine(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    sample_id = 0

    controller.transport.pad.set_pad_eq(sample_id, 1.0, -2.0, 3.0)
    audio_engine_mock.return_value.set_pad_eq.assert_called_with(sample_id, 1.0, -2.0, 3.0)
    assert controller.project.pad_eq_low_db[sample_id] == 1.0
    assert controller.project.pad_eq_mid_db[sample_id] == -2.0
    assert controller.project.pad_eq_high_db[sample_id] == 3.0

    controller.transport.pad.set_pad_eq(sample_id, 999.0, -999.0, 0.0)
    audio_engine_mock.return_value.set_pad_eq.assert_called_with(
        sample_id,
        PAD_EQ_DB_MAX,
        PAD_EQ_DB_MIN,
        0.0,
    )
    assert controller.project.pad_eq_low_db[sample_id] == PAD_EQ_DB_MAX
    assert controller.project.pad_eq_mid_db[sample_id] == PAD_EQ_DB_MIN


def test_poll_audio_messages_updates_session_peak(
    controller: AppController, audio_engine_mock: Mock, monkeypatch: pytest.MonkeyPatch
) -> None:
    now = 123.0
    monkeypatch.setattr("flitzis_looper.controller.metering.monotonic", lambda: now)

    msg1 = Mock()
    msg1.pad_peak.return_value = (0, 0.8)
    msg1.pad_playhead.return_value = (0, 0.1)
    msg2 = Mock()
    msg2.pad_peak.return_value = (0, 1.2)
    msg2.pad_playhead.return_value = (0, 0.2)
    msg3 = Mock()
    msg3.pad_peak.return_value = None
    msg3.pad_playhead.return_value = None

    audio_engine_mock.return_value.receive_msg.side_effect = [msg1, msg2, msg3, None]

    controller.metering.poll_audio_messages()

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
        beat_grid=BeatGrid(beats=[0.0, 0.5], downbeats=[0.0]),
    )
    assert controller.transport.pad.effective_key(sample_id) == "C#m"

    controller.transport.pad.set_manual_key(sample_id, "Gm")
    assert controller.transport.pad.effective_key(sample_id) == "Gm"
