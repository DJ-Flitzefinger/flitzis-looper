//! HC21 composes real Cold/QM/Window preparation and own callback ACKs with Hold/Pair.
//! Generated quarter pulses are fixture truth, not musical or device acceptance.

use super::*;
use crate::audio_engine::accepted_timing_refresh::{self, AcceptedTimingRefreshTicket};
use crate::audio_engine::audio_stream::drain_parameter_messages;
use crate::audio_engine::cold_load::admit_for_format_selected;
use crate::audio_engine::cold_residency::ResidentLoadHint;
use crate::audio_engine::constant_timing::{self, AcceptedTimingProjection, ConstantTimingTicket};
use crate::audio_engine::input_runtime_binding::{self, InputRuntimePadBinding};
use crate::audio_engine::pad_start::CapturedPadStart;
use crate::audio_engine::resident_relocation::{
    self, ResidentWindowTicket, WindowRequest, launch_with_producer, prepare_window_with_producer,
};
use crate::messages::{ControlMessage, LoaderEvent};
use flitzis_looper_analysis::tempo_acceptance::{
    IndependentTimingOrigin, TimingAcceptanceDecision, TimingIntent,
};
use flitzis_looper_analysis::tempo_evidence::TimingBound;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods};
use serde_json::json;
use std::time::{Duration, Instant};

const QUARTER_SECONDS: f64 = 60.0 / 119.999;
const LOOP_END_SECONDS: f64 = 1.75;
// Deliberate manual fixture intent exercises signed fractional source coordinates.
// This origin is not a measured downbeat of the generated quarter pulses.
const ORIGIN_SECONDS: f64 = -0.125_012_3;

struct RefreshFixture {
    callback: CallbackFixture,
    _directory: tempfile::TempDir,
}

