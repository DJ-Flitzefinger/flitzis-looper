# Tasks: Improve Controller Test Coverage

## Phase 1: Core Infrastructure Tests (Priority 1)

### 1.1 Create `test_base.py`
- [X] Add test_base_controller_initialization
- [X] Add test_output_sample_rate_hz_success
- [X] Add test_output_sample_rate_hz_none
- [X] Add test_output_sample_rate_hz_handles_runtime_error
- [X] Add test_output_sample_rate_hz_handles_type_error
- [X] Add test_output_sample_rate_hz_handles_value_error
- [X] Add test_mark_project_changed_with_callback
- [X] Add test_mark_project_changed_without_callback

### 1.2 Create `test_validation.py`
- [X] Add test_ensure_finite_valid_values
- [X] Add test_ensure_finite_nan_raises
- [X] Add test_ensure_finite_inf_raises
- [X] Add test_normalize_bpm_none
- [X] Add test_normalize_bpm_valid
- [X] Add test_normalize_bpm_non_finite
- [X] Add test_normalize_bpm_non_positive

### 1.3 Create `test_metering.py`
- [X] Add test_metering_controller_initialization
- [X] Add test_handle_pad_peak_message_clamps_peak
- [X] Add test_handle_pad_peak_message_ignores_non_finite
- [X] Add test_handle_pad_peak_message_ignores_invalid_sample_id
- [X] Add test_handle_pad_playhead_message_ignores_negative
- [X] Add test_handle_pad_playhead_message_ignores_non_finite
- [X] Add test_decay_pad_peaks_exponential_decay
- [X] Add test_decay_pad_peaks_clears_below_threshold
- [X] Add test_decay_pad_peaks_skips_zero_peaks
- [X] Add test_decay_pad_peaks_updates_timestamp
- [X] Add test_multiple_pad_peaks_decay_independently
- [X] Add test_poll_audio_messages_ignores_missing_attributes

## Phase 2: App Initialization and Lifecycle Tests (Priority 2)

### 2.1 Create `test_transport.py`
- [X] Add test_transport_controller_initialization
- [X] Add test_transport_controller_passes_references
- [X] Add test_apply_project_state_to_audio

### 2.2 Create `test_state.py`
- [X] Add test_apply_project_state_initialization
- [X] Add test_apply_global_audio_settings_only_when_changed
- [X] Add test_apply_global_audio_settings_calls_all_methods
- [X] Add test_apply_per_pad_mixing_only_when_changed
- [X] Add test_apply_per_pad_mixing_calls_gain_for_each_pad
- [X] Add test_apply_per_pad_mixing_calls_eq_for_each_pad
- [X] Add test_apply_pad_loop_regions_skips_unloaded_pads
- [X] Add test_apply_pad_loop_regions_only_when_changed
- [X] Add test_apply_pad_loop_regions_applies_effective_region
- [X] Add test_apply_pad_bpm_settings_only_when_available
- [X] Add test_apply_pad_bpm_settings_triggers_bpm_update
- [X] Add test_apply_bpm_lock_settings_enabled
- [X] Add test_apply_bpm_lock_settings_disabled
- [X] Add test_apply_bpm_lock_settings_triggers_recompute
- [X] Add test_apply_project_state_to_audio_calls_all_methods
- [X] Add test_apply_project_state_with_defaults
- [X] Add test_apply_project_state_with_modified_state

### 2.3 Create `test_app.py`
- [X] Add test_app_controller_initialization
- [X] Add test_app_controller_loads_project_from_persistence
- [X] Add test_app_controller_applies_project_state
- [X] Add test_app_controller_restores_samples
- [X] Add test_app_controller_shut_down_flushes_persistence
- [X] Add test_app_controller_shut_down_stops_audio
- [X] Add test_app_controller_shut_down_shuts_down_audio
- [X] Add test_app_controller_shut_down_suppresses_os_error
- [X] Add test_app_controller_project_property
- [X] Add test_app_controller_session_property
- [X] Add test_app_controller_persistence_property

## Phase 3: Complete Metering Controller Coverage (Priority 3)

### 3.1 Additions to `test_pad.py`
- [x] Add test_set_manual_key_empty_raises
- [x] Add test_set_pad_gain_non_finite_raises
- [x] Add test_set_pad_eq_non_finite_raises

## Phase 4: Fill Gaps in Existing Test Files (Priority 4)

