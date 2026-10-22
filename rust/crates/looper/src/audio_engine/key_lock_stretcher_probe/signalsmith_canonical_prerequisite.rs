//! One alternative algorithm under the unchanged canonical kick and unity criteria.

use super::*;

#[path = "signalsmith_probe_owner.rs"]
mod owner;
use owner::{admit_arrays, capture_signalsmith, pcm_bytes};

const ARRAY_BUDGET: usize = 256 * 1024 * 1024;

fn reference_scope() {
    emit(serde_json::json!({
        "stage":"signalsmith_frozen_source_scope",
        "stretch_commit":"88061fd4eda56a2353c5d65c74aa3f4ccf76f9da",
        "linear_commit":"7bb1c8f36794d6980080ce817c91f8da8fe56a66",
        "stretch_header_SHA256":"1188667959ac19dd40c0a6abbce694e44705615ec4f0e8db8af0f1cfb4c5dea7",
        "linear_stft_SHA256":"20f3695bb8d162cf4b3f1aacdca477f0c8a301ffebd6fec53b437e9ce623462b",
        "linear_fft_SHA256":"fb55f69ac14cdbac66b9c76053069ff63bc36d18af20d8c028b9acc0aad6516a",
        "profile":"real C++ SignalsmithStretch<float,std::mt19937>; presetDefault(2,44100,false); fresh seed0",
        "q":1.0,"p":[1.0,2.0],"formant_shift":false,"tonality_limit":0,
        "construction_processing_destruction":"one exclusive off-realtime thread",
        "source_timing_or_productive_backend_change":false,
        "ending":"P0; canonical once then Li zeros; one flush Lo+2W with playbackRate1",
        "tail_limit":"full declared capture retained; API has no terminal availability; no internal exhaustion claim"
    }));
}

fn identical_complete_pcm(label: &str, reference: &[Vec<f32>], actual: &[Vec<f32>]) {
    assert_eq!(reference.len(), PROBE_CHANNELS);
    assert_eq!(actual.len(), PROBE_CHANNELS);
    for (a, b) in reference.iter().zip(actual) {
        assert_eq!(a.len(), b.len());
        assert!(
            a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits()),
            "caller changed complete Signalsmith PCM for {label}"
        );
    }
    emit(serde_json::json!({
        "stage":"signalsmith_entire_caller_pcm_parity","label":label,
        "frames":actual[0].len(),"bitwise_equal":true,"mismatched_samples":0,
        "reference_complete_Raw_sha256":pcm_hash(reference),
        "actual_complete_Raw_sha256":pcm_hash(actual),"tolerance":0
    }));
}

