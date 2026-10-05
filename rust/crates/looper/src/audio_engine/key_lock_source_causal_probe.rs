//! Test-only causal gating of a matched continuous native musical reference.
//!
//! C is the API nominal delay used as an illustrative anchor, not a newly accepted musical
//! compensation. U keeps the original isolated peak/99.9%-energy rule. Actual nonzero mixtures
//! supply finite-window content accounting, never a target-response retention denominator.
//! The earlier failed timing gate is unchanged. No bridge, fitted translation, live scheduling,
//! device timing, click safety, pitch acceptance or listening acceptance is established here.

use super::*;

const CONTENT_WINDOW_MS: f64 = 40.0;
const ILLUSTRATIVE_TARGET: i64 = 100_000;

/// One raw native timeline: t = T + (n - H - C). Boundaries include raw capture end so
/// half-open intervals can be checked without converting an invalid signed index to usize.
#[derive(Clone, Copy)]
struct Coordinates {
    history: i64,
    target: i64,
    anchor: i64,
    raw_frames: usize,
}

impl Coordinates {
    fn raw_index(self, time: i64) -> Option<usize> {
        let index = self
            .history
            .checked_add(self.anchor)?
            .checked_add(time.checked_sub(self.target)?)?;
        usize::try_from(index)
            .ok()
            .filter(|index| *index <= self.raw_frames)
    }

    fn raw_time(self, index: usize) -> Option<i64> {
        if index > self.raw_frames {
            return None;
        }
        self.target
            .checked_add(i64::try_from(index).ok()?.checked_sub(self.history)?)?
            .checked_sub(self.anchor)
    }

    /// Canonical source progress uses H+(t-T), independently of raw native displacement C.
    fn logical_index(self, time: i64) -> Option<usize> {
        usize::try_from(self.history.checked_add(time.checked_sub(self.target)?)?).ok()
    }
}

#[derive(Clone, Copy)]
enum Policy {
    StrictTarget,
    HypotheticalEarlyRetention,
}

impl Policy {
    fn name(self) -> &'static str {
        match self {
            Self::StrictTarget => "strict_target_gate",
            Self::HypotheticalEarlyRetention => "hypothetical_early_retention_gate",
        }
    }

    fn start(self, earliest: i64, coordinates: Coordinates, upper: i64) -> Option<i64> {
        let advance = match self {
            Self::StrictTarget => 0,
            Self::HypotheticalEarlyRetention => coordinates.anchor.checked_sub(upper)?.max(0),
        };
        Some(earliest.max(coordinates.target.checked_sub(advance)?))
    }
}

struct IsolatedBound {
    upper: i64,
    original_peak: usize,
    energy_discard: usize,
    total_energy: f64,
    energy: Vec<f64>,
}

/// Reuse exactly the original helper criterion; silence has no target retention denominator.
fn isolated_bound(history: usize, raw: &[Vec<f32>]) -> Option<IsolatedBound> {
    let energy = stereo_energy(raw);
    let total_energy = energy.iter().sum::<f64>();
    if !total_energy.is_finite() || total_energy <= 0.0 {
        return None;
    }
    let original_peak = stereo_response(raw).peak_frame;
    let energy_discard = maximum_energy_discard(&energy);
    Some(IsolatedBound {
        upper: i64::try_from(original_peak.min(energy_discard))
            .ok()?
            .checked_sub(i64::try_from(history).ok()?)?,
        original_peak,
        energy_discard,
        total_energy,
        energy,
    })
}

#[derive(Clone, Copy)]
struct ContentInterval {
    start: i64,
    end: i64,
    energy: f64,
    left_sum: f64,
    right_sum: f64,
}

fn content_interval(
    raw: &[Vec<f32>],
    coordinates: Coordinates,
    start: i64,
    end: i64,
    window_start: i64,
    window_end: i64,
) -> ContentInterval {
    assert!(start <= end && window_start < window_end);
    let start = start.clamp(window_start, window_end);
    let end = end.clamp(window_start, window_end);
    let first = coordinates.raw_index(start).unwrap();
    let last = coordinates.raw_index(end).unwrap();
    let channel_sum = |channel: usize| {
        raw[channel][first..last]
            .iter()
            .map(|value| f64::from(*value))
            .sum()
    };
    let energy = (first..last)
        .map(|frame| f64::from(raw[0][frame]).powi(2) + f64::from(raw[1][frame]).powi(2))
        .sum();
    ContentInterval {
        start,
        end,
        energy,
        left_sum: channel_sum(0),
        right_sum: channel_sum(1),
    }
}

