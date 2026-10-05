//! Bounded offline history/discard sweep. The independent source/native oracle is shared with
//! the earlier proof, never replaced by the production reader. Engineering budgets below are
//! deliberately declared before measuring; this test reports failures without changing them.

use super::*;

const HISTORIES: [usize; 3] = [0, 8192, 16_384];
const MARKERS: [usize; 3] = [0, 17, 511];
const TIMING_BUDGET_MS: f64 = 2.0;
const MAX_DISCARDED_ENERGY: f64 = 0.001;
const MAX_CAPTURE_TAIL_ENERGY: f64 = 0.000_001;

#[derive(Clone, Copy)]
enum Signal {
    Impulse,
    Tone,
    Percussion,
}

impl Signal {
    fn name(self) -> &'static str {
        match self {
            Self::Impulse => "impulse",
            Self::Tone => "tone",
            Self::Percussion => "percussion",
        }
    }
}

fn marker_fixture(
    rate: u32,
    ratio: f32,
    history: usize,
    marker: usize,
    signal: Signal,
    capture_frames: usize,
) -> Fixture {
    let mut fixture = impulse_fixture(rate, ratio, history + marker, capture_frames);
    if matches!(signal, Signal::Impulse) {
        return fixture;
    }
    let mut samples = vec![0.0; fixture.sample.samples.len()];
    let first = ((history + marker) as f64 * f64::from(ratio)).round() as usize;
    // Fixed source-domain durations/frequencies: dry varispeed and wet Key Lock differ in pitch,
    // so waveform correlation would not be a timing oracle for these bursts.
    let duration = (f64::from(rate) * 0.008).round() as usize;
    for offset in 0..duration {
        let time = offset as f64 / f64::from(rate);
        let progress = offset as f64 / duration as f64;
        let value = match signal {
            Signal::Tone => {
                let envelope = (std::f64::consts::PI * progress).sin().powi(2);
                (envelope * (std::f64::consts::TAU * 900.0 * time + 0.37).sin()) as f32
            }
            Signal::Percussion => {
                let envelope = (-7.0 * progress).exp();
                (envelope * (std::f64::consts::TAU * 180.0 * time + 0.7).sin()) as f32 * 0.7
                    + component_sample(offset, 0, 0) * envelope as f32 * 2.0
            }
            Signal::Impulse => unreachable!(),
        };
        samples[(first + offset) * 2] = value;
        samples[(first + offset) * 2 + 1] = value * 0.5;
    }
    fixture.sample.samples = Arc::from(samples);
    fixture
}

fn stereo_energy(channels: &[Vec<f32>]) -> Vec<f64> {
    assert_eq!(channels.len(), 2);
    assert_eq!(channels[0].len(), channels[1].len());
    channels[0]
        .iter()
        .zip(&channels[1])
        .map(|(left, right)| f64::from(*left).powi(2) + f64::from(*right).powi(2))
        .collect()
}

fn energy_time(energy: &[f64], fraction: f64) -> usize {
    let total = energy.iter().sum::<f64>();
    assert!(total.is_finite() && total > 0.0);
    let target = total * fraction;
    let mut cumulative = 0.0;
    for (frame, value) in energy.iter().enumerate() {
        cumulative += value;
        if cumulative >= target {
            return frame;
        }
    }
    energy.len() - 1
}

/// Centered 0.5-ms box energy envelope. Its origin is the original sample timeline, with zero
/// extension at capture edges, so no causal-window latency is added to the reported times.
fn envelope_energy(energy: &[f64], rate: u32) -> Vec<f64> {
    let half_window = (f64::from(rate) * 0.000_25).round() as usize;
    let mut prefix = Vec::with_capacity(energy.len() + 1);
    prefix.push(0.0);
    for value in energy {
        prefix.push(prefix.last().unwrap() + value);
    }
    // Include negative/after-capture envelope centers. Truncation at zero would lose half
    // the envelope energy of a marker at zero and bias the reference quantiles.
    (0..energy.len() + 2 * half_window)
        .map(|frame| {
            let start = frame.saturating_sub(2 * half_window).min(energy.len());
            let end = (frame + 1).min(energy.len());
            (prefix[end] - prefix[start]) / (2 * half_window + 1) as f64
        })
        .collect()
}

