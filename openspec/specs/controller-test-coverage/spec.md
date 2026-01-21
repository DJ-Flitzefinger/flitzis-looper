# controller-test-coverage Specification

## Purpose
TBD - created by archiving change improve-controller-test-coverage. Update Purpose after archive.
## Requirements
### Requirement: BaseController Test Coverage
The test suite SHALL include unit tests for BaseController covering initialization, audio sample rate retrieval, and project change marking behavior.

The BaseController tests SHALL verify:
- Constructor parameter storage
- `_output_sample_rate_hz` behavior with available/unavailable audio engine
- `_output_sample_rate_hz` error handling (RuntimeError, TypeError, ValueError)
- `_mark_project_changed` callback invocation

#### Scenario: BaseController stores constructor parameters
- **GIVEN** a BaseController is created with project, session, audio, and callback parameters
- **WHEN** the controller is initialized
- **THEN** the controller stores all constructor parameters for later use

#### Scenario: _output_sample_rate_hz returns int when available
- **GIVEN** a BaseController with an available audio engine
- **WHEN** `_output_sample_rate_hz` is called
- **THEN** it returns an integer sample rate value

#### Scenario: _output_sample_rate_hz returns None when not available
- **GIVEN** a BaseController without an available audio engine
- **WHEN** `_output_sample_rate_hz` is called
- **THEN** it returns None

#### Scenario: _output_sample_rate_hz handles RuntimeError gracefully
- **GIVEN** a BaseController with an audio engine that raises RuntimeError
- **WHEN** `_output_sample_rate_hz` is called
- **THEN** it returns None without propagating the exception

#### Scenario: _output_sample_rate_hz handles TypeError gracefully
- **GIVEN** a BaseController with an audio engine that raises TypeError
- **WHEN** `_output_sample_rate_hz` is called
- **THEN** it returns None without propagating the exception

#### Scenario: _output_sample_rate_hz handles ValueError gracefully
- **GIVEN** a BaseController with an audio engine that raises ValueError
- **WHEN** `_output_sample_rate_hz` is called
- **THEN** it returns None without propagating the exception

#### Scenario: _mark_project_changed calls callback when set
- **GIVEN** a BaseController with a callback function configured
- **WHEN** `_mark_project_changed` is called
- **THEN** the configured callback is invoked

#### Scenario: _mark_project_changed does nothing when callback is None
- **GIVEN** a BaseController with callback=None
- **WHEN** `_mark_project_changed` is called
- **THEN** no error occurs and no callback is invoked

---

### Requirement: Validation Utilities Test Coverage
The test suite SHALL include unit tests for validation functions in validation.py covering finite value validation and BPM normalization.

The validation tests SHALL verify:
- `ensure_finite` accepts finite values
- `ensure_finite` raises ValueError for NaN and infinity
- `normalize_bpm` handles None, finite positive, non-finite, and non-positive values

#### Scenario: ensure_finite accepts finite values
- **GIVEN** ensure_finite is called with a finite numeric value
- **WHEN** validation runs
- **THEN** the value passes through without error

#### Scenario: ensure_finite raises ValueError for NaN
- **GIVEN** ensure_finite is called with NaN (float('nan'))
- **WHEN** validation runs
- **THEN** ValueError is raised

#### Scenario: ensure_finite raises ValueError for infinity
- **GIVEN** ensure_finite is called with infinity (float('inf') or float('-inf'))
- **WHEN** validation runs
- **THEN** ValueError is raised

#### Scenario: normalize_bpm returns None for None input
- **GIVEN** normalize_bpm is called with None
- **WHEN** normalization runs
- **THEN** None is returned

#### Scenario: normalize_bpm returns float for valid BPM
- **GIVEN** normalize_bpm is called with a valid positive BPM (e.g., 120.0)
- **WHEN** normalization runs
- **THEN** a float representation of the BPM is returned

