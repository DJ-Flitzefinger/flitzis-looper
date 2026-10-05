//! Test-only unity-source launch bridge and matched continuous-mixture diagnostics.
//!
//! The bridge preserves source carrier pitch by reading one source frame per output frame.
//! Its source phase consequently differs from canonical tempo progression until the fixed
//! fade ends. Finite-window metrics describe added launch deformation, never attack recovery,
//! an additive native response, a revised acceptance budget, or live/device acceptance.

use super::*;

const PITCH_MARKERS: [usize; 3] = [0, 17, 511];
// A numerical denominator floor, not a perceptual threshold or musical acceptance rule.
const MEAN_STEREO_ENERGY_FLOOR: f64 = 1.0e-16;

fn bridge_frames(rate: u32) -> (usize, usize) {
    let hold = (f64::from(rate) * DRY_HOLD_MS / 1000.0).round() as usize;
    let fade = (f64::from(rate) * FADE_MS / 1000.0).round() as usize;
    (hold, hold + fade)
}

/// Independent source distance is H*r+n, rather than (H+n)*r. No production clock or
/// reader participates in these algebraic taps or expected fractional source positions.
fn unity_position(fixture: &Fixture, history: usize, frame: usize) -> FractionalSourcePosition {
    let distance = history as f64 * f64::from(fixture.ratio) + frame as f64;
    let (source_frame, seek_mode) = fixture.reference_integer_position(distance.floor() as usize);
    FractionalSourcePosition {
        frame: source_frame,
        fraction: distance.fract(),
        seek_mode,
    }
}

fn unity_reference(fixture: &Fixture, history: usize, frames: usize) -> Vec<Vec<f32>> {
    (0..2)
        .map(|channel| {
            (0..frames)
                .map(|frame| {
                    let distance = history as f64 * f64::from(fixture.ratio) + frame as f64;
                    let whole = distance.floor() as usize;
                    let fraction = distance.fract() as f32;
                    let left = fixture.reference_tap(whole, channel);
                    if fraction == 0.0 {
                        left
                    } else {
                        left + (fixture.reference_tap(whole + 1, channel) - left) * fraction
                    }
                })
                .collect()
        })
        .collect()
}

fn shared_unity_branch(fixture: &Fixture, history: usize, frames: usize) -> Vec<Vec<f32>> {
    let expected = unity_reference(fixture, history, frames);
    let request = fixture.request(history, 0);
    let logical_before = request.logical.position();
    let mut generated = None;
    for (_, pattern) in [PATTERNS[2], PATTERNS[3]] {
        let mut playback = request.logical.at_constant_ratio(1.0);
        let mut buffers = vec![vec![0.0; DEFAULT_BLOCK_SAMPLES]; 2];
        let mut output = vec![Vec::with_capacity(frames), Vec::with_capacity(frames)];
        let mut elapsed = 0;
        let mut partition = 0;
        while elapsed < frames {
            let count = pattern[partition % pattern.len()].min(frames - elapsed);
            assert_eq!(
                playback.position(),
                unity_position(fixture, history, elapsed)
            );
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
        }
        assert_eq!(
            playback.position(),
            unity_position(fixture, history, frames)
        );
        assert_eq!(
            output, expected,
            "unity reader differs from independent taps"
        );
        assert_eq!(request.logical.position(), logical_before);
        generated = Some(output);
    }
    generated.unwrap()
}

fn matched_native_suffix(
    fixture: &Fixture,
    history: usize,
    reference: &NativeReference,
    frames: usize,
) -> Vec<Vec<f32>> {
    let discard = history + reference.delay;
    let expected = suffix(&reference.output, discard, frames);
    let mut generated = None;
    for (_, pattern) in [PATTERNS[2], PATTERNS[3]] {
        let actual = history_continuation(fixture, history, discard, reference, pattern, frames);
        assert_eq!(
            actual, expected,
            "native history/source/block-phase suffix mismatch"
        );
        generated = Some(actual);
    }
    generated.unwrap()
}

fn total_energy(channels: &[Vec<f32>]) -> f64 {
    stereo_energy(channels).iter().sum()
}