fn wait_until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "owned refresh worker timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The WAV integers independently define each quarter; the native loader supplies
/// the actual complete reader, digest, finite PCM and original source lease.
fn pulse_wav(path: &std::path::Path) {
    let frames = (64.0 * QUARTER_SECONDS * f64::from(RATE)).ceil() as usize;
    let mut integers = vec![0_i16; frames];
    for quarter in 0..64 {
        let frame = (quarter as f64 * QUARTER_SECONDS * f64::from(RATE)).round() as usize;
        for (offset, value) in [32_767_i16, -16_384, 8_192, -4_096].into_iter().enumerate() {
            integers[frame + offset * 6..frame + (offset + 1) * 6].fill(value);
        }
    }
    let bytes = (integers.len() * 2) as u32;
    let mut wav = Vec::with_capacity(bytes as usize + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for integer in integers {
        wav.extend_from_slice(&integer.to_le_bytes());
    }
    std::fs::write(path, wav).unwrap();
}

impl RefreshFixture {
    fn new() -> Self {
        let callback = CallbackFixture::new(32);
        let scratch = std::env::temp_dir().join("codex");
        std::fs::create_dir_all(&scratch).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("goal-r2-native-hold-refresh-")
            .tempdir_in(scratch)
            .unwrap();
        let source = directory.path().join("owned-quarter-pulses.wav");
        pulse_wav(&source);
        let mut fixture = Self {
            callback,
            _directory: directory,
        };
        let request = admit_for_format_selected(
            &fixture.callback.control.engine,
            SOURCE,
            source.to_string_lossy().into_owned(),
            (
                false,
                false,
                true,
                Some(ResidentLoadHint {
                    start_s: 0.0,
                    end_s: LOOP_END_SECONDS,
                    key_lock: false,
                }),
            ),
            fixture.callback.control.producer.clone(),
            (1, RATE, fixture._directory.path().join("samples")),
        )
        .unwrap();
        wait_until(|| {
            fixture.callback.drain();
            fixture.callback.control.engine.cold_loading[SOURCE].load(Ordering::Acquire) == 0
        });
        let mut accepted = false;
        while let Ok(event) = fixture
            .callback
            .control
            .engine
            .loader_rx
            .lock()
            .unwrap()
            .try_recv()
        {
            match event.into_test_payload() {
                LoaderEvent::Success { request_id, .. } if request_id == request => accepted = true,
                LoaderEvent::Error {
                    request_id, error, ..
                } if request_id == request => {
                    panic!("actual source preparation failed: {error}")
                }
                _ => {}
            }
        }
        assert!(
            accepted,
            "actual source loader did not publish its own success"
        );
        assert!(
            !fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(SOURCE))
        );
        let initial = fixture.sample(SOURCE);
        assert_eq!(initial.resident_start(), 0);
        assert_eq!(
            initial.resident_end(),
            (LOOP_END_SECONDS * f64::from(RATE)) as usize
        );
        constant_timing::set_intent(
            &fixture.callback.control.engine,
            &fixture.callback.control.producer,
            SOURCE,
            TimingIntent::Automatic,
        )
        .unwrap();
        assert_eq!(fixture.callback.drain(), 1);
        fixture.publish_timing(SOURCE, "initial explicit generated-quarter decision");

        // Public speed/BPM controls enter their real queues, without a stream/device.
        let (parameter_producer, mut parameter_consumer) = RingBuffer::new(4);
        fixture.callback.control.engine.test_parameter_producer =
            Some(Arc::new(Mutex::new(parameter_producer)));
        fixture.callback.control.engine.set_speed(0.73).unwrap();
        drain_parameter_messages(
            &mut parameter_consumer,
            &mut fixture.callback.mixer,
            &mut fixture.callback.transport,
        );
        fixture
            .callback
            .control
            .engine
            .set_bpm_lock_with_binding(false, None)
            .unwrap();
        assert_eq!(fixture.callback.drain(), 1);
        let ready = fixture.window(
            SOURCE,
            WindowRequest {
                // A mode-only ticket owns no launch intent. Capture this exact
                // physical loop with the stopped mode preparation so its own ACK
                // can authorize the subsequent guarded resident start.
                loop_region: Some((0.0, Some(LOOP_END_SECONDS))),
                key_lock: Some(true),
                ..Default::default()
            },
        );
        let start = fixture.launch(&ready);
        let native = fixture.complete_launch(&start);
        fixture.render(13);
        let lane = fixture.lane(SOURCE);
        fixture.render(257);
        assert_eq!(
            fixture.callback.mixer.voices[lane]
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            fixture.callback.mixer.voices[lane]
                .stretch
                .adopted_request_id(),
            Some(start.sequence())
        );
        assert!(
            fixture.callback.mixer.voices[lane]
                .stretch
                .productive_history()
                .is_some()
        );
        fixture
    }

    fn sample(&self, id: usize) -> SampleBuffer {
        self.callback.control.engine.sample_cache.lock().unwrap()[id]
            .clone()
            .unwrap()
    }

    fn lane(&self, id: usize) -> usize {
        self.callback
            .mixer
            .voices
            .iter()
            .position(|voice| voice.is_playing_sample(id))
            .unwrap()
    }

    fn binding(&self, id: usize) -> InputRuntimePadBinding {
        input_runtime_binding::capture(&self.callback.control.engine, id)
            .unwrap()
            .unwrap()
    }

    fn render(&mut self, frames: usize) {
        self.callback.render(frames);
    }

    fn launch(&mut self, ready: &ResidentWindowTicket) -> CapturedPadStart {
        let frame = self.callback.transport.output_frame();
        assert!(
            launch_with_producer(
                &self.callback.control.engine,
                ready,
                false,
                1,
                &self.callback.control.producer,
            )
            .unwrap()
        );
        // Retain the actual resident start already queued by the productive API.
        // Observing its first target message transfers no Work or scheduling custody.
        let start = match self.callback.control.consumer.peek().unwrap() {
            ControlMessage::CapturePadStartTarget(start) => start.clone(),
            _ => panic!("resident launch did not queue its original target first"),
        };
        assert_eq!(start.target_frame(), None);
        // The original target capture and PreparedStart are distinct messages.
        assert_eq!(self.callback.drain(), 2);
        assert!(start.accepted(), "reason={:?}", start.reason());
        assert_eq!(start.original_frame(), Some(frame));
        assert_eq!(self.callback.transport.output_frame(), frame);
        start
    }

    fn complete_launch(&mut self, start: &CapturedPadStart) -> usize {
        let frame = self.callback.transport.output_frame();
        let execution = start.execution_frame().unwrap();
        assert!(execution > frame);
        wait_until(|| {
            let runtime = self.callback.mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            assert_ne!(start.state(), "rejected", "reason={:?}", start.reason());
            runtime.has_ready_action(start.sequence())
        });
        assert_eq!(self.callback.transport.output_frame(), frame);
        assert_eq!(
            self.callback.scheduler.peek_next_target_frame(),
            Some(execution)
        );
        self.render(usize::try_from(execution - frame).unwrap());
        self.render(1);
        assert_eq!(start.effective_frame(), Some(execution));
        assert_eq!(start.execution_frame(), Some(execution));
        assert_eq!(start.state(), "applied", "reason={:?}", start.reason());
        let voice = &self.callback.mixer.voices[self.lane(SOURCE)];
        assert_eq!(
            voice.source_admission.as_ref().unwrap().content_key(),
            Some(start.content_key())
        );
        assert_eq!(voice.stretch.adopted_request_id(), Some(start.sequence()));
        let native = voice.stretch.native_state_address();
        assert_ne!(native, 0);
        native
    }

    fn publish_timing(&mut self, id: usize, decision: &str) -> ConstantTimingTicket {
        let ticket = constant_timing::prepare(
            &self.callback.control.engine,
            id,
            TimingBound {
                halfwidth_seconds: 0.05,
                provenance: "generated pulse/quarter matching bound; no musical calibration".into(),
            },
        )
        .unwrap();
        let counts = Python::attach(|py| {
            let metadata = ticket.metadata(py).unwrap();
            let metadata = metadata.bind(py).cast::<PyDict>().unwrap();
            assert!(metadata.get_item("beat_frames").unwrap().is_some());
            let beats: Vec<f64> = metadata
                .get_item("beat_seconds")
                .unwrap()
                .unwrap()
                .extract()
                .unwrap();
            assert!(
                beats.len() >= 24,
                "actual native QM omitted quarter-pulse evidence"
            );
            beats
                .into_iter()
                .map(|seconds| Some((seconds / QUARTER_SECONDS).round() as i64))
                .collect::<Vec<_>>()
        });
        let hypotheses = json!([{
            "id": "independent-generated-quarter-coordinates",
            "provenance": "QM detections associated with independently generated quarter coordinates",
            "verification": "verified",
            "quarter_note_denominator": 1,
            "quarter_counts": counts,
        }])
        .to_string();
        constant_timing::publish(
            &self.callback.control.engine,
            &self.callback.control.producer,
            &ticket,
            &hypotheses,
            IndependentTimingOrigin {
                seconds: ORIGIN_SECONDS,
                provenance:
                    "manual signed fractional fixture origin; not a measured pulse downbeat".into(),
            },
            TimingAcceptanceDecision {
                policy_version: "hc21-explicit-native-fixture-v1".into(),
                provenance: decision.into(),
            },
        )
        .unwrap();
        assert_eq!(ticket.publication_status().unwrap(), "pending");
        assert_eq!(self.callback.drain(), 1);
        assert_eq!(ticket.publication_status().unwrap(), "accepted");
        assert!(self.binding(id).available());
        ticket
    }

    fn refresh(&mut self, id: usize, start: f64) -> AcceptedTimingRefreshTicket {
        let ticket = accepted_timing_refresh::enqueue(
            &self.callback.control.engine,
            &self.callback.control.producer,
            &self.binding(id),
            start,
            Some(LOOP_END_SECONDS),
            None,
        )
        .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        assert_eq!(self.callback.drain(), 1);
        self.render(2);
        ticket
    }

    fn window(&mut self, id: usize, request: WindowRequest) -> ResidentWindowTicket {
        let ticket = prepare_window_with_producer(
            &self.callback.control.engine,
            id,
            request,
            self.callback.control.producer.clone(),
        )
        .unwrap();
        wait_until(|| {
            if ticket.publication_status() == "pending" && ticket.control_published_for_test() {
                let _published = self.callback.control.producer.lock().unwrap();
                return !self.callback.control.consumer.is_empty();
            }
            matches!(
                ticket.publication_status(),
                "failed" | "cancelled" | "rejected"
            )
        });
        assert_eq!(
            ticket.publication_status(),
            "pending",
            "{:?}",
            ticket.error().unwrap()
        );
        assert_eq!(self.callback.drain(), 1);
        assert_eq!(
            ticket.publication_status(),
            "accepted",
            "{:?}",
            ticket.error().unwrap()
        );
        assert!(ticket.is_current());
        resident_relocation::reconcile(&self.callback.control.engine).unwrap();
        assert!(ticket.is_current());
        ticket
    }
}