fn unity_control_signalsmith(retained_source_bytes: usize) -> bool {
    // Before construction: this fixed fixture and every later sample/scorer array
    // fit the same conservative admission used for the actual capture.
    let input_bound = 2 * PROBE_CHANNELS * UNITY_FRAMES * std::mem::size_of::<f32>();
    admit_arrays(
        retained_source_bytes.checked_add(input_bound).unwrap(),
        UNITY_FRAMES,
        None,
    );
    let mut input = vec![vec![0.0_f32; UNITY_FRAMES]; PROBE_CHANNELS];
    input[0][UNITY_EVENT] = 1.0;
    input[1][UNITY_EVENT] = -0.73;
    assert_eq!(stereo_peak(&input), (UNITY_EVENT, 1.0));
    let resident = retained_source_bytes
        .checked_add(pcm_bytes(&input))
        .unwrap();
    emit(serde_json::json!({
        "stage":"signalsmith_predeclared_unity","frames":UNITY_FRAMES,"event":UNITY_EVENT,
        "q":1.0,"p":1.0,"P":0,"expected_D":5292,"expected_streaming_blocks":[37,93],
        "PCM_error_budget":null,"sample_exact_unity_required":false,
        "fixed_timing_energy_peak_criteria_unchanged":true
    }));
    let mut previous: Option<Vec<Vec<f32>>> = None;
    let mut passes = 0;
    for caller in PROCESS_PATTERNS {
        let capture = capture_signalsmith(&input, 1.0, caller, resident, previous.as_deref());
        let label = format!("Signalsmith_unity_{}", caller.0);
        let metrics = nominal_metrics(&label, &capture, &input, UNITY_EVENT);
        let complete = capture.raw[0].len() >= capture.delay + UNITY_FRAMES;
        let mut max_error = 0.0_f64;
        let mut squared = 0.0;
        if complete {
            for (channel, dry) in input.iter().enumerate() {
                for (wet, dry) in capture.raw[channel][capture.delay..capture.delay + UNITY_FRAMES]
                    .iter()
                    .zip(dry)
                {
                    let error = f64::from(*wet) - f64::from(*dry);
                    max_error = max_error.max(error.abs());
                    squared += error * error;
                }
            }
        }
        let pass = metrics.pass
            && metrics
                .peak_offset
                .is_some_and(|n| n.abs() <= TIMING_FRAMES);
        passes += usize::from(pass);
        emit(serde_json::json!({
            "stage":"signalsmith_unity_result","caller":caller.0,"pass":pass,
            "peak_offset_frames":metrics.peak_offset,"maximum_absolute_PCM_error":complete.then_some(max_error),
            "RMS_PCM_error":complete.then(||(squared/(PROBE_CHANNELS*UNITY_FRAMES) as f64).sqrt()),
            "PCM_error_budget":null,"waveform_identity_or_timbre_qualification":false
        }));
        if let Some(reference) = &previous {
            identical_complete_pcm(&label, reference, &capture.raw);
        } else {
            previous = Some(capture.raw);
        }
    }
    let pass = passes == PROCESS_PATTERNS.len();
    emit(serde_json::json!({
        "stage":"signalsmith_unity_complete","captures":PROCESS_PATTERNS.len(),"passes":passes,"pass":pass,
        "failed_fixed_criteria_are_negative_audio_results":true
    }));
    pass
}

#[test]
fn signalsmith_canonical_unity_coordinate_control() {
    reference_scope();
    let _ = unity_control_signalsmith(0);
}

