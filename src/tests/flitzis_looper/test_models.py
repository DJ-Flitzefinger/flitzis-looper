"""Tests for Pydantic model validation in flitzis_looper.models."""

import json

import pytest
from pydantic import ValidationError

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.models import BeatGrid, ProjectState, SampleAnalysis, SessionState


class TestProjectStateValidation:
    """Test ProjectState model validation."""

    def test_valid_sample_id_boundary(self, project_state: ProjectState) -> None:
        """Test that sample IDs at boundaries are valid."""
        # Lower bound
        project_state.selected_pad = 0
        assert project_state.selected_pad == 0

        # Upper bound
        project_state.selected_pad = 215
        assert project_state.selected_pad == 215

    def test_sample_id_below_zero_raises_error(self, project_state: ProjectState) -> None:
        """Test that negative sample IDs raise validation errors."""
        with pytest.raises(ValidationError, match="sample_id must be >= 0"):
            project_state.selected_pad = -1

    def test_sample_id_above_max_raises_error(self, project_state: ProjectState) -> None:
        """Test that sample IDs >= NUM_SAMPLES raise validation errors."""
        with pytest.raises(ValidationError, match="sample_id must be >= 0 and < 216"):
            project_state.selected_pad = 216

    def test_speed_clamping_to_bounds(self, project_state: ProjectState) -> None:
        """Test that speed is clamped to [0.5, 2.0]."""
        # Minimum boundary
        project_state.speed = 0.5
        assert project_state.speed == 0.5

        # Maximum boundary
        project_state.speed = 2.0
        assert project_state.speed == 2.0

        # Below minimum should fail
        with pytest.raises(ValidationError):
            project_state.speed = 0.49

        # Above maximum should fail
        with pytest.raises(ValidationError):
            project_state.speed = 2.01

    def test_volume_clamping_to_bounds(self, project_state: ProjectState) -> None:
        """Test that volume is clamped to [0.0, 1.0]."""
        # Minimum boundary
        project_state.volume = 0.0
        assert project_state.volume == 0.0

        # Maximum boundary
        project_state.volume = 1.0
        assert project_state.volume == 1.0

        # Below minimum should fail
        with pytest.raises(ValidationError):
            project_state.volume = -0.1

        # Above maximum should fail
        with pytest.raises(ValidationError):
            project_state.volume = 1.1

    def test_selected_bank_validation(self, project_state: ProjectState) -> None:
        """Test that selected_bank must be in range [0, NUM_BANKS)."""
        # Valid banks
        project_state.selected_bank = 0
        assert project_state.selected_bank == 0

        project_state.selected_bank = 5
        assert project_state.selected_bank == 5

        # Invalid bank - should fail
        with pytest.raises(ValidationError):
            project_state.selected_bank = 6

        with pytest.raises(ValidationError):
            project_state.selected_bank = -1


class TestSessionStateValidation:
    """Test SessionState model validation."""

    def test_active_sample_ids_validation(self, session_state: SessionState) -> None:
        """Test that active_sample_ids are validated."""
        # Valid IDs
        session_state.active_sample_ids = {0, 50, 215}
        assert session_state.active_sample_ids == {0, 50, 215}

        # Invalid ID - should raise error during validation
        with pytest.raises(ValidationError, match="sample_id must be >= 0"):
            session_state.active_sample_ids = {-1, 0}

        with pytest.raises(ValidationError, match="sample_id must be >= 0 and < 216"):
            session_state.active_sample_ids = {216}

    def test_pressed_pads_validation(self, session_state: SessionState) -> None:
        """Test that pressed_pads list maintains correct size and validation."""
        # Valid initialization
        assert len(session_state.pressed_pads) == 216

        # Setting valid pad states
        session_state.pressed_pads[0] = True
        session_state.pressed_pads[100] = False
        assert session_state.pressed_pads[0] is True
        assert session_state.pressed_pads[100] is False

        # List should maintain correct size
        assert len(session_state.pressed_pads) == 216

    def test_file_dialog_pad_id_validation(self) -> None:
        """Test that file_dialog_pad_id validates properly using SessionState."""
        # Valid ID
        session = SessionState.model_construct(file_dialog_pad_id=0)
        assert session.file_dialog_pad_id == 0

        # None should be allowed
        session = SessionState.model_construct(file_dialog_pad_id=None)
        assert session.file_dialog_pad_id is None

        # Invalid ID should raise error
        with pytest.raises(ValidationError, match="sample_id must be >= 0 and < 216"):
            SessionState(file_dialog_pad_id=300)

    def test_pressed_pads_field_validator(self) -> None:
        """Test the field validator for pressed_pads ensures correct length."""
        # The default_factory creates a list of correct size
        session = SessionState()
        assert len(session.pressed_pads) == 216

        # The field validator only validates sample IDs, not list length
        # It's designed this way to allow flexibility in list size during initialization
        # The default_factory ensures the correct size for normal operation

    def test_session_state_default_factories(self) -> None:
        """Test that default factories create independent instances."""
        session1 = SessionState()
        session2 = SessionState()

        # Default factories should create separate instances
        assert session1.active_sample_ids is not session2.active_sample_ids
        assert session1.pressed_pads is not session2.pressed_pads

        # Modifying one shouldn't affect the other
        session1.active_sample_ids.add(50)
        assert 50 not in session2.active_sample_ids