/// Accepted timing may change its evidence revision, and a resident ACK may install
/// new storage. Neither grants permission to replace living native/filter chronology.
fn assert_paused_continuity(
    before: &LaneEvidence,
    fixture: &RefreshFixture,
    id: usize,
    sample: &SampleBuffer,
    accepted: Option<AcceptedTimingProjection>,
    invalidations: u64,
) {
    let mixer = &fixture.callback.mixer;
    let voice = &mixer.voices[before.lane];
    assert!(voice.active && voice.paused);
    assert_eq!(voice.sample_id, id);
    assert_eq!(voice.generation, before.generation);
    assert_eq!(
        voice.source_admission.as_ref().unwrap().content_key(),
        Some(before.key)
    );
    assert!(Arc::ptr_eq(&voice.history_lifetime, &before.lifetime));
    assert_eq!(
        voice.history_lifetime.load(Ordering::Acquire),
        before.lifetime_value
    );
    assert!(Arc::ptr_eq(
        voice.stretch.source_epoch_owner(),
        &before.epoch
    ));
    assert_eq!(
        voice.stretch.source_epoch_owner().load(Ordering::Acquire),
        before.epoch_value + invalidations
    );
    assert_eq!(voice.stretch.native_state_address(), before.native);
    assert_eq!(voice.stretch.pending_fifo_frames(), before.fifo);
    assert_eq!(voice.source_playback.position(), before.position);
    assert!(voice.sample.as_ref().unwrap().same_source(&before.sample));
    assert!(voice.sample.as_ref().unwrap().same_window(sample));
    assert!(Arc::ptr_eq(
        &voice.sample.as_ref().unwrap().samples,
        &sample.samples
    ));
    assert_eq!(voice.source_timing.accepted, accepted);
    assert_eq!(
        voice.source_timing.legacy_period_seconds,
        before.legacy_period
    );
    assert_eq!(
        voice.source_timing.legacy_origin_frame,
        before.legacy_origin
    );
    assert_history(voice.stretch.productive_history(), before.history);
    assert_history(mixer.dsp_source_history_for_test(id), Some(before.dsp));
}

