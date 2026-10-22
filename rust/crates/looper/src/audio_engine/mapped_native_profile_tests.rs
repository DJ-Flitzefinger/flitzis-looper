// Test-only comparison of existing Native options, through the same genuine owner.
// Acoustic negatives are measurements; PCM/protocol/custody assertions are checks.

use super::*;
use std::ops::Range;

#[derive(Default)]
struct Counts {
    declared: usize,
    available: usize,
    eligible: usize,
    passed: usize,
}

impl Counts {
    fn record(&mut self, available: bool, eligible: bool, passed: bool) {
        assert!(!eligible || available);
        assert!(!passed || eligible);
        self.declared += 1;
        self.available += usize::from(available);
        self.eligible += usize::from(eligible);
        self.passed += usize::from(passed);
    }

    fn value(&self) -> Value {
        json!({"declared":self.declared,"available":self.available,
            "unavailable":self.declared-self.available,"eligible":self.eligible,
            "ineligible":self.available-self.eligible,"passed":self.passed,
            "available_failed":self.available-self.passed})
    }
}

#[derive(Default)]
struct Results {
    natural_isolated: Counts,
    nominal_full_raw_timing: Counts,
    first_sound_retention: Counts,
    nominal_end_window: Availability,
    natural_tone_8192: Counts,
    nominal_tone_8192: Counts,
    supplemental_natural_tone_24576: Counts,
    supplemental_nominal_tone_24576: Counts,
}

#[derive(Default)]
struct Availability {
    declared: usize,
    available: usize,
}

impl Availability {
    fn record(&mut self, available: bool) {
        self.declared += 1;
        self.available += usize::from(available);
    }

    fn value(&self) -> Value {
        json!({"declared":self.declared,"available":self.available,
            "unavailable":self.declared-self.available,"combined_acoustic_pass":null})
    }
}

fn profile_emit(value: Value) {
    println!("MAPPED_NATIVE_WINDOW_PROFILES {value}");
}

fn signed_raw_time(raw_index: usize, raw_cut: usize, compensation: usize) -> i64 {
    i64::try_from(raw_index).unwrap()
        - i64::try_from(raw_cut).unwrap()
        - i64::try_from(compensation).unwrap()
}

fn complete_window(first: i64, last: i64, captured: usize) -> Option<Range<usize>> {
    if first < 0 || last <= first {
        return None;
    }
    let first = usize::try_from(first).ok()?;
    let last = usize::try_from(last).ok()?;
    (last <= captured).then_some(first..last)
}

fn nominal_window(capture: &Capture, first: usize, last: usize) -> Option<Range<usize>> {
    let origin = capture.raw_cut + capture.native_delay + BLOCK - 1;
    complete_window(
        i64::try_from(first.checked_add(origin)?).ok()?,
        i64::try_from(last.checked_add(origin)?).ok()?,
        capture.raw[0].len(),
    )
}

