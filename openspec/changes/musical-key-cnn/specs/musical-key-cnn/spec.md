# musical-key-cnn Specification

## Purpose
To define CNN-based musical key detection using the KeyNet model (MusicalKeyCNN) running entirely in Rust on a background analysis thread. The pipeline computes a CQT spectrogram from mono audio, runs ONNX inference via `ort`, and maps the 24-class output to a musical key string. Preprocessing (mono mixing, resampling) is shared with the BPM detection pipeline to avoid redundant work, and both pipelines execute in parallel for maximum throughput.

## Requirements

### Requirement: Shared Preprocessing Before Parallel Pipelines
The system SHALL decode the audio file once, convert to mono, and resample to a common sample rate before forking into parallel BPM and key detection pipelines. The mono audio buffer SHALL be cloned or shared (not re-decoded) for each pipeline.

The system SHALL resample the mono buffer to 44100 Hz when the source sample rate differs from 44100 Hz, because both the BPM pipeline (qm-dsp) and the key detection pipeline (CQT at fixed parameters) require consistent sample rate input.

#### Scenario: Mono buffer is shared between pipelines
- **GIVEN** a loaded audio file with stereo channels at 48000 Hz
- **WHEN** analysis is triggered
- **THEN** the system decodes the file once and converts to mono
- **AND** the system resamples the mono buffer to 44100 Hz
- **AND** the resulting mono buffer is used by both the BPM detection pipeline and the key detection pipeline
- **AND** no additional decoding or resampling occurs

#### Scenario: Mono buffer at native 44100 Hz skips resampling
- **GIVEN** a loaded audio file at 44100 Hz
- **WHEN** analysis is triggered
- **THEN** the system converts to mono without resampling
- **AND** the mono buffer is shared between both pipelines

### Requirement: Parallel BPM and Key Detection
The system SHALL execute BPM detection and key detection concurrently on separate threads after shared preprocessing completes. Both pipelines SHALL run to completion before the analysis result is assembled and returned.

The system SHALL use Rust threading (e.g., `std::thread::scope` or `rayon`) to run both pipelines in parallel. The total analysis time SHALL be bounded by the slower of the two pipelines, not their sum.

#### Scenario: Both pipelines run concurrently
- **GIVEN** a loaded audio file with a shared mono buffer
- **WHEN** analysis is triggered
- **THEN** the BPM detection pipeline starts on one thread
- **AND** the key detection pipeline starts on a separate thread
- **AND** both pipelines execute concurrently
- **AND** the system waits for both to complete before returning results

#### Scenario: Analysis result combines both pipeline outputs
- **GIVEN** both pipelines have completed
- **WHEN** the system assembles the analysis result
- **THEN** the result contains BPM from the BPM pipeline
- **AND** the result contains a musical key string from the key detection pipeline
- **AND** the result contains the beat grid from the BPM pipeline

### Requirement: CQT Spectrogram Preprocessing
The system SHALL compute a Constant-Q Transform spectrogram from the mono 44100 Hz audio buffer using parameters that match the model's training data exactly.

The system SHALL use the following CQT parameters:
- `n_bins`: 105
- `bins_per_octave`: 24
- `fmin`: 65 Hz
- `hop_length`: 8820 samples

The system SHALL apply log-magnitude compression (`log1p`) to the CQT magnitude and SHALL remove the last frequency bin, producing a tensor of shape `(1, 104, T)` where `T` is the number of time frames.

#### Scenario: CQT produces correctly shaped tensor
- **GIVEN** a mono audio buffer at 44100 Hz with 441000 samples (10 seconds)
- **WHEN** the CQT preprocessing runs
- **THEN** the system computes the CQT with n_bins=105, bins_per_octave=24, fmin=65, hop_length=8820
- **AND** the system applies log1p to the magnitude
- **AND** the system removes the last frequency bin
- **AND** the resulting tensor has shape (1, 104, 49)

#### Scenario: CQT output matches librosa reference
- **GIVEN** a known audio buffer
- **WHEN** the Rust CQT implementation processes it
- **THEN** the output values are within floating-point tolerance of `librosa.cqt` with matching parameters
- **AND** the log1p-transformed tensor matches the Python preprocessing pipeline

### Requirement: ONNX Model Inference
The system SHALL load the pretrained KeyNet ONNX model (`keynet.onnx`) and execute inference using the `ort` Rust crate. The model SHALL be loaded once and cached for reuse across all analysis calls.