#[derive(Default)]
struct CausalRow {
    fields: Vec<(String, String)>,
}

impl CausalRow {
    fn add(&mut self, name: &str, value: impl std::fmt::Display) {
        self.fields.push((name.to_owned(), value.to_string()));
    }

    fn interval(&mut self, name: &str, interval: ContentInterval, target: i64) {
        self.add(&format!("{name}_start_relative_T"), interval.start - target);
        self.add(&format!("{name}_end_relative_T"), interval.end - target);
        self.add(
            &format!("{name}_frame_count"),
            interval.end - interval.start,
        );
        self.add(&format!("{name}_stereo_energy"), interval.energy);
        self.add(&format!("{name}_signed_left_sum"), interval.left_sum);
        self.add(&format!("{name}_signed_right_sum"), interval.right_sum);
    }
}

#[derive(Default)]
struct CausalCsv {
    header: Option<Vec<String>>,
    rows: String,
    count: usize,
}

impl CausalCsv {
    fn push(&mut self, row: CausalRow) {
        let names = row.fields.iter().map(|(name, _)| name.clone()).collect();
        if let Some(header) = &self.header {
            assert_eq!(header, &names);
        } else {
            self.header = Some(names);
        }
        let values = row
            .fields
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        assert!(
            values
                .iter()
                .all(|value| !value.contains([',', '\n', '\r']))
        );
        writeln!(self.rows, "{}", values.join(",")).unwrap();
        self.count += 1;
    }

    fn save_if_requested(self) {
        if let Some(path) = std::env::var_os("FLITZIS_KEY_LOCK_CAUSAL_PROBE_CSV") {
            let path = std::path::PathBuf::from(path);
            assert!(path.is_absolute(), "causal probe CSV path must be absolute");
            let header = self.header.unwrap().join(",");
            std::fs::write(path, format!("{header}\n{}", self.rows)).unwrap();
        }
    }
}

fn position_fields(row: &mut CausalRow, name: &str, fixture: &Fixture, logical: usize) {
    let (frame, fraction, _) = fixture.reference_position(logical);
    fixture.assert_position(fixture.request(logical, 0).logical.position(), logical);
    row.add(&format!("{name}_output_coordinate"), logical);
    row.add(&format!("{name}_source_frame"), frame);
    row.add(&format!("{name}_source_fraction"), fraction);
}

/// Actual generated suffix, checked against the same immutable fixture/reference twice. H at
/// launch changes to H+(start-T), preserving the original forward origin and source catch-up.
/// The raw discard is H+C+(start-T); native feed and logical progress remain separate.
fn prove_gate_suffix(
    fixture: &Fixture,
    reference: &NativeReference,
    coordinates: Coordinates,
    start: i64,
    window_end: i64,
) -> Vec<Vec<f32>> {
    let logical = coordinates.logical_index(start).unwrap();
    let discard = coordinates.raw_index(start).unwrap();
    let frames = usize::try_from(window_end - start).unwrap();
    let expected = suffix(&reference.output, discard, frames);
    let mut generated = None;
    for (_, pattern) in [PATTERNS[2], PATTERNS[3]] {
        let actual = history_continuation(fixture, logical, discard, reference, pattern, frames);
        assert_eq!(
            actual, expected,
            "causal gate changed the matched native suffix"
        );
        generated = Some(actual);
    }
    generated.unwrap()
}

fn close_sum(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= 1.0e-10 * expected.abs().max(1.0));
}

fn finite_energy_fraction(numerator: f64, denominator: f64) -> Option<f64> {
    (numerator.is_finite() && numerator >= 0.0 && denominator.is_finite() && denominator > 0.0)
        .then(|| numerator / denominator)
}