#### Scenario: normalize_bpm returns None for non-finite values
- **GIVEN** normalize_bpm is called with NaN or infinity
- **WHEN** normalization runs
- **THEN** None is returned

#### Scenario: normalize_bpm returns None for non-positive BPM
- **GIVEN** normalize_bpm is called with BPM ≤ 0 (e.g., 0, -10)
- **WHEN** normalization runs
- **THEN** None is returned

---

### Requirement: MeteringController Test Coverage
The test suite SHALL include comprehensive unit tests for MeteringController covering peak/playhead message handling and exponential decay behavior.

The MeteringController tests SHALL verify:
- Initialization behavior
- Peak message clamping and validation
- Playhead message validation
- Exponential decay with half-life 0.25s
- Peak clearing below threshold
- Message polling with missing attributes

#### Scenario: MeteringController stores session and audio
- **GIVEN** a MeteringController is created with session and audio parameters
- **WHEN** initialization completes
- **THEN** session and audio references are stored

#### Scenario: _handle_pad_peak_message clamps peak to [0.0, 1.0]
- **GIVEN** a peak message with value > 1.0 (e.g., 1.5)
- **WHEN** `_handle_pad_peak_message` processes it
- **THEN** the peak is clamped to 1.0

#### Scenario: _handle_pad_peak_message ignores non-finite peaks
- **GIVEN** a peak message with NaN or infinity value
- **WHEN** `_handle_pad_peak_message` processes it
- **THEN** the message is ignored and state is not updated

#### Scenario: _handle_pad_peak_message ignores invalid sample_id
- **GIVEN** a peak message with non-numeric or out-of-range sample_id
- **WHEN** `_handle_pad_peak_message` processes it
- **THEN** the message is ignored

#### Scenario: _handle_pad_playhead_message ignores negative positions
- **GIVEN** a playhead message with negative position value
- **WHEN** `_handle_pad_playhead_message` processes it
- **THEN** the message is ignored

#### Scenario: _handle_pad_playhead_message ignores non-finite positions
- **GIVEN** a playhead message with NaN or infinity position
- **WHEN** `_handle_pad_playhead_message` processes it
- **THEN** the message is ignored

#### Scenario: _decay_pad_peaks uses exponential decay
- **GIVEN** a pad with a peak value (e.g., 0.8) and current timestamp
- **WHEN** 0.25 seconds pass and decay is applied
- **THEN** the peak decays by half (to 0.4)

#### Scenario: _decay_pad_peaks clears peaks below threshold
- **GIVEN** a pad with a peak value below 1e-4
- **WHEN** decay occurs
- **THEN** the peak is cleared (set to None or 0)

#### Scenario: _decay_pad_peaks skips zero peaks
- **GIVEN** a pad with zero peak value
- **WHEN** decay occurs
- **THEN** the timestamp is not updated

#### Scenario: _decay_pad_peaks updates timestamp for all pads
- **GIVEN** multiple pads with peak values
- **WHEN** decay occurs
- **THEN** all pad timestamps are updated

#### Scenario: multiple pads' peaks decay independently
- **GIVEN** two pads with different peak values (e.g., 0.8 and 0.2)
- **WHEN** decay occurs
- **THEN** each peak decays independently based on its own value

#### Scenario: poll_audio_messages ignores messages without attributes
- **GIVEN** an audio message missing pad_peak or playhead attributes
- **WHEN** poll_audio_messages processes it
- **THEN** the message is ignored

---

### Requirement: TransportController Test Coverage
The test suite SHALL include unit tests for TransportController verifying initialization and delegation behavior.

The TransportController tests SHALL verify:
- Sub-controller creation (BpmController, PadController, etc.)
- Reference passing to sub-controllers
- `apply_project_state_to_audio` delegation to ApplyProjectState