### 4.1 Additions to `test_loader.py`
- [x] Add test_load_sample_async_already_loading
- [x] Add test_loader_started_event_handling
- [x] Add test_loader_progress_event_handling
- [x] Add test_loader_error_event_handling
- [x] Add test_analyze_sample_async_success
- [x] Add test_analyze_sample_async_already_loading
- [x] Add test_analyze_sample_async_runtime_error
- [x] Add test_invalid_analysis_data_ignored
- [x] Add test_analysis_validation_error_ignored
- [x] Add test_pending_sample_path
- [x] Add test_sample_load_error
- [x] Add test_sample_load_progress
- [x] Add test_sample_load_stage
- [x] Add test_unload_sample_windows_path
- [x] Add test_unload_sample_deletes_cached_file
- [x] Add test_unload_sample_outside_samples_dir
- [x] Add test_poll_loader_events_with_malformed_events

### 4.2 Additions to `test_persistence.py`
- [x] Add test_atomic_write_failure_cleanup
- [x] Add test_normalize_path_absolute
- [x] Add test_normalize_path_relative
- [x] Add test_normalize_path_outside_samples
- [x] Add test_flush_without_dirty
- [x] Add test_maybe_flush_not_dirty
- [x] Add test_complex_project_state
- [x] Add test_windows_paths_preserved
- [x] Add test_config_path_creation_os_error

### 4.3 Additions to `test_bpm.py`
- [x] Add test_set_manual_bpm_invalid_raises
- [x] Add test_set_manual_bpm_non_finite_raises
- [x] Add test_tap_bpm_less_than_three
- [x] Add test_tap_bpm_non_monotonic_timestamps
- [x] Add test_tap_bpm_negative_intervals
- [x] Add test_tap_bpm_very_slow_tempo
- [x] Add test_recompute_master_bpm_unlocked
- [x] Add test_recompute_master_bpm_none_anchor
- [x] Add test_recompute_master_bpm_non_finite_anchor
- [x] Add test_on_pad_bpm_changed_updates_audio
- [x] Add test_on_pad_bpm_changed_updates_master_bpm
- [x] Add test_on_pad_bpm_changed_not_anchor

### 4.4 Additions to `test_loop.py`
- [x] Add test_reset_no_analysis
- [x] Add test_reset_no_beats
- [x] Add test_reset_quantizes_to_samples
- [x] Add test_set_auto_enable_snaps_start
- [x] Add test_set_auto_disable_no_change
- [x] Add test_set_auto_no_op
- [x] Add test_set_bars_clamps_to_one
- [x] Add test_set_bars_no_op
- [x] Add test_set_start_negative_clamps
- [x] Add test_set_start_quantizes
- [x] Add test_set_end_none
- [x] Add test_set_end_clears_when_past_start
- [x] Add test_set_end_quantizes
- [x] Add test_set_end_non_finite_raises
- [x] Add test_effective_region_manual_mode
- [x] Add test_effective_region_auto_no_bpm
- [x] Add test_effective_region_auto_no_beats
- [x] Add test_snap_to_nearest_beat_empty
- [x] Add test_snap_to_nearest_beat_with_beats
- [x] Add test_quantize_time_none_sample_rate
- [x] Add test_quantize_time_invalid_sample_rate
- [x] Add test_apply_effective_region_not_loaded

### 4.5 Additions to `test_global_params.py`
- [x] Add test_set_key_lock_no_op
- [x] Add test_set_bpm_lock_no_op
- [x] Add test_set_bpm_lock_none_effective_bpm
- [x] Add test_set_bpm_lock_non_finite_effective_bpm
- [x] Add test_set_bpm_lock_disable_clears_anchor

### 4.6 Additions to `test_playback.py`
- [x] Add test_trigger_pad_applies_loop_region
- [x] Add test_trigger_pad_plays_with_gain
- [x] Add test_play_separate_method
- [x] Add test_play_applies_loop_region
- [x] Add test_toggle_stop_active
- [x] Add test_toggle_start_inactive
- [x] Add test_trigger_invalid_sample_id
- [x] Add test_stop_invalid_sample_id

## Phase 5: Validation

### 5.1 Test execution and coverage
- [x] Run all new tests with pytest
- [x] Verify all tests pass
- [x] Generate coverage report
- [x] Document coverage improvement

### 5.2 Quality checks
- [x] Run lint check on all new test files
- [x] Run type check on all new test files
- [x] Ensure all tests follow existing patterns
- [x] Review test fixture complexity

## Dependencies

- Phase 1 must complete before Phase 2 (tests BaseController/validation needed by other controllers)
- Phase 2 must complete before Phase 3 (app lifecycle tests needed)
- Phases 3 and 4 can run in parallel
- Phase 5 must run after all phases complete

## Estimated effort

- Phase 1: ~6-8 hours (29 tests)
- Phase 2: ~8-10 hours (38 tests)
- Phase 3: ~1-2 hours (3 tests)
- Phase 4: ~16-20 hours (67 tests)
- Phase 5: ~2-3 hours (validation)
- **Total: ~33-43 hours**
