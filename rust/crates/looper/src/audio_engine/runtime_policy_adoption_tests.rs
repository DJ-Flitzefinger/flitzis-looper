//! Public Native runtime publication through the real queue, scheduler and reader.
//! Generated PCM/address/lifetime evidence only; no device or musical acceptance.

use super::*;
use crate::audio_engine::audio_stream::{
    ScheduledExecution, drain_parameter_messages, execute_scheduled_command,
};
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, create_audio_buffer_retirement_with_callback_paused,
};
use crate::audio_engine::input_mapping::{
    PitchPadProjection, RuntimePolicyError, RuntimePolicySnapshot,
};
use crate::audio_engine::scheduler::{ScheduledCommand, with_input_order_exhaustion_for_test};
use crate::audio_engine::source_playback::{FractionalSourcePosition, SourcePlayback};
use crate::audio_engine::source_reader::{FrameRange, SourceLoopDomain};
use crate::audio_engine::voice_slot::ExplicitSeekMode;
use std::sync::{Arc, mpsc};

fn prepared() -> Path {
    let mut path = Path::new(false);
    path.publish(Some(wire(false, false, 1)), true, None);
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    rows[0].key_lock = false;
    rows[1].key_lock = false;
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_pitch_runtime_state(rows)
        .unwrap();
    assert_eq!(path.start().state(), "applied");
    path.render(17);
    path
}

fn fixed_target() -> f64 {
    // The original manual-BPM route converts BPM back to a period before
    // dividing by the master. Retain that operation order and every bit.
    (60.0 / (60.0 / SOURCE)) / MASTER
}

fn request(path: &Path) -> Arc<RuntimePolicySnapshot> {
    path.source
        .engine
        .request_input_runtime_policy_adoption()
        .unwrap();
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .runtime_policy_snapshot()
        .unwrap()
        .unwrap()
}

fn settle_parameters(path: &mut Path) {
    let (_producer, mut consumer) = rtrb::RingBuffer::new(1);
    drain_parameter_messages(&mut consumer, &mut path.source.mixer, &mut path.timeline);
}

fn service(path: &mut Path) {
    path.drain();
    settle_parameters(path);
    path.render(0);
}

fn scalar_state(path: &Path) -> Vec<(usize, u64, SourcePlayback, SourceSyncRequest)> {
    path.source
        .mixer
        .voices
        .iter()
        .filter(|voice| voice.active)
        .map(|voice| {
            (
                voice.sample_id,
                voice.generation,
                voice.source_playback,
                voice.sync_request,
            )
        })
        .collect()
}

fn unchanged(path: &Path, before: &[(usize, u64, SourcePlayback, SourceSyncRequest)]) {
    let after = scalar_state(path);
    assert_eq!(after.len(), before.len());
    for (a, b) in after.iter().zip(before) {
        assert_eq!((a.0, a.1, a.3), (b.0, b.1, b.3));
        assert!(a.2.matches_exact(&b.2));
    }
}

#[test]
fn live_fixed_off_publication_uses_both_drains_actual_e_and_one_existing_step() {
    let mut path = prepared();
    let original = path.voice(0).source_playback;
    let generation = path.voice(0).generation;
    let master = path.timeline;
    let mut history = SourceFeedSnapshot::new();
    assert!(path.voice(0).source_feed_history.capture_into(&mut history));
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    path.drain();
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    assert!(path.voice(0).source_playback.matches_exact(&original));
    settle_parameters(&mut path);
    path.render(0);
    let observed = snapshot.observation().unwrap();
    assert_eq!(
        (observed.state, observed.execution_frame),
        ("applied", Some(17))
    );
    assert_eq!(path.voice(0).generation, generation);
    assert!(path.timeline.matches_clock_epoch(&master));
    let mut expected = original;
    assert!(expected.set_target(fixed_target()));
    expected.chunk(1024);
    assert!(path.voice(0).source_playback.matches_exact(&expected));
    assert_eq!(path.feedback(0)["adoption"]["target_policy_revision"], 2);
    assert_eq!(path.feedback(0)["settled"], false);
    let mut retained = SourceFeedSnapshot::new();
    assert!(
        path.voice(0)
            .source_feed_history
            .capture_into(&mut retained)
    );
    assert_eq!(history.retained_range(), retained.retained_range());
    for (a, b) in history.spans().zip(retained.spans()) {
        assert!(a.playback.matches_exact(&b.playback));
    }
    path.render(2049);
    assert_eq!(path.feedback(0)["settled"], true);
    let mut off = path.voice(0).source_playback;
    assert!(off.set_target(1.5));
    off.chunk(1024);
    path.publish(Some(wire(false, true, 3)), true, None);
    request(&path);
    service(&mut path);
    assert_eq!(path.voice(0).sync_request.mode, SourceSyncMode::Off);
    assert_eq!(path.voice(0).source_playback.rate_target(), 1.5);
    assert!(path.voice(0).source_playback.matches_exact(&off));
    assert!(path.timeline.matches_clock_epoch(&master));
}

#[test]
fn equal_target_revision_and_cache_hit_preserve_complete_pending_glide() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let first = request(&path);
    service(&mut path);
    path.render(37);
    let pending = path.voice(0).source_playback;
    path.publish(Some(wire(true, false, 4)), true, None);
    let next = request(&path);
    assert!(!Arc::ptr_eq(&first, &next));
    service(&mut path);
    assert!(path.voice(0).source_playback.matches_exact(&pending));
    assert_eq!(path.voice(0).sync_request.global_revision, Some(4));
    for _ in 0..8 {
        assert!(Arc::ptr_eq(&request(&path), &next));
    }
    assert!(path.source.consumer.is_empty());
    assert!(path.voice(0).source_playback.matches_exact(&pending));
    assert_eq!(next.observation().unwrap().attempt, 1);
}

#[test]
fn retarget_unequal_callback_partitions_have_identical_pcm_and_copied_clock() {
    let mut results = Vec::new();
    for parts in [&[1, 17, 513][..], &[256, 3, 71][..]] {
        let mut path = prepared();
        path.publish(Some(wire(true, false, 2)), true, None);
        request(&path);
        service(&mut path);
        let mut pcm = Vec::new();
        let mut index = 0;
        while pcm.len() / 2 < 2601 {
            let n = parts[index % parts.len()].min(2601 - pcm.len() / 2);
            pcm.extend(path.render(n));
            index += 1;
        }
        results.push((pcm, path.voice(0).source_playback));
    }
    assert_eq!(results[0].0, results[1].0);
    assert!(results[0].1.matches_exact(&results[1].1));
}

