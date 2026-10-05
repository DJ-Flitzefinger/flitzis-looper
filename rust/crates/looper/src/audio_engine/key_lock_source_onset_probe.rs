//! Longer-history musical onset diagnostic, compiled only through the test-only history probe.
//!
//! A paired attack-minus-control output is an incremental nonlinear diagnostic, never an
//! isolated attack oracle. The fixed launch bridge deliberately has a varispeed-pitched dry
//! beginning. Neither its numerical result nor exact suffix equality selects live behavior.

use super::*;
use std::collections::BTreeMap;

const LONG_HISTORIES: [usize; 2] = [32_768, 65_536];
const ONSET_RATIOS: [f32; 3] = [0.5, 1.0, 2.0];
const ONSET_MARKERS: [usize; 2] = [17, 511];
const DRY_HOLD_MS: f64 = 2.0;
const FADE_MS: f64 = 5.0;

#[derive(Clone, Copy)]
struct Attack {
    signal: Signal,
    duration_ms: f64,
    frequency_hz: f64,
    phase: f64,
}

fn attacks() -> [Attack; 6] {
    std::array::from_fn(|index| {
        let (duration_ms, frequency_hz) = [(1.0, 180.0), (8.0, 900.0), (32.0, 3500.0)][index / 2];
        Attack {
            signal: if index % 2 == 0 {
                Signal::Tone
            } else {
                Signal::Percussion
            },
            duration_ms,
            frequency_hz,
            phase: if (index + index / 2) % 2 == 0 {
                0.37
            } else {
                1.1
            },
        }
    })
}

#[derive(Clone, Copy)]
struct OnsetCase {
    rate: u32,
    ratio: f32,
    history: usize,
    marker: usize,
    attack: Attack,
    background: bool,
}

impl OnsetCase {
    fn source_attack(self) -> usize {
        ((self.history + self.marker) as f64 * f64::from(self.ratio)).round() as usize
    }

    fn fixture(self, capture_frames: usize, include_attack: bool) -> Fixture {
        let mut fixture = impulse_fixture(
            self.rate,
            self.ratio,
            self.history + self.marker,
            capture_frames,
        );
        let first = self.source_attack();
        let duration = (f64::from(self.rate) * self.attack.duration_ms / 1000.0).round() as usize;
        let samples = (0..fixture.sample.samples.len() / 2)
            .flat_map(|frame| {
                (0..2).map(move |channel| {
                    let relative_frame = frame as i64 - first as i64;
                    // Anchor background phase to the attack, not to history origin. Increasing H
                    // adds preceding content without changing any local source event samples.
                    let time = relative_frame as f64 / f64::from(self.rate);
                    let background = if self.background {
                        let phase = channel as f64 * 0.83;
                        (0.032 * (std::f64::consts::TAU * 220.0 * time + phase).sin()
                            + 0.019 * (std::f64::consts::TAU * 660.0 * time + phase + 0.4).sin())
                            as f32
                    } else {
                        0.0
                    };
                    let attack = if include_attack && (0..duration as i64).contains(&relative_frame)
                    {
                        let offset = relative_frame as usize;
                        let progress = offset as f64 / duration as f64;
                        let carrier = (std::f64::consts::TAU * self.attack.frequency_hz * time
                            + self.attack.phase)
                            .sin();
                        let value = match self.attack.signal {
                            Signal::Tone => {
                                (std::f64::consts::PI * progress).sin().powi(2) * carrier
                            }
                            Signal::Percussion => {
                                (-7.0 * progress).exp()
                                    * (0.7 * carrier
                                        + f64::from(component_sample(offset, 0, 0)) * 2.0)
                            }
                            Signal::Impulse => unreachable!(),
                        };
                        value as f32 * if channel == 0 { 1.0 } else { 0.5 }
                    } else {
                        0.0
                    };
                    background + attack
                })
            })
            .collect::<Vec<_>>();
        if self.background {
            assert!(
                samples[..first * 2]
                    .iter()
                    .any(|sample| sample.abs() > 0.01)
            );
        }
        fixture.sample.samples = Arc::from(samples);
        fixture
    }
}

fn dry_reference(fixture: &Fixture, frames: usize) -> Vec<Vec<f32>> {
    (0..2)
        .map(|channel| {
            (0..frames)
                .map(|frame| fixture.reference_sample(frame, channel))
                .collect()
        })
        .collect()
}

fn suffix(channels: &[Vec<f32>], start: usize, frames: usize) -> Vec<Vec<f32>> {
    channels
        .iter()
        .map(|channel| channel[start..start + frames].to_vec())
        .collect()
}

fn difference(left: &[Vec<f32>], right: &[Vec<f32>]) -> Vec<Vec<f32>> {
    assert_eq!(left.len(), right.len());
    left.iter()
        .zip(right)
        .map(|(left, right)| {
            assert_eq!(left.len(), right.len());
            left.iter()
                .zip(right)
                .map(|(left, right)| left - right)
                .collect()
        })
        .collect()
}