#[test]
#[ignore = "explicit alternative-algorithm four-capture prerequisite; acoustic rejection remains an actual result"]
fn signalsmith_canonical_kick_prerequisite() {
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
    // Six conservative full input-length float arrays cover source, shifted and
    // canonical fixture construction; their actual capacities are rechecked below.
    let fixture_bound = MAX_INPUT_FRAMES
        .checked_mul(6 * std::mem::size_of::<f32>())
        .unwrap();
    admit_arrays(fixture_bound, 393220, None);
    let source = original_source();
    let source_bytes = pcm_bytes(&source);
    reference_scope();
    emit(serde_json::json!({
        "stage":"signalsmith_predeclared_prerequisite_matrix",
        "source_pcm_sha256":original_hash(&source),"original_source_frames":ORIGINAL_SOURCE_FRAMES,
        "r":SOURCE_RATE,"h":1.0,"key_lock":true,"canonical_warp_already_applied":true,
        "q":1.0,"p":PREREQUISITE_PITCH,"source_events":[0,INTERIOR_SOURCE_EVENT],
        "canonical_events":[0,HISTORY],"canonical_frames":[262148,393220],
        "P":0,"expected_Li":2646,"expected_Lo":2646,"expected_D":5292,
        "expected_window_W":5292,"expected_interval_I":1323,"flush_F":13230,
        "expected_streaming_blocks":[[517,90],[773,90]],
        "expected_Raw_extents":[278024,409096],
        "callers":PROCESS_PATTERNS.map(|(name,pattern)|serde_json::json!({"name":name,"frames":pattern})),
        "captures":PREREQUISITE_CAPTURES,"fixed_timing_budget_frames":TIMING_FRAMES,
        "candidate_survival":"all four actual nominal AND unchanged strict prerequisites plus both own unity controls",
        "product_backend_guards_clock_or_source_trajectory_change":false
    }));
    let unity = unity_control_signalsmith(source_bytes);
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
        let resident = source_bytes
            .checked_add(pcm_bytes(&shifted))
            .and_then(|n| n.checked_add(pcm_bytes(&canonical)))
            .unwrap();
        assert!(resident <= fixture_bound);
        for channel in 0..PROBE_CHANNELS {
            assert_eq!(
                canonical[channel][event].to_bits(),
                source[channel][0].to_bits()
            );
            assert!(canonical[channel][..event].iter().all(|n| *n == 0.0));
        }
        emit(serde_json::json!({
            "stage":"signalsmith_existing_canonical_input_identity","event":event,
            "source_pcm_sha256":original_hash(&source),"canonical_pcm_sha256":pcm_hash(&canonical),
            "canonical_frames":canonical_frames,"unchanged_shared_dry_mapping":true
        }));
        if source_event == INTERIOR_SOURCE_EVENT {
            let live_bound = resident
                .checked_add(
                    2 * PROBE_CHANNELS * LIVE_CAPTURE * std::mem::size_of::<f32>()
                        + 4 * std::mem::size_of::<Vec<f32>>(),
                )
                .unwrap();
            assert!(
                live_bound <= ARRAY_BUDGET,
                "before retained Live comparison allocation"
            );
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
            let live_bytes = resident.checked_add(pcm_bytes(&live)).unwrap();
            assert!(live_bytes <= ARRAY_BUDGET);
            emit(serde_json::json!({
                "stage":"signalsmith_retained_Live_input_parity","prefix_frames":LIVE_CAPTURE,
                "entire_prefix_bitwise_equal":true,"retained_Live_input_sha256":pcm_hash(&live),
                "source_shifted_canonical_Live_array_bytes":live_bytes,
                "comparison_arrays_retired_before_Native_capture":true
            }));
        }
        let mut previous: Option<Vec<Vec<f32>>> = None;
        for caller in PROCESS_PATTERNS {
            let capture = capture_signalsmith(
                &canonical,
                PREREQUISITE_PITCH,
                caller,
                resident,
                previous.as_deref(),
            );
            captures += 1;
            assert!(captures <= PREREQUISITE_CAPTURES);
            let label = format!("Signalsmith_canonical_event{event}_{}", caller.0);
            let nominal = nominal_metrics(&label, &capture, &canonical, event).pass;
            let strict = strict_prerequisite(&label, &capture, &canonical, event);
            nominal_passes += usize::from(nominal);
            strict_passes += usize::from(strict);
            joint_passes += usize::from(nominal && strict);
            if let Some(reference) = &previous {
                identical_complete_pcm(&label, reference, &capture.raw);
            } else {
                previous = Some(capture.raw);
            }
        }
    }
    assert_eq!(captures, PREREQUISITE_CAPTURES);
    let survived = unity && joint_passes == PREREQUISITE_CAPTURES;
    emit(serde_json::json!({
        "stage":"signalsmith_prerequisite_complete","captures":captures,"own_unity_control_pass":unity,
        "actual_nominal_passes":nominal_passes,"unchanged_strict_passes":strict_passes,"joint_passes":joint_passes,
        "candidate_survived_prerequisite":survived,
        "candidate_status":if !unity {"BLOCKED_UNITY_CONTROL"} else if survived {"SURVIVED_PREREQUISITE_ONLY"} else {"REJECTED"},
        "matrix_expansion_permitted":survived,"technical_execution_is_audio_qualification":false,
        "opaque_Native_RAM_callback_deadlines_music_all_rates_pitch_startpolicies_or_Human_proven":false,
        "production_backend_guards_clock_or_default_changed":false
    }));
}