#[test]
fn all_32_lanes_refuse_together_including_unchanged_unsupported_lane_and_retry_same_owner() {
    let mut path = prepared();
    for id in 2..crate::audio_engine::constants::MAX_VOICES {
        publish_additional_source(&mut path, id);
    }
    publish_members(&path, 1, false);
    for id in 1..crate::audio_engine::constants::MAX_VOICES {
        let captured = path
            .source
            .engine
            .capture_pad_start(
                id,
                0.0,
                Some(END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        path.drain();
        assert!(
            path.source
                .engine
                .publish_pad_start(&captured, None)
                .unwrap()
        );
        path.drain();
        assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
    }
    path.render(1);
    assert_eq!(
        path.source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        32
    );
    path.source.mixer.voices[31].paused = true;
    publish_members(&path, 2, true);
    let before = scalar_state(&path);
    let snapshot = request(&path);
    service(&mut path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&path, &before);
    path.source.mixer.voices[31].paused = false;
    assert!(Arc::ptr_eq(&snapshot, &request(&path)));
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert!(
        path.source
            .mixer
            .voices
            .iter()
            .all(|voice| voice.active && voice.sync_request.global_revision == Some(2))
    );
}

#[test]
fn away_back_raw_choices_supersede_old_original_without_false_ack() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let old = request(&path);
    path.publish(Some(wire(false, true, 3)), true, None);
    let current = request(&path);
    service(&mut path);
    assert_eq!(
        old.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    assert_eq!(current.observation().unwrap().state, "applied");
    assert_eq!(path.feedback(0)["policy_revision"], 3);
    assert_eq!(path.feedback(0)["policy_realtime_bpm"], true);
    assert_eq!(
        path.feedback(0)["adoption"]["snapshot_revision"],
        current.revision()
    );
}

#[test]
fn mapped_intent_and_missing_master_leave_original_scalar_audio_unchanged() {
    for mapped in [false, true] {
        let mut path = prepared();
        path.publish(Some(wire(true, mapped, 2)), true, None);
        if !mapped {
            path.source.mixer.set_bpm_lock(false);
            path.timeline.clear_master_bpm();
        }
        let before = scalar_state(&path);
        let snapshot = request(&path);
        service(&mut path);
        assert_eq!(snapshot.observation().unwrap().state, "refused");
        unchanged(&path, &before);
        assert_eq!(path.voice(0).sync_request.global_revision, Some(1));
    }
}

#[test]
fn intro_tail_and_pair_physical_keep_their_existing_projection_domain_and_exact_e_pair() {
    for mode in [
        ExplicitSeekMode::BeforeLoop,
        ExplicitSeekMode::AfterLoop,
        ExplicitSeekMode::Normal,
    ] {
        let mut path = prepared();
        let region = FrameRange {
            start: 128,
            end: END,
        };
        path.source
            .mixer
            .set_loop_region_frames(0, region.start, Some(region.end));
        let domain = SourceLoopDomain::physical(4096, region);
        let frame = match mode {
            ExplicitSeekMode::BeforeLoop => 12,
            ExplicitSeekMode::AfterLoop => 2048,
            _ => 513,
        };
        let mut raw = if mode == ExplicitSeekMode::Normal {
            SourcePlayback::scalar_continuation(
                FractionalSourcePosition {
                    frame,
                    fraction: 0.125,
                    seek_mode: mode,
                },
                domain,
                1.5,
                1.5,
            )
            .unwrap()
        } else {
            SourcePlayback::new(frame, mode, 1.5)
        };
        assert!(raw.configure_domain(domain));
        let voice = path
            .source
            .mixer
            .voices
            .iter_mut()
            .find(|voice| voice.active)
            .unwrap();
        voice.source_playback = raw;
        voice.frame_pos = frame;
        voice.explicit_seek_mode = mode;
        // Public rows preserve this genuine physical loop; they do not reinterpret
        // a manual accepted musical period as physical.
        publish_region(&path, 128, END, 2);
        let snapshot = request(&path);
        service(&mut path);
        assert_eq!(snapshot.observation().unwrap().state, "applied");
        assert!(path.voice(0).source_playback.matches_domain(domain));
        assert_eq!(path.voice(0).source_playback.position(), raw.position());
    }
}

fn publish_region(path: &Path, start: usize, end: usize, revision: u64) {
    Python::attach(|py| {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < 2).then(|| {
                    Py::new(
                        py,
                        path.source
                            .engine
                            .current_input_runtime_pad_binding(id)
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap()
                })
            })
            .collect();
        let rows = (0..NUM_SAMPLES)
            .map(|id| {
                let row = PyDict::new(py);
                row.set_item("sync_requested", id < 2).unwrap();
                row.set_item("intent_revision", 7).unwrap();
                row.set_item("delivery", py.None()).unwrap();
                row.set_item("exact_quarters", py.None()).unwrap();
                row.unbind()
            })
            .collect();
        path.source
            .engine
            .set_input_runtime_state(
                py,
                true,
                (0..NUM_SAMPLES).map(|id| id < 2).collect(),
                vec![start as f64 / f64::from(RATE); NUM_SAMPLES],
                vec![Some(end as f64 / f64::from(RATE)); NUM_SAMPLES],
                Some(bindings),
                None,
                Some(rows),
                Some(wire(true, false, revision).to_string()),
            )
            .unwrap();
    });
}

fn publish_additional_source(path: &mut Path, id: usize) {
    use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
    let sample = path.source.engine.sample_cache.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .clone();
    let ownership = path.source.engine.input_runtime_ownership.clone();
    let before = ownership.slot_snapshot(id).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(
            before,
            crate::audio_engine::content_lifetime::reserve().unwrap(),
        )
        .unwrap();
    let generation = 100 + id as u64;
    let publication = ownership
        .publish_source_write(
            &write,
            &sample,
            RATE,
            generation,
            ownership.next_authority(id).unwrap(),
            Some(TimingIntent::Manual),
            false,
            || true,
        )
        .unwrap();
    path.source.engine.sample_cache.lock().unwrap()[id] = Some(sample.clone());
    path.source.engine.pad_request_ids.lock().unwrap()[id] = generation;
    path.source.engine.loaded_source_generations.lock().unwrap()[id] = (generation, RATE);
    path.source.engine.loaded_source_digests.lock().unwrap()[id] =
        Some(format!("{generation:064x}"));
    path.source.engine.timing_intents.lock().unwrap()[id] = TimingIntent::Manual;
    path.source
        .producer
        .lock()
        .unwrap()
        .push(crate::messages::ControlMessage::LoadSample {
            id,
            sample,
            publication: Some(publication),
        })
        .unwrap();
    path.drain();
    assert_eq!(
        ownership.callback_feedback_key(id),
        ownership.slot_snapshot(id).unwrap().content_key()
    );
    path.source.mixer.set_pad_key_lock(id, false);
    path.source.mixer.set_pad_bpm(id, Some(60.0 / SOURCE));
}