#### Scenario: TransportController creates all sub-controllers
- **GIVEN** a TransportController is initialized with required dependencies
- **WHEN** initialization completes
- **THEN** all sub-controllers (BpmController, PadController, PadLoopController, etc.) are created

#### Scenario: TransportController passes references to sub-controllers
- **GIVEN** a TransportController is initialized
- **WHEN** initialization completes
- **THEN** references (project, session, audio) are passed to all sub-controllers

#### Scenario: apply_project_state_to_audio delegates to ApplyProjectState
- **GIVEN** apply_project_state_to_audio is called on TransportController
- **WHEN** delegation occurs
- **THEN** ApplyProjectState.apply_project_state_to_audio is invoked with the same parameters

---

### Requirement: ApplyProjectState Test Coverage
The test suite SHALL include comprehensive unit tests for ApplyProjectState covering all apply methods with default and modified project states.

The ApplyProjectState tests SHALL verify:
- Initialization behavior
- Apply methods only call when values differ from default
- Apply methods call correct audio engine methods
- Apply methods handle edge cases (no samples, None BPM, etc.)
- Full project state application

#### Scenario: ApplyProjectState stores transport reference
- **GIVEN** ApplyProjectState is created with a transport reference
- **WHEN** initialization occurs
- **THEN** the transport reference is stored for later use

#### Scenario: _apply_global_audio_settings only calls when changed
- **GIVEN** project state with default global settings (volume=1.0, speed=1.0, key_lock=False, bpm_lock=False)
- **WHEN** `_apply_global_audio_settings` runs
- **THEN** no audio engine methods are called

#### Scenario: _apply_global_audio_settings calls all methods
- **GIVEN** project state with modified global settings
- **WHEN** `_apply_global_audio_settings` runs
- **THEN** set_volume, set_speed, set_key_lock, set_bpm_lock are called on the audio engine

#### Scenario: _apply_per_pad_mixing only calls when changed
- **GIVEN** project state with default pad mixing (gain=1.0, EQ=(0,0,0))
- **WHEN** `_apply_per_pad_mixing` runs
- **THEN** no pad mixing methods are called

#### Scenario: _apply_per_pad_mixing calls set_pad_gain for each pad
- **GIVEN** project state with modified pad gains
- **WHEN** `_apply_per_pad_mixing` runs
- **THEN** set_pad_gain is called for each pad with modified gain

#### Scenario: _apply_per_pad_mixing calls set_pad_eq for each pad
- **GIVEN** project state with modified pad EQ values
- **WHEN** `_apply_per_pad_mixing` runs
- **THEN** set_pad_eq is called for each pad with modified EQ

#### Scenario: _apply_pad_loop_regions skips unloaded pads
- **GIVEN** a pad without a loaded sample
- **WHEN** `_apply_pad_loop_regions` runs
- **THEN** the pad is skipped

#### Scenario: _apply_pad_loop_regions only calls when changed
- **GIVEN** project state with default loop regions
- **WHEN** `_apply_pad_loop_regions` runs
- **THEN** no loop region methods are called

#### Scenario: _apply_pad_loop_regions applies effective region
- **GIVEN** a pad with modified loop region
- **WHEN** `_apply_pad_loop_regions` runs
- **THEN** the effective loop region is applied to the audio engine

#### Scenario: _apply_pad_bpm_settings only calls when BPM available
- **GIVEN** a pad without BPM data (None or missing)
- **WHEN** `_apply_pad_bpm_settings` runs
- **THEN** the pad is skipped

#### Scenario: _apply_pad_bpm_settings triggers BPM update
- **GIVEN** a pad with BPM data
- **WHEN** `_apply_pad_bpm_settings` runs
- **THEN** BPM update is triggered for the pad

#### Scenario: _apply_bpm_lock_settings sets anchor when enabled
- **GIVEN** bpm_lock=True with effective BPM available
- **WHEN** `_apply_bpm_lock_settings` runs
- **THEN** anchor pad and BPM are set