fn defined_energy(energy: f64, frames: usize) -> bool {
    energy.is_finite() && energy > frames as f64 * MEAN_STEREO_ENERGY_FLOOR
}

/// Same centered zero-extended box convention as the earlier 0.5-ms envelope helper.
fn envelope_at_scale(energy: &[f64], rate: u32, width_ms: f64) -> Vec<f64> {
    let half = (f64::from(rate) * width_ms / 2000.0).round() as usize;
    let mut prefix = Vec::with_capacity(energy.len() + 1);
    prefix.push(0.0);
    for value in energy {
        prefix.push(prefix.last().unwrap() + value);
    }
    (0..energy.len() + 2 * half)
        .map(|frame| {
            let start = frame.saturating_sub(2 * half).min(energy.len());
            let end = (frame + 1).min(energy.len());
            (prefix[end] - prefix[start]) / (2 * half + 1) as f64
        })
        .collect()
}

fn optional(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
}

const WINDOW_FIELDS: [&str; 18] = [
    "start_frame",
    "frame_count",
    "native_defined",
    "candidate_defined",
    "dry_defined",
    "native_energy",
    "candidate_energy",
    "dry_energy",
    "candidate_energy_ratio",
    "envelope_rel_l1_0_5ms",
    "envelope_rel_l1_5ms",
    "pcm_rel_l2",
    "candidate_q10_delta_native",
    "candidate_q50_delta_native",
    "candidate_q10_delta_dry",
    "candidate_q50_delta_dry",
    "native_q10_delta_dry",
    "native_q50_delta_dry",
];

/// All quantiles are explicitly finite-window diagnostics with their own normalization.
/// Actual stereo mixtures are compared directly; no attack-minus-background oracle is used.
fn window_metrics(
    native: &[Vec<f32>],
    candidate: &[Vec<f32>],
    dry: &[Vec<f32>],
    start: usize,
    frames: usize,
    rate: u32,
) -> Vec<String> {
    let native = suffix(native, start, frames);
    let candidate = suffix(candidate, start, frames);
    let dry = suffix(dry, start, frames);
    let energies = [&native, &candidate, &dry].map(|audio| stereo_energy(audio));
    let totals = energies.each_ref().map(|energy| energy.iter().sum::<f64>());
    let defined = totals.map(|energy| defined_energy(energy, frames));
    let quantiles: [Option<[i64; 3]>; 3] =
        std::array::from_fn(|index| defined[index].then(|| energy_times(&energies[index], rate)));
    let mut values = vec![
        start.to_string(),
        frames.to_string(),
        usize::from(defined[0]).to_string(),
        usize::from(defined[1]).to_string(),
        usize::from(defined[2]).to_string(),
    ];
    values.extend(totals.map(|energy| energy.to_string()));
    values.push(optional(defined[0].then(|| totals[1] / totals[0])));
    for width in [0.5, 5.0] {
        values.push(optional(defined[0].then(|| {
            let native_envelope = envelope_at_scale(&energies[0], rate, width);
            let candidate_envelope = envelope_at_scale(&energies[1], rate, width);
            native_envelope
                .iter()
                .zip(candidate_envelope)
                .map(|(native, candidate)| (candidate - native).abs())
                .sum::<f64>()
                / totals[0]
        })));
    }
    values.push(optional(defined[0].then(|| {
        (total_energy(&difference(&candidate, &native)) / totals[0]).sqrt()
    })));
    for (left, right) in [(1, 0), (1, 2), (0, 2)] {
        for index in [0, 1] {
            values.push(optional(
                quantiles[left]
                    .zip(quantiles[right])
                    .map(|(left, right)| (left[index] - right[index]) as f64),
            ));
        }
    }
    assert_eq!(values.len(), WINDOW_FIELDS.len());
    values
}