fn publish_members(path: &Path, revision: u64, sync: bool) {
    let count = crate::audio_engine::constants::MAX_VOICES;
    Python::attach(|py| {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < count).then(|| {
                    Py::new(
                        py,
                        path.source
                            .engine
                            .current_input_runtime_pad_binding(id)
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap()
                })
            })
            .collect();
        let rows = (0..NUM_SAMPLES)
            .map(|id| {
                let row = PyDict::new(py);
                row.set_item("sync_requested", id < count).unwrap();
                row.set_item("intent_revision", 7).unwrap();
                row.set_item("delivery", py.None()).unwrap();
                row.set_item("exact_quarters", py.None()).unwrap();
                row.unbind()
            })
            .collect();
        path.source
            .engine
            .set_input_runtime_state(
                py,
                true,
                (0..NUM_SAMPLES).map(|id| id < count).collect(),
                vec![0.0; NUM_SAMPLES],
                vec![Some(END as f64 / f64::from(RATE)); NUM_SAMPLES],
                Some(bindings),
                None,
                Some(rows),
                Some(wire(sync, false, revision).to_string()),
            )
            .unwrap();
    });
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for row in &mut rows[..count] {
        row.key_lock = false;
    }
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_pitch_runtime_state(rows)
        .unwrap();
}

#[test]
fn scalar_legacy_target_changes_are_unsettled_until_real_glide_converges() {
    for from_fixed in [false, true] {
        let mut path = prepared();
        if from_fixed {
            path.publish(Some(wire(true, false, 2)), true, None);
            request(&path);
            service(&mut path);
            path.render(2049);
        } else {
            path.source.mixer.set_bpm_lock(true);
        }
        path.publish(None, false, None);
        let snapshot = request(&path);
        service(&mut path);
        assert_eq!(snapshot.observation().unwrap().state, "applied");
        assert_eq!(path.feedback(0)["policy_mode"], "legacy");
        assert!(path.feedback(0)["policy_revision"].is_null());
        assert_eq!(path.feedback(0)["settled"], false);
        path.render(2049);
        assert_eq!(path.feedback(0)["settled"], true);
        assert_eq!(
            path.voice(0).source_playback.rate_target(),
            if from_fixed { 1.5 } else { fixed_target() }
        );
    }
}

