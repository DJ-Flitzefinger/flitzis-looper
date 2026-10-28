//! Fixed prerequisite falsification, not a supported-domain or musical acceptance gate.
//! The interior analytic kick/input is the exact 44.1k/r=.5/h=1/phase0 Live Medium
//! hard negative from key_lock_native_envelope_tests. Offline input uses the original
//! source, q=1/r and p=h; its dry varispeed baseline is not a pitch-preserving PCM oracle.

use super::*;
use crate::audio_engine::audio_quality_metrics::stereo_response;
use crate::audio_engine::key_lock_physical_onset_tests::{PROFILES, RawNative};
use sha2::{Digest, Sha256};

const RATE: u32 = 44_100;
const SOURCE_RATE: f64 = 0.5;
const TOTAL_SEMITONES: i32 = 0;
const HISTORY: usize = 131_072;
const POSTROLL: usize = 65_536;
const LIVE_CAPTURE: usize = HISTORY + POSTROLL;
const ORIGINAL_SOURCE_FRAMES: usize = POSTROLL * 2 + 2;
const INTERIOR_SOURCE_EVENT: usize = HISTORY / 2;
const CAPTURE_COUNT: usize = 2 * 3 * 2 * 2;
// Exact pinned rubberband-c.h Stretcher flags. Crisp, Laminar, Standard and
// SmoothingOff are zero; R3 receives no ineffective transient/detector/phase flags.
const DETECTOR_PERCUSSIVE: c_int = 0x0000_0400;
const WINDOW_SHORT: c_int = 0x0010_0000;

#[derive(Clone, Copy)]
struct FixedProfile {
    name: &'static str,
    engine: c_int,
    options: c_int,
}

const FIXED_PROFILES: [FixedProfile; 3] = [
    FixedProfile {
        name: "R2_crisp_percussive_laminar_standard",
        engine: 2,
        options: THREADING_NEVER | CHANNELS_TOGETHER | DETECTOR_PERCUSSIVE,
    },
    FixedProfile {
        name: "R2_crisp_percussive_laminar_short",
        engine: 2,
        options: THREADING_NEVER | CHANNELS_TOGETHER | DETECTOR_PERCUSSIVE | WINDOW_SHORT,
    },
    FixedProfile {
        name: "R3_standard_control",
        engine: 3,
        options: THREADING_NEVER | CHANNELS_TOGETHER | ENGINE_FINER,
    },
];

/// Exact decaying-kick definition, operation order and 5ms source taper from the
/// original envelope fixture. This duplication keeps that existing file untouched.
fn kick(frame: usize, channel: usize) -> f32 {
    let end = (RATE as usize * 80).div_ceil(1000);
    if frame >= end {
        return 0.0;
    }
    let time = frame as f64 / f64::from(RATE);
    let tau = std::f64::consts::TAU;
    let phase = tau * (42.0 * time + 38.0 * 0.012 * (1.0 - (-time / 0.012).exp()));
    let low = 0.52 * (-time / 0.018).exp() * phase.cos();
    let click = 0.07 * (-time / 0.0007).exp() * (tau * 2200.0 * time).cos();
    let level = if channel == 0 { 1.0 } else { 0.74 };
    let value = level * (low + click);
    let taper_frames = RATE as usize / 200;
    let taper_start = end - taper_frames;
    let taper = if frame < taper_start {
        1.0
    } else {
        0.5 * (1.0
            + (std::f64::consts::PI * (frame - taper_start) as f64 / (taper_frames - 1) as f64)
                .cos())
    };
    (value * taper) as f32
}

fn original_source() -> [Vec<f32>; 2] {
    std::array::from_fn(|channel| {
        (0..ORIGINAL_SOURCE_FRAMES)
            .map(|frame| kick(frame, channel))
            .collect()
    })
}