#### Scenario: _apply_bpm_lock_settings clears anchor when disabled
- **GIVEN** bpm_lock=False
- **WHEN** `_apply_bpm_lock_settings` runs
- **THEN** anchor pad and BPM are cleared

#### Scenario: _apply_bpm_lock_settings triggers recomputation
- **GIVEN** bpm_lock settings change
- **WHEN** `_apply_bpm_lock_settings` runs
- **THEN** master BPM recomputation is triggered

#### Scenario: apply_project_state_to_audio calls all methods
- **GIVEN** a project state
- **WHEN** apply_project_state_to_audio runs
- **THEN** all apply methods are called in order

#### Scenario: apply_project_state_with_defaults works
- **GIVEN** a default project state
- **WHEN** apply_project_state_to_audio runs
- **THEN** all methods complete without errors

#### Scenario: apply_project_state_with_modified_state works
- **GIVEN** a modified project state
- **WHEN** apply_project_state_to_audio runs
- **THEN** all methods complete without errors

---

### Requirement: AppController Test Coverage
The test suite SHALL include comprehensive unit tests for AppController covering initialization, persistence, and shutdown lifecycle.

The AppController tests SHALL verify:
- Initialization creates all components
- Project loading from persistence
- Project state application to audio engine
- Sample restoration from project state
- Shutdown behavior (flush, stop audio, error handling)
- Property accessors

#### Scenario: AppController initializes all components correctly
- **GIVEN** an AppController is created with required dependencies
- **WHEN** initialization completes
- **THEN** all sub-controllers and persistence are created

#### Scenario: AppController loads project from persistence
- **GIVEN** an AppController is initialized
- **WHEN** initialization completes
- **THEN** project is loaded from persistence

#### Scenario: AppController applies project state to audio
- **GIVEN** an AppController is initialized
- **WHEN** initialization completes
- **THEN** project state is applied to the audio engine

#### Scenario: AppController restores samples from project state
- **GIVEN** a project with loaded samples in persistence
- **WHEN** initialization completes
- **THEN** samples are restored

#### Scenario: AppController shutdown flushes persistence
- **GIVEN** an AppController with dirty state
- **WHEN** shut_down is called
- **THEN** persistence is flushed

#### Scenario: AppController shutdown stops all audio
- **GIVEN** an AppController with active samples
- **WHEN** shut_down is called
- **THEN** all audio is stopped

#### Scenario: AppController shutdown shuts down audio engine
- **GIVEN** an AppController is initialized
- **WHEN** shut_down is called
- **THEN** audio engine is shut down

#### Scenario: AppController shutdown suppresses OSError
- **GIVEN** OSError occurs during shutdown (e.g., during flush)
- **WHEN** shut_down is called
- **THEN** the error is suppressed

#### Scenario: AppController project property returns project
- **GIVEN** an AppController is initialized
- **WHEN** the project property is accessed
- **THEN** the project instance is returned

#### Scenario: AppController session property returns session
- **GIVEN** an AppController is initialized
- **WHEN** the session property is accessed
- **THEN** the session instance is returned

#### Scenario: AppController persistence property returns persistence
- **GIVEN** an AppController is initialized
- **WHEN** the persistence property is accessed
- **THEN** the persistence instance is returned

---

### Requirement: LoaderController Event Handling Test Coverage
The test suite SHALL include unit tests for LoaderController covering all event types and analysis methods.

The LoaderController tests SHALL verify:
- Event handling (started, progress, error)
- Analysis method behavior
- Sample path and error handling
- Windows path preservation

#### Scenario: load_sample_async unloads when already loading
- **GIVEN** a sample is already loading in slot 0
- **WHEN** load_sample_async is called for the same slot
- **THEN** the loading sample is unloaded first

#### Scenario: loader_started event handling
- **GIVEN** a loader_started event is received
- **WHEN** the event is processed
- **THEN** state is cleared and loading flag is set