/// Signed cross energy exposes coherent cancellation; separate branch energies do not
/// demonstrate restoration of discarded native content. The final entry is f32 roundoff.
fn bridge_component_energies(
    unity: &[Vec<f32>],
    native: &[Vec<f32>],
    candidate: &[Vec<f32>],
    frames: usize,
    rate: u32,
) -> [f64; 4] {
    let mut unity_energy = 0.0;
    let mut native_energy = 0.0;
    let mut cross = 0.0;
    for frame in 0..frames {
        let weight = f64::from(dry_weight(frame, rate));
        for channel in 0..2 {
            let unity = weight * f64::from(unity[channel][frame]);
            let native = (1.0 - weight) * f64::from(native[channel][frame]);
            unity_energy += unity * unity;
            native_energy += native * native;
            cross += 2.0 * unity * native;
        }
    }
    let actual = total_energy(&suffix(candidate, 0, frames));
    [
        unity_energy,
        native_energy,
        cross,
        actual - unity_energy - native_energy - cross,
    ]
}

const BASE_FIELDS: [&str; 32] = [
    "rate",
    "ratio",
    "history",
    "marker",
    "signal",
    "duration_ms",
    "frequency_hz",
    "carrier_phase",
    "source_attack_frame",
    "target_output_frame",
    "target_outside_bridge",
    "nominal_delay",
    "hold_frames",
    "bridge_end_frame",
    "source_deviation_hold",
    "source_deviation_end",
    "output_equivalent_deviation_hold",
    "output_equivalent_deviation_end",
    "exact_shared_unity_512_irregular",
    "exact_native_suffix_512_irregular",
    "unchanged_wet_suffix",
    "launch7ms_weighted_unity_energy",
    "launch7ms_weighted_native_energy",
    "launch7ms_cross_energy",
    "launch7ms_energy_balance_roundoff",
    "window_mean_energy_floor",
    "diagnostic_only",
    "native_prelaunch_start_frame",
    "native_prelaunch_frame_count",
    "native_prelaunch_energy",
    "native_prelaunch_peak",
    "native_prelaunch_context_only",
];

#[test]
fn unity_source_launch_reports_target_local_matched_continuous_mixtures() {
    let mut header = BASE_FIELDS
        .iter()
        .map(|field| (*field).to_string())
        .collect::<Vec<_>>();
    for prefix in ["launch7ms", "launch40ms", "target7ms", "target40ms"] {
        header.extend(WINDOW_FIELDS.map(|field| format!("{prefix}_{field}")));
    }
    let mut csv = format!("{}\n", header.join(","));
    let mut fixtures = 0;
    let mut changed_launches = 0;
    for rate in RATES {
        for ratio in ONSET_RATIOS {
            for history in LONG_HISTORIES {
                for marker in PITCH_MARKERS {
                    for attack in attacks() {
                        let case = OnsetCase {
                            rate,
                            ratio,
                            history,
                            marker,
                            attack,
                            background: true,
                        };
                        let frames = marker + rate as usize / 4;
                        let fixture = case.fixture(history + frames, true);
                        let reference = native_reference(&fixture, 0, history + frames);
                        assert_eq!(history % reference.block_size, 0);
                        let native = matched_native_suffix(&fixture, history, &reference, frames);
                        let unity = shared_unity_branch(&fixture, history, frames);
                        let candidate = launch_bridge(&unity, &native, rate);
                        let dry =
                            suffix(&dry_reference(&fixture, history + frames), history, frames);
                        let (hold, end) = bridge_frames(rate);
                        for channel in 0..2 {
                            assert_eq!(candidate[channel][end..], native[channel][end..]);
                        }
                        changed_launches += usize::from(candidate != native);
                        // Known source-coordinate event crossing, not a detected waveform onset.
                        let target = (case.source_attack() as f64
                            - history as f64 * f64::from(ratio))
                            / f64::from(ratio);
                        assert!(target >= 0.0);
                        let deviation = |frame: usize| (1.0 - f64::from(ratio)) * frame as f64;
                        let mut row = vec![
                            rate.to_string(),
                            ratio.to_string(),
                            history.to_string(),
                            marker.to_string(),
                            attack.signal.name().to_string(),
                            attack.duration_ms.to_string(),
                            attack.frequency_hz.to_string(),
                            attack.phase.to_string(),
                            case.source_attack().to_string(),
                            target.to_string(),
                            usize::from(target >= end as f64).to_string(),
                            reference.delay.to_string(),
                            hold.to_string(),
                            end.to_string(),
                            deviation(hold).to_string(),
                            deviation(end).to_string(),
                            (deviation(hold) / f64::from(ratio)).to_string(),
                            (deviation(end) / f64::from(ratio)).to_string(),
                            "1".into(),
                            "1".into(),
                            "1".into(),
                        ];
                        row.extend(
                            bridge_component_energies(&unity, &native, &candidate, end, rate)
                                .map(|energy| energy.to_string()),
                        );
                        row.extend([MEAN_STEREO_ENERGY_FLOOR.to_string(), "1".into()]);
                        // The signed [-40ms,0) window belongs to the matched raw continuous
                        // mixture. It includes background and is not isolated attack loss or
                        // a replacement for the unchanged uncropped retention criterion.
                        let prelaunch_frames = (f64::from(rate) * 0.040).round() as usize;
                        let prelaunch = suffix(
                            &reference.output,
                            history + reference.delay - prelaunch_frames,
                            prelaunch_frames,
                        );
                        row.extend([
                            (-(prelaunch_frames as i64)).to_string(),
                            prelaunch_frames.to_string(),
                            total_energy(&prelaunch).to_string(),
                            stereo_peak(&prelaunch).1.to_string(),
                            "1".into(),
                        ]);
                        for (start, count) in [
                            (0, end),
                            (0, (f64::from(rate) * 0.040).round() as usize),
                            (target.floor() as usize, end),
                            (
                                target.floor() as usize,
                                (f64::from(rate) * 0.040).round() as usize,
                            ),
                        ] {
                            row.extend(window_metrics(
                                &native, &candidate, &dry, start, count, rate,
                            ));
                        }
                        assert_eq!(row.len(), header.len());
                        csv.push_str(&row.join(","));
                        csv.push('\n');
                        fixtures += 1;
                    }
                }
            }
        }
    }
    assert_eq!(fixtures, 324);
    assert!(
        changed_launches > 0,
        "candidate differences must stay visible"
    );
    if let Some(path) = std::env::var_os("FLITZIS_KEY_LOCK_PITCH_PROBE_CSV") {
        let path = std::path::PathBuf::from(path);
        assert!(path.is_absolute(), "pitch probe CSV path must be absolute");
        std::fs::write(path, csv).unwrap();
    }
}

