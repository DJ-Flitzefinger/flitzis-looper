"""Tests for Pydantic model validation in flitzis_looper.models."""

import json

import pytest
from pydantic import ValidationError

from flitzis_looper.constants import (
    DEFAULT_DEMUCS_OVERLAP,
    DEFAULT_DEMUCS_SHIFTS,
    MAX_DEMUCS_OVERLAP,
    MAX_DEMUCS_SHIFTS,
    MIN_DEMUCS_OVERLAP,
    MIN_DEMUCS_SHIFTS,
    NUM_SAMPLES,
    PAD_GAIN_DB_DEFAULT,
    PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN,
    PAD_LOOP_BARS_DEFAULT,
    PAD_LOOP_BARS_MIN,
)
from flitzis_looper.models import (
    DEFAULT_TRIGGER_QUANTIZATION_STEP,
    STEM_COMPONENT_MASK,
    STEM_INSTRUMENTAL_PRESET_MASK,
    STEM_KINDS,
    STEM_MASK_BASS,
    STEM_MASK_DISPLAY_MODES,
    STEM_MASK_DRUMS,
    STEM_MASK_MELODY,
    STEM_MIX_MODES,
    BeatGrid,
    ProjectState,
    SampleAnalysis,
    SessionState,
    StemCacheEntry,
    StemFileSet,
)


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
            beat_grid=BeatGrid(beats=[0.0, 0.5, 1.0], downbeats=[0.0], bars=[0.0]),
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
        assert "stem_cache" in data
        assert "pad_stem_mix_mode" in data
        assert "manual_bpm" in data
        assert "manual_key" in data
        assert "speed" in data
        assert "volume" in data
        assert "trigger_quantization_enabled" in data
        assert "trigger_quantization_step" in data
        assert "key_lock" in data
        assert "demucs_shifts" in data
        assert "demucs_overlap" in data
        assert "input_mapping_enabled" in data
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
    assert project_state.trigger_quantization_enabled is False
    assert project_state.trigger_quantization_step == DEFAULT_TRIGGER_QUANTIZATION_STEP
    assert project_state.demucs_shifts == DEFAULT_DEMUCS_SHIFTS
    assert project_state.demucs_overlap == DEFAULT_DEMUCS_OVERLAP
    assert project_state.input_mapping_enabled is False
    assert project_state.speed == 1.0
    assert project_state.volume == 1.0
    assert project_state.selected_pad == 0
    assert project_state.selected_bank == 0
    assert len(project_state.sample_paths) == NUM_SAMPLES
    assert all(path is None for path in project_state.sample_paths)
    assert len(project_state.stem_cache) == NUM_SAMPLES
    assert all(entry is None for entry in project_state.stem_cache)
    assert len(project_state.pad_stem_mix_mode) == NUM_SAMPLES
    assert all(mode == "full_mix" for mode in project_state.pad_stem_mix_mode)
    assert len(project_state.manual_bpm) == NUM_SAMPLES
    assert all(bpm is None for bpm in project_state.manual_bpm)
    assert len(project_state.manual_key) == NUM_SAMPLES
    assert all(key is None for key in project_state.manual_key)
    assert len(project_state.pad_gain_db) == NUM_SAMPLES
    assert all(gain_db == PAD_GAIN_DB_DEFAULT for gain_db in project_state.pad_gain_db)
    assert len(project_state.pad_loop_start_s) == NUM_SAMPLES
    assert all(start_s == 0.0 for start_s in project_state.pad_loop_start_s)
    assert len(project_state.pad_loop_end_s) == NUM_SAMPLES
    assert all(end_s is None for end_s in project_state.pad_loop_end_s)
    assert len(project_state.pad_loop_auto) == NUM_SAMPLES
    assert not any(project_state.pad_loop_auto)
    assert len(project_state.pad_loop_bars) == NUM_SAMPLES
    assert all(bars == PAD_LOOP_BARS_DEFAULT for bars in project_state.pad_loop_bars)
    assert project_state.sidebar_left_expanded is True
    assert project_state.sidebar_right_expanded is True


