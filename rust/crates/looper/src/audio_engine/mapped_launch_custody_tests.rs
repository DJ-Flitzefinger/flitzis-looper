// Real source-qualified ordinary launch, callback reads and owner retirement.
// Constant fixture PCM proves addressing/lifetime, not musical or acoustic quality.

use super::*;
use crate::audio_engine::audio_stream::{
    drain_control_messages, drain_parameter_messages, render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, ImmediateAudioBufferRetirement,
};
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use crate::audio_engine::transport::TransportTimeline;
use crate::audio_engine::voice_slot::VoiceSlot;
use pyo3::types::PyDict;
use serde_json::json;
use std::sync::Arc;

const RATE: u32 = 48_000;
const PERIOD: f64 = 64.0 / RATE as f64;

struct LaunchPath {
    source: ColdCustody,
    timeline: TransportTimeline,
    scheduler: FixedCapacityScheduler<16>,
    feedback: Vec<crate::messages::AudioMessage>,
}

impl LaunchPath {
    fn new() -> Self {
        let mut source = runtime_fixture();
        source.engine.test_output_format = Some((RATE, 2));
        source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_render_format(RATE, 2)
            .unwrap();
        source.mixer.set_bpm_lock(true);
        source.mixer.set_speed(1.0);
        source.mixer.set_master_period(PERIOD);
        source.mixer.set_pad_key_lock(0, false);
        let mut timeline = TransportTimeline::new(RATE);
        assert!(timeline.set_master_period(PERIOD));
        Self {
            source,
            timeline,
            scheduler: FixedCapacityScheduler::new(),
            feedback: Vec::new(),
        }
    }

    fn drain(&mut self) {
        drain_control_messages(
            &mut self.source.consumer,
            &mut self.scheduler,
            self.timeline.output_frame(),
            &mut crate::messages::TriggerQuantization::Immediate,
            &mut self.timeline,
            &mut self.source.mixer,
            &mut self.feedback,
            &mut ImmediateAudioBufferRetirement,
        );
    }

    fn capture(&self) -> CapturedPadStart {
        self.source
            .engine
            .capture_pad_start(
                0,
                32.0 / f64::from(RATE),
                Some(64.0 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap()
    }

    fn publish_capture(&mut self, captured: &CapturedPadStart) {
        self.drain();
        assert!(
            self.source
                .engine
                .publish_pad_start(captured, None)
                .unwrap()
        );
        self.drain();
    }

    fn start(&mut self) -> CapturedPadStart {
        let captured = self.capture();
        self.publish_capture(&captured);
        assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
        assert_eq!(captured.target_frame(), Some(0));
        assert_eq!(captured.execution_frame(), Some(0));
        assert_eq!(captured.effective_frame(), Some(0));
        assert!(self.voice().source_playback.is_mapped());
        assert!(
            self.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&captured)
        );
        captured
    }

    fn voice(&self) -> &VoiceSlot {
        self.source
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 0)
            .unwrap()
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        self.render_with_retirement(frames, &mut ImmediateAudioBufferRetirement)
    }

    fn render_with_retirement(
        &mut self,
        frames: usize,
        retirement: &mut impl AudioBufferRetirement,
    ) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        let start = self.timeline.output_frame();
        render_scheduled_audio(
            &mut self.source.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            start,
            2,
            &mut self.timeline,
            &mut self.feedback,
            retirement,
        );
        self.timeline.advance_by_rendered_frames(frames);
        assert!(output.iter().all(|sample| sample.is_finite()));
        output
    }
}

fn variable_original_delivery(path: &LaunchPath) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    let mut expected = record(&path.source, 0);
    let accepted = &mut expected["record"]["accepted"];
    let first = accepted["anchors"][0].clone();
    let mut middle = accepted["anchors"][1].clone();
    middle["id"] = json!("fractional-variable-knot");
    middle["source_seconds_bits"] = json!(bits(48.0 / f64::from(RATE)));
    middle["beat_bits"] = json!(bits(0.6));
    let last = accepted["anchors"][1].clone();
    let interval = accepted["intervals"][0].clone();
    accepted["anchors"] = json!([first, middle, last]);
    accepted["intervals"] = json!([interval.clone(), interval]);
    seal(&mut expected);
    let (job, event) = stored_event(&path.source, &expected);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    (job, delivery)
}