#[test]
fn unity_source_reader_preserves_fractional_loop_seek_and_stem_addresses() {
    let mut fractional = 0;
    for ratio in RATIOS {
        for mode in [
            ExplicitSeekMode::Normal,
            ExplicitSeekMode::BeforeLoop,
            ExplicitSeekMode::AfterLoop,
        ] {
            for mask in [None, Some(0b1010), Some(0b1111)] {
                let fixture = loop_fixture(48_000, ratio, mode, mask);
                for history in [1, 17, 511] {
                    let branch = shared_unity_branch(&fixture, history, 2048);
                    assert_eq!(branch, unity_reference(&fixture, history, 2048));
                    fractional += usize::from(unity_position(&fixture, history, 0).fraction != 0.0);
                }
            }
        }
    }
    assert!(fractional > 0);
}

#[test]
fn unity_window_diagnostics_keep_silence_undefined_and_cancellation_visible() {
    let silence = vec![vec![0.0; 64]; 2];
    let values = window_metrics(&silence, &silence, &silence, 0, 64, 48_000);
    assert_eq!(&values[2..5], &["0", "0", "0"]);
    assert!(values[8..].iter().all(String::is_empty));
    let native = vec![vec![1.0; 400]; 2];
    let unity = vec![vec![-1.0; 400]; 2];
    let candidate = launch_bridge(&unity, &native, 48_000);
    let (_, end) = bridge_frames(48_000);
    let components = bridge_component_energies(&unity, &native, &candidate, end, 48_000);
    assert!(components[2] < 0.0);
    assert!(components[3].abs() < 1.0e-4);
    assert_ne!(candidate, native);
    let diagnostic = window_metrics(&native, &candidate, &native, 0, end, 48_000);
    assert!(diagnostic[9].parse::<f64>().unwrap() > 0.0);
}
