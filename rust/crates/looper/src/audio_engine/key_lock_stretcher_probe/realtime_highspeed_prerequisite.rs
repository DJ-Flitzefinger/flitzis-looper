//! A distinct fixed-pitch before-resampling prerequisite, using the one test owner.
//! The v4.0.0 source predicts the mechanism; it does not prove the linked DLL's source.

use super::*;

const HIGH_SPEED: RealtimeProfile = RealtimeProfile {
    options: 0x3001_0001,
    unity_pad: 2048,
    unity_delay: 2048,
    prerequisite_pad: 4096,
    prerequisite_delay: 2048,
    pre_signal_stage: "realtime_highspeed_pre_signal_api",
    capture_stage: "realtime_highspeed_complete_raw_capture",
};
const SYNTHETIC_ARRAY_BUDGET_BYTES: usize = 256 * 1024 * 1024;
const FRAME_BYTES: usize = PROBE_CHANNELS * std::mem::size_of::<f32>();
const STAGER_BYTES: usize = PROBE_CHANNELS * MAX_PROCESS_SIZE * std::mem::size_of::<f32>();
const RAW_CHANNEL_CAPACITY_BOUND: usize = 2 * MAX_CAPTURE_FRAMES;
const PROCESS_BLOCK_CAPACITY_BOUND: usize = 2 * MAX_PROCESS_CALLS;
// More than the 35 outer/process/scorer Vec headers even when every temporary
// header is counted together. Resident planar channel headers are counted too.
const ADDITIONAL_VECTOR_HEADER_BOUND: usize = 64;

#[derive(Clone, Copy)]
struct ResidentPcm {
    original: usize,
    shifted: usize,
    canonical: usize,
}

impl ResidentPcm {
    fn total(self) -> usize {
        self.original
            .checked_add(self.shifted)
            .unwrap()
            .checked_add(self.canonical)
            .unwrap()
    }
}

fn reference_scope() {
    report_loaded_identity();
    emit(serde_json::json!({
        "stage":"realtime_highspeed_primary_reference_scope",
        "primary_mechanism_reference":"RubberBand v4.0.0 R3Stretcher.h404-453 and R3Stretcher.cpp470-500,743-797",
        "predeclared_mechanism":"HighSpeed fixed p2 resamples before stretching; HC and HQ fixed p2 resample after",
        "linked_library_semver":null,"exact_linked_DLL_source_version_proven":false,
        "engine_number_is_library_semver":false,"additional_library_load_or_install":false
    }));
}

fn pcm_array_bytes(channels: &[Vec<f32>]) -> usize {
    channels
        .iter()
        .fold(std::mem::size_of_val(channels), |bytes, channel| {
            bytes
                .checked_add(
                    channel
                        .capacity()
                        .checked_mul(std::mem::size_of::<f32>())
                        .unwrap(),
                )
                .unwrap()
        })
}

/// Sum all nominal/strict diagnostic arrays as if they coexisted, including the
/// already-dropped can_diagnose energy. Every envelope has a prefix AND output.
/// R terms: nominal28R + strict100R. C terms: dry24C + nominal32C.
fn scorer_array_bytes(raw_frames: usize, canonical_frames: usize) -> usize {
    let half_window = (f64::from(RATE) * 0.000_25).round() as usize;
    assert_eq!(half_window, 11);
    let six_envelope_edges = 6 * (1 + 2 * half_window) * std::mem::size_of::<f64>();
    raw_frames
        .checked_mul(128)
        .and_then(|bytes| bytes.checked_add(canonical_frames.checked_mul(56)?))
        .and_then(|bytes| bytes.checked_add(six_envelope_edges))
        .unwrap()
}