fn energy_times(energy: &[f64], rate: u32) -> [i64; 3] {
    let envelope = envelope_energy(energy, rate);
    let half_window = (f64::from(rate) * 0.000_25).round() as i64;
    [0.1, 0.5, 0.9].map(|fraction| energy_time(&envelope, fraction) as i64 - half_window)
}

/// Largest raw discard D for which energy in [0,D) is within the declared budget.
fn maximum_energy_discard(energy: &[f64]) -> usize {
    let budget = energy.iter().sum::<f64>() * MAX_DISCARDED_ENERGY;
    let mut cumulative = 0.0;
    for (frame, value) in energy.iter().enumerate() {
        cumulative += value;
        if cumulative > budget {
            return frame;
        }
    }
    energy.len()
}

fn history_continuation(
    fixture: &Fixture,
    history: usize,
    discard: usize,
    reference: &NativeReference,
    pattern: &[usize],
    output_frames: usize,
) -> Vec<Vec<f32>> {
    let mut request = fixture.request(history, discard);
    let logical_before = request.logical.position();
    request.source_history = Some(SourceHistory {
        origin: fixture.request(0, 0).logical,
        output_frames: history,
    });
    let mut prepared = PreparedSourceStream::prepare(
        &fixture.sample,
        None,
        request,
        RubberBandLiveShifter::new(fixture.rate, 2).unwrap(),
    )
    .unwrap();
    assert_eq!(request.logical.position(), logical_before);
    assert_eq!(prepared.logical_position(), logical_before);
    assert_eq!(prepared.history_output_frames(), history);
    fixture.assert_position(prepared.history_origin_position(), 0);
    let block = reference.block_size;
    let feed_frames = (discard + block - 1).div_ceil(block) * block;
    let ready_frames = feed_frames - discard;
    assert_eq!(prepared.prepared_feed_frames(), feed_frames);
    assert_eq!(prepared.retained_frames(), ready_frames);
    let mut output = (0..2)
        .map(|_| Vec::with_capacity(output_frames))
        .collect::<Vec<_>>();
    let mut elapsed = 0;
    let mut partition = 0;
    while elapsed < output_frames {
        let frames = pattern[partition % pattern.len()].min(output_frames - elapsed);
        let rendered = prepared.render(frames).unwrap();
        for channel in 0..2 {
            assert_eq!(
                &rendered[channel][..frames],
                &reference.output[channel][discard + elapsed..discard + elapsed + frames],
                "history suffix differs: rate={} ratio={} H={} D={} elapsed={}",
                fixture.rate,
                fixture.ratio,
                history,
                discard,
                elapsed,
            );
            output[channel].extend_from_slice(&rendered[channel][..frames]);
        }
        elapsed += frames;
        partition += 1;
        fixture.assert_position(prepared.logical_position(), history + elapsed);
        fixture.assert_position(prepared.feed_position(), feed_frames + elapsed);
        assert_eq!(prepared.pending_input_frames(), elapsed % block);
        assert_eq!(prepared.fifo_occupancy(), ready_frames - elapsed % block);
    }
    output
}

#[derive(Default)]
struct HistoryCsv {
    rows: String,
    groups: String,
}

impl HistoryCsv {
    fn save_if_requested(self) {
        if let Some(path) = std::env::var_os("FLITZIS_KEY_LOCK_HISTORY_PROBE_CSV") {
            let path = std::path::PathBuf::from(path);
            assert!(
                path.is_absolute(),
                "history probe CSV path must be absolute"
            );
            let header = concat!(
                "sample_rate_hz,tempo_ratio,signal,history_frames,marker,callback_pattern,",
                "candidate_offset_frames,discard_frames,native_delay,exact_reference_equal,",
                "dry_onset,dry_one_percent_onset,dry_peak,dry_q10,dry_q50,dry_q90,",
                "raw_onset_residual,raw_one_percent_onset_residual,raw_peak_residual,",
                "raw_q10_residual,raw_q50_residual,raw_q90_residual,",
                "retained_onset,retained_one_percent_raw_peak_onset,retained_peak,",
                "original_peak_retained,discarded_energy_fraction,capture_tail_energy_fraction,",
                "cut_jump,join_jump,max_raw_adjacent_jump,criterion_pass,",
                "max_energy_discard_frame,raw_q10_frame,raw_q50_frame,raw_peak_frame\n"
            );
            std::fs::write(&path, format!("{header}{}", self.rows)).unwrap();
            let summary_header = concat!(
                "sample_rate_hz,tempo_ratio,history_frames,fixture_count,",
                "timing_lower_frames,timing_upper_frames,retention_upper_frames,",
                "combined_lower_frames,combined_upper_frames,capture_tail_pass,",
                "any_integer_translation_feasible,common_tested_candidates\n"
            );
            std::fs::write(
                path.with_extension("summary.csv"),
                format!("{summary_header}{}", self.groups),
            )
            .unwrap();
        }
    }
}