fn original_hash(source: &[Vec<f32>; 2]) -> String {
    let mut hash = Sha256::new();
    for frame in 0..ORIGINAL_SOURCE_FRAMES {
        for channel in source {
            hash.update(channel[frame].to_le_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

/// Independent finite, zero-extended original-source addressing. No live clock,
/// prepared history, Native output or fitted attack coordinate supplies these taps.
fn dry_mapping(input: &[Vec<f32>], output_frames: usize, source_event: usize) -> Vec<Vec<f32>> {
    let output_event = source_event * 2;
    input
        .iter()
        .map(|channel| {
            (0..output_frames)
                .map(|frame| {
                    // The declared physical event has silent negative coordinates; the
                    // first positive attack tap cannot leak into the preceding half-step.
                    if frame < output_event {
                        return 0.0;
                    }
                    let coordinate =
                        source_event as f64 + (frame - output_event) as f64 * SOURCE_RATE;
                    let left = coordinate.floor() as usize;
                    let a = channel.get(left).copied().unwrap_or(0.0);
                    let b = channel.get(left + 1).copied().unwrap_or(0.0);
                    a + (b - a) * coordinate.fract() as f32
                })
                .collect()
        })
        .collect()
}

fn live_hard_negative_input(source: &[Vec<f32>; 2]) -> [Vec<f32>; 2] {
    assert_eq!(LIVE_CAPTURE % MAX_PROCESS_SIZE, 0);
    assert_eq!(HISTORY % MAX_PROCESS_SIZE, 0);
    assert!(
        (LIVE_CAPTURE - 1 - HISTORY) as f64 * SOURCE_RATE + 1.0 < ORIGINAL_SOURCE_FRAMES as f64
    );
    std::array::from_fn(|channel| {
        (0..LIVE_CAPTURE)
            .map(|frame| {
                let signed = frame as i64 - HISTORY as i64;
                if signed < 0 {
                    return 0.0;
                }
                let coordinate = signed as f64 * SOURCE_RATE;
                let left = coordinate.floor() as usize;
                let a = source[channel][left];
                let b = source[channel][left + 1];
                a + (b - a) * coordinate.fract() as f32
            })
            .collect()
    })
}

struct Diagnostic {
    timing_interval: Option<[i64; 2]>,
    strict_pass: bool,
}

fn diagnose(label: &str, output: &[Vec<f32>], dry: &[Vec<f32>], event: usize) -> Diagnostic {
    assert_eq!(output.len(), 2);
    assert_eq!(dry.len(), 2);
    let frames = output[0].len();
    assert!((1..=MAX_CAPTURE_FRAMES).contains(&frames));
    assert!(
        output
            .iter()
            .chain(dry)
            .all(|channel| channel.len() == frames)
    );
    assert!(
        output
            .iter()
            .chain(dry)
            .flatten()
            .all(|sample| sample.is_finite())
    );
    assert!(event < frames);
    let budget = (f64::from(RATE) * TIMING_BUDGET_MS / 1000.0).floor() as i64;
    assert_eq!(
        budget, 88,
        "the unchanged 2ms gate admits 88, not 89, integer frames"
    );
    let dry_energy = stereo_energy(dry);
    let dry_times = energy_times(&dry_energy, RATE);
    let energy = stereo_energy(output);
    let total = energy.iter().sum::<f64>();
    assert!(total.is_finite());
    let positive = total > 0.0;
    let raw = stereo_response(output);
    let times = positive.then(|| energy_times(&energy, RATE));
    let errors = times.map(|times| std::array::from_fn::<_, 3, _>(|i| times[i] - dry_times[i]));
    let timing_interval = errors.map(|error| {
        [
            error[0].max(error[1]) - budget,
            error[0].min(error[1]) + budget,
        ]
    });
    let prefix = positive.then(|| energy[..event].iter().sum::<f64>() / total);
    let tail_frames = RATE as usize / 50;
    assert!(tail_frames < frames);
    let tail = positive.then(|| energy[frames - tail_frames..].iter().sum::<f64>() / total);
    let energy_boundary = positive.then(|| maximum_energy_discard(&energy));
    let peak_retained = positive && raw.peak_frame >= event;
    let retention_upper =
        energy_boundary.map(|boundary| raw.peak_frame.min(boundary) as i64 - event as i64);
    let joint_interval = timing_interval
        .zip(retention_upper)
        .map(|(range, upper)| [range[0], range[1].min(upper)]);
    let timing_exists = timing_interval.is_some_and(|range| range[0] <= range[1]);
    let necessary_lead = timing_interval
        .zip(retention_upper)
        .and_then(|(range, upper)| (range[0] <= range[1]).then_some((range[0] - upper).max(0)));
    let raw_timing =
        errors.is_some_and(|error| error[..2].iter().all(|value| value.abs() <= budget));
    let loss_pass = prefix.is_some_and(|fraction| fraction <= MAX_DISCARDED_ENERGY);
    let tail_pass = tail.is_some_and(|fraction| fraction <= MAX_CAPTURE_TAIL_ENERGY);
    let mut gated = output.to_vec();
    for channel in &mut gated {
        channel[..event].fill(0.0);
    }
    let gated_energy = stereo_energy(&gated);
    let gated_total = gated_energy.iter().sum::<f64>();
    let gated_times = (gated_total > 0.0).then(|| energy_times(&gated_energy, RATE));
    let gated_errors =
        gated_times.map(|times| std::array::from_fn::<_, 3, _>(|i| times[i] - dry_times[i]));
    let gated_timing =
        gated_errors.is_some_and(|error| error[..2].iter().all(|value| value.abs() <= budget));
    let strict_pass = gated_timing && peak_retained && loss_pass && tail_pass;
    let mut record = serde_json::json!({
        "stage":"hard_negative_quality_diagnostic", "label":label,
        "sample_rate_hz":RATE,"source_rate_r":SOURCE_RATE,"total_semitones":TOTAL_SEMITONES,
        "complete_capture_frames":frames,"raw_output_origin":0,"declared_event_frame":event,
        "external_translation_frames":0,"external_crop_frames":0,"external_gain":1.0,
        "dry_q10_q50_q90":dry_times,"raw_q10_q50_q90":times,"raw_q_errors":errors,
        "raw_stereo_onset":raw.onset,"raw_stereo_one_percent_onset":raw.significant_onset,
        "ORIGINAL_stereo_peak_frame":raw.peak_frame,"ORIGINAL_stereo_peak_amplitude":raw.peak,
        "ORIGINAL_total_energy_denominator":total,"dry_total_energy":dry_energy.iter().sum::<f64>(),
        "energy_fraction_before_declared_event":prefix,"final_20ms_energy_fraction":tail,
    });
    let details = serde_json::json!({
        "maximum_retaining_raw_boundary":energy_boundary,"retention_U_relative_to_event":retention_upper,
        "timing_only_integer_translation_interval":timing_interval,"timing_interval_exists":timing_exists,
        "joint_timing_retention_integer_interval":joint_interval,
        "joint_interval_with_complete_tail_exists":joint_interval.is_some_and(|range| range[0]<=range[1]) && tail_pass,
        "necessary_earlier_lead_frames_if_timing_interval_exists":necessary_lead,
        "q_spread_minimax_lower_bound_frames":errors.map(|error| (error[0]-error[1]).abs() as f64/2.0),
        "strict_gate_same_full_origin":{
            "all_pre_event_samples_zero":gated.iter().all(|channel| channel[..event].iter().all(|value| *value==0.0)),
            "q10_q50_q90":gated_times,"q_errors":gated_errors,
            "ORIGINAL_peak_retained":peak_retained,"retained_fraction_of_ORIGINAL_energy":positive.then(||gated_total/total),
            "changes_only_pre_event_samples":true
        },
        "fixed_gates":{
            "timing_budget_ms":TIMING_BUDGET_MS,"timing_integer_budget_frames":budget,
            "maximum_discarded_energy":MAX_DISCARDED_ENERGY,"maximum_final20ms_energy":MAX_CAPTURE_TAIL_ENERGY,
            "raw_timing_pass":raw_timing,"strict_gated_timing_pass":gated_timing,
            "ORIGINAL_peak_retained_pass":peak_retained,"discarded_energy_pass":loss_pass,"capture_tail_pass":tail_pass,
            "raw_criterion_pass":raw_timing && peak_retained && loss_pass && tail_pass,
            "strict_gated_criterion_pass":strict_pass
        },
        "translation_intervals_are_diagnostics_not_selected_C":true,
        "pitch_accuracy_evaluated":false,"active_glide_compatibility_evaluated":false,
        "product_or_listening_acceptance_claim":false
    });
    record
        .as_object_mut()
        .unwrap()
        .extend(details.as_object().unwrap().clone());
    emit(record);
    Diagnostic {
        timing_interval,
        strict_pass,
    }
}

#[test]
fn original_source_stretcher_fixed_profiles_qualify_known_44100_kick_hard_negative() {
    assert_eq!(TIMING_BUDGET_MS, 2.0);
    assert_eq!(MAX_DISCARDED_ENERGY, 0.001);
    assert_eq!(MAX_CAPTURE_TAIL_ENERGY, 0.000_001);
    let h = 2.0_f64.powf(f64::from(TOTAL_SEMITONES) / 12.0);
    let offline_q = 1.0 / SOURCE_RATE;
    let offline_p = h;
    let live_p = h / SOURCE_RATE;
    assert_eq!((offline_q, offline_p, live_p), (2.0, 1.0, 2.0));
    let source = original_source();
    let support_end = (RATE as usize * 80).div_ceil(1000);
    assert_eq!(support_end, 3528);
    assert!(
        source
            .iter()
            .all(|channel| channel.len() == ORIGINAL_SOURCE_FRAMES
                && channel.iter().all(|sample| sample.is_finite())
                && channel[support_end..].iter().all(|sample| *sample == 0.0))
    );
    assert_eq!(source[0][0], 0.59_f32);
    let hash = original_hash(&source);
    emit(serde_json::json!({
        "stage":"predeclared_hard_negative_qualification", "sample_rate_hz":RATE,
        "source_identity":"kick_80ms_80_to_42hz_18ms_decay_v1","source_pcm_sha256":hash,
        "original_source_frames":ORIGINAL_SOURCE_FRAMES,"source_support":[0,support_end],
        "r":SOURCE_RATE,"total_semitones":TOTAL_SEMITONES,"h":h,"live_p":live_p,
        "offline_q":offline_q,"offline_p":offline_p,"event_phase":0,
        "known_Live_Medium_timing_interval":[4076,3770],
        "source_event_frames":[INTERIOR_SOURCE_EVENT,0],"output_event_frames":[HISTORY,0],
        "finite_input_frames":[INTERIOR_SOURCE_EVENT+ORIGINAL_SOURCE_FRAMES,ORIGINAL_SOURCE_FRAMES],
        "complete_output_frames":[(INTERIOR_SOURCE_EVENT+ORIGINAL_SOURCE_FRAMES)*2,ORIGINAL_SOURCE_FRAMES*2],
        "maps":["absent","positive_linear_geometry_anchors"],
        "profiles":FIXED_PROFILES.map(|profile| serde_json::json!({
            "name":profile.name,"engine":profile.engine,"options_bits":profile.options,
            "R2_transients":(profile.engine==2).then_some("Crisp"),
            "R2_detector":(profile.engine==2).then_some("Percussive"),
            "R2_phase":(profile.engine==2).then_some("Laminar"),
            "R3_ignored_transient_labels":false
        })),
        "caller_partitions":[[512],[73,511,7,512,127,3]],"native_ingress":"same canonical512 adapter, final remainder retained",
        "capture_count":CAPTURE_COUNT,"max_input_frames":MAX_INPUT_FRAMES,"max_capture_frames":MAX_CAPTURE_FRAMES,
        "strict_quality_gates_reported_without_acceptance_assertion":true,
        "offline_native_alignment_is_internal_not_an_external_selected_C":true,
        "dry_baseline_is_independent_varispeed_not_a_pitch_preserving_waveform_oracle":true
    }));
    let mut captures = 0;
    let mut strict_passes = 0;
    for source_event in [INTERIOR_SOURCE_EVENT, 0] {
        let source_frames = source_event + ORIGINAL_SOURCE_FRAMES;
        let output_frames = source_frames * 2;
        let output_event = source_event * 2;
        let input = (0..2)
            .map(|channel| {
                (0..source_frames)
                    .map(|frame| {
                        frame
                            .checked_sub(source_event)
                            .map_or(0.0, |frame| source[channel][frame])
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let dry = dry_mapping(&input, output_frames, source_event);
        assert!(
            dry.iter()
                .all(|channel| channel[..output_event].iter().all(|sample| *sample == 0.0))
        );
        for channel in 0..2 {
            assert_eq!(dry[channel][output_event], source[channel][0]);
        }
        if source_event == INTERIOR_SOURCE_EVENT {
            let live_input = live_hard_negative_input(&source);
            for channel in 0..2 {
                assert_eq!(
                    &dry[channel][..LIVE_CAPTURE],
                    live_input[channel].as_slice()
                );
                assert!(
                    live_input[channel][LIVE_CAPTURE - RATE as usize / 50..]
                        .iter()
                        .all(|sample| *sample == 0.0)
                );
            }
            // This fixed control reproduces the exact prior Live geometry. The
            // longer offline captures below retain their own complete outputs.
            let (live, _) = RawNative::capture_at_rate(&live_input, RATE, live_p, PROFILES[1]);
            let control = diagnose(
                "existing_Live_Medium_hard_negative_control",
                &live,
                &live_input,
                HISTORY,
            );
            assert_eq!(control.timing_interval, Some([4076, 3770]));
        }
        for profile in FIXED_PROFILES {
            for mapped in [false, true] {
                // At source zero use a positive midpoint control anchor; never
                // send 0->0 or describe the terminal boundary as an attack anchor.
                let first_anchor = if source_event == 0 {
                    source_frames / 2
                } else {
                    source_event
                };
                let keyframes = mapped.then_some(OfflineKeyFrames {
                    from: [
                        u32::try_from(first_anchor).unwrap(),
                        u32::try_from(source_frames).unwrap(),
                    ],
                    to: [
                        u32::try_from(first_anchor * 2).unwrap(),
                        u32::try_from(output_frames).unwrap(),
                    ],
                });
                let configuration = OfflineConfiguration {
                    sample_rate_hz: RATE,
                    engine: profile.engine,
                    options: profile.options,
                    time_ratio: offline_q,
                    pitch_scale: offline_p,
                    keyframes,
                };
                emit(serde_json::json!({
                    "stage":"predeclared_hard_negative_case", "profile":profile.name,"source_event":source_event,
                    "output_event":output_event,"input_frames":source_frames,"output_frames":output_frames,
                    "map_present":mapped,"positive_native_from":keyframes.map(|map|map.from),"positive_native_to":keyframes.map(|map|map.to),
                    "semantic_zero_origin_is_implicit":true,"map_is_geometry_not_measured_attack_fit":true,
                    "terminal_keyframe_is_end_exclusive_duration_boundary":true
                }));
                let mut previous: Option<Vec<Vec<f32>>> = None;
                for (pattern_name, pattern) in PROCESS_PATTERNS {
                    captures += 1;
                    assert!(captures <= CAPTURE_COUNT);
                    let output = offline_capture_configured(
                        &input,
                        configuration,
                        pattern_name,
                        pattern,
                        output_frames,
                        true,
                    );
                    let label = format!(
                        "{}_event{}_map{}_{}",
                        profile.name, source_event, mapped, pattern_name
                    );
                    strict_passes +=
                        usize::from(diagnose(&label, &output, &dry, output_event).strict_pass);
                    if let Some(reference) = &previous {
                        let (equal, mismatches, error) = compare_complete_pcm(
                            profile.engine,
                            &label,
                            reference,
                            &output,
                            output_frames,
                        );
                        assert!(
                            equal,
                            "fixed canonical ingress changed whole PCM: {mismatches} samples, max {error}"
                        );
                    } else {
                        previous = Some(output);
                    }
                }
            }
        }
    }
    assert_eq!(captures, CAPTURE_COUNT);
    emit(serde_json::json!({
        "stage":"hard_negative_qualification_complete","captures":captures,"strict_gate_pass_count":strict_passes,
        "quality_failures_remain_reported":true,"supported_domain_evaluated":false,
        "selected_backend_or_C":false,"musical_device_or_Human_acceptance":false
    }));
}
