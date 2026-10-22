//! Generated PCM, genuine copy-first/ACK, public capture and actual callback reads.
//! These engineering coordinates do not establish musical or hearing acceptance.

use super::*;
use crate::audio_engine::analysis_jobs::OfflineAnalysisJob;
use crate::audio_engine::audio_stream::{drain_control_messages, render_scheduled_audio};
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::InputRuntime;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::source_beatmap::{SourceBeatmap, editor};
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use crate::audio_engine::source_map_test_support::ColdCustody;
use crate::audio_engine::timing::InputClock;
use crate::audio_engine::transport::TransportTimeline;
use crate::audio_engine::voice_slot::VoiceSlot;
use crate::audio_engine::{CapturedPadStart, LoaderEventDelivery};
use crate::messages::{AudioMessage, TriggerQuantization};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const MASTER: f64 = 512.0 / RATE as f64;
const SOURCE: f64 = 640.0 / RATE as f64;
const END: usize = 1024;

struct Path {
    source: ColdCustody,
    timeline: TransportTimeline,
    scheduler: FixedCapacityScheduler<16>,
    events: Vec<AudioMessage>,
}

impl Path {
    fn new(bpm_lock: bool) -> Self {
        let pcm = (0..4096)
            .map(|frame| (frame * 11 - 16000) as i16)
            .collect::<Vec<_>>();
        let mut source = ColdCustody::new_selected_with_signal(None, &pcm);
        source.engine.test_output_format = Some((RATE, 2));
        source.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            source.producer.clone(),
            InputClock::new(),
            source.engine.input_runtime_ownership.clone(),
        ));
        source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_render_format(RATE, 2)
            .unwrap();
        source.mixer.set_bpm_lock(bpm_lock);
        source.mixer.set_master_period(MASTER);
        source.mixer.set_speed(1.5);
        source.mixer.set_volume(1.0);
        for id in 0..2 {
            source.mixer.set_pad_bpm(id, Some(60.0 / SOURCE));
            source.mixer.set_pad_key_lock(id, false);
        }
        let mut timeline = TransportTimeline::new(RATE);
        assert!(timeline.set_master_period(MASTER));
        Self {
            source,
            timeline,
            scheduler: FixedCapacityScheduler::new(),
            events: Vec::new(),
        }
    }

    fn publish(
        &self,
        policy: Option<Value>,
        requested: bool,
        delivery: Option<&LoaderEventDelivery>,
    ) {
        Python::attach(|py| {
            let bindings = (0..NUM_SAMPLES)
                .map(|id| {
                    (id < 2)
                        .then(|| {
                            self.source
                                .engine
                                .current_input_runtime_pad_binding(id)
                                .unwrap()
                                .unwrap()
                        })
                        .map(|binding| Py::new(py, binding).unwrap())
                })
                .collect();
            let rows = (0..NUM_SAMPLES)
                .map(|id| {
                    let row = PyDict::new(py);
                    row.set_item("sync_requested", id < 2 && requested).unwrap();
                    row.set_item("intent_revision", 7_u64).unwrap();
                    row.set_item(
                        "delivery",
                        if id == 0 {
                            delivery.map(|value| Py::new(py, value.clone()).unwrap())
                        } else {
                            None
                        },
                    )
                    .unwrap();
                    row.set_item("exact_quarters", py.None()).unwrap();
                    row.unbind()
                })
                .collect();
            self.source
                .engine
                .set_input_runtime_state(
                    py,
                    true,
                    (0..NUM_SAMPLES).map(|id| id < 2).collect(),
                    vec![0.0; NUM_SAMPLES],
                    vec![Some(END as f64 / f64::from(RATE)); NUM_SAMPLES],
                    Some(bindings),
                    None,
                    Some(rows),
                    policy.map(|value| value.to_string()),
                )
                .unwrap();
        });
    }

    fn drain(&mut self) {
        drain_control_messages(
            &mut self.source.consumer,
            &mut self.scheduler,
            self.timeline.output_frame(),
            &mut TriggerQuantization::Immediate,
            &mut self.timeline,
            &mut self.source.mixer,
            &mut self.events,
            &mut ImmediateAudioBufferRetirement,
        );
    }

    fn capture(&self) -> CapturedPadStart {
        self.source
            .engine
            .capture_pad_start(
                0,
                0.0,
                Some(END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap()
    }

    fn start(&mut self) -> CapturedPadStart {
        let captured = self.capture();
        self.drain();
        assert!(
            self.source
                .engine
                .publish_pad_start(&captured, None)
                .unwrap()
        );
        self.drain();
        captured
    }

    fn voice(&self, id: usize) -> &VoiceSlot {
        self.source
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .unwrap()
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        render_scheduled_audio(
            &mut self.source.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.timeline.output_frame(),
            2,
            &mut self.timeline,
            &mut self.events,
            &mut ImmediateAudioBufferRetirement,
        );
        self.timeline.advance_by_rendered_frames(frames);
        output
    }

    fn feedback(&self, id: usize) -> Value {
        Python::attach(|py| {
            let value = self
                .source
                .engine
                .pad_source_sync_status(py, id)
                .unwrap()
                .unwrap();
            let encoded = py
                .import("json")
                .unwrap()
                .call_method1("dumps", (value,))
                .unwrap()
                .extract::<String>()
                .unwrap();
            serde_json::from_str(&encoded).unwrap()
        })
    }

    fn map(&self) -> (OfflineAnalysisJob, LoaderEventDelivery) {
        let manifest: Value =
            serde_json::from_str(&self.source.engine.cold_source_manifest(0).unwrap().unwrap())
                .unwrap();
        let map =
            SourceBeatmap::manual(manifest["descriptor"]["decoder"].clone(), 0.0, MASTER).unwrap();
        let mut value: Value = serde_json::from_str(map.canonical_record()).unwrap();
        let template = value["record"]["accepted"]["anchors"][0].clone();
        value["record"]["accepted"]["anchors"] = Value::Array(
            [(0, 0.0_f64), (384, 1.0), (END, 2.0)]
                .into_iter()
                .enumerate()
                .map(|(index, (frame, beat))| {
                    let mut anchor = template.clone();
                    anchor["id"] = json!(format!("authored-policy-{index}"));
                    anchor["source_seconds_bits"] = json!(format!(
                        "{:016x}",
                        (frame as f64 / f64::from(RATE)).to_bits()
                    ));
                    anchor["beat_bits"] = json!(format!("{:016x}", beat.to_bits()));
                    anchor
                })
                .collect(),
        );
        let interval = value["record"]["accepted"]["intervals"][0].clone();
        value["record"]["accepted"]["intervals"] = json!([interval.clone(), interval]);
        let map = editor::seal(value).unwrap();
        let job = self
            .source
            .engine
            .begin_source_beatmap_store_async(0, map.canonical_record().into())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let terminal = Python::attach(|py| {
                job.source_beatmap_async_status(py)
                    .unwrap()
                    .unwrap()
                    .bind(py)
                    .get_item("terminal")
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            });
            if terminal {
                break;
            }
            assert!(Instant::now() < deadline, "real map worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        let delivery = Python::attach(|py| {
            while let Some(event) = self.source.engine.poll_loader_events(py).unwrap() {
                let event = event.bind(py).cast::<PyDict>().unwrap();
                if event
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap()
                    == "source_beatmap_completed"
                {
                    let owner = event.get_item("_native_event_owner").unwrap().unwrap();
                    let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
                    assert!(
                        job.resolve_source_beatmap_artifact_completion(&delivery)
                            .unwrap()
                            .is_some()
                    );
                    return delivery.clone();
                }
            }
            panic!("genuine private map delivery required");
        });
        (job, delivery)
    }
}