fn publish_original(path: &LaunchPath, delivery: &LoaderEventDelivery, revision: u64) {
    // Exercise strict parser with the actual original Native object. No DTO can
    // construct this delivery, its source qualification or its held artifact.
    let row = Python::attach(|py| {
        let row = PyDict::new(py);
        row.set_item("sync_requested", true).unwrap();
        row.set_item("intent_revision", revision).unwrap();
        row.set_item("delivery", Py::new(py, delivery.clone()).unwrap())
            .unwrap();
        row.set_item("exact_quarters", py.None()).unwrap();
        crate::audio_engine::source_map_api::parse_rows(py, vec![row.unbind()])
            .unwrap()
            .pop()
            .unwrap()
    });
    publish(&path.source, row).unwrap();
}

fn verify_actual_reads(
    path: &LaunchPath,
    captured: &CapturedPadStart,
    count: usize,
) -> Vec<(usize, u64)> {
    let voice = path.voice();
    let geometry = captured.mapped_geometry().unwrap();
    let epoch = voice.source_playback.mapped_epoch().unwrap();
    let origin = captured.target_frame().unwrap();
    assert_eq!(
        voice.source_playback.mapped_output_frame(),
        Some(origin + count as u64)
    );
    assert_eq!(
        voice.source_playback.position(),
        geometry.position_at(epoch, origin + count as u64).unwrap()
    );
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut snapshot));
    assert_eq!(snapshot.retained_range(), Some(0..count as u64));
    assert!(
        snapshot.spans().len() > 3,
        "real reads must split map knots and wraps"
    );
    let mut addresses = Vec::with_capacity(count);
    for span in snapshot.spans() {
        let start = span.playback.mapped_output_frame().unwrap();
        assert_eq!(start, origin + addresses.len() as u64);
        for offset in 0..=span.frames {
            let absolute = start + offset as u64;
            let position = span.playback.position_at(offset).unwrap();
            assert_eq!(position, geometry.position_at(epoch, absolute).unwrap());
            // Independent physical inverse of the declared 0/48/64-frame map.
            let beat = geometry.start_beat()
                + ((absolute - origin) as f64 / 64.0).rem_euclid(geometry.exact_quarters());
            let expected = if beat < 0.6 {
                80.0 * beat
            } else {
                48.0 + 40.0 * (beat - 0.6)
            };
            let actual = position.frame as f64 + position.fraction;
            assert!(
                (actual - expected).abs() < 1e-9,
                "absolute={absolute} actual={actual} expected={expected}"
            );
            if offset < span.frames {
                addresses.push((position.frame, position.fraction.to_bits()));
            }
        }
    }
    assert_eq!(addresses.len(), count);
    addresses
}

#[derive(Default)]
struct StopOwnerRetirement {
    owner: Option<CapturedPadStart>,
}