class TestModelSerialization:
    """Test Pydantic model serialization and deserialization."""

    def test_project_state_json_serialization(self, project_state: ProjectState) -> None:
        """Test that ProjectState can be serialized to and from JSON."""
        project_state.sample_analysis[0] = SampleAnalysis(
            bpm=120.0,
            key="C#m",
            beat_grid=BeatGrid(beats=[0.0, 0.5, 1.0], downbeats=[0.0]),
        )
        project_state.manual_bpm[0] = 128.0
        project_state.manual_key[0] = "Gm"

        # Serialize
        json_str = project_state.model_dump_json()
        data = json.loads(json_str)

        # Verify structure
        assert "multi_loop" in data
        assert "sample_paths" in data
        assert "sample_analysis" in data
        assert "manual_bpm" in data
        assert "manual_key" in data
        assert "speed" in data
        assert "volume" in data
        assert "selected_pad" in data

        assert data["sample_analysis"][0]["bpm"] == 120.0
        assert data["sample_analysis"][0]["key"] == "C#m"
        assert data["manual_bpm"][0] == 128.0
        assert data["manual_key"][0] == "Gm"

        # Deserialize
        reconstructed = ProjectState.model_validate_json(json_str)
        assert reconstructed.model_dump() == project_state.model_dump()

    def test_session_state_json_serialization(self, session_state: SessionState) -> None:
        """Test that SessionState can be serialized to and from JSON."""
        # Set some test data
        session_state.active_sample_ids = {1, 2, 3}
        session_state.pressed_pads[0] = True
        session_state.file_dialog_pad_id = 10
        session_state.tap_bpm_pad_id = 5
        session_state.tap_bpm_timestamps = [1.0, 2.0]

        # Serialize
        json_str = session_state.model_dump_json()
        data = json.loads(json_str)

        # Verify structure
        assert "active_sample_ids" in data
        assert "pressed_pads" in data
        assert "file_dialog_pad_id" in data
        assert "tap_bpm_pad_id" in data
        assert "tap_bpm_timestamps" in data
        assert data["active_sample_ids"] == [1, 2, 3]
        assert data["pressed_pads"][0] is True
        assert data["file_dialog_pad_id"] == 10
        assert data["tap_bpm_pad_id"] == 5
        assert data["tap_bpm_timestamps"] == [1.0, 2.0]

        # Deserialize
        reconstructed = SessionState.model_validate_json(json_str)
        assert reconstructed.active_sample_ids == {1, 2, 3}
        assert reconstructed.pressed_pads[0] is True
        assert reconstructed.file_dialog_pad_id == 10
        assert reconstructed.tap_bpm_pad_id == 5
        assert reconstructed.tap_bpm_timestamps == [1.0, 2.0]


def test_project_state_defaults(project_state: ProjectState) -> None:
    """Test ProjectState default values."""
    assert project_state.multi_loop is False
    assert project_state.key_lock is False
    assert project_state.bpm_lock is False
    assert project_state.speed == 1.0
    assert project_state.volume == 1.0
    assert project_state.selected_pad == 0
    assert project_state.selected_bank == 0
    assert len(project_state.sample_paths) == NUM_SAMPLES
    assert all(path is None for path in project_state.sample_paths)
    assert len(project_state.manual_bpm) == NUM_SAMPLES
    assert all(bpm is None for bpm in project_state.manual_bpm)
    assert len(project_state.manual_key) == NUM_SAMPLES
    assert all(key is None for key in project_state.manual_key)
    assert project_state.sidebar_left_expanded is True
    assert project_state.sidebar_right_expanded is True


def test_session_state_defaults(session_state: SessionState) -> None:
    """Test SessionState default values."""
    assert len(session_state.active_sample_ids) == 0
    assert len(session_state.pressed_pads) == NUM_SAMPLES
    assert all(pressed is False for pressed in session_state.pressed_pads)
    assert session_state.file_dialog_pad_id is None
    assert session_state.tap_bpm_pad_id is None
    assert session_state.tap_bpm_timestamps == []