fn memory_bound_before_capture(label: &str, resident: ResidentPcm, canonical_frames: usize) {
    let raw_slots = 2 * PROBE_CHANNELS * RAW_CHANNEL_CAPACITY_BOUND * std::mem::size_of::<f32>();
    let vector_headers = ADDITIONAL_VECTOR_HEADER_BOUND * std::mem::size_of::<Vec<f32>>();
    let process_blocks = PROCESS_BLOCK_CAPACITY_BOUND * std::mem::size_of::<usize>();
    let scorer = scorer_array_bytes(MAX_CAPTURE_FRAMES, canonical_frames);
    let peak = resident
        .total()
        .checked_add(raw_slots)
        .and_then(|bytes| bytes.checked_add(vector_headers))
        .and_then(|bytes| bytes.checked_add(process_blocks))
        .and_then(|bytes| bytes.checked_add(2 * STAGER_BYTES))
        .and_then(|bytes| bytes.checked_add(scorer))
        .unwrap();
    assert!(peak <= SYNTHETIC_ARRAY_BUDGET_BYTES);
    emit(serde_json::json!({
        "stage":"realtime_highspeed_pre_signal_array_budget","label":label,
        "original_PCM_capacity_and_headers_bytes":resident.original,
        "shifted_PCM_capacity_and_headers_bytes":resident.shifted,
        "canonical_input_PCM_capacity_and_headers_bytes":resident.canonical,
        "resident_original_shifted_canonical_PCM_capacity_and_headers_bytes":resident.total(),
        "two_complete_current_and_previous_Raw_slots_capacity_bytes":raw_slots,
        "maximum_Raw_channel_capacity_samples":RAW_CHANNEL_CAPACITY_BOUND,
        "additional_vector_headers_bytes":vector_headers,"maximum_process_block_array_bytes":process_blocks,
        "stager_bytes":STAGER_BYTES,"drain_scratch_bytes":STAGER_BYTES,
        "scorer_arrays_including_all_nonoverlapping_temporaries_summed_bytes":scorer,
        "scorer_formula":"128*RawFrames + 56*canonicalFrames + 6*(1+2*11)*8",
        "conservative_synthetic_array_peak_bound_bytes":peak,
        "synthetic_array_budget_bytes":SYNTHETIC_ARRAY_BUDGET_BYTES,
        "scope":"owned sample/vector payloads and headers; excludes allocator metadata, JSON diagnostics and opaque Native internal RAM",
        "callback_Native_internal_RSS_or_resource216_proof":false
    }));
}

fn actual_array_memory(
    label: &str,
    resident: ResidentPcm,
    canonical_frames: usize,
    capture: &Capture,
    previous: Option<&[Vec<f32>]>,
) {
    for channels in std::iter::once(capture.raw.as_slice()).chain(previous) {
        assert_eq!(channels.len(), PROBE_CHANNELS);
        assert!(
            channels
                .iter()
                .all(|channel| channel.capacity() <= RAW_CHANNEL_CAPACITY_BOUND)
        );
    }
    let current_raw = pcm_array_bytes(&capture.raw);
    let previous_raw = previous.map_or(0, pcm_array_bytes);
    let scorer = scorer_array_bytes(capture.raw[0].len(), canonical_frames);
    assert!(capture.process_block_capacity <= PROCESS_BLOCK_CAPACITY_BOUND);
    let process_blocks = capture.process_block_capacity * std::mem::size_of::<usize>();
    // The owner and its buffers have retired before scoring. Counting their full
    // caps here too deliberately exceeds the actual overlap of these arrays.
    let peak = resident
        .total()
        .checked_add(current_raw)
        .and_then(|bytes| bytes.checked_add(previous_raw))
        .and_then(|bytes| bytes.checked_add(scorer))
        .and_then(|bytes| bytes.checked_add(process_blocks + 2 * STAGER_BYTES))
        .and_then(|bytes| {
            bytes.checked_add(ADDITIONAL_VECTOR_HEADER_BOUND * std::mem::size_of::<Vec<f32>>())
        })
        .unwrap();
    assert!(peak <= SYNTHETIC_ARRAY_BUDGET_BYTES);
    emit(serde_json::json!({
        "stage":"realtime_highspeed_actual_array_memory","label":label,
        "original_PCM_capacity_and_headers_bytes":resident.original,
        "shifted_PCM_capacity_and_headers_bytes":resident.shifted,
        "canonical_input_PCM_capacity_and_headers_bytes":resident.canonical,
        "resident_original_shifted_canonical_PCM_capacity_and_headers_bytes":resident.total(),
        "complete_current_Raw_frames":capture.raw[0].len(),
        "complete_current_Raw_channel_capacities":capture.raw.iter().map(Vec::capacity).collect::<Vec<_>>(),
        "complete_current_Raw_capacity_and_headers_bytes":current_raw,
        "complete_previous_comparison_Raw_capacity_and_headers_bytes":previous_raw,
        "actual_process_block_array_capacity":capture.process_block_capacity,
        "actual_process_block_array_capacity_bytes":process_blocks,
        "comparison_makes_no_PCM_copy":true,
        "scorer_arrays_including_all_nonoverlapping_temporaries_summed_bytes":scorer,
        "conservative_actual_extent_synthetic_array_peak_bytes":peak,
        "synthetic_array_budget_bytes":SYNTHETIC_ARRAY_BUDGET_BYTES,
        "allocator_metadata_JSON_diagnostics_and_Native_internal_RAM_included":false,
        "callback_or_whole_system_RAM_proof":false
    }));
}