impl AudioBufferRetirement for StopOwnerRetirement {
    fn retire_pad_start_target(&mut self, owner: CapturedPadStart) {
        assert!(self.owner.is_none());
        self.owner = Some(owner);
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_sample(&mut self, _: crate::messages::SampleBuffer) {}
    fn retire_prepared_stems(&mut self, _: crate::messages::PreparedStemSet) {}
    fn retire_constant_timing(
        &mut self,
        _: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn available_retirement_slots(&mut self) -> usize {
        usize::MAX
    }
}

#[test]
fn real_original_public_ordinary_launch_reads_canonical_map_and_retires_exact_owner() {
    let mut reference = None;
    for partitions in [&[1][..], &[17, 3, 29, 1, 64][..]] {
        let mut path = LaunchPath::new();
        let (_job, delivery) = variable_original_delivery(&path);
        publish_original(&path, &delivery, 1);
        let captured = path.start();
        let mut rendered = 0;
        let mut index = 0;
        while rendered < 257 {
            let frames = partitions[index % partitions.len()].min(257 - rendered);
            path.render(frames);
            rendered += frames;
            index += 1;
        }
        let actual = verify_actual_reads(&path, &captured, 257);
        if let Some(reference) = &reference {
            assert_eq!(&actual, reference);
        } else {
            reference = Some(actual);
        }
        let mut retirement = StopOwnerRetirement::default();
        path.source.mixer.stop_sample_rt(0, &mut retirement);
        assert!(retirement.owner.as_ref().unwrap().same_owner(&captured));
        assert!(
            !path
                .source
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            !path
                .source
                .mixer
                .voices
                .iter()
                .any(|voice| voice.mapped_start.is_some())
        );
    }
}

#[test]
fn clear_or_original_job_cancel_refuses_pending_launch_and_keeps_effective_original_reads() {
    for clear in [false, true] {
        let mut path = LaunchPath::new();
        let (job, delivery) = variable_original_delivery(&path);
        publish_original(&path, &delivery, 1);
        let effective = path.start();
        path.render(17);
        let pending = path.capture();
        let before = path.voice().source_playback;
        let expected_reason = if clear {
            publish(
                &path.source,
                SourceMapRow {
                    sync_requested: true,
                    intent_revision: 2,
                    delivery: None,
                    exact_quarters: None,
                },
            )
            .unwrap();
            "launch_cancelled"
        } else {
            job.cancel();
            "source_changed"
        };
        path.drain();
        let error = path
            .source
            .engine
            .publish_pad_start(&pending, None)
            .unwrap_err();
        assert!(error.to_string().contains(expected_reason), "{error}");
        assert!(path.voice().source_playback.matches_exact(&before));
        assert!(
            path.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&effective)
        );
        path.render(61);
        verify_actual_reads(&path, &effective, 78);
    }
}

#[test]
fn unsupported_mapped_speed_tuple_rejects_without_replacing_previous_scalar_voice() {
    let mut path = LaunchPath::new();
    let (_job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    path.source
        .mixer
        .set_pad_loop_region(0, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)));
    path.source.mixer.set_speed(1.25);
    assert!(path.source.mixer.play_sample(0, 0.6));
    let before = path.voice().source_playback;
    let captured = path.capture();
    path.publish_capture(&captured);
    assert_eq!(captured.state(), "rejected");
    assert_eq!(captured.reason(), Some("mapped_timing_unavailable"));
    assert!(path.voice().source_playback.matches_exact(&before));
    assert!(!path.voice().source_playback.is_mapped());
    assert!(path.voice().mapped_start.is_none());
    path.render(7);
    assert!(!path.voice().source_playback.matches_exact(&before));
}

fn publish_mixed_members(path: &LaunchPath, delivery: &LoaderEventDelivery) {
    let mut bindings = vec![None; NUM_SAMPLES];
    for id in [0, 1] {
        bindings[id] = Some(binding(&path.source, id));
    }
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0] = SourceMapRow {
        sync_requested: true,
        intent_revision: 1,
        delivery: Some(delivery.clone()),
        exact_quarters: None,
    };
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![32.0 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(64.0 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            None,
            Some(maps),
        )
        .unwrap();
}

fn capture_mixed_global(
    path: &LaunchPath,
) -> (
    crate::audio_engine::CapturedGlobalStart,
    [CapturedPadStart; 2],
) {
    let captured = Python::attach(|py| {
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = path
                    .source
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (
                    Py::new(py, binding).unwrap(),
                    32.0 / f64::from(RATE),
                    Some(64.0 / f64::from(RATE)),
                    None,
                )
            })
            .collect();
        path.source
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    let members = captured
        .resolve()
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|member| member.start)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    (captured, members)
}