#### Scenario: loader_progress event handling
- **GIVEN** a loader_progress event with stage and percent
- **WHEN** the event is processed
- **THEN** stage and percent are updated in state

#### Scenario: loader_error event handling
- **GIVEN** a loader_error event with error message
- **WHEN** the event is processed
- **THEN** state is cleared and error is recorded

#### Scenario: analyze_sample_async schedules analysis
- **GIVEN** a loaded sample
- **WHEN** analyze_sample_async is called
- **THEN** analysis is scheduled

#### Scenario: analyze_sample_async returns when already loading
- **GIVEN** a sample is already being analyzed
- **WHEN** analyze_sample_async is called
- **THEN** the method returns immediately

#### Scenario: analyze_sample_async handles RuntimeError
- **GIVEN** RuntimeError occurs during analysis
- **WHEN** analyze_sample_async is called
- **THEN** the error is handled gracefully

#### Scenario: _store_sample_analysis ignores non-dict
- **GIVEN** non-dict analysis data
- **WHEN** _store_sample_analysis is called
- **THEN** the data is ignored

#### Scenario: _store_sample_analysis ignores ValidationError
- **GIVEN** ValidationError occurs during validation
- **WHEN** _store_sample_analysis is called
- **THEN** the error is ignored

#### Scenario: pending_sample_path returns path
- **GIVEN** a loading sample with path
- **WHEN** pending_sample_path is called
- **THEN** the sample path is returned

#### Scenario: sample_load_error returns error message
- **GIVEN** a loading error occurred
- **WHEN** sample_load_error is called
- **THEN** the error message is returned

#### Scenario: sample_load_progress returns progress
- **GIVEN** a loading sample with progress value
- **WHEN** sample_load_progress is called
- **THEN** the progress value is returned

#### Scenario: sample_load_stage returns stage
- **GIVEN** a loading sample with stage name
- **WHEN** sample_load_stage is called
- **THEN** the stage name is returned

#### Scenario: unload_sample preserves Windows paths
- **GIVEN** a sample with Windows-style path (backslashes)
- **WHEN** unload_sample is called
- **THEN** backslashes are preserved

#### Scenario: unload_sample deletes cached file
- **GIVEN** a cached sample file in samples directory
- **WHEN** unload_sample is called
- **THEN** the file is deleted

#### Scenario: unload_sample does not delete outside samples dir
- **GIVEN** a sample path outside samples directory
- **WHEN** unload_sample is called
- **THEN** no deletion occurs

#### Scenario: poll_loader_events handles malformed events
- **GIVEN** a malformed loader event
- **WHEN** poll_loader_events processes it
- **THEN** the event is handled gracefully without crashing

---

### Requirement: ProjectPersistence Path Normalization Test Coverage
The test suite SHALL include unit tests for ProjectPersistence covering path normalization and atomic write failures.

The ProjectPersistence tests SHALL verify:
- Path normalization for absolute and relative paths
- Path preservation for paths outside samples directory
- Atomic write failure handling
- Dirty state behavior

#### Scenario: atomic_write cleans up temp file on failure
- **GIVEN** atomic_write fails
- **WHEN** the failure occurs
- **THEN** the temp file is cleaned up

#### Scenario: _normalize_sample_paths_for_save handles absolute paths
- **GIVEN** an absolute path
- **WHEN** normalization occurs
- **THEN** the path is normalized to relative format

#### Scenario: _normalize_sample_paths_for_save handles relative paths
- **GIVEN** a relative path
- **WHEN** normalization occurs
- **THEN** the path is normalized

#### Scenario: _normalize_sample_paths_for_save preserves outside samples paths
- **GIVEN** a path outside samples directory
- **WHEN** normalization occurs
- **THEN** the path is preserved unchanged

#### Scenario: flush does not write when not dirty
- **GIVEN** persistence is not dirty
- **WHEN** flush is called
- **THEN** no write occurs