/// A single output-clock coefficient, shared by stereo and independent of callback partitions.
/// The bridge begins at launch H, never at a measured or signal-specific attack coordinate.
fn dry_weight(output_frame: usize, rate: u32) -> f32 {
    let hold = (f64::from(rate) * DRY_HOLD_MS / 1000.0).round() as usize;
    let fade = (f64::from(rate) * FADE_MS / 1000.0).round() as usize;
    if output_frame < hold {
        return 1.0;
    }
    if output_frame >= hold + fade {
        return 0.0;
    }
    let progress = (output_frame - hold) as f64 / fade as f64;
    ((1.0 + (std::f64::consts::PI * progress).cos()) * 0.5) as f32
}

fn launch_bridge(dry: &[Vec<f32>], wet: &[Vec<f32>], rate: u32) -> Vec<Vec<f32>> {
    assert_eq!(dry.len(), wet.len());
    dry.iter()
        .zip(wet)
        .map(|(dry, wet)| {
            assert_eq!(dry.len(), wet.len());
            dry.iter()
                .zip(wet)
                .enumerate()
                .map(|(frame, (dry, wet))| {
                    let weight = dry_weight(frame, rate);
                    // Explicit endpoints preserve the original dry/wet samples bit for bit.
                    if weight == 1.0 {
                        *dry
                    } else if weight == 0.0 {
                        *wet
                    } else {
                        weight * dry + (1.0 - weight) * wet
                    }
                })
                .collect()
        })
        .collect()
}

fn stereo_peak(channels: &[Vec<f32>]) -> (usize, f32) {
    let mut result = (0, 0.0_f32);
    for (frame, (left, right)) in channels[0].iter().zip(&channels[1]).enumerate() {
        let value = left.abs().max(right.abs());
        if value > result.1 {
            result = (frame, value);
        }
    }
    result
}

fn stereo_response(channels: &[Vec<f32>]) -> Response {
    let magnitudes = channels[0]
        .iter()
        .zip(&channels[1])
        .map(|(left, right)| left.abs().max(right.abs()))
        .collect::<Vec<_>>();
    response(&magnitudes)
}