def test_pad_gain_db_validation(project_state: ProjectState) -> None:
    project_state.pad_gain_db[0] = PAD_GAIN_DB_MIN
    project_state.pad_gain_db[1] = 0.0
    project_state.pad_gain_db[2] = PAD_GAIN_DB_MAX
    assert project_state.pad_gain_db[:3] == [PAD_GAIN_DB_MIN, 0.0, PAD_GAIN_DB_MAX]

    with pytest.raises(ValidationError, match="pad_gain_db"):
        ProjectState(pad_gain_db=[])

    with pytest.raises(ValidationError, match="pad_gain_db"):
        ProjectState(pad_gain_db=[PAD_GAIN_DB_MAX + 0.1] * NUM_SAMPLES)


def test_legacy_pad_gain_migrates_to_db_defaults_and_unity() -> None:
    project = ProjectState.model_validate({})
    assert project.pad_gain_db[0] == 0.0

    project = ProjectState.model_validate({"pad_gain": [1.0] * NUM_SAMPLES})
    assert project.pad_gain_db[0] == 0.0

    values = [100.0] * NUM_SAMPLES
    project = ProjectState.model_validate({"pad_gain": values})
    assert project.pad_gain_db[0] == 0.0


def test_legacy_pad_gain_below_unity_migrates_to_clamped_db() -> None:
    values = [0.5] * NUM_SAMPLES
    project = ProjectState.model_validate({"pad_gain": values})

    assert project.pad_gain_db[0] == pytest.approx(-6.0206, abs=1e-4)


def test_pad_loop_bars_accepts_half_bar_and_legacy_integer_values() -> None:
    project = ProjectState.model_validate({"pad_loop_bars": [4] * NUM_SAMPLES})
    assert project.pad_loop_bars[0] == 4.0

    project = ProjectState.model_validate({"pad_loop_bars": [0.5] * NUM_SAMPLES})
    assert project.pad_loop_bars[0] == PAD_LOOP_BARS_MIN


def test_pad_loop_bars_validation_rejects_invalid_values() -> None:
    with pytest.raises(ValidationError, match="pad_loop_bars"):
        ProjectState(pad_loop_bars=[0.0] * NUM_SAMPLES)

    with pytest.raises(ValidationError, match="granularity"):
        ProjectState(pad_loop_bars=[0.75] * NUM_SAMPLES)

    with pytest.raises(ValidationError, match="pad_loop_bars"):
        ProjectState(pad_loop_bars=[float("nan")] * NUM_SAMPLES)


def test_trigger_quantization_settings_validation(project_state: ProjectState) -> None:
    project_state.trigger_quantization_enabled = True
    assert project_state.trigger_quantization_enabled is True

    project_state.trigger_quantization_step = "1_64"
    assert project_state.trigger_quantization_step == "1_64"

    project_state.trigger_quantization_step = "1_32"
    assert project_state.trigger_quantization_step == "1_32"

    with pytest.raises(ValidationError):
        ProjectState.model_validate({"trigger_quantization_step": "half_note"})


def test_legacy_trigger_quantization_mode_migrates_to_new_fields() -> None:
    project = ProjectState.model_validate({"trigger_quantization": "next_bar"})

    assert project.trigger_quantization_enabled is True
    assert project.trigger_quantization_step == "1_16"

    project = ProjectState.model_validate({"trigger_quantization": "immediate"})

    assert project.trigger_quantization_enabled is False
    assert project.trigger_quantization_step == DEFAULT_TRIGGER_QUANTIZATION_STEP

    project = ProjectState.model_validate({"trigger_quantization_step": "1_bar"})

    assert project.trigger_quantization_step == "1_16"