#### Scenario: maybe_flush returns False when not dirty
- **GIVEN** persistence is not dirty
- **WHEN** maybe_flush is called
- **THEN** False is returned

#### Scenario: persistence round-trip with complex state
- **GIVEN** a project with all fields populated
- **WHEN** write and read occur
- **THEN** all fields are preserved

#### Scenario: persistence preserves Windows paths
- **GIVEN** Windows-style paths with backslashes
- **WHEN** persistence occurs
- **THEN** backslashes are preserved

#### Scenario: atomic_write handles OSError during directory creation
- **GIVEN** OSError occurs during directory creation
- **WHEN** atomic_write runs
- **THEN** the error is handled gracefully

---

### Requirement: BpmController Validation Test Coverage
The test suite SHALL include unit tests for BpmController covering invalid inputs and edge cases.

The BpmController tests SHALL verify:
- Invalid BPM validation (≤0, non-finite)
- Tap BPM edge cases (<3 taps, non-monotonic, negative intervals, very slow)
- Master BPM recomputation scenarios
- Audio engine updates

#### Scenario: set_manual_bpm raises ValueError for BPM ≤ 0
- **GIVEN** BPM ≤ 0 (e.g., -10, 0)
- **WHEN** set_manual_bpm is called
- **THEN** ValueError is raised

#### Scenario: set_manual_bpm raises ValueError for non-finite BPM
- **GIVEN** non-finite BPM (NaN or infinity)
- **WHEN** set_manual_bpm is called
- **THEN** ValueError is raised

#### Scenario: tap_bpm returns None with < 3 taps
- **GIVEN** fewer than 3 taps recorded
- **WHEN** tap_bpm is called
- **THEN** None is returned

#### Scenario: tap_bpm returns None for non-monotonic timestamps
- **GIVEN** non-monotonic tap timestamps (same time or decreasing)
- **WHEN** tap_bpm is called
- **THEN** None is returned

#### Scenario: tap_bpm returns None for negative intervals
- **GIVEN** negative tap intervals
- **WHEN** tap_bpm is called
- **THEN** None is returned

#### Scenario: tap_bpm returns None for very slow tempo
- **GIVEN** very slow tap intervals (approaching infinity)
- **WHEN** tap_bpm is called
- **THEN** None is returned

#### Scenario: recompute_master_bpm clears master BPM when unlocked
- **GIVEN** bpm_lock is False
- **WHEN** recompute_master_bpm is called
- **THEN** master BPM is cleared

#### Scenario: recompute_master_bpm clears master BPM when anchor is None
- **GIVEN** anchor_bpm is None
- **WHEN** recompute_master_bpm is called
- **THEN** master BPM is cleared

#### Scenario: recompute_master_bpm clears master BPM when anchor is non-finite
- **GIVEN** non-finite anchor_bpm
- **WHEN** recompute_master_bpm is called
- **THEN** master BPM is cleared

#### Scenario: on_pad_bpm_changed updates pad BPM on audio
- **GIVEN** a pad with BPM change
- **WHEN** on_pad_bpm_changed is called
- **THEN** pad BPM is updated on the audio engine

#### Scenario: on_pad_bpm_changed updates master BPM when anchor changed
- **GIVEN** anchor pad BPM changed
- **WHEN** on_pad_bpm_changed is called
- **THEN** master BPM is updated

#### Scenario: on_pad_bpm_changed does nothing when not anchor
- **GIVEN** non-anchor pad BPM changed
- **WHEN** on_pad_bpm_changed is called
- **THEN** no master BPM update occurs

---

### Requirement: PadController Validation Test Coverage
The test suite SHALL include unit tests for PadController covering invalid key and gain values.

The PadController tests SHALL verify:
- Empty key validation
- Non-finite gain validation
- Non-finite EQ validation

