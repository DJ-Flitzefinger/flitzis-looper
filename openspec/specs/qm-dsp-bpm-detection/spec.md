# qm-dsp-bpm-detection Specification

## Purpose
TBD - created by archiving change qm-dsp-bpm. Update Purpose after archive.
## Requirements
### Requirement: Onset Detection Function Computation
The system SHALL compute an onset detection function (ODF) for a mono audio buffer using the Complex Spectral Difference method.

The system SHALL use a Hann-windowed real FFT per frame, extract magnitude and phase, compute phase deviation (second-order phase derivative), and produce a single scalar ODF value per frame representing the summed complex spectral difference across all frequency bins.

The frame length SHALL be the next power of 2 of `sampleRate / maxBinHz` (default maxBinHz = 50 Hz). The step size SHALL be `sampleRate * stepSecs` (default stepSecs = 0.01161).

#### Scenario: ODF computed for a valid audio buffer
- **GIVEN** a mono audio buffer with valid samples and a known sample rate
- **WHEN** the DetectionFunction processes the buffer
- **THEN** the system produces a sequence of ODF values (one per frame)
- **AND** the number of ODF values equals `ceil(totalSamples / stepSize)`

#### Scenario: ODF handles silent input
- **GIVEN** a mono audio buffer containing only zero samples
- **WHEN** the DetectionFunction processes the buffer
- **THEN** the system produces ODF values that are all zero or near-zero

### Requirement: Beat Period Estimation via Viterbi HMM
The system SHALL estimate the most likely beat period sequence using a Viterbi hidden Markov model over a resonator comb filter (RCF) bank.

The system SHALL compute a Rayleigh weighting curve centered at the input tempo (default 120 BPM) over 128 candidate beat periods. For each sliding window of ODF frames (default window=512, hop=128), the system SHALL compute an RCF bank score for each candidate period using autocorrelation of the adaptively-thresholded ODF. The Viterbi decoder SHALL find the most likely beat period sequence through the RCF probability matrix using a diagonal-Gaussian transition model (default sigma=8).

#### Scenario: Beat period estimated for rhythmic audio
- **GIVEN** an ODF sequence from a rhythmic audio signal
- **WHEN** the TempoTrackV2 calculates beat periods
- **THEN** the system produces a sequence of beat period values (in frames)
- **AND** the beat period values change smoothly over time (enforced by Viterbi transitions)

#### Scenario: Beat period defaults to input tempo for ambiguous input
- **GIVEN** an ODF sequence with weak or no rhythmic structure
- **WHEN** the TempoTrackV2 calculates beat periods
- **THEN** the system produces beat period values centered around the input tempo

### Requirement: Beat Position Tracking via Dynamic Programming
The system SHALL determine precise beat positions using a dynamic programming approach over the ODF sequence.

For each ODF frame, the system SHALL find the best previous beat location within +/- 50% of the current beat period, weighted by a Gaussian transition pattern. The cumulative score SHALL blend the best previous score with the local ODF value using alpha blending (default alpha=0.9, tightness=4.0). The system SHALL backtrack from the strongest point in the last beat period to recover all beat positions.

#### Scenario: Beat positions tracked for consistent tempo audio
- **GIVEN** an ODF sequence and beat period estimates from TempoTrackV2
- **WHEN** the system calculates beat positions
- **THEN** the system produces a list of beat positions in frames
- **AND** consecutive beat intervals are approximately equal to the estimated beat period

#### Scenario: Beat positions handle tempo changes
- **GIVEN** an ODF sequence with gradual tempo changes
- **AND** beat period estimates that vary over time
- **WHEN** the system calculates beat positions
- **THEN** the beat positions follow the changing tempo
- **AND** beat intervals adjust to match the local beat period estimates

### Requirement: Downbeat and Bar Detection
The system SHALL estimate downbeat positions (first beat of each bar) from the detected beat positions.

The system SHALL analyze the spectral difference between regions before and after each beat to identify which beats are downbeats. The system SHALL group downbeats into bar positions.

#### Scenario: Downbeats detected for 4/4 audio
- **GIVEN** a list of beat positions from a 4/4 time signature audio signal
- **WHEN** the system calculates downbeats
- **THEN** the system produces a list of downbeat positions
- **AND** downbeat positions are a subset of the beat positions

#### Scenario: Empty downbeats for ambiguous input
- **GIVEN** a list of beat positions with no clear bar structure
- **WHEN** the system calculates downbeats
- **THEN** the system may produce an empty list of downbeats
- **AND** the system does not error

### Requirement: Analysis Config with Sensible Defaults
The system SHALL expose an `AnalysisConfig` struct that controls all tunable parameters of the detection pipeline.

The default configuration SHALL match the Mixxx qm-dsp defaults: stepSecs=0.01161, maxBinHz=50, inputTempo=120, alpha=0.9, tightness=4.0, viterbiSigma=8.0, windowLength=512, hopSize=128.

#### Scenario: Default config produces valid analysis
- **GIVEN** an `AnalysisConfig` created with default values
- **WHEN** the analysis pipeline processes a valid audio buffer
- **THEN** the system produces BPM, beat positions, and downbeat positions