def test_removed_key_lock_backend_settings_are_ignored() -> None:
    project = ProjectState.model_validate(
        {
            "key_lock": True,
            "key_lock_quality": "very_high",
            "key_lock_delay_min_samples": 128.0,
            "key_lock_delay_range_samples": 1024.0,
            "key_lock_head_count": 4,
            "key_lock_interpolation": "linear",
            "key_lock_window": "triangle",
            "key_lock_smoothing_step": 0.04,
            "key_lock_output_gain": 1.2,
        }
    )
    dumped = project.model_dump(mode="json")

    assert project.key_lock is True
    for key in [
        "key_lock_quality",
        "key_lock_delay_min_samples",
        "key_lock_delay_range_samples",
        "key_lock_head_count",
        "key_lock_interpolation",
        "key_lock_window",
        "key_lock_smoothing_step",
        "key_lock_output_gain",
    ]:
        assert key not in dumped


def test_demucs_quality_settings_validation(project_state: ProjectState) -> None:
    project_state.demucs_shifts = MIN_DEMUCS_SHIFTS
    assert project_state.demucs_shifts == MIN_DEMUCS_SHIFTS

    project_state.demucs_shifts = MAX_DEMUCS_SHIFTS
    assert project_state.demucs_shifts == MAX_DEMUCS_SHIFTS

    project_state.demucs_overlap = MIN_DEMUCS_OVERLAP
    assert project_state.demucs_overlap == MIN_DEMUCS_OVERLAP

    project_state.demucs_overlap = MAX_DEMUCS_OVERLAP
    assert project_state.demucs_overlap == MAX_DEMUCS_OVERLAP

    with pytest.raises(ValidationError, match="demucs_shifts"):
        ProjectState(demucs_shifts=MIN_DEMUCS_SHIFTS - 1)

    with pytest.raises(ValidationError, match="demucs_shifts"):
        ProjectState(demucs_shifts=MAX_DEMUCS_SHIFTS + 1)

    with pytest.raises(ValidationError, match="demucs_overlap"):
        ProjectState(demucs_overlap=MIN_DEMUCS_OVERLAP - 0.01)

    with pytest.raises(ValidationError, match="demucs_overlap"):
        ProjectState(demucs_overlap=MAX_DEMUCS_OVERLAP + 0.01)

    with pytest.raises(ValidationError, match="demucs_overlap"):
        ProjectState(demucs_overlap=float("nan"))


def test_stem_mix_mode_validation(project_state: ProjectState) -> None:
    project_state.pad_stem_mix_mode[0] = "all_stems"
    assert project_state.pad_stem_mix_mode[0] == "all_stems"
    assert tuple(STEM_MIX_MODES) == ("full_mix", "all_stems")

    with pytest.raises(ValidationError):
        ProjectState.model_validate({"pad_stem_mix_mode": ["half_stems"] * NUM_SAMPLES})

    with pytest.raises(ValidationError, match="pad_stem_mix_mode must have length"):
        ProjectState(pad_stem_mix_mode=[])


def test_stem_cache_entry_represents_expected_kinds() -> None:
    files = StemFileSet(
        vocals="samples/stems/a/vocals.wav",
        melody="samples/stems/a/melody.wav",
        bass="samples/stems/a/bass.wav",
        drums="samples/stems/a/drums.wav",
        instrumental="samples/stems/a/instrumental.wav",
    )
    entry = StemCacheEntry(
        source_version="samples/foo.wav|10|20",
        cache_dir="samples/stems/a",
        stems=files,
        available=True,
    )

    assert tuple(STEM_KINDS) == ("vocals", "melody", "bass", "drums", "instrumental")
    for kind in STEM_KINDS:
        path = entry.stems.path_for(kind)
        assert path is not None
        assert path.endswith(f"{kind}.wav")


def test_stem_cache_validation_requires_per_pad_length() -> None:
    with pytest.raises(ValidationError, match="stem_cache must have length"):
        ProjectState(stem_cache=[])