fn wire(sync: bool, realtime: bool, revision: u64) -> Value {
    json!({"schema_version":1,"sync":sync,"realtime_bpm":realtime,"revision":revision})
}

fn verify_reads(path: &Path, mode: SourceSyncMode, frames: usize) {
    let mut history = SourceFeedSnapshot::new();
    assert!(path.voice(0).source_feed_history.capture_into(&mut history));
    assert_eq!(history.retained_range(), Some(0..frames as u64));
    let mut output = 0;
    for span in history.spans() {
        for offset in 0..=span.frames {
            let n = output + offset;
            let expected = match mode {
                SourceSyncMode::Mapped => {
                    let beat = (n as f64 / 512.0).rem_euclid(2.0);
                    if beat < 1.0 {
                        384.0 * beat
                    } else {
                        384.0 + 640.0 * (beat - 1.0)
                    }
                }
                SourceSyncMode::Fixed => (n as f64 * 1.25).rem_euclid(END as f64),
                _ => (n as f64 * 1.5).rem_euclid(END as f64),
            };
            let position = span.playback.position_at(offset).unwrap();
            let actual = position.frame as f64 + position.fraction;
            assert!(
                (actual - expected).abs() < 1e-8,
                "mode={mode:?} n={n} actual={actual} expected={expected}"
            );
        }
        output += span.frames;
    }
    assert_eq!(output, frames);
}