#### Scenario: set_manual_key raises ValueError for empty string
- **GIVEN** an empty key string
- **WHEN** set_manual_key is called
- **THEN** ValueError is raised

#### Scenario: set_pad_gain raises ValueError for non-finite value
- **GIVEN** non-finite gain (NaN or infinity)
- **WHEN** set_pad_gain is called
- **THEN** ValueError is raised

#### Scenario: set_pad_eq raises ValueError for non-finite values
- **GIVEN** non-finite EQ values
- **WHEN** set_pad_eq is called
- **THEN** ValueError is raised

---

### Requirement: PadLoopController Edge Case Test Coverage
The test suite SHALL include unit tests for PadLoopController covering quantization, snap behavior, and edge cases.

The PadLoopController tests SHALL verify:
- Reset behavior with missing analysis
- Quantization to sample boundaries
- Set auto snap behavior
- Set bars validation and no-op
- Set start validation and quantization
- Set end behavior
- Effective region computation
- Snap to nearest beat
- Time quantization

#### Scenario: reset sets auto=True and end=None when no analysis
- **GIVEN** no analysis available for sample
- **WHEN** reset is called
- **THEN** auto is set to True and end is set to None

#### Scenario: reset uses 0.0 when no beats
- **GIVEN** analysis with no beats
- **WHEN** reset is called
- **THEN** start is set to 0.0

#### Scenario: reset quantizes times to sample boundaries
- **GIVEN** a sample rate
- **WHEN** reset is called
- **THEN** times are quantized to sample boundaries

#### Scenario: set_auto snaps start when enabled
- **GIVEN** auto is enabled and beats exist
- **WHEN** set_auto(True) is called
- **THEN** start snaps to nearest beat

#### Scenario: set_auto does nothing when disabled
- **GIVEN** auto is disabled
- **WHEN** set_auto(False) is called
- **THEN** no change occurs

#### Scenario: set_auto does nothing when already in state
- **GIVEN** auto already in requested state
- **WHEN** set_auto is called
- **THEN** no change occurs

#### Scenario: set_bars clamps to 1 when ≤ 0
- **GIVEN** bars ≤ 0
- **WHEN** set_bars is called
- **THEN** bars is clamped to 1

#### Scenario: set_bars does nothing when unchanged
- **GIVEN** bars already at requested value
- **WHEN** set_bars is called
- **THEN** no change occurs

#### Scenario: set_start clamps negative to 0.0
- **GIVEN** negative start value
- **WHEN** set_start is called
- **THEN** start is clamped to 0.0

#### Scenario: set_start quantizes to sample boundaries
- **GIVEN** a sample rate
- **WHEN** set_start is called
- **THEN** start is quantized to sample boundaries

#### Scenario: set_end clears when None
- **GIVEN** None for end
- **WHEN** set_end is called
- **THEN** end is cleared

#### Scenario: set_end clears when ≤ start
- **GIVEN** end ≤ start_s
- **WHEN** set_end is called
- **THEN** end is cleared

#### Scenario: set_end quantizes to sample boundaries
- **GIVEN** a sample rate
- **WHEN** set_end is called
- **THEN** end is quantized to sample boundaries

#### Scenario: set_end raises ValueError for non-finite
- **GIVEN** non-finite end
- **WHEN** set_end is called
- **THEN** ValueError is raised

#### Scenario: effective_region returns manual values when auto=False
- **GIVEN** auto=False
- **WHEN** effective_region is called
- **THEN** manual values are returned

#### Scenario: effective_region handles auto=True with None BPM
- **GIVEN** auto=True and BPM is None
- **WHEN** effective_region is called
- **THEN** region is computed without BPM

#### Scenario: effective_region handles auto=True with no beats
- **GIVEN** auto=True and no beats
- **WHEN** effective_region is called
- **THEN** region is computed without beats

#### Scenario: _snap_to_nearest_beat returns target when empty
- **GIVEN** empty beats list
- **WHEN** _snap_to_nearest_beat is called
- **THEN** target is returned unchanged

