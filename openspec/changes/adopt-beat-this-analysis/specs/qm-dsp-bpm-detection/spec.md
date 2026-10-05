## MODIFIED Requirements

### Requirement: Onset Detection Function Computation
The system SHALL provide the qm-dsp Complex Spectral Difference onset detection function (ODF)
for explicitly selected legacy or diagnostic analysis, not as the mandatory new-analysis default
or an automatic fallback from unavailable Beat This inference.

For that backend, the system SHALL use a Hann-windowed real FFT per frame, extract magnitude
and phase, compute phase deviation (second-order phase derivative), and produce a scalar ODF
value per frame representing summed complex spectral difference across frequency bins.

The frame length SHALL be the next power of 2 of `sampleRate / maxBinHz` (default maxBinHz=50).
The integer sample step SHALL be derived from `sampleRate * stepSecs` (default stepSecs=0.01161)
and carried with an explicit unit when converting legacy analysis positions.

#### Scenario: Explicit legacy ODF computation
- **GIVEN** a mono buffer with a known sample rate and explicitly selected legacy analysis
- **WHEN** DetectionFunction processes the buffer
- **THEN** it produces one ODF value per frame
- **AND** the ODF count equals `ceil(totalSamples / stepSize)`

#### Scenario: Legacy ODF handles silence
- **GIVEN** a zero-valued mono buffer in legacy/diagnostic analysis
- **WHEN** DetectionFunction processes the buffer
- **THEN** its ODF values are zero or near-zero

### Requirement: Beat Period Estimation via Viterbi HMM
The system SHALL estimate the most likely legacy beat-period sequence using a Viterbi hidden
Markov model over a resonator comb filter bank when the qm-dsp backend is explicitly selected.

The system SHALL compute a Rayleigh weighting curve centered at input tempo (default 120 BPM)
over 128 candidate periods. For each sliding ODF window (default window=512, hop=128), it SHALL
compute candidate RCF scores using autocorrelation of adaptively thresholded ODF. The Viterbi
decoder SHALL find a sequence through the probability matrix using a diagonal-Gaussian
transition model (default sigma=8). These priors SHALL NOT be imposed on Beat This output.

#### Scenario: Legacy beat periods follow rhythmic input
- **GIVEN** an ODF from rhythmic audio under explicit legacy analysis
- **WHEN** TempoTrackV2 calculates beat periods
- **THEN** it produces beat-period values in ODF frames
- **AND** Viterbi transitions favor smooth changes

#### Scenario: Ambiguous legacy input uses its prior
- **GIVEN** a legacy ODF with weak or no rhythmic structure
- **WHEN** TempoTrackV2 calculates beat periods
- **THEN** its periods are centered around configured input tempo
- **AND** this does not establish a confident Beat This or accepted-map result

### Requirement: Beat Position Tracking via Dynamic Programming
The system SHALL determine legacy beat positions by dynamic programming over the ODF when
the qm-dsp backend is explicitly selected for legacy/diagnostic analysis.

For each ODF frame, the system SHALL find the best previous beat within +/-50% of the current
period using a Gaussian transition pattern. The cumulative score SHALL blend the best previous
score with local ODF using alpha blending (default alpha=0.9, tightness=4.0). It SHALL backtrack
from the strongest point in the last period to recover all positions. These positions SHALL
retain legacy provenance and SHALL NOT be relabeled as Beat This detections.

#### Scenario: Legacy positions follow consistent tempo
- **GIVEN** legacy ODF and TempoTrackV2 period estimates
- **WHEN** legacy beat-position tracking runs
- **THEN** it produces beat positions in ODF frames
- **AND** consecutive intervals approximately match estimated periods

#### Scenario: Legacy positions follow gradual tempo change
- **GIVEN** a legacy ODF and period estimates with gradual tempo changes
- **WHEN** legacy beat-position tracking runs
- **THEN** beat intervals adjust to local period estimates
- **AND** no subsequent requirement forces these positions to a constant-tempo grid

### Requirement: Downbeat and Bar Detection
The system SHALL estimate legacy downbeats from detected beat positions when the qm-dsp
backend is explicitly selected, retaining that backend's limited bar-phase model and provenance.

It SHALL analyze spectral difference before/after beats to identify downbeats and group them
into bar positions. ODF indices SHALL convert using the actual integer sample increment;
seconds SHALL NOT be passed as a sample-hop count. This legacy requirement SHALL NOT mandate
its phase model or bar grouping for the selected Beat This default.

#### Scenario: Explicit legacy downbeat estimation
- **GIVEN** beat positions for 4/4 audio under explicit legacy/diagnostic analysis
- **WHEN** legacy downbeat detection runs with the correct sample increment
- **THEN** it produces estimated downbeat positions drawn from those beats
- **AND** the result retains legacy provenance rather than implying verified meter labels

#### Scenario: Ambiguous legacy bar structure
- **GIVEN** legacy beat positions with no clear bar structure
- **WHEN** legacy downbeat detection runs
- **THEN** it may produce an empty downbeat list without error
- **AND** lack of an error is not proof of correct bar phase

### Requirement: Analysis Config with Sensible Defaults
The system SHALL expose `AnalysisConfig` for explicitly selected legacy/diagnostic qm-dsp
analysis, independently of the selected Beat This worker/model configuration.

Its defaults SHALL remain stepSecs=0.01161, maxBinHz=50, inputTempo=120, alpha=0.9, tightness=4.0,
viterbiSigma=8.0, windowLength=512 and hopSize=128. Preserved legacy configuration SHALL NOT
select the legacy backend automatically when the new default runtime or model is missing.

#### Scenario: Explicit legacy defaults remain reproducible
- **GIVEN** a default AnalysisConfig and explicitly selected legacy analysis
- **WHEN** the legacy pipeline processes valid audio
- **THEN** it produces legacy BPM/beat/downbeat estimates using the documented defaults
- **AND** its configuration is not applied to Beat This logits or postprocessing