fn declared_partition(canonical_frames: usize, pitch: f64, full_blocks: usize, rest: usize) {
    let pad = if pitch == 1.0 {
        HIGH_SPEED.unity_pad
    } else {
        HIGH_SPEED.prerequisite_pad
    };
    assert_eq!(
        pad + canonical_frames,
        full_blocks * MAX_PROCESS_SIZE + rest
    );
    assert_eq!((pad + canonical_frames) / MAX_PROCESS_SIZE, full_blocks);
    assert_eq!((pad + canonical_frames) % MAX_PROCESS_SIZE, rest);
    emit(serde_json::json!({
        "stage":"realtime_highspeed_predeclared_input_partition",
        "canonical_frames":canonical_frames,"pitch_p":pitch,"expected_P":pad,
        "expected_full_512_blocks":full_blocks,"expected_final_real_block_frames":rest,
        "actual_block_sequence_asserted_by_shared_capture":true,"caller_partitions_do_not_change_Native_blocks":true
    }));
}

fn highspeed_unity_control(retained_original_bytes: usize) -> bool {
    assert_eq!(STAGER_BYTES, 4096);
    emit(serde_json::json!({
        "stage":"realtime_highspeed_predeclared_unity_control","frames":UNITY_FRAMES,
        "event":UNITY_EVENT,"stereo_impulse":[1.0,-0.73],"time_ratio_q":TIME_RATIO,"pitch_p":1.0,
        "options_bits":HIGH_SPEED.options,"expected_P":HIGH_SPEED.unity_pad,"expected_N":HIGH_SPEED.unity_delay,
        "nominal_axis":"raw_n-N","nominal_peak_offset_budget_frames":TIMING_FRAMES,
        "callers":["512","irregular"],"PCM_error_budget":null,
        "PCM_error_scope":"own HighSpeed waveform measured; no HC waveform oracle or sample-exact unity promise"
    }));
    declared_partition(UNITY_FRAMES, 1.0, 36, 7);
    let mut input = vec![vec![0.0_f32; UNITY_FRAMES]; PROBE_CHANNELS];
    input[0][UNITY_EVENT] = 1.0;
    input[1][UNITY_EVENT] = -0.73;
    assert_eq!(stereo_peak(&input), (UNITY_EVENT, 1.0));
    assert_eq!(
        input
            .iter()
            .flatten()
            .filter(|sample| **sample != 0.0)
            .count(),
        2
    );
    let resident = ResidentPcm {
        original: retained_original_bytes,
        shifted: 0,
        canonical: pcm_array_bytes(&input),
    };
    memory_bound_before_capture("HighSpeed_unity", resident, UNITY_FRAMES);
    let mut previous: Option<Vec<Vec<f32>>> = None;
    let mut passes = 0;
    for caller in PROCESS_PATTERNS {
        let capture = capture_with_profile(&input, 1.0, caller, HIGH_SPEED);
        let label = format!("HighSpeed_unity_control_{}", caller.0);
        actual_array_memory(
            &label,
            resident,
            UNITY_FRAMES,
            &capture,
            previous.as_deref(),
        );
        let metrics = nominal_metrics(&label, &capture, &input, UNITY_EVENT);
        let complete = capture.raw[0].len() >= capture.delay + UNITY_FRAMES;
        let mut max_error = 0.0_f64;
        let mut squared_error = 0.0;
        if complete {
            for (channel, dry) in input.iter().enumerate() {
                for (wet, dry) in capture.raw[channel][capture.delay..capture.delay + UNITY_FRAMES]
                    .iter()
                    .zip(dry)
                {
                    let error = f64::from(*wet) - f64::from(*dry);
                    max_error = max_error.max(error.abs());
                    squared_error += error * error;
                }
            }
        }
        let peak_pass = metrics
            .peak_offset
            .is_some_and(|offset| offset.abs() <= TIMING_FRAMES);
        let control_pass = metrics.pass && peak_pass;
        passes += usize::from(control_pass);
        emit(serde_json::json!({
            "stage":"realtime_highspeed_unity_control_result","caller":caller.0,
            "nominal_peak_offset_frames":metrics.peak_offset,"nominal_peak_offset_budget_frames":TIMING_FRAMES,
            "maximum_absolute_PCM_error":complete.then_some(max_error),
            "RMS_PCM_error":complete.then(||(squared_error/(PROBE_CHANNELS*UNITY_FRAMES) as f64).sqrt()),
            "PCM_error_budget":null,"sample_exact_unity_waveform_required":false,
            "fixed_nominal_coordinate_and_energy_budgets_pass":control_pass
        }));
        if let Some(reference) = &previous {
            let (equal, mismatches, error) =
                compare_complete_pcm(3, &label, reference, &capture.raw, reference[0].len());
            assert!(
                equal,
                "HighSpeed unity caller changed complete PCM: {mismatches} samples, max {error}"
            );
        } else {
            previous = Some(capture.raw);
        }
    }
    let pass = passes == PROCESS_PATTERNS.len();
    emit(serde_json::json!({
        "stage":"realtime_highspeed_unity_control_complete","passes":passes,
        "captures":PROCESS_PATTERNS.len(),"pass":pass
    }));
    pass
}

