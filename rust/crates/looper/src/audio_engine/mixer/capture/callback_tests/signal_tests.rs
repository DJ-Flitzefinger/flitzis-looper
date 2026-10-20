//! Actual final-output copy, independent synthetic-signal checks and bounded Rust allocation.
//! No CPAL, device/listening, encoded-file or independent Native/EQ implementation claim.

use super::*;
#[cfg(windows)]
use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
#[cfg(windows)]
use crate::audio_engine::prepared_source::PreparedSourcePermit;
#[cfg(windows)]
use crate::messages::{PreparedStemSet, STEM_COMPONENT_MASK, StemMixMode};
use crate::test_memory::{ThreadAllocations, observe_current_thread};
#[cfg(windows)]
use crate::test_memory::{reset_phase, snapshot};

#[cfg(windows)]
const RATE: u32 = 48_000;
#[cfg(windows)]
const STEM_HZ: f64 = 330.0;
#[cfg(windows)]
const RATE_RATIO: f64 = 1.5;

#[cfg(windows)]
fn stereo_tone(frames: usize, frequency: f64, amplitude: f32) -> SampleBuffer {
    SampleBuffer {
        channels: 2,
        residency: None,
        samples: Arc::from(
            (0..frames)
                .flat_map(|frame| {
                    let phase = frame as f64 * frequency * std::f64::consts::TAU / f64::from(RATE);
                    [
                        amplitude * phase.sin() as f32,
                        amplitude * 0.65 * (phase + 0.37).sin() as f32,
                    ]
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

/// Same tracked Native source/permit publication used by the existing mixer
/// prepared-stem regressions. Only immutable synthetic PCM is constructed here.
#[cfg(windows)]
fn load_native_stem_tone(rig: &mut Rig, key_lock: bool) {
    let state = rig.state();
    let mixer = &mut state.mixer;
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    mixer.set_input_runtime_ownership(ownership.clone());
    // Full mix deliberately has a different frequency: silence or the wrong
    // source cannot pass the independent selected-stem frequency check.
    let reference = stereo_tone(96_000, 880.0, 0.17);
    ownership.publish_test_source(0, &reference, RATE, 1);
    mixer.load_sample(0, reference.clone());
    let placement = ownership.slot_snapshot(0).unwrap();
    let binding = InputPadBinding {
        placement: Some(placement),
        resident: reference.resident_binding(),
        source_address: reference.source_address(),
        sample_count: reference.source_sample_count(),
        channels: reference.channels,
        sample_rate_hz: RATE,
        authority_revision: ownership.authority_revision(0),
        runtime_revision: ownership.runtime_revision(0),
        accepted: mixer.pad_accepted_timing[0],
    };
    let publication = PreparedSourcePermit::for_native_binding_for_test(
        0,
        binding,
        ownership,
        mixer.current_timing_acknowledgements.clone(),
        mixer.prepared_source_epochs[placement.cell().index()].clone(),
    )
    .unwrap();
    let silence = SampleBuffer {
        channels: 2,
        residency: None,
        samples: Arc::from(vec![0.0; reference.samples.len()].into_boxed_slice()),
    };
    let stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x6d; 32]),
        accepted_timing: binding.accepted,
        reference_samples: reference.samples.clone(),
        reference_residency: reference.residency.clone(),
        publication,
        source_version_hash: 0x6d,
        sample_rate_hz: RATE,
        channels: 2,
        frame_count: reference.frame_count(),
        available_mask: STEM_COMPONENT_MASK,
        stems: [
            stereo_tone(96_000, STEM_HZ, 0.2),
            silence.clone(),
            silence.clone(),
            silence,
        ],
    };
    assert!(mixer.publish_prepared_stems_rt(0, stems, &mut state.retirement));
    assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 0x6d));
    mixer.set_speed(RATE_RATIO);
    mixer.set_pad_key_lock(0, key_lock);
    mixer.set_pad_gain(0, -4.0);
    mixer.set_pad_eq(0, -9.0, 2.0, -3.0);
    mixer.set_volume(0.6);
    assert!(mixer.play_sample(0, 0.7));
}

#[cfg(windows)]
fn warm_irregular(rig: &mut Rig, frames: usize) {
    let mut output = [0.0; 1024];
    let mut rendered = 0;
    for requested in [127, 385, 64, 448, 511, 1].into_iter().cycle() {
        let count = requested.min(frames - rendered);
        rig.render(&mut output[..count * 2]);
        rendered += count;
        if rendered == frames {
            break;
        }
    }
}