fn max_adjacent_jump(channels: &[Vec<f32>]) -> f32 {
    channels
        .iter()
        .flat_map(|channel| channel.windows(2))
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

fn adjacent_jump(channels: &[Vec<f32>], frame: usize) -> f32 {
    assert!(frame > 0 && frame < channels[0].len());
    channels
        .iter()
        .map(|channel| (channel[frame] - channel[frame - 1]).abs())
        .fold(0.0, f32::max)
}

fn cut_amplitude(channels: &[Vec<f32>]) -> f32 {
    channels
        .iter()
        .map(|channel| channel[0].abs())
        .fold(0.0, f32::max)
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct HistoryWindowKey {
    rate: u32,
    ratio: u32,
    marker: usize,
    duration: u64,
    frequency: u64,
    phase: u64,
    signal: &'static str,
    background: bool,
    scope: &'static str,
}

#[derive(Default)]
struct OnsetCsv {
    rows: String,
    groups: String,
    short_history_windows: BTreeMap<HistoryWindowKey, Vec<Vec<f32>>>,
}

impl OnsetCsv {
    fn emit(
        &mut self,
        case: OnsetCase,
        scope: &str,
        metric: &str,
        value: impl std::fmt::Display,
        unit: &str,
    ) {
        writeln!(
            self.rows,
            "{},{},{},{},{},{},{},{},{},{scope},{metric},{value},{unit}",
            case.rate,
            case.ratio,
            case.history,
            case.marker,
            case.attack.signal.name(),
            case.attack.duration_ms,
            case.attack.frequency_hz,
            case.attack.phase,
            if case.background {
                "periodic_stereo"
            } else {
                "silence"
            }
        )
        .unwrap();
    }

    /// Compare matching H-relative raw output, not two raw prefixes at different event times.
    /// A difference here diagnoses remaining history dependence; it does not certify steady state.
    fn compare_histories(
        &mut self,
        case: OnsetCase,
        scope: &'static str,
        raw: &[Vec<f32>],
        delay: usize,
        frames: usize,
    ) {
        let preceding = 2048;
        let window = suffix(raw, case.history - preceding, preceding + delay + frames);
        let key = HistoryWindowKey {
            rate: case.rate,
            ratio: case.ratio.to_bits(),
            marker: case.marker,
            duration: case.attack.duration_ms.to_bits(),
            frequency: case.attack.frequency_hz.to_bits(),
            phase: case.attack.phase.to_bits(),
            signal: case.attack.signal.name(),
            background: case.background,
            scope,
        };
        if case.history == LONG_HISTORIES[0] {
            assert!(self.short_history_windows.insert(key, window).is_none());
        } else {
            let shorter = self.short_history_windows.remove(&key).unwrap();
            let error = difference(&window, &shorter);
            let total = stereo_energy(&shorter).iter().sum::<f64>();
            self.emit(
                case,
                scope,
                "history_window_start_relative_to_H",
                -(preceding as i64),
                "frames",
            );
            self.emit(
                case,
                scope,
                "history_window_end_relative_to_H",
                delay + frames,
                "frames",
            );
            self.emit(
                case,
                scope,
                "history_stability_relative_l2",
                (stereo_energy(&error).iter().sum::<f64>() / total).sqrt(),
                "ratio",
            );
            self.emit(
                case,
                scope,
                "history_stability_max_sample_error",
                stereo_peak(&error).1,
                "linear",
            );
        }
    }

    fn save_if_requested(self) {
        assert!(
            self.short_history_windows.is_empty(),
            "every short-history window needs its longer partner"
        );
        if let Some(path) = std::env::var_os("FLITZIS_KEY_LOCK_ONSET_PROBE_CSV") {
            let path = std::path::PathBuf::from(path);
            assert!(path.is_absolute(), "onset probe CSV path must be absolute");
            let header = concat!(
                "sample_rate_hz,tempo_ratio,history_frames,marker,signal,",
                "duration_ms,frequency_hz,carrier_phase,background,measurement_scope,metric,value,unit\n"
            );
            std::fs::write(&path, format!("{header}{}", self.rows)).unwrap();
            let summary = concat!(
                "sample_rate_hz,tempo_ratio,history_frames,background,",
                "measurement_scope,fixture_count,min_offset_frames,max_offset_frames,",
                "timing_spread_frames,minimax_integer_frames,best_offset_frames,",
                "timing_lower_frames,timing_upper_frames,retention_upper_frames,",
                "tail_pass,any_integer_translation_feasible,nominal_passes,bridge_passes\n"
            );
            std::fs::write(
                path.with_extension("summary.csv"),
                format!("{summary}{}", self.groups),
            )
            .unwrap();
        }
    }
}

/// Exact integer minimax for a common translation of q10/q50 offsets. Adding a common delay
/// shifts the optimum but cannot change the smallest achievable worst residual.
fn integer_minimax(minimum: i64, maximum: i64) -> (i64, i64) {
    assert!(minimum <= maximum);
    let spread = maximum - minimum;
    (minimum + spread / 2, (spread + 1) / 2)
}

struct TimingGroup {
    count: usize,
    minimum: i64,
    maximum: i64,
    lower: i64,
    upper: i64,
    retention_upper: i64,
    tail_pass: bool,
    nominal_passes: usize,
    bridge_passes: usize,
}

impl TimingGroup {
    fn new() -> Self {
        Self {
            count: 0,
            minimum: i64::MAX,
            maximum: i64::MIN,
            lower: i64::MIN,
            upper: i64::MAX,
            retention_upper: i64::MAX,
            tail_pass: true,
            nominal_passes: 0,
            bridge_passes: 0,
        }
    }

    fn add(&mut self, case: OnsetCase, measured: &MeasuredOnset) {
        let budget = timing_budget(case.rate);
        self.count += 1;
        for offset in measured.offsets[..2].iter().copied() {
            self.minimum = self.minimum.min(offset);
            self.maximum = self.maximum.max(offset);
            self.lower = self.lower.max(offset - budget);
            self.upper = self.upper.min(offset + budget);
        }
        self.lower = self.lower.max(-(case.history as i64));
        self.upper = self
            .upper
            .min((MAX_PREPARATION_OUTPUT_FRAMES - measured.block + 1) as i64 - case.history as i64);
        self.retention_upper = self.retention_upper.min(measured.retention_upper);
        self.tail_pass &= measured.tail_pass;
        self.nominal_passes += usize::from(measured.nominal_pass);
        self.bridge_passes += usize::from(measured.bridge_pass);
    }
}

fn timing_budget(rate: u32) -> i64 {
    (f64::from(rate) * TIMING_BUDGET_MS / 1000.0).ceil() as i64
}

struct MeasuredOnset {
    offsets: [i64; 3],
    retention_upper: i64,
    tail_pass: bool,
    nominal_pass: bool,
    bridge_pass: bool,
    block: usize,
}

fn emit_quantiles(csv: &mut OnsetCsv, case: OnsetCase, scope: &str, name: &str, values: [i64; 3]) {
    for (quantile, value) in ["q10", "q50", "q90"].into_iter().zip(values) {
        csv.emit(case, scope, &format!("{name}_{quantile}"), value, "frames");
    }
}

struct RawOnset {
    dry_times: [i64; 3],
    wet_times: [i64; 3],
    offsets: [i64; 3],
    residuals: [i64; 3],
    dry_response: Response,
    wet_response: Response,
    total: f64,
    discarded_fraction: f64,
    tail_fraction: f64,
    retention_upper: i64,
    peak_retained: bool,
}

impl RawOnset {
    fn new(case: OnsetCase, dry: &[Vec<f32>], wet: &[Vec<f32>], delay: usize) -> Self {
        let discard = case.history + delay;
        let energy = stereo_energy(wet);
        let total = energy.iter().sum::<f64>();
        let dry_times = energy_times(&stereo_energy(dry), case.rate);
        let wet_times = energy_times(&energy, case.rate);
        let offsets: [i64; 3] = std::array::from_fn(|index| wet_times[index] - dry_times[index]);
        let wet_response = stereo_response(wet);
        let retention_upper = wet_response.peak_frame.min(maximum_energy_discard(&energy)) as i64
            - case.history as i64;
        let peak_retained = wet_response.peak_frame >= discard;
        Self {
            dry_times,
            wet_times,
            offsets,
            residuals: offsets.map(|offset| offset - delay as i64),
            dry_response: stereo_response(dry),
            wet_response,
            total,
            discarded_fraction: energy[..discard].iter().sum::<f64>() / total,
            tail_fraction: energy[energy.len() - case.rate as usize / 50..]
                .iter()
                .sum::<f64>()
                / total,
            retention_upper,
            peak_retained,
        }
    }

    fn retention_pass(&self) -> bool {
        self.peak_retained && self.discarded_fraction <= MAX_DISCARDED_ENERGY
    }

    fn tail_pass(&self) -> bool {
        self.tail_fraction <= MAX_CAPTURE_TAIL_ENERGY
    }

    fn report(&self, csv: &mut OnsetCsv, case: OnsetCase, scope: &str, delay: usize) {
        let discard = case.history + delay;
        emit_quantiles(
            csv,
            case,
            scope,
            "dry_uncropped",
            self.dry_times.map(|time| time - case.history as i64),
        );
        emit_quantiles(
            csv,
            case,
            scope,
            "wet_uncropped",
            self.wet_times.map(|time| time - discard as i64),
        );
        emit_quantiles(csv, case, scope, "wet_uncropped_residual", self.residuals);
        csv.emit(
            case,
            scope,
            "q50_q10_spread_change",
            self.offsets[1] - self.offsets[0],
            "frames",
        );
        csv.emit(
            case,
            scope,
            "q90_q10_spread_change",
            self.offsets[2] - self.offsets[0],
            "frames",
        );
        let (_, minimax) = integer_minimax(
            self.offsets[0].min(self.offsets[1]),
            self.offsets[0].max(self.offsets[1]),
        );
        csv.emit(case, scope, "fixture_q10_q50_minimax", minimax, "frames");
        for (name, raw, dry) in [
            ("onset", self.wet_response.onset, self.dry_response.onset),
            (
                "one_percent_onset",
                self.wet_response.significant_onset,
                self.dry_response.significant_onset,
            ),
            (
                "peak",
                Some(self.wet_response.peak_frame),
                Some(self.dry_response.peak_frame),
            ),
        ] {
            csv.emit(
                case,
                scope,
                &format!("wet_uncropped_{name}"),
                raw.map_or(-1, |frame| frame as i64 - discard as i64),
                "frames",
            );
            csv.emit(
                case,
                scope,
                &format!("wet_uncropped_{name}_residual"),
                raw.zip(dry)
                    .map_or(-1, |(raw, dry)| raw as i64 - delay as i64 - dry as i64),
                "frames",
            );
        }
        csv.emit(
            case,
            scope,
            "raw_wet_peak_frame",
            self.wet_response.peak_frame,
            "frames",
        );
        csv.emit(
            case,
            scope,
            "raw_wet_retention_upper_frames",
            self.retention_upper,
            "frames",
        );
        csv.emit(
            case,
            scope,
            "raw_wet_peak_retained",
            usize::from(self.peak_retained),
            "bool",
        );
        csv.emit(
            case,
            scope,
            "raw_wet_discarded_energy_fraction",
            self.discarded_fraction,
            "fraction",
        );
        csv.emit(
            case,
            scope,
            "raw_wet_capture_tail_energy_fraction",
            self.tail_fraction,
            "fraction",
        );
        csv.emit(
            case,
            scope,
            "raw_wet_retention_pass",
            usize::from(self.retention_pass()),
            "bool",
        );
        csv.emit(
            case,
            scope,
            "raw_wet_tail_pass",
            usize::from(self.tail_pass()),
            "bool",
        );
    }

    /// q10=1, q50=2, original native peak=4, discarded native energy=8, capture tail=16.
    fn failure_mask(&self, residuals: [i64; 3], rate: u32) -> usize {
        usize::from(residuals[0].abs() > timing_budget(rate))
            | (usize::from(residuals[1].abs() > timing_budget(rate)) << 1)
            | (usize::from(!self.peak_retained) << 2)
            | (usize::from(self.discarded_fraction > MAX_DISCARDED_ENERGY) << 3)
            | (usize::from(!self.tail_pass()) << 4)
    }
}

fn report_launched_response(
    csv: &mut OnsetCsv,
    case: OnsetCase,
    scope: &str,
    name: &str,
    channels: &[Vec<f32>],
    dry: &[Vec<f32>],
    raw: &RawOnset,
) -> [i64; 3] {
    let times = energy_times(&stereo_energy(channels), case.rate);
    let dry_times = energy_times(&stereo_energy(dry), case.rate);
    let residuals = std::array::from_fn(|index| times[index] - dry_times[index]);
    emit_quantiles(
        csv,
        case,
        scope,
        &format!("{name}_launched_residual"),
        residuals,
    );
    let measured = stereo_response(channels);
    let dry_response = stereo_response(dry);
    for (metric, value) in [
        ("onset", measured.onset.map_or(-1, |frame| frame as i64)),
        (
            "one_percent_onset",
            measured.significant_onset.map_or(-1, |frame| frame as i64),
        ),
        (
            "one_percent_raw_peak_onset",
            channels[0]
                .iter()
                .zip(&channels[1])
                .position(|(left, right)| {
                    left.abs().max(right.abs()) >= raw.wet_response.peak * 0.01
                })
                .map_or(-1, |frame| frame as i64),
        ),
        ("peak_frame", measured.peak_frame as i64),
        (
            "peak_residual",
            measured.peak_frame as i64 - dry_response.peak_frame as i64,
        ),
    ] {
        csv.emit(
            case,
            scope,
            &format!("{name}_launched_{metric}"),
            value,
            "frames",
        );
    }
    let total = stereo_energy(channels).iter().sum::<f64>();
    csv.emit(
        case,
        scope,
        &format!("{name}_energy_over_launched_dry"),
        total / stereo_energy(dry).iter().sum::<f64>(),
        "ratio",
    );
    csv.emit(
        case,
        scope,
        &format!("{name}_launched_energy_over_raw_wet"),
        total / raw.total,
        "ratio",
    );
    csv.emit(
        case,
        scope,
        &format!("{name}_launched_peak_over_raw_peak"),
        f64::from(measured.peak) / f64::from(raw.wet_response.peak),
        "ratio",
    );
    residuals
}

fn report_bridge_jumps(
    csv: &mut OnsetCsv,
    case: OnsetCase,
    scope: &str,
    dry: &[Vec<f32>],
    wet: &[Vec<f32>],
    bridge: &[Vec<f32>],
    reference: &NativeReference,
) {
    let frames = wet[0].len();
    let discard = case.history + reference.delay;
    let ready = (discard + reference.block_size - 1).div_ceil(reference.block_size)
        * reference.block_size
        - discard;
    let wet_only_start = (f64::from(case.rate) * DRY_HOLD_MS / 1000.0).round() as usize
        + (f64::from(case.rate) * FADE_MS / 1000.0).round() as usize;
    let unchanged_wet = bridge
        .iter()
        .zip(wet)
        .all(|(bridge, wet)| bridge[wet_only_start..] == wet[wet_only_start..]);
    let weighted_dry = dry
        .iter()
        .map(|channel| {
            channel
                .iter()
                .enumerate()
                .map(|(frame, sample)| sample * dry_weight(frame, case.rate))
                .collect()
        })
        .collect::<Vec<_>>();
    let bridge_total = stereo_energy(bridge).iter().sum::<f64>();
    csv.emit(
        case,
        scope,
        "bridge_remaining_dry_component_energy_ratio",
        stereo_energy(&weighted_dry).iter().sum::<f64>() / bridge_total,
        "ratio_with_cross_terms",
    );
    csv.emit(
        case,
        scope,
        "bridge_dry_weight_sum",
        (0..frames)
            .map(|frame| f64::from(dry_weight(frame, case.rate)))
            .sum::<f64>(),
        "weighted_frames",
    );
    csv.emit(
        case,
        scope,
        "bridge_wet_only_start",
        wet_only_start,
        "frames",
    );
    csv.emit(
        case,
        scope,
        "bridge_wet_suffix_bit_exact",
        usize::from(unchanged_wet),
        "bool",
    );
    for (name, channels) in [("wet", wet), ("bridge", bridge)] {
        csv.emit(
            case,
            scope,
            &format!("{name}_cut_amplitude"),
            cut_amplitude(channels),
            "linear",
        );
        csv.emit(
            case,
            scope,
            &format!("{name}_fifo_join_jump"),
            adjacent_jump(channels, ready),
            "linear",
        );
        csv.emit(
            case,
            scope,
            &format!("{name}_max_adjacent_jump"),
            max_adjacent_jump(channels),
            "linear",
        );
    }
    csv.emit(
        case,
        scope,
        "bridge_transition_end_jump",
        adjacent_jump(bridge, wet_only_start),
        "linear",
    );
    assert!(
        unchanged_wet,
        "fixed bridge must leave every later wet sample untouched"
    );
}

/// Uncropped raw evidence and separately labelled launched/bridge metrics share unchanged budgets.
fn measure_onset(
    csv: &mut OnsetCsv,
    case: OnsetCase,
    scope: &'static str,
    dry: &[Vec<f32>],
    raw: &[Vec<f32>],
    reference: &NativeReference,
    launched: &[Vec<f32>],
) -> MeasuredOnset {
    let measured = RawOnset::new(case, dry, raw, reference.delay);
    measured.report(csv, case, scope, reference.delay);
    csv.compare_histories(case, scope, raw, reference.delay, launched[0].len());
    let dry_launch = suffix(dry, case.history, launched[0].len());
    emit_quantiles(
        csv,
        case,
        scope,
        "dry_launched",
        energy_times(&stereo_energy(&dry_launch), case.rate),
    );
    report_launched_response(csv, case, scope, "wet", launched, &dry_launch, &measured);
    let bridge = launch_bridge(&dry_launch, launched, case.rate);
    let bridge_residuals =
        report_launched_response(csv, case, scope, "bridge", &bridge, &dry_launch, &measured);
    report_bridge_jumps(csv, case, scope, &dry_launch, launched, &bridge, reference);
    let nominal_mask = measured.failure_mask(measured.residuals, case.rate);
    let bridge_mask = measured.failure_mask(bridge_residuals, case.rate);
    // Dry insertion cannot establish retention of native energy already cut at D.
    for (name, mask) in [("nominal", nominal_mask), ("bridge", bridge_mask)] {
        csv.emit(
            case,
            scope,
            &format!("{name}_numeric_criterion_pass"),
            usize::from(mask == 0),
            "bool",
        );
        csv.emit(
            case,
            scope,
            &format!("{name}_failure_mask"),
            mask,
            "bitmask",
        );
    }
    csv.emit(
        case,
        scope,
        "raw_wet_timing_pass",
        usize::from(nominal_mask & 3 == 0),
        "bool",
    );
    csv.emit(
        case,
        scope,
        "bridge_timing_pass",
        usize::from(bridge_mask & 3 == 0),
        "bool",
    );
    MeasuredOnset {
        offsets: measured.offsets,
        retention_upper: measured.retention_upper,
        tail_pass: measured.tail_pass(),
        nominal_pass: nominal_mask == 0,
        bridge_pass: bridge_mask == 0,
        block: reference.block_size,
    }
}

fn emit_nonadditivity(
    csv: &mut OnsetCsv,
    case: OnsetCase,
    incremental: &[Vec<f32>],
    isolated: &[Vec<f32>],
    dry_delta: &[Vec<f32>],
    dry_isolated: &[Vec<f32>],
) {
    let wet_error = difference(incremental, isolated);
    let dry_error = difference(dry_delta, dry_isolated);
    let isolated_energy = stereo_energy(isolated).iter().sum::<f64>();
    csv.emit(
        case,
        "paired_difference_diagnostic",
        "wet_nonadditivity_relative_l2",
        (stereo_energy(&wet_error).iter().sum::<f64>() / isolated_energy).sqrt(),
        "ratio",
    );
    csv.emit(
        case,
        "paired_difference_diagnostic",
        "wet_nonadditivity_peak_error",
        stereo_peak(&wet_error).1,
        "linear",
    );
    csv.emit(
        case,
        "paired_difference_diagnostic",
        "dry_subtraction_max_roundoff",
        stereo_peak(&dry_error).1,
        "linear",
    );
    // f32 mixture/subtraction can round; the exact known source sum is not native additivity.
    assert!(stereo_peak(&dry_error).1 <= 2.0e-7);
}

fn prove_nominal_suffix(
    csv: &mut OnsetCsv,
    case: OnsetCase,
    fixture: &Fixture,
    reference: &NativeReference,
    frames: usize,
) -> Vec<Vec<f32>> {
    let discard = case.history + reference.delay;
    let mut canonical = None;
    for (name, pattern) in [PATTERNS[2], PATTERNS[3]] {
        let output =
            history_continuation(fixture, case.history, discard, reference, pattern, frames);
        if let Some(expected) = &canonical {
            assert_eq!(&output, expected);
        } else {
            canonical = Some(output);
        }
        csv.emit(
            case,
            "actual_mix",
            &format!("exact_nominal_suffix_{name}"),
            1,
            "bool",
        );
    }
    canonical.unwrap()
}

/// Feed the bridge's dry branch through the production reader, while retaining the independent
/// algebraic reference as the timing oracle. Both callback partitions must produce exact PCM.
fn prove_shared_dry_launch(
    csv: &mut OnsetCsv,
    case: OnsetCase,
    fixture: &Fixture,
    expected: &[Vec<f32>],
    frames: usize,
) -> Vec<Vec<f32>> {
    let mut canonical = None;
    for (name, pattern) in [PATTERNS[2], PATTERNS[3]] {
        let request = fixture.request(case.history, 0);
        let mut playback = request.logical;
        let mut buffers = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; 2];
        let mut output = (0..2)
            .map(|_| Vec::with_capacity(frames))
            .collect::<Vec<_>>();
        let mut elapsed = 0;
        let mut partition = 0;
        while elapsed < frames {
            let count = pattern[partition % pattern.len()].min(frames - elapsed);
            request.plan.fill_fractional_buffers(
                &fixture.sample,
                fixture.stems.as_ref(),
                &playback,
                &mut buffers,
                count,
            );
            for channel in 0..2 {
                output[channel].extend_from_slice(&buffers[channel][..count]);
            }
            playback.advance(count);
            elapsed += count;
            partition += 1;
            fixture.assert_position(playback.position(), case.history + elapsed);
        }
        assert_eq!(
            &output, expected,
            "shared-reader dry launch differs from independent source"
        );
        csv.emit(
            case,
            "actual_mix",
            &format!("exact_shared_dry_launch_{name}"),
            1,
            "bool",
        );
        canonical = Some(output);
    }
    canonical.unwrap()
}

fn probe_case(
    csv: &mut OnsetCsv,
    case: OnsetCase,
    frames: usize,
    isolated_fixture: &Fixture,
    isolated_reference: &NativeReference,
    control: Option<(&Fixture, &NativeReference)>,
) -> MeasuredOnset {
    let capture = case.history + frames;
    let mix_fixture = case.background.then(|| case.fixture(capture, true));
    let mix_reference = mix_fixture
        .as_ref()
        .map(|fixture| native_reference(fixture, 0, capture));
    let fixture = mix_fixture.as_ref().unwrap_or(isolated_fixture);
    let reference = mix_reference.as_ref().unwrap_or(isolated_reference);
    assert_eq!(case.history % reference.block_size, 0);
    let launched = prove_nominal_suffix(csv, case, fixture, reference, frames);
    let mut dry = dry_reference(fixture, reference.output[0].len());
    let independent_launch = suffix(&dry, case.history, frames);
    let shared_launch = prove_shared_dry_launch(csv, case, fixture, &independent_launch, frames);
    // The uncropped dry oracle is unchanged byte for byte; the candidate's launch interval
    // comes from the shared production reader after direct independent equality checks.
    for channel in 0..2 {
        dry[channel][case.history..case.history + frames].copy_from_slice(&shared_launch[channel]);
    }
    csv.emit(
        case,
        "actual_mix",
        "nominal_delay",
        reference.delay,
        "frames",
    );
    csv.emit(
        case,
        "actual_mix",
        "discard",
        case.history + reference.delay,
        "frames",
    );
    if let Some((control_fixture, control_reference)) = control {
        measure_onset(
            csv,
            case,
            "total_mix_context_not_attack",
            &dry,
            &reference.output,
            reference,
            &launched,
        );
        let control_dry = dry_reference(control_fixture, dry[0].len());
        let dry_delta = difference(&dry, &control_dry);
        let dry_isolated = dry_reference(isolated_fixture, dry[0].len());
        let incremental = difference(&reference.output, &control_reference.output);
        emit_nonadditivity(
            csv,
            case,
            &incremental,
            &isolated_reference.output,
            &dry_delta,
            &dry_isolated,
        );
        let incremental_launch = suffix(&incremental, case.history + reference.delay, frames);
        measure_onset(
            csv,
            case,
            "paired_difference_diagnostic",
            &dry_delta,
            &incremental,
            reference,
            &incremental_launch,
        )
    } else {
        measure_onset(
            csv,
            case,
            "isolated_attack",
            &dry,
            &reference.output,
            reference,
            &launched,
        )
    }
}

#[test]
fn longer_history_musical_onsets_report_fixed_launch_bridge_and_nonadditivity() {
    let mut csv = OnsetCsv::default();
    let mut groups: BTreeMap<(u32, u32, usize, bool), TimingGroup> = BTreeMap::new();
    let mut fixtures = 0;
    for rate in RATES {
        for ratio in ONSET_RATIOS {
            for history in LONG_HISTORIES {
                for marker in ONSET_MARKERS {
                    let frames = marker + rate as usize / 4;
                    let control_case = OnsetCase {
                        rate,
                        ratio,
                        history,
                        marker,
                        attack: attacks()[0],
                        background: true,
                    };
                    let control_fixture = control_case.fixture(history + frames, false);
                    let control_reference = native_reference(&control_fixture, 0, history + frames);
                    for attack in attacks() {
                        let isolated_case = OnsetCase {
                            attack,
                            background: false,
                            ..control_case
                        };
                        let isolated_fixture = isolated_case.fixture(history + frames, true);
                        let isolated_reference =
                            native_reference(&isolated_fixture, 0, history + frames);
                        for background in [false, true] {
                            let case = OnsetCase {
                                background,
                                ..isolated_case
                            };
                            let measured = probe_case(
                                &mut csv,
                                case,
                                frames,
                                &isolated_fixture,
                                &isolated_reference,
                                background.then_some((&control_fixture, &control_reference)),
                            );
                            groups
                                .entry((rate, ratio.to_bits(), history, background))
                                .or_insert_with(TimingGroup::new)
                                .add(case, &measured);
                            fixtures += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(fixtures, 432);
    for ((rate, ratio, history, background), group) in groups {
        let ratio = f32::from_bits(ratio);
        let (best, minimax) = integer_minimax(group.minimum, group.maximum);
        let feasible = group.tail_pass && group.lower <= group.upper.min(group.retention_upper);
        writeln!(
            csv.groups,
            "{rate},{ratio},{history},{},{},{},{},{},{},{minimax},{best},{},{},{},{},{},{},{}",
            if background {
                "periodic_stereo"
            } else {
                "silence"
            },
            if background {
                "paired_difference_diagnostic"
            } else {
                "isolated_attack"
            },
            group.count,
            group.minimum,
            group.maximum,
            group.maximum - group.minimum,
            group.lower,
            group.upper,
            group.retention_upper,
            usize::from(group.tail_pass),
            usize::from(feasible),
            group.nominal_passes,
            group.bridge_passes
        )
        .unwrap();
    }
    csv.save_if_requested();
}

#[test]
fn longer_history_changes_only_preceding_content_not_local_source_event() {
    for ratio in ONSET_RATIOS {
        for attack in attacks() {
            let case = OnsetCase {
                rate: 48_000,
                ratio,
                history: LONG_HISTORIES[0],
                marker: 511,
                attack,
                background: true,
            };
            let longer = OnsetCase {
                history: LONG_HISTORIES[1],
                ..case
            };
            let first = case.fixture(case.history + 16_000, true);
            let second = longer.fixture(longer.history + 16_000, true);
            let event = case.source_attack();
            let later_event = longer.source_attack();
            assert_eq!(
                &first.sample.samples[(event - 1000) * 2..(event + 2000) * 2],
                &second.sample.samples[(later_event - 1000) * 2..(later_event + 2000) * 2]
            );
            // Matching H-relative windows include the same fractional source sampling, not
            // merely the same integer PCM neighborhood. Raw-wet stability is measured, not assumed.
            let first_dry = dry_reference(&first, case.history + 4096);
            let second_dry = dry_reference(&second, longer.history + 4096);
            assert_eq!(
                suffix(&first_dry, case.history - 2048, 6144),
                suffix(&second_dry, longer.history - 2048, 6144)
            );
        }
    }
}

#[test]
fn fixed_bridge_coefficients_share_stereo_endpoints_and_absolute_output_time() {
    for rate in RATES {
        let frames = rate as usize / 50;
        let dry = vec![vec![0.7; frames], vec![-0.35; frames]];
        let wet = vec![vec![-0.2; frames], vec![0.1; frames]];
        let expected = launch_bridge(&dry, &wet, rate);
        assert_eq!(expected[0][0], dry[0][0]);
        assert_eq!(expected[1][0], dry[1][0]);
        assert_eq!(dry_weight(frames - 1, rate), 0.0);
        assert_eq!(expected[0][frames - 1], wet[0][frames - 1]);
        for (left, right) in expected[0].iter().zip(&expected[1]) {
            assert_eq!(*left, -2.0 * right);
        }
        for (_, pattern) in [PATTERNS[2], PATTERNS[3]] {
            let mut assembled = vec![Vec::new(), Vec::new()];
            let mut elapsed = 0;
            let mut partition = 0;
            while elapsed < frames {
                let count = pattern[partition % pattern.len()].min(frames - elapsed);
                for channel in 0..2 {
                    for frame in elapsed..elapsed + count {
                        let weight = dry_weight(frame, rate);
                        let value = if weight == 1.0 {
                            dry[channel][frame]
                        } else if weight == 0.0 {
                            wet[channel][frame]
                        } else {
                            weight * dry[channel][frame] + (1.0 - weight) * wet[channel][frame]
                        };
                        assembled[channel].push(value);
                    }
                }
                elapsed += count;
                partition += 1;
            }
            assert_eq!(assembled, expected);
        }
    }
}

#[test]
fn centered_energy_and_integer_minimax_preserve_translation_and_spread() {
    let mut original = vec![0.0; 1000];
    original[350] = 1.0;
    original[450] = 3.0;
    let mut translated = vec![0.0; 1200];
    translated[550] = 1.0;
    translated[650] = 3.0;
    for rate in RATES {
        let first = energy_times(&original, rate);
        let second = energy_times(&translated, rate);
        assert_eq!(second, first.map(|time| time + 200));
        assert_eq!(second[2] - second[0], first[2] - first[0]);
    }
    for (minimum, maximum) in [(-3, 4), (2, 2), (-100, -94), (300, 309)] {
        let (best, bound) = integer_minimax(minimum, maximum);
        assert_eq!((minimum - best).abs().max((maximum - best).abs()), bound);
        assert_eq!(
            integer_minimax(minimum + 137, maximum + 137),
            (best + 137, bound)
        );
        assert!((-150..=450).all(|candidate| {
            (minimum - candidate).abs().max((maximum - candidate).abs()) >= bound
        }));
    }
}

#[path = "key_lock_source_pitch_probe.rs"]
mod pitch_probe;

#[path = "key_lock_source_causal_probe.rs"]
mod causal_probe;