The system SHALL pass the CQT tensor as input to the model and SHALL extract the 24-class logits from the output. The predicted key SHALL be the class with the highest logit value (`argmax`).

#### Scenario: Model loads and caches on first use
- **GIVEN** the system starts with no cached model
- **WHEN** key detection is triggered for the first time
- **THEN** the system loads `keynet.onnx` and creates an `ort::Session`
- **AND** the session is cached for subsequent calls
- **AND** subsequent calls reuse the cached session without reloading

#### Scenario: Inference produces 24-class logits
- **GIVEN** a cached ONNX session and a valid CQT tensor
- **WHEN** inference is executed
- **THEN** the model outputs a tensor of shape (1, 24)
- **AND** the system identifies the class with the highest logit value
- **AND** the predicted class index is in the range 0 to 23

#### Scenario: Model load failure is handled gracefully
- **GIVEN** the model file is missing or corrupted
- **WHEN** key detection is triggered
- **THEN** the system reports a key detection error
- **AND** the analysis result uses `"unknown"` as the key value
- **AND** the BPM pipeline result is not affected

### Requirement: Camelot Index to Key String Mapping
The system SHALL map the predicted Camelot Wheel index (0–23) to a musical key string. Indices 0–11 represent minor keys and indices 12–23 represent major keys, following the Camelot Wheel ordering.

The system SHALL produce key strings in musical notation format (e.g., `"Am"`, `"C"`, `"Bbm"`, `"F#m"`) suitable for display in a professional audio application.

#### Scenario: Minor key mapping
- **GIVEN** a predicted class index of 7
- **WHEN** the system maps the index to a key string
- **THEN** the result is `"Am"` (A minor)

#### Scenario: Major key mapping
- **GIVEN** a predicted class index of 19
- **WHEN** the system maps the index to a key string
- **THEN** the result is `"C"` (C major)

#### Scenario: All 24 keys map correctly
- **GIVEN** each class index from 0 to 23
- **WHEN** the system maps each index
- **THEN** the results match the Camelot Wheel mapping:
  - 0→"Abm", 1→"Ebm", 2→"Bbm", 3→"Fm", 4→"Cm", 5→"Gm"
  - 6→"Dm", 7→"Am", 8→"Em", 9→"Bm", 10→"F#m", 11→"C#m"
  - 12→"B", 13→"F#", 14→"C#", 15→"Ab", 16→"Eb", 17→"Bb"
  - 18→"F", 19→"C", 20→"G", 21→"D", 22→"A", 23→"E"

### Requirement: Key Detection Runs on Background Thread
The system SHALL execute all key detection work (CQT computation, ONNX inference, key mapping) on a background thread. The system SHALL NOT execute key detection on the real-time audio callback thread.

The system SHALL NOT acquire the Python GIL during key detection. All operations SHALL be pure Rust with no Python interop.

#### Scenario: Key detection does not block the audio thread
- **GIVEN** the audio callback is running
- **WHEN** key detection executes on a background thread
- **THEN** the audio callback experiences no additional latency
- **AND** no GIL acquisition occurs during key detection

#### Scenario: Key detection does not block the UI thread
- **GIVEN** the UI is rendering frames
- **WHEN** key detection executes on a background thread
- **THEN** the UI remains responsive
- **AND** analysis progress is reported via the existing event system

### Requirement: Graceful Error Handling
The system SHALL handle all key detection failures gracefully. When key detection fails (model error, CQT error, empty audio, silence), the system SHALL return `"unknown"` as the key value and SHALL NOT propagate the error to the BPM pipeline or the caller.

The system SHALL distinguish between recoverable errors (logged and retried on next analysis) and fatal errors (reported to the user via the analysis error channel).

#### Scenario: Empty audio returns unknown key
- **GIVEN** an audio buffer with fewer samples than required for one CQT frame
- **WHEN** key detection is triggered
- **THEN** the system returns `"unknown"` as the key value
- **AND** no error is propagated to the caller

#### Scenario: CQT computation failure returns unknown key
- **GIVEN** a mono buffer that causes a CQT computation error
- **WHEN** key detection is triggered
- **THEN** the system returns `"unknown"` as the key value
- **AND** the BPM pipeline result is not affected
- **AND** the error is logged for diagnostics