#### Scenario: _snap_to_nearest_beat finds closest beat
- **GIVEN** beats list
- **WHEN** _snap_to_nearest_beat is called
- **THEN** closest beat is returned

#### Scenario: _quantize_time returns unchanged when sample_rate is None
- **GIVEN** sample_rate is None
- **WHEN** _quantize_time_to_output_samples is called
- **THEN** value is returned unchanged

#### Scenario: _quantize_time returns unchanged when sample_rate is invalid
- **GIVEN** invalid sample_rate
- **WHEN** _quantize_time_to_output_samples is called
- **THEN** value is returned unchanged

#### Scenario: _apply_effective_region skips when not loaded
- **GIVEN** sample not loaded
- **WHEN** _apply_effective_pad_loop_region_to_audio is called
- **THEN** the method skips

---

### Requirement: GlobalParametersController No-Op Test Coverage
The test suite SHALL include unit tests for GlobalParametersController covering no-op behavior and edge cases.

The GlobalParametersController tests SHALL verify:
- No-op behavior for already-enabled/disabled states
- None and non-finite effective_bpm handling
- Anchor clearing on disable

#### Scenario: set_key_lock does nothing when already in state
- **GIVEN** key_lock already in requested state
- **WHEN** set_key_lock is called
- **THEN** no change occurs

#### Scenario: set_bpm_lock does nothing when already in state
- **GIVEN** bpm_lock already in requested state
- **WHEN** set_bpm_lock is called
- **THEN** no change occurs

#### Scenario: set_bpm_lock handles None effective_bpm
- **GIVEN** effective_bpm is None
- **WHEN** set_bpm_lock(True) is called
- **THEN** the method handles gracefully

#### Scenario: set_bpm_lock handles non-finite effective_bpm
- **GIVEN** non-finite effective_bpm
- **WHEN** set_bpm_lock(True) is called
- **THEN** the method handles gracefully

#### Scenario: set_bpm_lock disable clears anchor
- **GIVEN** bpm_lock is being disabled
- **WHEN** set_bpm_lock(False) is called
- **THEN** anchor pad and BPM are cleared

---

### Requirement: PadPlaybackController Loop Region Test Coverage
The test suite SHALL include unit tests for PadPlaybackController covering loop region application and validation.

The PadPlaybackController tests SHALL verify:
- Loop region application before playback
- Gain application
- Play method behavior
- Toggle method behavior
- Invalid sample_id validation

#### Scenario: trigger_pad applies loop region before playing
- **GIVEN** a pad with loop region configured
- **WHEN** trigger_pad is called
- **THEN** loop region is applied before playback starts

#### Scenario: trigger_pad plays with gain=1.0
- **GIVEN** a pad is triggered
- **WHEN** trigger_pad is called
- **THEN** sample plays with gain=1.0

#### Scenario: play method plays without stopping others
- **GIVEN** a pad is played
- **WHEN** play is called
- **THEN** other pads continue playing

#### Scenario: play applies loop region before playing
- **GIVEN** a pad with loop region configured
- **WHEN** play is called
- **THEN** loop region is applied before playback starts

#### Scenario: toggle stops pad when active
- **GIVEN** an active pad
- **WHEN** toggle is called
- **THEN** the pad is stopped

#### Scenario: toggle starts pad when inactive
- **GIVEN** an inactive pad
- **WHEN** toggle is called
- **THEN** the pad is started

#### Scenario: trigger_pad raises ValueError for invalid sample_id
- **GIVEN** invalid sample_id (out of range)
- **WHEN** trigger_pad is called
- **THEN** ValueError is raised

#### Scenario: stop_pad raises ValueError for invalid sample_id
- **GIVEN** invalid sample_id (out of range)
- **WHEN** stop_pad is called
- **THEN** ValueError is raised