#[test]
fn bounded_history_discard_marker_and_burst_sweep_preserves_reference_and_reports_criterion() {
    let mut csv = HistoryCsv::default();
    let mut comparisons = 0;
    let mut failures = 0;
    for rate in RATES {
        for ratio in RATIOS {
            // Exact initialization queried independently, before generating any source stream.
            let mut native = RubberBandLiveShifter::new(rate, 2).unwrap();
            native.set_pitch_scale(f64::from(1.0_f32 / ratio)).unwrap();
            native.reset_for_preparation();
            let delay = native.start_delay();
            let block = native.block_size();
            let candidates = [
                delay.saturating_sub(2 * block),
                delay - block,
                delay,
                delay + block,
            ];
            for history in HISTORIES {
                let budget = (f64::from(rate) * TIMING_BUDGET_MS / 1000.0).ceil() as i64;
                let mut timing_lower = -(history as i64);
                let mut timing_upper =
                    (MAX_PREPARATION_OUTPUT_FRAMES - block + 1) as i64 - history as i64;
                let mut retention_upper = timing_upper;
                let mut tail_valid = true;
                let mut common_tested = [true; 4];
                for marker in MARKERS {
                    for signal in [Signal::Impulse, Signal::Tone, Signal::Percussion] {
                        let output_frames = marker + rate as usize / 8;
                        let max_discard = history + candidates[3];
                        let fixture = marker_fixture(
                            rate,
                            ratio,
                            history,
                            marker,
                            signal,
                            max_discard + output_frames,
                        );
                        let reference = native_reference(&fixture, 0, max_discard + output_frames);
                        let dry = (0..2)
                            .map(|channel| {
                                (0..history + output_frames)
                                    .map(|frame| fixture.reference_sample(frame, channel))
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>();
                        let dry_response = response(&dry[0]);
                        let dry_times = energy_times(&stereo_energy(&dry), rate);
                        let raw_response = response(&reference.output[0]);
                        let raw_energy = stereo_energy(&reference.output);
                        let raw_times = energy_times(&raw_energy, rate);
                        let total_energy = raw_energy.iter().sum::<f64>();
                        let tail_frames = rate as usize / 50;
                        let tail_fraction = raw_energy[raw_energy.len() - tail_frames..]
                            .iter()
                            .sum::<f64>()
                            / total_energy;
                        for index in 0..2 {
                            let center = raw_times[index] - dry_times[index];
                            timing_lower = timing_lower.max(center - budget);
                            timing_upper = timing_upper.min(center + budget);
                        }
                        let energy_limit = maximum_energy_discard(&raw_energy);
                        retention_upper = retention_upper
                            .min(raw_response.peak_frame.min(energy_limit) as i64 - history as i64);
                        tail_valid &= tail_fraction <= MAX_CAPTURE_TAIL_ENERGY;
                        let max_raw_jump = reference.output[0]
                            .windows(2)
                            .map(|pair| (pair[1] - pair[0]).abs())
                            .fold(0.0_f32, f32::max);
                        for (candidate_index, candidate) in candidates.into_iter().enumerate() {
                            let discard = history + candidate;
                            let discarded_fraction =
                                raw_energy[..discard].iter().sum::<f64>() / total_energy;
                            let peak_retained = raw_response.peak_frame >= discard;
                            let q_residuals: [i64; 3] = std::array::from_fn(|index| {
                                raw_times[index] - candidate as i64 - dry_times[index]
                            });
                            let passed = q_residuals[0].abs() <= budget
                                && q_residuals[1].abs() <= budget
                                && peak_retained
                                && discarded_fraction <= MAX_DISCARDED_ENERGY
                                && tail_fraction <= MAX_CAPTURE_TAIL_ENERGY;
                            common_tested[candidate_index] &= passed;
                            for (pattern_name, pattern) in PATTERNS {
                                let retained = history_continuation(
                                    &fixture,
                                    history,
                                    discard,
                                    &reference,
                                    pattern,
                                    output_frames,
                                );
                                let retained_response = response(&retained[0]);
                                let retained_onset_at_raw_threshold = retained[0]
                                    .iter()
                                    .position(|sample| sample.abs() >= raw_response.peak * 0.01);
                                let cut_jump = retained[0][0].abs();
                                // Join is between the initially ready FIFO suffix and its first
                                // continued native output sample; no mode crossfade is claimed.
                                let ready = (discard + block - 1).div_ceil(block) * block - discard;
                                let join_jump = (retained[0][ready] - retained[0][ready - 1]).abs();
                                let raw_residual = |frame: usize, dry_frame: usize| {
                                    frame as i64 - candidate as i64 - dry_frame as i64
                                };
                                let relative_dry = |frame: usize| frame as i64 - history as i64;
                                writeln!(csv.rows,
                                    "{rate},{ratio},{},{history},{marker},{pattern_name},{candidate},{discard},{delay},1,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{discarded_fraction},{tail_fraction},{cut_jump},{join_jump},{max_raw_jump},{},{},{},{},{}",
                                    signal.name(),
                                    relative_dry(dry_response.onset.unwrap()),
                                    relative_dry(dry_response.significant_onset.unwrap()),
                                    relative_dry(dry_response.peak_frame),
                                    dry_times[0] - history as i64, dry_times[1] - history as i64, dry_times[2] - history as i64,
                                    raw_residual(raw_response.onset.unwrap(), dry_response.onset.unwrap()),
                                    raw_residual(raw_response.significant_onset.unwrap(), dry_response.significant_onset.unwrap()),
                                    raw_residual(raw_response.peak_frame, dry_response.peak_frame),
                                    q_residuals[0], q_residuals[1], q_residuals[2],
                                    retained_response.onset.map_or(-1, |frame| frame as i64),
                                    retained_onset_at_raw_threshold.map_or(-1, |frame| frame as i64),
                                    retained_response.peak_frame, usize::from(peak_retained),
                                    usize::from(passed), maximum_energy_discard(&raw_energy),
                                    raw_times[0], raw_times[1], raw_response.peak_frame,
                                ).unwrap();
                                comparisons += 1;
                                failures += usize::from(!passed);
                            }
                        }
                    }
                }
                let combined_upper = timing_upper.min(retention_upper);
                writeln!(
                    csv.groups,
                    "{rate},{ratio},{history},9,{timing_lower},{timing_upper},{retention_upper},{timing_lower},{combined_upper},{},{},{}",
                    usize::from(tail_valid),
                    usize::from(tail_valid && timing_lower <= combined_upper),
                    common_tested.into_iter().filter(|passed| *passed).count(),
                ).unwrap();
            }
        }
    }
    assert_eq!(comparisons, 6480);
    assert!(
        failures > 0,
        "sweep must expose failed musical timing candidates"
    );
    csv.save_if_requested();
}

#[test]
fn centered_envelope_has_no_causal_delay_and_stereo_energy_uses_both_channels() {
    let mut signal = vec![vec![0.0; 1000]; 2];
    signal[1][500] = 2.0;
    let energy = stereo_energy(&signal);
    assert_eq!(energy.iter().sum::<f64>(), 4.0);
    let times = energy_times(&energy, 48_000);
    assert_eq!(times[1], 500);
    assert_eq!(times[0] + times[2], 1000);
    assert!(times[0] < 500 && times[2] > 500);
    signal[1][500] = 0.0;
    signal[1][0] = 2.0;
    let at_start = energy_times(&stereo_energy(&signal), 48_000);
    assert_eq!(at_start[1], 0);
    assert_eq!(at_start[0] + at_start[2], 0);
    assert!(at_start[0] < 0 && at_start[2] > 0);
    assert_eq!(maximum_energy_discard(&[0.001, 0.999]), 1);
}
