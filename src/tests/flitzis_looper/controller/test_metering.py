from typing import TYPE_CHECKING
from unittest.mock import Mock, patch

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.metering import MeteringController

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState


@pytest.fixture
def metering_controller(
    project_state: ProjectState, session_state: SessionState, audio_engine_mock: Mock
) -> MeteringController:
    return MeteringController(project_state, session_state, audio_engine_mock)


def test_metering_controller_initialization(
    metering_controller: MeteringController,
    project_state: ProjectState,
    session_state: SessionState,
    audio_engine_mock: Mock,
) -> None:
    """Test MeteringController initialization adds decay to frame render callbacks."""
    assert metering_controller._project is project_state
    assert metering_controller._session is session_state
    assert metering_controller._audio is audio_engine_mock
    assert len(metering_controller._on_frame_render_callbacks) == 1
    assert metering_controller._decay_pad_peaks in metering_controller._on_frame_render_callbacks


def test_handle_pad_peak_message_clamps_peak(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test handle_pad_peak_message clamps peak between 0.0 and 1.0."""
    msg = Mock()
    msg.sample_id.return_value = 0
    msg.pad_peak.return_value = 1.5

    metering_controller.handle_pad_peak_message(msg)

    assert session_state.pad_peak[0] == 1.0
    assert session_state.pad_peak[0] > 0


def test_handle_pad_peak_message_ignores_non_finite(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test handle_pad_peak_message ignores non-finite values."""
    session_state.pad_peak[0] = 0.5

    msg_nan = Mock()
    msg_nan.sample_id.return_value = 0
    msg_nan.pad_peak.return_value = float("nan")

    metering_controller.handle_pad_peak_message(msg_nan)

    assert session_state.pad_peak[0] == 0.5

    msg_inf = Mock()
    msg_inf.sample_id.return_value = 1
    msg_inf.pad_peak.return_value = float("inf")

    metering_controller.handle_pad_peak_message(msg_inf)

    assert session_state.pad_peak[1] == 0.0


def test_handle_pad_peak_message_ignores_invalid_sample_id(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test handle_pad_peak_message ignores invalid sample IDs."""
    session_state.pad_peak[0] = 0.5

    msg_none = Mock()
    msg_none.sample_id.return_value = None
    msg_none.pad_peak.return_value = 0.8

    metering_controller.handle_pad_peak_message(msg_none)

    assert session_state.pad_peak[0] == 0.5

    msg_invalid = Mock()
    msg_invalid.sample_id.return_value = NUM_SAMPLES
    msg_invalid.pad_peak.return_value = 0.8

    metering_controller.handle_pad_peak_message(msg_invalid)

    assert session_state.pad_peak[0] == 0.5


def test_handle_pad_playhead_message_ignores_negative(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test handle_pad_playhead_message ignores negative positions."""
    session_state.pad_playhead_s[0] = 1.0

    msg = Mock()
    msg.sample_id.return_value = 0
    msg.pad_playhead.return_value = -0.5

    metering_controller.handle_pad_playhead_message(msg)

    assert session_state.pad_playhead_s[0] == 1.0


def test_handle_pad_playhead_message_ignores_non_finite(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test handle_pad_playhead_message ignores non-finite values."""
    session_state.pad_playhead_s[0] = 1.0
    session_state.pad_playhead_s[1] = 0.5

    msg_nan = Mock()
    msg_nan.sample_id.return_value = 0
    msg_nan.pad_playhead.return_value = float("nan")

    metering_controller.handle_pad_playhead_message(msg_nan)

    assert session_state.pad_playhead_s[0] == 1.0

    msg_inf = Mock()
    msg_inf.sample_id.return_value = 1
    msg_inf.pad_playhead.return_value = float("inf")

    metering_controller.handle_pad_playhead_message(msg_inf)

    assert session_state.pad_playhead_s[1] == 0.5


def test_decay_pad_peaks_exponential_decay(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test _decay_pad_peaks applies exponential decay."""
    session_state.pad_peak[0] = 1.0
    session_state.pad_peak_updated_at[0] = 0.0

    with patch("flitzis_looper.controller.metering.monotonic", return_value=0.1):
        metering_controller._decay_pad_peaks()

    assert session_state.pad_peak[0] == 1.0
    assert session_state.pad_peak_updated_at[0] == 0.1

    with patch("flitzis_looper.controller.metering.monotonic", return_value=0.35):
        metering_controller._decay_pad_peaks()

    decayed = session_state.pad_peak[0]
    expected_decay = 0.5 ** (0.25 / 0.25)

    assert decayed == pytest.approx(expected_decay)
    assert decayed < 1.0


def test_decay_pad_peaks_clears_below_threshold(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test _decay_pad_peaks clears peaks below threshold."""
    session_state.pad_peak[0] = 0.00005
    session_state.pad_peak_updated_at[0] = 1.0

    with patch("flitzis_looper.controller.metering.monotonic", return_value=2.5):
        metering_controller._decay_pad_peaks()

    assert session_state.pad_peak[0] == 0.0


def test_decay_pad_peaks_skips_zero_peaks(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test _decay_pad_peaks skips peaks that are already zero."""
    session_state.pad_peak[0] = 0.0
    session_state.pad_peak_updated_at[0] = 0.0

    metering_controller._decay_pad_peaks()

    assert session_state.pad_peak[0] == 0.0


def test_decay_pad_peaks_updates_timestamp(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test _decay_pad_peaks updates timestamp for decayed peaks."""
    session_state.pad_peak[0] = 0.8
    session_state.pad_peak_updated_at[0] = 0.0

    with patch("flitzis_looper.controller.metering.monotonic", return_value=0.1):
        metering_controller._decay_pad_peaks()

    assert session_state.pad_peak_updated_at[0] > 0.0


def test_multiple_pad_peaks_decay_independently(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test multiple pad peaks decay independently."""
    session_state.pad_peak[0] = 0.8
    session_state.pad_peak_updated_at[0] = 0.0
    session_state.pad_peak[1] = 0.8
    session_state.pad_peak_updated_at[1] = 0.0

    with patch("flitzis_looper.controller.metering.monotonic", return_value=0.5):
        metering_controller._decay_pad_peaks()

    assert session_state.pad_peak[0] == session_state.pad_peak[1]
    assert session_state.pad_peak[0] == 0.8

    with patch("flitzis_looper.controller.metering.monotonic", return_value=0.75):
        metering_controller._decay_pad_peaks()

    assert session_state.pad_peak[0] == session_state.pad_peak[1]
    assert session_state.pad_peak[0] < 0.8


def test_poll_audio_messages_ignores_missing_attributes(
    metering_controller: MeteringController, session_state: SessionState
) -> None:
    """Test message handling ignores messages with missing attributes."""
    msg_no_sample_id = Mock()
    msg_no_sample_id.sample_id.return_value = None
    msg_no_sample_id.pad_peak.return_value = 0.5

    metering_controller.handle_pad_peak_message(msg_no_sample_id)

    assert session_state.pad_peak[0] == 0.0

    msg_no_peak = Mock()
    msg_no_peak.sample_id.return_value = 0
    msg_no_peak.pad_peak.return_value = None

    metering_controller.handle_pad_peak_message(msg_no_peak)

    assert session_state.pad_peak[0] == 0.0