fn isolated_profile_metrics(label: &str, marker: usize, capture: &Capture, results: &mut Results) {
    // Preserve the old natural calculation and its full diagnostic row verbatim.
    isolated_metrics(label, marker, capture);
    let energy = stereo_energy(&capture.output);
    let dry_energy = stereo_energy(&capture.dry);
    let raw_energy = stereo_energy(&capture.raw);
    let total = energy.iter().sum::<f64>();
    let dry_total = dry_energy.iter().sum::<f64>();
    let raw_total = raw_energy.iter().sum::<f64>();
    if total <= 0.0 || dry_total <= 0.0 || raw_total <= 0.0 {
        results.natural_isolated.record(true, false, false);
        results.nominal_full_raw_timing.record(true, false, false);
        results.first_sound_retention.record(true, false, false);
        results.nominal_end_window.record(false);
        profile_emit(json!({"stage":"isolated_profile","label":label,
            "missing_response":true,"nominal_combined_acoustic_pass":null,
            "public_or_human_acceptance":false}));
        return;
    }
    let compensation = capture.native_delay + BLOCK - 1;
    let dry_q = energy_times(&dry_energy, RATE);
    let natural_q = energy_times(&energy, RATE);
    let raw_q = energy_times(&raw_energy, RATE);
    let natural_errors = std::array::from_fn::<_, 3, _>(|i| natural_q[i] - dry_q[i]);
    let full_raw_natural_errors =
        std::array::from_fn::<_, 3, _>(|i| raw_q[i] - capture.raw_cut as i64 - dry_q[i]);
    let nominal_q = raw_q.map(|frame| frame - capture.raw_cut as i64 - compensation as i64);
    let nominal_errors = std::array::from_fn::<_, 3, _>(|i| nominal_q[i] - dry_q[i]);
    let budget = (f64::from(RATE) * TIMING_BUDGET_MS / 1000.0).floor() as i64;
    assert_eq!(budget, 96);
    let natural_timing = natural_errors[..2]
        .iter()
        .all(|error| error.abs() <= budget);
    let nominal_timing = nominal_errors[..2]
        .iter()
        .all(|error| error.abs() <= budget);
    let timing_lower = full_raw_natural_errors[0].max(full_raw_natural_errors[1]) - budget;
    let timing_upper = full_raw_natural_errors[0].min(full_raw_natural_errors[1]) + budget;
    let nominal_interval = [
        timing_lower - compensation as i64,
        timing_upper - compensation as i64,
    ];
    assert_eq!(
        timing_lower <= timing_upper,
        nominal_interval[0] <= nominal_interval[1]
    );
    let (raw_peak_frame, raw_peak) = stereo_peak(&capture.raw);
    let (peak_frame, peak) = stereo_peak(&capture.output);
    let retained_peak = (capture.raw_cut..capture.raw_cut + OUTPUT).contains(&raw_peak_frame)
        && peak_frame + capture.raw_cut == raw_peak_frame
        && (peak - raw_peak).abs() <= 3.0e-6;
    let discarded = raw_energy[..capture.raw_cut].iter().sum::<f64>() / raw_total;
    let tail = energy[OUTPUT - RATE as usize / 50..].iter().sum::<f64>() / total;
    let extra_suffix = raw_energy[capture.raw_cut + OUTPUT..].iter().sum::<f64>() / raw_total;
    let complete = tail <= MAX_CAPTURE_TAIL_ENERGY && extra_suffix <= MAX_CAPTURE_TAIL_ENERGY;
    let natural_pass =
        complete && natural_timing && retained_peak && discarded <= MAX_DISCARDED_ENERGY;

    // The proposed FirstSound gate is measured against ALL original raw response,
    // never against a newly cropped response normalized to its surviving energy.
    let gate = capture.raw_cut + compensation;
    let gate_available = gate <= raw_energy.len();
    let gated_discarded = raw_energy
        .get(..gate)
        .map(|prefix| prefix.iter().sum::<f64>() / raw_total);
    let gated_peak_retained = gate_available && raw_peak_frame >= gate;
    let retention_pass = gated_peak_retained
        && gated_discarded.is_some_and(|fraction| fraction <= MAX_DISCARDED_ENERGY);
    let retention_u =
        raw_peak_frame.min(maximum_energy_discard(&raw_energy)) as i64 - capture.raw_cut as i64;
    let necessary_earlier_s = capture.target as i64 + retention_u - compensation as i64;
    let nominal_tail_window = nominal_window(capture, OUTPUT - RATE as usize / 50, OUTPUT);
    let nominal_tail = nominal_tail_window
        .as_ref()
        .map(|window| raw_energy[window.clone()].iter().sum::<f64>() / raw_total);
    let nominal_extra_suffix = nominal_tail_window
        .as_ref()
        .map(|window| raw_energy[window.end..].iter().sum::<f64>() / raw_total);
    results.natural_isolated.record(true, true, natural_pass);
    results
        .nominal_full_raw_timing
        .record(true, true, nominal_timing);
    results
        .first_sound_retention
        .record(gate_available, gate_available, retention_pass);
    results
        .nominal_end_window
        .record(nominal_tail_window.is_some());
    let mut row = json!({"stage":"isolated_profile","label":label,"profile":capture.profile.label(),
        "declared_dry_event_frame":marker,"target":capture.target,"N":capture.native_delay,
        "Hmin":capture.minimum_history,"Cnom":compensation,"D":capture.raw_cut,
        "full_raw_frames":capture.raw[0].len(),"full_raw_energy":raw_total,
        "natural_emitted_frames":OUTPUT,"natural_emitted_energy":total,
        "dry_quantiles":dry_q,"natural_emitted_quantiles":natural_q,"full_raw_quantiles":raw_q,
        "natural_emitted_errors_q10_q50_q90":natural_errors,
        "full_raw_natural_errors_q10_q50_q90":full_raw_natural_errors,
        "nominal_full_raw_quantiles_including_negative_time":nominal_q,
        "nominal_full_raw_errors_q10_q50_q90":nominal_errors,
        "natural_discarded_original_raw_energy_fraction":discarded,
        "natural_and_full_raw_quantiles_use_distinct_denominators":true,
        "natural_timing_q10_q50_pass":natural_timing,"natural_original_peak_retained":retained_peak,
        "natural_last_20ms_fraction":tail,"natural_extra_raw_suffix_fraction":extra_suffix,
        "natural_bounded_response_complete":complete,"natural_combined_acoustic_pass":natural_pass,
        "full_raw_natural_c_interval":[timing_lower,timing_upper],
        "nominal_full_raw_timing_error_interval":nominal_interval,
        "shared_timing_interval_exists":timing_lower<=timing_upper,
        "constant_shift_cannot_repair_empty_interval":true,"nominal_timing_q10_q50_pass":nominal_timing,
        "original_raw_peak_frame":raw_peak_frame,"original_raw_peak":raw_peak,
        "original_raw_peak_nominal_time":signed_raw_time(raw_peak_frame,capture.raw_cut,compensation)});
    let Value::Object(retention_and_bounds) = json!({
        "first_sound_gate_raw_frame":gate,"first_sound_gate_available":gate_available,
        "first_sound_discarded_full_raw_energy_fraction":gated_discarded,
        "first_sound_original_peak_retained":gated_peak_retained,"first_sound_retention_pass":retention_pass,
        "retention_u":retention_u,"necessary_c_le_u":(compensation as i64)<=retention_u,
        "attack_at_grid_required_s_upper_bound":necessary_earlier_s,
        "attack_at_grid_requirement":"S <= T + U - Cnom and independently proven controllable Emin <= S",
        "actual_s":null,"actual_e":null,"product_token":null,
        "nominal_raw_bounds_relative_to_target":[signed_raw_time(0,capture.raw_cut,compensation),
            signed_raw_time(capture.raw[0].len(),capture.raw_cut,compensation)],
        "nominal_fixed_end_window":[OUTPUT-RATE as usize/50,OUTPUT],
        "nominal_fixed_end_window_available":nominal_tail_window.is_some(),
        "nominal_fixed_end_window_full_raw_energy_fraction":nominal_tail,
        "nominal_extra_raw_suffix_full_raw_energy_fraction":nominal_extra_suffix,
        "nominal_combined_acoustic_pass":null,
        "nominal_scope":"complete signed raw timing and necessary retention diagnostics; no fabricated OUTPUT capture",
        "timing_budget_frames":budget,"discard_budget":MAX_DISCARDED_ENERGY,
        "tail_budget":MAX_CAPTURE_TAIL_ENERGY,"fitted_delay_gain_crop":false,
        "internal_native_tail_exhaustion_proven":false,"public_or_human_acceptance":false})
    else {
        unreachable!("declared metric object")
    };
    row.as_object_mut().unwrap().extend(retention_and_bounds);
    profile_emit(row);
}