#[test]
fn actual_midi_dispatcher_and_public_runtime_trigger_keep_the_original_mapped_start() {
    for midi in [true, false] {
        let mut path = LaunchPath::new();
        let (_job, delivery) = variable_original_delivery(&path);
        publish_original(&path, &delivery, 1);
        let captured = if midi {
            let runtime = path.source.engine.input_runtime.as_ref().unwrap();
            runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
            runtime.set_enabled(true);
            assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let event = loop {
                if let Some(event) = runtime.poll_event() {
                    break event;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "mapped MIDI dispatch timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            };
            assert!(event.direct && event.dispatched);
            assert_eq!(event.action_key.as_deref(), Some("pad.trigger:0"));
            let captured = event.captured_trigger.unwrap().captured_start();
            assert_eq!(captured.received_timestamp(), event.received_at_ns);
            assert_eq!(captured.target_frame(), None);
            captured
        } else {
            let runtime = path.source.engine.input_runtime.as_ref().unwrap();
            assert!(runtime.trigger_pad(0, 42));
            let message = path.source.consumer.pop().unwrap();
            let crate::messages::ControlMessage::CapturePadStartTarget(captured) = &message else {
                panic!("public runtime start must queue its original target first");
            };
            let captured = captured.clone();
            crate::audio_engine::audio_stream::process_control_message(
                message,
                &mut path.scheduler,
                0,
                &mut crate::messages::TriggerQuantization::Immediate,
                &mut path.timeline,
                &mut path.source.mixer,
                &mut path.feedback,
                &mut ImmediateAudioBufferRetirement,
            );
            assert_eq!(captured.received_timestamp(), 42);
            captured
        };
        path.drain();
        assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
        assert_eq!(
            (
                captured.target_frame(),
                captured.execution_frame(),
                captured.effective_frame()
            ),
            (Some(0), Some(0), Some(0))
        );
        assert!(
            path.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&captured)
        );
        for frames in [1, 13, 7, 37, 39] {
            path.render(frames);
        }
        verify_actual_reads(&path, &captured, 97);
    }
}

#[test]
fn actual_global_mapped_and_scalar_members_claim_one_common_original_frame() {
    let mut path = LaunchPath::new();
    let (_job, delivery) = variable_original_delivery(&path);
    publish_mixed_members(&path, &delivery);
    let (captured, members) = capture_mixed_global(&path);
    assert_eq!(captured.original_frame(), None);
    path.drain();
    assert_eq!(captured.original_frame(), Some(0));
    let ticket = path
        .source
        .engine
        .publish_captured_global_start(&captured)
        .unwrap()
        .unwrap();
    path.drain();
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
    for member in &members {
        assert_eq!(
            (
                member.target_frame(),
                member.execution_frame(),
                member.effective_frame()
            ),
            (Some(0), Some(0), Some(0))
        );
    }
    assert!(path.voice().source_playback.is_mapped());
    let scalar = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    assert!(!scalar.source_playback.is_mapped());
    assert!(scalar.mapped_start.is_none());
    for frames in [31, 7, 64, 1, 13] {
        path.render(frames);
    }
    verify_actual_reads(&path, &members[0], 116);
    let scalar = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    assert_eq!(scalar.source_playback.position().frame, 32 + 116 % 32);
    let mut history = SourceFeedSnapshot::new();
    assert!(scalar.source_feed_history.capture_into(&mut history));
    assert_eq!(history.retained_range(), Some(0..116));
    assert!(history.spans().all(|span| !span.playback.is_mapped()));
}

#[test]
fn one_unsupported_global_member_refuses_entire_group_before_replacing_previous_audio() {
    let mut path = LaunchPath::new();
    let (_job, delivery) = variable_original_delivery(&path);
    publish_mixed_members(&path, &delivery);
    path.source.mixer.set_speed(1.25);
    path.source
        .mixer
        .set_pad_loop_region(1, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)));
    assert!(path.source.mixer.play_sample(1, 0.4));
    let prior = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    let generation = prior.generation;
    let playback = prior.source_playback;
    let (captured, members) = capture_mixed_global(&path);
    path.drain();
    let ticket = path
        .source
        .engine
        .publish_captured_global_start(&captured)
        .unwrap()
        .unwrap();
    path.drain();
    assert_eq!(ticket.publication_status(), "rejected");
    assert_eq!(captured.state(), "rejected");
    assert_eq!(captured.reason(), Some("mapped_timing_unavailable"));
    assert!(
        members
            .iter()
            .all(|member| member.effective_frame().is_none())
    );
    assert!(
        !path
            .source
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    let retained = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    assert_eq!(retained.generation, generation);
    assert!(retained.source_playback.matches_exact(&playback));
    path.render(17);
    let continued = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    assert_eq!(continued.generation, generation);
    assert!(!continued.source_playback.matches_exact(&playback));
}

struct PausedRetirement {
    open: Arc<std::sync::atomic::AtomicBool>,
    capacity: usize,
    owner: crate::audio_engine::buffer_retirement::CallbackCloseOwner<
        crate::audio_engine::buffer_retirement::RtAudioBufferRetirement,
    >,
    _worker: crate::audio_engine::buffer_retirement::AudioBufferRetirementWorker,
}