#[test]
fn causal_native_gates_preserve_matched_mixture_suffix_and_expose_content_cost() {
    let mut csv = CausalCsv::default();
    let mut old_gate_failures = 0;
    let mut exact_suffix_checks = 0;
    let mut pre_target_emissions = 0;
    let selected_attacks = [attacks()[2], attacks()[3]];
    for rate in RATES {
        let half_window = (f64::from(rate) * CONTENT_WINDOW_MS / 1000.0).round() as i64;
        let window_start = ILLUSTRATIVE_TARGET - half_window;
        let window_end = ILLUSTRATIVE_TARGET + half_window;
        // Four fixed input/control situations, not a fitted offset grid. At 120 BPM a 1/64
        // spacing is 31.25 ms; a nearest future target is at most 15.625 ms away. Rounding this
        // illustrative half-step to an output sample is explicit in the exported E coordinate.
        let schedules = [
            ("future_40ms_headroom", -half_window),
            (
                "future_nearest_1_64_at_120bpm",
                -(f64::from(rate) * 0.015_625).round() as i64,
            ),
            ("exact_target", 0),
            ("late_5ms", (f64::from(rate) * 0.005).round() as i64),
        ];
        for ratio in ONSET_RATIOS {
            for history in LONG_HISTORIES {
                for attack in selected_attacks {
                    let case = OnsetCase {
                        rate,
                        ratio,
                        history,
                        marker: 0,
                        attack,
                        background: true,
                    };
                    // 250 ms after H covers the full isolated attack/tail and every window.
                    let capture = history + rate as usize / 4;
                    let isolated_case = OnsetCase {
                        background: false,
                        ..case
                    };
                    let isolated_fixture = isolated_case.fixture(capture, true);
                    let isolated = native_reference(&isolated_fixture, 0, capture);
                    let bound = isolated_bound(history, &isolated.output).unwrap();
                    let old = RawOnset::new(
                        isolated_case,
                        &dry_reference(&isolated_fixture, isolated.output[0].len()),
                        &isolated.output,
                        isolated.delay,
                    );
                    let old_pass = old.residuals[..2]
                        .iter()
                        .all(|value| value.abs() <= timing_budget(rate))
                        && old.retention_pass()
                        && old.tail_pass();
                    old_gate_failures += usize::from(!old_pass);
                    assert_eq!(old.retention_upper, bound.upper);
                    let fixture = case.fixture(capture, true);
                    let reference = native_reference(&fixture, 0, capture);
                    assert_eq!(reference.delay, isolated.delay);
                    assert_eq!(reference.block_size, isolated.block_size);
                    let coordinates = Coordinates {
                        history: history as i64,
                        target: ILLUSTRATIVE_TARGET,
                        anchor: reference.delay as i64,
                        raw_frames: reference.output[0].len(),
                    };
                    assert_eq!(
                        coordinates.raw_index(coordinates.target),
                        Some(history + reference.delay)
                    );
                    let raw_window = content_interval(
                        &reference.output,
                        coordinates,
                        window_start,
                        window_end,
                        window_start,
                        window_end,
                    );
                    assert!(raw_window.energy > 0.0);
                    for (situation, offset) in schedules {
                        let earliest = coordinates.target + offset;
                        for policy in [Policy::StrictTarget, Policy::HypotheticalEarlyRetention] {
                            let start = policy.start(earliest, coordinates, bound.upper).unwrap();
                            assert!(
                                start >= earliest && start >= window_start && start < window_end
                            );
                            let k = coordinates.anchor + start - coordinates.target;
                            let discard = coordinates.raw_index(start).unwrap();
                            assert_eq!(discard as i64, coordinates.history + k);
                            assert_eq!(coordinates.raw_time(discard), Some(start));
                            let lost_isolated =
                                bound.energy[..discard].iter().sum::<f64>() / bound.total_energy;
                            let peak_retained = discard <= bound.original_peak;
                            let original_retention =
                                peak_retained && lost_isolated <= MAX_DISCARDED_ENERGY;
                            assert_eq!(original_retention, k <= bound.upper);
                            if matches!(policy, Policy::HypotheticalEarlyRetention) {
                                assert_eq!(
                                    original_retention,
                                    earliest
                                        <= coordinates.target + bound.upper - coordinates.anchor
                                );
                            } else {
                                assert!(
                                    start >= coordinates.target,
                                    "strict gate emitted before T"
                                );
                            }
                            let actual = prove_gate_suffix(
                                &fixture,
                                &reference,
                                coordinates,
                                start,
                                window_end,
                            );
                            exact_suffix_checks += 2;
                            let prefix = usize::try_from(start - window_start).unwrap();
                            let visible = actual
                                .iter()
                                .map(|channel| {
                                    let mut samples = vec![0.0; prefix];
                                    samples.extend_from_slice(channel);
                                    samples
                                })
                                .collect::<Vec<_>>();
                            assert!(visible.iter().all(|channel| {
                                channel[..prefix].iter().all(|value| *value == 0.0)
                            }));
                            assert_eq!(visible[0].len(), (window_end - window_start) as usize);
                            let first_nonzero = (0..visible[0].len())
                                .find(|frame| {
                                    visible[0][*frame] != 0.0 || visible[1][*frame] != 0.0
                                })
                                .unwrap();
                            assert!(first_nonzero >= prefix);
                            if matches!(policy, Policy::StrictTarget) {
                                assert!(window_start + first_nonzero as i64 >= coordinates.target);
                                let pre_target = (coordinates.target - window_start) as usize;
                                assert!(visible.iter().all(|channel| {
                                    channel[..pre_target].iter().all(|value| *value == 0.0)
                                }));
                            }
                            pre_target_emissions += usize::from(start < coordinates.target);
                            // Disjoint omission categories. Late [T,E) is already unavailable;
                            // restricting the first category to pre-T prevents double counting.
                            let unavailable = content_interval(
                                &reference.output,
                                coordinates,
                                window_start,
                                earliest.min(coordinates.target),
                                window_start,
                                window_end,
                            );
                            let controllable_crop = content_interval(
                                &reference.output,
                                coordinates,
                                earliest.max(window_start).min(coordinates.target),
                                start.min(coordinates.target),
                                window_start,
                                window_end,
                            );
                            let late = content_interval(
                                &reference.output,
                                coordinates,
                                coordinates.target,
                                earliest.max(coordinates.target),
                                window_start,
                                window_end,
                            );
                            let omitted = content_interval(
                                &reference.output,
                                coordinates,
                                window_start,
                                start,
                                window_start,
                                window_end,
                            );
                            let emitted = content_interval(
                                &reference.output,
                                coordinates,
                                start,
                                window_end,
                                window_start,
                                window_end,
                            );
                            let emitted_pre_target = content_interval(
                                &reference.output,
                                coordinates,
                                start.min(coordinates.target),
                                coordinates.target,
                                window_start,
                                window_end,
                            );
                            assert_eq!(
                                unavailable.end - unavailable.start + controllable_crop.end
                                    - controllable_crop.start
                                    + late.end
                                    - late.start,
                                omitted.end - omitted.start
                            );
                            close_sum(
                                unavailable.energy + controllable_crop.energy + late.energy,
                                omitted.energy,
                            );
                            close_sum(
                                unavailable.left_sum + controllable_crop.left_sum + late.left_sum,
                                omitted.left_sum,
                            );
                            close_sum(
                                unavailable.right_sum
                                    + controllable_crop.right_sum
                                    + late.right_sum,
                                omitted.right_sum,
                            );
                            close_sum(omitted.energy + emitted.energy, raw_window.energy);
                            close_sum(stereo_energy(&visible).iter().sum(), emitted.energy);
                            let mut row = CausalRow::default();
                            row.add("sample_rate_hz", rate);
                            row.add("tempo_ratio", ratio);
                            row.add("history_frames", history);
                            row.add("marker", 0);
                            row.add("signal", attack.signal.name());
                            row.add("duration_ms", attack.duration_ms);
                            row.add("frequency_hz", attack.frequency_hz);
                            row.add("carrier_phase", attack.phase);
                            row.add("background", "periodic_stereo");
                            row.add("situation", situation);
                            row.add("policy", policy.name());
                            row.add("target_absolute_output_frame_T", coordinates.target);
                            row.add("earliest_relative_T", offset);
                            row.add(
                                "earliest_relation_to_T",
                                if offset < 0 {
                                    "future"
                                } else if offset == 0 {
                                    "exact"
                                } else {
                                    "late"
                                },
                            );
                            row.add("start_relative_T", start - coordinates.target);
                            row.add(
                                "start_relation_to_T",
                                if start < coordinates.target {
                                    "pre_target"
                                } else if start == coordinates.target {
                                    "at_target"
                                } else {
                                    "late"
                                },
                            );
                            row.add("anchor_C_api_nominal_frames", coordinates.anchor);
                            row.add("retention_upper_U_isolated_frames", bound.upper);
                            row.add(
                                "required_pre_target_frames",
                                (coordinates.anchor - bound.upper).max(0),
                            );
                            row.add("available_pre_target_frames", (-offset).max(0));
                            row.add("raw_cut_relative_H_k", k);
                            row.add("raw_first_emitted_index", discard);
                            row.add(
                                "raw_index_at_T",
                                coordinates.raw_index(coordinates.target).unwrap(),
                            );
                            row.add(
                                "first_nonzero_relative_T",
                                window_start + first_nonzero as i64 - coordinates.target,
                            );
                            row.add("original_isolated_peak_raw_index", bound.original_peak);
                            row.add(
                                "original_isolated_max_energy_discard_raw_index",
                                bound.energy_discard,
                            );
                            row.add("isolated_original_total_energy", bound.total_energy);
                            row.add("isolated_discarded_energy_fraction", lost_isolated);
                            row.add(
                                "isolated_original_peak_retained",
                                usize::from(peak_retained),
                            );
                            row.add(
                                "isolated_original_retention_pass",
                                usize::from(original_retention),
                            );
                            row.add("old_nominal_q10_residual", old.residuals[0]);
                            row.add("old_nominal_q50_residual", old.residuals[1]);
                            row.add("old_nominal_original_criterion_pass", usize::from(old_pass));
                            row.add("old_capture_tail_fraction", old.tail_fraction);
                            row.add("matched_suffix_exact_512", 1);
                            row.add("matched_suffix_exact_irregular", 1);
                            row.add("finite_mixture_energy_is_target_retention", 0);
                            row.add("listening_or_live_acceptance", 0);
                            position_fields(
                                &mut row,
                                "logical_start",
                                &fixture,
                                coordinates.logical_index(start).unwrap(),
                            );
                            position_fields(&mut row, "logical_target", &fixture, history);
                            position_fields(
                                &mut row,
                                "logical_window_end",
                                &fixture,
                                coordinates.logical_index(window_end).unwrap(),
                            );
                            row.add(
                                "canonical_source_delta_from_target_frames",
                                f64::from(ratio) * (start - coordinates.target) as f64,
                            );
                            row.interval("finite_window", raw_window, coordinates.target);
                            row.interval("unavailable_pre_target", unavailable, coordinates.target);
                            row.interval(
                                "controllable_pre_target_crop",
                                controllable_crop,
                                coordinates.target,
                            );
                            row.interval(
                                "authorized_late_missed_content",
                                late,
                                coordinates.target,
                            );
                            row.interval("total_omitted", omitted, coordinates.target);
                            row.interval("exact_emitted", emitted, coordinates.target);
                            row.interval(
                                "emitted_pre_target",
                                emitted_pre_target,
                                coordinates.target,
                            );
                            let fraction =
                                finite_energy_fraction(omitted.energy, raw_window.energy);
                            row.add(
                                "finite_mixture_ratio_defined",
                                usize::from(fraction.is_some()),
                            );
                            row.add(
                                "finite_mixture_omitted_energy_fraction",
                                fraction.map_or_else(String::new, |value| value.to_string()),
                            );
                            csv.push(row);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(csv.count, 288);
    assert_eq!(exact_suffix_checks, 576);
    assert!(old_gate_failures > 0, "retain old failure evidence");
    assert!(
        pre_target_emissions > 0,
        "hypothetical retention must expose pre-T sound"
    );
    csv.save_if_requested();
}

#[test]
fn causal_coordinates_and_original_retention_boundary_are_explicit() {
    let coordinates = Coordinates {
        history: 2048,
        target: 10_000,
        anchor: 1000,
        raw_frames: 8192,
    };
    let upper = 900;
    let target = coordinates.target;
    assert_eq!(
        Policy::StrictTarget.start(target - 200, coordinates, upper),
        Some(target)
    );
    assert_eq!(
        Policy::HypotheticalEarlyRetention.start(target - 200, coordinates, upper),
        Some(target - 100)
    );
    // Exactly sufficient headroom retains the original bound; one later controllable sample
    // cannot retain it. This is a causal boundary, not a fitted transient timing assertion.
    for (earliest, expected_k) in [
        (target - 101, 900),
        (target - 100, 900),
        (target - 99, 901),
        (target, 1000),
        (target + 5, 1005),
    ] {
        let start = Policy::HypotheticalEarlyRetention
            .start(earliest, coordinates, upper)
            .unwrap();
        let k = coordinates.anchor + start - target;
        assert_eq!(k, expected_k);
        assert_eq!(k <= upper, earliest <= target - 100);
        assert_eq!(
            coordinates.raw_time(coordinates.raw_index(start).unwrap()),
            Some(start)
        );
        assert_eq!(
            coordinates.logical_index(start),
            Some((2048 + start - target) as usize)
        );
    }
    assert_eq!(
        Policy::HypotheticalEarlyRetention.start(target - 200, coordinates, 1000),
        Some(target)
    );
    assert_eq!(
        Policy::HypotheticalEarlyRetention.start(target - 200, coordinates, 1100),
        Some(target)
    );
    let silent = vec![vec![0.0; 8192]; 2];
    assert!(isolated_bound(2048, &silent).is_none());
    let silence_interval = content_interval(
        &silent,
        coordinates,
        target - 100,
        target + 100,
        target - 100,
        target + 100,
    );
    assert_eq!(silence_interval.energy, 0.0);
    assert_eq!(silence_interval.left_sum, 0.0);
    assert_eq!(silence_interval.right_sum, 0.0);
    assert_eq!(finite_energy_fraction(0.0, silence_interval.energy), None);
    assert_eq!(finite_energy_fraction(0.0, 1.0), Some(0.0));
    assert_eq!(finite_energy_fraction(1.0, f64::NAN), None);
    // Exactly representable PCM: the small first sample has 2/20002 of total stereo
    // energy. D=1 retains the original peak and >99.9%; D=2 loses the peak and all energy.
    let known = vec![vec![1.0, 100.0]; 2];
    let bound = isolated_bound(0, &known).unwrap();
    assert_eq!(bound.original_peak, 1);
    assert_eq!(bound.energy_discard, 1);
    assert_eq!(bound.upper, 1);
    assert_eq!(bound.total_energy, 20_002.0);
    assert!(bound.energy[..1].iter().sum::<f64>() / bound.total_energy <= MAX_DISCARDED_ENERGY);
    assert!(bound.energy[..2].iter().sum::<f64>() / bound.total_energy > MAX_DISCARDED_ENERGY);
    let relative = coordinates.raw_time(0).unwrap();
    assert_eq!(coordinates.raw_index(relative), Some(0));
    assert_eq!(coordinates.raw_index(relative - 1), None);
    assert_eq!(coordinates.raw_time(coordinates.raw_frames + 1), None);
    assert_eq!(
        coordinates.raw_index(coordinates.raw_time(coordinates.raw_frames).unwrap()),
        Some(coordinates.raw_frames)
    );
    assert_eq!(
        coordinates.logical_index(target - coordinates.history - 1),
        None
    );
    assert_eq!(coordinates.raw_index(i64::MIN), None);
    assert_eq!(coordinates.raw_time(usize::MAX), None);
    let overflow = Coordinates {
        history: i64::MAX,
        ..coordinates
    };
    assert_eq!(overflow.raw_index(target), None);
    assert_eq!(
        Policy::HypotheticalEarlyRetention.start(target, coordinates, i64::MIN),
        None
    );
}
