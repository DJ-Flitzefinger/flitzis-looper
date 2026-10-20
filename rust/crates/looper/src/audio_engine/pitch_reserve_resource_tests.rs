//! Opt-in Native endpoint-reserve costs, isolated from App/device and acceptance gates.
//!
//! Each child retains independent actual Native/adapter bundles for one count/profile.
//! One generated immutable stereo source is shared deliberately, so its PCM allocation
//! and source-authority bookkeeping are present before the memory baseline.

#![cfg(all(test, windows))]

use super::c3_process_metrics;
use super::constant_timing::CurrentTimingAcknowledgements;
use super::input_runtime_binding::InputRuntimeOwnership;
use super::native_history_permit::NativeHistoryContext;
use super::pitch_intent::PitchIntent;
use super::prepared_native_history::{NativeAdapterState, NativeHistoryRequest};
use super::productive_source_history::ProductiveSourceBinding;
use super::rubberband_backend::RubberBandLiveShifter;
use super::source_playback::SourcePlayback;
use super::source_reader::{
    ExplicitSeekMode, FrameRange, SourceReadPlan, StemRenderSelection, StemTransition,
    tap_observation_for_test,
};
use crate::messages::SampleBuffer;
use serde_json::json;
use std::sync::{Arc, atomic::AtomicU64};
use std::time::Instant;

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const SOURCE_FRAMES: usize = 8192;
const REGION: FrameRange = FrameRange {
    start: 137,
    end: 8077,
};
const CHILD_COUNT: &str = "FLITZI_PITCH_RESERVE_RESOURCE_COUNT";
const CHILD_PROFILE: &str = "FLITZI_PITCH_RESERVE_RESOURCE_PROFILE";
const TEST: &str = "audio_engine::pitch_reserve_resource_tests::pitch_reserve_unique_native_endpoint_resource_probe";

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    base: i32,
    extra: i32,
    ratio: f64,
}

const PROFILES: [Profile; 4] = [
    Profile {
        name: "unity_native_calibration",
        base: 0,
        extra: 0,
        ratio: 1.0,
    },
    Profile {
        name: "saved_lock_rate_073",
        base: 0,
        extra: 0,
        ratio: 0.73,
    },
    Profile {
        name: "lowest_supported_pitch_rate",
        base: -5,
        extra: -18,
        ratio: 2.0,
    },
    Profile {
        name: "highest_supported_pitch_rate",
        base: 6,
        extra: 18,
        ratio: 0.5,
    },
];

#[test]
#[ignore = "explicit isolated Windows Native reserve feasibility probe; no device/App/P6 claim"]
fn pitch_reserve_unique_native_endpoint_resource_probe() {
    if let Some(count) = std::env::var_os(CHILD_COUNT) {
        let count = count.to_str().unwrap().parse::<usize>().unwrap();
        assert!([1, 8, 32, 64].contains(&count));
        let profile = std::env::var(CHILD_PROFILE)
            .unwrap()
            .parse::<usize>()
            .unwrap();
        measure(count, PROFILES[profile]);
        return;
    }

    for profile in 0..PROFILES.len() {
        for count in [1, 8, 32, 64] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    TEST,
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_COUNT, count.to_string())
                .env(CHILD_PROFILE, profile.to_string())
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success(),
                "Native resource child failed:\n{stdout}\n{stderr}"
            );
            assert!(stdout.contains("1 passed; 0 failed"));
            assert!(stdout.contains("pitch_reserve_resource_measurement"));
            print!("{stdout}");
            if !stderr.is_empty() {
                eprint!("{stderr}");
            }
        }
    }
}

fn source() -> SampleBuffer {
    let samples = (0..SOURCE_FRAMES)
        .flat_map(|frame| {
            let phase = frame as f64 / f64::from(RATE);
            let left = (phase * 173.0 * std::f64::consts::TAU).sin() as f32 * 0.23
                + (phase * 947.0 * std::f64::consts::TAU).cos() as f32 * 0.07;
            let right = (phase * 271.0 * std::f64::consts::TAU).cos() as f32 * -0.19
                + (phase * 613.0 * std::f64::consts::TAU).sin() as f32 * 0.11;
            [left, right]
        })
        .collect::<Vec<_>>();
    SampleBuffer {
        samples: samples.into(),
        channels: CHANNELS,
        residency: None,
    }
}