#[test]
fn four_explicit_modes_drive_real_callback_reads_feedback_and_partition_invariant_pcm() {
    for (sync, realtime, mode) in [
        (false, false, SourceSyncMode::Off),
        (false, true, SourceSyncMode::Off),
        (true, false, SourceSyncMode::Fixed),
        (true, true, SourceSyncMode::Mapped),
    ] {
        let mut captures = Vec::new();
        for partitions in [&[31, 7, 113][..], &[128, 17, 43][..]] {
            // Explicit OFF overrides real old BPM matching; explicit Mapped is
            // tested with old BPM Lock OFF and the same real fixed master.
            let mut path = Path::new(!sync);
            let original = path.timeline;
            let owned = sync.then(|| path.map());
            path.publish(
                Some(wire(sync, realtime, 11)),
                true,
                owned
                    .as_ref()
                    .filter(|_| realtime)
                    .map(|(_, delivery)| delivery),
            );
            let captured = path.start();
            assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
            assert_eq!(captured.sync_request().mode, mode);
            let mut pcm = Vec::new();
            let mut index = 0;
            while pcm.len() / 2 < 1537 {
                let count = partitions[index % partitions.len()].min(1537 - pcm.len() / 2);
                pcm.extend(path.render(count));
                index += 1;
            }
            verify_reads(&path, mode, 1537);
            assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
            assert!(path.timeline.matches_clock_epoch(&original));
            let feedback = path.feedback(0);
            assert_eq!(feedback["policy_mode"], mode.name());
            assert_eq!(feedback["policy_revision"], 11);
            assert_eq!(feedback["policy_sync"], sync);
            assert_eq!(feedback["policy_realtime_bpm"], realtime);
            assert_eq!(feedback["settled"], true);
            assert_eq!(feedback["effective"], sync);
            assert_eq!(
                feedback["map_revision"].is_string(),
                mode == SourceSyncMode::Mapped
            );
            captures.push(pcm);
        }
        assert_eq!(captures[0], captures[1]);
    }
}

#[test]
fn active_fixed_policy_and_full_identity_survive_requested_off_and_away_back_until_restart() {
    let mut path = Path::new(true);
    path.publish(Some(wire(true, false, 0)), true, None);
    assert_eq!(path.start().state(), "applied");
    path.render(23);
    let first = path.feedback(0);
    let pending = path.capture();
    path.drain();
    path.publish(Some(wire(false, false, 0)), true, None);
    assert!(
        path.source
            .engine
            .publish_pad_start(&pending, None)
            .is_err()
    );
    path.render(13);
    assert_eq!(path.feedback(0), first);
    path.publish(Some(wire(true, false, 2)), true, None);
    path.render(11);
    assert_eq!(path.feedback(0), first);
    path.publish(Some(wire(false, false, 3)), true, None);
    let restarted = path.start();
    assert_eq!(restarted.state(), "applied");
    path.render(5);
    let next = path.feedback(0);
    assert_eq!(next["policy_mode"], "off");
    assert_eq!(next["policy_revision"], 3);
    assert_eq!(next["effective"], false);
    assert_eq!(path.voice(0).source_playback.tempo_ratio(), 1.5);
}