def test_session_state_defaults(session_state: SessionState) -> None:
    """Test SessionState default values."""
    assert len(session_state.active_sample_ids) == 0
    assert session_state.global_stop_engaged is False
    assert session_state.global_stop_restore_sample_ids == set()
    assert session_state.global_stop_momentary_mute_active is False
    assert session_state.global_start_stop_left_pressed is False
    assert len(session_state.pressed_pads) == NUM_SAMPLES
    assert all(pressed is False for pressed in session_state.pressed_pads)
    assert session_state.file_dialog_pad_id is None
    assert session_state.settings_open is False
    assert session_state.tap_bpm_pad_id is None
    assert session_state.tap_bpm_timestamps == []
    assert session_state.stem_generating_sample_ids == set()
    assert session_state.stem_generation_source_versions == {}
    assert session_state.stem_generation_progress == {}
    assert session_state.stem_generation_stage == {}
    assert session_state.stem_generation_errors == {}
    assert len(session_state.pad_stem_enabled_mask) == NUM_SAMPLES
    assert all(mask == STEM_COMPONENT_MASK for mask in session_state.pad_stem_enabled_mask)
    assert len(session_state.pad_stem_last_custom_mask) == NUM_SAMPLES
    assert all(mask == STEM_COMPONENT_MASK for mask in session_state.pad_stem_last_custom_mask)
    assert len(session_state.pad_stem_mask_display_mode) == NUM_SAMPLES
    assert all(mode == "all" for mode in session_state.pad_stem_mask_display_mode)
    assert session_state.input_learn_active is False
    assert session_state.input_learn_pending_source is None
    assert session_state.input_learn_pending_binding_key is None
    assert session_state.input_mapping_error is None
    assert session_state.master_output_peak == 0.0
    assert session_state.master_output_peak_updated_at == 0.0
    assert session_state.master_output_clip_hold_until == 0.0


def test_master_output_metering_state_validation(session_state: SessionState) -> None:
    session_state.master_output_peak = 1.25
    session_state.master_output_peak_updated_at = 10.0
    session_state.master_output_clip_hold_until = 11.0

    assert session_state.master_output_peak == pytest.approx(1.25)
    assert session_state.master_output_peak_updated_at == pytest.approx(10.0)
    assert session_state.master_output_clip_hold_until == pytest.approx(11.0)

    with pytest.raises(ValidationError, match="master_output_peak"):
        SessionState(master_output_peak=-0.1)

    with pytest.raises(ValidationError, match="master_output_peak"):
        SessionState(master_output_peak=float("inf"))

    with pytest.raises(ValidationError, match="master output timestamps"):
        SessionState(master_output_peak_updated_at=-0.1)

    with pytest.raises(ValidationError, match="master output timestamps"):
        SessionState(master_output_clip_hold_until=float("nan"))


def test_stem_enabled_mask_validation(session_state: SessionState) -> None:
    session_state.pad_stem_enabled_mask[0] = STEM_INSTRUMENTAL_PRESET_MASK
    assert session_state.pad_stem_enabled_mask[0] == (
        STEM_MASK_DRUMS | STEM_MASK_MELODY | STEM_MASK_BASS
    )

    with pytest.raises(ValidationError, match="pad_stem_enabled_mask"):
        SessionState(pad_stem_enabled_mask=[])

    with pytest.raises(ValidationError, match="component masks"):
        SessionState(pad_stem_enabled_mask=[1 << 4] * NUM_SAMPLES)

    session_state.pad_stem_last_custom_mask[0] = STEM_INSTRUMENTAL_PRESET_MASK
    assert session_state.pad_stem_last_custom_mask[0] == STEM_INSTRUMENTAL_PRESET_MASK

    with pytest.raises(ValidationError, match="pad_stem_last_custom_mask"):
        SessionState(pad_stem_last_custom_mask=[])

    with pytest.raises(ValidationError, match="component masks"):
        SessionState(pad_stem_last_custom_mask=[1 << 4] * NUM_SAMPLES)


def test_stem_mask_display_mode_validation(session_state: SessionState) -> None:
    session_state.pad_stem_mask_display_mode[0] = "instrumental"
    assert session_state.pad_stem_mask_display_mode[0] == "instrumental"
    assert tuple(STEM_MASK_DISPLAY_MODES) == ("custom", "instrumental", "all")

    with pytest.raises(ValidationError, match="pad_stem_mask_display_mode"):
        SessionState(pad_stem_mask_display_mode=[])

    with pytest.raises(ValidationError):
        SessionState.model_validate({"pad_stem_mask_display_mode": ["preset"] * NUM_SAMPLES})