impl PausedRetirement {
    fn new_full() -> Self {
        let open = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let gate = open.clone();
        let (mut owner, worker) = crate::audio_engine::buffer_retirement::create_audio_buffer_retirement_with_callback_paused(
            Box::new, move || {
                while !gate.load(std::sync::atomic::Ordering::Acquire) {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }).unwrap();
        let capacity = owner.state_mut().available_retirement_slots();
        let marker = Arc::new(std::sync::atomic::AtomicU8::new(0));
        while owner.state_mut().available_retirement_slots() > 0 {
            owner.state_mut().retire_cold_adoption(marker.clone());
        }
        Self {
            open,
            capacity,
            owner,
            _worker: worker,
        }
    }

    fn release(&self) {
        self.open.store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Drop for PausedRetirement {
    fn drop(&mut self) {
        self.release();
    }
}

#[test]
fn real_retirement_pressure_keeps_original_before_claim_and_retries_before_its_fixed_grid() {
    let mut path = LaunchPath::new();
    let (_job, delivery) = variable_original_delivery(&path);
    publish_mixed_members(&path, &delivery);
    path.source
        .mixer
        .set_pad_loop_region(1, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)));
    assert!(path.source.mixer.play_sample(1, 0.4));
    path.render(1);
    let captured = path.capture();
    drain_control_messages(
        &mut path.source.consumer,
        &mut path.scheduler,
        1,
        &mut crate::messages::TriggerQuantization::Grid { step_64ths: 16 },
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert_eq!(captured.target_frame(), Some(64));
    let generation = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap()
        .generation;
    let mut retirement = PausedRetirement::new_full();
    assert_eq!(retirement.owner.state_mut().available_retirement_slots(), 0);
    assert!(
        path.source
            .engine
            .publish_pad_start(&captured, None)
            .unwrap()
    );
    assert_eq!(
        drain_control_messages(
            &mut path.source.consumer,
            &mut path.scheduler,
            1,
            &mut crate::messages::TriggerQuantization::Immediate,
            &mut path.timeline,
            &mut path.source.mixer,
            &mut path.feedback,
            retirement.owner.state_mut()
        ),
        0
    );
    assert!(!captured.accepted() && captured.effective_frame().is_none());
    assert!(path.scheduler.is_empty());
    path.render_with_retirement(17, retirement.owner.state_mut());
    assert!(
        !path
            .source
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    assert_eq!(
        path.source
            .mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == 1)
            .unwrap()
            .generation,
        generation
    );
    retirement.release();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let full_capacity = retirement.capacity;
    while retirement.owner.state_mut().available_retirement_slots() < full_capacity {
        assert!(
            std::time::Instant::now() < deadline,
            "retirement worker did not free admission capacity"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    drain_control_messages(
        &mut path.source.consumer,
        &mut path.scheduler,
        18,
        &mut crate::messages::TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.feedback,
        retirement.owner.state_mut(),
    );
    assert!(captured.accepted());
    assert_eq!(captured.target_frame(), Some(64));
    assert_eq!(captured.execution_frame(), Some(64));
    assert_eq!(captured.effective_frame(), None);
    path.render_with_retirement(46, retirement.owner.state_mut());
    path.render_with_retirement(77, retirement.owner.state_mut());
    assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
    assert_eq!(captured.effective_frame(), Some(64));
    verify_actual_reads(&path, &captured, 77);
}

#[test]
fn changed_real_master_anchor_before_quantized_claim_refuses_map_and_keeps_prior_audio() {
    let mut path = LaunchPath::new();
    let (_job, delivery) = variable_original_delivery(&path);
    publish_mixed_members(&path, &delivery);
    path.source
        .mixer
        .set_pad_loop_region(1, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)));
    assert!(path.source.mixer.play_sample(1, 0.4));
    path.render(1);
    let captured = path.capture();
    drain_control_messages(
        &mut path.source.consumer,
        &mut path.scheduler,
        1,
        &mut crate::messages::TriggerQuantization::Grid { step_64ths: 16 },
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert_eq!(captured.target_frame(), Some(64));
    assert!(
        path.source
            .engine
            .publish_pad_start(&captured, None)
            .unwrap()
    );
    path.drain();
    assert!(captured.accepted());
    assert_eq!(captured.execution_frame(), Some(64));
    let generation = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap()
        .generation;
    assert!(path.timeline.anchor_beat_position_at_frame(0.25, 1));
    path.render(63);
    let mut expected = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap()
        .source_playback;
    assert!(expected.advance(1));
    let output = path.render(1);
    assert_eq!(captured.state(), "rejected");
    assert_eq!(captured.reason(), Some("mapped_timing_unavailable"));
    assert_eq!(captured.target_frame(), Some(64));
    assert_eq!(captured.effective_frame(), None);
    assert!(
        !path
            .source
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    let continued = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    assert_eq!(continued.generation, generation);
    assert!(continued.source_playback.matches_exact(&expected));
    assert!(output.iter().any(|sample| sample.abs() > 0.0));
}

fn repeat_current_rate_through_public_parameter_drain(path: &mut LaunchPath) {
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    path.source.engine.test_parameter_producer = Some(Arc::new(std::sync::Mutex::new(producer)));
    let epoch = path.timeline;
    let playbacks = path
        .source
        .mixer
        .voices
        .iter()
        .filter(|voice| voice.active)
        .map(|voice| (voice.sample_id, voice.generation, voice.source_playback))
        .collect::<Vec<_>>();
    for _ in 0..2 {
        path.source
            .engine
            .set_speed_and_master_period(1.0, PERIOD)
            .unwrap();
        drain_parameter_messages(&mut consumer, &mut path.source.mixer, &mut path.timeline);
        assert!(consumer.is_empty());
        assert!(
            path.timeline.matches_clock_epoch(&epoch),
            "an equal output period must retain its original beat anchor"
        );
        for (id, generation, playback) in &playbacks {
            let voice = path
                .source
                .mixer
                .voices
                .iter()
                .find(|voice| {
                    voice.active && voice.sample_id == *id && voice.generation == *generation
                })
                .unwrap();
            assert!(voice.source_playback.matches_exact(playback));
        }
    }
}

#[test]
fn repeated_equal_public_rate_keeps_pending_mapped_target_epoch_and_prior_audio() {
    let mut path = LaunchPath::new();
    let (_job, delivery) = variable_original_delivery(&path);
    publish_mixed_members(&path, &delivery);
    path.source
        .mixer
        .set_pad_loop_region(1, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)));
    assert!(path.source.mixer.play_sample(1, 0.4));
    path.render(1);
    let captured = path.capture();
    drain_control_messages(
        &mut path.source.consumer,
        &mut path.scheduler,
        1,
        &mut crate::messages::TriggerQuantization::Grid { step_64ths: 16 },
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert_eq!(captured.target_frame(), Some(64));
    assert!(
        path.source
            .engine
            .publish_pad_start(&captured, None)
            .unwrap()
    );
    path.drain();
    assert!(captured.accepted());
    let original_epoch = path.timeline;
    let scalar_generation = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap()
        .generation;
    repeat_current_rate_through_public_parameter_drain(&mut path);
    path.render(17);
    repeat_current_rate_through_public_parameter_drain(&mut path);
    assert!(path.timeline.matches_clock_epoch(&original_epoch));
    assert_eq!(captured.effective_frame(), None);
    let scalar = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    assert_eq!(scalar.generation, scalar_generation);
    assert_eq!(scalar.source_playback.position().frame, 32 + 18 % 32);
    path.render(46);
    path.render(97);
    assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
    assert_eq!(
        (
            captured.target_frame(),
            captured.execution_frame(),
            captured.effective_frame()
        ),
        (Some(64), Some(64), Some(64))
    );
    assert!(
        path.voice()
            .mapped_start
            .as_ref()
            .unwrap()
            .same_owner(&captured)
    );
    verify_actual_reads(&path, &captured, 97);
}

#[test]
fn repeated_equal_public_rate_keeps_active_map_epoch_owner_and_canonical_reads() {
    let mut path = LaunchPath::new();
    let (_job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    let captured = path.start();
    path.render(17);
    let original_epoch = path.timeline;
    for frames in [3, 29, 1, 64, 31] {
        repeat_current_rate_through_public_parameter_drain(&mut path);
        assert!(path.timeline.matches_clock_epoch(&original_epoch));
        assert!(
            path.voice()
                .mapped_start
                .as_ref()
                .unwrap()
                .same_owner(&captured)
        );
        path.render(frames);
    }
    verify_actual_reads(&path, &captured, 145);
}