#[test]
fn realtime_standard_highspeed_unity_padding_control() {
    reference_scope();
    assert!(
        highspeed_unity_control(0),
        "own HighSpeed unity control failed; do not fit N or interpret kick mapping"
    );
}

#[test]
#[ignore = "explicit four-capture HighSpeed canonical kick measurement; acoustic rejection is evidence, not productive acceptance"]
fn realtime_standard_highspeed_canonical_kick_prerequisite() {
    assert_eq!(TIMING_BUDGET_MS, 2.0);
    assert_eq!(
        (f64::from(RATE) * TIMING_BUDGET_MS / 1000.0).floor() as i64,
        TIMING_FRAMES
    );
    assert_eq!(MAX_DISCARDED_ENERGY, 0.001);
    assert_eq!(MAX_CAPTURE_TAIL_ENERGY, 0.000_001);
    assert_eq!(
        PREREQUISITE_PITCH,
        2.0_f64.powf(f64::from(TOTAL_SEMITONES) / 12.0) / SOURCE_RATE
    );
    assert_eq!(MAX_PROCESS_CALLS, 3907);
    assert_eq!(FRAME_BYTES, 8);
    let source = original_source();
    reference_scope();
    emit(serde_json::json!({
        "stage":"realtime_highspeed_predeclared_prerequisite_matrix",
        "source_pcm_sha256":original_hash(&source),"original_source_frames":ORIGINAL_SOURCE_FRAMES,
        "sample_rate_hz":RATE,"r":SOURCE_RATE,"h":1.0,"key_lock":true,
        "canonical_warp_already_applied":true,"q":TIME_RATIO,"p":PREREQUISITE_PITCH,
        "options_bits":HIGH_SPEED.options,"expected_P":HIGH_SPEED.prerequisite_pad,"expected_N":HIGH_SPEED.prerequisite_delay,
        "source_events":[0,INTERIOR_SOURCE_EVENT],"canonical_events":[0,HISTORY],
        "canonical_frames":[262148,393220],
        "caller_partitions":PROCESS_PATTERNS.map(|(name,pattern)|serde_json::json!({"name":name,"frames":pattern})),
        "captures":PREREQUISITE_CAPTURES,"nominal_axis":"raw_n-N, complete negative nominal prefix retained in Raw",
        "nominal_tail_frames":RATE as usize/50,"timing_budget_frames":TIMING_FRAMES,
        "maximum_discarded_energy":MAX_DISCARDED_ENERGY,"maximum_tail_and_extra_energy":MAX_CAPTURE_TAIL_ENERGY,
        "candidate_survival":"all four actual nominal AND unchanged strict prerequisites plus both own HighSpeed unity controls",
        "synthetic_array_budget_bytes":SYNTHETIC_ARRAY_BUDGET_BYTES,"no_productive_backend_or_guard_change":true
    }));
    let source_bytes = pcm_array_bytes(&source);
    let unity_pass = highspeed_unity_control(source_bytes);
    let mut captures = 0;
    let mut nominal_passes = 0;
    let mut strict_passes = 0;
    let mut joint_passes = 0;
    for source_event in [0, INTERIOR_SOURCE_EVENT] {
        let source_frames = source_event + ORIGINAL_SOURCE_FRAMES;
        let canonical_frames = source_frames * 2;
        let event = source_event * 2;
        assert_eq!(
            canonical_frames,
            if source_event == 0 { 262148 } else { 393220 }
        );
        assert_eq!(event, if source_event == 0 { 0 } else { 131072 });
        declared_partition(
            canonical_frames,
            PREREQUISITE_PITCH,
            if source_event == 0 { 520 } else { 776 },
            4,
        );
        let shifted = (0..PROBE_CHANNELS)
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
        let canonical = dry_mapping(&shifted, canonical_frames, source_event);
        for channel in 0..PROBE_CHANNELS {
            assert_eq!(
                canonical[channel][event].to_bits(),
                source[channel][0].to_bits()
            );
            assert!(
                canonical[channel][..event]
                    .iter()
                    .all(|sample| *sample == 0.0)
            );
        }
        let resident = ResidentPcm {
            original: source_bytes,
            shifted: pcm_array_bytes(&shifted),
            canonical: pcm_array_bytes(&canonical),
        };
        if source_event == INTERIOR_SOURCE_EVENT {
            let live = live_hard_negative_input(&source);
            assert_eq!(LIVE_CAPTURE, 196608);
            for channel in 0..PROBE_CHANNELS {
                assert!(
                    canonical[channel][..LIVE_CAPTURE]
                        .iter()
                        .zip(&live[channel])
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
            }
            let live_comparison_bytes = resident.total() + pcm_array_bytes(&live);
            assert!(live_comparison_bytes <= SYNTHETIC_ARRAY_BUDGET_BYTES);
            emit(serde_json::json!({
                "stage":"realtime_highspeed_old_Live_input_parity",
                "canonical_prefix_frames_compared":LIVE_CAPTURE,"full_prefix_bitwise_equal":true,
                "original_shifted_canonical_and_old_Live_PCM_capacity_and_headers_bytes":live_comparison_bytes,
                "old_Live_comparison_retired_before_Native_capture":true
            }));
        }
        memory_bound_before_capture(
            &format!("HighSpeed_canonical_event{event}"),
            resident,
            canonical_frames,
        );
        let mut previous: Option<Vec<Vec<f32>>> = None;
        for caller in PROCESS_PATTERNS {
            captures += 1;
            assert!(captures <= PREREQUISITE_CAPTURES);
            let capture = capture_with_profile(&canonical, PREREQUISITE_PITCH, caller, HIGH_SPEED);
            let label = format!(
                "R3_realtime_Standard_HighSpeed_canonical_event{event}_{}",
                caller.0
            );
            actual_array_memory(
                &label,
                resident,
                canonical_frames,
                &capture,
                previous.as_deref(),
            );
            let nominal = nominal_metrics(&label, &capture, &canonical, event).pass;
            let strict = strict_prerequisite(&label, &capture, &canonical, event);
            nominal_passes += usize::from(nominal);
            strict_passes += usize::from(strict);
            joint_passes += usize::from(nominal && strict);
            if let Some(reference) = &previous {
                let (equal, mismatches, error) =
                    compare_complete_pcm(3, &label, reference, &capture.raw, reference[0].len());
                assert!(
                    equal,
                    "HighSpeed canonical caller changed complete PCM: {mismatches} samples, max {error}"
                );
            } else {
                previous = Some(capture.raw);
            }
        }
    }
    assert_eq!(captures, PREREQUISITE_CAPTURES);
    let survived = unity_pass && joint_passes == PREREQUISITE_CAPTURES;
    emit(serde_json::json!({
        "stage":"realtime_highspeed_prerequisite_complete","captures":captures,"own_unity_control_pass":unity_pass,
        "actual_nominal_passes":nominal_passes,"unchanged_strict_passes":strict_passes,"joint_passes":joint_passes,
        "candidate_survived_prerequisite":survived,
        "candidate_status":if !unity_pass {"BLOCKED_UNITY_CONTROL"} else if survived {"SURVIVED_PREREQUISITE_ONLY"} else {"REJECTED"},
        "acoustic_failures_remain_real_results":true,"fitted_delay_or_criteria_relaxation":false,
        "matrix_expansion_permitted":survived,"productive_backend_profile_or_guard_change":false,
        "full_domain_tones_dynamic_pitch_ownership_callback_deadline_musical_or_Human_acceptance":false
    }));
}