fn request(
    source: &SampleBuffer,
    ownership: &Arc<InputRuntimeOwnership>,
    acknowledgements: &Arc<CurrentTimingAcknowledgements>,
    epoch: &Arc<AtomicU64>,
    profile: Profile,
    request_id: u64,
) -> NativeHistoryRequest {
    let mut playback = SourcePlayback::new(REGION.start, ExplicitSeekMode::Normal, profile.ratio);
    playback.configure(SOURCE_FRAMES, REGION);
    let permit = NativeHistoryContext {
        id: 0,
        ownership,
        acknowledgements,
        preparation_epoch: epoch,
    }
    .capture(source, RATE, None)
    .unwrap();
    NativeHistoryRequest {
        pitch_intent: PitchIntent::new(profile.base, profile.extra).unwrap(),
        key_lock: true,
        sample: source.clone(),
        stems: None,
        permit,
        coverage: None,
        binding: ProductiveSourceBinding::new(source, RATE, None),
        playback,
        plan: SourceReadPlan {
            channels: CHANNELS,
            sample_frames: SOURCE_FRAMES,
            frame_pos: REGION.start,
            loop_region: REGION,
            loop_period: None,
            seek_mode: ExplicitSeekMode::Normal,
            selection: StemRenderSelection::full_mix(),
            transition: StemTransition::default(),
        },
        actual_feed: None,
        target_output_frame: 0,
        request_id,
        epoch: 1,
    }
}

