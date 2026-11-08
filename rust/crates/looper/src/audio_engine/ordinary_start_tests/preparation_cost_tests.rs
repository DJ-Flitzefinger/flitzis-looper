//! Genuine resident-PCM original groups; no delayed worker gates or device acquisition.
use super::*;
use crate::audio_engine::pitch_runtime_tests::publish_tone_with_rate;
use crate::audio_engine::preparation_observation::CollectionScope;
use std::os::windows::io::{AsRawHandle, IntoRawHandle};
use std::sync::Arc;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

fn fixture_96(count: usize) -> Fixture {
    let mut fixture = Fixture::new_format(true, 96_000, 2);
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    for id in 1..count {
        publish_tone_with_rate(&fixture.control, id, sample.clone(), true, 96_000);
    }
    adopt_current_source_ids(&mut fixture, true, &(0..count).collect::<Vec<_>>());
    refresh_actual_runtime_bindings(&fixture);
    fixture.mixer.set_bpm_lock(true);
    fixture.mixer.set_master_bpm(120.0);
    for id in 0..count {
        fixture.mixer.set_pad_loop_region(id, 0.1, Some(2.1));
        fixture.mixer.set_pad_bpm(id, Some(150.0));
    }
    fixture.feedback.reserve(512);
    fixture
}

/// Time only the actual scheduled mixer; output storage is already on the stack.
fn render96(fixture: &mut Fixture) -> ([f32; 192], u128) {
    let mut output = [0.0; 192];
    let frame = fixture.transport.output_frame();
    let begin = Instant::now();
    render_scheduled_audio(
        &mut fixture.mixer,
        &mut fixture.scheduler,
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        frame,
        2,
        &mut fixture.transport,
        &mut fixture.feedback,
        &mut fixture.retirement,
    );
    let ns = begin.elapsed().as_nanos();
    fixture.transport.advance_by_rendered_frames(96);
    (output, ns)
}

struct Cohort {
    pcm: Vec<u32>,
    phase: Vec<(usize, u64, u64, u64, usize, usize)>,
    result: serde_json::Value,
}