struct ToneWindow {
    segment: usize,
    range: Range<usize>,
    family: &'static str,
    nominal: bool,
}

fn tone_window_metrics(
    label: &str,
    shift: i32,
    capture: &Capture,
    declared: ToneWindow,
    counts: &mut Counts,
) {
    let ToneWindow {
        segment,
        range: window,
        family,
        nominal,
    } = declared;
    assert!(window.start >= segment * SPAN && window.end <= (segment + 1) * SPAN);
    let raw_window = if nominal {
        nominal_window(capture, window.start, window.end)
    } else {
        complete_window(
            window.start as i64,
            window.end as i64,
            capture.output[0].len(),
        )
    };
    for (channel, source_hz) in [160.0, 211.0].into_iter().enumerate() {
        let expected = source_hz * 2.0_f64.powf(f64::from(shift) / 12.0) * RATES[segment];
        let Some(actual_window) = raw_window.clone() else {
            counts.record(false, false, false);
            profile_emit(
                json!({"stage":"tone_profile","label":label,"profile":capture.profile.label(),
                "family":family,"timeline":if nominal {"nominal"} else {"natural"},
                "segment":segment,"channel":channel,"window":[window.start,window.end],
                "expected_hz":expected,"available":false,"eligible":false,"pass":false,
                "reason":"entire predeclared shifted window unavailable; no truncation",
                "public_or_human_acceptance":false}),
            );
            continue;
        };
        assert_eq!(actual_window.len(), window.len());
        let planar = if nominal {
            &capture.raw
        } else {
            &capture.output
        };
        let samples = &planar[channel][actual_window.clone()];
        let crossings = samples
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
            .map(|(frame, pair)| frame as f64 - f64::from(pair[0]) / f64::from(pair[1] - pair[0]))
            .collect::<Vec<_>>();
        let measured = (crossings.len() >= 3).then(|| {
            (crossings.len() - 1) as f64 * f64::from(RATE)
                / (crossings.last().unwrap() - crossings[0])
        });
        let mean_energy = samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / samples.len() as f64;
        let cents = measured.map(|frequency| 1200.0 * (frequency / expected).log2());
        let eligible = crossings.len() > 8 && mean_energy > 0.001;
        let passed = eligible && cents.is_some_and(|value| value.abs() < 15.0);
        counts.record(true, eligible, passed);
        profile_emit(
            json!({"stage":"tone_profile","label":label,"profile":capture.profile.label(),
            "family":family,"timeline":if nominal {"nominal"} else {"natural"},
            "segment":segment,"channel":channel,"window":[window.start,window.end],
            "actual_capture_window":[actual_window.start,actual_window.end],
            "expected_hz":expected,"measured_hz":measured,"cents":cents,
            "positive_crossings":crossings.len(),"required_crossings_strict_greater_than":8,
            "mean_energy":mean_energy,"required_energy_strict_greater_than":0.001,
            "budget_cents_strict_less_than":15.0,"available":true,"eligible":eligible,"pass":passed,
            "fitted_delay_gain_crop":false,"public_or_human_acceptance":false}),
        );
    }
}