/// Positive crossings with sub-frame interpolation; no productive source,
/// resampler, Native, DSP, capture or spool helper computes the expected pitch.
#[cfg(windows)]
fn positive_crossing_frequency(stereo: &[f32]) -> f64 {
    let mut first = None;
    let mut last = 0.0;
    let mut crossings = 0;
    for frame in 1..stereo.len() / 2 {
        let before = f64::from(stereo[(frame - 1) * 2]);
        let after = f64::from(stereo[frame * 2]);
        if before <= 0.0 && after > 0.0 {
            let position = (frame - 1) as f64 - before / (after - before);
            first.get_or_insert(position);
            last = position;
            crossings += 1;
        }
    }
    assert!(
        crossings > 10,
        "known sine must contain enough positive crossings"
    );
    (crossings - 1) as f64 * f64::from(RATE) / (last - first.unwrap())
}

#[cfg(windows)]
#[test]
fn prepared_stem_key_lock_rate_and_eq_capture_copies_actual_irregular_final_mix() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    load_native_stem_tone(&mut rig, true);
    warm_irregular(&mut rig, 16_384);
    let voice = rig
        .state()
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert!(
        voice.stretch.wet_render_ready(),
        "actual Native processing must be effective"
    );
    assert!(!voice.stretch.wet_mode_failed());
    let native = voice.stretch.native_identity();
    assert_ne!(native, 0);
    assert!(
        (voice.stretch.native_pitch_scale_observation().unwrap() - 1.0 / RATE_RATIO).abs()
            < 1.0e-12
    );
    assert!((voice.source_playback.tempo_ratio() - RATE_RATIO).abs() < 1.0e-12);
    let source_before = voice.frame_pos;
    assert!(
        source_before > 16_384,
        "nonunity source rate must actually advance"
    );

    let callback_start = rig.state().transport.output_frame();
    let first = callback_start + 3;
    let end = first + 4_099;
    rig.start(&take, first);
    rig.stop(&take, end);
    assert_eq!(rig.drain(), 2);
    let total = 4_117;
    let mut mix = vec![0.0; total * 2];
    let mut rendered = 0;
    for requested in [2, 127, 385, 1, 64, 448, 511].into_iter().cycle() {
        let frames = requested.min(total - rendered);
        rig.render(&mut mix[rendered * 2..(rendered + frames) * 2]);
        rendered += frames;
        if rendered == total {
            break;
        }
    }
    let recorded = &mix[6..(3 + 4_099) * 2];
    assert!(recorded.iter().all(|sample| sample.is_finite()));
    assert!(recorded.iter().any(|sample| sample.abs() > 1.0e-3));
    let actual_hz = positive_crossing_frequency(recorded);
    assert!(
        (actual_hz - STEM_HZ).abs() < 6.0,
        "known selected-stem/Key Lock pitch: {actual_hz}"
    );
    assert!(
        (actual_hz - STEM_HZ * RATE_RATIO).abs() > 100.0,
        "varispeed must not masquerade as Key Lock"
    );
    let voice = rig
        .state()
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert_eq!(voice.stretch.native_identity(), native);
    assert!(voice.stretch.wet_render_ready());
    assert!(voice.frame_pos > source_before + total);
    assert_eq!(
        rig.state().mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!(
        (status.first_frame, status.end_frame),
        (Some(first), Some(end))
    );
    assert_eq!(
        (status.submitted_frames, status.written_frames),
        (4_099, 4_099)
    );
    // Exact bit equality proves this capture tap copied the actual final DSP
    // output. It is deliberately not called an independent Native/EQ oracle.
    assert_eq!(
        take.bits(),
        recorded
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn productive_drain_due_start_stop_and_full_retirement_use_no_rust_allocations() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    load_constant(&mut rig);
    rig.start(&take, 3);
    rig.stop(&take, 11);
    rig.state()
        .scheduler
        .schedule(
            3,
            ScheduledCommand::PlaySample {
                id: 0,
                volume: 0.8,
                received_at_ns: None,
                expected_source_generation: None,
            },
        )
        .unwrap();
    let sink_capacity = rig.state().messages.capacity();
    let sink_address = rig.state().messages.as_ptr();
    assert!(sink_capacity >= 64);
    let (processed, drain_allocations) = observe_current_thread(|| rig.drain());
    assert_eq!(processed, 2);
    assert_eq!(drain_allocations, ThreadAllocations::default());
    rig.fill_retirement();
    assert_eq!(rig.state().retirement.available_retirement_slots(), 0);
    let mut output = [0.0; 28];
    let (_, render_allocations) = observe_current_thread(|| {
        rig.render(&mut output[..4]);
        rig.render(&mut output[4..14]);
        rig.render(&mut output[14..16]);
        rig.render(&mut output[16..]);
        rig.render(&mut []);
    });
    assert_eq!(render_allocations, ThreadAllocations::default());
    assert_eq!(rig.state().messages.capacity(), sink_capacity);
    assert_eq!(rig.state().messages.as_ptr(), sink_address);
    assert_eq!(rig.state().messages.len(), 1);
    assert!(matches!(
        rig.state().messages[0],
        AudioMessage::SampleStarted { id: 0 }
    ));
    let status = take.status();
    assert!(status.rt_sealed && !status.rt_released);
    assert_eq!(status.end_frame, Some(11));
    assert_eq!(
        rig.state().mixer.mix_capture.as_ref().unwrap().session_id(),
        take.id()
    );
    assert!(rig.state().scheduler.is_empty());
    assert_eq!(rig.state().retirement.available_retirement_slots(), 0);
    let expected = 0.75 * 10.0_f32.powf(-6.0206 / 20.0) * 0.8 * 0.5;
    assert!(output[..6].iter().all(|sample| *sample == 0.0));
    assert!(
        output[6..]
            .iter()
            .all(|sample| (*sample - expected).abs() < 1.0e-6)
    );
    rig.close();
    assert!(take.joined().successful_spool());
    assert_eq!(
        take.bits(),
        output[6..22]
            .iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>()
    );
}

#[cfg(windows)]
#[test]
#[ignore = "process-wide Rust peak profile: run this exact filter alone with --ignored --test-threads=1"]
fn capture_default_and_max_pool_rust_resource_profile() {
    // First observe the real Native preparation route separately. These are
    // System/Rust requests; Rubber Band's C++ allocations and process RSS are
    // explicitly outside this allocator's observation.
    let native_baseline = reset_phase();
    let mut rig = Rig::new(false);
    load_native_stem_tone(&mut rig, true);
    warm_irregular(&mut rig, 16_384);
    let native_metrics = snapshot(native_baseline);
    let voice = rig
        .state()
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active)
        .unwrap();
    assert!(voice.stretch.wet_render_ready());
    assert_ne!(voice.stretch.native_identity(), 0);
    assert!(native_metrics.1 > 0 && native_metrics.2 > 0);
    rig.close();
    drop(rig); // All owned Native/preparation/retirement workers are terminal.

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let parent = workspace.join("test-artifacts/record-capture-20261020");
    fs::create_dir_all(&parent).unwrap();
    for (name, block_frames, block_count, budget) in [
        // Exact current prepare_mix_capture public defaults (stereo output).
        ("default", 4096, 32, 8 * 1024 * 1024),
        ("max-pool", 65_536, 128, 64 * 1024 * 1024),
    ] {
        let config = CaptureConfig {
            sample_rate_hz: RATE,
            channels: 2,
            block_frames,
            block_count,
            max_pool_pcm_bytes: budget,
        };
        let directory = tempfile::Builder::new()
            .prefix("resource-")
            .tempdir_in(&parent)
            .unwrap();
        fs::write(directory.path().join("OWNER.md"),
            b"Owned isolated Rust allocator mix-capture profile: default and admitted maximum synthetic pools, actual nonempty spool worker and actual Native preparation separately. No device/model/Human input or C++/RSS claim. Parent Take teardown retains unsettled data and waits for actual RT release/writer join.\n").unwrap();
        let path = directory.path().join(format!("{name}.capture"));
        let block_bytes = block_frames * 2 * size_of::<f32>();
        let pcm_bytes = block_bytes * block_count;
        // Synthetic caller input is fixed before the capture allocation phase;
        // it is separate from the admitted recorder pool and writer scratch.
        let input = vec![0.125_f32; block_frames * 2];
        let baseline = reset_phase();
        let (control, rt) = prepare(&path, config).unwrap();
        let mut take = Take {
            control,
            publication: Arc::new(CapturePublication::new(rt)),
            path,
            directory: Some(directory),
        };
        let deadline = Instant::now() + WAIT;
        while !take.status().writer_ready {
            assert!(
                Instant::now() < deadline,
                "resource writer readiness timeout"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let measured = snapshot(baseline);
        let status = take.status();
        assert_eq!(status.pool_pcm_bytes, pcm_bytes);
        assert_eq!(status.writer_scratch_bytes, block_bytes);
        assert_eq!(status.allocated_blocks, block_count);
        assert_eq!(fs::metadata(&take.path).unwrap().len(), SPOOL_HEADER_BYTES);
        // Two pointer rings, boxed Block records, Shared/control/thread/path
        // metadata are separate from admitted PCM. This explicit allowance is
        // bounded by block count; it is not hidden inside max_pool_pcm_bytes.
        let overhead_bound = block_count * 256 + 64 * 1024;
        assert!(
            measured.1 >= pcm_bytes + block_bytes,
            "pool and scratch must really exist: {measured:?}"
        );
        assert!(
            measured.1 <= pcm_bytes + block_bytes + overhead_bound,
            "capture Rust peak exceeds PCM + scratch + bounded overhead: {measured:?}"
        );
        assert_eq!(
            measured.2, block_bytes,
            "largest actual Rust allocation is one fixed block/scratch"
        );
        let mut rt = take.publication.claim().unwrap();
        assert!(rt.start(take.id(), 0, RATE, 2));
        // At most half the actual pool is submitted, so this finite throughput
        // sample cannot depend on the writer keeping up with an unbounded
        // synthetic producer. Overflow/slow-writer failures have separate tests.
        let submitted_blocks = block_count / 2;
        let frames = block_frames as u64 * submitted_blocks as u64;
        let bytes = frames * 2 * size_of::<f32>() as u64;
        let copy_started = Instant::now();
        for block in 0..submitted_blocks {
            assert!(rt.append_segment(block as u64 * block_frames as u64, &input));
        }
        assert!(rt.stop(take.id(), frames));
        let seal_elapsed = copy_started.elapsed();
        let drain_started = Instant::now();
        drop(rt); // Actual original Box releases on this profile's control thread.
        let terminal = take.joined();
        let drain_close_elapsed = drain_started.elapsed();
        let write_elapsed = copy_started.elapsed();
        assert!(terminal.rt_released && terminal.worker_closed && terminal.worker_joined);
        assert!(terminal.successful_spool());
        assert_eq!(terminal.failure, None);
        assert_eq!(
            (terminal.submitted_frames, terminal.written_frames),
            (frames, frames)
        );
        assert_eq!(
            (terminal.first_frame, terminal.end_frame),
            (Some(0), Some(frames))
        );
        assert_eq!(
            fs::metadata(&take.path).unwrap().len(),
            SPOOL_HEADER_BYTES + bytes
        );
        let operation_metrics = snapshot(baseline);
        assert!(
            operation_metrics.1 <= pcm_bytes + block_bytes + overhead_bound,
            "whole finite writer cycle exceeds the declared capture allocation bound: {operation_metrics:?}"
        );
        drop(take); // Reuse parent's actual close/join/retained-failure fixture policy.
        let returned = snapshot(baseline);
        assert!(
            returned.0 <= baseline + 16 * 1024,
            "capture Rust allocations not returned: {returned:?}"
        );
        println!(
            "{}",
            serde_json::json!({
                "case": name, "pid": std::process::id(), "config": {
                    "rate": RATE, "channels": 2, "block_frames": block_frames,
                    "block_count": block_count, "pcm_admission_bytes": budget,
                },
                "pool_pcm_bytes": pcm_bytes, "scratch_bytes": block_bytes,
                "ring_box_shared_thread_path_overhead_bound": overhead_bound,
                "spool_header_file_bytes": SPOOL_HEADER_BYTES,
                "rust_prepare_live_peak_largest": measured,
                "rust_whole_cycle_live_peak_largest": operation_metrics,
                "rust_after_release_live_peak_largest": returned,
                "native_preparation_rust_live_peak_largest": native_metrics,
                "written_frames": frames, "written_bytes": bytes,
                "copy_and_seal_seconds": seal_elapsed.as_secs_f64(),
                "drain_close_join_seconds": drain_close_elapsed.as_secs_f64(),
                "copy_through_closed_seconds": write_elapsed.as_secs_f64(),
                "observed_written_mib_per_second": bytes as f64 / (1024.0 * 1024.0) / write_elapsed.as_secs_f64(),
                "rt_released": terminal.rt_released, "worker_joined": terminal.worker_joined,
                "scope": "isolated actual Rust/System allocations and finite synthetic writer throughput under current concurrent host load; actual Native wet preparation separately; no sustainable CPAL rate/C++/RSS/hard CPU/device/hearing or whole recording acceptance",
            })
        );
    }
}