fn cohort(fixture: &mut Fixture, count: usize, index: usize, mode: usize) -> Cohort {
    fixture.refresh_clock();
    let cpu_before = crate::audio_engine::c3_process_metrics::snapshot();
    let collection = CollectionScope::begin();
    let starts = playback_group(fixture, count);
    let observations = collection.finish();
    assert_eq!(observations.len(), count);
    let sequences: Vec<_> = starts.iter().map(CapturedPadStart::sequence).collect();
    wait_ready(fixture, &sequences);
    let deadline = Instant::now() + WORKER_WAIT;
    while observations
        .iter()
        .any(|o| o.snapshot().ready_after_ns == 0)
    {
        assert!(Instant::now() < deadline, "ready publication observation");
        std::thread::sleep(Duration::from_millis(1));
    }
    let rows: Vec<_> = observations.iter().map(|o| o.snapshot()).collect();
    assert_eq!(
        rows.iter().map(|r| r.sequence).collect::<Vec<_>>(),
        sequences
    );
    assert!(rows.iter().all(|r| r.enqueue_before_ns > 0
        && r.enqueue_after_ns >= r.enqueue_before_ns
        && r.start_ns >= r.enqueue_before_ns
        && r.end_ns >= r.start_ns
        && r.ready_before_ns >= r.end_ns
        && r.ready_after_ns >= r.ready_before_ns
        && r.polled_ns >= r.ready_before_ns
        && r.native != 0
        && r.cpu_end_ns >= r.cpu_start_ns));
    let mut natives: Vec<_> = rows.iter().map(|r| r.native).collect();
    natives.sort_unstable();
    natives.dedup();
    assert_eq!(natives.len(), count);
    let cpu_after = crate::audio_engine::c3_process_metrics::snapshot();
    let render_start = fixture.transport.output_frame();
    let mut pcm = Vec::new();
    let mut render_ns = Vec::new();
    // Consume real requests until application, then preserve 4128 complete own-E frames.
    while starts.iter().any(|start| start.effective_frame().is_none()) {
        assert!(fixture.transport.output_frame() - render_start <= 16_384);
        let (block, ns) = render96(fixture);
        pcm.extend(block);
        render_ns.push(ns);
    }
    let effect = starts[0].effective_frame().unwrap();
    assert!(
        starts
            .iter()
            .all(|s| s.state() == "applied" && s.effective_frame() == Some(effect))
    );
    let generations: Vec<_> = (0..count).map(|id| fixture.voice(id).generation).collect();
    let offset = usize::try_from(effect - render_start).unwrap();
    while pcm.len() < (offset + 43 * 96) * 2 {
        let (block, ns) = render96(fixture);
        pcm.extend(block);
        render_ns.push(ns);
    }
    let mut phase = Vec::new();
    for (id, row) in rows.iter().enumerate() {
        let voice = fixture.voice(id);
        assert_eq!(voice.generation, generations[id]);
        assert_eq!(voice.stretch.native_identity(), row.native);
        assert_eq!(
            voice.stretch.adopted_request_id(),
            Some(starts[id].sequence())
        );
        assert_eq!(voice.source_playback.tempo_ratio(), 0.8);
        assert!(voice.stretch.wet_current_source_output_ready());
        let samples = fixture.control.engine.sample_cache.lock().unwrap();
        assert!(
            voice
                .sample
                .as_ref()
                .unwrap()
                .same_source(samples[id].as_ref().unwrap())
        );
        let ownership = &fixture.control.engine.input_runtime_ownership;
        assert_eq!(
            ownership.callback_feedback_key(id),
            ownership.slot_snapshot(id).unwrap().content_key()
        );
        let position = voice.source_playback.position();
        let fifo = voice.stretch.pending_fifo_frames();
        phase.push((
            position.frame,
            position.fraction.to_bits(),
            voice.source_playback.rate_target().to_bits(),
            voice.source_playback.tempo_ratio().to_bits(),
            fifo.0,
            fifo.1,
        ));
    }
    assert!(pcm.iter().all(|v| v.is_finite()));
    assert!(pcm.iter().any(|v| v.abs() > 0.001));
    let cpu = |v: &serde_json::Value| {
        v["kernel_cpu_ns"].as_u64().unwrap() + v["user_cpu_ns"].as_u64().unwrap()
    };
    let result = serde_json::json!({"pads":count,"cohort":index,"workers":mode,"rate":96000,"channels":2,"frames":96,
        "source":"resident PCM, new original Native per member, no selected endpoint/gate",
        "preparation_control_poll_process_cpu_ns":cpu(&cpu_after)-cpu(&cpu_before),
        "process_before":cpu_before,"process_after":cpu_after,"render_start":render_start,
        "E":effect,"T":starts.iter().map(CapturedPadStart::original_frame).collect::<Vec<_>>(),
        "rows":rows,"render_ns":render_ns,"phase":phase});
    let pcm = pcm[offset * 2..(offset + 43 * 96) * 2]
        .iter()
        .map(|v| v.to_bits())
        .collect();
    for id in 0..count {
        fixture.control.engine.stop_sample(id).unwrap();
    }
    fixture.drain();
    render96(fixture);
    fixture.mixer.pitch_runtime.as_mut().unwrap().poll();
    assert_eq!(
        fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .unused_action_capacity(),
        (64, 64)
    );
    assert!(fixture.scheduler.is_empty());
    assert!((0..count).all(|id| !fixture.mixer.voices.iter().any(|v| v.is_playing_sample(id))));
    fixture.feedback.clear();
    Cohort { pcm, phase, result }
}

fn close_handles(handles: Vec<std::os::windows::io::OwnedHandle>) {
    for handle in handles {
        // The duplicate identifies the exact worker kernel object across ID reuse.
        assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }, 0);
        assert_ne!(unsafe { CloseHandle(handle.into_raw_handle()) }, 0);
    }
}

#[test]
#[ignore = "optimized original2/4/6 preparation cost diagnostic"]
fn original_group_preparation_cost_at_96k_stereo96() {
    for count in [2, 4, 6] {
        let mut fixture =
            crate::audio_engine::preparation_observation::with_workers(1, || fixture_96(count));
        let handles = fixture.mixer.thread_handles_for_test();
        let readers: Vec<_> = fixture
            .control
            .engine
            .sample_cache
            .lock()
            .unwrap()
            .iter()
            .take(count)
            .map(|s| Arc::downgrade(&s.as_ref().unwrap().samples))
            .collect();
        for index in 0..8 {
            eprintln!(
                "PREPARATION COST {}",
                cohort(&mut fixture, count, index, 1).result
            );
        }
        drop(fixture);
        close_handles(handles);
        assert!(readers.iter().all(|r| r.upgrade().is_none()));
        eprintln!(
            "PREPARATION COST closed pads={count}; all mixer worker handles and current source readers END"
        );
    }
}

