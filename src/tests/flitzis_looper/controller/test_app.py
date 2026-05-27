from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import patch

import pytest

import flitzis_looper.controller.app as app_module
from flitzis_looper.constants import NUM_SAMPLES

if TYPE_CHECKING:
    from unittest.mock import Mock

    from flitzis_looper.controller import AppController


class _PadPeakMessage:
    def __init__(self, sample_id: int, peak: float) -> None:
        self._sample_id = sample_id
        self._peak = peak

    def sample_id(self) -> int:
        return self._sample_id

    def pad_peak(self) -> float:
        return self._peak


class _MasterPeakMessage:
    def __init__(self, peak: float) -> None:
        self._peak = peak

    def master_peak(self) -> float:
        return self._peak


class _PadPlayheadMessage:
    def __init__(self, sample_id: int, playhead_s: float) -> None:
        self._sample_id = sample_id
        self._playhead_s = playhead_s

    def sample_id(self) -> int:
        return self._sample_id

    def pad_playhead(self) -> float:
        return self._playhead_s


class _SampleStartedMessage:
    def __init__(self, sample_id: int) -> None:
        self._sample_id = sample_id

    def sample_id(self) -> int:
        return self._sample_id


class _SampleStoppedMessage:
    def __init__(self, sample_id: int) -> None:
        self._sample_id = sample_id

    def sample_id(self) -> int:
        return self._sample_id


def test_controller_initializes_states(controller: AppController) -> None:
    """Test AppController creates all components during initialization."""
    assert controller.project is not None
    assert controller.session is not None
    assert controller.transport is not None
    assert controller.loader is not None
    assert controller.metering is not None
    assert controller.stems is not None
    assert controller.input_mapping is not None
    assert len(controller.project.sample_paths) == NUM_SAMPLES


def test_controller_creates_audio_engine(
    controller: AppController, audio_engine_mock: Mock
) -> None:
    """Test AppController creates and runs audio engine during initialization."""
    assert controller._audio == audio_engine_mock
    audio_engine_mock.run.assert_called_once()


def test_controller_applies_project_state(controller: AppController) -> None:
    """Test AppController applies project state to audio during initialization."""
    assert controller.transport.apply_project_state_to_audio is not None


def test_controller_restores_samples(controller: AppController) -> None:
    """Test AppController restores samples from project during initialization."""
    assert controller.loader.restore_samples_from_project_state is not None


def test_controller_shut_down_flushes_persistence(controller: AppController) -> None:
    """Test AppController flushes persistence on shutdown."""
    with patch.object(controller._persistence, "flush") as mock_flush:
        controller.shut_down()
        mock_flush.assert_called_once()


def test_controller_shut_down_stops_audio(controller: AppController) -> None:
    """Test AppController stops all audio on shutdown."""
    assert hasattr(controller._audio, "stop_all")
    controller.shut_down()


def test_controller_shut_down_shuts_down_audio(controller: AppController) -> None:
    """Test AppController shuts down audio engine on shutdown."""
    assert hasattr(controller._audio, "shut_down")
    controller.shut_down()


def test_controller_shut_down_suppresses_os_error(controller: AppController) -> None:
    """Test AppController suppresses OSError during persistence flush on shutdown."""
    with patch.object(controller._persistence, "flush", side_effect=OSError("File not found")):
        controller.shut_down()


def test_controller_project_property(controller: AppController) -> None:
    """Test AppController.project property returns project state."""
    assert controller.project is controller._project


def test_controller_session_property(controller: AppController) -> None:
    """Test AppController.session property returns session state."""
    assert controller.session is controller._session


def test_controller_persistence_property(controller: AppController) -> None:
    """Test AppController.persistence property returns persistence instance."""
    assert controller.persistence is controller._persistence


def test_controller_on_frame_render_calls_all_controllers(
    controller: AppController,
) -> None:
    """Test AppController.on_frame_render calls on_frame_render on all controllers."""
    with (
        patch.object(controller.transport, "on_frame_render") as mock_transport,
        patch.object(controller.loader, "on_frame_render") as mock_loader,
        patch.object(controller.metering, "on_frame_render") as mock_metering,
        patch.object(controller.stems, "on_frame_render") as mock_stems,
        patch.object(controller.input_mapping, "on_frame_render") as mock_input_mapping,
    ):
        controller.on_frame_render()

        mock_transport.assert_called_once()
        mock_loader.assert_called_once()
        mock_metering.assert_called_once()
        mock_stems.assert_called_once()
        mock_input_mapping.assert_called_once()


def test_controller_registers_controllers(controller: AppController) -> None:
    """Test AppController registers all controllers for on_frame_render."""
    assert len(controller._controllers) == 5


def test_controller_poll_runtime_events_dispatches_audio_messages(
    controller: AppController,
    audio_engine_mock: Mock,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Test controller-owned runtime polling updates session audio projections."""
    audio_messages = SimpleNamespace(
        PadPeak=_PadPeakMessage,
        MasterPeak=_MasterPeakMessage,
        PadPlayhead=_PadPlayheadMessage,
        SampleStarted=_SampleStartedMessage,
        SampleStopped=_SampleStoppedMessage,
    )
    monkeypatch.setattr(app_module, "AudioMessage", audio_messages)
    monkeypatch.setattr("flitzis_looper.controller.metering.monotonic", lambda: 123.0)

    audio_engine_mock.poll_loader_events.return_value = None
    audio_engine_mock.receive_msg.side_effect = [
        _PadPeakMessage(0, 0.75),
        _MasterPeakMessage(1.25),
        _PadPlayheadMessage(0, 1.25),
        _SampleStartedMessage(0),
        None,
    ]

    controller.poll_runtime_events()

    assert controller.session.pad_peak[0] == pytest.approx(0.75)
    assert controller.session.master_output_peak == pytest.approx(1.25)
    assert controller.session.master_output_clip_hold_until == pytest.approx(124.0)
    assert controller.session.pad_playhead_s[0] == pytest.approx(1.25)
    assert controller.session.active_sample_ids == {0}
    audio_engine_mock.poll_loader_events.assert_called_once()

    audio_engine_mock.receive_msg.side_effect = [_SampleStoppedMessage(0), None]

    controller.poll_runtime_events()

    assert controller.session.active_sample_ids == set()
    assert controller.session.paused_sample_ids == set()