fn measure(count: usize, profile: Profile) {
    let source = source();
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_test_source(0, &source, RATE, 1);
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    let epoch = Arc::new(AtomicU64::new(1));
    let expected_pitch =
        2.0_f64.powf(f64::from(profile.base + profile.extra) / 12.0) / profile.ratio;
    let mut handles = Vec::with_capacity(count);
    let mut bundles = Vec::with_capacity(count);
    let mut addresses = Vec::with_capacity(count);
    let mut constructor_ns = Vec::with_capacity(count);
    let mut adapter_ns = Vec::with_capacity(count);
    let mut preparation_ns = Vec::with_capacity(count);
    let mut bundle_counts = Vec::with_capacity(count);
    let baseline = c3_process_metrics::snapshot();

    let constructor_start = Instant::now();
    for _ in 0..count {
        let start = Instant::now();
        let native = RubberBandLiveShifter::new(RATE, CHANNELS).unwrap();
        constructor_ns.push(start.elapsed().as_nanos());
        let address = native.state_address();
        assert_ne!(address, 0);
        assert!(
            !addresses.contains(&address),
            "every retained Native handle is distinct"
        );
        addresses.push(address);
        handles.push(native);
    }
    let constructor_elapsed_ns = constructor_start.elapsed().as_nanos();
    let native_constructed = c3_process_metrics::snapshot();

    let adapter_start = Instant::now();
    for native in handles.drain(..) {
        let start = Instant::now();
        bundles.push(Box::new(NativeAdapterState::new(Some(native), CHANNELS)));
        adapter_ns.push(start.elapsed().as_nanos());
    }
    let adapter_elapsed_ns = adapter_start.elapsed().as_nanos();
    let adapters_constructed = c3_process_metrics::snapshot();

    let preparation_start = Instant::now();
    for (index, bundle) in bundles.iter_mut().enumerate() {
        let request = request(
            &source,
            &ownership,
            &acknowledgements,
            &epoch,
            profile,
            index as u64 + 1,
        );
        let endpoint = request.playback;
        let start = Instant::now();
        bundle.prepare_source_at_endpoint(request).unwrap();
        preparation_ns.push(start.elapsed().as_nanos());
        // Exact-pitch setup updates the handle's nominal delay. Query the bound
        // from that selected Native state, rather than its constructor's unity state.
        let history_frames = bundle
            .rubberband
            .as_ref()
            .unwrap()
            .source_history_frames(expected_pitch)
            .unwrap();
        let taps = tap_observation_for_test();
        assert!(
            taps.left_reads > 0,
            "preparation must process the generated source PCM"
        );
        assert_eq!(taps.missing_reads, 0);
        assert!(taps.min_frame.is_some_and(|frame| frame >= REGION.start));
        assert!(taps.max_frame.is_some_and(|frame| frame < REGION.end));
        assert!(bundle.active && bundle.used && !bundle.dirty);
        assert_eq!(bundle.pitch_scale.to_bits(), expected_pitch.to_bits());
        assert_eq!(bundle.primed_output_frames, history_frames);
        let native = bundle.rubberband.as_ref().unwrap();
        assert_eq!(native.state_address(), addresses[index]);
        let retained = bundle.source.as_ref().unwrap();
        assert_eq!(retained.request_id, index as u64 + 1);
        assert!(retained.playback.matches_exact(&endpoint));
        assert!(Arc::ptr_eq(&retained.sample.samples, &source.samples));
        bundle_counts.push(json!({
            "history_output_frames": history_frames,
            "source_left_reads": taps.left_reads,
            "source_right_reads": taps.right_reads,
            "source_missing_reads": taps.missing_reads,
            "input_fifo_frames": bundle.input_fifo[0].len(),
            "output_fifo_frames": bundle.output_fifo[0].len(),
            "block_frames": native.block_size(),
            "native_pitch_scale": native.pitch_scale(),
        }));
    }
    let preparation_elapsed_ns = preparation_start.elapsed().as_nanos();
    std::hint::black_box(&bundles);
    let prepared = c3_process_metrics::snapshot();
    assert_eq!(bundles.len(), count);
    assert_eq!(addresses.len(), count);
    println!(
        "{}",
        json!({
            "measurement": "pitch_reserve_resource_measurement",
            "scope": "single-source unique Native endpoint reserves; feasibility only, no latency/resource/P6 acceptance",
            "rate_hz": RATE, "channels": CHANNELS,
            "profile": profile.name, "base": profile.base, "extra": profile.extra,
            "source_rate": profile.ratio, "requested_native_pitch": expected_pitch,
            "retained_unique_native_states": count, "retained_adapter_bundles": bundles.len(),
            "shared_source_pcm_bytes": source.samples.len() * size_of::<f32>(),
            "loop_start_frame": REGION.start, "loop_end_frame": REGION.end,
            "baseline": baseline, "native_constructed": native_constructed,
            "adapters_constructed": adapters_constructed, "prepared_and_retained": prepared,
            "native_memory_delta": c3_process_metrics::delta(&baseline, &native_constructed),
            "adapter_memory_delta": c3_process_metrics::delta(&native_constructed, &adapters_constructed),
            "preparation_memory_delta": c3_process_metrics::delta(&adapters_constructed, &prepared),
            "total_memory_delta": c3_process_metrics::delta(&baseline, &prepared),
            "constructor_phase_wall_ns": constructor_elapsed_ns,
            "adapter_phase_wall_ns": adapter_elapsed_ns,
            "preparation_phase_wall_ns": preparation_elapsed_ns,
            "native_constructor_call_total_ns": constructor_ns.iter().sum::<u128>(),
            "adapter_constructor_call_total_ns": adapter_ns.iter().sum::<u128>(),
            "exact_pitch_and_source_history_call_total_ns": preparation_ns.iter().sum::<u128>(),
            "constructor_ns_per_bundle": constructor_ns,
            "adapter_ns_per_bundle": adapter_ns,
            "exact_pitch_and_source_history_ns_per_bundle": preparation_ns,
            "bundle_raw_counts": bundle_counts,
        })
    );
    drop(bundles);
    let released = c3_process_metrics::snapshot();
    println!(
        "{}",
        json!({
            "measurement": "pitch_reserve_resource_release",
            "profile": profile.name, "released_unique_native_states": count,
            "after_release": released,
            "delta_from_retained": c3_process_metrics::delta(&prepared, &released),
            "delta_from_baseline": c3_process_metrics::delta(&baseline, &released),
        })
    );
}