#[test]
#[ignore = "counterbalanced optimized one/two-worker original groups and full PCM"]
fn original_group_preparation_two_worker_pair() {
    paired_groups(100, true);
}

#[test]
fn preparation_pool_smoke_matches_full_pcm() {
    paired_groups(2, false);
}

fn paired_groups(cohorts: usize, require_parallel: bool) {
    use crate::audio_engine::preparation_observation::with_workers;
    use sha2::{Digest, Sha256};
    for count in [2, 4, 6] {
        let mut fixtures = [
            with_workers(1, || fixture_96(count)),
            with_workers(2, || fixture_96(count)),
        ];
        let handles: Vec<_> = fixtures
            .iter()
            .flat_map(|f| f.mixer.thread_handles_for_test())
            .collect();
        assert_eq!(
            handles.len(),
            5,
            "one KeyLock worker per mixer plus the complete1/2action pool"
        );
        let readers: Vec<_> = fixtures
            .iter()
            .flat_map(|f| {
                f.control
                    .engine
                    .sample_cache
                    .lock()
                    .unwrap()
                    .iter()
                    .take(count)
                    .map(|s| Arc::downgrade(&s.as_ref().unwrap().samples))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut bad = 0;
        let mut first_bad = None;
        let mut parallel_cohorts = 0;
        let mut hashes = [Sha256::new(), Sha256::new()];
        let whole_cpu_before = crate::audio_engine::c3_process_metrics::snapshot();
        for index in 0..cohorts {
            let order = if index % 2 == 0 { [0, 1] } else { [1, 0] };
            let mut pair: [Option<Cohort>; 2] = [None, None];
            for side in order {
                let observation = cohort(&mut fixtures[side], count, index, side + 1);
                for bits in &observation.pcm {
                    hashes[side].update(bits.to_le_bytes());
                }
                eprintln!("PREPARATION PAIR {}", observation.result);
                pair[side] = Some(observation);
            }
            let [reference, candidate] = pair.map(Option::unwrap);
            let rows = candidate.result["rows"].as_array().unwrap();
            if rows.iter().enumerate().any(|(i, a)| {
                rows.iter().skip(i + 1).any(|b| {
                    a["worker_id"] != b["worker_id"]
                        && a["start_ns"].as_u64().unwrap() < b["end_ns"].as_u64().unwrap()
                        && b["start_ns"].as_u64().unwrap() < a["end_ns"].as_u64().unwrap()
                })
            }) {
                parallel_cohorts += 1;
            }
            if reference.pcm != candidate.pcm
                || reference.phase != candidate.phase
                || reference.result["E"] != candidate.result["E"]
                || reference.result["T"] != candidate.result["T"]
            {
                bad += 1;
                if first_bad.is_none() {
                    let mismatch = reference
                        .pcm
                        .iter()
                        .zip(&candidate.pcm)
                        .filter(|(a, b)| a != b)
                        .count();
                    first_bad = Some(serde_json::json!({"cohort":index,"pcm_mismatches":mismatch,
                        "reference":reference.result,"candidate":candidate.result}));
                }
            }
        }
        let whole_cpu_after = crate::audio_engine::c3_process_metrics::snapshot();
        drop(fixtures);
        close_handles(handles);
        assert!(readers.iter().all(|r| r.upgrade().is_none()));
        let hashes = hashes.map(|h| format!("{:x}", h.finalize()));
        eprintln!(
            "PREPARATION PAIR closed {}",
            serde_json::json!({"pads":count,"cohorts_per_mode":cohorts,
            "frames_compared_per_pair":4128,"pcm_hashes":hashes,"bad_pairs":bad,"first_bad":first_bad,
            "parallel_cohorts":parallel_cohorts,"whole_process_before":whole_cpu_before,"whole_process_after":whole_cpu_after,
            "all_mixer_workers_and_current_source_readers_end":true})
        );
        if require_parallel {
            assert!(
                parallel_cohorts > 0,
                "actual independent preparation intervals must overlap"
            );
        }
        assert_eq!(
            bad, 0,
            "whole own-E PCM, trajectory, FIFO and original common E/T must match"
        );
        assert_eq!(hashes[0], hashes[1]);
    }
}

#[path = "preparation_pool_tests.rs"]
mod pool_guards;