fn pair_while_held(fixture: &mut RefreshFixture, target: usize, hold: &HoldActionTicket) {
    let ticket = fixture.callback.prepare_pair(target);
    fixture
        .callback
        .control
        .engine
        ._publish_pad_pair(&ticket)
        .unwrap();
    assert_eq!(fixture.callback.drain(), 1);
    let ack = ticket.native_acknowledgement().unwrap();
    assert_eq!(ack.target_after.lifetime, hold.action.key().lifetime());
    assert_eq!(
        ack.kind,
        if target == EMPTY {
            PairPlacementKind::Move
        } else {
            PairPlacementKind::Swap
        }
    );
    let carried = ack.holds[0].unwrap();
    assert_eq!(carried.action_id, hold.action.action_id());
    assert_eq!(carried.effect, hold.action.effect());
    assert_eq!(carried.state, HoldActionState::PausedOwned);
    assert!(carried.effect_current);
    fixture.callback.finish(&ticket);
}

fn release_preserved(fixture: &mut RefreshFixture, hold: &HoldActionTicket, id: usize) {
    let source = LaneEvidence::capture(&fixture.callback.mixer, id);
    let effect = hold.action.effect().unwrap();
    assert!(
        fixture
            .callback
            .control
            .engine
            .release_waveform_hold(hold)
            .unwrap()
    );
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(hold.action.state(), HoldActionState::Resumed);
    assert_eq!(hold.action.effect(), Some(effect));
    source.assert_retained(&fixture.callback.mixer, id, 1);
    assert!(!fixture.callback.mixer.voices[source.lane].paused);
    let terminal = hold.action.projection();
    assert!(
        fixture
            .callback
            .control
            .engine
            .release_waveform_hold(hold)
            .unwrap()
    );
    assert_eq!(fixture.callback.drain(), 0);
    assert_eq!(hold.action.projection(), terminal);
}