fn tone_profile_metrics(label: &str, shift: i32, capture: &Capture, results: &mut Results) {
    for segment in 0..4 {
        let first = segment * SPAN + SPAN / 2;
        for nominal in [false, true] {
            let counts = if nominal {
                &mut results.nominal_tone_8192
            } else {
                &mut results.natural_tone_8192
            };
            tone_window_metrics(
                label,
                shift,
                capture,
                ToneWindow {
                    segment,
                    range: first..first + 8192,
                    family: "original-8192",
                    nominal,
                },
                counts,
            );
        }
    }
    for segment in 0..2 {
        let first = segment * SPAN + 4096;
        let last = segment * SPAN + 28672;
        for nominal in [false, true] {
            let counts = if nominal {
                &mut results.supplemental_nominal_tone_24576
            } else {
                &mut results.supplemental_natural_tone_24576
            };
            tone_window_metrics(
                label,
                shift,
                capture,
                ToneWindow {
                    segment,
                    range: first..last,
                    family: "supplemental-first-two-24576",
                    nominal,
                },
                counts,
            );
        }
    }
}

#[test]
fn window_profile_setup_matches_independent_cabi_before_source_signal() {
    for (profile, expected_options) in [
        (NativeOutputProfile::Short, 0x1000_0000),
        (NativeOutputProfile::Medium, 0x1010_0000),
    ] {
        assert_eq!(profile.options().bits(), expected_options);
        for shift in [7, -23, 24] {
            let pitch = 2.0_f64.powf(f64::from(shift) / 12.0);
            let oracle = RawOracle::with_profile(pitch, profile);
            let mut actual = profile.native();
            actual.prepare_exact_pitch(pitch).unwrap();
            assert_eq!(actual.block_size(), BLOCK);
            assert_eq!(actual.pitch_scale(), pitch);
            assert_eq!(actual.start_delay(), oracle.delay);
            assert_eq!(
                actual.source_history_frames(pitch).unwrap(),
                oracle.minimum_history
            );
            assert!(oracle.delay > 0);
            assert_eq!(oracle.minimum_history % BLOCK, 0);
            assert!(oracle.minimum_history >= 4096);
            for phase in [0, 17] {
                let history = oracle.minimum_history + phase;
                let raw_cut = history - (BLOCK - 1);
                let compensation = oracle.delay + BLOCK - 1;
                assert_eq!(raw_cut + BLOCK - 1, history);
                assert_eq!(
                    signed_raw_time(raw_cut, raw_cut, compensation),
                    -(compensation as i64)
                );
                assert_eq!(
                    signed_raw_time(raw_cut + compensation, raw_cut, compensation),
                    0
                );
            }
        }
    }
}