#[test]
fn fixed_missing_or_out_of_range_timing_refuses_and_does_not_replace_old_audio() {
    for bpm in [None, Some(60.0 / (MASTER * 8.0))] {
        let mut path = Path::new(true);
        path.publish(Some(wire(false, false, 1)), true, None);
        let original = path.start();
        assert_eq!(original.state(), "applied");
        path.render(17);
        path.source.mixer.set_pad_bpm(0, bpm);
        path.publish(Some(wire(true, false, 2)), true, None);
        let refused = path.start();
        assert_eq!(refused.state(), "rejected");
        assert_eq!(refused.reason(), Some("timing_unavailable"));
        assert_eq!(path.voice(0).sync_request.global_revision, Some(1));
        assert_eq!(path.voice(0).sync_request.mode, SourceSyncMode::Off);
        path.render(9);
        assert_eq!(path.feedback(0)["policy_revision"], 1);
    }
}

#[test]
fn runtime_midi_fixed_and_global_fixed_off_captures_adopt_the_actual_request() {
    for midi in [false, true] {
        let mut path = Path::new(false);
        path.publish(Some(wire(true, false, 19)), true, None);
        let runtime = path.source.engine.input_runtime.as_ref().unwrap();
        if midi {
            runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
            runtime.set_enabled(true);
            assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if let Some(event) = runtime.poll_event() {
                    assert!(event.direct && event.dispatched);
                    assert_eq!(
                        event
                            .captured_trigger
                            .unwrap()
                            .captured_start()
                            .sync_request()
                            .mode,
                        SourceSyncMode::Fixed
                    );
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        } else {
            assert!(runtime.trigger_pad(0, 42));
        }
        path.drain();
        path.render(13);
        assert_eq!(path.feedback(0)["policy_mode"], "fixed");
        assert_eq!(path.feedback(0)["effective"], true);
    }
    for sync in [false, true] {
        // GLOBAL OFF must override enabled old BPM matching for every member.
        let mut path = Path::new(!sync);
        let original = path.timeline;
        path.publish(Some(wire(sync, false, 23)), true, None);
        let captured = Python::attach(|py| {
            let entries = [0, 1]
                .into_iter()
                .map(|id| {
                    (
                        Py::new(
                            py,
                            path.source
                                .engine
                                .current_input_runtime_pad_binding(id)
                                .unwrap()
                                .unwrap(),
                        )
                        .unwrap(),
                        0.0,
                        Some(END as f64 / f64::from(RATE)),
                        None,
                    )
                })
                .collect();
            path.source
                .engine
                .capture_global_start(py, entries, None)
                .unwrap()
        });
        path.drain();
        assert!(
            path.source
                .engine
                .publish_captured_global_start(&captured)
                .unwrap()
                .is_some()
        );
        path.drain();
        let pcm = path.render(13);
        assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
        assert_eq!(captured.state(), "applied");
        assert!(path.timeline.matches_clock_epoch(&original));
        let mode = if sync {
            SourceSyncMode::Fixed
        } else {
            SourceSyncMode::Off
        };
        let ratio = if sync { 1.25 } else { 1.5 };
        verify_reads(&path, mode, 13);
        for id in 0..2 {
            let voice = path.voice(id);
            assert_eq!(voice.sync_request.mode, mode);
            assert_eq!(voice.sync_request.global_revision, Some(23));
            assert_eq!(voice.sync_request.policy_sync, Some(sync));
            assert_eq!(voice.sync_request.policy_realtime_bpm, Some(false));
            assert!((voice.source_playback.tempo_ratio() - ratio).abs() < 1e-12);
            assert!(voice.mapped_start.is_none());
            let position = voice.source_playback.position();
            assert!((position.frame as f64 + position.fraction - 13.0 * ratio).abs() < 1e-8);
            let observed = path.feedback(id);
            assert_eq!(observed["policy_mode"], mode.name());
            assert_eq!(observed["policy_revision"], 23);
            assert_eq!(observed["policy_sync"], sync);
            assert_eq!(observed["policy_realtime_bpm"], false);
            assert_eq!(observed["settled"], true);
            assert_eq!(observed["effective"], sync);
            assert!(observed["map_revision"].is_null());
        }
    }
}

#[test]
fn old_legacy_matching_and_explicit_pad_optout_have_different_actual_scalar_ack() {
    let mut path = Path::new(true);
    path.publish(None, false, None);
    assert_eq!(path.start().state(), "applied");
    path.render(19);
    assert!((path.voice(0).source_playback.tempo_ratio() - 1.25).abs() < 1e-12);
    let old = path.feedback(0);
    assert_eq!(old["policy_mode"], "legacy");
    assert_eq!(old["effective"], false);
    assert!(old["policy_revision"].is_null());
    path.publish(Some(wire(true, false, 1)), false, None);
    path.render(7);
    assert_eq!(path.feedback(0), old);
    assert_eq!(path.start().state(), "applied");
    path.render(11);
    let off = path.feedback(0);
    assert_eq!(off["policy_mode"], "off");
    assert_eq!(off["policy_revision"], 1);
    assert_eq!(off["effective"], false);
    assert_eq!(path.voice(0).source_playback.tempo_ratio(), 1.5);
}

#[test]
fn same_revision_same_mode_different_frozen_choices_cancel_old_capture_without_borrowing_ack() {
    let mut path = Path::new(true);
    path.publish(Some(wire(false, false, 0)), true, None);
    assert_eq!(path.start().state(), "applied");
    path.render(13);
    let old = path.feedback(0);
    let captured = path.capture();
    path.drain();
    path.publish(Some(wire(false, true, 0)), true, None);
    assert!(
        path.source
            .engine
            .publish_pad_start(&captured, None)
            .is_err()
    );
    path.render(17);
    assert_eq!(path.feedback(0), old);
    assert_eq!(path.start().state(), "applied");
    path.render(7);
    let adopted = path.feedback(0);
    assert_eq!(adopted["policy_revision"], 0);
    assert_eq!(adopted["policy_mode"], "off");
    assert_eq!(adopted["policy_realtime_bpm"], true);

    let mut mapped = Path::new(true);
    let (_job, delivery) = mapped.map();
    mapped.publish(None, true, Some(&delivery));
    let captured = mapped.capture();
    mapped.drain();
    mapped.publish(Some(wire(true, true, 0)), true, Some(&delivery));
    assert!(
        mapped
            .source
            .engine
            .publish_pad_start(&captured, None)
            .is_err()
    );
    assert!(!mapped.source.mixer.voices.iter().any(|voice| voice.active));
}

#[test]
fn fixed_and_off_global_rate_targets_use_the_adopted_policy_and_fixed_ack_waits_for_glide() {
    use crate::audio_engine::audio_stream::drain_parameter_messages;
    use std::sync::{Arc, Mutex};
    for sync in [false, true] {
        let mut path = Path::new(true);
        path.publish(Some(wire(sync, false, 5)), true, None);
        assert_eq!(path.start().state(), "applied");
        path.render(31);
        let generation = path.voice(0).generation;
        let (producer, mut consumer) = rtrb::RingBuffer::new(16);
        path.source.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
        let period = MASTER / 0.75;
        path.source
            .engine
            .set_speed_and_master_period(0.75, period)
            .unwrap();
        path.drain();
        drain_parameter_messages(&mut consumer, &mut path.source.mixer, &mut path.timeline);
        path.render(1);
        let preparing = path.feedback(0);
        assert_eq!(preparing["policy_revision"], 5);
        assert_eq!(preparing["settled"], false);
        assert_eq!(preparing["effective"], false);
        path.render(9000);
        let expected = if sync { SOURCE / period } else { 0.75 };
        assert!((path.voice(0).source_playback.tempo_ratio() - expected).abs() < 1e-12);
        assert_eq!(path.voice(0).generation, generation);
        let settled = path.feedback(0);
        assert_eq!(settled["policy_revision"], 5);
        assert_eq!(settled["settled"], true);
        assert_eq!(settled["effective"], sync);
        assert!(settled["map_revision"].is_null());
    }
}

#[test]
fn missing_map_or_actual_master_cannot_fall_back_or_create_fixed_effectiveness() {
    let mut path = Path::new(false);
    path.publish(Some(wire(true, true, 2)), true, None);
    assert!(
        path.source
            .engine
            .capture_pad_start(
                0,
                0.0,
                Some(END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None
            )
            .is_err()
    );
    path.source.mixer.set_bpm_lock(false); // Explicitly remove only the actual mixer master.
    path.publish(Some(wire(true, false, 3)), true, None);
    let fixed = path.start();
    assert_eq!(fixed.state(), "rejected");
    assert_eq!(fixed.reason(), Some("timing_unavailable"));
    let (_job, delivery) = path.map();
    path.publish(Some(wire(true, true, 4)), true, Some(&delivery));
    let mapped = path.start();
    assert_eq!(mapped.state(), "rejected");
    assert_eq!(mapped.reason(), Some("mapped_timing_unavailable"));
    assert_eq!(path.timeline.master_period_seconds(), Some(MASTER));
    assert!(!path.source.mixer.voices.iter().any(|voice| voice.active));
}

#[test]
fn genuine_source_public_policy_change_waits_for_whole_native_pair_reservation() {
    let mut path = Path::new(true);
    path.publish(Some(wire(true, false, 1)), true, None);
    assert_eq!(path.start().state(), "applied");
    path.render(17);
    let old = path.feedback(0);
    let captured = [0, 1].map(|id| {
        path.source
            .engine
            .current_input_runtime_pad_binding(id)
            .unwrap()
            .unwrap()
    });
    let ownership = &path.source.engine.input_runtime_ownership;
    let slots = [0, 1].map(|id| ownership.slot_snapshot(id).unwrap());
    let before = (0..NUM_SAMPLES)
        .map(|id| {
            (
                ownership.runtime_revision(id),
                ownership.launch_revision(id),
            )
        })
        .collect::<Vec<_>>();
    ownership.reserve_pair(&slots, 571).unwrap();
    let result = Python::attach(|py| {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                captured
                    .get(id)
                    .map(|value| Py::new(py, value.clone()).unwrap())
            })
            .collect();
        path.source.engine.set_input_runtime_state(
            py,
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![0.0; NUM_SAMPLES],
            vec![Some(END as f64 / f64::from(RATE)); NUM_SAMPLES],
            Some(bindings),
            None,
            None,
            Some(wire(false, false, 2).to_string()),
        )
    });
    assert!(result.is_err());
    assert_eq!(
        (0..NUM_SAMPLES)
            .map(|id| (
                ownership.runtime_revision(id),
                ownership.launch_revision(id)
            ))
            .collect::<Vec<_>>(),
        before
    );
    ownership.release_pair(&slots, 571).unwrap();
    path.render(7);
    assert_eq!(path.feedback(0), old);
    path.publish(Some(wire(false, false, 2)), true, None);
    assert_eq!(path.start().state(), "applied");
    path.render(7);
    assert_eq!(path.feedback(0)["policy_revision"], 2);
}