#[test]
fn hc21_actual_qm_timing_and_executor_ack_preserve_hold_through_move_and_swap() {
    for target in [EMPTY, OTHER] {
        let mut fixture = RefreshFixture::new();
        let hold = fixture.callback.hold();
        assert_eq!(fixture.callback.drain(), 1);
        let before = LaneEvidence::capture(&fixture.callback.mixer, SOURCE);
        let other = LaneEvidence::capture(&fixture.callback.mixer, OTHER);
        let effect = hold.action.effect().unwrap();
        let timing = fixture.publish_timing(SOURCE, "distinct explicit same-source HC21 decision");
        fixture.render(2);
        let current = fixture.binding(SOURCE).binding.accepted.unwrap();
        let previous = before.accepted.unwrap();
        assert_ne!(current.revision, previous.revision);
        assert_ne!(current.publication_epoch, previous.publication_epoch);
        assert_eq!(
            current.period_seconds.to_bits(),
            previous.period_seconds.to_bits()
        );
        assert_eq!(
            current.origin_seconds.to_bits(),
            previous.origin_seconds.to_bits()
        );
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        assert_paused_continuity(&before, &fixture, SOURCE, &before.sample, Some(current), 1);
        let refresh = fixture.refresh(SOURCE, 0.0);
        assert_eq!(refresh.publication_status(), "accepted");
        assert!(refresh.is_current());
        assert_eq!(
            fixture.callback.mixer.loop_region_frames(SOURCE),
            (0, Some(84_000))
        );
        assert_eq!(hold.action.state(), HoldActionState::PausedOwned);
        assert_eq!(hold.action.effect(), Some(effect));
        assert_paused_continuity(&before, &fixture, SOURCE, &before.sample, Some(current), 1);
        pair_while_held(&mut fixture, target, &hold);
        assert_paused_continuity(&before, &fixture, target, &before.sample, Some(current), 1);
        let moved_refresh = fixture.refresh(target, 0.0);
        assert_eq!(moved_refresh.publication_status(), "accepted");
        assert!(moved_refresh.is_current());
        assert_paused_continuity(&before, &fixture, target, &before.sample, Some(current), 1);
        other.assert_retained(
            &fixture.callback.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
        release_preserved(&mut fixture, &hold, target);
        other.assert_retained(
            &fixture.callback.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
        assert!(fixture.callback.mixer.voices[other.lane].paused);
    }
}

#[test]
fn hc21_actual_resident_worker_ack_preserves_native_history_hold_and_pair_continuity() {
    for target in [EMPTY, OTHER] {
        let mut fixture = RefreshFixture::new();
        let hold = fixture.callback.hold();
        assert_eq!(fixture.callback.drain(), 1);
        let before = LaneEvidence::capture(&fixture.callback.mixer, SOURCE);
        let other = LaneEvidence::capture(&fixture.callback.mixer, OTHER);
        let effect = hold.action.effect().unwrap();
        let refresh = fixture.window(
            SOURCE,
            WindowRequest {
                storage_range: Some((0.0, 2.0)),
                ..Default::default()
            },
        );
        let read = refresh.read_observation_for_test().unwrap();
        assert_eq!(
            (read.source.start_frame, read.source.end_frame),
            (0, 96_000)
        );
        assert_eq!(read.source.read_bytes, 96_000 * 4);
        assert_eq!(read.source.allocated_bytes, 96_000 * 4);
        assert_eq!(read.stems.read_bytes, [0; 5]);
        let next = fixture.sample(SOURCE);
        assert!(!next.same_window(&before.sample));
        assert!(next.same_source(&before.sample));
        assert!(!Arc::ptr_eq(&next.samples, &before.sample.samples));
        assert_eq!(next.window_revision(), before.sample.window_revision() + 1);
        fixture.render(2);
        assert_paused_continuity(&before, &fixture, SOURCE, &next, before.accepted, 0);
        assert_eq!(hold.action.effect(), Some(effect));
        pair_while_held(&mut fixture, target, &hold);
        assert_paused_continuity(&before, &fixture, target, &next, before.accepted, 0);
        let moved = fixture.window(
            target,
            WindowRequest {
                storage_range: Some((0.0, 2.25)),
                ..Default::default()
            },
        );
        assert_eq!(moved.publication_status(), "accepted");
        assert_eq!(
            moved.read_observation_for_test().unwrap().source.end_frame,
            108_000
        );
        let moved_sample = fixture.sample(target);
        fixture.render(2);
        assert_paused_continuity(&before, &fixture, target, &moved_sample, before.accepted, 0);
        assert_eq!(hold.action.state(), HoldActionState::PausedOwned);
        assert_eq!(hold.action.effect(), Some(effect));
        other.assert_retained(
            &fixture.callback.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
        release_preserved(&mut fixture, &hold, target);
        other.assert_retained(
            &fixture.callback.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
    }
}

#[test]
fn hc21_physical_marker_refresh_rejects_and_real_resident_retrigger_fences_old_hold() {
    let mut fixture = RefreshFixture::new();
    let hold = fixture.callback.hold();
    assert_eq!(fixture.callback.drain(), 1);
    let before = LaneEvidence::capture(&fixture.callback.mixer, SOURCE);
    let other = LaneEvidence::capture(&fixture.callback.mixer, OTHER);
    let rejected = fixture.refresh(SOURCE, 1.0 / f64::from(RATE));
    assert_eq!(rejected.publication_status(), "rejected");
    assert!(!rejected.is_current());
    assert_eq!(
        fixture.callback.mixer.loop_region_frames(SOURCE),
        (0, Some(84_000))
    );
    assert_paused_continuity(
        &before,
        &fixture,
        SOURCE,
        &before.sample,
        before.accepted,
        0,
    );
    assert_eq!(hold.action.state(), HoldActionState::PausedOwned);
    let refresh = fixture.window(
        SOURCE,
        WindowRequest {
            storage_range: Some((0.0, 2.0)),
            ..Default::default()
        },
    );
    assert_eq!(refresh.publication_status(), "accepted");
    // Storage maintenance carries no launch intent. An unchanged physical-loop
    // request prepares its own real launch-capable ticket and callback ACK.
    let ready = fixture.window(
        SOURCE,
        WindowRequest {
            loop_region: Some((0.0, Some(LOOP_END_SECONDS))),
            ..Default::default()
        },
    );
    assert_eq!(ready.publication_status(), "accepted");
    let current = fixture.sample(SOURCE);
    assert_paused_continuity(&before, &fixture, SOURCE, &current, before.accepted, 0);
    let start = fixture.launch(&ready);
    assert_eq!(hold.action.state(), HoldActionState::Superseded);
    assert_paused_continuity(&before, &fixture, SOURCE, &current, before.accepted, 0);
    let native = fixture.complete_launch(&start);
    assert_ne!(native, before.native);
    // New productive feed proves a replacement chronology. Its scalar cohort
    // generation already fenced H at the accepted launch, before this render.
    fixture.render(19);
    let new_cohort = LaneEvidence::capture(&fixture.callback.mixer, SOURCE);
    assert_eq!(new_cohort.key, before.key);
    assert_ne!(new_cohort.generation, before.generation);
    assert_ne!(new_cohort.lifetime_value, before.lifetime_value);
    assert_ne!(
        new_cohort.history.unwrap().binding,
        before.history.unwrap().binding
    );
    assert!(!fixture.callback.mixer.voices[new_cohort.lane].paused);
    let terminal = hold.action.projection();
    for _ in 0..2 {
        assert!(
            fixture
                .callback
                .control
                .engine
                .release_waveform_hold(&hold)
                .unwrap()
        );
        assert_eq!(fixture.callback.drain(), 0);
        assert_eq!(hold.action.projection(), terminal);
        new_cohort.assert_retained(&fixture.callback.mixer, SOURCE, 0);
        other.assert_retained(&fixture.callback.mixer, OTHER, 0);
    }
}

#[test]
fn qualified_bpm_anchor_follows_real_pair_move_without_runtime_revision_self_invalidation() {
    let mut fixture = CallbackFixture::new(32);
    fixture.mixer.set_pad_bpm(SOURCE, Some(120.0));
    let original = input_runtime_binding::capture(&fixture.control.engine, SOURCE)
        .unwrap()
        .unwrap();
    let key = original.binding.placement.unwrap().content_key().unwrap();
    fixture
        .control
        .engine
        .set_bpm_lock_with_binding(true, Some(&original))
        .unwrap();
    let pair = fixture.prepare_pair(EMPTY);
    fixture.control.engine._publish_pad_pair(&pair).unwrap();
    fixture.drain();
    fixture.finish(&pair);
    let (_producer, mut parameters) = RingBuffer::new(1);
    drain_parameter_messages(&mut parameters, &mut fixture.mixer, &mut fixture.transport);
    fixture.render(0);
    let (revision, effect) = fixture
        .control
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap();
    assert_eq!(
        fixture
            .control
            .engine
            .input_runtime_ownership
            .callback_key_slot(key),
        Some(EMPTY)
    );
    assert!(effect.applied && effect.effective);
    assert_eq!(revision, effect.revision);
}

#[test]
fn admitted_bpm_anchor_cannot_inherit_replaced_source_or_aba_content() {
    let mut fixture = CallbackFixture::new(32);
    fixture.mixer.set_pad_bpm(SOURCE, Some(120.0));
    let original = input_runtime_binding::capture(&fixture.control.engine, SOURCE)
        .unwrap()
        .unwrap();
    fixture
        .control
        .engine
        .set_bpm_lock_with_binding(true, Some(&original))
        .unwrap();
    // Two real source publications return the displayed slot/value, not its lifetime/authority.
    fixture.control.publish(SOURCE, 0.75, 99);
    fixture.control.publish(SOURCE, 0.25, 100);
    fixture.drain();
    let (_producer, mut parameters) = RingBuffer::new(1);
    drain_parameter_messages(&mut parameters, &mut fixture.mixer, &mut fixture.transport);
    fixture.render(0);
    let effect = fixture
        .control
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap()
        .1;
    assert!(!effect.applied && !effect.effective);
    assert_eq!(
        effect.reason,
        crate::audio_engine::global_rate_status::BpmReason::ReferenceChanged
    );
}