#[test]
fn unconfigured_scalar_refuses_then_retries_the_same_snapshot_after_normal_render_configuration() {
    let mut path = prepared();
    let raw = SourcePlayback::new(23, ExplicitSeekMode::Normal, 1.5);
    path.source
        .mixer
        .voices
        .iter_mut()
        .find(|voice| voice.active)
        .unwrap()
        .source_playback = raw;
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    service(&mut path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    assert!(path.voice(0).source_playback.matches_exact(&raw));
    path.render(1);
    assert!(Arc::ptr_eq(&snapshot, &request(&path)));
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
}

#[test]
fn public_hold_refusal_keeps_cause_and_original_audio_until_real_release_then_retry() {
    let mut path = prepared();
    let ticket = Python::attach(|py| {
        let press = 991_u64.into_pyobject(py).unwrap();
        path.source
            .engine
            .pause_waveform_hold(0, press.as_any(), None)
            .unwrap()
    });
    path.drain();
    assert!(path.voice(0).paused);
    let pending_start = path.capture();
    path.drain();
    assert!(ticket.is_current());
    path.publish(Some(wire(true, false, 2)), true, None);
    assert!(ticket.is_current());
    assert!(
        path.source
            .engine
            .publish_pad_start(&pending_start, None)
            .is_err()
    );
    let snapshot = request(&path);
    let before = scalar_state(&path);
    service(&mut path);
    unchanged(&path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    assert!(path.source.engine.release_waveform_hold(&ticket).unwrap());
    path.drain();
    assert_eq!(ticket.publication_status(), "resumed");
    assert!(!path.voice(0).paused);
    assert!(Arc::ptr_eq(&snapshot, &request(&path)));
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
}

#[test]
fn mapped_hold_raw_request_change_keeps_original_owner_domain_phase_and_release_cause() {
    let mut path = prepared();
    path.source.engine.stop_sample(0).unwrap();
    path.drain();
    let (_job, delivery) = path.map();
    path.publish(Some(wire(true, true, 2)), true, Some(&delivery));
    let original = path.start();
    assert_eq!(original.state(), "applied", "{:?}", original.reason());
    path.render(17);
    assert!(path.voice(0).source_playback.is_mapped());
    let source = path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let ticket = Python::attach(|py| {
        let press = 992_u64.into_pyobject(py).unwrap();
        path.source
            .engine
            .pause_waveform_hold(0, press.as_any(), None)
            .unwrap()
    });
    path.drain();
    assert!(ticket.is_current());
    let pending = path.capture();
    path.drain();
    let before = scalar_state(&path);
    path.publish(Some(wire(true, false, 3)), true, Some(&delivery));
    assert!(ticket.is_current());
    assert!(
        path.source
            .engine
            .publish_pad_start(&pending, None)
            .is_err()
    );
    let snapshot = request(&path);
    service(&mut path);
    unchanged(&path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    let custody = path.voice(0).mapped_start.as_ref().unwrap();
    assert!(custody.original().python_same_owner(&original));
    assert!(Arc::ptr_eq(custody.effective_source().unwrap(), &source));
    assert!(path.source.engine.release_waveform_hold(&ticket).unwrap());
    path.drain();
    assert_eq!(ticket.publication_status(), "resumed");
    assert!(!path.voice(0).paused);
    let after = path.voice(0).source_playback.position();
    let prior = before[0].2.position();
    assert_eq!(
        (after.frame, after.fraction.to_bits(), after.seek_mode),
        (prior.frame, prior.fraction.to_bits(), prior.seek_mode)
    );
    assert_eq!(path.voice(0).sync_request.global_revision, Some(2));
    assert!(path.voice(0).source_playback.is_mapped());
    assert!(Arc::ptr_eq(
        path.voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap(),
        &source
    ));
}

#[test]
fn mixed_loop_and_request_publication_preserves_existing_hold_supersession() {
    let mut path = prepared();
    let ticket = Python::attach(|py| {
        let press = 993_u64.into_pyobject(py).unwrap();
        path.source
            .engine
            .pause_waveform_hold(0, press.as_any(), None)
            .unwrap()
    });
    path.drain();
    assert!(ticket.is_current());
    publish_region(&path, 128, END, 2);
    assert!(!ticket.is_current());
    assert!(path.source.engine.release_waveform_hold(&ticket).unwrap());
    path.drain();
    assert_eq!(ticket.publication_status(), "superseded");
    assert!(path.voice(0).paused);
}

#[test]
fn request_publication_exhaustion_is_atomic_and_does_not_wrap_or_strand_original_hold() {
    use std::sync::atomic::Ordering;
    let mut path = prepared();
    let ownership = path.source.engine.input_runtime_ownership.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    // Only a counter-boundary fixture uses this direct initialization. The real
    // public Hold command owns the subsequent MAX transport admission and effect.
    ownership
        .transport_intent_for_cell(key.cell())
        .store(u64::MAX - 1, Ordering::Release);
    let ticket = Python::attach(|py| {
        let press = 994_u64.into_pyobject(py).unwrap();
        path.source
            .engine
            .pause_waveform_hold(0, press.as_any(), None)
            .unwrap()
    });
    path.drain();
    assert!(ticket.is_current());
    let before = scalar_state(&path);
    let stamp = ownership.runtime_policy_revision();
    let launch = ownership.launch_revision(0);
    let bindings = (0..NUM_SAMPLES)
        .map(|id| {
            (id < 2).then(|| {
                path.source
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let error = path
        .source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![0.0; NUM_SAMPLES],
            vec![Some(END as f64 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            None,
            None,
            crate::audio_engine::source_sync_policy::GlobalTimingPolicy::parse(Some(
                &wire(true, false, 2).to_string(),
            ))
            .unwrap(),
        )
        .unwrap_err();
    assert!(error.contains("exhausted"), "{error}");
    assert_eq!(ownership.runtime_policy_revision(), stamp);
    assert_eq!(ownership.launch_revision(0), launch);
    assert_eq!(
        ownership
            .transport_intent_for_cell(key.cell())
            .load(Ordering::Acquire),
        u64::MAX
    );
    unchanged(&path, &before);
    assert!(ticket.is_current());
    assert!(path.source.engine.release_waveform_hold(&ticket).unwrap());
    path.drain();
    assert_eq!(ticket.publication_status(), "resumed");
    assert!(!path.voice(0).paused);
}

#[test]
fn scheduler_full_and_order_exhaustion_retire_original_guard_and_allow_cached_retry() {
    for exhaust_order in [false, true] {
        let mut path = prepared();
        path.publish(Some(wire(true, false, 2)), true, None);
        let snapshot = request(&path);
        if exhaust_order {
            with_input_order_exhaustion_for_test(|| path.drain());
        } else {
            for _ in 0..16 {
                path.scheduler
                    .schedule(99_000, ScheduledCommand::StopAll)
                    .unwrap();
            }
            path.drain();
            assert_eq!(path.scheduler.len(), 16);
        }
        assert_eq!(
            snapshot.observation().unwrap().reason,
            Some(RuntimePolicyError::SchedulerFull)
        );
        assert_eq!(snapshot.observation().unwrap().execution_frame, None);
        assert_eq!(path.voice(0).sync_request.global_revision, Some(1));
        // Dispose only the test's scalar filler commands. Production owner custody
        // has already moved through its actual retirement method.
        if !exhaust_order {
            path.scheduler = FixedCapacityScheduler::new();
        }
        assert!(Arc::ptr_eq(&snapshot, &request(&path)));
        service(&mut path);
        assert_eq!(snapshot.observation().unwrap().state, "applied");
    }
}

struct CloseImage {
    scheduler: FixedCapacityScheduler<16>,
    retirement: crate::audio_engine::buffer_retirement::RtAudioBufferRetirement,
}

struct ReleaseOnDrop(Option<mpsc::Sender<()>>);
impl ReleaseOnDrop {
    fn release(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.release();
    }
}

fn wait_for(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(
            Instant::now() < end,
            "bounded retirement worker wait timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn actual_retirement_queue_and_backlog_preserve_unpopped_control_owner_until_capacity_returns() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    let (release, blocked) = mpsc::channel();
    let (mut callback, worker) = create_audio_buffer_retirement_with_callback_paused(
        |retirement| {
            Box::new(CloseImage {
                scheduler: FixedCapacityScheduler::new(),
                retirement,
            })
        },
        move || {
            let _ = blocked.recv();
        },
    )
    .unwrap();
    let mut release = ReleaseOnDrop(Some(release));
    let state = callback.state_mut();
    let sample = path.voice(0).sample.as_ref().unwrap().clone();
    let mut filled = 0;
    while state.retirement.available_retirement_slots() > 0 {
        state.retirement.retire_sample(sample.clone());
        filled += 1;
        assert!(filled < 2000);
    }
    assert!(filled > crate::audio_engine::buffer_retirement::RETIRED_AUDIO_BUFFER_QUEUE_CAPACITY);
    let before = scalar_state(&path);
    assert_eq!(
        drain_control_messages(
            &mut path.source.consumer,
            &mut state.scheduler,
            path.timeline.output_frame(),
            &mut TriggerQuantization::Immediate,
            &mut path.timeline,
            &mut path.source.mixer,
            &mut path.events,
            &mut state.retirement
        ),
        0
    );
    assert!(!path.source.consumer.is_empty());
    unchanged(&path, &before);
    release.release();
    wait_for(|| state.retirement.available_retirement_slots() > 0);
    drain_control_messages(
        &mut path.source.consumer,
        &mut state.scheduler,
        path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.events,
        &mut state.retirement,
    );
    settle_parameters(&mut path);
    render_scheduled_audio(
        &mut path.source.mixer,
        &mut state.scheduler,
        &mut [],
        &mut [0.0; NUM_SAMPLES],
        path.timeline.output_frame(),
        2,
        &mut path.timeline,
        &mut path.events,
        &mut state.retirement,
    );
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    drop(callback);
    worker.join_checked().unwrap();
}

#[test]
fn callback_close_releases_pending_lease_with_cache_still_strong_and_with_cache_replaced() {
    for replace_cache in [false, true] {
        let mut path = prepared();
        path.publish(Some(wire(true, false, 2)), true, None);
        let snapshot = request(&path);
        let weak = Arc::downgrade(&snapshot);
        let (release, blocked) = mpsc::channel();
        let (mut callback, worker) = create_audio_buffer_retirement_with_callback_paused(
            |retirement| {
                Box::new(CloseImage {
                    scheduler: FixedCapacityScheduler::new(),
                    retirement,
                })
            },
            move || {
                let _ = blocked.recv();
            },
        )
        .unwrap();
        let mut release = ReleaseOnDrop(Some(release));
        let state = callback.state_mut();
        drain_control_messages(
            &mut path.source.consumer,
            &mut state.scheduler,
            path.timeline.output_frame(),
            &mut TriggerQuantization::Immediate,
            &mut path.timeline,
            &mut path.source.mixer,
            &mut path.events,
            &mut state.retirement,
        );
        assert_eq!(snapshot.observation().unwrap().state, "pending");
        // Cancellation of an unrelated capture moves the owning policy event intact.
        state.scheduler.schedule_capture_start(555, 90_000).unwrap();
        assert_eq!(state.scheduler.cancel_capture_session(555), 1);
        assert_eq!(state.scheduler.len(), 1);
        if replace_cache {
            path.publish(Some(wire(false, true, 3)), true, None);
            request(&path);
        }
        drop(callback);
        assert_eq!(snapshot.observation().unwrap().state, "pending");
        if replace_cache {
            drop(snapshot);
        }
        release.release();
        worker.join_checked().unwrap();
        if replace_cache {
            assert!(
                weak.upgrade().is_none(),
                "last original snapshot belongs to close/retirement worker"
            );
        } else {
            let snapshot = weak.upgrade().unwrap();
            assert_eq!(
                snapshot.observation().unwrap().reason,
                Some(RuntimePolicyError::Busy)
            );
            assert!(Arc::ptr_eq(&snapshot, &request(&path)));
            service(&mut path);
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            assert_eq!(snapshot.observation().unwrap().attempt, 2);
        }
    }
}

#[test]
fn applied_cache_observes_new_legacy_callback_birth_and_reuses_original_snapshot() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    service(&mut path);
    let generation = path.voice(0).generation;
    path.source
        .producer
        .lock()
        .unwrap()
        .push(crate::messages::ControlMessage::PlaySample {
            id: 0,
            volume: 1.0,
            received_at_ns: None,
            expected_source_generation: None,
        })
        .unwrap();
    path.drain();
    path.render(1);
    assert!(path.voice(0).generation > generation);
    assert_eq!(path.voice(0).sync_request.mode, SourceSyncMode::Legacy);
    assert!(Arc::ptr_eq(&snapshot, &request(&path)));
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().attempt, 2);
    assert_eq!(path.voice(0).sync_request.global_revision, Some(2));
}

#[test]
fn full_control_queue_is_normal_refusal_and_retries_cached_original_without_reallocation() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    while path
        .source
        .producer
        .lock()
        .unwrap()
        .push(crate::messages::ControlMessage::Ping())
        .is_ok()
    {}
    let snapshot = request(&path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Capacity)
    );
    while path.source.consumer.pop().is_ok() {}
    assert!(Arc::ptr_eq(&snapshot, &request(&path)));
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
}

#[test]
fn actual_e_max_is_present_and_callback_preflight_has_no_rust_allocations_or_deallocations() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    let crate::messages::ControlMessage::AdoptRuntimePolicy(work) =
        path.source.consumer.pop().unwrap()
    else {
        panic!("original work")
    };
    path.drain();
    settle_parameters(&mut path);
    let (execution, allocations) = crate::test_memory::observe_current_thread(|| {
        execute_scheduled_command(
            &mut path.source.mixer,
            &mut path.timeline,
            u64::MAX,
            ScheduledCommand::AdoptRuntimePolicy(work),
            &mut path.events,
            &mut ImmediateAudioBufferRetirement,
        )
    });
    assert!(matches!(execution, ScheduledExecution::Complete));
    assert_eq!(
        allocations,
        crate::test_memory::ThreadAllocations::default()
    );
    assert_eq!(
        snapshot.observation().unwrap().execution_frame,
        Some(u64::MAX)
    );
    assert_eq!(
        path.feedback(0)["adoption"]["execution_frame"],
        json!(u64::MAX)
    );
    assert!(std::mem::size_of::<SourcePlayback>() * 32 <= 16 * 1024);
    assert!(std::mem::size_of::<crate::audio_engine::RuntimePolicyWork>() <= 16);
}

#[test]
fn stop_and_real_retrigger_do_not_borrow_old_attempt_ack_or_replace_original_launch() {
    for behavior in ["first_sound_at_grid", "attack_at_grid"] {
        let mut path = Path::new(false);
        path.drain();
        path.source.mixer.finish_key_lock_start_callback(true);
        assert!(
            path.source
                .engine
                .set_key_lock_start_behavior(behavior)
                .unwrap()
        );
        path.publish(Some(wire(false, false, 1)), true, None);
        let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
        rows[0].key_lock = false;
        rows[1].key_lock = false;
        path.source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_pitch_runtime_state(rows)
            .unwrap();
        let original = path.start();
        path.render(17);
        path.publish(Some(wire(true, false, 2)), true, None);
        let snapshot = request(&path);
        path.source.engine.stop_sample(0).unwrap();
        service(&mut path);
        assert!(!path.source.mixer.voices.iter().any(|voice| voice.active));
        assert_ne!(snapshot.observation().unwrap().state, "applied");
        assert_eq!(original.state(), "applied");
        let next = path.start();
        path.render(1);
        assert_eq!(next.state(), "applied");
        let clock = path.voice(0).source_playback;
        request(&path);
        service(&mut path);
        assert!(path.voice(0).source_playback.matches_exact(&clock));
        assert_eq!(path.voice(0).sync_request.global_revision, Some(2));
    }
}

#[test]
fn genuine_future_pitch_work_blocks_scalar_adoption_without_key_lock_pending_arrays() {
    use crate::audio_engine::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START};
    let mut fixture = Fixture::new(false);
    fixture.mixer.set_master_period(0.5);
    assert!(fixture.transport.set_master_period(0.5));
    fixture.mixer.set_pad_bpm(0, Some(120.0));
    let initial = fixture.capture(0, "pitch.extra:0:set_retrigger");
    fixture.publish(&initial);
    fixture.render(1);
    fixture.render(17_000);
    let bindings = (0..NUM_SAMPLES)
        .map(|id| {
            (id < 2).then(|| {
                fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let mut maps =
        vec![crate::audio_engine::source_map_admission::SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[1].sync_requested = true;
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![LOOP_START as f64 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(LOOP_END as f64 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            None,
            Some(maps),
            GlobalTimingPolicy::parse(Some(&wire(true, false, 2).to_string())).unwrap(),
        )
        .unwrap();
    fixture.rows(PitchPadProjection {
        key_lock: false,
        quantize_step_64ths: Some(16),
        ..PitchPadProjection::default()
    });
    let pending = fixture.capture(0, "pitch.extra:0:set_retrigger");
    fixture.publish(&pending);
    assert!(pending.execution_target() > fixture.transport.output_frame());
    let key = pending.content_key();
    assert!(
        fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_pending_content(key)
    );
    let before = fixture.voice(0).source_playback;
    fixture
        .control
        .engine
        .request_input_runtime_policy_adoption()
        .unwrap();
    fixture.drain();
    let (_producer, mut parameters) = rtrb::RingBuffer::new(1);
    drain_parameter_messages(&mut parameters, &mut fixture.mixer, &mut fixture.transport);
    fixture.render(0);
    let snapshot = fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .runtime_policy_snapshot()
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    assert!(fixture.voice(0).source_playback.matches_exact(&before));
    assert!(!pending.effective());
    assert!(
        fixture
            .control
            .engine
            .cancel_pitch_action(&pending)
            .unwrap()
    );
    fixture.drain();
    assert!(
        !fixture
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_pending_content(key)
    );
}

#[test]
fn genuine_future_ordinary_start_blocks_even_an_unchanged_scalar_target() {
    let mut path = prepared();
    path.render(350);
    path.publish(Some(wire(false, true, 2)), true, None);
    let pending = path.capture();
    let mut grid = TriggerQuantization::Grid { step_64ths: 16 };
    drain_control_messages(
        &mut path.source.consumer,
        &mut path.scheduler,
        path.timeline.output_frame(),
        &mut grid,
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.events,
        &mut ImmediateAudioBufferRetirement,
    );
    assert!(pending.target_frame().unwrap() > path.timeline.output_frame());
    assert!(
        path.source
            .engine
            .publish_pad_start(&pending, None)
            .unwrap()
    );
    path.drain();
    let key = pending.content_key();
    assert!(
        path.source
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_pending_content(key)
    );
    let before = scalar_state(&path);
    let snapshot = request(&path);
    service(&mut path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&path, &before);
    assert_eq!(path.voice(0).sync_request.global_revision, Some(1));
    assert!(pending.effective_frame().is_none());
}

#[test]
fn due_retirement_exhaustion_preserves_owner_old_audio_and_reports_actual_late_e_on_retry() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    let (release, blocked) = mpsc::channel();
    let (mut callback, worker) = create_audio_buffer_retirement_with_callback_paused(
        |retirement| {
            Box::new(CloseImage {
                scheduler: FixedCapacityScheduler::new(),
                retirement,
            })
        },
        move || {
            let _ = blocked.recv();
        },
    )
    .unwrap();
    let mut release = ReleaseOnDrop(Some(release));
    let state = callback.state_mut();
    drain_control_messages(
        &mut path.source.consumer,
        &mut state.scheduler,
        path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.events,
        &mut state.retirement,
    );
    settle_parameters(&mut path);
    let sample = path.voice(0).sample.as_ref().unwrap().clone();
    let mut filled = 0;
    while state.retirement.available_retirement_slots() > 0 {
        state.retirement.retire_sample(sample.clone());
        filled += 1;
        assert!(filled < 2000);
    }
    let start = path.timeline.output_frame();
    let mut output = [0.0; 20];
    render_scheduled_audio(
        &mut path.source.mixer,
        &mut state.scheduler,
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        start,
        2,
        &mut path.timeline,
        &mut path.events,
        &mut state.retirement,
    );
    path.timeline.advance_by_rendered_frames(10);
    assert!(output.iter().any(|sample| *sample != 0.0));
    assert!(state.scheduler.deferred_due().is_some());
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    assert_eq!(path.voice(0).sync_request.global_revision, Some(1));
    release.release();
    wait_for(|| state.retirement.available_retirement_slots() > 0);
    drain_control_messages(
        &mut path.source.consumer,
        &mut state.scheduler,
        path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.events,
        &mut state.retirement,
    );
    // Existing deferred-due retry precedes the new drain. It must refuse Busy,
    // rather than claiming a policy effect before both drains have settled.
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Busy)
    );
    assert_eq!(
        snapshot.observation().unwrap().execution_frame,
        Some(start + 10)
    );
    wait_for(|| {
        request(&path);
        !path.source.consumer.is_empty()
    });
    let before = path.voice(0).source_playback.position();
    service(&mut path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert_eq!(
        snapshot.observation().unwrap().execution_frame,
        Some(start + 10)
    );
    assert_eq!(path.voice(0).source_playback.position(), before);
    drop(callback);
    worker.join_checked().unwrap();
}

#[test]
fn actual_swap_refuses_pre_mirror_snapshot_and_keeps_same_living_source_clock() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let original = request(&path);
    let before = path.voice(0).source_playback;
    let generation = path.voice(0).generation;
    let captured = path.source.engine._capture_pad_pair(0, 1).unwrap();
    let ticket = path.source.engine._prepare_pad_pair(&captured).unwrap();
    path.source.engine._publish_pad_pair(&ticket).unwrap();
    service(&mut path);
    assert_eq!(original.observation().unwrap().state, "refused");
    assert!(path.voice(1).source_playback.matches_exact(&before));
    assert_eq!(path.voice(1).generation, generation);
    assert!(path.source.engine._reconcile_pad_pair(&ticket).unwrap());
    let ack = ticket.own_acknowledgement().unwrap();
    path.source.engine._finish_pad_pair(&ticket, &ack).unwrap();
    let fresh = request(&path);
    assert!(!Arc::ptr_eq(&original, &fresh));
    service(&mut path);
    assert_eq!(fresh.observation().unwrap().state, "applied");
    assert_eq!(path.voice(1).source_playback.position(), before.position());
    assert_eq!(path.voice(1).sync_request.global_revision, Some(2));
}

#[test]
fn runtime_pitch_change_before_claim_refuses_all_copied_lanes() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    path.drain();
    settle_parameters(&mut path);
    let runtime = Arc::new(path.source.engine.input_runtime.take().unwrap());
    let change = runtime.clone();
    let before = scalar_state(&path);
    crate::audio_engine::mixer::RtMixer::with_runtime_policy_before_claim_for_test(
        move || {
            let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
            rows[0].key_lock = false;
            rows[1].key_lock = false;
            rows[0].intent = crate::audio_engine::pitch_intent::PitchIntent::new(0, 1).unwrap();
            change.set_pitch_runtime_state(rows).unwrap();
        },
        || {
            path.render(0);
        },
    );
    unchanged(&path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    path.source.engine.input_runtime = Some(Arc::try_unwrap(runtime).ok().unwrap());
}

#[test]
fn independent_callback_ack_transition_recaptures_original_rows_once_before_cache_retry() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let ownership = path.source.engine.input_runtime_ownership.clone();
    let acknowledgements = path.source.engine.current_timing_acknowledgements.clone();
    let key = ownership.slot_snapshot(0).unwrap().content_key().unwrap();
    // A manual source's authorized timing token is zero. An unrelated ACK is
    // deliberately unqualified; restoring zero must make the next capture usable.
    assert!(acknowledgements.acknowledge_for_key(&ownership, key, 7));
    let pending_ack = request(&path);
    let before = scalar_state(&path);
    // Use the original callback ACK writer, without any Native row publication.
    // This isolates the independently mutable fence even for a manual source.
    assert!(acknowledgements.clear_for_key(&ownership, key));
    let ready = request(&path);
    assert!(!Arc::ptr_eq(&pending_ack, &ready));
    assert_eq!(ready.revision(), pending_ack.revision() + 1);
    assert!(Arc::ptr_eq(&ready, &request(&path)));
    unchanged(&path, &before);
    service(&mut path);
    assert_eq!(
        pending_ack.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    assert_eq!(ready.observation().unwrap().state, "applied");
    assert!(Arc::ptr_eq(&ready, &request(&path)));
    assert!(path.source.consumer.is_empty());
}

#[test]
fn original_offline_source_request_before_due_refuses_policy_without_changing_living_audio() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    let binding = snapshot.row(0).unwrap().binding;
    let ownership = path.source.engine.input_runtime_ownership.clone();
    let original_request = ownership
        .binding_source_request_incarnation(0, binding)
        .unwrap();
    let sample = path.source.engine.sample_cache.lock().unwrap()[0]
        .as_ref()
        .unwrap()
        .clone();
    let reader = crate::audio_engine::complete_context::CompleteSourceReader::capture(
        &path.source.engine,
        0,
        sample,
    )
    .unwrap();
    let (generation, rate) = path.source.engine.loaded_source_generations.lock().unwrap()[0];
    // Exact productive Native admission used by begin_offline_analysis; its public
    // wrapper additionally requires a device. No analysis/DSP worker is launched.
    let job = path
        .source
        .engine
        .offline_jobs
        .begin_complete(
            0,
            reader,
            rate,
            generation,
            crate::audio_engine::analysis_jobs::OfflineRequestOwner {
                request_ids: path.source.engine.pad_request_ids.clone(),
                prepared_epoch: path.source.engine.prepared_source_epoch(0).unwrap().clone(),
            },
            path.source.engine.loader_tx.clone(),
        )
        .unwrap();
    assert_ne!(
        ownership.binding_source_request_incarnation(0, binding),
        Some(original_request)
    );
    let before = scalar_state(&path);
    service(&mut path);
    unchanged(&path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::SourceChanged)
    );
    assert_eq!(path.voice(0).sync_request.global_revision, Some(1));
    job.cancel();
}

#[test]
fn real_resident_window_ack_before_due_refuses_old_row_without_policy_partial_effect() {
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    path.drain();
    let original = path.voice(0).sample.as_ref().unwrap().clone();
    let window = crate::audio_engine::resident_relocation::relocate_with_producer(
        &path.source.engine,
        0,
        0.0,
        2048.0 / f64::from(RATE),
        path.source.producer.clone(),
    )
    .unwrap();
    wait_for(|| {
        !path.source.consumer.is_empty()
            || matches!(
                window.publication_status(),
                "failed" | "cancelled" | "rejected"
            )
    });
    assert_eq!(
        window.publication_status(),
        "pending",
        "{:?}",
        window.error()
    );
    path.drain();
    assert_eq!(
        window.publication_status(),
        "accepted",
        "{:?}",
        window.error()
    );
    assert_eq!(
        path.voice(0).sample.as_ref().unwrap().window_revision(),
        original.window_revision() + 1
    );
    let before = scalar_state(&path);
    settle_parameters(&mut path);
    path.render(0);
    unchanged(&path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::SourceChanged)
    );
    assert_eq!(path.voice(0).sync_request.global_revision, Some(1));
    crate::audio_engine::resident_relocation::reconcile(&path.source.engine).unwrap();
}

#[test]
fn real_public_resident_control_admission_before_claim_refuses_all_policy_lanes() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let mut path = prepared();
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    path.drain();
    settle_parameters(&mut path);
    let before = scalar_state(&path);
    // The deterministic hook stands for the control writer between dry preflight
    // and claim. The callback still holds its original mixer/ownership; the spare
    // engine only lets this test move the original API owner into that closure.
    let spare = crate::audio_engine::AudioEngine::new().unwrap();
    let original = Rc::new(std::mem::replace(&mut path.source.engine, spare));
    let control = original.clone();
    let ticket = Rc::new(RefCell::new(None));
    let captured = ticket.clone();
    crate::audio_engine::mixer::RtMixer::with_runtime_policy_before_claim_for_test(
        move || {
            let prepared = control
                .prepare_resident_control(
                    0,
                    Some(0.0),
                    Some(END as f64 / f64::from(RATE)),
                    None,
                    Some(false),
                )
                .unwrap();
            // Heavy proposals deliberately retain the old authority while only
            // preparing. Order the REAL worker's Native publication before the
            // final claim; returning a proposal alone grants no such ordering.
            wait_for(|| {
                control.input_runtime_ownership.resident_control_pending(0)
                    || matches!(
                        prepared.publication_status(),
                        "failed" | "cancelled" | "rejected"
                    )
            });
            assert_eq!(
                prepared.publication_status(),
                "pending",
                "{:?}",
                prepared.error()
            );
            assert!(control.input_runtime_ownership.resident_control_pending(0));
            *captured.borrow_mut() = Some(prepared);
        },
        || {
            path.render(0);
        },
    );
    unchanged(&path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    path.source.engine = Rc::try_unwrap(original).ok().unwrap();
    // Finish the independent genuine transaction normally, after proving S made
    // no write. Its own source/window effect is not attributed to the refusal.
    let ticket = ticket.borrow_mut().take().unwrap();
    wait_for(|| {
        !path.source.consumer.is_empty()
            || matches!(
                ticket.publication_status(),
                "failed" | "cancelled" | "rejected"
            )
    });
    path.drain();
    assert_eq!(
        ticket.publication_status(),
        "accepted",
        "{:?}",
        ticket.error()
    );
    crate::audio_engine::resident_relocation::reconcile(&path.source.engine).unwrap();
}

#[test]
fn real_resident_control_already_published_before_preflight_refuses_entire_scalar_cohort() {
    let mut path = prepared();
    let second = path
        .source
        .engine
        .capture_pad_start(
            1,
            0.0,
            Some(END as f64 / f64::from(RATE)),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    path.drain();
    assert!(path.source.engine.publish_pad_start(&second, None).unwrap());
    path.drain();
    path.render(1);
    assert_eq!(
        path.source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        2
    );
    path.publish(Some(wire(true, false, 2)), true, None);
    let snapshot = request(&path);
    path.drain();
    settle_parameters(&mut path);
    let ticket = path
        .source
        .engine
        .prepare_resident_control(
            0,
            Some(0.0),
            Some(END as f64 / f64::from(RATE)),
            None,
            Some(false),
        )
        .unwrap();
    wait_for(|| {
        path.source
            .engine
            .input_runtime_ownership
            .resident_control_pending(0)
            || matches!(
                ticket.publication_status(),
                "failed" | "cancelled" | "rejected"
            )
    });
    assert_eq!(
        ticket.publication_status(),
        "pending",
        "{:?}",
        ticket.error()
    );
    assert!(
        path.source
            .engine
            .input_runtime_ownership
            .resident_control_pending(0)
    );
    // The existing pending Native authority is already part of the first dry
    // capture. Merely comparing it unchanged at claim must not make it eligible.
    let before = scalar_state(&path);
    path.render(0);
    unchanged(&path, &before);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    assert!(
        path.source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .all(|voice| voice.sync_request.global_revision == Some(1))
    );
    path.drain();
    assert_eq!(
        ticket.publication_status(),
        "accepted",
        "{:?}",
        ticket.error()
    );
    crate::audio_engine::resident_relocation::reconcile(&path.source.engine).unwrap();
}

#[test]
fn source_sync_target_current_is_public_during_ordinary_global_rate_glide() {
    let mut path = prepared();
    let raw = path.voice(0).sync_request;
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    path.source.engine.test_parameter_producer = Some(Arc::new(std::sync::Mutex::new(producer)));
    path.source.engine.set_speed(1.75).unwrap();
    path.drain();
    drain_parameter_messages(&mut consumer, &mut path.source.mixer, &mut path.timeline);
    path.render(1);
    assert_eq!(path.voice(0).sync_request, raw);
    assert_eq!(path.voice(0).source_playback.rate_target(), 1.75);
    assert_ne!(
        path.voice(0).source_playback.tempo_ratio().to_bits(),
        1.75_f64.to_bits()
    );
    let callback = path
        .source
        .engine
        .input_runtime_ownership
        .source_sync_status(0)
        .unwrap()
        .policy
        .unwrap();
    assert!(callback.target_current && !callback.settled);
    let status = path.feedback(0);
    assert_eq!(status["policy_mode"], "off");
    assert_eq!(status["policy_revision"], 1);
    assert_eq!(status["settled"], false);
    assert_eq!(status["target_current"], true);
    path.render(4097);
    let settled = path.feedback(0);
    assert_eq!(settled["settled"], true);
    assert_eq!(settled["target_current"], true);
    assert_eq!(path.feedback(1).get("target_current"), Some(&Value::Null));
}

#[test]
fn source_sync_target_current_survives_redundant_unsupported_attempt_on_native_pitch_glide() {
    use crate::audio_engine::pitch_runtime_tests::{Fixture, LOOP_END, LOOP_START};
    let mut fixture = Fixture::new(true);
    // Direct fixture-bank installation is not a source ACK. Use the exact real
    // SourcePublication/LoadSample claim already shared by endpoint route tests.
    crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources(&mut fixture, true);
    crate::audio_engine::ordinary_endpoint_reserve_tests::refresh_actual_runtime_bindings(&fixture);
    assert_eq!(
        fixture
            .control
            .engine
            .input_runtime_ownership
            .callback_feedback_key(0),
        fixture
            .control
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key()
    );
    let bindings = (0..NUM_SAMPLES)
        .map(|id| {
            (id < 2).then(|| {
                fixture
                    .control
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let mut maps =
        vec![crate::audio_engine::source_map_admission::SourceMapRow::default(); NUM_SAMPLES];
    maps[0].sync_requested = true;
    maps[1].sync_requested = true;
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![LOOP_START as f64 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(LOOP_END as f64 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            None,
            Some(maps),
            GlobalTimingPolicy::parse(Some(&wire(false, false, 1).to_string())).unwrap(),
        )
        .unwrap();
    let action = fixture.capture(0, "pitch.extra:1:set_retrigger");
    fixture.publish(&action);
    fixture.wait_worker(&[&action]);
    fixture.render_to(action.execution_target());
    fixture.render(1);
    assert!(action.effective(), "{:?}", action.reason());
    assert_eq!(
        fixture
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active && voice.sample_id == 0)
            .count(),
        1
    );
    assert!(
        fixture
            .voice(0)
            .sample
            .as_ref()
            .unwrap()
            .same_window(fixture.mixer.bank_for_measurement()[0].as_ref().unwrap())
    );
    let native = fixture.voice(0).stretch.native_state_address();
    assert_ne!(native, 0);
    let (pitch, key_lock, _) = fixture.voice(0).stretch.native_origin_tuple().unwrap();
    assert!(key_lock);
    assert_eq!(pitch.total_semitones(), 1);
    let raw = fixture.voice(0).sync_request;
    assert_eq!(
        (raw.mode, raw.global_revision),
        (SourceSyncMode::Off, Some(1))
    );
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    fixture.control.engine.test_parameter_producer =
        Some(Arc::new(std::sync::Mutex::new(producer)));
    // This headless fixture's input clock advertises the original 0.5 s master.
    // Publish it to BOTH real callback authorities through the normal rate API;
    // a clock hint alone must not pass the policy kernel's master fence.
    fixture
        .control
        .engine
        .set_speed_and_master_period(1.25, 0.5)
        .unwrap();
    fixture.drain();
    drain_parameter_messages(&mut consumer, &mut fixture.mixer, &mut fixture.transport);
    fixture.render(1);
    assert_eq!(fixture.transport.master_period_seconds(), Some(0.5));
    assert_eq!(fixture.voice(0).source_playback.rate_target(), 1.25);
    assert_ne!(
        fixture.voice(0).source_playback.tempo_ratio().to_bits(),
        1.25_f64.to_bits()
    );
    let before = fixture.voice(0).source_playback;
    fixture
        .control
        .engine
        .request_input_runtime_policy_adoption()
        .unwrap();
    fixture.drain();
    fixture.render(0);
    assert!(fixture.voice(0).source_playback.matches_exact(&before));
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).sync_request, raw);
    let callback = fixture
        .control
        .engine
        .input_runtime_ownership
        .source_sync_status(0)
        .unwrap()
        .policy
        .unwrap();
    assert!(callback.target_current && !callback.settled);
    let status: Value = Python::attach(|py| {
        let dictionary = fixture
            .control
            .engine
            .pad_source_sync_status(py, 0)
            .unwrap()
            .unwrap();
        let json = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (dictionary,))
            .unwrap()
            .extract::<String>()
            .unwrap();
        serde_json::from_str(&json).unwrap()
    });
    assert_eq!(status["policy_mode"], "off");
    assert_eq!(status["policy_revision"], 1);
    assert_eq!(status["settled"], false);
    assert_eq!(status["adoption"]["state"], "refused");
    assert_eq!(status["adoption"]["reason"], "unsupported");
    assert_eq!(
        status["adoption"]["target_policy_revision"],
        status["policy_revision"]
    );
    assert_eq!(status["target_current"], true);
}