#[test]
fn nominal_windows_require_complete_signed_capture_bounds_without_truncation() {
    assert!(complete_window(-1, 8191, OUTPUT).is_none());
    assert!(complete_window(0, 0, OUTPUT).is_none());
    assert!(complete_window(OUTPUT as i64 - 8191, OUTPUT as i64 + 1, OUTPUT).is_none());
    assert_eq!(complete_window(0, 8192, OUTPUT), Some(0..8192));
    assert_eq!(
        complete_window(OUTPUT as i64 - 8192, OUTPUT as i64, OUTPUT),
        Some(OUTPUT - 8192..OUTPUT)
    );
    assert_eq!(signed_raw_time(0, 3585, 3077), -6662);
    assert_eq!(signed_raw_time(6662, 3585, 3077), 0);
    for segment in 0..2 {
        let first = segment * SPAN + 4096;
        let last = segment * SPAN + 28672;
        assert_eq!(last - first, 24576);
        assert!(first >= segment * SPAN && last <= (segment + 1) * SPAN);
    }
}

#[test]
#[ignore = "explicit bounded existing Short/Medium mapped output comparison; acoustic failures remain results"]
fn measure_original_owned_mapped_native_window_profiles() {
    profile_emit(json!({"stage":"predeclared_matrix_before_signal",
        "profiles":["short-together","medium-together-classification-readahead"],
        "signals":["IsolatedAttack(0)","IsolatedAttack(73728)","StereoTone"],
        "shifts":[7,-23,24],"block_phases":[0,17],"callers":["512","irregular"],
        "streams":72,"output_capture_frames":OUTPUT,"no_wrapped_capture_extension":true,
        "nominal_c_rule":"actual post-preparation pre-source N + 511",
        "tone_original_frames":8192,"tone_supplemental_windows":[[4096,28672],[36864,61440]],
        "timing_budget_frames":96,"discard_budget":MAX_DISCARDED_ENERGY,
        "tail_budget":MAX_CAPTURE_TAIL_ENERGY,"tone_crossings_strict_greater_than":8,
        "tone_energy_strict_greater_than":0.001,"tone_cents_strict_less_than":15.0,
        "identity":crate::audio_engine::pitch_runtime_tests::native_identity_tests::identify_loaded_native()
            .expect("actual linked Native DLL identity must be available")}));
    let mut results = Results::default();
    let mut streams = 0;
    for profile in [NativeOutputProfile::Short, NativeOutputProfile::Medium] {
        for signal in [
            Signal::IsolatedAttack(0),
            Signal::IsolatedAttack(73728),
            Signal::StereoTone,
        ] {
            let authored = CanonicalFixture::new(signal);
            authored.verify_pcm();
            let pcm = authored
                .sample
                .samples
                .iter()
                .map(|sample| {
                    (f64::from(*sample) * 32768.0)
                        .round()
                        .clamp(i16::MIN as f64, i16::MAX as f64) as i16
                })
                .collect::<Vec<_>>();
            let decoded = pcm
                .iter()
                .map(|sample| f32::from(*sample) / 32768.0)
                .collect::<Vec<_>>();
            let quantization_error = authored
                .sample
                .samples
                .iter()
                .zip(&decoded)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(quantization_error <= 1.0 / 65536.0);
            profile_emit(
                json!({"stage":"declared_profile_source","profile":profile.label(),
                "signal":format!("{signal:?}"),"source_frames":SOURCE,"channels":2,
                "authored_pcm_sha256":authored.pcm_hash,"decoded_pcm16_sha256":pcm_hash(&decoded),
                "quantization_max_error":quantization_error,"source_rates":RATES,"source_anchors":ANCHORS,
                "creative_phase_output_frames":0,"automatic_musical_qualification":false}),
            );
            for shift in [7, -23, 24] {
                for block_phase in [0, 17] {
                    let mut previous: Option<Capture> = None;
                    for caller in CALLERS {
                        let capture =
                            capture_profile(&pcm, &decoded, shift, block_phase, caller, profile);
                        if let Some(prior) = &previous {
                            assert_eq!(capture.output, prior.output);
                            assert_eq!(capture.dry, prior.dry);
                            assert_eq!(capture.raw, prior.raw);
                            assert_eq!(capture.native_delay, prior.native_delay);
                            assert_eq!(capture.minimum_history, prior.minimum_history);
                            assert_eq!(capture.raw_cut, prior.raw_cut);
                        }
                        let label = format!(
                            "{}_{signal:?}_k{shift}_blockphase{block_phase}_{}",
                            profile.label(),
                            caller.0
                        );
                        match signal {
                            Signal::IsolatedAttack(marker) => {
                                isolated_profile_metrics(&label, marker, &capture, &mut results)
                            }
                            Signal::StereoTone => {
                                tone_profile_metrics(&label, shift, &capture, &mut results)
                            }
                        }
                        previous = Some(capture);
                        streams += 1;
                    }
                }
            }
        }
    }
    assert_eq!(streams, 72);
    assert_eq!(results.natural_isolated.declared, 48);
    assert_eq!(results.nominal_full_raw_timing.declared, 48);
    assert_eq!(results.first_sound_retention.declared, 48);
    assert_eq!(results.nominal_end_window.declared, 48);
    assert_eq!(results.natural_tone_8192.declared, 192);
    assert_eq!(results.nominal_tone_8192.declared, 192);
    assert_eq!(results.supplemental_natural_tone_24576.declared, 96);
    assert_eq!(results.supplemental_nominal_tone_24576.declared, 96);
    profile_emit(
        json!({"stage":"technical_profile_comparison_complete","streams":streams,
        "natural_isolated_combined":results.natural_isolated.value(),
        "nominal_full_raw_timing_only":results.nominal_full_raw_timing.value(),
        "first_sound_necessary_retention_only":results.first_sound_retention.value(),
        "nominal_end_window_availability_no_combined_pass":results.nominal_end_window.value(),
        "natural_original_tone_8192":results.natural_tone_8192.value(),
        "nominal_original_tone_8192":results.nominal_tone_8192.value(),
        "supplemental_natural_tone_24576":results.supplemental_natural_tone_24576.value(),
        "supplemental_nominal_tone_24576":results.supplemental_nominal_tone_24576.value(),
        "acoustic_failures_are_results":true,"nominal_combined_acoustic_pass":null,
        "actual_s":null,"actual_e":null,"product_token":null,"public_native_admission":false,
        "device_music_human_or_full_goal_acceptance":false}),
    );
}
