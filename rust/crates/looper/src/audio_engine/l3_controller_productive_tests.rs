//! Genuine StemController -> Native candidate Capture/Adopt -> continuous output.
//! V2/V3 permute real files; independent complete V1 PCM supplies the
//! algebraic feed, raw Native/FIFO or dry Varispeed, and uninterrupted EQ oracle.

use super::*;

#[path = "l3_public_loader_b_lifecycle_tests.rs"]
mod l3_public_loader_b_lifecycle_tests;
use crate::audio_engine::buffer_retirement::AudioBufferRetirement;

// These probes follow today's public ownership APIs. Updating a test carrier is
// not evidence that the historical source passed; only a fresh run can establish it.
struct ExpectedStoppedFeedback {
    id: usize,
    owner: usize,
    cell: usize,
    lifetime: u64,
    transport_revision: u64,
    kind: u8,
}

impl ExpectedStoppedFeedback {
    fn source_command(
        mixer: &RtMixer,
        consumer: &rtrb::Consumer<ControlMessage>,
        id: usize,
    ) -> Self {
        let (publication, key, revision) = match consumer.peek().unwrap() {
            ControlMessage::UnloadSample {
                publication: Some(publication),
                id: target,
            } => {
                assert_eq!(*target, id);
                let (key, revision) = publication
                    .retired_feedback
                    .expect("original retirement feedback");
                (publication, key, revision)
            }
            ControlMessage::LoadColdSample {
                publication,
                id: target,
                replace_assignment: true,
                ..
            } => {
                assert_eq!(*target, id);
                let key = publication.key().expect("original incoming source content");
                let revision = publication
                    .ownership
                    .transport_intent_for_cell(key.cell())
                    .load(Ordering::Acquire);
                (publication, key, revision)
            }
            message => panic!("expected exact source STOP publication, got {message:?}"),
        };
        assert!(Arc::ptr_eq(
            &mixer.input_ownership_for_effect(),
            &publication.ownership
        ));
        assert!(key.belongs_to(publication.ownership.owner_identity()));
        Self {
            id,
            owner: publication.ownership.owner_identity().diagnostic(),
            cell: key.cell().index(),
            lifetime: key.lifetime(),
            transport_revision: revision,
            kind: 2,
        }
    }

    fn capture(mixer: &RtMixer, id: usize) -> Self {
        let ownership = mixer.input_ownership_for_effect();
        let key = ownership
            .callback_feedback_key(id)
            .expect("feedback must belong to the original callback content");
        Self {
            id,
            owner: ownership.owner_identity().diagnostic(),
            cell: key.cell().index(),
            lifetime: key.lifetime(),
            transport_revision: ownership
                .transport_intent_for_cell(key.cell())
                .load(Ordering::Acquire),
            kind: 2,
        }
    }

    fn capture_started(
        mixer: &RtMixer,
        id: usize,
        start: &crate::audio_engine::pad_start::CapturedPadStart,
    ) -> Self {
        use crate::audio_engine::input_runtime_binding::LaunchWriteKind;
        assert!(start.effective_frame().is_none());
        assert!(
            !start.exclusive(),
            "these original starts keep the peer active"
        );
        let ownership = mixer.input_ownership_for_effect();
        assert!(start.belongs_to(&ownership));
        let key = start.content_key();
        assert_eq!(ownership.callback_key_slot(key), Some(id));
        let placement = ownership.slot_snapshot(id).unwrap();
        assert_eq!(placement.content_key(), Some(key));
        let claim = ownership
            .prepare_launch_write_fixed(placement, LaunchWriteKind::Admit)
            .unwrap();
        assert!(ownership.launch_write_current(claim));
        let mut expected = Self::capture(mixer, id);
        assert_eq!(expected.cell, key.cell().index());
        assert_eq!(expected.lifetime, key.lifetime());
        // The original PreparedStart's successful Admit claim advances exactly
        // this content's transport intent once BEFORE capturing START feedback.
        // Freeze that next revision before the effect; no emitted message or
        // audible source cursor supplies any expected field.
        expected.transport_revision = expected.transport_revision.checked_add(1).unwrap();
        expected.kind = 1;
        expected
    }

    fn assert_message(&self, message: &AudioMessage) {
        let AudioMessage::ContentFeedback {
            id,
            owner,
            cell,
            lifetime,
            transport_revision,
            kind,
            value,
        } = message
        else {
            panic!("expected original content-owned feedback, got {message:?}");
        };
        assert_eq!(*id, self.id);
        assert_eq!(*owner, self.owner);
        assert_eq!(*cell, self.cell);
        assert_eq!(*lifetime, self.lifetime);
        assert_eq!(*transport_revision, self.transport_revision);
        assert_eq!(*kind, self.kind);
        assert_eq!(value.to_bits(), 0.0_f64.to_bits());
    }
}

/// Preserve the original public attack and optional independent window dependency.
/// This facade supplies the existing device-free producer, never a replacement ACK.
#[pyclass]
struct PublicCapturedStartAudio {
    engine: Arc<AudioEngine>,
}

#[pymethods]
impl PublicCapturedStartAudio {
    fn _resolve_pad_feedback(&self, message: &AudioMessage) -> PyResult<Option<usize>> {
        self.engine._resolve_pad_feedback(message)
    }

    fn capture_input_timestamp_ns(&self) -> u64 {
        self.engine.capture_input_timestamp_ns()
    }

    #[pyo3(signature = (captured, window=None))]
    fn prepare_captured_pad_start(
        &self,
        captured: &crate::audio_engine::pad_start::CapturedPadStart,
        window: Option<&crate::audio_engine::resident_relocation::ResidentWindowTicket>,
    ) -> PyResult<crate::audio_engine::resident_relocation::ResidentWindowTicket> {
        self.engine
            .python_prepare_captured_pad_start(captured, window)
    }
}

fn ordinary_start_from_python(
    locals: &Py<PyDict>,
) -> crate::audio_engine::pad_start::CapturedPadStart {
    Python::attach(|py| {
        locals
            .bind(py)
            .get_item("ordinary_start")
            .unwrap()
            .unwrap()
            .extract::<PyRef<crate::audio_engine::pad_start::CapturedPadStart>>()
            .unwrap()
            .clone()
    })
}

fn assert_original_target_at_front(consumer: &rtrb::Consumer<ControlMessage>, locals: &Py<PyDict>) {
    let original = ordinary_start_from_python(locals);
    let ControlMessage::CapturePadStartTarget(queued) = consumer.peek().unwrap() else {
        panic!("expected original ordinary target before window publication");
    };
    assert!(queued.same_owner(&original));
    assert_eq!(queued.sequence(), original.sequence());
    assert_eq!(queued.received_timestamp(), original.received_timestamp());
}

fn synchronize_callback_frame(callback: &mut Callback, frame: u64) {
    // `frame` is ControllerPlayback's count of callback frames already rendered
    // through the unscheduled PCM oracle. This accounts for those same samples;
    // it never jumps to a requested T/E or consumes unrendered source frames.
    let before = callback.transport.output_frame();
    assert!(before <= frame);
    callback
        .transport
        .advance_by_rendered_frames((frame - before) as usize);
    assert_eq!(callback.transport.output_frame(), frame);
}

fn drain_callback_at_current_frame(
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
) -> usize {
    drain_control_messages(
        consumer,
        &mut callback.scheduler,
        callback.transport.output_frame(),
        &mut callback.quantization,
        &mut callback.transport,
        &mut callback.mixer,
        &mut callback.feedback,
        &mut callback.retirement,
    )
}

/// Poll the original public launch only; no capture or substitute Trigger is allowed.
fn admit_original_public_start(
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
) -> crate::audio_engine::pad_start::CapturedPadStart {
    let start = ordinary_start_from_python(locals);
    let original = start
        .original_frame()
        .expect("original target already drained");
    let timestamp = start.received_timestamp();
    let sequence = start.sequence();
    controller_python(
        locals,
        r#"
transport.on_frame_render()
assert transport.residency.status(0)[1] is None
assert ordinary_launch.start is ordinary_start
assert captured_start_calls == [(ordinary_start, ordinary_timestamp)]
assert all(owner is ordinary_start and dependency is ordinary_launch.dependency
           for owner, dependency in captured_dependency_calls)
"#,
    );
    let original_timestamp: u64 = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("ordinary_timestamp")
            .unwrap()
            .unwrap()
            .extract()
            .unwrap()
    });
    assert_eq!(timestamp, original_timestamp);
    Python::attach(|py| {
        let ticket = locals
            .bind(py)
            .get_item("ordinary_launch")
            .unwrap()
            .unwrap()
            .getattr("ticket")
            .unwrap();
        let ticket = ticket
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        assert!(ticket.is_current());
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(
            ticket
                .capture_pad_start_launch_guard(&start)
                .unwrap()
                .is_some()
        );
    });
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::PreparedStart(_))
    ));
    let before = callback.feedback.events.len();
    let expected = ExpectedStoppedFeedback::capture_started(&callback.mixer, 0, &start);
    assert_eq!(
        drain_callback_at_current_frame(callback, consumer),
        1,
        "one original PreparedStart"
    );
    assert!(consumer.is_empty());
    if start.state() == "applied" {
        assert_eq!(callback.feedback.events.len(), before + 1);
        expected.assert_message(&callback.feedback.events[before]);
    } else {
        assert_eq!(callback.feedback.events.len(), before);
    }
    wait_until(|| {
        assert_ne!(start.state(), "rejected", "{:?}", start.reason());
        assert_eq!(start.sequence(), sequence);
        assert_eq!(start.received_timestamp(), timestamp);
        assert_eq!(start.original_frame(), Some(original));
        if start.state() == "applied" {
            return true;
        }
        let runtime = callback.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        runtime.has_ready_action(sequence)
    });
    assert!(start.execution_frame().unwrap() >= original);
    start
}

fn execute_public_start_at_frame(
    callback: &mut Callback,
    start: &crate::audio_engine::pad_start::CapturedPadStart,
) {
    let execution = start.execution_frame().unwrap();
    assert_eq!(callback.transport.output_frame(), execution);
    let feedback_before = callback.feedback.events.len();
    let already_applied = start.state() == "applied";
    let expected = (!already_applied)
        .then(|| ExpectedStoppedFeedback::capture_started(&callback.mixer, 0, start));
    crate::audio_engine::audio_stream::render_scheduled_audio(
        &mut callback.mixer,
        &mut callback.scheduler,
        &mut [],
        &mut [0.0; NUM_SAMPLES],
        execution,
        2,
        &mut callback.transport,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    assert_eq!(start.state(), "applied", "{:?}", start.reason());
    assert_eq!(start.effective_frame(), Some(execution));
    assert!(callback.scheduler.is_empty());
    // OFF may execute in the control drain at E, in which case the exact START
    // already precedes this empty scheduled boundary. ON executes here.
    if !already_applied {
        assert_eq!(callback.feedback.events.len(), feedback_before + 1);
        expected
            .unwrap()
            .assert_message(&callback.feedback.events[feedback_before]);
    } else {
        assert_eq!(callback.feedback.events.len(), feedback_before);
    }
}

fn prepare_public_start_reference(mixer: &mut RtMixer, execution: u64) {
    use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::pitch_intent::PitchIntent;
    use crate::audio_engine::prepared_native_history::NativeAdapterState;
    let index = mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap();
    let snapshot = mixer.capture_pitch_voice_snapshot(index).unwrap();
    let accepted = snapshot.accepted;
    let ownership = mixer.input_ownership_for_effect();
    let acknowledgements = Arc::new(CurrentTimingAcknowledgements::default());
    if let Some(projection) = accepted {
        if let Some(key) = ownership
            .binding_placement(0)
            .and_then(|placement| placement.content_key())
        {
            assert!(acknowledgements.acknowledge_for_key(
                &ownership,
                key,
                projection.publication_epoch
            ));
        } else {
            acknowledgements.acknowledge(0, projection.publication_epoch);
        }
    }
    mixer.set_current_timing_acknowledgements(acknowledgements.clone());
    let epoch = mixer.voices[index].stretch.preparation_epoch();
    let permit = NativeHistoryContext {
        id: 0,
        ownership: &ownership,
        acknowledgements: &acknowledgements,
        preparation_epoch: mixer.voices[index].stretch.source_epoch_owner(),
    }
    .capture(&snapshot.sample, RATE, accepted)
    .unwrap();
    let request = snapshot
        .into_request(permit, PitchIntent::default(), true, execution, 1, epoch)
        .unwrap();
    let mut candidate = Box::new(NativeAdapterState::new(
        Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
        2,
    ));
    candidate.prepare_source_at_physical_birth(request).unwrap();
    assert!(mixer.adopt_prepared_pitch_voice(
        index,
        &mut candidate,
        execution,
        PitchIntent::default(),
        true,
        epoch,
        1
    ));
}

fn prepared_public_start_raw(playback: &ControllerPlayback) -> ControllerRawNative {
    let mut raw = ControllerRawNative::new(playback.ratio, true);
    let history = ControllerRawNative::constant_history_frames(1.0 / playback.ratio);
    // A fresh physical birth has no predecessor's loop-tail PCM. This independent
    // raw Native keeps its own initial adapter FIFO and warms explicit silence;
    // the complete first source sample is fed by ControllerPlayback::render at E.
    let feed = std::array::from_fn(|_| vec![0.0; history]);
    assert!(raw.render(&feed).iter().all(|sample| *sample == 0.0));
    assert!(raw.used);
    raw
}

fn render_physical_birth_prefix(playback: &mut ControllerPlayback) -> (Vec<f32>, Vec<f32>) {
    let mut actual = Vec::new();
    let mut counterfactual = Vec::new();
    // Compare all 4096 output frames from E, including the original Native/FIFO
    // lead. Unequal callback partitions neither crop silence nor move the onset.
    for frames in [1, 31, 137, 257, 511, 777, 96, 257, 1021, 1008] {
        let (output, full_mix) = playback.render(frames, true);
        actual.extend(output);
        counterfactual.extend(full_mix);
    }
    assert_eq!(actual.len(), 4096 * 2);
    assert!(actual.iter().any(|sample| sample.abs() > 1.0e-6));
    (actual, counterfactual)
}

#[path = "l4_disk_settlement_tests.rs"]
mod l4_disk_settlement_tests;
#[path = "l4_shared_job_tests.rs"]
mod l4_shared_job_tests;
#[path = "public_generate_adoption_tests.rs"]
mod public_generate_adoption_tests;
#[path = "saved_keylock_controller_tests.rs"]
mod saved_keylock_controller_tests;

const CHILD: &str = "FLITZI_L3_CONTROLLER_CANDIDATE_CHILD";
const TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_first_and_replacement_candidates_keep_own_wet_chronology";
const VARISPEED_CHILD: &str = "FLITZI_L3_CONTROLLER_VARISPEED_CHILD";
const VARISPEED_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_keylock_off_first_and_replacement_keep_varispeed_feed_and_eq";
const STOPPED_NEW_SET_CHILD: &str = "FLITZI_L3_CONTROLLER_STOPPED_NEW_SET_CHILD";
const STOPPED_NEW_SET_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_stopped_replacement_play_then_fresh_all_uses_own_wet_receipt";
const STOPPED_NEW_SET_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_STOPPED_NEW_SET_OFF_CHILD";
const STOPPED_NEW_SET_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_stopped_replacement_play_then_fresh_all_uses_own_dry_receipt";
const PAUSED_FIRST_CHILD: &str = "FLITZI_L3_CONTROLLER_PAUSED_FIRST_CHILD";
const PAUSED_FIRST_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_wet_full_mix_paused_before_first_candidate_keeps_consumed_clock";
const PAUSED_FIRST_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_PAUSED_FIRST_OFF_CHILD";
const PAUSED_FIRST_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_varispeed_paused_before_first_candidate_keeps_consumed_clock";
const PAUSED_REPLACEMENT_ON_CHILD: &str = "FLITZI_L3_PUBLIC_PAUSED_REPLACEMENT_ON_CHILD";
const PAUSED_REPLACEMENT_ON_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_paused_v1_replacement_requires_own_resumed_wet_effect";
const PAUSED_REPLACEMENT_OFF_CHILD: &str = "FLITZI_L3_PUBLIC_PAUSED_REPLACEMENT_OFF_CHILD";
const PAUSED_REPLACEMENT_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_paused_v1_replacement_requires_own_resumed_dry_effect";
const REPEATED_SETS_CHILD: &str = "FLITZI_L3_CONTROLLER_REPEATED_SETS_CHILD";
const REPEATED_SETS_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_empty_handover_defers_v3_and_uses_latest_mask_with_own_ack";
const WITHDRAW_PRECLAIM_CHILD: &str = "FLITZI_L3_CONTROLLER_WITHDRAW_PRECLAIM_CHILD";
const WITHDRAW_PRECLAIM_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_full_mix_withdraws_queued_v2_adopt_then_all_uses_fresh_authority";
const ADMISSION_PRESSURE_CHILD: &str = "FLITZI_L3_CONTROLLER_ADMISSION_PRESSURE_CHILD";
const ADMISSION_PRESSURE_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_queue_full_keeps_v1_then_explicit_all_admits_own_v2";
const WORKER_POST_PRESSURE_CHILD: &str = "FLITZI_L3_CONTROLLER_WORKER_POST_PRESSURE_CHILD";
const WORKER_POST_PRESSURE_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_worker_adopt_queue_failure_keeps_v1_then_own_explicit_retry";
const CAPTURE_DEADLINE_CHILD: &str = "FLITZI_L3_CONTROLLER_CAPTURE_DEADLINE_CHILD";
const CAPTURE_DEADLINE_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_python_controller_pending_capture_deadline_keeps_v1_then_fresh_own_retry";
const PUBLIC_BPM_CHILD: &str = "FLITZI_L3_CONTROLLER_PUBLIC_BPM_CHILD";
const PUBLIC_BPM_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_bpm_target_then_own_replacement_preserves_musical_wet_chronology";
const PUBLIC_RATE_ON_CHILD: &str = "FLITZI_L3_PUBLIC_RATE_ADOPTION_ON_CHILD";
const PUBLIC_RATE_ON_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_bpm_glide_adoption_requires_own_wet_effect";
const PUBLIC_RATE_OFF_CHILD: &str = "FLITZI_L3_PUBLIC_RATE_ADOPTION_OFF_CHILD";
const PUBLIC_RATE_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_bpm_glide_adoption_requires_own_dry_effect";
const LIVE_BPM_CHILD: &str = "FLITZI_L3_CONTROLLER_LIVE_BPM_CHILD";
const LIVE_BPM_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_live_bpm_rejects_queued_v2_then_explicit_retry_keeps_wet_chronology";
const LIVE_BPM_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_LIVE_BPM_OFF_CHILD";
const LIVE_BPM_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_live_bpm_keylock_off_rejects_queued_v2_then_own_dry_retry";
const PAUSED_LIVE_BPM_CHILD: &str = "FLITZI_L3_CONTROLLER_PAUSED_LIVE_BPM_CHILD";
const PAUSED_LIVE_BPM_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_live_bpm_paused_v1_rejects_then_resumes_and_retries_with_keylock_on";
const PAUSED_LIVE_BPM_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_PAUSED_LIVE_BPM_OFF_CHILD";
const PAUSED_LIVE_BPM_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_live_bpm_paused_v1_rejects_then_resumes_and_retries_with_keylock_off";
const CLAIMED_MASKS_CHILD: &str = "FLITZI_L3_CONTROLLER_CLAIMED_MASKS_CHILD";
const CLAIMED_MASKS_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_latest_masks_during_nonempty_v2_claim_require_own_wet_ack";
const CLAIMED_MASKS_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_CLAIMED_MASKS_OFF_CHILD";
const CLAIMED_MASKS_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_latest_masks_during_nonempty_v2_claim_require_own_dry_ack";
const PRECLAIM_MASKS_CHILD: &str = "FLITZI_L3_CONTROLLER_PRECLAIM_MASKS_CHILD";
const PRECLAIM_MASKS_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_preclaim_mask_supersession_requires_own_wet_ack";
const PRECLAIM_MASKS_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_PRECLAIM_MASKS_OFF_CHILD";
const PRECLAIM_MASKS_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_preclaim_mask_supersession_requires_own_dry_ack";
const RETRIGGER_CHILD: &str = "FLITZI_L3_CONTROLLER_RETRIGGER_CHILD";
const RETRIGGER_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_play_rejects_queued_v2_then_new_voice_requires_own_wet_ack";
const RETRIGGER_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_RETRIGGER_OFF_CHILD";
const RETRIGGER_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_play_rejects_queued_v2_then_new_voice_requires_own_dry_ack";
const WINDOW_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_WINDOW_OFF_CHILD";
const WINDOW_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_window_rejects_queued_v2_then_requires_fresh_dry_preparation";
const WINDOW_CHILD: &str = "FLITZI_L3_CONTROLLER_WINDOW_CHILD";
const WINDOW_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_window_rejects_queued_v2_then_requires_fresh_wet_preparation";
const SOURCE_CHILD: &str = "FLITZI_L3_CONTROLLER_SOURCE_CHILD";
const SOURCE_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_source_replacement_fences_queued_v2_before_own_ack_with_keylock_on";
const SOURCE_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_SOURCE_OFF_CHILD";
const SOURCE_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_source_replacement_fences_queued_v2_before_own_ack_with_keylock_off";
const SOURCE_PLAY_CHILD: &str = "FLITZI_L3_CONTROLLER_SOURCE_PLAY_CHILD";
const SOURCE_PLAY_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_play_of_replaced_source_requires_own_voice_after_keylock_on";
const SOURCE_PLAY_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_SOURCE_PLAY_OFF_CHILD";
const SOURCE_PLAY_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_play_of_replaced_source_requires_own_voice_after_keylock_off";
const SOURCE_STEMS_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_SOURCE_STEMS_OFF_CHILD";
const SOURCE_STEMS_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_all_of_replaced_source_requires_own_dry_stem_effect";
const SOURCE_KEY_LOCK_CHILD: &str = "FLITZI_L3_CONTROLLER_SOURCE_KEY_LOCK_CHILD";
const SOURCE_KEY_LOCK_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_keylock_on_of_replaced_source_requires_own_wet_confirmation";
const SOURCE_STEMS_ON_CHILD: &str = "FLITZI_L3_CONTROLLER_SOURCE_STEMS_ON_CHILD";
const SOURCE_STEMS_ON_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_all_of_replaced_source_requires_own_wet_stem_effect";
const UNLOAD_ON_CHILD: &str = "FLITZI_L3_CONTROLLER_UNLOAD_ON_CHILD";
const UNLOAD_ON_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_unload_rejects_queued_v2_before_own_stop_with_keylock_on";
const UNLOAD_OFF_CHILD: &str = "FLITZI_L3_CONTROLLER_UNLOAD_OFF_CHILD";
const UNLOAD_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_unload_rejects_queued_v2_before_own_stop_with_keylock_off";
const FROZEN_ON_CHILD: &str = "FLITZI_L3_FROZEN_OLD_ON_CHILD";
const FROZEN_ON_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_native_current_bank_candidate_cannot_borrow_frozen_old_voice_with_keylock_on";
const FROZEN_OFF_CHILD: &str = "FLITZI_L3_FROZEN_OLD_OFF_CHILD";
const FROZEN_OFF_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_native_current_bank_candidate_cannot_borrow_frozen_old_voice_with_keylock_off";
const FINAL_READER_CHILD: &str = "FLITZI_L4_PRODUCTIVE_FINAL_READER_CHILD";
const FINAL_READER_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_productive_replacement_waits_for_last_native_reader_and_asset_worker";
const UNLOAD_FINAL_READER_CHILD: &str = "FLITZI_L4_UNLOAD_FINAL_READER_CHILD";
const UNLOAD_FINAL_READER_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_public_unload_waits_for_queued_candidate_and_final_reader_cleanup";
const SAVED_COPY_CHILD: &str = "FLITZI_L4_SAVED_COPY_CHILD";
const SAVED_COPY_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_saved_copy_retains_v1_after_origin_final_readers_and_unload";
const PCM_PRESERVATION_CHILD: &str = "FLITZI_L4_POST_ADMIT_PCM_CHILD";
const PCM_PRESERVATION_TEST: &str = "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::actual_paired_pcm_preserves_unknown_child_after_native_final_readers";
const UNKNOWN_PCM_CHILD: &str = "late-private.keep";
const UNKNOWN_PCM_BYTES: &[u8] = b"caller-owned post-admission PCM child";

#[derive(Clone, Copy)]
enum CandidateTrace {
    Ordinary,
    FullTrack,
    Explicit {
        intro: bool,
        paused: bool,
        musical: bool,
    },
    NormalSeek {
        equal: bool,
        paused: bool,
        musical: bool,
    },
    CompletePartial {
        seek: Option<bool>,
        paused: bool,
        stopped: bool,
        musical: bool,
    },
    RepeatedSets,
    WithdrawPreclaim,
    AdmissionPressure,
    WorkerPostPressure,
    CaptureDeadline,
    PublicBpm(f64),
    PublicRateAdoption,
    LiveBpm,
    PausedLiveBpm,
    ClaimedMasks,
    PreclaimMasks,
    Retrigger,
    WindowChange,
    SourceReplacement,
    SourcePlay,
    SourceStemsOff,
    SourceKeyLockOn,
    SourceStemsOn,
    Unload,
    FrozenOld,
    PausedReplacement,
    FinalReader,
    UnloadFinalReader,
    SavedCopyFinalReader,
    PostAdmitPcmFinalReader,
    SharedJobFinalReader,
    DiskSettlementFinalReader,
}

// Reader lifetime is orthogonal to the source domain and its own Seek history.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CandidateLifecycle {
    Existing,
    CleanupAfterClaim,
    QueuedUnloadCleanup,
    FrozenUnload,
    FrozenResumeReaders,
    FrozenPausedUnload,
}

#[derive(Clone, Copy)]
enum PublicSeekTarget {
    Intro,
    Tail,
    InLoop { equal: bool },
}

#[derive(Clone, Copy)]
enum FinalReaderOutcome {
    Cleanup,
    SavedCopy,
    PreserveUnknownPcm,
}

/// Public cold admission/event polling are unchanged. The stream-bound scalar
/// default setters forward their real kernels into this fixture's queues.
#[pyclass]
struct PublicLoadAudio {
    engine: Arc<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    parameters: Arc<Mutex<rtrb::Producer<ControlParameterMessage>>>,
}

#[pymethods]
impl PublicLoadAudio {
    fn unload_sample(&self, id: usize) -> PyResult<()> {
        crate::audio_engine::cold_load::unload_for_producer(&self.engine, id, &self.producer)
    }

    #[pyo3(signature=(id, path, run_analysis=true, replace_assignment=true, source_intent="import"))]
    fn load_sample_async(
        &self,
        id: usize,
        path: String,
        run_analysis: bool,
        replace_assignment: bool,
        source_intent: &str,
    ) -> PyResult<u64> {
        assert!(run_analysis && replace_assignment);
        assert_eq!(source_intent, "import");
        self.engine.load_sample_async(
            id,
            path,
            Some(run_analysis),
            false,
            replace_assignment,
            None,
            None,
            false,
            source_intent,
        )
    }

    fn load_incoming_sample_async(&self, id: usize, path: String, key_lock: bool) -> PyResult<u64> {
        self.engine.load_incoming_sample_async(id, path, key_lock)
    }

    #[allow(clippy::too_many_arguments)] // Mirror the public request-owned incoming API.
    #[pyo3(signature=(id, request_id, revision, key_lock, loop_start_s=None, loop_end_s=None, loop_auto=true, loop_bars=8.0, timing_override=false, period_seconds=None, grid_origin_s=None))]
    fn update_incoming_load_intent(
        &self,
        id: usize,
        request_id: u64,
        revision: u64,
        key_lock: bool,
        loop_start_s: Option<f64>,
        loop_end_s: Option<f64>,
        loop_auto: bool,
        loop_bars: f64,
        timing_override: bool,
        period_seconds: Option<f64>,
        grid_origin_s: Option<f64>,
    ) -> PyResult<bool> {
        self.engine.update_incoming_load_intent(
            id,
            request_id,
            revision,
            key_lock,
            loop_start_s,
            loop_end_s,
            loop_auto,
            loop_bars,
            timing_override,
            period_seconds,
            grid_origin_s,
        )
    }

    fn poll_loader_events(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.engine.poll_loader_events(py)
    }

    fn _resolve_loader_event(
        &self,
        delivery: &crate::audio_engine::loader_events::LoaderEventDelivery,
    ) -> PyResult<Option<(usize, bool)>> {
        self.engine._resolve_loader_event(delivery)
    }

    fn pad_timing_intent(&self, id: usize) -> PyResult<&'static str> {
        self.engine.pad_timing_intent(id)
    }

    fn waveform_source_identity(&self, id: usize) -> PyResult<Option<(u64, String, usize, u32)>> {
        self.engine.waveform_source_identity(id)
    }

    #[pyo3(signature=(id, bpm=None, *, expected_source_generation=None))]
    fn set_pad_bpm(
        &self,
        id: usize,
        bpm: Option<f64>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        crate::audio_engine::constant_timing::publish_legacy_bpm_with_source(
            &self.engine,
            &self.parameters,
            id,
            bpm,
            expected_source_generation,
        )
    }

    fn set_pad_gain(&self, id: usize, gain_db: f32) -> PyResult<()> {
        assert_eq!(gain_db, 0.0);
        crate::audio_engine::push_parameter_message(
            &mut self.parameters.lock().unwrap(),
            ControlParameterMessage::SetPadGain { id, gain_db },
            "SetPadGain",
        )
    }

    fn set_pad_eq(&self, id: usize, low_db: f32, mid_db: f32, high_db: f32) -> PyResult<()> {
        assert_eq!([low_db, mid_db, high_db], [0.0; 3]);
        crate::audio_engine::push_parameter_message(
            &mut self.parameters.lock().unwrap(),
            ControlParameterMessage::SetPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            },
            "SetPadEq",
        )
    }

    #[pyo3(signature=(id, start_s, end_s=None, *, expected_source_generation=None))]
    fn set_pad_loop_region(
        &self,
        id: usize,
        start_s: f64,
        end_s: Option<f64>,
        expected_source_generation: Option<u64>,
    ) -> PyResult<()> {
        assert_eq!(start_s, 0.0);
        assert_eq!(end_s, None);
        if expected_source_generation.is_some() {
            return self.engine.publish_pad_loop_region_with_source(
                id,
                start_s,
                end_s,
                expected_source_generation,
                Some((RATE, &self.producer)),
            );
        }
        reconcile(&self.engine)?;
        let _requests = self.engine.pad_request_ids.lock().unwrap();
        let mut producer = self.producer.lock().unwrap();
        assert!(!producer.is_full());
        if let Some(source) = self.engine.sample_cache.lock().unwrap()[id].as_ref() {
            assert_eq!(source.resident_start(), 0);
            assert_eq!(source.resident_end(), source.frame_count());
        }
        let authority = self.engine.input_runtime_ownership.next_authority(id)?;
        self.engine.note_resident_loop_intent(id, start_s, end_s)?;
        self.engine.input_runtime_ownership.revoke(id, authority);
        producer
            .push(ControlMessage::SetPadLoopRegion {
                id,
                start_s,
                end_s,
                expected_source_generation: None,
            })
            .map_err(|_| {
                pyo3::exceptions::PyRuntimeError::new_err("source default loop queue full")
            })
    }

    #[pyo3(signature=(id, enabled, *, expected_source_generation=None))]
    fn set_pad_key_lock(
        &self,
        id: usize,
        enabled: bool,
        expected_source_generation: Option<u64>,
    ) -> PyResult<u64> {
        assert!(!enabled);
        reconcile(&self.engine)?;
        if expected_source_generation.is_none() {
            self.engine.admit_resident_key_lock(id, enabled)?;
        }
        crate::audio_engine::key_lock_feedback::enqueue_with_source(
            &self.engine,
            id,
            enabled,
            expected_source_generation,
        )
    }
}

fn public_rate_before_candidate(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    accepted: crate::audio_engine::constant_timing::AcceptedTimingProjection,
    key_lock: bool,
) -> ControllerRateLedger {
    assert!(consumer.is_empty() && parameters.is_empty());
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    assert_eq!(history.is_some(), key_lock);
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    let revision = engine.global_timing_revision.load(Ordering::Acquire);
    let old_rate = playback.ratio;
    let start = playback.source_frame;
    controller_python(
        locals,
        r#"
if selected_v1:
    assert first.is_current() and first.effective_mode_status() == 'effective'
else:
    assert controller.stem_residency_status(0).effective is None
previous_master = session.master_period_seconds
assert transport.global_params.set_effective_display_bpm(210.0)
assert project.speed == accepted_period * (210.0 / 60)
assert session.master_period_seconds == previous_master
assert abs(transport.global_params.effective_display_bpm() - 210.0) < 1e-10
assert transport.bpm.current_timing(0) == timing_before_bpm
assert previous_control.ticket.is_current()
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    let speed = accepted.period_seconds * (210.0 / 60.0);
    let master = accepted.period_seconds / speed;
    let ratio = accepted.period_seconds / master;
    let rate_revision = original_public_rate_revision(locals);
    assert!(rate_revision > revision);
    assert_eq!(
        engine.global_timing_revision.load(Ordering::Acquire),
        rate_revision
    );
    assert!(
        matches!(parameters.peek(), Ok(ControlParameterMessage::SetRateIntent {
        revision, speed: Some(actual), period_seconds: Some(period_seconds), ..
    }) if *revision == rate_revision && actual.to_bits() == speed.to_bits()
        && period_seconds.to_bits() == master.to_bits())
    );
    assert_eq!(
        playback.drain(consumer),
        0,
        "qualified rate reuses the existing bootstrap"
    );
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(consumer.is_empty() && parameters.is_empty());
    let applied = engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(
        (applied.request_revision, applied.effect.revision),
        (rate_revision, rate_revision)
    );
    assert_eq!(
        applied.effect.state,
        crate::audio_engine::global_rate_status::RateState::Applied
    );
    assert_eq!(applied.effect.effective.speed, speed);
    assert_eq!(applied.effect.effective.period, Some(master));
    assert_eq!(applied.effect.execution_frame, Some(playback.frame));
    controller_python(
        locals,
        "transport.global_params._poll_global_rate_status()\ntransport.global_params.poll_bpm_lock_application()\nassert session.master_period_seconds == accepted_period / project.speed",
    );
    assert_eq!(
        playback.callback.transport.master_period_seconds(),
        Some(master)
    );
    let after = voice(&playback.callback.mixer);
    assert!(
        after.source_playback.matches_exact(&clock),
        "public intent consumes no source"
    );
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let after = after.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(after.stretch.productive_history().is_none());
    }
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    let mut other = ControllerRateLedger::new(old_rate, (LOOP_END - LOOP_START) as f64);
    other.change(start, speed);
    playback.rate_ledger.as_mut().unwrap().change(start, ratio);
    playback.ratio = ratio;
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        reference.set_speed(speed);
        reference.set_master_period(master);
    }
    if key_lock {
        playback.render(13, false);
    } else {
        render_varispeed(playback, 13);
    }
    assert_eq!(playback.source_frame - start, 13);
    assert_ne!(
        voice(&playback.callback.mixer)
            .source_playback
            .tempo_ratio()
            .to_bits(),
        ratio.to_bits()
    );
    other
}

fn live_bpm_then_explicit_retry(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    accepted: crate::audio_engine::constant_timing::AcceptedTimingProjection,
    key_lock: bool,
) -> ControllerRateLedger {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    use crate::audio_engine::pitch_intent::PitchIntent;
    assert_eq!(playback.drain(consumer), 1, "actual V2 Capture at old BPM");
    wait_until(|| consumer.peek().is_ok());
    let queued = match consumer.peek().unwrap() {
        ControlMessage::CommittedStemWork(work) => {
            assert!(
                matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
                if transaction.id == 0 && transaction.mask == 0b1010 && transaction.candidate.is_some())
            );
            work.as_ref() as *const CommittedStemRtWork as usize
        }
        _ => panic!("expected actual already prepared V2 Adopt"),
    };
    let source = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let set = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let epoch = before.stretch.source_epoch_owner().load(Ordering::Acquire);
    let history = before.stretch.productive_history();
    assert_eq!(history.is_some(), key_lock);
    let paused = before.paused;
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    assert_eq!(other_voice(&playback.callback.mixer).paused, paused);
    let revision = engine.global_timing_revision.load(Ordering::Acquire);
    assert!(playback.frame > 0 && playback.frame == playback.callback.transport.output_frame());
    assert!(playback.callback.transport.bootstrap_reference().is_none());
    let beat = playback.callback.transport.beat_position().unwrap();
    let old_master = playback.callback.transport.master_period_seconds().unwrap();
    controller_python(
        locals,
        r#"
failed_bpm = candidate
bpm_selection = live.disk.selection
bpm_registration = bpm_selection.ticket
bpm_error = 'Committed stem residency or mode ownership changed'
assert failed_bpm.is_current() and not failed_bpm.claimed()
assert failed_bpm.effective_mode_status() == 'waiting'
previous_master = session.master_period_seconds
assert transport.global_params.set_effective_display_bpm(162.0)
assert project.speed == accepted_period * (162.0 / 60)
assert session.master_period_seconds == previous_master
assert abs(transport.global_params.effective_display_bpm() - 162.0) < 1e-10
assert transport.bpm.current_timing(0) == timing_before_bpm
assert live.attempt is bpm_selection and first.is_current()
"#,
    );
    let speed = accepted.period_seconds * (162.0 / 60.0);
    let master = accepted.period_seconds / speed;
    let ratio = accepted.period_seconds / master;
    let rate_revision = original_public_rate_revision(locals);
    assert!(rate_revision > revision);
    assert_eq!(
        engine.global_timing_revision.load(Ordering::Acquire),
        rate_revision
    );
    assert!(
        matches!(parameters.peek(), Ok(ControlParameterMessage::SetRateIntent {
        revision, speed: Some(actual), period_seconds: Some(period_seconds), ..
    }) if *revision == rate_revision && actual.to_bits() == speed.to_bits()
        && period_seconds.to_bits() == master.to_bits())
    );
    let unchanged = voice(&playback.callback.mixer);
    assert!(unchanged.source_playback.matches_exact(&clock));
    assert_eq!(unchanged.stretch.native_state_address(), native);
    assert_eq!(unchanged.stretch.pending_fifo_frames(), fifo);
    assert_eq!(
        unchanged
            .stretch
            .source_epoch_owner()
            .load(Ordering::Acquire),
        epoch
    );
    assert_eq!(
        playback.callback.mixer.output_bpm_for_sample_id(0),
        Some(60.0 / old_master)
    );
    // The real regular callback checks controls first. Its established three-slot
    // capacity gate retains the actual original front Adopt before any rate effect.
    playback.callback.retirement.slots = 2;
    assert_eq!(playback.drain(consumer), 0);
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if work.as_ref() as *const CommittedStemRtWork as usize == queued)
    );
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    assert_eq!(playback.callback.transport.beat_position().unwrap(), beat);
    assert_eq!(
        playback.callback.transport.master_period_seconds(),
        Some(old_master)
    );
    let preparing = engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(preparing.request_revision, rate_revision);
    assert_eq!(preparing.effect.revision, rate_revision);
    assert_eq!(
        preparing.effect.state,
        crate::audio_engine::global_rate_status::RateState::Preparing
    );
    assert_eq!(preparing.effect.effective.period, Some(old_master));
    assert_eq!(preparing.effect.execution_frame, None);
    let changed = voice(&playback.callback.mixer);
    assert!(
        changed.source_playback.matches_exact(&clock),
        "parameter admission consumes no source"
    );
    assert_eq!(changed.generation, generation);
    assert_eq!(changed.stretch.native_state_address(), native);
    assert_eq!(changed.stretch.pending_fifo_frames(), fifo);
    assert_eq!(
        changed.stretch.source_epoch_owner().load(Ordering::Acquire),
        epoch
    );
    if let Some(history) = history {
        let changed_history = changed.stretch.productive_history().unwrap();
        assert_eq!(changed_history.binding, history.binding);
        assert_eq!(changed_history.next_position, history.next_position);
        assert_eq!(changed_history.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(changed.stretch.productive_history().is_none());
    }
    let waiting_source_frame = playback.source_frame;
    if paused {
        // Silent callback time advances; the old applied source clock and both
        // independent PCM/Native/EQ oracles consume no frames before Resume.
        for frames in [1, 127, 384, 512, 777] {
            playback.render_paused(frames);
            assert_eq!(playback.source_frame, waiting_source_frame);
            assert!(
                voice(&playback.callback.mixer)
                    .source_playback
                    .matches_exact(&clock)
            );
            let other = other_voice(&playback.callback.mixer);
            assert!(other.paused && other.source_playback.matches_exact(&other_clock));
        }
    } else if key_lock {
        playback.render(1, true);
    } else {
        render_varispeed(playback, 1);
    }
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        playback.drain(consumer),
        0,
        "same real pressure still retains Adopt"
    );
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    assert_eq!(
        before.stretch.source_epoch_owner().load(Ordering::Acquire),
        epoch
    );
    assert_eq!(
        playback.callback.transport.master_period_seconds(),
        Some(old_master)
    );
    assert_eq!(history.is_some(), key_lock);
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(
        playback.drain(consumer),
        1,
        "the original stale Adopt rejects before the waiting qualified rate"
    );
    assert!(consumer.is_empty());
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let next_history = after.stretch.productive_history().unwrap();
        assert_eq!(next_history.binding, history.binding);
        assert_eq!(next_history.next_position, history.next_position);
        assert_eq!(next_history.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(after.stretch.productive_history().is_none());
    }
    let next_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(next_dsp.binding, dsp.binding);
    assert_eq!(next_dsp.next_position, dsp.next_position);
    assert_eq!(next_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(Arc::ptr_eq(
        &source.samples,
        &playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    let current_set = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &current_set.complete_set_identity
    ));
    for (old, current) in set.stems.iter().zip(&current_set.stems) {
        assert!(Arc::ptr_eq(&old.samples, &current.samples));
    }
    controller_python(
        locals,
        r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.attempt is bpm_selection
    assert live.control is previous_control and previous_control.receipt is first
    assert live.disk.selection is bpm_selection and bpm_selection.ticket is bpm_registration
    assert bpm_registration.is_current() and bpm_registration.publication_status() == 'accepted'
    assert first.is_current() and first.effective_mode_status() == 'effective'
    assert not failed_bpm.is_current() and not failed_bpm.claimed()
    assert failed_bpm.residency_status() == 'rejected' and failed_bpm.effective_mode_status() == 'rejected'
    assert failed_bpm.error() == bpm_error and failed_bpm.effective_failure_reason() is None
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity == disk_v1.stem_set_identity
    assert status.attempt_stem_set_identity == disk_v2.stem_set_identity
    assert status.residency == 'rejected' and status.effective is None
    assert session.stem_generation_errors == {0: bpm_error}
"#,
    );
    assert!(consumer.is_empty(), "polling cannot retry V2");
    // The original stale operation has retired; both real drains may now admit
    // the qualified rate effect, without changing either original PCM owner.
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    let effect_frame = playback.callback.transport.output_frame();
    let effect_beat = playback.callback.transport.beat_position().unwrap();
    playback.callback.mixer.service_global_rate(
        effect_frame,
        &mut playback.callback.transport,
        &mut playback.callback.retirement,
    );
    let applied = engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap();
    assert_eq!(
        (applied.request_revision, applied.effect.revision),
        (rate_revision, rate_revision)
    );
    assert_eq!(
        applied.effect.state,
        crate::audio_engine::global_rate_status::RateState::Applied
    );
    assert_eq!(applied.effect.effective.speed, speed);
    assert_eq!(applied.effect.effective.period, Some(master));
    assert_eq!(applied.effect.execution_frame, Some(effect_frame));
    assert_eq!(playback.callback.transport.output_frame(), effect_frame);
    assert_eq!(
        playback.callback.transport.beat_position(),
        Some(effect_beat)
    );
    assert!(playback.callback.transport.bootstrap_reference().is_none());
    let applied_voice = voice(&playback.callback.mixer);
    assert!(applied_voice.source_playback.matches_exact(&clock));
    assert_eq!(applied_voice.generation, generation);
    assert_eq!(applied_voice.stretch.native_state_address(), native);
    assert_eq!(applied_voice.stretch.pending_fifo_frames(), fifo);
    assert!(
        applied_voice
            .stretch
            .source_epoch_owner()
            .load(Ordering::Acquire)
            > epoch
    );
    if let Some(history) = history {
        let actual = applied_voice.stretch.productive_history().unwrap();
        assert_eq!(actual.binding, history.binding);
        assert_eq!(actual.next_position, history.next_position);
        assert_eq!(actual.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(applied_voice.stretch.productive_history().is_none());
    }
    let effect_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(effect_dsp.binding, dsp.binding);
    assert_eq!(effect_dsp.next_position, dsp.next_position);
    assert_eq!(effect_dsp.fed_output_frames, dsp.fed_output_frames);
    controller_python(
        locals,
        "transport.global_params._poll_global_rate_status()\ntransport.global_params.poll_bpm_lock_application()\nassert session.master_period_seconds == accepted_period / project.speed",
    );
    let start = playback.source_frame;
    let rate_history = applied_voice.stretch.productive_history();
    let mut other = ControllerRateLedger::new(
        accepted.period_seconds * (150.0 / 60.0),
        (LOOP_END - LOOP_START) as f64,
    );
    other.change(start, speed);
    playback.rate_ledger.as_mut().unwrap().change(start, ratio);
    playback.ratio = ratio;
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        reference.set_speed(speed);
        reference.set_master_period(master);
    }
    if paused {
        let frozen_frame = playback.source_frame;
        controller_python(
            locals,
            r#"
assert session.paused_sample_ids == {0, 215}
assert live.blocked and first.effective_mode_status() == 'effective'
transport.playback.resume_pad(0)
transport.playback.resume_pad(215)
assert session.paused_sample_ids == {0, 215}
assert not any(transport.playback._pause_intent.requested_paused(pad) for pad in (0, 215))
assert session.stem_generation_errors == {0: bpm_error}
"#,
        );
        remember_public_pause_admissions(locals, false);
        assert_eq!(
            playback.drain(consumer),
            2,
            "actual public Resume after rejection"
        );
        assert_public_pause_effect(locals);
        controller_python(
            locals,
            "transport.playback.poll_pause_observations()\nassert not session.paused_sample_ids",
        );
        for reference in [&mut playback.reference, &mut playback.full_mix] {
            reference.resume_sample_at_output_frame(0, playback.frame);
        }
        playback.resumed_at = Some(playback.frame);
        let resumed = voice(&playback.callback.mixer);
        assert!(!resumed.paused && resumed.source_playback.matches_exact(&clock));
        assert_eq!(resumed.generation, generation);
        assert_eq!(resumed.stretch.native_state_address(), native);
        assert_eq!(resumed.stretch.pending_fifo_frames(), fifo);
        let other = other_voice(&playback.callback.mixer);
        assert!(!other.paused && other.source_playback.matches_exact(&other_clock));
        assert_eq!(playback.source_frame, frozen_frame);
        assert!(playback.frame > playback.source_frame);
    }
    let mut nontrivial = false;
    for frames in [31, 257, 1, 777, 137] {
        let output = if key_lock {
            playback.render(frames, true)
        } else {
            render_varispeed(playback, frames)
        }
        .0;
        nontrivial |= output.iter().any(|sample| sample.abs() > 0.001);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    assert!(nontrivial && playback.source_frame - start > 1_024);
    assert_eq!(
        voice(&playback.callback.mixer)
            .source_playback
            .tempo_ratio()
            .to_bits(),
        ratio.to_bits()
    );
    // The live glide is finished before a fresh V2 attempt. Productive V1 keeps
    // its own uninterrupted Native/FIFO, rather than waiting for a replacement.
    if key_lock {
        let retained = voice(&playback.callback.mixer);
        assert_eq!(retained.stretch.native_state_address(), native);
        assert!(
            retained
                .stretch
                .owns_pitch_contract(PitchIntent::default(), true)
        );
        let original_history = rate_history.expect("V1 already owns source-specific history");
        let retained_history = retained.stretch.productive_history().unwrap();
        assert_eq!(retained_history.binding, original_history.binding);
        assert_eq!(
            retained_history.fed_output_frames,
            original_history.fed_output_frames + playback.source_frame - start
        );
        assert_eq!(
            retained_history.next_position,
            retained.source_playback.position()
        );
    } else {
        for frames in [512, 257, 31] {
            render_varispeed(playback, frames);
        }
    }
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert live.blocked and live.attempt is bpm_selection
assert first.is_current() and first.effective_mode_status() == 'effective'
assert controller.set_stem_mix_mode(0, 'all_stems')
assert not live.blocked and live.attempt is bpm_selection
candidate = bpm_selection.receipt
assert candidate is not failed_bpm and candidate is not first
assert candidate.is_current() and not candidate.claimed()
assert bpm_selection.ticket is bpm_registration and bpm_registration.is_current()
assert candidate.residency_status() in {'captured', 'pending'}
assert candidate.effective_mode_status() == 'waiting'
assert session.stem_generation_errors == {0: bpm_error}
"#,
    );
    other
}

fn public_bpm_before_voice_birth(
    locals: &Py<PyDict>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    projection: crate::audio_engine::constant_timing::AcceptedTimingProjection,
    target_bpm: f64,
) -> f64 {
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("accepted_period", projection.period_seconds)
            .unwrap();
        locals.bind(py).set_item("target_bpm", target_bpm).unwrap();
    });
    controller_python(
        locals,
        r#"
from flitzis_looper.controller.transport import TransportController
class BpmTransportAudio:
    def __getattr__(self, name):
        if name in {'current_constant_timing', 'current_input_runtime_pad_binding', 'global_rate_status', 'set_bpm_lock', 'bpm_lock_status',
                    'set_master_period', 'set_speed_and_master_period'}:
            return getattr(bpm_audio, name)
        return getattr(playback_audio, name)
project.selected_pad = 0
project.speed = 1.0
project.bpm_lock = False
project.multi_loop = True
project.manual_bpm[0] = None
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = (declared_loop_start if explicit else 17) / fixture['rate']
project.pad_loop_end_s[0] = (declared_loop_end if explicit else 1517) / fixture['rate']
transport = TransportController(project, session, BpmTransportAudio(), persistence.mark_dirty)
timing_before_bpm = transport.bpm.current_timing(0)
assert timing_before_bpm is not None and timing_before_bpm.accepted_revision is not None
assert timing_before_bpm.period_seconds == accepted_period
transport.global_params.set_bpm_lock(enabled=True)
assert project.bpm_lock and session.bpm_lock_anchor_pad_id == 0
assert session.master_period_seconds is None
"#,
    );
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::SetBpmLockTarget(target)) if target.enabled && target.anchor.is_some())
    );
    let master_revision = callback.mixer.global_rate_timing_revision();
    assert!(callback.drain(consumer) >= 1);
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut callback.mixer,
        &mut callback.transport,
    );
    let frame = callback.transport.output_frame();
    callback.mixer.service_global_rate(
        frame,
        &mut callback.transport,
        &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
    );
    assert!(consumer.is_empty() && parameters.is_empty());
    controller_python(
        locals,
        "transport.global_params.poll_bpm_lock_application()\nassert session.master_period_seconds == accepted_period",
    );
    controller_python(
        locals,
        r#"
assert transport.global_params.set_effective_display_bpm(target_bpm)
assert project.speed == accepted_period * (target_bpm / 60)
assert session.master_period_seconds == accepted_period
assert abs(transport.global_params.effective_display_bpm() - target_bpm) < 1e-10
assert transport.bpm.current_timing(0) == timing_before_bpm
"#,
    );
    let admitted_speed = projection.period_seconds * (target_bpm / 60.0);
    let admitted_master = projection.period_seconds / admitted_speed;
    let rate_revision = original_public_rate_revision(locals);
    assert!(rate_revision > master_revision);
    assert_eq!(callback.mixer.global_rate_timing_revision(), rate_revision);
    // Python display/model admission is not an audio ACK: the actual callback
    // still uses its previously applied accepted source period at this point.
    assert_eq!(
        callback.mixer.output_bpm_for_sample_id(0).unwrap(),
        60.0 / projection.period_seconds
    );
    assert!(
        matches!(parameters.peek(), Ok(ControlParameterMessage::SetRateIntent {
        revision, speed: Some(speed), period_seconds: Some(period_seconds), ..
    }) if *revision == rate_revision && speed.to_bits() == admitted_speed.to_bits()
        && period_seconds.to_bits() == admitted_master.to_bits())
    );
    assert!(
        consumer.is_empty(),
        "later qualified master effects do not enqueue another bootstrap"
    );
    callback.drain(consumer);
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut callback.mixer,
        &mut callback.transport,
    );
    assert!(consumer.is_empty() && parameters.is_empty());
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    let actual = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("bpm_audio")
            .unwrap()
            .unwrap()
            .extract::<PyRef<PublicBpmAudio>>()
            .unwrap()
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap()
    });
    assert_eq!(
        (actual.request_revision, actual.effect.revision),
        (rate_revision, rate_revision)
    );
    assert_eq!(
        actual.effect.state,
        crate::audio_engine::global_rate_status::RateState::Applied
    );
    assert_eq!(actual.effect.effective.speed, admitted_speed);
    assert_eq!(actual.effect.effective.period, Some(admitted_master));
    assert_eq!(
        actual.effect.execution_frame,
        Some(callback.transport.output_frame())
    );
    controller_python(
        locals,
        "transport.global_params._poll_global_rate_status()\ntransport.global_params.poll_bpm_lock_application()\nassert session.master_period_seconds == accepted_period / project.speed",
    );
    assert_eq!(
        callback.mixer.output_bpm_for_sample_id(0).unwrap(),
        60.0 / admitted_master
    );
    projection.period_seconds / admitted_master
}

fn render_public_bpm_tail_sample(playback: &mut ControllerPlayback) {
    let last = (LOOP_END - LOOP_START - 1) as f64;
    let target = (playback.source_frame..playback.source_frame + 4_096)
        .find(|frame| {
            let phase = (*frame as f64 * playback.ratio).rem_euclid(playback.loop_period);
            phase > last + 0.1 && phase < playback.loop_period - 0.1
        })
        .expect("fractional displayed BPM must actually sample the virtual tail interior");
    while playback.source_frame < target {
        playback.render((target - playback.source_frame).min(777) as usize, true);
    }
    let phase = (target as f64 * playback.ratio).rem_euclid(playback.loop_period);
    let alpha = (phase - last) / (playback.loop_period - last);
    assert!(alpha > 0.0 && alpha < 1.0);
    assert_ne!(alpha as f32, phase.fract() as f32);
    playback.render(1, true);
    let taps = tap_observation_for_test();
    assert_eq!(taps.missing_reads, 0);
    assert_eq!(taps.min_frame, Some(LOOP_START));
    assert_eq!(taps.max_frame, Some(LOOP_END - 1));
}

/// The Python TransportController uses real source queries from ReadyPlaybackAudio
/// and these actual Native queue-admission primitives. No stream/device or fabricated
/// ACK is supplied; the stream-bound Engine setter wrappers are outside this proof.
#[pyclass]
struct PublicBpmAudio {
    engine: Arc<AudioEngine>,
    producer: Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    parameter_producer: Arc<Mutex<rtrb::Producer<ControlParameterMessage>>>,
    original_rate_revision: AtomicU64,
}

#[pymethods]
impl PublicBpmAudio {
    fn global_rate_status(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        self.engine.global_rate_status(py)
    }
    fn current_input_runtime_pad_binding(
        &self,
        id: usize,
    ) -> PyResult<Option<crate::audio_engine::input_runtime_binding::InputRuntimePadBinding>> {
        self.engine.current_input_runtime_pad_binding(id)
    }

    fn current_constant_timing(&self, py: Python<'_>, id: usize) -> PyResult<Option<Py<PyAny>>> {
        if id >= NUM_SAMPLES {
            return Err(pyo3::exceptions::PyValueError::new_err("id out of range"));
        }
        // The public Engine query returns None without a stream. This fixture
        // uses its exact current-source/intent/ACK resolver without a device.
        crate::audio_engine::constant_timing::current_metadata(&self.engine, py, id)
    }

    #[pyo3(signature = (enabled, *, anchor_binding=None))]
    fn set_bpm_lock(
        &self,
        enabled: bool,
        anchor_binding: Option<&crate::audio_engine::input_runtime_binding::InputRuntimePadBinding>,
    ) -> PyResult<()> {
        let _requests = self.engine.pad_request_ids.lock().unwrap();
        crate::audio_engine::push_bpm_lock_message(
            &self.engine,
            &mut self.producer.lock().unwrap(),
            enabled,
            anchor_binding,
        )
    }

    fn bpm_lock_status(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        crate::audio_engine::bpm_lock_status_with_producer(&self.engine, py, &self.producer)
    }

    #[pyo3(signature = (period_seconds, *, anchor_binding=None))]
    fn set_master_period(
        &self,
        period_seconds: f64,
        anchor_binding: Option<&crate::audio_engine::input_runtime_binding::InputRuntimePadBinding>,
    ) -> PyResult<()> {
        if !period_seconds.is_finite()
            || period_seconds <= 0.0
            || !(period_seconds * f64::from(RATE) * 4.0).is_finite()
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "master period out of range",
            ));
        }
        let _requests = self.engine.pad_request_ids.lock().unwrap();
        crate::audio_engine::push_global_rate_message_qualified(
            &self.engine.global_timing_revision,
            &self.engine.input_runtime_ownership.global_timing_publishing,
            &self.engine.input_runtime_ownership.global_rate_status,
            &mut self.parameter_producer.lock().unwrap(),
            None,
            Some(period_seconds),
            anchor_binding
                .map(|binding| crate::audio_engine::qualify_timing_anchor(&self.engine, binding))
                .transpose()?,
            "SetMasterPeriod",
        )?;
        // Capture the stamped request while its publisher still owns the lock,
        // before another independent global timing admission can supersede it.
        self.original_rate_revision.store(
            self.engine.global_timing_revision.load(Ordering::Acquire),
            Ordering::Release,
        );
        Ok(())
    }

    #[pyo3(signature = (speed, period_seconds, *, anchor_binding=None))]
    fn set_speed_and_master_period(
        &self,
        speed: f64,
        period_seconds: f64,
        anchor_binding: Option<&crate::audio_engine::input_runtime_binding::InputRuntimePadBinding>,
    ) -> PyResult<()> {
        use crate::audio_engine::constants::{SPEED_MAX, SPEED_MIN};
        if !speed.is_finite() || !(SPEED_MIN..=SPEED_MAX).contains(&speed) {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "speed out of range",
            ));
        }
        if !period_seconds.is_finite()
            || period_seconds <= 0.0
            || !(period_seconds * f64::from(RATE) * 4.0).is_finite()
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "master period out of range",
            ));
        }
        let _requests = self.engine.pad_request_ids.lock().unwrap();
        crate::audio_engine::push_global_rate_message_qualified(
            &self.engine.global_timing_revision,
            &self.engine.input_runtime_ownership.global_timing_publishing,
            &self.engine.input_runtime_ownership.global_rate_status,
            &mut self.parameter_producer.lock().unwrap(),
            Some(speed),
            Some(period_seconds),
            anchor_binding
                .map(|binding| crate::audio_engine::qualify_timing_anchor(&self.engine, binding))
                .transpose()?,
            "SetSpeedAndMasterPeriod",
        )?;
        self.original_rate_revision.store(
            self.engine.global_timing_revision.load(Ordering::Acquire),
            Ordering::Release,
        );
        Ok(())
    }
}

fn original_public_rate_revision(locals: &Py<PyDict>) -> u64 {
    Python::attach(|py| {
        locals
            .bind(py)
            .get_item("bpm_audio")
            .unwrap()
            .unwrap()
            .extract::<PyRef<PublicBpmAudio>>()
            .unwrap()
            .original_rate_revision
            .load(Ordering::Acquire)
    })
}

enum FixtureSet {
    Second,
    Third,
}

fn v2_files(
    engine: &AudioEngine,
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
) -> (PreparedStemPair, PreparedSourceTicket) {
    version_files(
        engine,
        root,
        version,
        original_reference,
        frames,
        FixtureSet::Second,
    )
}

fn version_files(
    engine: &AudioEngine,
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
    set: FixtureSet,
) -> (PreparedStemPair, PreparedSourceTicket) {
    let reference = version_wav_files(root, version, original_reference, frames, set);
    let source = engine
        .capture_prepared_source(0, version.to_owned())
        .unwrap();
    let pair = engine
        .prepare_stem_pair_at_root(root, 0, version, &reference, &source, false, None)
        .unwrap();
    assert!(!pair.has_components());
    assert_eq!(
        pair.pair_reader.descriptor.content.alignment.offset_frames,
        0
    );
    assert_complete_selection(root, &selection(&pair));
    (pair, source)
}

fn version_wav_files(
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
    set: FixtureSet,
) -> String {
    let (generation, first, second, instrumental) = match set {
        FixtureSet::Second => (0x301_u32, "vocals", "drums", [0, 1, 2]),
        FixtureSet::Third => (0x302_u32, "melody", "bass", [2, 1, 3]),
    };
    let generation = format!("{generation:032x}");
    let reference = original_reference.replace(GENERATION, &generation);
    let path = root.parent().unwrap().join(&reference);
    write_signal_set(&path, frames, version, 0);
    let first_path = path.join(format!("{first}.wav"));
    let second_path = path.join(format!("{second}.wav"));
    let first_contents = fs::read(&first_path).unwrap();
    let second_contents = fs::read(&second_path).unwrap();
    fs::write(first_path, second_contents).unwrap();
    fs::write(second_path, first_contents).unwrap();
    fs::write(
        path.join("instrumental.wav"),
        signal_sum_wav(frames, &instrumental),
    )
    .unwrap();
    let hashes: serde_json::Map<_, _> = STEM_FILE_NAMES
        .iter()
        .map(|name| {
            let content = fs::read(path.join(format!("{name}.wav"))).unwrap();
            (
                (*name).to_owned(),
                json!(format!("{:x}", Sha256::digest(content))),
            )
        })
        .collect();
    fs::write(
        path.join(".complete.json"),
        serde_json::to_vec(&json!({
            "schema": "stem-set-sha256-v1", "source_version": version, "stems": hashes,
        }))
        .unwrap(),
    )
    .unwrap();
    reference
}

fn commit_candidate_v2(
    engine: &AudioEngine,
    root: &Path,
    version: &str,
    original_reference: &str,
    frames: usize,
    locals: &Py<PyDict>,
) {
    let (v2, source_ticket) = v2_files(engine, root, version, original_reference, frames);
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("candidate_json", v2.selection_json().to_owned())
            .unwrap();
        locals
            .bind(py)
            .set_item("prepared_v2", Py::new(py, v2).unwrap())
            .unwrap();
        locals
            .bind(py)
            .set_item("source_v2", Py::new(py, source_ticket).unwrap())
            .unwrap();
    });
    controller_python(
        locals,
        r#"
disk_v2 = StemPairSelection.model_validate_json(candidate_json)
assert disk_v2 != disk_v1
entry_v1 = project.stem_cache[0]
entry_v2 = StemCacheEntry(source_version=fixture['source_version'], cache_dir=disk_v2.wav_generation,
    stems=expected_stem_files(disk_v2.wav_generation), pair=disk_v2, available=True)
# Both real complete assignment owners survive the actual fenced config replacement.
owner_v1 = audio.acquire_project_asset_lease(str(Path(entry_v1.cache_dir).absolute()))
owner_v2 = audio.acquire_project_asset_lease(str(Path(entry_v2.cache_dir).absolute()))
persistence.commit_stem_selection(0, entry_v1, entry_v2,
    lambda: project.stem_cache[0] is entry_v1 and source_v2.same_source_request(
        audio.capture_prepared_source(0, fixture['source_version'])))
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None
assert reopened.project.stem_cache[0].pair == disk_v2
project.stem_cache[0] = entry_v2
prepared_v2.select()
controller._register_committed_pair(0, prepared_v2, source_v2)
live = controller._committed_residency._live_selections[0]
assert live.control is previous_control and live.attempt is None
assert live.disk.selection.ticket.publication_status() == 'accepted'
assert previous_control.ticket.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
"#,
    );
}

fn complete_reference_at_loop(
    full: &SampleBuffer,
    stems: &PreparedStemSet,
    ratio: f64,
    key_lock_enabled: bool,
    timing: Option<(
        crate::audio_engine::constant_timing::AcceptedTimingProjection,
        f64,
    )>,
    region: crate::audio_engine::source_reader::FrameRange,
) -> Box<RtMixer> {
    let accepted = timing.map(|(projection, _)| projection);
    let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
    // Immutable standalone oracle; actual Engine source/timing authority stays above.
    let ownership = Arc::new(InputRuntimeOwnership::default());
    ownership.publish_test_source(0, full, RATE, 1);
    mixer.set_input_runtime_ownership(ownership.clone());
    mixer.load_sample(0, full.clone());
    assert!(mixer.publish_prepared_stems(0, stems.clone()));
    if let Some((projection, master_period)) = timing {
        let publication = PreparedSourcePermit::unrestricted();
        publication.mark_pending().unwrap();
        // Only this independent complete-PCM reference copies the already actual
        // ACK. Product source authority came from the queued publication above.
        assert!(
            mixer.publish_constant_timing_rt(
                0,
                crate::audio_engine::constant_timing::PreparedConstantTiming {
                    content_key: ownership
                        .binding_placement(0)
                        .and_then(|snapshot| snapshot.content_key()),
                    reference: full.clone(),
                    publication,
                    projection,
                },
                &mut crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement,
            )
        );
        mixer.set_bpm_lock(true);
        mixer.set_master_period(master_period);
    }
    mixer.set_loop_region_frames(0, region.start, Some(region.end));
    mixer.set_speed(ratio);
    let key_lock = ownership.next_key_lock_request(0).unwrap();
    mixer.set_pad_key_lock_request(
        0,
        key_lock_enabled,
        key_lock,
        InputPadBinding {
            placement: ownership.binding_placement(0),
            resident: full.resident_binding(),
            source_address: full.source_address(),
            sample_count: full.source_sample_count(),
            channels: full.channels,
            sample_rate_hz: RATE,
            authority_revision: ownership.authority_revision(0),
            runtime_revision: 0,
            accepted,
        },
        1,
    );
    mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn own_adoption(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    name: &str,
    key_lock: bool,
) {
    assert_eq!(playback.drain(consumer), 1, "own callback Capture");
    own_adoption_after_capture(playback, consumer, locals, name, key_lock);
}

fn own_adoption_after_capture(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    name: &str,
    key_lock: bool,
) {
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, name, "residency_status") == "rejected"
    });
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "pending"
    );
    if voice(&playback.callback.mixer).paused {
        playback.render_paused(31);
    } else if key_lock {
        playback.render(31, false);
    } else {
        render_varispeed(playback, 31);
    }
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    assert_eq!(history.is_some(), key_lock);
    assert_eq!(playback.drain(consumer), 1, "own callback Adopt");
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "transitioning"
    );
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let next_history = after.stretch.productive_history().unwrap();
        assert_eq!(next_history.binding, history.binding);
        assert_eq!(next_history.next_position, history.next_position);
        assert_eq!(next_history.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(after.stretch.productive_history().is_none());
    }
}

fn render_varispeed(playback: &mut ControllerPlayback, frames: usize) -> (Vec<f32>, Vec<f32>) {
    render_varispeed_with_historical_adoption(playback, frames, None)
}

fn render_varispeed_with_historical_adoption(
    playback: &mut ControllerPlayback,
    frames: usize,
    historical_adoption: Option<u64>,
) -> (Vec<f32>, Vec<f32>) {
    assert!(!playback.callback.mixer.key_lock_for_measurement(0));
    assert!(playback.raw.is_none() && playback.candidate.is_none());
    let capacity = playback.callback.mixer.max_realtime_render_frames();
    assert!(frames <= capacity || playback.rate_ledger.is_some());
    // The clock, interpolation, selected sum and ramp gains come from complete
    // independent PCM, not from the actual SourcePlayback or SourceReadPlan.
    let feed = playback.feed(playback.source_frame, frames);
    let before = voice(&playback.callback.mixer);
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let mut output = vec![0.0; frames * 2];
    let mut expected = vec![0.0; frames * 2];
    let mut full_mix = vec![0.0; frames * 2];
    reset_tap_observation_for_test();
    if playback.scheduled {
        assert_eq!(playback.callback.transport.output_frame(), playback.frame);
        let callback = playback.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        callback.transport.advance_by_rendered_frames(frames);
    } else {
        playback.callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            &mut playback.callback.retirement,
        );
    }
    let taps = tap_observation_for_test();
    assert_eq!(taps.missing_reads, 0);
    assert!(
        taps.min_frame
            // The callback observation includes pad215's independent loop.
            .is_some_and(|frame| frame >= playback.resident_start().min(LOOP_START))
    );
    assert!(
        taps.max_frame
            .is_some_and(|frame| frame < playback.resident_end())
    );
    for (mixer, target) in [
        (playback.reference.as_mut(), &mut expected),
        (playback.full_mix.as_mut(), &mut full_mix),
    ] {
        mixer.render_at_output_frame(playback.frame, target, &mut [0.0; NUM_SAMPLES]);
    }
    assert_eq!(
        output, expected,
        "controller Varispeed chronology at {}",
        playback.frame
    );
    let current = voice(&playback.callback.mixer);
    // Internal scratch retains only the final actual rate chunk. Derive its
    // prefix independently from the outer capacity and declared epoch boundary;
    // the complete rendered output below still verifies every callback sample.
    let scratch_start = playback.rate_ledger.as_ref().map_or(0, |ledger| {
        let outer = (frames - 1) / capacity * capacity;
        let epoch = ledger.epoch(playback.source_frame + frames as u64 - 1);
        outer.max(epoch.start.saturating_sub(playback.source_frame) as usize)
    });
    let scratch_frames = frames - scratch_start;
    for (channel, expected) in feed.iter().enumerate() {
        assert_eq!(
            &current.stretch.varispeed_buffers()[channel][..scratch_frames],
            &expected[scratch_start..]
        );
        assert_eq!(
            &current.stretch.output_buffers()[channel][..scratch_frames],
            &expected[scratch_start..]
        );
    }
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    assert!(current.stretch.productive_history().is_none());
    assert!(!current.stretch.wet_render_ready());
    assert!(!current.stretch.wet_output_ready());
    assert!(!current.stretch.wet_current_source_output_ready());
    assert_eq!(current.stretch.adopted_request_id(), historical_adoption);
    assert!(
        current
            .source_playback
            .matches_exact(&voice(&playback.reference).source_playback)
    );
    assert!(
        current
            .source_playback
            .matches_exact(&voice(&playback.full_mix).source_playback)
    );
    assert_eq!(
        current.stretch.pending_fifo_frames(),
        voice(&playback.reference).stretch.pending_fifo_frames()
    );
    let end = playback.source_frame + frames as u64;
    if playback.seek_epoch.is_some() {
        let (whole, fraction, mode) = playback.seek_position(end);
        let position = current.source_playback.position();
        assert_eq!(position.frame, whole);
        assert_eq!(position.fraction.to_bits(), fraction.to_bits());
        assert_eq!(
            current.source_playback.loop_period().map(f64::to_bits),
            (playback.loop_period != (playback.loop_end - playback.loop_start) as f64)
                .then_some(playback.loop_period.to_bits())
        );
        assert_eq!(current.explicit_seek_mode, mode);
    } else {
        let (whole, fraction) = playback.rate_ledger.as_ref().map_or_else(
            || {
                let phase = (end as f64 * playback.ratio).rem_euclid(playback.loop_period);
                (phase.floor() as usize, phase.fract())
            },
            |ledger| ledger.position(end),
        );
        assert_eq!(
            current.source_playback.position().frame,
            playback.loop_start + whole
        );
        assert_eq!(
            current.source_playback.position().fraction.to_bits(),
            fraction.to_bits()
        );
    }
    let applied = playback
        .rate_ledger
        .as_ref()
        .map_or(playback.ratio, |ledger| ledger.epoch(end - 1).rate);
    assert_eq!(
        current.source_playback.tempo_ratio().to_bits(),
        applied.to_bits()
    );
    if playback.rate_ledger.is_some() {
        assert_eq!(
            current.source_playback.loop_period().map(f64::to_bits),
            (playback.loop_period != (playback.loop_end - playback.loop_start) as f64)
                .then_some(playback.loop_period.to_bits())
        );
    }
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        playback.filter.begin_frame();
        for channel in 0..2 {
            assert_eq!(
                actual[channel],
                playback
                    .filter
                    .process_sample(channel, feed[channel][frame]),
                "complete PCM Varispeed + uninterrupted EQ at {}",
                playback.frame
            );
        }
    }
    for mixer in [
        &mut playback.callback.mixer,
        &mut playback.reference,
        &mut playback.full_mix,
    ] {
        assert!(!voice_mut(mixer).stretch.source_preparation_ready());
    }
    playback.frame += frames as u64;
    playback.source_frame += frames as u64;
    if playback.scheduled {
        assert_eq!(playback.callback.transport.output_frame(), playback.frame);
    }
    (output, full_mix)
}

fn ramp_varispeed(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    if playback.rate_ledger.is_none() {
        let declared_ratio = voice(&playback.callback.mixer)
            .source_timing
            .accepted
            .map_or(1.25, |projection| {
                projection.period_seconds * (playback.target_bpm / 60.0)
            });
        assert_eq!(playback.ratio.to_bits(), declared_ratio.to_bits());
    }
    let start = playback.source_frame;
    let mut remaining = playback.rate_ledger.as_ref().map_or_else(
        || (128.0 / playback.ratio).ceil() as usize - (start - playback.ramp_start) as usize,
        |ledger| {
            (1..=256)
                .find(|frames| {
                    ledger.distance(start + *frames as u64) - ledger.distance(playback.ramp_start)
                        >= 128.0
                })
                .expect("minimum supported rate consumes128 source frames within256 outputs")
        },
    );
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let count = frames.min(remaining);
        render_varispeed(playback, count);
        remaining -= count;
        if remaining == 0 {
            break;
        }
        assert_eq!(
            controller_ticket_status(locals, name, "effective_mode_status"),
            "transitioning"
        );
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
}

fn settle_varispeed(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let fifo = voice(&playback.callback.mixer)
        .stretch
        .pending_fifo_frames();
    ramp_varispeed(playback, locals, name);
    // OFF effectiveness follows actual dry source consumption. No Wet or 4096
    // Native preparation is invoked or used as evidence for this branch.
    let mut output = Vec::new();
    let mut counterfactual = Vec::new();
    for frames in [1, 127, 257, 511, 31, 137] {
        let (actual, full_mix) = render_varispeed(playback, frames);
        output.extend(actual);
        counterfactual.extend(full_mix);
    }
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert!(
        output
            .iter()
            .zip(counterfactual)
            .any(|(actual, full_mix)| (actual - full_mix).abs() > 0.005)
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .pending_fifo_frames(),
        fifo
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "effective"
    );
    if playback.scheduled && name == "candidate" {
        controller_python(
            locals,
            "if live_bpm:\n    assert session.stem_generation_errors == {0: bpm_error}",
        );
    }
    if name == "candidate" {
        controller_python(
            locals,
            "if retrigger:\n    assert session.stem_generation_errors == {0: play_error}\nif window_change:\n    assert session.stem_generation_errors == {0: window_error}",
        );
    }
    controller_python(
        locals,
        &format!(
            "assert {name}.is_current() and {name}.claimed()\ncontroller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors"
        ),
    );
}

fn ramp_wet_with_rate_ledger(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    let start = playback.source_frame;
    let ledger = playback.rate_ledger.as_ref().unwrap();
    let mut remaining = (1..=256)
        .find(|frames| {
            ledger.distance(start + *frames as u64) - ledger.distance(playback.ramp_start) >= 128.0
        })
        .expect("supported rate consumes128 source frames within256 outputs");
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let count = frames.min(remaining);
        playback.render(count, false);
        remaining -= count;
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            controller_ticket_status(locals, name, "effective_mode_status"),
            "transitioning"
        );
        if remaining == 0 {
            break;
        }
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }
}

fn settle_wet(playback: &mut ControllerPlayback, locals: &Py<PyDict>, name: &str) {
    settle_wet_after(playback, locals, name, None);
}

fn settle_wet_after(
    playback: &mut ControllerPlayback,
    locals: &Py<PyDict>,
    name: &str,
    minimum_warm_start: Option<u64>,
) {
    if playback.rate_ledger.is_some() {
        ramp_wet_with_rate_ledger(playback, locals, name);
    } else {
        playback.ramp(locals, name, playback.ratio);
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    finish_wet_after_ramp(playback, locals, name, minimum_warm_start);
}

fn finish_wet_after_ramp(
    playback: &mut ControllerPlayback,
    locals: &Py<PyDict>,
    name: &str,
    minimum_warm_start: Option<u64>,
) {
    // This real timestamped feed starts a fresh ordinary Native preparation.
    // adopt_native independently reconstructs its complete 4096-frame raw feed.
    let after_ramp = playback.frame;
    playback.render(13, true);
    if let Some(minimum) = minimum_warm_start {
        wait_until(|| {
            voice_mut(&mut playback.callback.mixer)
                .stretch
                .source_preparation_ready()
        });
        let target = voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        assert!(
            target - crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES as u64
                >= minimum.max(after_ramp),
            "own complete stem warm interval starts after its Adopt and ramp"
        );
    }
    let (actual, full_mix) = playback.adopt_native(false);
    assert!(actual.iter().any(|value| value.abs() > 0.001));
    assert!(
        actual
            .iter()
            .zip(full_mix)
            .any(|(actual, full)| (actual - full).abs() > 0.005)
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "effective"
    );
    if name == "candidate" {
        controller_python(
            locals,
            "if retrigger:\n    assert session.stem_generation_errors == {0: play_error}\nif window_change:\n    assert session.stem_generation_errors == {0: window_error}",
        );
    }
    controller_python(
        locals,
        &format!(
            "assert {name}.is_current() and {name}.claimed()\ncontroller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors"
        ),
    );
}

struct FinalReaderProbe {
    identity: std::sync::Weak<[u8; 32]>,
    pcm: [std::sync::Weak<[f32]>; 4],
    areas: [PathBuf; 3],
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl FinalReaderProbe {
    fn new(root: &Path, selection: &Value, actual: &PreparedStemSet) -> Self {
        let areas = ["wav_generation", "pcm_generation", "descriptor_reference"]
            .map(|field| selected_path(root, selection, field));
        let mut files = Vec::new();
        for area in &areas {
            if area.is_file() {
                files.push((area.clone(), fs::read(area).unwrap()));
            } else {
                for entry in fs::read_dir(area).unwrap() {
                    let path = entry.unwrap().path();
                    assert!(path.is_file());
                    files.push((path.clone(), fs::read(path).unwrap()));
                }
            }
        }
        Self {
            identity: Arc::downgrade(&actual.complete_set_identity),
            pcm: std::array::from_fn(|index| Arc::downgrade(&actual.stems[index].samples)),
            areas,
            files,
        }
    }

    fn assert_retained(&self) {
        assert!(self.identity.strong_count() > 0);
        assert!(self.pcm.iter().all(|pcm| pcm.strong_count() > 0));
        self.assert_files_retained();
    }

    fn assert_files_retained(&self) {
        for (path, before) in &self.files {
            assert_eq!(
                &fs::read(path).unwrap(),
                before,
                "retained file: {}",
                path.display()
            );
        }
    }

    fn assert_area_retained(&self, index: usize) {
        let mut count = 0;
        for (path, before) in &self.files {
            if path.starts_with(&self.areas[index]) {
                assert_eq!(
                    &fs::read(path).unwrap(),
                    before,
                    "preserved area file: {}",
                    path.display()
                );
                count += 1;
            }
        }
        assert!(count > 0, "preservation must cover known leaves");
    }

    fn wait_for(&self, stage: &str, mut ready: impl FnMut() -> bool) {
        let start = Instant::now();
        while !ready() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{stage}: identity={}, PCM={:?}, areas={:?}",
                self.identity.strong_count(),
                self.pcm.each_ref().map(std::sync::Weak::strong_count),
                self.areas.each_ref().map(|path| path.exists())
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn productive_final_reader_after_claim(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    locals: &Py<PyDict>,
    old: &FinalReaderProbe,
    current: &FinalReaderProbe,
    busy_consumer: Option<&mut rtrb::Consumer<ControlMessage>>,
    outcome: FinalReaderOutcome,
) {
    assert!(playback.callback.retirement.actual.is_some());
    assert!(playback.callback.retirement.retired.is_empty());
    // Test-local receipts/registrations must not counterfeit a final native reader.
    // The controller has already observed V2's own accepted residency above.
    controller_python(locals, "del first, previous_control");
    // The regular public Native loader poll reconciles accepted control caches.
    // Receipt-only controller polling cannot end those separate native owners.
    Python::attach(|py| {
        engine.poll_loader_events(py).unwrap();
    });
    old.assert_retained();
    current.assert_retained();
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let generation = voice(&playback.callback.mixer).generation;
    if let Some(consumer) = busy_consumer {
        public_busy_handover_pause_resume(playback, consumer, locals, old, current);
    }
    playback.callback.retirement.slots = 0;
    playback.ramp(locals, "candidate", playback.ratio);
    let outgoing = playback
        .callback
        .mixer
        .outgoing_stems_for_measurement(0)
        .unwrap();
    assert!(
        old.identity
            .ptr_eq(&Arc::downgrade(&outgoing.complete_set_identity))
    );
    old.assert_retained();
    current.assert_retained();
    assert!(playback.callback.retirement.retired.is_empty());
    let held_counts = old.pcm.each_ref().map(std::sync::Weak::strong_count);
    playback.callback.retirement.slots = usize::MAX;
    playback.render(1, false);
    assert!(
        playback
            .callback
            .mixer
            .outgoing_stems_for_measurement(0)
            .is_none()
    );
    // An older Adopt payload can also hold V1. Place an independent marker after
    // the actual outgoing transfer in the same production FIFO. Its completed
    // Drop proves the worker consumed every prior owner, not just some V1 Arc.
    let marker = Arc::new(AtomicU8::new(0));
    let completion = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    old.wait_for("FIFO worker completion after outgoing transfer", || {
        completion.strong_count() == 0
    });
    old.wait_for("audio worker outgoing transfer", || {
        old.pcm
            .iter()
            .zip(held_counts)
            .all(|(pcm, held)| pcm.strong_count() < held)
    });
    assert!(playback.callback.retirement.retired.is_empty());
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        controller_ticket_status(locals, "candidate", "effective_mode_status"),
        "transitioning"
    );
    // No outgoing slot or shared oracle Arcs remain. The used V1 Native/history
    // still protects every actual component and the complete pair on disk.
    old.assert_retained();
    current.assert_retained();
    assert!(fs::write(old.areas[1].join("vocals.f32le"), b"leased overwrite").is_err());
    if matches!(outcome, FinalReaderOutcome::PreserveUnknownPcm) {
        // Admission happened before this child existed. Actual Native/history
        // still owns every old component; neither the fixture nor its oracle does.
        assert_eq!(engine.project_assets.status().unwrap().0, 3);
        assert!(engine.project_assets.status().unwrap().3.is_empty());
        let unknown = old.areas[1].join(UNKNOWN_PCM_CHILD);
        assert!(!unknown.exists());
        fs::write(&unknown, UNKNOWN_PCM_BYTES).unwrap();
        old.assert_retained();
    }
    let after_ramp = playback.frame;
    finish_wet_after_ramp(playback, locals, "candidate", Some(after_ramp));
    assert_ne!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    Python::attach(|py| {
        engine.poll_loader_events(py).unwrap();
    });
    old.wait_for("actual final native pins", || {
        Python::attach(|py| {
            engine.poll_loader_events(py).unwrap();
        });
        old.identity.strong_count() == 0 && old.pcm.iter().all(|pcm| pcm.strong_count() == 0)
    });
    match outcome {
        FinalReaderOutcome::SavedCopy => {
            // C has a genuine saved-assignment lease, not an invented PCM owner.
            old.assert_files_retained();
        }
        FinalReaderOutcome::PreserveUnknownPcm => old.assert_area_retained(1),
        FinalReaderOutcome::Cleanup => {
            // Use the actual process-wide asset worker, with no collector shortcut.
            old.wait_for("productive asset worker cleanup", || {
                Python::attach(|py| {
                    engine.poll_loader_events(py).unwrap();
                });
                old.areas.iter().all(|path| !path.exists())
            });
        }
    }
    current.assert_retained();
    for frames in [1, 137, 511, 257] {
        playback.render(frames, false);
        current.assert_retained();
    }
}

fn pcm_preservation_after_readers(
    engine: &AudioEngine,
    root: &Path,
    playback: &mut ControllerPlayback,
    locals: &Py<PyDict>,
    probes: (&FinalReaderProbe, &FinalReaderProbe),
) {
    let (old, current) = probes;
    assert_eq!(old.identity.strong_count(), 0);
    assert!(old.pcm.iter().all(|pcm| pcm.strong_count() == 0));
    let unknown = old.areas[1].join(UNKNOWN_PCM_CHILD);
    let expected = format!(
        "{}: generation contains an unknown file; preserved",
        fs::canonicalize(&old.areas[1]).unwrap().display()
    );
    old.wait_for(
        "paired PCM terminal preservation and independent members",
        || {
            old.assert_area_retained(1);
            assert_eq!(fs::read(&unknown).unwrap(), UNKNOWN_PCM_BYTES);
            let status = engine.project_assets.status().unwrap();
            status.0 == 0
                && status.3.as_slice() == std::slice::from_ref(&expected)
                && !old.areas[0].exists()
                && !old.areas[2].exists()
        },
    );
    // The complete pair has independent targets. PCM preservation does not
    // imply all-three atomic preservation or an owned-original receipt.
    assert!(Arc::ptr_eq(
        &engine.project_assets,
        &crate::audio_engine::project_assets::ProjectAssets::shared()
    ));
    for (material, remove_unknown) in [
        ("M55555555555555555555555555555555", false),
        ("M66666666666666666666666666666666", true),
    ] {
        if remove_unknown {
            // Remove only the caller's own child. This is no re-admission of
            // the terminally preserved generation.
            fs::remove_file(&unknown).unwrap();
        }
        for frames in [1, 137, 511, 257] {
            playback.render(frames, false);
            current.assert_retained();
            old.assert_area_retained(1);
        }
        let sentinel = root
            .join("materials")
            .join(material)
            .join("original/sentinel.wav");
        fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        assert!(!sentinel.exists());
        fs::write(&sentinel, stereo_wav(64)).unwrap();
        let mut pin = engine.project_assets.acquire_pin(root, &sentinel).unwrap();
        engine
            .project_assets
            .retire(root, &sentinel, false)
            .unwrap();
        pin.release();
        drop(pin);
        old.wait_for(
            "same asset worker progress after paired PCM preservation",
            || {
                old.assert_area_retained(1);
                if !remove_unknown {
                    assert_eq!(fs::read(&unknown).unwrap(), UNKNOWN_PCM_BYTES);
                }
                !sentinel.exists()
            },
        );
        let status = engine.project_assets.status().unwrap();
        assert_eq!(status.0, 0);
        assert_eq!(status.3.as_slice(), std::slice::from_ref(&expected));
        assert!(status.3.len() <= 16, "bounded control errors");
        old.assert_area_retained(1);
        current.assert_retained();
    }
    assert!(!unknown.exists());
    assert_eq!(old.identity.strong_count(), 0);
    assert!(old.pcm.iter().all(|pcm| pcm.strong_count() == 0));
    controller_python(
        locals,
        "assert candidate.is_current() and candidate.claimed()\nassert candidate.effective_mode_status() == 'effective'\nassert not session.stem_generation_errors",
    );
}

fn settle_empty(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    assert_eq!(playback.selection, Some(0));
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    if key_lock {
        playback.ramp(locals, "candidate", playback.ratio);
        let empty_source_start = playback.source_frame;
        // Old Wet/FIFO samples and filter tails need not be silent. All samples
        // still match the uninterrupted independent raw-Native/EQ oracle.
        playback.render(13, true);
        wait_until(|| {
            voice_mut(&mut playback.callback.mixer)
                .stretch
                .source_preparation_ready()
        });
        let target = voice_mut(&mut playback.callback.mixer)
            .stretch
            .prepared_target_output_frame()
            .unwrap();
        let history_frames = crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES;
        assert_eq!(history_frames, 4096);
        let warm_source_start =
            target - history_frames as u64 - (playback.frame - playback.source_frame);
        assert!(
            warm_source_start >= empty_source_start,
            "own complete preparation follows the empty ramp"
        );
        assert!(
            playback
                .feed(warm_source_start, history_frames)
                .iter()
                .flatten()
                .all(|sample| *sample == 0.0)
        );
        controller_python(
            locals,
            r#"
assert candidate.effective_mode_status() == 'transitioning'
controller.on_frame_render()
assert live.disk.selection is None and live.disk.registration == 'deferred'
assert live.control is v2_control and live.attempt is v2_control
assert v2_control.receipt is candidate and v2_control.receipt_intent == ('all_stems', 0)
assert live.desired == ('all_stems', 0b1010)
assert candidate.claimed() and candidate.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v3.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v1.stem_set_identity
assert status.registration == 'deferred' and status.effective == 'transitioning'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
        );
        assert!(
            consumer.is_empty(),
            "V3 stays deferred after the ramp while own Native is prepared"
        );
        playback.adopt_native(false);
        assert_ne!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
        playback.render(127, true);
    } else {
        ramp_varispeed(playback, locals, "candidate");
        render_varispeed(playback, 127);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert!(
        playback
            .feed(playback.source_frame, 127)
            .iter()
            .flatten()
            .all(|sample| *sample == 0.0)
    );
    let actual = voice(&playback.callback.mixer);
    for channel in 0..2 {
        // Only the freshly rendered prefix is output, not unused scratch space.
        assert!(
            actual.stretch.varispeed_buffers()[channel][..127]
                .iter()
                .all(|sample| *sample == 0.0)
        );
        assert!(
            actual.stretch.output_buffers()[channel][..127]
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
    assert_eq!(
        controller_ticket_status(locals, "candidate", "effective_mode_status"),
        "effective"
    );
    // Do not poll the effective receipt yet: V2's own effectiveness precedes
    // any V3 registration, independently of the earlier pre-Native poll.
}

fn settle_full_mix(playback: &mut ControllerPlayback, locals: &Py<PyDict>, key_lock: bool) {
    assert_eq!(playback.selection, None);
    let generation = voice(&playback.callback.mixer).generation;
    let mut output = Vec::new();
    if key_lock {
        playback.ramp(locals, "full", playback.ratio);
        playback.render(13, true);
        output.extend(playback.adopt_native(false).0);
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
    } else {
        ramp_varispeed(playback, locals, "full");
        for frames in [1, 127, 257, 511, 31, 137] {
            output.extend(render_varispeed(playback, frames).0);
        }
    }
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(
        controller_ticket_status(locals, "full", "effective_mode_status"),
        "effective"
    );
    controller_python(
        locals,
        "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
    );
}

fn withdraw_queued_candidate_for_full_mix(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert_eq!(
        playback.drain(consumer),
        1,
        "actual V2 Capture before withdrawal"
    );
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, "candidate", "residency_status") == "rejected"
    });
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
            if transaction.id == 0 && transaction.target == StemMixMode::AllStems
            && transaction.mask == 0b1010 && transaction.candidate.is_some()))
    );
    controller_python(
        locals,
        r#"
withdrawn = candidate
withdrawn_registration = live.disk.selection.ticket
assert withdrawn.residency_status() == 'pending' and withdrawn.effective_mode_status() == 'waiting'
assert withdrawn.is_current() and not withdrawn.claimed()
assert controller.set_stem_mix_mode(0, 'full_mix')
full = live.control.receipt
assert full is not withdrawn and full.is_current()
assert full.residency_status() in {'captured', 'pending'} and full.effective_mode_status() == 'waiting'
assert live.control is previous_control and live.attempt is previous_control
assert live.control.receipt_intent == ('full_mix', 0b1010)
assert live.disk.selection is None and live.disk.registration == 'withdrawn'
assert live.desired == ('full_mix', 0b1010)
assert withdrawn.residency_status() == 'rejected' and withdrawn.effective_mode_status() == 'rejected'
assert not withdrawn.claimed() and not withdrawn.is_current()
assert withdrawn.effective_failure_reason() is None
assert withdrawn_registration.publication_status() == 'accepted' and not withdrawn_registration.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity == disk_v1.stem_set_identity
assert project.pad_stem_mix_mode[0] == 'full_mix' and project.stem_cache[0].pair == disk_v2
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[0].pair == disk_v2
assert reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    let set = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let message = consumer.pop().unwrap();
    assert!(matches!(&message, ControlMessage::CommittedStemWork(work)
        if matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
            if !transaction.selection.current() && transaction.stems.is_some())));
    let callback = playback.callback.as_mut();
    assert!(callback.retirement.available_retirement_slots() >= 3);
    // The unchanged actual queued V2 message reaches the same Native handler
    // as drain_control_messages, without also draining the later FullMix Capture.
    crate::audio_engine::audio_stream::process_control_message(
        message,
        &mut callback.scheduler,
        playback.frame,
        &mut callback.quantization,
        &mut callback.transport,
        &mut callback.mixer,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    let current = voice(&callback.mixer);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let after = current.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(current.stretch.productive_history().is_none());
    }
    let after_dsp = callback.mixer.dsp_source_history_for_test(0).unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(
        callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    assert_eq!(callback.mixer.stem_demand_for_measurement(0).1, 0b1010);
    let current_set = callback.mixer.stems_for_measurement()[0].as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &current_set.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&current_set.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), CommittedStemRtWork::Capture(capture)
            if capture.id == 0 && capture.target == StemMixMode::FullMix))
    );
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert not withdrawn.claimed() and withdrawn.residency_status() == 'rejected'
assert live.control is previous_control and live.attempt is previous_control
assert full.effective_mode_status() == 'waiting'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    if key_lock {
        playback.render(31, false);
    } else {
        render_varispeed(playback, 31);
    }
    own_adoption(playback, consumer, locals, "full", key_lock);
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert live.control is previous_control and live.control.receipt is full
assert full.claimed() and full.effective_mode_status() == 'transitioning'
assert controller.stem_residency_status(0).effective_stem_set_identity == disk_v1.stem_set_identity
"#,
    );
    settle_full_mix(playback, locals, key_lock);
    controller_python(
        locals,
        r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.control is previous_control and live.attempt is None
    assert live.disk.selection is None and live.disk.registration == 'withdrawn'
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity is None
    assert status.requested_mode == 'full_mix' and status.effective == 'effective'
assert full.is_current() and full.residency_status() == 'accepted'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(consumer.is_empty(), "poll cannot retry withdrawn V2 ALL");
    controller_python(
        locals,
        r#"
assert controller.set_stem_mix_mode(0, 'all_stems')
fresh_registration = live.disk.selection.ticket
candidate = live.attempt.receipt
assert fresh_registration is not withdrawn_registration and fresh_registration.is_current()
assert fresh_registration.publication_status() == 'accepted'
assert candidate is not withdrawn and candidate is not full
assert candidate.is_current() and not candidate.claimed()
assert candidate.residency_status() in {'captured', 'pending'} and candidate.effective_mode_status() == 'waiting'
assert live.control is previous_control
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity is None
assert project.pad_stem_mix_mode[0] == 'all_stems'
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[0].pair == disk_v2
assert reopened.project.pad_stem_mix_mode[0] == 'all_stems'
"#,
    );
}

fn full_queue_then_explicit_retry(
    engine: &AudioEngine,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
    worker_post: bool,
) {
    assert!(consumer.is_empty());
    let set = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    let source = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    assert_eq!(demand.0, StemMixMode::AllStems);
    assert_eq!(demand.1, 0b1010);
    let intent = engine.resident_stem_cache.lock().unwrap()[0]
        .intent_epoch
        .load(Ordering::Acquire);
    controller_python(
        locals,
        r#"
pressure_selection = live.disk.selection
pressure_registration = pressure_selection.ticket
pressure_error = ('committed stem command queue is full' if worker_post_pressure
    else 'Stem residency admission failed: committed residency queue is full')
assert first.is_current() and first.effective_mode_status() == 'effective'
assert pressure_registration.is_current() and pressure_registration.publication_status() == 'accepted'
assert pressure_selection.receipt is None and pressure_selection.receipt_intent is None
if worker_post_pressure:
    assert controller.set_stem_mix_mode(0, 'all_stems')
    failed_post = pressure_selection.receipt
    assert failed_post is not first and failed_post.is_current() and not failed_post.claimed()
    assert failed_post.effective_mode_status() == 'waiting'
    assert live.attempt is pressure_selection and not live.blocked
    assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    {
        let mut queue = producer.lock().unwrap();
        if worker_post {
            assert!(
                matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
                if matches!(work.as_ref(), crate::audio_engine::committed_stem_residency::CommittedStemRtWork::Capture(capture)
                    if capture.id == 0 && capture.target == StemMixMode::AllStems))
            );
            // Actual Capture can complete, but the worker cannot post Adopt while
            // this exact Producer mutex is held. Fill its freed slot before unlock.
            assert_eq!(
                playback.drain(consumer),
                1,
                "own Capture before worker post pressure"
            );
            assert!(consumer.is_empty());
        }
        // Ping uses this actual control queue; Volume uses a different queue.
        for _ in 0..8 {
            queue.push(ControlMessage::Ping()).unwrap();
        }
        assert!(queue.is_full());
    }
    if worker_post {
        wait_until(|| {
            Python::attach(|py| {
                locals
                    .bind(py)
                    .get_item("failed_post")
                    .unwrap()
                    .unwrap()
                    .call_method0("error")
                    .unwrap()
                    .extract::<Option<String>>()
                    .unwrap()
                    .as_deref()
                    == Some("committed stem command queue is full")
            })
        });
        wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    }
    controller_python(
        locals,
        r#"
if not worker_post_pressure:
    assert not controller.set_stem_mix_mode(0, 'all_stems')
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.desired == ('all_stems', 0b1010)
    assert live.disk.selection is pressure_selection
    if worker_post_pressure:
        assert live.attempt is pressure_selection and pressure_selection.receipt is failed_post
        assert pressure_selection.receipt_intent == ('all_stems', 0b1010)
        assert failed_post.residency_status() == 'rejected' and failed_post.effective_mode_status() == 'rejected'
        assert not failed_post.is_current() and not failed_post.claimed()
        assert failed_post.effective_failure_reason() is None and failed_post.error() == pressure_error
    else:
        assert live.attempt is None
        assert pressure_selection.receipt is None and pressure_selection.receipt_intent is None
    assert pressure_registration.is_current() and pressure_registration.publication_status() == 'accepted'
    assert live.control is previous_control and previous_control.receipt is first
    assert previous_control.ticket.is_current() and first.is_current()
    assert first.residency_status() == 'accepted' and first.effective_mode_status() == 'effective'
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity == disk_v1.stem_set_identity
    assert status.attempt_stem_set_identity == (disk_v2.stem_set_identity if worker_post_pressure else disk_v1.stem_set_identity)
    assert status.effective == (None if worker_post_pressure else 'effective')
    assert status.residency == ('rejected' if worker_post_pressure else 'accepted')
    assert status.desired_mode == 'all_stems'
    assert session.stem_generation_errors == {0: pressure_error}
assert project.stem_cache[0].pair == disk_v2 and project.pad_stem_mix_mode[0] == 'all_stems'
"#,
    );
    assert!(matches!(consumer.peek(), Ok(ControlMessage::Ping())));
    assert_eq!(playback.drain(consumer), 8, "only the eight actual Pings");
    assert!(
        consumer.is_empty(),
        "failed operation queued no additional work"
    );
    assert_eq!(
        engine.resident_stem_cache.lock().unwrap()[0]
            .intent_epoch
            .load(Ordering::Acquire),
        intent
    );
    assert!(!engine.input_runtime_ownership.resident_control_pending(0));
    let current = voice(&playback.callback.mixer);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let after = current.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(current.stretch.productive_history().is_none());
    }
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(Arc::ptr_eq(
        &source.samples,
        &playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    let after = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &after.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&after.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    let mut output = Vec::new();
    for frames in [1, 31, 137, 257, 511] {
        output.extend(if key_lock {
            playback.render(frames, false).0
        } else {
            render_varispeed(playback, frames).0
        });
        controller_python(
            locals,
            r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.disk.selection is pressure_selection
    if worker_post_pressure:
        assert live.attempt is pressure_selection and pressure_selection.receipt is failed_post
        assert not failed_post.is_current() and not failed_post.claimed()
        assert failed_post.residency_status() == 'rejected' and failed_post.error() == pressure_error
        assert controller.stem_residency_status(0).effective is None
    else:
        assert live.attempt is None
        assert pressure_selection.receipt is None and pressure_selection.receipt_intent is None
    assert first.is_current() and first.effective_mode_status() == 'effective'
    assert controller.stem_residency_status(0).effective_stem_set_identity == disk_v1.stem_set_identity
    assert session.stem_generation_errors == {0: pressure_error}
"#,
        );
        assert!(
            consumer.is_empty(),
            "drained queue does not authorize polling retry"
        );
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    assert!(output.iter().any(|sample| sample.abs() > 0.001));
    controller_python(
        locals,
        r#"
assert controller.set_stem_mix_mode(0, 'all_stems')
assert not live.blocked and live.attempt is pressure_selection
assert live.disk.selection is pressure_selection and pressure_selection.ticket is pressure_registration
candidate = pressure_selection.receipt
assert candidate is not first and candidate.is_current() and not candidate.claimed()
if worker_post_pressure:
    assert candidate is not failed_post and not failed_post.is_current() and not failed_post.claimed()
    assert failed_post.residency_status() == 'rejected' and failed_post.error() == pressure_error
assert candidate.residency_status() in {'captured', 'pending'} and candidate.effective_mode_status() == 'waiting'
assert pressure_selection.receipt_intent == ('all_stems', 0b1010)
assert live.control is previous_control and first.is_current()
status = controller.stem_residency_status(0)
assert status.effective == 'waiting' and status.attempt_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity == disk_v1.stem_set_identity
assert session.stem_generation_errors == {0: pressure_error}
"#,
    );
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), crate::audio_engine::committed_stem_residency::CommittedStemRtWork::Capture(capture)
            if capture.id == 0 && capture.target == StemMixMode::AllStems))
    );
}

fn pending_capture_deadline_then_explicit_retry(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert!(consumer.is_empty());
    assert_eq!(playback.ratio, 0.75);
    assert!(playback.raw.is_some());
    let set = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    let source = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    let start = playback.source_frame;
    controller_python(
        locals,
        r#"
deadline_selection = live.disk.selection
deadline_registration = deadline_selection.ticket
deadline_error = 'Committed stem voice capture deadline expired'
assert controller.set_stem_mix_mode(0, 'all_stems')
failed_capture = deadline_selection.receipt
assert failed_capture is not first and failed_capture.is_current() and not failed_capture.claimed()
assert failed_capture.residency_status() == 'captured' and failed_capture.effective_mode_status() == 'waiting'
assert deadline_registration.is_current() and deadline_registration.publication_status() == 'accepted'
assert live.attempt is deadline_selection and live.control is previous_control
assert first.is_current() and first.effective_mode_status() == 'effective'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    playback.callback.retirement.slots = 2;
    let mut partitions = [1, 31, 137, 257, 511].into_iter().cycle();
    let mut nontrivial_output = false;
    // StemController receipt polling does not reconcile the Native loader.
    // Let the genuine worker Capture deadline win before the first loader poll;
    // this watchdog does not replace or shorten either product deadline.
    wait_until_for(Duration::from_secs(45), || {
        let error = Python::attach(|py| {
            locals
                .bind(py)
                .get_item("failed_capture")
                .unwrap()
                .unwrap()
                .call_method0("error")
                .unwrap()
                .extract::<Option<String>>()
                .unwrap()
        });
        if error.as_deref() == Some("Committed stem voice capture deadline expired") {
            return true;
        }
        assert!(
            matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
            if matches!(work.as_ref(), CommittedStemRtWork::Capture(capture)
                if capture.id == 0 && capture.target == StemMixMode::AllStems))
        );
        assert_eq!(
            playback.drain(consumer),
            0,
            "retirement capacity retains real Capture"
        );
        let output = playback.render(partitions.next().unwrap(), false).0;
        nontrivial_output |= output.iter().any(|sample| sample.abs() > 0.001);
        controller_python(
            locals,
            r#"
controller.on_frame_render()
assert live.disk.selection is deadline_selection and live.attempt is deadline_selection
assert deadline_selection.receipt is failed_capture and live.control is previous_control
assert deadline_registration.is_current() and previous_control.ticket.is_current()
assert previous_control.receipt is first and first.is_current()
assert first.effective_mode_status() == 'effective'
assert live.desired == ('all_stems', 0b1010)
assert controller.stem_residency_status(0).effective_stem_set_identity == disk_v1.stem_set_identity
assert failed_capture.effective_failure_reason() is None
"#,
        );
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0),
            demand
        );
        false
    });
    assert!(nontrivial_output && playback.source_frame > start);
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    Python::attach(|py| {
        engine.poll_loader_events(py).unwrap();
    });
    controller_python(
        locals,
        r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.attempt is deadline_selection
    assert live.control is previous_control and previous_control.receipt is first
    assert live.disk.selection is deadline_selection and deadline_selection.ticket is deadline_registration
    assert deadline_registration.is_current() and deadline_registration.publication_status() == 'accepted'
    assert first.is_current() and first.effective_mode_status() == 'effective'
    assert failed_capture.residency_status() == 'rejected' and failed_capture.effective_mode_status() == 'rejected'
    assert not failed_capture.is_current() and not failed_capture.claimed()
    assert failed_capture.error() == deadline_error and failed_capture.effective_failure_reason() is None
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity == disk_v1.stem_set_identity
    assert status.attempt_stem_set_identity == disk_v2.stem_set_identity
    assert status.residency == 'rejected' and status.effective is None
    assert live.desired == ('all_stems', 0b1010)
    assert session.stem_generation_errors == {0: deadline_error}
"#,
    );
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history().unwrap();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(playback.drain(consumer), 1, "actual unchanged late Capture");
    assert!(consumer.is_empty(), "expired Capture cannot enqueue Adopt");
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    let after_history = after.stretch.productive_history().unwrap();
    assert_eq!(after_history.binding, history.binding);
    assert_eq!(after_history.next_position, history.next_position);
    assert_eq!(after_history.fed_output_frames, history.fed_output_frames);
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(Arc::ptr_eq(
        &source.samples,
        &playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    let after_set = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &after_set.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&after_set.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    for frames in [1, 31, 137, 257, 511] {
        playback.render(frames, false);
        controller_python(
            locals,
            "controller.on_frame_render()\nassert live.blocked and live.attempt is deadline_selection\nassert failed_capture.error() == deadline_error\nassert session.stem_generation_errors == {0: deadline_error}",
        );
        assert!(
            consumer.is_empty(),
            "polling after late Capture cannot retry"
        );
    }
    controller_python(
        locals,
        r#"
assert controller.set_stem_mix_mode(0, 'all_stems')
candidate = live.attempt.receipt
assert live.attempt is deadline_selection and live.disk.selection is deadline_selection
assert deadline_selection.ticket is deadline_registration and deadline_registration.is_current()
assert candidate is not failed_capture and candidate is not first
assert candidate.is_current() and not candidate.claimed() and candidate.effective_mode_status() == 'waiting'
assert live.control is previous_control and not live.blocked
assert controller.stem_residency_status(0).effective_stem_set_identity == disk_v1.stem_set_identity
assert session.stem_generation_errors == {0: deadline_error}
"#,
    );
}

#[allow(clippy::too_many_arguments)]
fn current_bank_candidate_cannot_borrow_frozen_voice(
    engine: &AudioEngine,
    root: &Path,
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    use crate::audio_engine::cold_residency::ResidentLoadHint;
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    use crate::audio_engine::source_reader::StemRenderSelection;
    use crate::messages::ResidentContext;

    // Keep the existing complete A PCM/raw Native/EQ chronology. This native
    // restore is deliberately separate from public Loader assignment replacement.
    assert!(!playback.scheduled && playback.candidate.is_none());
    let a = voice(&playback.callback.mixer).sample.clone().unwrap();
    let a_set = playback.callback.mixer.stems_for_measurement()[0].clone();
    let generation = voice(&playback.callback.mixer).generation;
    let adopted = voice(&playback.callback.mixer).stretch.adopted_request_id();
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let selection = playback
        .selection
        .map_or_else(StemRenderSelection::full_mix, |mask| {
            StemRenderSelection::for_stems(a_set.as_ref().unwrap(), mask)
        });
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let feedback = playback.callback.feedback.events.len();

    macro_rules! unchanged {
        ($action:block) => {{
            let current = voice(&playback.callback.mixer);
            let clock = current.source_playback;
            let fifo = current.stretch.pending_fifo_frames();
            let history = current.stretch.productive_history();
            let dsp = playback.callback.mixer.dsp_source_history_for_test(0).unwrap();
            $action
            let current = voice(&playback.callback.mixer);
            assert!(current.source_playback.matches_exact(&clock));
            assert_eq!(current.generation, generation);
            assert_eq!(current.stretch.native_state_address(), native);
            assert_eq!(current.stretch.pending_fifo_frames(), fifo);
            assert!(Arc::ptr_eq(&current.sample.as_ref().unwrap().samples, &a.samples));
            assert_eq!(history.is_some(), key_lock);
            if let Some(history) = history {
                let next = current.stretch.productive_history().unwrap();
                assert_eq!(next.binding, history.binding);
                assert_eq!(next.next_position, history.next_position);
                assert_eq!(next.fed_output_frames, history.fed_output_frames);
            } else {
                assert!(current.stretch.productive_history().is_none());
            }
            let next = playback.callback.mixer.dsp_source_history_for_test(0).unwrap();
            assert_eq!(next.binding, dsp.binding);
            assert_eq!(next.next_position, dsp.next_position);
            assert_eq!(next.fed_output_frames, dsp.fed_output_frames);
            assert_eq!(playback.callback.feedback.events.len(), feedback);
        }};
    }

    // Actual distinct B content and typed complete lease, not a cloned A source.
    let input = signal_sum_wav(4096, &[4, 5, 6, 7]);
    let expected_b: Vec<f32> = input[44..]
        .chunks_exact(2)
        .map(|bytes| f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0)
        .collect();
    let input_path = root.join("retained-old-bank-b.wav");
    fs::write(&input_path, input).unwrap();
    let material = Arc::new(prepare_material(root, &input_path, RATE, 2, &|| false).unwrap());
    let version = canonical_version(&material);
    let reference = canonical_wav_reference(&material);
    let wav_path = root.parent().unwrap().join(&reference);
    write_signal_set(&wav_path, 4096, &version, 4);
    assert_eq!(material.sample.samples.as_ref(), expected_b.as_slice());
    let _original_owner = engine
        .project_assets
        .acquire(root, &material.lease.original_path)
        .unwrap();
    let _component_owner = engine.project_assets.acquire(root, &wav_path).unwrap();

    // This negative intentionally exercises complete ON bank B; its physical
    // whole-source hint preserves that demand independently of finite admission.
    playback.callback.retirement.slots = 2;
    let admitted = crate::audio_engine::cold_load::admit_selected_material(
        engine,
        0,
        material.metadata()["new_reference"]
            .as_str()
            .unwrap()
            .to_owned(),
        (
            false,
            false,
            false,
            Some(ResidentLoadHint {
                start_s: if key_lock {
                    0.0
                } else {
                    LOOP_START as f64 / f64::from(RATE)
                },
                end_s: if key_lock { 4096.0 } else { LOOP_END as f64 } / f64::from(RATE),
                key_lock,
            }),
        ),
        producer.clone(),
        (2, RATE, root.to_path_buf()),
        "restore",
        Some(material.clone()),
    )
    .unwrap();
    wait_until(|| consumer.slots() == 2 || admitted.adoption.load(Ordering::Acquire) != 0);
    assert_eq!(admitted.adoption.load(Ordering::Acquire), 0);
    controller_python(
        locals,
        r#"
assert not source_v2.same_preparation_context(source_v2)
assert not candidate.is_current() and not candidate.claimed()
assert not previous_control.ticket.is_current()
"#,
    );
    unchanged!({
        assert_eq!(playback.drain(consumer), 0);
    });
    // Reject obsolete A Capture through its normal retirement gate; B Load
    // remains behind it until its larger normal reservation becomes available.
    playback.callback.retirement.slots = 3;
    unchanged!({
        assert_eq!(playback.drain(consumer), 1);
    });
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::LoadColdSample {
        id: 0, replace_assignment: false, sample, ..
    }) if !sample.same_source(&a))
    );
    playback.callback.retirement.slots = usize::MAX;
    unchanged!({
        assert_eq!(playback.drain(consumer), 1);
    });
    source_successes(engine, &[admitted.request_id]);
    let b = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(!b.same_source(&a));
    assert!(!Arc::ptr_eq(&b.samples, &a.samples));
    assert_eq!(b.frame_count(), 4096);
    let expected_context = if key_lock {
        ResidentContext::KeyLockFullTrack
    } else {
        // Prepared migration preserves the actual effective tuple, so its
        // finite coverage is Native-capable even while logical KEY LOCK is OFF.
        ResidentContext::KeyLockFiniteLoop
    };
    assert_eq!(b.residency.as_ref().unwrap().context, expected_context);
    assert_eq!(
        playback.callback.mixer.key_lock_for_measurement(0),
        key_lock
    );
    assert_eq!(
        b.samples.as_ref(),
        &expected_b[b.resident_start() * 2..b.resident_end() * 2]
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());

    let assert_retained = |playback: &ControllerPlayback| {
        let current = voice(&playback.callback.mixer);
        assert_eq!(current.generation, generation);
        assert_eq!(current.stretch.native_state_address(), native);
        assert_eq!(current.stretch.adopted_request_id(), adopted);
        assert_eq!(
            current.source_playback.tempo_ratio().to_bits(),
            playback.ratio.to_bits()
        );
        let phase = (playback.source_frame as f64 * playback.ratio)
            .rem_euclid((LOOP_END - LOOP_START) as f64);
        assert_eq!(
            current.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            current.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        if let Some(raw) = &playback.raw {
            assert_eq!(
                current.stretch.pending_fifo_frames(),
                (raw.input[0].len(), raw.output[0].len()),
                "retained A independent raw Native FIFO chronology"
            );
        }
        assert!(Arc::ptr_eq(
            &current.sample.as_ref().unwrap().samples,
            &a.samples
        ));
        let frozen = current
            .frozen_stems
            .as_ref()
            .expect("actual retained A voice");
        assert_eq!(frozen.selection, selection);
        assert!(!frozen.transition.is_active() && frozen.outgoing.is_none());
        match (&a_set, &frozen.set) {
            (Some(old), Some(frozen)) => {
                assert!(Arc::ptr_eq(
                    &old.complete_set_identity,
                    &frozen.complete_set_identity
                ));
                for (old, frozen) in old.stems.iter().zip(&frozen.stems) {
                    assert!(Arc::ptr_eq(&old.samples, &frozen.samples));
                }
            }
            (None, None) => {}
            _ => panic!("retained A readers changed"),
        }
        let other = other_voice(&playback.callback.mixer);
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        let phase =
            (playback.frame as f64 * playback.ratio).rem_euclid((LOOP_END - LOOP_START) as f64);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
        assert!(playback.callback.mixer.stems_for_measurement()[215].is_none());
        assert_eq!(playback.callback.feedback.events.len(), feedback);
    };
    assert_retained(playback);

    let window = prepare_window_with_producer(
        engine,
        0,
        WindowRequest {
            key_lock: Some(key_lock),
            loop_region: Some((
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        producer.clone(),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok() || window.error().unwrap().is_some());
    unchanged!({
        assert_eq!(playback.drain(consumer), 1);
    });
    assert_eq!(
        window.publication_status(),
        "accepted",
        "{:?}",
        window.error()
    );
    reconcile(engine).unwrap();
    let finite_b = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(finite_b.same_source(&b) && !finite_b.same_source(&a));
    assert_eq!(
        (finite_b.resident_start(), finite_b.resident_end()),
        (LOOP_START, LOOP_END)
    );
    assert_eq!(
        finite_b.residency.as_ref().unwrap().context,
        if key_lock {
            ResidentContext::KeyLockFiniteLoop
        } else {
            ResidentContext::FiniteLoop
        }
    );
    assert_eq!(
        finite_b.samples.as_ref(),
        &expected_b[LOOP_START * 2..LOOP_END * 2]
    );
    assert_retained(playback);

    let source = engine.capture_prepared_source(0, version.clone()).unwrap();
    assert!(source.same_preparation_context(&source));
    let pair = engine
        .prepare_stem_pair_at_root(root, 0, &version, &reference, &source, false, None)
        .unwrap();
    assert!(!pair.has_components());
    assert_eq!(
        pair.pair_reader.descriptor.content.alignment.offset_frames,
        0
    );
    pair.select();
    let registration = engine
        .register_pending_committed_stem_pair_with_producer(&pair, &source, producer)
        .unwrap();
    assert_eq!(registration.publication_status(), "accepted");
    assert!(registration.is_current() && registration.error().is_none());
    let receipt = engine
        .request_committed_stem_residency_with_producer(
            &registration,
            "all_stems",
            0b1010,
            producer,
        )
        .unwrap();
    assert_eq!(receipt.residency_status(), "captured");
    assert_eq!(receipt.effective_mode_status(), "waiting");
    assert!(receipt.is_current() && !receipt.claimed());
    let queued = match consumer.peek().unwrap() {
        ControlMessage::CommittedStemWork(work) => {
            assert!(
                matches!(work.as_ref(), CommittedStemRtWork::Capture(capture)
                if capture.id == 0 && capture.candidate.is_some())
            );
            work.as_ref() as *const CommittedStemRtWork as usize
        }
        _ => panic!("expected B's own actual Candidate Capture"),
    };
    playback.callback.retirement.slots = 2;
    unchanged!({
        assert_eq!(playback.drain(consumer), 0);
    });
    for frames in [1, 31, 137, 257] {
        if key_lock {
            playback.render(frames, false);
        } else {
            render_varispeed(playback, frames);
        }
        assert_retained(playback);
        assert_eq!(receipt.residency_status(), "captured");
        assert!(receipt.is_current() && !receipt.claimed());
        assert!(
            matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
            if work.as_ref() as *const CommittedStemRtWork as usize == queued)
        );
        unchanged!({
            assert_eq!(playback.drain(consumer), 0);
        });
    }
    playback.callback.retirement.slots = 3;
    unchanged!({
        assert_eq!(
            playback.drain(consumer),
            1,
            "own B Capture must reject frozen A"
        );
    });
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    assert_eq!(receipt.residency_status(), "rejected");
    assert_eq!(receipt.effective_mode_status(), "rejected");
    assert!(!receipt.is_current() && !receipt.claimed());
    assert_eq!(
        receipt.error().as_deref(),
        Some("Committed stem residency or mode ownership changed")
    );
    assert!(receipt.effective_failure_reason().is_none());
    assert_eq!(registration.publication_status(), "accepted");
    assert!(registration.is_current() && source.same_preparation_context(&source));
    assert!(consumer.is_empty(), "rejected Capture cannot queue Adopt");
    assert_retained(playback);
    for frames in [1, 31, 137, 257, 511] {
        if key_lock {
            playback.render(frames, false);
        } else {
            render_varispeed(playback, frames);
        }
        assert_retained(playback);
        assert!(
            playback.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap()
                .same_window(&finite_b)
        );
        assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
        assert!(registration.is_current() && !receipt.is_current() && !receipt.claimed());
        assert!(consumer.is_empty());
        unchanged!({
            assert_eq!(playback.drain(consumer), 0);
        });
    }
    controller_python(
        locals,
        r#"
assert not source_v2.same_preparation_context(source_v2)
assert candidate.residency_status() == 'rejected' and not candidate.claimed()
assert not candidate.is_current() and candidate.effective_failure_reason() is None
assert not previous_control.ticket.is_current()
"#,
    );
}

fn prepare_public_unload_loader(locals: &Py<PyDict>) {
    controller_python(
        locals,
        r#"
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.transport import TransportController
class UnloadAudio:
    def __getattr__(self, name):
        for delegate in (load_audio, playback_audio, audio):
            if hasattr(delegate, name):
                return getattr(delegate, name)
        raise AttributeError(name)
unload_audio = UnloadAudio()
unload_transport = TransportController(project, session, unload_audio)
unload_notifications = []
loader = LoaderController(project, session, unload_audio,
    unload_notifications.append, persistence.mark_dirty,
    on_stems_deleted=controller.delete_stems)
loader.bind_asset_lifecycle(controller._assets)
loader.set_stems_invalidated_callback(controller.invalidate_stem_cache)
def unloaded_control(sample_id):
    unload_transport.residency.cancel(sample_id)
    unload_transport.playback.discard_global_restore_for_unloaded_pad(sample_id)
loader.set_sample_unloaded_callback(unloaded_control)
"#,
    );
}

struct FrozenVoiceSnapshot {
    id: usize,
    clock: crate::audio_engine::source_playback::SourcePlayback,
    generation: u64,
    source_key: crate::audio_engine::content_placement::ContentKey,
    source_pcm: std::sync::Weak<[f32]>,
    source_address: usize,
    native: usize,
    adopted: Option<u64>,
    fifo: (usize, usize),
    history: Option<crate::audio_engine::productive_source_history::ProductiveSourceHistory>,
    dsp: Option<crate::audio_engine::productive_source_history::ProductiveSourceHistory>,
    filter: crate::audio_engine::dsp::DspStateForTest,
}

impl FrozenVoiceSnapshot {
    fn capture(mixer: &RtMixer, id: usize) -> Self {
        let voice = mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .unwrap();
        let sample = voice.sample.as_ref().unwrap();
        Self {
            id,
            clock: voice.source_playback,
            generation: voice.generation,
            source_key: voice
                .source_admission
                .as_ref()
                .unwrap()
                .content_key()
                .unwrap(),
            source_pcm: Arc::downgrade(&sample.samples),
            source_address: sample.source_address(),
            native: voice.stretch.native_state_address(),
            adopted: voice.stretch.adopted_request_id(),
            fifo: voice.stretch.pending_fifo_frames(),
            history: voice.stretch.productive_history(),
            dsp: mixer.dsp_source_history_for_test(id),
            filter: mixer
                .dsp_chain_snapshot_for_test(id)
                .unwrap()
                .state_for_test(),
        }
    }

    fn assert_current(&self, mixer: &RtMixer, paused: bool) {
        self.assert_current_with_control(mixer, paused, &self.filter.control);
    }

    fn assert_current_with_control(
        &self,
        mixer: &RtMixer,
        paused: bool,
        control: &crate::audio_engine::dsp::DspControlProvenanceForTest,
    ) {
        let voice = mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(self.id))
            .unwrap();
        assert_eq!(voice.paused, paused);
        assert!(voice.source_playback.matches_exact(&self.clock));
        assert_eq!(voice.generation, self.generation);
        assert_eq!(
            voice.source_admission.as_ref().unwrap().content_key(),
            Some(self.source_key)
        );
        let sample = voice.sample.as_ref().unwrap();
        assert!(self.source_pcm.ptr_eq(&Arc::downgrade(&sample.samples)));
        assert_eq!(sample.source_address(), self.source_address);
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.stretch.adopted_request_id(), self.adopted);
        assert_eq!(voice.stretch.pending_fifo_frames(), self.fifo);
        for (before, after) in [
            (self.history, voice.stretch.productive_history()),
            (self.dsp, mixer.dsp_source_history_for_test(self.id)),
        ] {
            match (before, after) {
                (Some(before), Some(after)) => {
                    assert_eq!(after.binding, before.binding);
                    assert_eq!(after.next_position, before.next_position);
                    assert_eq!(after.domain, before.domain);
                    assert_eq!(after.fed_output_frames, before.fed_output_frames);
                }
                (None, None) => {}
                _ => panic!("ordinary transport changed retained source history"),
            }
        }
        let actual_filter = mixer
            .dsp_chain_snapshot_for_test(self.id)
            .unwrap()
            .state_for_test();
        assert_eq!(actual_filter.signal_image, self.filter.signal_image);
        assert_eq!(&actual_filter.control, control);
    }
}

fn assert_current_bank_transport_refused(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    resume: bool,
) {
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    let transport = playback.callback.transport;
    let frame = playback.frame;
    let consumed = playback.source_frame;
    let paused = [
        voice(&playback.callback.mixer).paused,
        other_voice(&playback.callback.mixer).paused,
    ];
    let snapshots = [0, 215].map(|id| FrozenVoiceSnapshot::capture(&playback.callback.mixer, id));
    let epochs = [
        voice(&playback.callback.mixer),
        other_voice(&playback.callback.mixer),
    ]
    .map(|voice| voice.stretch.source_epoch_owner().load(Ordering::Acquire));
    let frozen = voice(&playback.callback.mixer)
        .frozen_stems
        .as_ref()
        .unwrap();
    let transition = frozen.transition;
    let selection = frozen.selection;
    let sets = [
        frozen.set.as_ref().unwrap(),
        frozen.outgoing.as_ref().unwrap(),
    ];
    let identities = sets.map(|set| Arc::as_ptr(&set.complete_set_identity) as usize);
    let pcm = sets.map(|set| {
        set.stems
            .each_ref()
            .map(|stem| stem.samples.as_ptr() as usize)
    });
    let binding = engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    assert!(binding.is_current());
    let b_key = binding.binding.placement.unwrap().content_key().unwrap();
    assert_ne!(b_key, snapshots[0].source_key);
    assert_eq!(
        engine.input_runtime_ownership.callback_content_key(0),
        Some(b_key)
    );
    let feedback_before = playback.callback.feedback.events.len();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("refused_bank_resume", resume)
            .unwrap();
    });
    controller_python(
        locals,
        r#"
original_bank_transport = (playback_audio.resume_sample(0) if refused_bank_resume
                           else playback_audio.pause_sample(0))
current_bank_binding = playback_audio.current_input_runtime_pad_binding(0)
assert current_bank_binding is not None and current_bank_binding.is_current()
assert original_bank_transport.current_control_slot() == 0
assert original_bank_transport.content_lifetime == current_bank_binding.metadata()['content_lifetime']
assert original_bank_transport.requested_paused is not refused_bank_resume
"#,
    );
    let admission =
        Python::attach(|py| {
            locals.bind(py).get_item("original_bank_transport").unwrap().unwrap()
            .extract::<PyRef<crate::audio_engine::ordinary_transport::OrdinaryTransportAdmission>>()
            .unwrap().clone()
        });
    assert_eq!(admission.content_lifetime(), b_key.lifetime());
    assert_eq!(
        playback.drain(consumer),
        1,
        "original current-B transport refusal"
    );
    assert!(consumer.is_empty());
    assert_eq!(playback.callback.feedback.events.len(), feedback_before + 1);
    let message = &playback.callback.feedback.events[feedback_before];
    let observation = admission.current_observation().unwrap().unwrap();
    assert_eq!(observation.current_control_slot().unwrap(), Some(0));
    assert!(observation.matches_admission(&admission) && observation.settles_admission(&admission));
    assert_eq!(observation.outcome(), Some("refused"));
    assert_eq!(
        observation.requested_effect(),
        Some(if resume { "resume" } else { "pause" })
    );
    assert!(!observation.active() && !observation.ordinary_paused());
    match message {
        AudioMessage::OrdinaryTransportFeedback {
            id,
            owner,
            cell,
            lifetime,
            transport_revision,
            observation_sequence,
        } => {
            assert_eq!(*id, 0);
            assert_eq!(
                *owner,
                engine.input_runtime_ownership.owner_identity().diagnostic()
            );
            assert_eq!(*cell, b_key.cell().index());
            assert_eq!(*lifetime, b_key.lifetime());
            assert_eq!(*transport_revision, admission.transport_revision());
            assert_eq!(*observation_sequence, observation.observation_sequence());
        }
        _ => panic!("current-B refusal must publish its own ordinary observation"),
    }
    let resolved = engine
        ._resolve_ordinary_transport_feedback(message)
        .unwrap()
        .unwrap();
    assert!(resolved.matches_admission(&admission) && resolved.settles_admission(&admission));
    assert_eq!(resolved.outcome(), Some("refused"));
    controller_python(
        locals,
        r#"
transport.playback.poll_pause_observations()
assert session.active_sample_ids == {215}
assert session.paused_sample_ids == ({215} if refused_bank_resume else set())
assert project.sample_paths[0] == frozen_saved_path and project.pad_content[0] == frozen_saved_content
assert candidate.claimed() and not candidate.is_current()
assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'rejected'
"#,
    );
    for (index, snapshot) in snapshots.iter().enumerate() {
        snapshot.assert_current(&playback.callback.mixer, paused[index]);
    }
    let after = voice(&playback.callback.mixer)
        .frozen_stems
        .as_ref()
        .unwrap();
    assert_eq!(after.selection, selection);
    assert!(after.transition.matches_exact(transition));
    let sets = [
        after.set.as_ref().unwrap(),
        after.outgoing.as_ref().unwrap(),
    ];
    assert_eq!(
        sets.map(|set| Arc::as_ptr(&set.complete_set_identity) as usize),
        identities
    );
    assert_eq!(
        sets.map(|set| set
            .stems
            .each_ref()
            .map(|stem| stem.samples.as_ptr() as usize)),
        pcm
    );
    assert_eq!(
        [
            voice(&playback.callback.mixer),
            other_voice(&playback.callback.mixer)
        ]
        .map(|voice| voice.stretch.source_epoch_owner().load(Ordering::Acquire)),
        epochs
    );
    assert_eq!((playback.frame, playback.source_frame), (frame, consumed));
    assert_eq!(
        format!("{:?}", playback.callback.transport),
        format!("{transport:?}")
    );
}

fn paused_frozen_source_readers_before_unload(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    probes: (&FinalReaderProbe, &FinalReaderProbe),
) {
    let (old, current) = probes;
    controller_python(
        locals,
        r#"
assert original_a_pause.current_control_slot() is None
assert original_a_pause.current_observation() is None
assert original_peer_pause.current_control_slot() == 215
peer_pause_observation = original_peer_pause.current_observation()
assert peer_pause_observation is not None and peer_pause_observation.settles_admission(original_peer_pause)
assert peer_pause_observation.outcome == 'applied' and peer_pause_observation.ordinary_paused
"#,
    );
    assert_current_bank_transport_refused(engine, playback, consumer, locals, true);
    let snapshots = [0, 215].map(|id| FrozenVoiceSnapshot::capture(&playback.callback.mixer, id));
    let frozen = voice(&playback.callback.mixer)
        .frozen_stems
        .as_ref()
        .unwrap();
    let transition = frozen.transition;
    let selection = frozen.selection;
    let identities = [
        frozen.set.as_ref().unwrap(),
        frozen.outgoing.as_ref().unwrap(),
    ]
    .map(|set| Arc::as_ptr(&set.complete_set_identity) as usize);
    let pcm = [
        frozen.set.as_ref().unwrap(),
        frozen.outgoing.as_ref().unwrap(),
    ]
    .map(|set| {
        set.stems
            .each_ref()
            .map(|stem| stem.samples.as_ptr() as usize)
    });
    assert!(transition.is_active());
    let frame = playback.frame;
    let consumed = playback.source_frame;
    let mut expected_transport = playback.callback.transport;
    let feedback = playback.callback.feedback.events.len();
    let raw_fifo = playback
        .raw
        .as_ref()
        .map(|raw| (raw.input[0].len(), raw.output[0].len()));
    let filter = format!("{:?}", playback.filter);
    let epochs = [
        voice(&playback.callback.mixer),
        other_voice(&playback.callback.mixer),
    ]
    .map(|voice| voice.stretch.source_epoch_owner().load(Ordering::Acquire));
    let current_keys = [0, 215].map(|id| {
        engine
            .input_runtime_ownership
            .slot_snapshot(id)
            .unwrap()
            .content_key()
            .unwrap()
    });
    let mut expected_controls = snapshots.each_ref().map(|snapshot| snapshot.filter.control);
    for (index, control) in expected_controls.iter_mut().enumerate() {
        let (original_key, since, gain_bits) = control
            .stable_input
            .expect("actual stable input before silence");
        assert!(since <= frame);
        assert!(
            control
                .input_observed_through
                .is_some_and(|end| end <= frame)
        );
        assert!(control.input_revision != u64::MAX);
        assert!(f32::from_bits(gain_bits).is_finite());
        // The expected signal image stays original. Only the callback's
        // current-content stability provenance changes at the first real
        // render boundary; no observer implementation is replayed here.
        control.stable_input = Some((
            current_keys[index],
            if original_key == current_keys[index] {
                since
            } else {
                frame
            },
            gain_bits,
        ));
    }
    for frames in [1, 127, 385] {
        let start = playback.frame;
        assert_eq!(
            [0, 215].map(|id| {
                engine
                    .input_runtime_ownership
                    .slot_snapshot(id)
                    .unwrap()
                    .content_key()
                    .unwrap()
            }),
            current_keys
        );
        playback.render_paused(frames);
        assert_eq!(playback.frame, start + frames as u64);
        assert_eq!(
            [0, 215].map(|id| {
                engine
                    .input_runtime_ownership
                    .slot_snapshot(id)
                    .unwrap()
                    .content_key()
                    .unwrap()
            }),
            current_keys
        );
        for (index, snapshot) in snapshots.iter().enumerate() {
            expected_controls[index].input_observed_through = Some(playback.frame);
            snapshot.assert_current_with_control(
                &playback.callback.mixer,
                true,
                &expected_controls[index],
            );
        }
        assert_eq!(
            [
                voice(&playback.callback.mixer),
                other_voice(&playback.callback.mixer)
            ]
            .map(|voice| voice.stretch.source_epoch_owner().load(Ordering::Acquire)),
            epochs
        );
        let frozen = voice(&playback.callback.mixer)
            .frozen_stems
            .as_ref()
            .unwrap();
        assert_eq!(frozen.selection, selection);
        assert!(frozen.transition.matches_exact(transition));
        assert_eq!(
            [
                frozen.set.as_ref().unwrap(),
                frozen.outgoing.as_ref().unwrap()
            ]
            .map(|set| Arc::as_ptr(&set.complete_set_identity) as usize),
            identities
        );
        assert_eq!(
            [
                frozen.set.as_ref().unwrap(),
                frozen.outgoing.as_ref().unwrap()
            ]
            .map(|set| set
                .stems
                .each_ref()
                .map(|stem| stem.samples.as_ptr() as usize)),
            pcm
        );
        old.assert_retained();
        current.assert_retained();
    }
    expected_transport.advance_by_rendered_frames(513);
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    assert_eq!(
        format!("{:?}", playback.callback.transport),
        format!("{expected_transport:?}")
    );
    assert_eq!(
        (playback.frame, playback.source_frame),
        (frame + 513, consumed)
    );
    assert_eq!(playback.callback.feedback.events.len(), feedback);
    assert_eq!(
        playback
            .raw
            .as_ref()
            .map(|raw| (raw.input[0].len(), raw.output[0].len())),
        raw_fifo
    );
    assert_eq!(format!("{:?}", playback.filter), filter);
    assert!(
        fs::write(
            old.areas[1].join("vocals.f32le"),
            b"paused retained overwrite"
        )
        .is_err()
    );
    assert!(
        fs::write(
            current.areas[1].join("vocals.f32le"),
            b"paused current overwrite"
        )
        .is_err()
    );
    controller_python(
        locals,
        r#"
transport.playback.resume_pad(215)
peer_resume = transport.playback._pause_intent._owners[project.pad_content[215].instance_id].admission
assert peer_resume is not None and not peer_resume.requested_paused
assert peer_resume.current_control_slot() == 215
assert session.active_sample_ids == {215} and session.paused_sample_ids == {215}
"#,
    );
    let transport = playback.callback.transport;
    assert_eq!(
        playback.drain(consumer),
        1,
        "only the independent peer resumes before Unload"
    );
    controller_python(
        locals,
        r#"
peer_resumed = peer_resume.current_observation()
assert peer_resumed is not None and peer_resumed.matches_admission(peer_resume)
assert peer_resumed.settles_admission(peer_resume) and peer_resumed.outcome == 'applied'
assert peer_resumed.active and not peer_resumed.ordinary_paused
transport.playback.poll_pause_observations()
assert session.active_sample_ids == {215} and not session.paused_sample_ids
"#,
    );
    snapshots[0].assert_current_with_control(&playback.callback.mixer, true, &expected_controls[0]);
    snapshots[1].assert_current_with_control(
        &playback.callback.mixer,
        false,
        &expected_controls[1],
    );
    assert_eq!(
        (playback.frame, playback.source_frame),
        (frame + 513, consumed)
    );
    assert_eq!(
        format!("{:?}", playback.callback.transport),
        format!("{transport:?}")
    );
    old.assert_retained();
    current.assert_retained();
}

fn frozen_source_final_readers_then_public_unload(
    engine: &AudioEngine,
    fixture: (&Path, &Arc<Mutex<rtrb::Producer<ControlMessage>>>),
    playback: &mut ControllerPlayback,
    queues: (
        &mut rtrb::Consumer<ControlMessage>,
        &mut rtrb::Consumer<ControlParameterMessage>,
    ),
    locals: &Py<PyDict>,
    probes: (&FinalReaderProbe, &FinalReaderProbe),
    lifecycle: CandidateLifecycle,
) {
    use crate::audio_engine::cold_residency::ResidentLoadHint;
    use crate::messages::ResidentContext;
    let (consumer, parameters) = queues;
    let (root, producer) = fixture;
    let (old, current) = probes;
    assert!(matches!(
        lifecycle,
        CandidateLifecycle::FrozenUnload
            | CandidateLifecycle::FrozenResumeReaders
            | CandidateLifecycle::FrozenPausedUnload
    ));
    assert!(consumer.is_empty() && parameters.is_empty());
    assert!(playback.callback.retirement.actual.is_some());
    assert!(playback.callback.retirement.retired.is_empty());
    assert!(!playback.scheduled && playback.seek_epoch.is_none());
    assert!(playback.selection.is_some() && playback.previous.is_some());
    assert_ne!(playback.selection, playback.previous);
    controller_python(
        locals,
        r#"
assert first.effective_mode_status() == 'rejected' and not first.is_current()
assert candidate.is_current() and candidate.claimed()
assert candidate.effective_mode_status() == 'transitioning'
assert candidate.effective_failure_reason() is None
assert project.stem_cache[0].pair == disk_v2 and project.pad_stem_mix_mode[0] == 'all_stems'
controller._assets.sync_assignments()
assert ('stems', 0) in controller._assets._assignments
assert ('original', 215) in controller._assets._assignments
frozen_saved_path = project.sample_paths[0]
frozen_saved_content = project.pad_content[0]
# Only observation/fixture aliases end here. The real Native and both actual
# callback sets remain until B freezes them and own Unload retires them.
del first, previous_control
"#,
    );
    Python::attach(|py| {
        engine.poll_loader_events(py).unwrap();
    });
    old.assert_retained();
    current.assert_retained();
    let paused_frozen = lifecycle == CandidateLifecycle::FrozenPausedUnload;
    if lifecycle == CandidateLifecycle::FrozenResumeReaders {
        synchronize_callback_frame(&mut playback.callback, playback.frame);
        let mut expected_transport = playback.callback.transport;
        let before_output = playback.frame;
        let before_source = playback.source_frame;
        public_busy_handover_pause_resume(playback, consumer, locals, old, current);
        assert_eq!(playback.frame, before_output + 513);
        assert_eq!(playback.source_frame, before_source);
        expected_transport.advance_by_rendered_frames(513);
        synchronize_callback_frame(&mut playback.callback, playback.frame);
        assert_eq!(
            format!("{:?}", playback.callback.transport),
            format!("{expected_transport:?}")
        );
    } else if paused_frozen {
        synchronize_callback_frame(&mut playback.callback, playback.frame);
        let before_transport = playback.callback.transport;
        let before_output = playback.frame;
        let before_source = playback.source_frame;
        let snapshots =
            [0, 215].map(|id| FrozenVoiceSnapshot::capture(&playback.callback.mixer, id));
        controller_python(
            locals,
            r#"
from flitzis_looper.controller.transport import TransportController
transport = TransportController(project, session, playback_audio)
assert candidate.is_current() and candidate.claimed()
assert candidate.effective_mode_status() == 'transitioning'
transport.playback.pause_pad(0)
transport.playback.pause_pad(215)
assert not session.paused_sample_ids
"#,
        );
        remember_public_pause_admissions(locals, true);
        controller_python(
            locals,
            "original_a_pause = original_pause_admissions[0][0]\noriginal_peer_pause = original_pause_admissions[215][0]",
        );
        assert_eq!(playback.drain(consumer), 2, "own current-A Pause before B");
        assert_public_pause_effect(locals);
        for snapshot in &snapshots {
            snapshot.assert_current(&playback.callback.mixer, true);
        }
        for reference in [&mut playback.reference, &mut playback.full_mix] {
            reference.pause_sample_at_output_frame(0, playback.frame);
        }
        assert_eq!(
            (playback.frame, playback.source_frame),
            (before_output, before_source)
        );
        assert_eq!(
            format!("{:?}", playback.callback.transport),
            format!("{before_transport:?}")
        );
        old.assert_retained();
        current.assert_retained();
    }
    let a = voice(&playback.callback.mixer).sample.clone().unwrap();
    let before = voice(&playback.callback.mixer);
    assert_eq!(before.paused, paused_frozen);
    assert!(before.frozen_stems.is_none());
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let adopted = before.stretch.adopted_request_id();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history().unwrap();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    assert!(transition.is_active());
    let selection = crate::audio_engine::source_reader::StemRenderSelection::for_stems(
        playback.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap(),
        0b1010,
    );
    let incoming_pcm = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| stem.samples.as_ptr() as usize);
    let outgoing_pcm = playback
        .callback
        .mixer
        .outgoing_stems_for_measurement(0)
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| stem.samples.as_ptr() as usize);
    let consumed = playback.source_frame;
    let output_frame = playback.frame;
    let feedback = playback.callback.feedback.events.len();
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_clock = other.source_playback;
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    assert_eq!(other.paused, paused_frozen);
    let restore_transport = playback.callback.transport;

    // Reuse the actual distinct-B native restore operation from the frozen-bank
    // rejection fixture. This does not change the saved public A assignment.
    let input = signal_sum_wav(4096, &[4, 5, 6, 7]);
    let expected_b: Vec<f32> = input[44..]
        .chunks_exact(2)
        .map(|bytes| f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0)
        .collect();
    let input_path = root.join("final-reader-bank-b.wav");
    fs::write(&input_path, input).unwrap();
    let material = Arc::new(prepare_material(root, &input_path, RATE, 2, &|| false).unwrap());
    assert_eq!(material.sample.samples.as_ref(), expected_b.as_slice());
    let b_original_before = fs::read(&material.lease.original_path).unwrap();
    let _original_owner = engine
        .project_assets
        .acquire(root, &material.lease.original_path)
        .unwrap();
    playback.callback.retirement.slots = 2;
    let admitted = crate::audio_engine::cold_load::admit_selected_material(
        engine,
        0,
        material.metadata()["new_reference"]
            .as_str()
            .unwrap()
            .to_owned(),
        (
            false,
            false,
            false,
            Some(ResidentLoadHint {
                start_s: 0.0,
                end_s: 4096.0 / f64::from(RATE),
                key_lock: true,
            }),
        ),
        producer.clone(),
        (2, RATE, root.to_path_buf()),
        "restore",
        Some(material.clone()),
    )
    .unwrap();
    wait_until(|| consumer.peek().is_ok() || admitted.adoption.load(Ordering::Acquire) != 0);
    assert_eq!(admitted.adoption.load(Ordering::Acquire), 0);
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::LoadColdSample {
        id: 0, replace_assignment: false, sample, ..
    }) if !sample.same_source(&a))
    );
    assert_eq!(
        playback.drain(consumer),
        0,
        "actual B restore waits for retirement capacity"
    );
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(
        playback.drain(consumer),
        1,
        "actual native B bank restore ACK"
    );
    assert!(consumer.is_empty());
    source_successes(engine, &[admitted.request_id]);
    let b = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(!b.same_source(&a) && !Arc::ptr_eq(&b.samples, &a.samples));
    assert_eq!(
        b.residency.as_ref().unwrap().context,
        ResidentContext::KeyLockFullTrack
    );
    assert_eq!(
        (b.resident_start(), b.resident_end(), b.frame_count()),
        (0, 4096, 4096)
    );
    assert_eq!(b.samples.as_ref(), expected_b.as_slice());
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(
        playback
            .callback
            .mixer
            .outgoing_stems_for_measurement(0)
            .is_none()
    );
    assert!(playback.callback.mixer.key_lock_for_measurement(0));
    let frozen = voice(&playback.callback.mixer)
        .frozen_stems
        .as_ref()
        .unwrap();
    assert_eq!(frozen.selection, selection);
    assert!(frozen.transition.matches_exact(transition) && frozen.transition.is_active());
    let assert_frozen_owners = |playback: &ControllerPlayback| {
        let voice = voice(&playback.callback.mixer);
        assert_eq!(voice.generation, generation);
        assert_eq!(voice.stretch.native_state_address(), native);
        assert_eq!(voice.stretch.adopted_request_id(), adopted);
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &a.samples
        ));
        let frozen = voice.frozen_stems.as_ref().expect("actual A frozen view");
        assert_eq!(frozen.selection, selection);
        assert!(frozen.transition.is_active());
        let incoming = frozen.set.as_ref().unwrap();
        let outgoing = frozen.outgoing.as_ref().unwrap();
        assert!(
            current
                .identity
                .ptr_eq(&Arc::downgrade(&incoming.complete_set_identity))
        );
        assert!(
            old.identity
                .ptr_eq(&Arc::downgrade(&outgoing.complete_set_identity))
        );
        assert_eq!(
            incoming
                .stems
                .each_ref()
                .map(|stem| stem.samples.as_ptr() as usize),
            incoming_pcm
        );
        assert_eq!(
            outgoing
                .stems
                .each_ref()
                .map(|stem| stem.samples.as_ptr() as usize),
            outgoing_pcm
        );
        assert!(
            incoming
                .stems
                .iter()
                .chain(&outgoing.stems)
                .all(|stem| stem.same_window(&a))
        );
        old.assert_retained();
        current.assert_retained();
    };
    assert_frozen_owners(playback);
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    let after_history = after.stretch.productive_history().unwrap();
    assert_eq!(after_history.binding, history.binding);
    assert_eq!(after_history.next_position, history.next_position);
    assert_eq!(after_history.fed_output_frames, history.fed_output_frames);
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert_eq!(
        (playback.frame, playback.source_frame),
        (output_frame, consumed)
    );
    assert_eq!(playback.callback.feedback.events.len(), feedback);
    assert_eq!(
        format!("{:?}", playback.callback.transport),
        format!("{restore_transport:?}")
    );
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    controller_python(
        locals,
        r#"
assert candidate.claimed() and not candidate.is_current()
assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'rejected'
assert candidate.effective_failure_reason() is None
assert project.sample_paths[0] == frozen_saved_path and project.pad_content[0] == frozen_saved_content
assert project.stem_cache[0].pair == disk_v2 and project.pad_stem_mix_mode[0] == 'all_stems'
assert ('stems', 0) in controller._assets._assignments
"#,
    );
    // The bank's former V1/V2 slot copies have really left the callback and
    // worker. Only actual frozen readers and used Native/history retain them.
    let marker = Arc::new(AtomicU8::new(0));
    let completion = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    old.wait_for("B restore prior bank-owner FIFO retirement", || {
        completion.strong_count() == 0
    });
    assert!(playback.callback.retirement.retired.is_empty());
    assert_frozen_owners(playback);

    if paused_frozen {
        paused_frozen_source_readers_before_unload(
            engine,
            playback,
            consumer,
            locals,
            (old, current),
        );
        public_unload_reader_pairs(
            engine,
            playback,
            (consumer, parameters),
            locals,
            &[old, current],
            None,
            true,
        );
        assert_eq!(
            fs::read(&material.lease.original_path).unwrap(),
            b_original_before
        );
        return;
    }

    // Seventeen productive frames exercise FrozenStemView's set AND outgoing
    // through the independent complete A PCM/raw Native/EQ oracle. The physical
    // source clock advances by12.75frames, leaving the128-source-frame ramp busy.
    let (actual, full_mix) = playback.render(17, false);
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    assert_ne!(actual, full_mix);
    assert_eq!(playback.source_frame, consumed + 17);
    assert_eq!(playback.frame, output_frame + 17);
    assert_frozen_owners(playback);
    let phase = (playback.source_frame as f64 * playback.ratio).rem_euclid(1500.0);
    let position = voice(&playback.callback.mixer).source_playback.position();
    assert_eq!(position.frame, LOOP_START + phase.floor() as usize);
    assert_eq!(position.fraction.to_bits(), phase.fract().to_bits());
    assert_eq!(
        voice(&playback.callback.mixer)
            .source_playback
            .loop_period(),
        None
    );
    let other = other_voice(&playback.callback.mixer);
    assert_eq!(
        other.source_playback.position().frame,
        LOOP_START + phase.floor() as usize
    );
    assert_eq!(
        other.source_playback.position().fraction.to_bits(),
        phase.fract().to_bits()
    );
    assert_eq!(other.generation, other_generation);
    assert_eq!(other.stretch.native_state_address(), other_native);
    assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
    assert!(Arc::ptr_eq(
        &other.sample.as_ref().unwrap().samples,
        &other_source.samples
    ));
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(215),
        other_demand
    );
    assert!(playback.callback.mixer.stems_for_measurement()[215].is_none());
    controller_python(
        locals,
        r#"
assert project.sample_paths[0] == frozen_saved_path and project.pad_content[0] == frozen_saved_content
assert project.stem_cache[0].pair == disk_v2
assert ('stems', 0) in controller._assets._assignments
assert not session.stem_generation_errors and not session.sample_load_errors, (
    session.stem_generation_errors, session.sample_load_errors, controller.stem_residency_status(0))
"#,
    );
    if lifecycle == CandidateLifecycle::FrozenResumeReaders {
        progressive_frozen_source_readers(engine, playback, consumer, locals, (old, current));
        public_unload_reader_pairs(
            engine,
            playback,
            (consumer, parameters),
            locals,
            &[current],
            None,
            true,
        );
        assert_eq!(old.identity.strong_count(), 0);
        assert!(old.pcm.iter().all(|pcm| pcm.strong_count() == 0));
        assert!(old.areas.iter().all(|path| !path.exists()));
    } else {
        public_unload_reader_pairs(
            engine,
            playback,
            (consumer, parameters),
            locals,
            &[old, current],
            None,
            true,
        );
    }
    assert_eq!(
        fs::read(&material.lease.original_path).unwrap(),
        b_original_before
    );
}

fn progressive_frozen_source_readers(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    probes: (&FinalReaderProbe, &FinalReaderProbe),
) {
    let (old, current) = probes;
    let before = voice(&playback.callback.mixer);
    // These aliases contain original PCM only. The pair probes are Weak and the
    // independent stem oracle owns deep copies, so neither can pin an old reader.
    let a = before.sample.clone().unwrap();
    let b = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(!a.same_source(&b));
    assert_eq!(
        (a.resident_start(), a.resident_end()),
        (LOOP_START, LOOP_END)
    );
    assert_eq!((b.resident_start(), b.resident_end()), (0, 4096));
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let adopted = before.stretch.adopted_request_id().unwrap();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history().unwrap();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let frozen = before.frozen_stems.as_ref().unwrap();
    let transition = frozen.transition;
    assert!(transition.is_active());
    let selection = frozen.selection;
    let incoming_pcm = frozen
        .set
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| stem.samples.as_ptr() as usize);
    let outgoing_pcm = frozen
        .outgoing
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| stem.samples.as_ptr() as usize);
    let output_frame = playback.frame;
    let consumed = playback.source_frame;
    let feedback = playback.callback.feedback.events.len();
    let entry_gap = output_frame - consumed;
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_clock = other.source_playback;
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let assert_frozen = |playback: &ControllerPlayback, outgoing: bool| {
        let voice = voice(&playback.callback.mixer);
        assert_eq!(voice.generation, generation);
        assert!(voice.sample.as_ref().unwrap().same_window(&a));
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &a.samples
        ));
        let bank = playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(bank.same_window(&b) && !bank.same_source(&a));
        assert!(Arc::ptr_eq(&bank.samples, &b.samples));
        assert_eq!(
            bank.residency.as_ref().unwrap().context,
            crate::messages::ResidentContext::KeyLockFullTrack
        );
        assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
        assert!(
            playback
                .callback
                .mixer
                .outgoing_stems_for_measurement(0)
                .is_none()
        );
        let frozen = voice.frozen_stems.as_ref().unwrap();
        assert_eq!(frozen.selection, selection);
        let incoming = frozen.set.as_ref().unwrap();
        assert!(
            current
                .identity
                .ptr_eq(&Arc::downgrade(&incoming.complete_set_identity))
        );
        assert_eq!(
            incoming
                .stems
                .each_ref()
                .map(|stem| stem.samples.as_ptr() as usize),
            incoming_pcm
        );
        assert!(incoming.stems.iter().all(|stem| stem.same_window(&a)));
        if outgoing {
            let outgoing = frozen.outgoing.as_ref().unwrap();
            assert!(
                old.identity
                    .ptr_eq(&Arc::downgrade(&outgoing.complete_set_identity))
            );
            assert_eq!(
                outgoing
                    .stems
                    .each_ref()
                    .map(|stem| stem.samples.as_ptr() as usize),
                outgoing_pcm
            );
            assert!(outgoing.stems.iter().all(|stem| stem.same_window(&a)));
        } else {
            assert!(frozen.outgoing.is_none() && !frozen.transition.is_active());
        }
        // Both pads consume the same declared physical trajectory. The retained
        // pause gap advances output time alone; B never supplies A's source clock.
        let phase = (playback.source_frame as f64 * playback.ratio).rem_euclid(1500.0);
        for source in [voice, other_voice(&playback.callback.mixer)] {
            let position = source.source_playback.position();
            assert_eq!(position.frame, LOOP_START + phase.floor() as usize);
            assert_eq!(position.fraction.to_bits(), phase.fract().to_bits());
            assert_eq!(source.source_playback.loop_period(), None);
            assert_eq!(
                source.source_playback.tempo_ratio().to_bits(),
                playback.ratio.to_bits()
            );
        }
        let raw = playback.raw.as_ref().unwrap();
        assert_eq!(
            voice.stretch.pending_fifo_frames(),
            (raw.input[0].len(), raw.output[0].len()),
            "retained-A independent Native FIFO chronology"
        );
        let other = other_voice(&playback.callback.mixer);
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
        assert!(playback.callback.mixer.stems_for_measurement()[215].is_none());
        current.assert_retained();
    };
    assert_frozen(playback, true);
    controller_python(
        locals,
        r#"
assert candidate.claimed() and not candidate.is_current()
assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'rejected'
assert candidate.effective_failure_reason() is None
"#,
    );
    // A's genuine Pause/Resume already completed before B replaced the current
    // content lifetime. B's own command cannot borrow that retained A cohort.
    assert!(
        playback
            .resumed_at
            .is_some_and(|frame| frame <= output_frame)
    );
    assert_current_bank_transport_refused(engine, playback, consumer, locals, false);
    assert_frozen(playback, true);
    old.assert_retained();
    let resumed = voice(&playback.callback.mixer);
    assert!(!resumed.paused && resumed.source_playback.matches_exact(&clock));
    assert_eq!(resumed.stretch.native_state_address(), native);
    assert_eq!(resumed.stretch.adopted_request_id(), Some(adopted));
    assert_eq!(resumed.stretch.pending_fifo_frames(), fifo);
    let resumed_history = resumed.stretch.productive_history().unwrap();
    assert_eq!(resumed_history.binding, history.binding);
    assert_eq!(resumed_history.next_position, history.next_position);
    assert_eq!(resumed_history.fed_output_frames, history.fed_output_frames);
    let resumed_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(resumed_dsp.binding, dsp.binding);
    assert_eq!(resumed_dsp.next_position, dsp.next_position);
    assert_eq!(resumed_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        resumed
            .frozen_stems
            .as_ref()
            .unwrap()
            .transition
            .matches_exact(transition)
    );
    let other = other_voice(&playback.callback.mixer);
    assert!(!other.paused && other.source_playback.matches_exact(&other_clock));
    assert_eq!(playback.source_frame, consumed);
    assert_eq!(playback.callback.feedback.events.len(), feedback + 1);

    // Actual consumed source distance ends the frozen ramp. Saturation prevents
    // the callback from transferring its real outgoing reader until a later pass.
    playback.callback.retirement.slots = 0;
    let mut remaining = (1..=256)
        .find(|frames| {
            (playback.source_frame + *frames as u64 - playback.ramp_start) as f64 * playback.ratio
                >= 128.0
        })
        .unwrap();
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let frames = frames.min(remaining);
        playback.render(frames, false);
        remaining -= frames;
        assert_frozen(playback, true);
        old.assert_retained();
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert_eq!(
            voice(&playback.callback.mixer).stretch.adopted_request_id(),
            Some(adopted)
        );
        assert_eq!(
            voice(&playback.callback.mixer)
                .frozen_stems
                .as_ref()
                .unwrap()
                .transition
                .is_active(),
            (playback.source_frame - playback.ramp_start) as f64 * playback.ratio < 128.0
        );
        assert!(playback.callback.retirement.retired.is_empty());
        if remaining == 0 {
            break;
        }
    }
    let held_counts = old.pcm.each_ref().map(std::sync::Weak::strong_count);
    playback.callback.retirement.slots = usize::MAX;
    playback.render(1, false);
    assert_frozen(playback, false);
    let marker = Arc::new(AtomicU8::new(0));
    let completion = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    old.wait_for("frozen outgoing production FIFO completion", || {
        completion.strong_count() == 0
    });
    old.wait_for("frozen outgoing actual worker transfer", || {
        old.pcm
            .iter()
            .zip(held_counts)
            .all(|(pcm, held)| pcm.strong_count() < held)
    });
    assert!(playback.callback.retirement.retired.is_empty());
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    assert_eq!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        Some(adopted)
    );
    assert_frozen(playback, false);
    // Outgoing callback/FIFO owners are gone. The used V1 Native/history remains
    // the genuine final reader and still protects all four PCM files and identity.
    old.assert_retained();
    assert!(fs::write(old.areas[1].join("vocals.f32le"), b"leased overwrite").is_err());
    controller_python(
        locals,
        r#"
assert candidate.claimed() and not candidate.is_current()
assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'rejected'
assert candidate.effective_failure_reason() is None
assert project.sample_paths[0] == frozen_saved_path and project.pad_content[0] == frozen_saved_content
assert project.stem_cache[0].pair == disk_v2 and ('stems', 0) in controller._assets._assignments
"#,
    );

    // The first timestamped settled frozen feed captures RetainedVoice authority
    // for finite A, while the actual current bank is still distinct complete B.
    let after_ramp = playback.frame;
    playback.render(13, true);
    wait_until(|| {
        voice_mut(&mut playback.callback.mixer)
            .stretch
            .source_preparation_ready()
    });
    let target = voice_mut(&mut playback.callback.mixer)
        .stretch
        .prepared_target_output_frame()
        .unwrap();
    let warm_start =
        target - crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES as u64;
    assert!(warm_start >= after_ramp && warm_start >= playback.resumed_at.unwrap());
    let prepared = voice_mut(&mut playback.callback.mixer)
        .stretch
        .prepared_native_address();
    assert_ne!(prepared, 0);
    assert_ne!(prepared, native);
    let (actual, full_mix) = playback.adopt_native(false);
    assert!(actual.iter().any(|value| value.abs() > 0.001));
    assert!(
        actual
            .iter()
            .zip(full_mix)
            .any(|(actual, full)| (actual - full).abs() > 0.005)
    );
    let adopted_voice = voice(&playback.callback.mixer);
    assert_eq!(adopted_voice.stretch.native_state_address(), prepared);
    assert!(
        adopted_voice
            .stretch
            .adopted_request_id()
            .is_some_and(|request| request > adopted)
    );
    assert!(adopted_voice.stretch.wet_current_source_output_ready());
    let taps = adopted_voice.stretch.native_tap_observation().unwrap();
    assert_eq!(taps.missing_reads, 0);
    assert!(taps.min_frame.is_some_and(|frame| frame >= LOOP_START));
    assert!(taps.max_frame.is_some_and(|frame| frame < LOOP_END));
    let adopted_history = adopted_voice.stretch.productive_history().unwrap();
    assert_eq!(adopted_history.binding, history.binding);
    assert_eq!(adopted_history.binding.source_address, a.source_address());
    assert_ne!(adopted_history.binding.source_address, b.source_address());
    assert_frozen(playback, false);
    old.wait_for("retained-A actual final V1 Native/history readers", || {
        playback.render(31, false);
        assert_frozen(playback, false);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            prepared
        );
        Python::attach(|py| engine.poll_loader_events(py).unwrap());
        old.identity.strong_count() == 0 && old.pcm.iter().all(|pcm| pcm.strong_count() == 0)
    });
    old.wait_for(
        "retained-A V1 asset worker three-area cleanup with V2 live",
        || {
            playback.render(137, false);
            assert_frozen(playback, false);
            assert_eq!(
                voice(&playback.callback.mixer)
                    .stretch
                    .native_state_address(),
                prepared
            );
            Python::attach(|py| engine.poll_loader_events(py).unwrap());
            old.areas.iter().all(|path| !path.exists())
        },
    );
    for frames in [1, 137, 511, 257] {
        playback.render(frames, false);
        assert_frozen(playback, false);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            prepared
        );
    }
    assert_eq!(playback.frame - playback.source_frame, entry_gap);
    assert!(consumer.is_empty() && playback.callback.retirement.retired.is_empty());
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert candidate.claimed() and not candidate.is_current()
assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'rejected'
assert candidate.effective_failure_reason() is None
assert project.sample_paths[0] == frozen_saved_path and project.pad_content[0] == frozen_saved_content
assert project.stem_cache[0].pair == disk_v2 and project.pad_stem_mix_mode[0] == 'all_stems'
assert ('stems', 0) in controller._assets._assignments and ('original', 215) in controller._assets._assignments
assert not session.stem_generation_errors and not session.sample_load_errors, (
    session.stem_generation_errors, session.sample_load_errors, controller.stem_residency_status(0))
"#,
    );
}

fn public_unload_effective_pair(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    current: &FinalReaderProbe,
    preserved: Option<&FinalReaderProbe>,
) {
    public_unload_reader_pairs(
        engine,
        playback,
        (consumer, parameters),
        locals,
        &[current],
        preserved,
        false,
    );
}

fn public_unload_reader_pairs(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    queues: (
        &mut rtrb::Consumer<ControlMessage>,
        &mut rtrb::Consumer<ControlParameterMessage>,
    ),
    locals: &Py<PyDict>,
    retiring: &[&FinalReaderProbe],
    preserved: Option<&FinalReaderProbe>,
    frozen_unload: bool,
) {
    let (consumer, parameters) = queues;
    let current = retiring.last().unwrap();
    assert!(playback.callback.retirement.actual.is_some());
    assert!(playback.callback.retirement.retired.is_empty());
    for probe in retiring {
        probe.assert_retained();
    }
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("preserve_saved_copy", preserved.is_some())
            .unwrap();
        locals
            .bind(py)
            .set_item("frozen_unload", frozen_unload)
            .unwrap();
    });
    let pause_gap = playback.frame - playback.source_frame;
    let frozen_source_frame = playback.source_frame;
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let other_path = Python::attach(|py| {
        locals
            .bind(py)
            .get_item("project")
            .unwrap()
            .unwrap()
            .getattr("sample_paths")
            .unwrap()
            .get_item(215)
            .unwrap()
            .extract::<String>()
            .unwrap()
    });
    prepare_public_unload_loader(locals);
    controller_python(
        locals,
        r#"
effective_unload_workers = audio.worker_call_count()
effective_other_path = project.sample_paths[215]
effective_other_content = project.pad_content[215]
assert effective_other_path is not None and effective_other_content is not None
assert candidate.claimed()
if frozen_unload:
    assert not candidate.is_current() and candidate.residency_status() == 'accepted'
    assert candidate.effective_mode_status() == 'rejected'
    assert candidate.error() == 'Committed stem residency or mode ownership changed'
    assert candidate.effective_failure_reason() is None
    assert project.stem_cache[0].pair == disk_v2
    assert project.pad_stem_mix_mode[0] == 'all_stems'
    assert ('stems', 0) in controller._assets._assignments
else:
    assert candidate.is_current() and candidate.effective_mode_status() == 'effective'
loader.unload_sample(0)
assert project.sample_paths[0] is None and project.pad_content[0] is None
assert project.stem_cache[0] is None and not controller._committed_residency.contains(0)
assert project.pad_stem_mix_mode[0] == 'full_mix' and project.pad_key_lock[0] is None
assert 0 not in session.active_sample_ids and 215 in session.active_sample_ids
assert not candidate.is_current()
assert controller.stem_residency_status(0) is None
assert ('stems', 0) not in controller._assets._assignments
if preserve_saved_copy:
    assert project.stem_cache[copy_id] is copy_entry and copy_entry.pair == disk_v1
    assert project.pad_content[copy_id] == copy_content
    assert project.pad_stem_mix_mode[copy_id] == 'all_stems'
    assert ('stems', copy_id) in controller._assets._assignments
assert unload_notifications == [0]
owner_v2.release()
del prepared_v2, candidate, live
"#,
    );
    if let Some(old) = preserved {
        old.assert_files_retained();
    }
    // Public saved custody and fixture pair/receipt aliases have ended. The
    // actual callback/frozen/Native readers still precede their own Unload ACK.
    for probe in retiring {
        probe.assert_retained();
    }
    let before = playback.callback.feedback.events.len();
    let stopped_feedback =
        ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, 0);
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    assert_eq!(
        playback.drain(consumer),
        4,
        "A own Unload then pair/loop/keylock defaults"
    );
    assert!(consumer.is_empty());
    assert!(playback.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(
        !playback
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert_eq!(playback.callback.feedback.events.len(), before + 1);
    stopped_feedback.assert_message(&playback.callback.feedback.events[before]);
    let assert_other = |playback: &ControllerPlayback| {
        let other = other_voice(&playback.callback.mixer);
        let phase = ((playback.frame - pause_gap) as f64 * playback.ratio).rem_euclid(1500.0);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
    };
    assert_other(playback);
    // The real outgoing/Native/history retirement precedes this marker in
    // the production FIFO; Weak=0 is observed only after worker completion.
    let marker = Arc::new(AtomicU8::new(0));
    let completion = Arc::downgrade(&marker);
    playback.callback.retirement.retire_cold_adoption(marker);
    let mut render_inactive = |frames: usize| {
        let mut output = vec![0.0; frames * 2];
        playback.callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            &mut playback.callback.retirement,
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        playback.frame += frames as u64;
        assert_other(playback);
        assert_eq!(playback.source_frame, frozen_source_frame);
        controller_python(
            locals,
            "loader.poll_loader_events()\ncontroller.on_frame_render()",
        );
        if let Some(old) = preserved {
            old.assert_files_retained();
        }
    };
    for frames in [1, 31, 137, 257, 511] {
        render_inactive(frames);
    }
    current.wait_for("effective Unload audio FIFO worker completion", || {
        completion.strong_count() == 0
    });
    current.wait_for("effective V2 Unload final Native/history readers", || {
        render_inactive(137);
        retiring.iter().all(|probe| {
            probe.identity.strong_count() == 0
                && probe.pcm.iter().all(|pcm| pcm.strong_count() == 0)
        })
    });
    current.wait_for("effective pair Unload asset worker cleanup", || {
        if let Some(old) = preserved {
            old.assert_files_retained();
        }
        retiring
            .iter()
            .all(|probe| probe.areas.iter().all(|path| !path.exists()))
    });
    controller_python(
        locals,
        r#"
persistence.flush()
effective_unload_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert effective_unload_reopened.load_error is None
assert effective_unload_reopened.project.sample_paths[0] is None
assert effective_unload_reopened.project.pad_content[0] is None
assert effective_unload_reopened.project.stem_cache[0] is None
assert effective_unload_reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert effective_unload_reopened.project.pad_key_lock[0] is None
assert effective_unload_reopened.project.sample_paths[215] == effective_other_path
assert effective_unload_reopened.project.pad_content[215] == effective_other_content
assert audio.worker_call_count() == effective_unload_workers
assert not session.stem_generation_errors and not session.sample_load_errors
assert not controller._assets.retirement_errors
"#,
    );
    assert_eq!(playback.source_frame, frozen_source_frame);
    for probe in retiring {
        assert_eq!(probe.identity.strong_count(), 0);
        assert!(probe.pcm.iter().all(|pcm| pcm.strong_count() == 0));
        assert!(probe.areas.iter().all(|path| !path.exists()));
    }
    assert!(playback.callback.retirement.retired.is_empty());
    assert!(consumer.is_empty());
    assert_other(playback);
    Python::attach(|py| {
        engine.poll_loader_events(py).unwrap();
        assert_eq!(
            locals
                .bind(py)
                .get_item("project")
                .unwrap()
                .unwrap()
                .getattr("sample_paths")
                .unwrap()
                .get_item(215)
                .unwrap()
                .extract::<String>()
                .unwrap(),
            other_path
        );
    });
}

fn saved_copy_after_origin_readers(
    engine: &AudioEngine,
    root: &Path,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    probes: (&FinalReaderProbe, &FinalReaderProbe),
) {
    let (old, current) = probes;
    assert_eq!(old.identity.strong_count(), 0);
    assert!(old.pcm.iter().all(|pcm| pcm.strong_count() == 0));
    old.assert_files_retained();
    current.assert_retained();
    controller_python(
        locals,
        r#"
assert project.stem_cache[copy_id] is copy_entry and copy_entry.pair == disk_v1
assert project.pad_content[copy_id] == copy_content
assert project.pad_stem_mix_mode[copy_id] == 'all_stems'
assert not controller._committed_residency.contains(copy_id)
assert project.stem_cache[0].pair == disk_v2
assert controller.stem_residency_status(0).effective_stem_set_identity == disk_v2.stem_set_identity
"#,
    );
    // An unrelated real retirement completes in this same shared asset service
    // while C alone retains V1 files; delayed worker startup cannot fake custody.
    let sentinel = root.join("materials/M55555555555555555555555555555555/original/sentinel.wav");
    fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
    assert!(!sentinel.exists());
    assert!(Arc::ptr_eq(
        &engine.project_assets,
        &crate::audio_engine::project_assets::ProjectAssets::shared()
    ));
    fs::write(&sentinel, stereo_wav(64)).unwrap();
    let mut pin = engine.project_assets.acquire_pin(root, &sentinel).unwrap();
    engine
        .project_assets
        .retire(root, &sentinel, false)
        .unwrap();
    pin.release();
    drop(pin);
    old.wait_for(
        "asset worker progress while saved C holds predecessor",
        || {
            old.assert_files_retained();
            !sentinel.exists()
        },
    );
    old.assert_files_retained();
    public_unload_effective_pair(
        engine,
        playback,
        consumer,
        parameters,
        locals,
        current,
        Some(old),
    );
    let after_unload = other_voice(&playback.callback.mixer).source_playback;
    controller_python(
        locals,
        r#"
persistence.flush()
copy_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert copy_reopened.load_error is None
assert copy_reopened.project.sample_paths[0] is None and copy_reopened.project.pad_content[0] is None
assert copy_reopened.project.stem_cache[0] is None
assert copy_reopened.project.pad_content[copy_id] == copy_content
assert copy_reopened.project.stem_cache[copy_id].pair == disk_v1
assert copy_reopened.project.pad_stem_mix_mode[copy_id] == 'all_stems'
assert copy_reopened.project.sample_paths[copy_id] == project.sample_paths[copy_id]
assert copy_reopened.project.pad_content[215] == project.pad_content[215]
assert copy_reopened.project.sample_paths[215] == project.sample_paths[215]
assert audio.worker_call_count() == effective_unload_workers
assert not session.stem_generation_errors and not session.sample_load_errors
assert controller.delete_stems(copy_id)
assert project.stem_cache[copy_id] is None and project.pad_stem_mix_mode[copy_id] == 'full_mix'
assert ('stems', copy_id) not in controller._assets._assignments
assert ('original', copy_id) in controller._assets._assignments
persistence.flush()
copy_deleted_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert copy_deleted_reopened.load_error is None
assert copy_deleted_reopened.project.stem_cache[copy_id] is None
assert copy_deleted_reopened.project.sample_paths[copy_id] == project.sample_paths[copy_id]
assert copy_deleted_reopened.project.pad_content[copy_id] == copy_content
"#,
    );
    assert_eq!(
        playback.drain(consumer),
        1,
        "C explicit own FullMix deletion"
    );
    assert!(consumer.is_empty());
    old.wait_for(
        "saved C final assignment release and paired asset cleanup",
        || {
            controller_python(
                locals,
                "loader.poll_loader_events()\ncontroller.on_frame_render()",
            );
            old.areas.iter().all(|path| !path.exists())
        },
    );
    assert_eq!(old.identity.strong_count(), 0);
    assert!(old.pcm.iter().all(|pcm| pcm.strong_count() == 0));
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&after_unload)
    );
    assert!(playback.callback.retirement.retired.is_empty());
    controller_python(
        locals,
        "assert not controller._assets.retirement_errors\nassert not session.stem_generation_errors\nassert project.sample_paths[215] == effective_other_path",
    );
}

fn public_unload_queued_candidate(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
    cleanup: Option<(&Path, &FinalReaderProbe)>,
) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert_eq!(playback.drain(consumer), 1, "own V2 Capture before Unload");
    wait_until(|| consumer.peek().is_ok());
    let queued = match consumer.peek().unwrap() {
        ControlMessage::CommittedStemWork(work) => {
            assert!(
                matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
                if transaction.id == 0 && transaction.candidate.is_some())
            );
            work.as_ref() as *const CommittedStemRtWork as usize
        }
        _ => panic!("expected actual queued V2 Adopt"),
    };
    let pending = cleanup.map(|(root, old)| {
        let selection: Value = Python::attach(|py| {
            serde_json::from_str(
                &locals
                    .bind(py)
                    .get_item("candidate_json")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
            )
            .unwrap()
        });
        let Ok(ControlMessage::CommittedStemWork(work)) = consumer.peek() else {
            unreachable!()
        };
        let CommittedStemRtWork::Adopt(transaction) = work.as_ref() else {
            unreachable!()
        };
        let pending = FinalReaderProbe::new(root, &selection, transaction.stems.as_ref().unwrap());
        assert!(playback.callback.retirement.actual.is_some());
        assert!(playback.callback.retirement.retired.is_empty());
        old.assert_retained();
        pending.assert_retained();
        pending
    });
    playback.callback.retirement.slots = 2;
    assert_eq!(playback.drain(consumer), 0);
    let bank = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let stems = playback.callback.mixer.stems_for_measurement()[0].clone();
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    let pause_gap = playback.frame - playback.source_frame;
    let other_ratio = playback.ratio;
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    prepare_public_unload_loader(locals);
    controller_python(
        locals,
        r#"
unload_candidate = candidate
unload_registration = live.disk.selection.ticket
unload_workers = audio.worker_call_count()
unload_other_path = project.sample_paths[215]
unload_other_content = project.pad_content[215]
assert candidate.is_current() and not candidate.claimed()
loader.unload_sample(0)
assert not candidate.is_current() and not candidate.claimed()
assert candidate.residency_status() == 'rejected' and candidate.effective_mode_status() == 'rejected'
assert candidate.effective_failure_reason() is None
assert candidate.error() == 'Committed stem residency or mode ownership changed'
assert not unload_registration.is_current()
assert not source_v2.same_preparation_context(source_v2)
assert project.sample_paths[0] is None and project.pad_content[0] is None
assert project.stem_cache[0] is None and project.pad_stem_mix_mode[0] == 'full_mix'
assert project.pad_key_lock[0] is None
assert 0 not in session.active_sample_ids and 215 in session.active_sample_ids
assert not controller._committed_residency.contains(0) and controller.stem_residency_status(0) is None
assert not session.stem_generation_errors and not session.sample_load_errors
assert unload_notifications == [0]
"#,
    );
    if let Some((_, old)) = cleanup {
        // End fixture-only pair/receipt/assignment aliases before the queued
        // Adopt and own Unload run. Genuine callback/Native owners remain.
        controller_python(
            locals,
            "owner_v2.release()\ndel prepared_v2, first, previous_control, live",
        );
        old.assert_retained();
        pending.as_ref().unwrap().assert_retained();
        assert!(fs::write(old.areas[1].join("vocals.f32le"), b"leased overwrite").is_err());
        assert!(
            fs::write(
                pending.as_ref().unwrap().areas[1].join("vocals.f32le"),
                b"queued overwrite"
            )
            .is_err()
        );
    }
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if work.as_ref() as *const CommittedStemRtWork as usize == queued)
    );
    // Unload publishes genuine parameter defaults before callback command drain.
    // The independent filters receive declared neutral targets without reset.
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    // Public Unload restores Legacy(None) through the parameter queue before
    // its held callback command. Mirror that declared reset in the independent
    // complete-PCM references; the used Native and its uninterrupted EQ remain.
    if let Some(projection) = voice(&playback.reference).source_timing.accepted {
        let fallback_speed = projection.period_seconds * (playback.target_bpm / 60.0);
        assert_eq!(
            fallback_speed.to_bits(),
            playback.ratio.to_bits(),
            "these declared87.6 BPM cases keep the physical fallback rate exactly"
        );
        playback.loop_period = (playback.loop_end - playback.loop_start) as f64;
    }
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        reference.set_pad_bpm(0, None);
        reference.clear_constant_timing(0, u64::MAX);
        reference.set_pad_gain(0, 0.0);
        reference.set_pad_eq(0, 0.0, 0.0, 0.0);
    }
    for slot in [
        crate::audio_engine::dsp::DspParameterSlot::Slot0,
        crate::audio_engine::dsp::DspParameterSlot::Slot1,
        crate::audio_engine::dsp::DspParameterSlot::Slot2,
    ] {
        assert!(playback.filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            0.5
        ));
    }
    let assert_current = |playback: &ControllerPlayback| {
        let current = voice(&playback.callback.mixer);
        assert_eq!(current.generation, generation);
        assert_eq!(current.stretch.native_state_address(), native);
        assert!(current.sample.as_ref().unwrap().same_window(&bank));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0),
            demand
        );
        match (&stems, &playback.callback.mixer.stems_for_measurement()[0]) {
            (Some(before), Some(after)) => {
                assert!(Arc::ptr_eq(
                    &before.complete_set_identity,
                    &after.complete_set_identity
                ));
                for (before, after) in before.stems.iter().zip(&after.stems) {
                    assert!(Arc::ptr_eq(&before.samples, &after.samples));
                }
            }
            (None, None) => {}
            _ => panic!("Unload admission cannot change current stem readers"),
        }
    };
    let assert_other = |playback: &ControllerPlayback| {
        let other = other_voice(&playback.callback.mixer);
        let phase = ((playback.frame - pause_gap) as f64 * other_ratio).rem_euclid(1500.0);
        assert_eq!(
            other.source_playback.tempo_ratio().to_bits(),
            other_ratio.to_bits()
        );
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
    };
    for frames in [1, 31, 137, 257] {
        if key_lock {
            playback.render(frames, true);
        } else {
            render_varispeed(playback, frames);
        }
        assert_current(playback);
        assert_eq!(voice(&playback.callback.mixer).source_timing.accepted, None);
        assert_eq!(
            voice(&playback.callback.mixer)
                .source_playback
                .loop_period(),
            None
        );
        assert_other(playback);
        if let Some((_, old)) = cleanup {
            old.assert_retained();
            pending.as_ref().unwrap().assert_retained();
        }
        controller_python(
            locals,
            "loader.poll_loader_events()\ncontroller.on_frame_render()\nassert not session.stem_generation_errors\nassert not unload_candidate.claimed()\nassert audio.worker_call_count() == unload_workers",
        );
        assert_eq!(playback.drain(consumer), 0);
    }
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let feedback_count = playback.callback.feedback.events.len();
    playback.callback.retirement.slots = 3;
    assert_eq!(
        playback.drain(consumer),
        1,
        "reject late V2 while own Unload remains held"
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::UnloadSample { id: 0, .. })
    ));
    assert_current(playback);
    if let Some((_, old)) = cleanup {
        old.assert_retained();
    }
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let after = after.stretch.productive_history().unwrap();
        assert_eq!(after.binding, history.binding);
        assert_eq!(after.next_position, history.next_position);
        assert_eq!(after.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(after.stretch.productive_history().is_none());
    }
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert_eq!(playback.callback.feedback.events.len(), feedback_count);
    for frames in [31, 257] {
        if key_lock {
            playback.render(frames, true);
        } else {
            render_varispeed(playback, frames);
        }
        assert_current(playback);
        assert_other(playback);
        assert_eq!(playback.drain(consumer), 0);
        if let Some((_, old)) = cleanup {
            old.assert_retained();
        }
    }
    // This is only an observation alias. Remove it while the genuine V1 bank,
    // voice and Native/history still exist, before own Unload's callback ACK.
    drop(stems);
    playback.callback.retirement.slots = usize::MAX;
    let stopped_feedback =
        ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, 0);
    assert_eq!(
        playback.drain(consumer),
        4,
        "own Unload then pair/loop/keylock defaults"
    );
    assert!(consumer.is_empty());
    assert!(playback.callback.mixer.bank_for_measurement()[0].is_none());
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert!(
        !playback
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert_eq!(playback.callback.feedback.events.len(), feedback_count + 1);
    stopped_feedback.assert_message(&playback.callback.feedback.events[feedback_count]);
    assert_other(playback);
    let frozen_source_frame = playback.source_frame;
    for frames in [1, 31, 137, 257, 511] {
        let mut output = vec![0.0; frames * 2];
        playback.callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            &mut playback.callback.retirement,
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        playback.frame += frames as u64;
        assert_eq!(playback.source_frame, frozen_source_frame);
        assert_other(playback);
        controller_python(
            locals,
            "loader.poll_loader_events()\ncontroller.on_frame_render()\nassert not unload_candidate.claimed()\nassert unload_candidate.residency_status() == 'rejected'\nassert controller.stem_residency_status(0) is None\nassert project.sample_paths[0] is None\nassert project.stem_cache[0] is None\nassert not session.stem_generation_errors",
        );
        assert!(consumer.is_empty());
    }
    controller_python(
        locals,
        r#"
persistence.flush()
unload_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert unload_reopened.load_error is None
assert unload_reopened.project.sample_paths[0] is None and unload_reopened.project.pad_content[0] is None
assert unload_reopened.project.stem_cache[0] is None and unload_reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert unload_reopened.project.pad_key_lock[0] is None
assert unload_reopened.project.sample_paths[215] == unload_other_path
assert unload_reopened.project.pad_content[215] == unload_other_content
assert audio.worker_call_count() == unload_workers
assert not session.stem_generation_errors and not session.sample_load_errors
assert not unload_candidate.claimed() and not unload_candidate.is_current()
assert unload_candidate.effective_failure_reason() is None
"#,
    );
    if let Some((_, old)) = cleanup {
        let pending = pending.as_ref().unwrap();
        // Status observation tickets also retain SelectionOwner identities;
        // their final assertions above are complete, so end those test aliases.
        controller_python(
            locals,
            "del candidate, unload_candidate, unload_registration",
        );
        // Same production FIFO marker follows the actual rejected Adopt and
        // Unload transfers, distinguishing consumption from a mere Arc decrease.
        let marker = Arc::new(AtomicU8::new(0));
        let completion = Arc::downgrade(&marker);
        playback.callback.retirement.retire_cold_adoption(marker);
        old.wait_for("Unload audio FIFO worker completion", || {
            completion.strong_count() == 0
        });
        old.wait_for(
            "Unload actual final Native/history/complete readers",
            || {
                // An in-flight Native preparation may finish after Unload. The real
                // inactive render path recycles it; regular loader polling reconciles.
                let frames = 137;
                let mut output = vec![0.0; frames * 2];
                playback.callback.mixer.render_rt_at_output_frame(
                    &mut output,
                    &mut [0.0; NUM_SAMPLES],
                    playback.frame,
                    &mut playback.callback.retirement,
                );
                assert!(output.iter().all(|sample| *sample == 0.0));
                playback.frame += frames as u64;
                assert_eq!(playback.source_frame, frozen_source_frame);
                assert_other(playback);
                controller_python(
                    locals,
                    "loader.poll_loader_events()\ncontroller.on_frame_render()",
                );
                [old, pending].iter().all(|probe| {
                    probe.identity.strong_count() == 0
                        && probe.pcm.iter().all(|pcm| pcm.strong_count() == 0)
                })
            },
        );
        old.wait_for("Unload productive paired asset cleanup", || {
            controller_python(
                locals,
                "loader.poll_loader_events()\ncontroller.on_frame_render()",
            );
            [old, pending]
                .iter()
                .all(|probe| probe.areas.iter().all(|path| !path.exists()))
        });
        assert!(playback.callback.retirement.retired.is_empty());
        controller_python(
            locals,
            "assert project.sample_paths[215] == unload_other_path\nassert project.pad_content[215] == unload_other_content\nassert not controller._assets.retirement_errors",
        );
    }
}

fn public_source_replacement(
    engine: &AudioEngine,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    parameters: &mut rtrb::Consumer<ControlParameterMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) -> (Vec<f32>, u64, Option<u64>) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert_eq!(playback.drain(consumer), 1, "actual old-source V2 Capture");
    wait_until(|| consumer.peek().is_ok());
    let queued = match consumer.peek().unwrap() {
        ControlMessage::CommittedStemWork(work) => {
            assert!(
                matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
                if transaction.id == 0 && transaction.candidate.is_some())
            );
            work.as_ref() as *const CommittedStemRtWork as usize
        }
        _ => panic!("expected actual old-source queued Adopt"),
    };
    playback.callback.retirement.slots = 2;
    assert_eq!(playback.drain(consumer), 0);
    let old_source = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let old_set = playback.callback.mixer.stems_for_measurement()[0].clone();
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let historical_adoption = before.stretch.adopted_request_id();
    assert_eq!(historical_adoption.is_some(), key_lock);
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let other = other_voice(&playback.callback.mixer);
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_source = other.sample.clone().unwrap();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let source_epoch_before = engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let input = std::env::current_dir()
        .unwrap()
        .join("public-replacement.wav");
    let input_bytes = signal_sum_wav(4096, &[4, 5, 6, 7]);
    let expected_source: Vec<f32> = input_bytes[44..]
        .chunks_exact(2)
        .map(|bytes| f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0)
        .collect();
    fs::write(&input, &input_bytes).unwrap();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("source_input", input.to_string_lossy().as_ref())
            .unwrap();
    });
    controller_python(
        locals,
        r#"
from flitzis_looper.controller.loader import LoaderController
class SourceLoadAudio:
    def __init__(self):
        self.success_receipts = []
    def __getattr__(self, name):
        for delegate in (load_audio, playback_audio, audio):
            if hasattr(delegate, name):
                return getattr(delegate, name)
        raise AttributeError(name)
    def poll_loader_events(self):
        event = load_audio.poll_loader_events()
        if event is not None and event.get('type') == 'success':
            # Keep only scalar receipt evidence, not the original lease or PCM custody.
            self.success_receipts.append((event['request_id'], dict(event['incoming_defaults'])))
            assert event['analysis']['bpm'] == 0
            assert event['analysis']['beat_grid']['beats'] == []
        return event
source_audio = SourceLoadAudio()
source_bpm_notifications = []
loader = LoaderController(project, session, source_audio,
    source_bpm_notifications.append, persistence.mark_dirty)
loader.bind_asset_lifecycle(controller._assets)
loader.set_stems_invalidated_callback(controller.invalidate_stem_cache)
source_candidate = candidate
source_selection = live.disk.selection
source_registration = source_selection.ticket
source_path_before = project.sample_paths[0]
source_content_before = project.pad_content[0]
source_worker_calls = audio.worker_call_count()
assert source_candidate.is_current() and not source_candidate.claimed()
assert source_registration.is_current() and source_registration.publication_status() == 'accepted'
assert source_v2.same_preparation_context(source_v2)
loader.load_sample_async(0, source_input)
assert not session.sample_load_errors, session.sample_load_errors
source_request = loader._load_request_ids[0]
source_expected_defaults = {'revision': 0, 'key_lock': False, 'start_s': 0.0,
    'end_s': None, 'period_seconds': None, 'grid_origin_s': 0.0,
    'loop_auto': True, 'loop_bars': 8.0}
assert not project.key_lock and project.pad_key_lock[0] is None
assert not source_audio.success_receipts
assert 0 in session.loading_sample_ids and session.pending_sample_paths[0] == source_input
assert project.sample_paths[0] == source_path_before and project.pad_content[0] == source_content_before
assert project.stem_cache[0].pair == disk_v2
assert not source_candidate.claimed()
assert source_registration.publication_status() == 'accepted'
assert live.control is previous_control and live.attempt is source_selection
"#,
    );
    let admitted = voice(&playback.callback.mixer);
    assert!(admitted.source_playback.matches_exact(&clock));
    assert_eq!(admitted.generation, generation);
    assert_eq!(admitted.stretch.native_state_address(), native);
    assert_eq!(admitted.stretch.pending_fifo_frames(), fifo);
    assert!(Arc::ptr_eq(
        &old_source.samples,
        &admitted.sample.as_ref().unwrap().samples
    ));
    assert_eq!(history.is_some(), key_lock);
    if let Some(history) = history {
        let current = admitted.stretch.productive_history().unwrap();
        assert_eq!(current.binding, history.binding);
        assert_eq!(current.next_position, history.next_position);
        assert_eq!(current.fed_output_frames, history.fed_output_frames);
    }
    let current_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(current_dsp.binding, dsp.binding);
    assert_eq!(current_dsp.next_position, dsp.next_position);
    assert_eq!(current_dsp.fed_output_frames, dsp.fed_output_frames);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    // Proposal admission preserves the usable predecessor. The worker arms the
    // new source only after complete preparation and reserved queue capacity.
    // Keep the old Adopt queued so neither source has received a callback ACK.
    wait_until(|| consumer.slots() == 2);
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if work.as_ref() as *const CommittedStemRtWork as usize == queued)
    );
    assert_ne!(
        engine.prepared_source_epochs[0].load(Ordering::Acquire),
        source_epoch_before,
        "queued new-source work has genuinely armed its source authority"
    );
    controller_python(
        locals,
        r#"
assert not source_candidate.is_current() and not source_candidate.claimed()
assert not source_registration.is_current() and source_registration.publication_status() == 'accepted'
assert not source_v2.same_preparation_context(source_v2)
assert live.control is previous_control and live.attempt is source_selection
assert project.sample_paths[0] == source_path_before and project.pad_content[0] == source_content_before
assert project.stem_cache[0].pair == disk_v2 and not source_audio.success_receipts
"#,
    );
    assert_eq!(playback.drain(consumer), 0);
    for frames in [1, 31, 137, 257] {
        if key_lock {
            playback.render(frames, false);
        } else {
            render_varispeed(playback, frames);
        }
        controller_python(
            locals,
            r#"
loader.poll_loader_events()
controller.on_frame_render()
assert project.sample_paths[0] == source_path_before and project.pad_content[0] == source_content_before
assert 0 in session.loading_sample_ids and project.stem_cache[0].pair == disk_v2
assert not source_candidate.claimed() and source_candidate.error() == 'Committed stem residency or mode ownership changed'
assert source_candidate.effective_failure_reason() is None
assert live.control is previous_control and live.disk.selection is source_selection
assert audio.worker_call_count() == source_worker_calls
assert not source_audio.success_receipts
"#,
        );
        assert_eq!(
            consumer.slots(),
            2,
            "polling admits no implicit old-source retry"
        );
    }
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    // Three slots permit the real old Adopt rejection, but remain below the
    // normal whole-assignment LoadColdSample reservation. No manual queue pop.
    playback.callback.retirement.slots = 3;
    assert_eq!(
        playback.drain(consumer),
        1,
        "old Adopt rejects before source ACK"
    );
    let (new_source, adoption, incoming_defaults) = match consumer.peek().unwrap() {
        ControlMessage::LoadColdSample {
            id,
            sample,
            adoption,
            replace_assignment,
            incoming_defaults,
            ..
        } => {
            assert_eq!(*id, 0);
            assert!(*replace_assignment);
            assert_eq!(adoption.load(Ordering::Acquire), 0);
            (
                sample.clone(),
                adoption.clone(),
                incoming_defaults.expect("actual incoming source-owned defaults"),
            )
        }
        _ => panic!("expected genuine new-source preparation behind rejected Adopt"),
    };
    assert!(!new_source.same_source(&old_source));
    assert!(!Arc::ptr_eq(&new_source.samples, &old_source.samples));
    assert_eq!(new_source.frame_count(), 4096);
    assert_eq!(new_source.resident_start(), 0);
    assert_eq!(new_source.resident_end(), 4096);
    // B is active at frame zero. Its 4096-frame input yields only seven
    // resampled ODF frames: one tracker column cannot establish a BPM/end.
    assert!(expected_source[1].abs() > 0.01);
    assert_eq!(incoming_defaults.sample_rate_hz, RATE);
    assert_eq!(incoming_defaults.revision, 0);
    assert!(!incoming_defaults.key_lock);
    assert_eq!(incoming_defaults.start_frame, 0);
    assert_eq!(incoming_defaults.end_frame, None);
    assert_eq!(incoming_defaults.period_seconds, None);
    assert_eq!(incoming_defaults.origin_frame, 0.0);
    assert!(incoming_defaults.loop_auto);
    assert_eq!(incoming_defaults.loop_bars, 8.0);
    assert_eq!(
        new_source.samples.as_ref(),
        expected_source.as_slice(),
        "actual imported B PCM matches independent input WAV decoding"
    );
    let rejected = voice(&playback.callback.mixer);
    assert!(rejected.source_playback.matches_exact(&clock));
    assert_eq!(rejected.generation, generation);
    assert_eq!(rejected.stretch.native_state_address(), native);
    assert_eq!(rejected.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let current = rejected.stretch.productive_history().unwrap();
        assert_eq!(current.binding, history.binding);
        assert_eq!(current.next_position, history.next_position);
        assert_eq!(current.fed_output_frames, history.fed_output_frames);
    }
    let current_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(current_dsp.binding, dsp.binding);
    assert_eq!(current_dsp.next_position, dsp.next_position);
    assert_eq!(current_dsp.fed_output_frames, dsp.fed_output_frames);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert!(Arc::ptr_eq(
        &old_source.samples,
        &playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    match (
        &old_set,
        &playback.callback.mixer.stems_for_measurement()[0],
    ) {
        (Some(old), Some(current)) => {
            assert!(Arc::ptr_eq(
                &old.complete_set_identity,
                &current.complete_set_identity
            ));
            for (old, current) in old.stems.iter().zip(&current.stems) {
                assert!(Arc::ptr_eq(&old.samples, &current.samples));
            }
        }
        (None, None) => {}
        _ => panic!("old readers changed before source ACK"),
    }
    controller_python(
        locals,
        r#"
loader.poll_loader_events()
controller.on_frame_render()
assert project.sample_paths[0] == source_path_before and project.pad_content[0] == source_content_before
assert 0 in session.loading_sample_ids
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
assert source_candidate.error() == 'Committed stem residency or mode ownership changed'
assert source_candidate.effective_failure_reason() is None
assert not source_audio.success_receipts
"#,
    );
    for frames in [1, 31, 137, 257, 511] {
        if key_lock {
            playback.render(frames, false);
        } else {
            render_varispeed(playback, frames);
        }
        assert_eq!(adoption.load(Ordering::Acquire), 0);
        assert_eq!(playback.drain(consumer), 0);
    }
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    let other_fifo = other_voice(&playback.callback.mixer)
        .stretch
        .pending_fifo_frames();
    let feedback_before = playback.callback.feedback.events.len();
    playback.callback.retirement.slots = usize::MAX;
    let stopped_feedback =
        ExpectedStoppedFeedback::source_command(&playback.callback.mixer, consumer, 0);
    assert_eq!(playback.drain(consumer), 1, "actual replacement source ACK");
    assert!(consumer.is_empty());
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    assert_eq!(playback.callback.mixer.loop_region_frames(0), (0, None));
    assert!(!playback.callback.mixer.key_lock_for_measurement(0));
    assert!(
        !playback
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    assert!(Arc::ptr_eq(
        &new_source.samples,
        &playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .samples
    ));
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    // Loading removes actual stem readers and resets the scalar mask. Its old
    // source-bound mode/hash is not an effective selection for the new source.
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        (demand.0, 0b1111, demand.2)
    );
    assert_eq!(playback.callback.feedback.events.len(), feedback_before + 1);
    stopped_feedback.assert_message(&playback.callback.feedback.events[feedback_before]);
    let other = other_voice(&playback.callback.mixer);
    assert!(other.source_playback.matches_exact(&other_clock));
    assert_eq!(other.generation, other_generation);
    assert_eq!(other.stretch.native_state_address(), other_native);
    assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
    wait_until(|| engine.cold_jobs.counts_for_test() == (0, 0, 0));
    wait_until(|| {
        controller_python(
            locals,
            "loader.poll_loader_events()\nassert not session.sample_load_errors, session.sample_load_errors",
        );
        Python::attach(|py| {
            !locals
                .bind(py)
                .get_item("session")
                .unwrap()
                .unwrap()
                .getattr("loading_sample_ids")
                .unwrap()
                .call_method1("__contains__", (0,))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        })
    });
    controller_python(
        locals,
        r#"
assert project.sample_paths[0] != source_path_before
assert project.pad_content[0].material_id != source_content_before.material_id
assert project.pad_content[0].instance_id != source_content_before.instance_id
assert project.stem_cache[0] is None and project.pad_stem_mix_mode[0] == 'full_mix'
assert project.pad_key_lock[0] is None
assert 0 not in session.active_sample_ids and 215 in session.active_sample_ids
assert not controller._committed_residency.contains(0)
assert controller.stem_residency_status(0) is None
assert not source_candidate.is_current() and not source_candidate.claimed()
assert source_candidate.residency_status() == 'rejected'
assert source_candidate.error() == 'Committed stem residency or mode ownership changed'
assert source_candidate.effective_failure_reason() is None
assert not session.stem_generation_errors and not session.sample_load_errors
assert source_audio.success_receipts == [(source_request, source_expected_defaults)]
assert source_bpm_notifications == []
assert project.pad_loop_start_s[0] == 0.0 and project.pad_loop_end_s[0] is None
assert project.pad_loop_auto[0] and project.pad_loop_bars[0] == 8.0
assert project.manual_bpm[0] is None and project.pad_timing_intent[0] == 'legacy'
assert project.pad_grid_anchor_s[0] == 0.0 and project.pad_grid_offset_samples[0] == 0
assert project.pad_gain_db[0] == 0
assert (project.pad_eq_low_db[0], project.pad_eq_mid_db[0], project.pad_eq_high_db[0]) == (0, 0, 0)
source_version_now = controller.source_version_for_pad(0)
source_identity_now = source_audio.waveform_source_identity(0)
assert source_identity_now[0] == source_request
assert source_identity_now[2:] == (4096, fixture['rate'])
source_now = audio.capture_prepared_source(0, source_version_now)
assert not source_v2.same_source_request(source_now)
assert not source_v2.same_preparation_context(source_now)
assert audio.worker_call_count() == source_worker_calls
persistence.mark_dirty()
persistence.flush()
source_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert source_reopened.load_error is None
assert source_reopened.project.sample_paths[0] == project.sample_paths[0]
assert source_reopened.project.pad_content[0] == project.pad_content[0]
assert source_reopened.project.stem_cache[0] is None
assert source_reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert source_reopened.project.pad_key_lock[0] is None
"#,
    );
    assert_eq!(
        playback.drain(consumer),
        0,
        "own source ACK already installed its defaults"
    );
    assert!(consumer.is_empty());
    assert!(parameters.is_empty());
    crate::audio_engine::audio_stream::drain_parameter_messages(
        parameters,
        &mut playback.callback.mixer,
        &mut playback.callback.transport,
    );
    assert!(parameters.is_empty());
    assert!(
        !playback
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    );
    for frames in [1, 31, 137, 257] {
        let mut output = vec![0.0; frames * 2];
        playback.callback.mixer.render_rt(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            &mut playback.callback.retirement,
        );
        assert!(
            output.iter().all(|sample| *sample == 0.0),
            "no automatic replacement voice start"
        );
        playback.frame += frames as u64;
        controller_python(
            locals,
            r#"
loader.poll_loader_events()
controller.on_frame_render()
assert controller.stem_residency_status(0) is None
assert not source_candidate.claimed() and not source_candidate.is_current()
assert not session.stem_generation_errors and not session.sample_load_errors
assert audio.worker_call_count() == source_worker_calls
"#,
        );
        assert!(consumer.is_empty());
        assert!(
            !playback
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0))
        );
        let other = other_voice(&playback.callback.mixer);
        let phase = (playback.frame as f64 * playback.ratio).rem_euclid(1500.0);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
    }
    controller_python(locals, "loader.shut_down()");
    (expected_source, generation, historical_adoption)
}

fn public_replaced_source_play(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    expected_pcm: Vec<f32>,
    predecessor_generation: u64,
    historical_adoption: Option<u64>,
    trace: CandidateTrace,
) -> Option<[Arc<InputRuntimeOwnership>; 2]> {
    assert!(!playback.scheduled && playback.rate_ledger.is_none());
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    assert_eq!(playback.loop_period, 1500.0);
    assert_eq!(playback.loop_end, LOOP_END);
    let assigned = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert_eq!(assigned.samples.as_ref(), expected_pcm.as_slice());
    assert!(!playback.callback.mixer.key_lock_for_measurement(0));
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let other_clock = other.source_playback;
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    playback.callback.retirement.slots = 2;
    controller_python(
        locals,
        r#"
from flitzis_looper.controller.transport import TransportController
class ReplacementPlayAudio(CapturedTransportAudio):
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('new-source Play must wait for its own resident window ACK')
    play_sample_exclusive = play_sample
assert project.pad_key_lock[0] is None and not project.resolve_pad_key_lock(0)
assert project.pad_gain_db[0] == 0
assert (project.pad_eq_low_db[0], project.pad_eq_mid_db[0], project.pad_eq_high_db[0]) == (0, 0, 0)
# Independently declared finite intent; full App automatic-loop initialization is separate.
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = 17 / fixture['rate']
project.pad_loop_end_s[0] = 1517 / fixture['rate']
transport = TransportController(project, session, ReplacementPlayAudio(), persistence.mark_dirty)
ordinary_timestamp = captured_audio.capture_input_timestamp_ns()
transport.playback.trigger_pad_keep_others(0, received_at_ns=ordinary_timestamp)
ordinary_launch = next(iter(transport.residency._captured_starts.values()))
ordinary_start = ordinary_launch.start
source_play_window = ordinary_launch.ticket
assert source_play_window is not None
assert source_play_window.publication_status() in {'preparing', 'pending'}
assert source_play_window.error() is None
assert not controller._committed_residency.contains(0)
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
assert audio.worker_call_count() == source_worker_calls
"#,
    );
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    wait_until(|| {
        Python::attach(|py| {
            locals
                .bind(py)
                .get_item("source_play_window")
                .unwrap()
                .unwrap()
                .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
                .unwrap()
                .control_published_for_test()
        })
    });
    assert_original_target_at_front(consumer, locals);
    assert_eq!(
        playback.drain(consumer),
        1,
        "original target precedes held real new-source window"
    );
    assert_eq!(
        ordinary_start_from_python(locals).original_frame(),
        Some(playback.frame)
    );
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::RelocateResident(_))
    ));
    let is_stopped = |playback: &ControllerPlayback| {
        !playback
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0))
    };
    for frames in [1, 31, 137, 257] {
        assert!(is_stopped(playback));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0),
            demand
        );
        assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
        let mut output = vec![0.0; frames * 2];
        playback.callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            &mut playback.callback.retirement,
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        playback.frame += frames as u64;
        controller_python(
            locals,
            r#"
transport.on_frame_render()
controller.on_frame_render()
assert source_play_window.publication_status() == 'pending'
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
assert controller.stem_residency_status(0) is None
assert not session.stem_generation_errors and transport.residency.status(0)[1] is None
assert audio.worker_call_count() == source_worker_calls
"#,
        );
        assert!(matches!(
            consumer.peek(),
            Ok(ControlMessage::RelocateResident(_))
        ));
        assert_eq!(playback.drain(consumer), 0);
    }
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    let feedback_before = playback.callback.feedback.events.len();
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(
        playback.drain(consumer),
        1,
        "own new-source resident window ACK"
    );
    assert!(consumer.is_empty() && is_stopped(playback));
    assert_eq!(playback.callback.feedback.events.len(), feedback_before);
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    controller_python(
        locals,
        "assert source_play_window.publication_status() == 'accepted'\nassert source_play_window.is_current()",
    );
    Python::attach(|py| {
        let window = locals
            .bind(py)
            .get_item("source_play_window")
            .unwrap()
            .unwrap();
        let window = window
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        let observation = window
            .read_observation_for_test()
            .expect("genuine new-source finite range read");
        assert_eq!(
            (observation.source.start_frame, observation.source.end_frame),
            (LOOP_START, LOOP_END)
        );
        assert_eq!(observation.source.read_bytes, 1500 * 2 * 4);
        assert!(
            observation.stems.read_bytes.iter().all(|bytes| *bytes == 0),
            "new original has no old component reads"
        );
    });
    let finite = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(finite.same_source(&assigned));
    assert_ne!(finite.window_revision(), assigned.window_revision());
    assert_eq!(
        (
            finite.resident_start(),
            finite.resident_end(),
            finite.frame_count()
        ),
        (LOOP_START, LOOP_END, 4096)
    );
    assert_eq!(
        finite.samples.as_ref(),
        &expected_pcm[LOOP_START * 2..LOOP_END * 2]
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    // Accepted geometry remains a stopped source until the explicit launch is polled.
    for frames in [1, 31, 137] {
        let mut output = vec![0.0; frames * 2];
        playback.callback.mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            &mut playback.callback.retirement,
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        playback.frame += frames as u64;
        assert!(is_stopped(playback) && consumer.is_empty());
        controller_python(
            locals,
            "controller.on_frame_render()\nassert not source_candidate.claimed()\nassert audio.worker_call_count() == source_worker_calls",
        );
    }
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    let expected_started = ExpectedStoppedFeedback::capture_started(
        &playback.callback.mixer,
        0,
        &ordinary_start_from_python(locals),
    );
    let launch_timeline = playback.callback.transport;
    let start = admit_original_public_start(&mut playback.callback, consumer, locals);
    let birth = start.execution_frame().unwrap();
    let original_start = start.original_frame().unwrap();
    assert!(original_start < launch_timeline.output_frame());
    let launch = LoopPhaseReference::capture(Some(launch_timeline), original_start);
    while playback.frame < birth {
        let frames = (birth - playback.frame).min(257) as usize;
        let mut output = vec![0.0; frames * 2];
        let callback = playback.callback.as_mut();
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            playback.frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        callback.transport.advance_by_rendered_frames(frames);
        playback.frame += frames as u64;
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert!(is_stopped(playback) && start.effective_frame().is_none());
    }
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    execute_public_start_at_frame(&mut playback.callback, &start);
    assert!(consumer.is_empty());
    let current = voice(&playback.callback.mixer);
    let generation = current.generation;
    assert_eq!(
        generation,
        predecessor_generation + 2,
        "own stop and own new voice birth"
    );
    assert!(current.sample.as_ref().unwrap().same_window(&finite));
    assert!(birth >= original_start);
    assert_eq!(current.source_playback.position().frame, LOOP_START);
    assert_eq!(
        current.source_playback.position().fraction.to_bits(),
        0.0_f64.to_bits()
    );
    assert_eq!(
        current.source_playback.tempo_ratio().to_bits(),
        playback.ratio.to_bits()
    );
    assert!(
        current
            .source_playback
            .phase_reference_current(&launch_timeline)
    );
    assert!(!current.launch_qualified);
    assert_eq!(playback.callback.feedback.events.len(), feedback_before + 1);
    expected_started.assert_message(&playback.callback.feedback.events[feedback_before]);
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    // Each oracle starts from independent complete B bytes and neutral defaults,
    // without copying the actual resident read plan, predecessor DSP or old stems.
    let independent = SampleBuffer {
        channels: 2,
        samples: Arc::from(expected_pcm),
        residency: None,
    };
    assert!(
        independent
            .samples
            .iter()
            .zip(playback.source.samples.iter())
            .any(|(new, old)| (new - old).abs() > 0.005),
        "independent B PCM differs from old A PCM"
    );
    let ratio = playback.ratio;
    let reference_owners = matches!(
        trace,
        CandidateTrace::SourceKeyLockOn | CandidateTrace::SourceStemsOn
    )
    // Independent B PCM oracles use unrestricted component permits.
    .then(|| std::array::from_fn(|_| Arc::new(InputRuntimeOwnership::default())));
    let independent_stems = matches!(
        trace,
        CandidateTrace::SourceStemsOff | CandidateTrace::SourceStemsOn
    )
    .then(|| {
        let version = Python::attach(|py| {
            locals
                .bind(py)
                .get_item("source_version_now")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap()
        });
        PreparedStemSet {
            complete_set_identity: Arc::new([0xb4; 32]),
            reference_samples: independent.samples.clone(),
            reference_residency: independent.residency.clone(),
            publication: PreparedSourcePermit::unrestricted(),
            accepted_timing: None,
            source_version_hash: crate::audio_engine::stem_cache::source_version_hash(&version),
            sample_rate_hz: RATE,
            channels: 2,
            frame_count: 4096,
            available_mask: 0b1111,
            stems: std::array::from_fn(|index| {
                replacement_component_pcm(&signal_wav(4096, index + 4))
            }),
        }
    });
    let reference = |index: usize| {
        let mut mixer = Box::new(RtMixer::new(2, RATE as f32));
        if let Some(owners) = &reference_owners {
            owners[index].publish_test_source(0, &independent, RATE, 1);
            mixer.set_input_runtime_ownership(owners[index].clone());
        }
        mixer.load_sample(0, independent.clone());
        mixer.set_loop_region_frames(0, LOOP_START, Some(LOOP_END));
        mixer.set_speed(ratio);
        mixer.set_pad_gain(0, 0.0);
        mixer.set_pad_eq(0, 0.0, 0.0, 0.0);
        if let Some(stems) = &independent_stems {
            // Only the complete-PCM reference predeclares immutable B components
            // before its birth; actual B still owns no stems until its own Adopt.
            assert!(mixer.publish_prepared_stems(0, stems.clone()));
        }
        assert!(mixer.play_sample_at_output_frame(0, 1.0, birth));
        install_public_start_reference(&mut mixer, launch, &launch_timeline);
        mixer
    };
    playback.reference = reference(0);
    playback.full_mix = reference(1);
    if let Some(stems) = independent_stems {
        playback.components = stems.stems;
    }
    playback.source = independent;
    playback.source_frame = 0;
    playback.resumed_at = Some(birth);
    playback.selection = None;
    playback.previous = None;
    playback.ramp_start = 0;
    playback.raw = None;
    playback.candidate = None;
    playback.filter = PerPadDspChain::new(0, RATE as f32, 2);
    playback.filter.reset();
    playback.advance_expected_birth(original_start, birth);
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    let fifo = voice(&playback.callback.mixer)
        .stretch
        .pending_fifo_frames();
    assert_eq!(
        voice(&playback.callback.mixer).stretch.adopted_request_id(),
        historical_adoption
    );
    // This cfg(test) historical ID is not a new request, source owner or effect.
    // reset() clears actual history/Wet/FIFOs; retain the exact old measurement.
    let mut non_silent = false;
    for frames in [1, 31, 137, 257, 511, 511, 257, 511] {
        let (actual, full_mix) =
            render_varispeed_with_historical_adoption(playback, frames, historical_adoption);
        assert_eq!(actual, full_mix, "own new B FullMix without old stems");
        non_silent |= actual.iter().any(|value| value.abs() > 0.001);
        let current = voice(&playback.callback.mixer);
        assert_eq!(current.generation, generation);
        assert_eq!(current.stretch.native_state_address(), native);
        assert_eq!(current.stretch.pending_fifo_frames(), fifo);
        assert!(current.sample.as_ref().unwrap().same_window(&finite));
        assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
        controller_python(
            locals,
            r#"
transport.on_frame_render()
controller.on_frame_render()
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
assert not source_candidate.is_current()
assert source_candidate.error() == 'Committed stem residency or mode ownership changed'
assert source_candidate.effective_failure_reason() is None
assert controller.stem_residency_status(0) is None
assert not session.stem_generation_errors and transport.residency.status(0)[1] is None
assert audio.worker_call_count() == source_worker_calls
"#,
        );
        assert!(consumer.is_empty());
        let other = other_voice(&playback.callback.mixer);
        let phase = (playback.frame as f64 * playback.ratio).rem_euclid(1500.0);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
        assert_eq!(playback.frame - playback.source_frame, birth);
    }
    assert!(non_silent && playback.source_frame as f64 * playback.ratio > 1500.0);
    controller_python(
        locals,
        r#"
persistence.mark_dirty()
persistence.flush()
source_play_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert source_play_reopened.load_error is None
assert source_play_reopened.project.sample_paths[0] == project.sample_paths[0]
assert source_play_reopened.project.pad_content[0] == project.pad_content[0]
assert source_play_reopened.project.stem_cache[0] is None
assert source_play_reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert source_play_reopened.project.pad_key_lock[0] is None
assert source_play_reopened.project.pad_loop_start_s[0] == 17 / fixture['rate']
assert source_play_reopened.project.pad_loop_end_s[0] == 1517 / fixture['rate']
"#,
    );
    reference_owners
}

fn assert_current_replaced_source_started(locals: &Py<PyDict>, message_name: &str) {
    controller_python(
        locals,
        &format!(
            r#"
current_b_source = audio.capture_prepared_source(0, source_version_now)
assert current_b_source.same_source_request(source_now)
assert not current_b_source.same_source_request(source_v2)
current_b_binding = playback_audio.current_input_runtime_pad_binding(0)
assert current_b_binding is not None and current_b_binding.is_current()
current_b_observation = playback_audio._current_ordinary_transport_observation(0)
assert current_b_observation is not None
assert current_b_observation.current_control_slot() == 0
assert current_b_observation.content_lifetime == current_b_binding.metadata()['content_lifetime']
assert current_b_observation.active and not current_b_observation.ordinary_paused
# The real render loop already polls durable current-B lifecycle state. Its
# original Started diagnostic must still resolve exactly and be idempotent.
transport.playback.poll_pause_observations()
assert 0 in session.active_sample_ids and 0 not in session.paused_sample_ids
before_started_active = set(session.active_sample_ids)
before_started_paused = set(session.paused_sample_ids)
resolved_started = captured_audio._resolve_pad_feedback({message_name})
assert resolved_started == 0
transport.playback.handle_sample_started_message({message_name}, sample_id=resolved_started)
assert session.active_sample_ids == before_started_active
assert session.paused_sample_ids == before_started_paused
del current_b_source, current_b_binding, current_b_observation
"#
        ),
    );
}

fn public_replaced_source_key_lock_on(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    reference_owners: [Arc<InputRuntimeOwnership>; 2],
) {
    assert_eq!(playback.ratio, 1.25);
    assert!(consumer.is_empty() && playback.raw.is_none() && playback.selection.is_none());
    let old_window = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let current = voice(&playback.callback.mixer);
    let generation = current.generation;
    let native = current.stretch.native_state_address();
    let birth = playback.frame - playback.source_frame;
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let started = playback
        .callback
        .feedback
        .events
        .iter()
        .rev()
        .find(|event| matches!(event, AudioMessage::ContentFeedback { id: 0, kind: 1, .. }))
        .expect("actual B public Play feedback")
        .clone();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("b_on_started", Py::new(py, started).unwrap())
            .unwrap()
    });
    assert_current_replaced_source_started(locals, "b_on_started");
    controller_python(
        locals,
        r#"
assert 0 in session.active_sample_ids
b_on_path = project.sample_paths[0]
b_on_content = project.pad_content[0]
b_on_workers = audio.worker_call_count()
b_off_source = audio.capture_prepared_source(0, source_version_now)
assert project.pad_key_lock[0] is None and not project.key_lock
transport.pad.set_pad_key_lock(0, enabled=True)
b_on_window = transport.residency._pending[0].ticket
assert b_on_window is not None and b_on_window.key_lock_request_id is not None
assert project.pad_key_lock[0] is True and not project.key_lock
assert not project.resolve_pad_key_lock(215)
assert project.stem_cache[0] is None and not controller._committed_residency.contains(0)
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
status = transport.residency.key_lock_status(0)
assert status.requested and status.pending and status.error is None
"#,
    );
    wait_until(|| consumer.peek().is_ok());
    assert!(matches!(
        consumer.peek(),
        Ok(ControlMessage::RelocateResident(_))
    ));
    playback.callback.retirement.slots = 2;
    assert_eq!(playback.drain(consumer), 0, "own ON-window retirement hold");
    for frames in [1, 31, 137, 257] {
        let (actual, full_mix) = render_varispeed(playback, frames);
        assert_eq!(actual, full_mix, "B stays dry before its own ON-window ACK");
        assert!(!playback.callback.mixer.key_lock_for_measurement(0));
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert!(
            voice(&playback.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .same_window(&old_window)
        );
        controller_python(
            locals,
            r#"
transport.on_frame_render()
controller.on_frame_render()
assert b_on_window.publication_status() == 'pending'
assert transport.residency.key_lock_status(0).pending
assert not session.stem_generation_errors
assert audio.worker_call_count() == b_on_workers
assert not source_candidate.claimed()
"#,
        );
        assert_eq!(playback.drain(consumer), 0);
    }
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let feedback_count = playback.callback.feedback.events.len();
    let on_ack_frame = playback.frame;
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(playback.drain(consumer), 1, "own B KEY LOCK ON-window ACK");
    assert!(consumer.is_empty());
    let on_window = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(on_window.same_source(&old_window));
    assert_ne!(on_window.window_revision(), old_window.window_revision());
    assert_eq!(
        (on_window.resident_start(), on_window.resident_end()),
        (LOOP_START, LOOP_END)
    );
    assert_eq!(on_window.samples.as_ref(), old_window.samples.as_ref());
    let after = voice(&playback.callback.mixer);
    assert_eq!(after.generation, generation);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.stretch.native_state_address(), native);
    assert!(after.sample.as_ref().unwrap().same_window(&on_window));
    assert_eq!(
        playback.callback.feedback.events.len(),
        feedback_count,
        "ON does not relaunch B"
    );
    let after_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(after_dsp.binding, dsp.binding);
    assert_eq!(after_dsp.next_position, dsp.next_position);
    assert_eq!(after_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(playback.callback.mixer.key_lock_for_measurement(0));
    controller_python(
        locals,
        r#"
transport.on_frame_render()
assert b_on_window.publication_status() == 'accepted' and b_on_window.is_current()
feedback = transport.residency._mode_feedback(0)
assert feedback['window_revision'] == b_on_window.window_revision
assert feedback['request_id'] == b_on_window.key_lock_request_id
assert feedback['effective'] is False and feedback['ready'] is False
status = transport.residency.key_lock_status(0)
assert status.requested and status.pending and status.effective is False and status.error is None
assert transport.residency._pending[0].ticket is b_on_window
assert project.stem_cache[0] is None and controller.stem_residency_status(0) is None
"#,
    );
    for (reference, owner) in [playback.reference.as_mut(), playback.full_mix.as_mut()]
        .into_iter()
        .zip(&reference_owners)
    {
        let source = &playback.source;
        let request = owner.next_key_lock_request(0).unwrap();
        reference.set_pad_key_lock_request(
            0,
            true,
            request,
            InputPadBinding {
                placement: owner.binding_placement(0),
                resident: source.resident_binding(),
                source_address: source.source_address(),
                sample_count: source.source_sample_count(),
                channels: source.channels,
                sample_rate_hz: RATE,
                authority_revision: owner.authority_revision(0),
                runtime_revision: 0,
                accepted: None,
            },
            1,
        );
        assert!(reference.key_lock_for_measurement(0));
        assert!(voice(reference).source_playback.matches_exact(&clock));
    }
    playback.raw = Some(ControllerRawNative::new(playback.ratio, false));
    playback.render(13, true);
    controller_python(
        locals,
        r#"
transport.on_frame_render()
assert transport.residency.key_lock_status(0).pending
assert transport.residency._pending[0].ticket is b_on_window
assert audio.worker_call_count() == b_on_workers
"#,
    );
    wait_until(|| {
        voice_mut(&mut playback.callback.mixer)
            .stretch
            .source_preparation_ready()
    });
    let target = voice_mut(&mut playback.callback.mixer)
        .stretch
        .prepared_target_output_frame()
        .unwrap();
    assert!(
        target - crate::audio_engine::prepared_native_history::PREPARED_HISTORY_FRAMES as u64
            >= on_ack_frame
    );
    let (actual, full_mix) = playback.adopt_native(true);
    assert_eq!(
        actual, full_mix,
        "own B FullMix wet output has no old stems"
    );
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert!(
        voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready()
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    controller_python(
        locals,
        r#"
transport.on_frame_render()
controller.on_frame_render()
assert transport.residency.status(0) == (None, None)
status = transport.residency.key_lock_status(0)
assert status.requested and status.effective is True and not status.pending
assert not status.unconfirmed and status.error is None
feedback = transport.residency._mode_feedback(0)
assert feedback['window_revision'] == b_on_window.window_revision
assert feedback['request_id'] == b_on_window.key_lock_request_id
assert feedback['effective'] is True and feedback['ready'] is True
b_on_source = audio.capture_prepared_source(0, source_version_now)
assert b_on_source.same_source_request(b_off_source)
assert not b_on_source.same_preparation_context(b_off_source)
assert b_on_source.same_preparation_context(b_on_source)
assert not b_on_source.same_source_request(source_v2)
assert project.sample_paths[0] == b_on_path and project.pad_content[0] == b_on_content
assert project.stem_cache[0] is None and controller.stem_residency_status(0) is None
assert not source_candidate.claimed() and source_candidate.residency_status() == 'rejected'
assert source_candidate.error() == 'Committed stem residency or mode ownership changed'
assert source_candidate.effective_failure_reason() is None
assert audio.worker_call_count() == b_on_workers and not session.stem_generation_errors
persistence.flush()
b_on_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert b_on_reopened.load_error is None and b_on_reopened.project.pad_key_lock[0] is True
assert not b_on_reopened.project.key_lock and not b_on_reopened.project.resolve_pad_key_lock(215)
assert b_on_reopened.project.sample_paths[0] == b_on_path
assert b_on_reopened.project.pad_content[0] == b_on_content
assert b_on_reopened.project.stem_cache[0] is None
assert b_on_reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert b_on_reopened.project.pad_loop_start_s[0] == 17 / fixture['rate']
assert b_on_reopened.project.pad_loop_end_s[0] == 1517 / fixture['rate']
"#,
    );
    for frames in [1, 31, 137, 257, 511] {
        let (actual, full_mix) = playback.render(frames, true);
        assert_eq!(actual, full_mix);
        assert_eq!(playback.frame - playback.source_frame, birth);
        let current = voice(&playback.callback.mixer);
        assert_eq!(current.generation, generation);
        assert!(current.sample.as_ref().unwrap().same_window(&on_window));
        let other = other_voice(&playback.callback.mixer);
        let phase = (playback.frame as f64 * playback.ratio).rem_euclid(1500.0);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
        controller_python(
            locals,
            "transport.on_frame_render()\ncontroller.on_frame_render()\nassert audio.worker_call_count() == b_on_workers\nassert not session.stem_generation_errors\nassert not source_candidate.claimed()\nassert transport.residency.status(0) == (None, None)",
        );
        assert!(consumer.is_empty());
    }
}

fn replacement_component_pcm(bytes: &[u8]) -> SampleBuffer {
    // Independent committed-component PCM16 convention divides by32767.
    // The original decoder uses32768; neither oracle copies resident views.
    let samples: Vec<f32> = bytes[44..]
        .chunks_exact(2)
        .map(|sample| {
            let integer = i16::from_le_bytes(sample.try_into().unwrap());
            assert_ne!(
                integer,
                i16::MIN,
                "fixture components need no negative saturation"
            );
            f32::from(integer) / 32767.0
        })
        .collect();
    assert_eq!(samples.len(), 4096 * 2);
    SampleBuffer {
        channels: 2,
        samples: Arc::from(samples),
        residency: None,
    }
}

fn public_replaced_source_stems(
    root: &Path,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert_eq!(playback.ratio, 1.25);
    assert_eq!(
        playback.callback.mixer.key_lock_for_measurement(0),
        key_lock
    );
    assert!(consumer.is_empty() && playback.selection.is_none());
    assert_eq!(playback.raw.is_some(), key_lock);
    let finite = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let before = voice(&playback.callback.mixer);
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let full_mix_adoption = before.stretch.adopted_request_id();
    let birth = playback.frame - playback.source_frame;
    assert!(!key_lock || full_mix_adoption.is_some());
    let other = other_voice(&playback.callback.mixer);
    let other_source = other.sample.clone().unwrap();
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let other_demand = playback.callback.mixer.stem_demand_for_measurement(215);
    let started = playback
        .callback
        .feedback
        .events
        .iter()
        .rev()
        .find(|event| matches!(event, AudioMessage::ContentFeedback { id: 0, kind: 1, .. }))
        .expect("B's genuine public launch feedback")
        .clone();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("b_started", Py::new(py, started).unwrap())
            .unwrap();
        locals
            .bind(py)
            .set_item("b_stem_key_lock", key_lock)
            .unwrap();
    });
    assert_current_replaced_source_started(locals, "b_started");
    controller_python(
        locals,
        r#"
assert 0 in session.active_sample_ids and 0 not in session.paused_sample_ids
assert project.stem_cache[0] is None and project.pad_stem_mix_mode[0] == 'full_mix'
b_path = project.sample_paths[0]
b_content = project.pad_content[0]
b_version = controller.source_version_for_pad(0)
assert b_version is not None and b_version != fixture['source_version']
b_source = audio.capture_prepared_source(0, b_version)
assert b_source.same_source_request(source_now)
assert not b_source.same_preparation_context(source_now)
assert not b_source.same_source_request(source_v2)
if b_stem_key_lock:
    assert b_version == source_version_now and project.pad_key_lock[0] is True
    assert b_source.same_preparation_context(b_on_source)
    assert b_source.same_source_request(b_off_source)
    assert not b_source.same_preparation_context(b_off_source)
    status = transport.residency.key_lock_status(0)
    assert status.requested and status.effective is True and not status.pending
b_worker_calls = audio.worker_call_count()
assert controller.set_stem_enabled_mask(0, 0b1010)
assert project.pad_stem_intent[0].enabled_mask == 0b1010
controller.bind_disk_selection_commit(persistence.commit_stem_selection, persistence.flush)
"#,
    );
    let (original, version) = Python::attach(|py| {
        let locals = locals.bind(py);
        (
            locals
                .get_item("b_path")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
            locals
                .get_item("b_version")
                .unwrap()
                .unwrap()
                .extract::<String>()
                .unwrap(),
        )
    });
    let material_root =
        crate::audio_engine::material_paths::material_root_for_original(root, Path::new(&original))
            .unwrap();
    let directory = material_root
        .join("stems")
        .join(format!(".ready-{:032x}", 0xb401_u32));
    write_signal_set(&directory, 4096, &version, 4);
    let project_root = fs::canonicalize(root.parent().unwrap()).unwrap();
    let reference = directory
        .strip_prefix(&project_root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    // Read actual independent complete B files, never resident component views.
    let components: [SampleBuffer; 4] = std::array::from_fn(|index| {
        let bytes = fs::read(directory.join(format!("{}.wav", STEM_FILE_NAMES[index]))).unwrap();
        let decoded = replacement_component_pcm(&bytes);
        assert_eq!(
            decoded.samples.as_ref(),
            playback.components[index].samples.as_ref(),
            "independent prebirth oracle matches its own complete B component file"
        );
        decoded
    });
    Python::attach(|py| locals.bind(py).set_item("b_cache", reference).unwrap());
    controller_python(
        locals,
        r#"
b_entry = StemCacheEntry(source_version=b_version, cache_dir=b_cache,
    stems=expected_stem_files(b_cache), available=False)
assert b_entry.pair is None
assert controller._queue_stem_publication(0, b_entry, b_source)
b_requested = controller._requested_preparations[0].spec
assert b_requested.entry is b_entry and b_requested.previous_entry is None
assert b_requested.ticket is b_source and b_requested.content_id == b_content.instance_id
assert b_requested.binding.source_path == b_path and not b_requested.residency_only
assert 0 not in controller._pair_preparations.pending
controller.on_frame_render()
b_request = controller._pair_preparations.pending[0]
assert 0 not in controller._requested_preparations
assert b_request.ticket is b_requested.ticket and b_request.entry is b_requested.entry
assert b_request.entry is b_entry and b_request.previous_entry is None
assert b_request.ticket is b_source and b_request.content_id == b_content.instance_id
assert b_request.source_path == b_path and not b_request.residency_only
assert project.stem_cache[0] is None
"#,
    );
    let assert_continuity = |playback: &ControllerPlayback| {
        assert_eq!(playback.frame - playback.source_frame, birth);
        let current = voice(&playback.callback.mixer);
        assert_eq!(current.generation, generation);
        assert!(current.sample.as_ref().unwrap().same_window(&finite));
        if key_lock {
            assert!(current.stretch.productive_history().is_some());
        } else {
            assert_eq!(current.stretch.native_state_address(), native);
            assert_eq!(current.stretch.pending_fifo_frames(), fifo);
            assert!(current.stretch.productive_history().is_none());
            assert!(!current.stretch.wet_render_ready());
        }
        let phase = (playback.source_frame as f64 * playback.ratio).rem_euclid(1500.0);
        assert_eq!(
            current.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            current.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        let other = other_voice(&playback.callback.mixer);
        let phase = (playback.frame as f64 * playback.ratio).rem_euclid(1500.0);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(other.generation, other_generation);
        assert_eq!(other.stretch.native_state_address(), other_native);
        assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
        assert!(Arc::ptr_eq(
            &other.sample.as_ref().unwrap().samples,
            &other_source.samples
        ));
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(215),
            other_demand
        );
    };
    for frames in [1, 31, 137] {
        let (actual, full_mix) = if key_lock {
            playback.render(frames, true)
        } else {
            render_varispeed(playback, frames)
        };
        assert_eq!(
            actual, full_mix,
            "B FullMix during own complete-pair preparation"
        );
        assert!(consumer.is_empty());
        assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
        assert_continuity(playback);
    }
    wait_until(|| {
        let ready = Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                "controller.on_frame_render()\nassert not session.stem_generation_errors, session.stem_generation_errors",
            );
            locals
                .bind(py)
                .get_item("controller")
                .unwrap()
                .unwrap()
                .getattr("_committed_residency")
                .unwrap()
                .call_method1("contains", (0,))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        });
        assert!(
            consumer.is_empty(),
            "pair-worker polling grants no audio request"
        );
        ready
    });
    controller_python(
        locals,
        r#"
b_live = controller._committed_residency._live_selections[0]
b_selection = b_live.disk.selection
assert b_selection is not None and b_selection.candidate
assert b_selection.ticket.publication_status() == 'accepted' and b_selection.ticket.is_current()
assert b_selection.ticket.error() is None and b_selection.receipt is None
assert 0 not in controller._pair_preparations.pending
assert 0 not in controller._disk_selection_settlements
assert b_live.desired is None and not b_live.blocked
assert b_live.control is None and b_live.attempt is None and b_live.effective_identity is None
assert not b_live.disk.prepared.has_components()
assert b_live.disk.source_ticket.same_preparation_context(b_source)
b_disk = project.stem_cache[0].pair
assert b_disk is not None and b_disk != disk_v1 and b_disk != disk_v2
assert project.stem_cache[0].source_version == b_version
assert project.sample_paths[0] == b_path and project.pad_content[0] == b_content
b_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert b_reopened.load_error is None and b_reopened.project.stem_cache[0].pair == b_disk
assert b_reopened.project.pad_stem_mix_mode[0] == 'full_mix'
assert b_reopened.project.sample_paths[0] == b_path and b_reopened.project.pad_content[0] == b_content
assert b_reopened.project.pad_key_lock[0] is (True if b_stem_key_lock else None)
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == b_disk.stem_set_identity
assert status.control_stem_set_identity is None and status.effective_stem_set_identity is None
assert status.residency is None and status.effective is None
assert audio.worker_call_count() == b_worker_calls + 1 and audio.prepared_only_on_pool_threads()
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
"#,
    );
    assert!(
        consumer.is_empty(),
        "disk commit and pending registration grant no Capture"
    );
    for frames in [31, 257] {
        let (actual, full_mix) = if key_lock {
            playback.render(frames, true)
        } else {
            render_varispeed(playback, frames)
        };
        assert_eq!(
            actual, full_mix,
            "disk-only B selection leaves FullMix productive"
        );
        assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
        assert_continuity(playback);
    }
    controller_python(
        locals,
        r#"
assert controller.set_stem_mix_mode(0, 'all_stems')
b_candidate = b_live.attempt.receipt
assert b_candidate is not source_candidate and b_candidate is not candidate
assert b_candidate.is_current() and not b_candidate.claimed()
assert b_candidate.effective_mode_status() == 'waiting'
for _ in range(16):
    controller.on_frame_render()
assert b_candidate.effective_mode_status() == 'waiting' and not b_candidate.claimed()
assert b_live.control is None and b_live.effective_identity is None
"#,
    );
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), CommittedStemRtWork::Capture(_)))
    );
    own_adoption(playback, consumer, locals, "b_candidate", key_lock);
    let stem_ack_frame = playback.frame;
    assert!(consumer.is_empty());
    assert_continuity(playback);
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .native_state_address(),
        native
    );
    let resident = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        resident.source_version_hash,
        crate::audio_engine::stem_cache::source_version_hash(&version)
    );
    for (actual, expected) in resident.stems.iter().zip(&components) {
        assert_eq!(
            (actual.resident_start(), actual.resident_end()),
            (LOOP_START, LOOP_END)
        );
        assert_eq!(
            actual.samples.as_ref(),
            &expected.samples[LOOP_START * 2..LOOP_END * 2]
        );
    }
    let oracle_hash = crate::audio_engine::stem_cache::source_version_hash(&version);
    assert_eq!(
        playback.reference.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .source_version_hash,
        oracle_hash
    );
    // Independent B components were declared before the oracle's birth.
    // Only scalar selection changes now; its voice, phase and filters continue.
    assert!(
        playback
            .reference
            .set_stem_enabled_mask(0, 0b1010, oracle_hash)
    );
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::AllStems, oracle_hash)
    );
    playback.components = components;
    playback.select(Some(0b1010));
    controller_python(
        locals,
        r#"
controller.on_frame_render()
status = controller.stem_residency_status(0)
assert status.control_stem_set_identity == b_disk.stem_set_identity
assert status.effective_stem_set_identity is None and status.effective == 'transitioning'
assert b_candidate.claimed() and not b_candidate.withdraw()
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
"#,
    );
    let ramp_start = playback.source_frame;
    if key_lock {
        settle_wet_after(playback, locals, "b_candidate", Some(stem_ack_frame));
        assert_ne!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        assert!(
            voice(&playback.callback.mixer)
                .stretch
                .wet_current_source_output_ready()
        );
        let adopted = voice(&playback.callback.mixer).stretch.adopted_request_id();
        assert!(adopted.is_some() && adopted != full_mix_adoption);
    } else {
        settle_varispeed(playback, locals, "b_candidate");
    }
    assert!(playback.source_frame - ramp_start > 103);
    assert_continuity(playback);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        (StemMixMode::AllStems, 0b1010, oracle_hash)
    );
    controller_python(
        locals,
        r#"
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == b_disk.stem_set_identity
assert status.control_stem_set_identity == b_disk.stem_set_identity
assert status.effective_stem_set_identity == b_disk.stem_set_identity and status.effective == 'effective'
assert b_candidate.claimed() and b_candidate.is_current() and b_candidate.error() is None
assert b_candidate.effective_failure_reason() is None
assert source_candidate.residency_status() == 'rejected' and not source_candidate.claimed()
assert not source_candidate.is_current()
assert source_candidate.error() == 'Committed stem residency or mode ownership changed'
assert source_candidate.effective_failure_reason() is None
assert not session.stem_generation_errors
assert audio.worker_call_count() == b_worker_calls + 1
persistence.flush()
b_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert b_reopened.load_error is None and b_reopened.project.stem_cache[0].pair == b_disk
assert b_reopened.project.stem_cache[0].source_version == b_version
assert b_reopened.project.sample_paths[0] == b_path and b_reopened.project.pad_content[0] == b_content
assert b_reopened.project.pad_stem_mix_mode[0] == 'all_stems'
assert b_reopened.project.pad_key_lock[0] is (True if b_stem_key_lock else None)
assert not b_reopened.project.key_lock and not b_reopened.project.resolve_pad_key_lock(215)
"#,
    );
    for frames in [1, 31, 137, 511] {
        if key_lock {
            playback.render(frames, true);
        } else {
            render_varispeed(playback, frames);
        }
        assert_continuity(playback);
        controller_python(
            locals,
            "controller.on_frame_render()\nassert audio.worker_call_count() == b_worker_calls + 1\nassert not session.stem_generation_errors\nassert not source_candidate.claimed()\nassert b_candidate.effective_mode_status() == 'effective'",
        );
        assert!(consumer.is_empty());
    }
}

fn public_window_then_explicit_retry(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) -> SampleBuffer {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert!(!playback.scheduled && playback.rate_ledger.is_none());
    assert_eq!(
        playback.drain(consumer),
        1,
        "actual V2 Capture before public window intent"
    );
    wait_until(|| consumer.peek().is_ok());
    let queued = match consumer.peek().unwrap() {
        ControlMessage::CommittedStemWork(work) => {
            assert!(
                matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
                if transaction.id == 0 && transaction.mask == 0b1010 && transaction.candidate.is_some())
            );
            work.as_ref() as *const CommittedStemRtWork as usize
        }
        _ => panic!("expected actual queued V2 Adopt"),
    };
    playback.callback.retirement.slots = 2;
    assert_eq!(playback.drain(consumer), 0);
    let source = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let old_set = playback.callback.mixer.stems_for_measurement()[0].clone();
    let before = voice(&playback.callback.mixer);
    let old_clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let demand = playback.callback.mixer.stem_demand_for_measurement(0);
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("window_key_lock", key_lock)
            .unwrap()
    });
    controller_python(
        locals,
        r#"
from flitzis_looper.controller.transport import TransportController
class WindowTransportAudio:
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('finite window edit must not retrigger')
    play_sample_exclusive = play_sample
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = 17 / fixture['rate']
project.pad_loop_end_s[0] = 1517 / fixture['rate']
project.pad_key_lock[0] = window_key_lock
transport = TransportController(project, session, WindowTransportAudio())
failed_window = candidate
window_selection = live.disk.selection
window_registration = window_selection.ticket
window_ownership_error = 'Committed stem residency or mode ownership changed'
window_worker_calls = audio.worker_call_count()
assert failed_window.is_current() and not failed_window.claimed()
transport.loop.set_end(0, 1581 / fixture['rate'])
ready_window = transport.residency._pending[0].ticket
assert ready_window is not None
assert not failed_window.claimed()
"#,
    );
    wait_until(|| {
        controller_ticket_status(locals, "ready_window", "publication_status") == "pending"
            && consumer.slots() == 2
    });
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if work.as_ref() as *const CommittedStemRtWork as usize == queued)
    );
    assert_eq!(consumer.slots(), 2);
    controller_python(
        locals,
        "assert not failed_window.is_current() and not failed_window.claimed()",
    );
    assert_eq!(playback.drain(consumer), 0);
    let admitted = voice(&playback.callback.mixer);
    assert!(admitted.source_playback.matches_exact(&old_clock));
    assert_eq!(admitted.generation, generation);
    assert_eq!(admitted.stretch.native_state_address(), native);
    assert_eq!(admitted.stretch.pending_fifo_frames(), fifo);
    assert!(Arc::ptr_eq(
        &source.samples,
        &admitted.sample.as_ref().unwrap().samples
    ));
    assert_eq!(history.is_some(), key_lock);
    if let Some(history) = history {
        let next = admitted.stretch.productive_history().unwrap();
        assert_eq!(next.binding, history.binding);
        assert_eq!(next.next_position, history.next_position);
        assert_eq!(next.fed_output_frames, history.fed_output_frames);
    }
    let next_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(next_dsp.binding, dsp.binding);
    assert_eq!(next_dsp.next_position, dsp.next_position);
    assert_eq!(next_dsp.fed_output_frames, dsp.fed_output_frames);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    controller_python(
        locals,
        r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.attempt is window_selection and live.control is previous_control
    assert failed_window.error() == window_ownership_error and failed_window.effective_failure_reason() is None
    assert not failed_window.claimed() and not failed_window.is_current()
    assert session.stem_generation_errors == {0: window_ownership_error}
    assert audio.worker_call_count() == window_worker_calls
"#,
    );
    for frames in [1, 31, 137, 257] {
        let output = if key_lock {
            playback.render(frames, false)
        } else {
            render_varispeed(playback, frames)
        }
        .0;
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
    }
    // Choose an interior ACK position from the independent original geometry.
    // This proves an expansion without invoking the separate source-cut domain.
    let phase = (playback.source_frame as f64 * playback.ratio).rem_euclid(1500.0);
    if !(256.0..768.0).contains(&phase) {
        let frames = ((256.0 - phase).rem_euclid(1500.0) / playback.ratio).ceil() as usize;
        if key_lock {
            playback.render(frames, false);
        } else {
            render_varispeed(playback, frames);
        }
    }
    let phase = (playback.source_frame as f64 * playback.ratio).rem_euclid(1500.0);
    assert!((256.0..768.0).contains(&phase));
    let before_ack = voice(&playback.callback.mixer);
    let fifo = before_ack.stretch.pending_fifo_frames();
    let history = before_ack.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if work.as_ref() as *const CommittedStemRtWork as usize == queued)
    );
    playback.callback.retirement.slots = usize::MAX;
    assert_eq!(
        playback.drain(consumer),
        2,
        "normal old Adopt rejection then changed Window ACK"
    );
    assert!(consumer.is_empty());
    let current = voice(&playback.callback.mixer);
    assert_eq!(current.generation, generation);
    assert_eq!(
        current.source_playback.position().frame,
        LOOP_START + phase.floor() as usize
    );
    assert_eq!(
        current.source_playback.position().fraction.to_bits(),
        phase.fract().to_bits()
    );
    assert_eq!(
        current.source_playback.tempo_ratio().to_bits(),
        playback.ratio.to_bits()
    );
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let next = current.stretch.productive_history().unwrap();
        assert_eq!(next.binding, history.binding);
        assert_eq!(next.next_position, history.next_position);
        assert_eq!(next.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(current.stretch.productive_history().is_none());
    }
    let next_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(next_dsp.binding, dsp.binding);
    assert_eq!(next_dsp.next_position, dsp.next_position);
    assert_eq!(next_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    let changed = playback.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(!Arc::ptr_eq(&source.samples, &changed.samples));
    assert!(source.same_source(&changed));
    assert_eq!(changed.resident_start(), LOOP_START);
    assert_eq!(changed.resident_end(), 1581);
    assert_eq!(changed.window_revision(), source.window_revision() + 1);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0),
        demand
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    match (
        old_set.as_ref(),
        playback.callback.mixer.stems_for_measurement()[0].as_ref(),
    ) {
        (Some(old), Some(next)) => {
            assert!(Arc::ptr_eq(
                &old.complete_set_identity,
                &next.complete_set_identity
            ));
            for (old, next) in old.stems.iter().zip(&next.stems) {
                assert!(!Arc::ptr_eq(&old.samples, &next.samples));
                assert!(next.same_window(&changed));
            }
        }
        (None, None) => {}
        _ => panic!("window ACK cannot select the rejected V2"),
    }
    Python::attach(|py| {
        let ready = locals.bind(py).get_item("ready_window").unwrap().unwrap();
        let ready = ready
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        let read = ready
            .read_observation_for_test()
            .expect("changed window needs genuine range reads");
        assert_eq!(
            (read.source.start_frame, read.source.end_frame),
            (LOOP_START, 1581)
        );
        assert_eq!(read.source.read_bytes, 1564 * 2 * 4);
        assert_eq!(
            read.stems.read_bytes.iter().any(|bytes| *bytes > 0),
            old_set.is_some()
        );
    });
    let epoch = playback.source_frame;
    playback.loop_end = 1581;
    playback.loop_period = 1564.0;
    playback.rate_ledger = Some(ControllerRateLedger {
        period: 1564.0,
        epochs: vec![
            ControllerRateEpoch {
                start: 0,
                whole: 0,
                fraction: 0.0,
                rate: playback.ratio,
                distance: 0.0,
                period: 1500.0,
            },
            ControllerRateEpoch {
                start: epoch,
                whole: phase.floor() as usize,
                fraction: phase.fract(),
                rate: playback.ratio,
                distance: epoch as f64 * playback.ratio,
                period: 1564.0,
            },
        ],
    });
    playback.resumed_at = Some(playback.frame);
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        reference.set_loop_region_frames(0, LOOP_START, Some(1581));
    }
    controller_python(
        locals,
        r#"
assert failed_window.residency_status() == 'rejected' and not failed_window.claimed()
assert ready_window.publication_status() == 'accepted'
transport.on_frame_render()
controller.on_frame_render()
assert live.blocked and live.attempt is window_selection
assert session.stem_generation_errors == {0: window_ownership_error}
assert audio.worker_call_count() == window_worker_calls
fresh_window_source = audio.capture_prepared_source(0, fixture['source_version'])
assert source_v2.same_source_request(fresh_window_source)
assert not source_v2.same_preparation_context(fresh_window_source)
assert fresh_window_source.same_preparation_context(fresh_window_source)
foreign_window_source = audio.capture_prepared_source(215, fixture['source_version'])
assert not source_v2.same_source_request(foreign_window_source)
assert not source_v2.same_preparation_context(foreign_window_source)
assert not controller._committed_residency.requires_current_preparation(0, foreign_window_source)
assert controller._committed_residency.requires_current_preparation(0, fresh_window_source)
assert live.control is previous_control and live.attempt is window_selection
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.attempt is window_selection
    assert audio.worker_call_count() == window_worker_calls
"#,
    );
    if key_lock {
        playback.render(13, true);
        let output = playback.adopt_native(true).0;
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
    } else {
        for frames in [1, 31, 137, 257, 511, 777] {
            render_varispeed(playback, frames);
        }
    }
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    controller_python(
        locals,
        r#"
# Actual explicit ALL, without privately refreshing the candidate beforehand.
assert controller.set_stem_mix_mode(0, 'all_stems'), session.stem_generation_errors
window_error = 'Stem residency admission failed: stale or foreign prepared source ticket'
assert session.stem_generation_errors == {0: window_error}
requested_window = controller._requested_preparations[0].spec
assert requested_window.residency_only
assert requested_window.ticket.same_preparation_context(fresh_window_source)
assert requested_window.previous_entry is project.stem_cache[0]
assert 0 not in controller._pair_preparations.pending
controller.on_frame_render()
request = controller._pair_preparations.pending[0]
assert 0 not in controller._requested_preparations
assert request.ticket is requested_window.ticket and request.entry is requested_window.entry
assert request.residency_only and request.ticket.same_preparation_context(fresh_window_source)
assert source_v2.same_source_request(request.ticket) and not source_v2.same_preparation_context(request.ticket)
assert request.entry.pair == disk_v2
assert live.blocked and live.attempt is None and live.control is previous_control
"#,
    );
    wait_until(|| {
        controller_python(
            locals,
            "controller.on_frame_render()\nassert session.stem_generation_errors == {0: window_error}",
        );
        Python::attach(|py| {
            let live = locals.bind(py).get_item("live").unwrap().unwrap();
            if live.getattr("attempt").unwrap().is_none() {
                return false;
            }
            worker_python(
                py,
                locals.bind(py),
                r#"
candidate = live.attempt.receipt
fresh_window_selection = live.disk.selection
fresh_window_registration = fresh_window_selection.ticket
assert live.control is previous_control and not live.blocked
assert fresh_window_selection is not window_selection and fresh_window_registration is not window_registration
assert candidate is not failed_window and candidate.is_current() and not candidate.claimed()
assert candidate.effective_mode_status() == 'waiting'
assert audio.worker_call_count() == window_worker_calls + 1 and audio.prepared_only_on_pool_threads()
assert live.disk.source_ticket.same_preparation_context(fresh_window_source)
assert project.stem_cache[0].pair == disk_v2
assert StemPairSelection.model_validate_json(live.disk.prepared.selection_json()) == disk_v2
assert not failed_window.is_current() and not failed_window.claimed()
assert failed_window.error() == window_ownership_error and failed_window.effective_failure_reason() is None
"#,
            );
            true
        })
    });
    assert_eq!(
        consumer.slots(),
        1,
        "fresh own Capture only after worker preparation"
    );
    changed
}

fn public_play_then_explicit_retry(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) -> (u64, u64, u64) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert!(!playback.scheduled && playback.rate_ledger.is_none());
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    assert_eq!(playback.drain(consumer), 1, "actual old-voice V2 Capture");
    wait_until(|| consumer.peek().is_ok());
    let queued = match consumer.peek().unwrap() {
        ControlMessage::CommittedStemWork(work) => {
            assert!(
                matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
                if transaction.id == 0 && transaction.mask == 0b1010 && transaction.candidate.is_some())
            );
            work.as_ref() as *const CommittedStemRtWork as usize
        }
        _ => panic!("expected genuine old-voice V2 Adopt"),
    };
    playback.callback.retirement.slots = 2;
    assert_eq!(playback.drain(consumer), 0);
    controller_python(
        locals,
        r#"
failed_play = candidate
play_selection = live.disk.selection
play_registration = play_selection.ticket
play_error = 'Committed stem residency or mode ownership changed'
assert failed_play.is_current() and not failed_play.claimed()
assert failed_play.residency_status() == 'pending' and failed_play.effective_mode_status() == 'waiting'
"#,
    );
    let old_generation = voice(&playback.callback.mixer).generation;
    for stage in 0..2 {
        let source = playback.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let set = playback.callback.mixer.stems_for_measurement()[0].clone();
        let before = voice(&playback.callback.mixer);
        let clock = before.source_playback;
        let native = before.stretch.native_state_address();
        let fifo = before.stretch.pending_fifo_frames();
        let history = before.stretch.productive_history();
        let dsp = playback
            .callback
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap();
        let demand = playback.callback.mixer.stem_demand_for_measurement(0);
        let transition = playback.callback.mixer.stem_transition_for_measurement(0);
        let other_clock = other_voice(&playback.callback.mixer).source_playback;
        if stage == 0 {
            Python::attach(|py| locals.bind(py).set_item("play_key_lock", key_lock).unwrap());
            controller_python(
                locals,
                r#"
from flitzis_looper.controller.transport import TransportController
class ReadyTransportAudio(CapturedTransportAudio):
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('ordinary Play must use its own resident window ACK')
    play_sample_exclusive = play_sample
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = 17 / fixture['rate']
project.pad_loop_end_s[0] = 1517 / fixture['rate']
project.pad_key_lock[0] = play_key_lock
transport = TransportController(project, session, ReadyTransportAudio())
ordinary_timestamp = captured_audio.capture_input_timestamp_ns()
transport.playback.trigger_pad_keep_others(0, received_at_ns=ordinary_timestamp)
ordinary_launch = next(iter(transport.residency._captured_starts.values()))
ordinary_start = ordinary_launch.start
ready_play = ordinary_launch.ticket
assert ready_play is not None and ready_play.publication_status() == 'pending'
assert failed_play.residency_status() == 'pending' and failed_play.effective_mode_status() == 'rejected'
assert not failed_play.is_current() and not failed_play.claimed()
assert play_registration.publication_status() == 'accepted' and not play_registration.is_current()
assert play_registration.error() is None
"#,
            );
            assert_eq!(
                playback.drain(consumer),
                0,
                "old Adopt still holds same-window Play ACK behind it"
            );
            assert!(
                matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
                if work.as_ref() as *const CommittedStemRtWork as usize == queued)
            );
            assert_eq!(consumer.slots(), 3);
        } else {
            assert!(
                matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
                if work.as_ref() as *const CommittedStemRtWork as usize == queued)
            );
            playback.callback.retirement.slots = usize::MAX;
            synchronize_callback_frame(&mut playback.callback, playback.frame);
            let capture_frame = playback.frame;
            assert_eq!(
                playback.drain(consumer),
                3,
                "unchanged old Adopt rejects before original target and real same-window ACK"
            );
            assert_eq!(
                ordinary_start_from_python(locals).original_frame(),
                Some(capture_frame)
            );
            assert!(consumer.is_empty());
            controller_python(
                locals,
                "assert failed_play.residency_status() == 'rejected'\nassert ready_play.publication_status() == 'accepted'",
            );
            Python::attach(|py| {
                let ready = locals.bind(py).get_item("ready_play").unwrap().unwrap();
                let ready = ready.extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>().unwrap();
                assert!(
                    ready.read_observation_for_test().is_none(),
                    "Play reuses the exact ready window"
                );
            });
        }
        let after = voice(&playback.callback.mixer);
        assert!(after.source_playback.matches_exact(&clock));
        assert_eq!(after.generation, old_generation);
        assert_eq!(after.stretch.native_state_address(), native);
        assert_eq!(after.stretch.pending_fifo_frames(), fifo);
        assert_eq!(history.is_some(), key_lock);
        if let Some(history) = history {
            let next = after.stretch.productive_history().unwrap();
            assert_eq!(next.binding, history.binding);
            assert_eq!(next.next_position, history.next_position);
            assert_eq!(next.fed_output_frames, history.fed_output_frames);
        } else {
            assert!(after.stretch.productive_history().is_none());
        }
        let next_dsp = playback
            .callback
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap();
        assert_eq!(next_dsp.binding, dsp.binding);
        assert_eq!(next_dsp.next_position, dsp.next_position);
        assert_eq!(next_dsp.fed_output_frames, dsp.fed_output_frames);
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0),
            demand
        );
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .matches_exact(transition)
        );
        assert!(
            other_voice(&playback.callback.mixer)
                .source_playback
                .matches_exact(&other_clock)
        );
        let current_source = playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(&source.samples, &current_source.samples));
        assert_eq!(source.resident_binding(), current_source.resident_binding());
        match (
            set.as_ref(),
            playback.callback.mixer.stems_for_measurement()[0].as_ref(),
        ) {
            (Some(old), Some(current)) => {
                assert!(Arc::ptr_eq(
                    &old.complete_set_identity,
                    &current.complete_set_identity
                ));
                for (old, next) in old.stems.iter().zip(&current.stems) {
                    assert!(Arc::ptr_eq(&old.samples, &next.samples));
                }
            }
            (None, None) => {}
            _ => panic!("Play admission/ACK installed rejected V2 readers"),
        }
        controller_python(
            locals,
            r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.attempt is play_selection and live.control is previous_control
    assert live.disk.selection is play_selection and play_selection.receipt is failed_play
    assert not failed_play.claimed() and not failed_play.is_current()
    assert failed_play.effective_mode_status() == 'rejected' and failed_play.error() == play_error
    assert failed_play.effective_failure_reason() is None
    assert play_registration.publication_status() == 'accepted' and not play_registration.is_current()
    assert previous_control.ticket.is_current()
    if selected_v1:
        assert not first.is_current()
    else:
        assert previous_control.receipt is None
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if selected_v1 else None)
    assert status.effective is None and session.stem_generation_errors == {0: play_error}
"#,
        );
        assert_eq!(
            consumer.slots(),
            if stage == 0 { 3 } else { 0 },
            "stem polling cannot retry or launch Play"
        );
        for frames in [1, 31, 137, 257] {
            let output = if key_lock {
                playback.render(frames, false)
            } else {
                render_varispeed(playback, frames)
            }
            .0;
            assert!(output.iter().any(|sample| sample.abs() > 0.001));
            assert_eq!(voice(&playback.callback.mixer).generation, old_generation);
        }
    }
    synchronize_callback_frame(&mut playback.callback, playback.frame);
    let launch_timeline = playback.callback.transport;
    let start = admit_original_public_start(&mut playback.callback, consumer, locals);
    let birth = start.execution_frame().unwrap();
    playback.scheduled = true;
    let original_start = start.original_frame().unwrap();
    assert!(original_start < launch_timeline.output_frame());
    let launch = LoopPhaseReference::capture(Some(launch_timeline), original_start);
    while playback.frame < birth {
        let frames = (birth - playback.frame).min(257) as usize;
        let output = if key_lock {
            playback.render(frames, true)
        } else {
            render_varispeed(playback, frames)
        }
        .0;
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
        assert_eq!(voice(&playback.callback.mixer).generation, old_generation);
        assert!(start.effective_frame().is_none());
    }
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    execute_public_start_at_frame(&mut playback.callback, &start);
    playback.scheduled = false;
    assert!(consumer.is_empty());
    let new_voice = voice(&playback.callback.mixer);
    let generation = new_voice.generation;
    assert_ne!(generation, old_generation);
    assert!(birth >= original_start);
    assert_eq!(new_voice.source_playback.position().frame, LOOP_START);
    assert_eq!(
        new_voice.source_playback.position().fraction.to_bits(),
        0.0_f64.to_bits()
    );
    assert!(
        new_voice
            .source_playback
            .phase_reference_current(&launch_timeline)
    );
    assert!(!new_voice.launch_qualified);
    assert!(
        other_voice(&playback.callback.mixer)
            .source_playback
            .matches_exact(&other_clock)
    );
    // The predecessor remains audible until actual E. Every independent new
    // reference starts its complete selected beginning at that same birth.
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        assert!(reference.play_sample_at_output_frame(0, 1.0, birth));
        install_public_start_reference(reference, launch, &launch_timeline);
        if key_lock {
            prepare_public_start_reference(reference, birth);
        }
    }
    playback.source_frame = 0;
    playback.resumed_at = Some(birth);
    playback.previous = playback.selection;
    playback.ramp_start = 0;
    playback.raw = key_lock.then(|| prepared_public_start_raw(playback));
    playback.candidate = None;
    playback.filter = controller_filter();
    playback.advance_expected_birth(original_start, birth);
    if key_lock {
        assert_eq!(
            voice(&playback.callback.mixer).stretch.adopted_request_id(),
            Some(start.sequence())
        );
        render_physical_birth_prefix(playback);
    } else {
        for frames in [1, 31, 137, 257, 511] {
            render_varispeed(playback, frames);
        }
    }
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    controller_python(
        locals,
        r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.blocked and live.attempt is play_selection
    assert failed_play.residency_status() == 'rejected' and not failed_play.claimed()
    assert controller.stem_residency_status(0).effective is None
    assert session.stem_generation_errors == {0: play_error}
assert controller.set_stem_mix_mode(0, 'all_stems')
fresh_play_selection = live.disk.selection
fresh_play_registration = fresh_play_selection.ticket
candidate = live.attempt.receipt
assert fresh_play_selection is not play_selection and fresh_play_registration is not play_registration
assert fresh_play_registration.is_current() and fresh_play_registration.publication_status() == 'accepted'
assert candidate is not failed_play and candidate.is_current() and not candidate.claimed()
assert candidate.effective_mode_status() == 'waiting' and not live.blocked
assert live.control is previous_control
assert session.stem_generation_errors == {0: play_error}
"#,
    );
    (original_start, birth, generation)
}

fn supersede_queued_candidate_mask(
    producer: &Arc<Mutex<rtrb::Producer<ControlMessage>>>,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
) {
    use crate::audio_engine::committed_stem_residency::CommittedStemRtWork;
    assert_eq!(playback.drain(consumer), 1, "actual old V2 Capture");
    wait_until(|| consumer.peek().is_ok());
    let queued = match consumer.peek().unwrap() {
        ControlMessage::CommittedStemWork(work) => {
            assert!(
                matches!(work.as_ref(), CommittedStemRtWork::Adopt(transaction)
                if transaction.id == 0 && transaction.target == StemMixMode::AllStems
                && transaction.mask == 0b1010 && transaction.candidate.is_some())
            );
            work.as_ref() as *const CommittedStemRtWork as usize
        }
        _ => panic!("expected actual prepared old V2 Adopt"),
    };
    playback.callback.retirement.slots = 2;
    assert_eq!(
        playback.drain(consumer),
        0,
        "real preclaim retirement pressure"
    );
    controller_python(
        locals,
        r#"
superseded = candidate
superseded_selection = live.disk.selection
superseded_registration = superseded_selection.ticket
assert superseded.is_current() and not superseded.claimed()
assert superseded.residency_status() == 'pending' and superseded.effective_mode_status() == 'waiting'
assert superseded_selection.receipt_intent == ('all_stems', 0b1010)
assert superseded_registration.is_current() and superseded_registration.publication_status() == 'accepted'
"#,
    );
    // Check public admission and the normal late-Adopt/new-Capture batch against
    // their own current snapshots. Rendering between them keeps the old actual
    // selection productive, with its independent PCM/Native/EQ oracle unchanged.
    for stage in 0..2 {
        let source = playback.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        let predecessor = playback.callback.mixer.stems_for_measurement()[0].clone();
        let before = voice(&playback.callback.mixer);
        let clock = before.source_playback;
        let generation = before.generation;
        let native = before.stretch.native_state_address();
        let fifo = before.stretch.pending_fifo_frames();
        let history = before.stretch.productive_history();
        let epoch = before.stretch.source_epoch_owner().load(Ordering::Acquire);
        assert_eq!(history.is_some(), key_lock);
        let demand = playback.callback.mixer.stem_demand_for_measurement(0);
        let transition = playback.callback.mixer.stem_transition_for_measurement(0);
        assert!(!transition.is_active());
        let dsp = playback
            .callback
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap();
        let other_clock = other_voice(&playback.callback.mixer).source_playback;
        if stage == 0 {
            controller_python(
                locals,
                r#"
assert controller.set_stem_enabled_mask(0, 0b0101)
latest_selection = live.disk.selection
fresh_registration = latest_selection.ticket
candidate = live.attempt.receipt
assert latest_selection is not superseded_selection
assert fresh_registration is not superseded_registration
assert fresh_registration.is_current() and fresh_registration.publication_status() == 'accepted'
assert candidate is not superseded and candidate.is_current() and not candidate.claimed()
assert latest_selection.receipt_intent == ('all_stems', 0b0101)
assert project.pad_stem_mix_mode[0] == 'all_stems' and project.stem_cache[0].pair == disk_v2
persistence.flush()
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[0].pair == disk_v2
assert reopened.project.pad_stem_mix_mode[0] == 'all_stems'
"#,
            );
            assert_eq!(
                playback.drain(consumer),
                0,
                "pressure retains old Adopt before new Capture"
            );
            assert!(
                matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
                if work.as_ref() as *const CommittedStemRtWork as usize == queued)
            );
            assert_eq!(
                consumer.slots(),
                2,
                "only old Adopt and new own Capture queued"
            );
        } else {
            // Admission already succeeded. The callback never takes this mutex;
            // holding it here prevents the fresh worker from posting its Adopt
            // inside the old-Adopt/new-Capture batch. No Python call or wait runs
            // under the guard, and no queued message is popped or reordered.
            let guard = producer.lock().unwrap();
            assert!(
                matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
                if work.as_ref() as *const CommittedStemRtWork as usize == queued)
            );
            playback.callback.retirement.slots = usize::MAX;
            assert_eq!(
                playback.drain(consumer),
                2,
                "normal old Adopt rejection then own latest Capture"
            );
            assert!(consumer.is_empty());
            drop(guard);
        }
        let after = voice(&playback.callback.mixer);
        assert!(after.source_playback.matches_exact(&clock));
        assert_eq!(after.generation, generation);
        assert_eq!(after.stretch.native_state_address(), native);
        assert_eq!(after.stretch.pending_fifo_frames(), fifo);
        assert_eq!(
            after.stretch.source_epoch_owner().load(Ordering::Acquire),
            epoch
        );
        if let Some(history) = history {
            let next = after.stretch.productive_history().unwrap();
            assert_eq!(next.binding, history.binding);
            assert_eq!(next.next_position, history.next_position);
            assert_eq!(next.fed_output_frames, history.fed_output_frames);
        } else {
            assert!(after.stretch.productive_history().is_none());
        }
        assert!(
            other_voice(&playback.callback.mixer)
                .source_playback
                .matches_exact(&other_clock)
        );
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0),
            demand
        );
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .matches_exact(transition)
        );
        let next_dsp = playback
            .callback
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap();
        assert_eq!(next_dsp.binding, dsp.binding);
        assert_eq!(next_dsp.next_position, dsp.next_position);
        assert_eq!(next_dsp.fed_output_frames, dsp.fed_output_frames);
        let current_source = playback.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(&source.samples, &current_source.samples));
        assert_eq!(source.resident_binding(), current_source.resident_binding());
        match (
            predecessor.as_ref(),
            playback.callback.mixer.stems_for_measurement()[0].as_ref(),
        ) {
            (Some(old), Some(current)) => {
                assert!(Arc::ptr_eq(
                    &old.complete_set_identity,
                    &current.complete_set_identity
                ));
                for (old, next) in old.stems.iter().zip(&current.stems) {
                    assert!(Arc::ptr_eq(&old.samples, &next.samples));
                }
            }
            (None, None) => {}
            _ => panic!("superseded Adopt or fresh Capture replaced actual readers"),
        }
        controller_python(
            locals,
            r#"
for _ in range(16):
    controller.on_frame_render()
    assert live.control is previous_control and live.attempt is latest_selection
    assert live.disk.selection is latest_selection and latest_selection.ticket is fresh_registration
    assert latest_selection.receipt is candidate and candidate.is_current() and not candidate.claimed()
    assert candidate.residency_status() in {'captured', 'pending'} and candidate.effective_mode_status() == 'waiting'
    assert live.desired == ('all_stems', 0b0101) and project.pad_stem_intent[0].enabled_mask == 0b0101
    assert not superseded.is_current() and not superseded.claimed()
    assert superseded.residency_status() == 'rejected' and superseded.effective_mode_status() == 'rejected'
    assert superseded.effective_failure_reason() is None
    assert superseded.error() == 'Committed stem residency or mode ownership changed'
    assert superseded_selection.receipt is superseded
    assert not superseded_registration.is_current() and superseded_registration.publication_status() == 'accepted'
    assert superseded_registration.error() is None
    assert previous_control.ticket.is_current()
    if selected_v1:
        assert previous_control.receipt is first and first.is_current() and first.effective_mode_status() == 'effective'
    else:
        assert previous_control.receipt is None
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v1.stem_set_identity
    assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if selected_v1 else None)
    assert status.effective == 'waiting' and not live.blocked
    assert not session.stem_generation_errors, session.stem_generation_errors
"#,
        );
        if stage == 0 {
            assert_eq!(
                consumer.slots(),
                2,
                "polling cannot duplicate latest admission"
            );
            let mut nontrivial = false;
            for frames in [1, 31, 137, 257, 511] {
                let output = if key_lock {
                    playback.render(frames, false)
                } else {
                    render_varispeed(playback, frames)
                }
                .0;
                nontrivial |= output.iter().any(|sample| sample.abs() > 0.001);
            }
            assert!(nontrivial);
        }
    }
    // The normal batch above really dispatched the fresh Capture. Continue only
    // its worker/Adopt half; a second invented Capture would hide FIFO ordering.
}

fn latest_masks_after_nonempty_claim(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    full_stems: &PreparedStemSet,
    predecessor: Option<&PreparedStemSet>,
    key_lock: bool,
) {
    use crate::audio_engine::source_reader::StemRenderSelection;
    let v2 = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    let expected_from = predecessor.map_or(StemRenderSelection::full_mix(), |set| {
        StemRenderSelection::for_stems(set, 0b1010)
    });
    assert_eq!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .required_from_selection(),
        Some(expected_from)
    );
    assert_eq!(playback.selection, Some(0b0011));
    let generation = voice(&playback.callback.mixer).generation;
    let native = voice(&playback.callback.mixer)
        .stretch
        .native_state_address();
    controller_python(
        locals,
        r#"
initial_nonempty = candidate
nonempty_selection = live.control
nonempty_registration = nonempty_selection.ticket
assert live.attempt is nonempty_selection and live.disk.selection is nonempty_selection
assert nonempty_selection.receipt_intent == ('all_stems', 0b1010)
assert initial_nonempty.claimed() and initial_nonempty.is_current()
assert initial_nonempty.effective_mode_status() == 'transitioning'
"#,
    );
    let mut nontrivial = false;
    for (mask, frames) in [(0b0101, 1), (0, 31), (0b0101, 17)] {
        let before = voice(&playback.callback.mixer);
        let clock = before.source_playback;
        let fifo = before.stretch.pending_fifo_frames();
        let history = before.stretch.productive_history();
        let transition = playback.callback.mixer.stem_transition_for_measurement(0);
        let demand = playback.callback.mixer.stem_demand_for_measurement(0);
        let dsp = playback
            .callback
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap();
        Python::attach(|py| locals.bind(py).set_item("latest_mask", mask).unwrap());
        controller_python(
            locals,
            r#"
assert controller.set_stem_enabled_mask(0, latest_mask)
for _ in range(16):
    controller.on_frame_render()
    assert project.pad_stem_intent[0].enabled_mask == latest_mask
    assert live.desired == ('all_stems', latest_mask)
    assert live.control is nonempty_selection and live.attempt is nonempty_selection
    assert live.disk.selection is nonempty_selection
    assert nonempty_selection.ticket is nonempty_registration and nonempty_registration.is_current()
    assert nonempty_selection.receipt is initial_nonempty
    assert nonempty_selection.receipt_intent == ('all_stems', 0b1010)
    assert initial_nonempty.claimed() and initial_nonempty.is_current() and not initial_nonempty.withdraw()
    assert initial_nonempty.effective_mode_status() == 'transitioning'
    assert controller.stem_residency_status(0).effective == 'transitioning'
    assert not session.stem_generation_errors, session.stem_generation_errors
"#,
        );
        assert!(
            consumer.is_empty(),
            "latest intent cannot Capture through occupied handover"
        );
        let after = voice(&playback.callback.mixer);
        assert!(after.source_playback.matches_exact(&clock));
        assert_eq!(after.generation, generation);
        assert_eq!(after.stretch.native_state_address(), native);
        assert_eq!(after.stretch.pending_fifo_frames(), fifo);
        if let Some(history) = history {
            let next = after.stretch.productive_history().unwrap();
            assert_eq!(next.binding, history.binding);
            assert_eq!(next.next_position, history.next_position);
            assert_eq!(next.fed_output_frames, history.fed_output_frames);
        } else {
            assert!(after.stretch.productive_history().is_none());
        }
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .matches_exact(transition)
        );
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0),
            demand
        );
        let next_dsp = playback
            .callback
            .mixer
            .dsp_source_history_for_test(0)
            .unwrap();
        assert_eq!(next_dsp.binding, dsp.binding);
        assert_eq!(next_dsp.next_position, dsp.next_position);
        assert_eq!(next_dsp.fed_output_frames, dsp.fed_output_frames);
        assert_eq!(demand.1, 0b1010);
        let output = if key_lock {
            playback.render(frames, false)
        } else {
            render_varispeed(playback, frames)
        }
        .0;
        nontrivial |= output.iter().any(|sample| sample.abs() > 0.001);
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
        assert_eq!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .required_from_selection(),
            Some(expected_from)
        );
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
    }
    assert!(
        nontrivial && (playback.source_frame - playback.ramp_start) as f64 * playback.ratio < 128.0
    );
    // Complete the original source-set ramp, not another128 frames after the
    // already proved partial interval. Only its own actual effect releases busy.
    if key_lock {
        settle_wet(playback, locals, "initial_nonempty");
    } else {
        settle_varispeed(playback, locals, "initial_nonempty");
    }
    controller_python(
        locals,
        r#"
candidate = nonempty_selection.receipt
assert candidate is not initial_nonempty
assert live.control is nonempty_selection and live.attempt is nonempty_selection
assert live.disk.selection is nonempty_selection
assert nonempty_selection.ticket is nonempty_registration and nonempty_registration.is_current()
assert nonempty_registration.publication_status() == 'accepted'
assert nonempty_selection.receipt_intent == ('all_stems', 0b0101)
assert live.desired == ('all_stems', 0b0101)
assert project.pad_stem_intent[0].enabled_mask == 0b0101
assert candidate.is_current() and not candidate.claimed()
assert candidate.residency_status() in {'captured', 'pending'}
assert candidate.effective_mode_status() == 'waiting'
assert initial_nonempty.is_current()
assert initial_nonempty.effective_failure_reason() is None
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert status.effective == 'waiting'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(
        consumer.peek().is_ok(),
        "latest authorized mask has its own actual Capture"
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).1,
        0b1010
    );
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert_eq!(playback.drain(consumer), 1, "latest mask's own Capture");
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, "candidate", "residency_status") == "rejected"
    });
    assert!(
        matches!(consumer.peek(), Ok(ControlMessage::CommittedStemWork(work))
        if matches!(work.as_ref(), crate::audio_engine::committed_stem_residency::CommittedStemRtWork::Adopt(transaction)
            if transaction.id == 0 && transaction.mask == 0b0101))
    );
    controller_python(
        locals,
        r#"
assert candidate.residency_status() == 'pending' and candidate.is_current() and not candidate.claimed()
assert not initial_nonempty.is_current()
assert initial_nonempty.effective_failure_reason() is None
"#,
    );
    own_adoption_after_capture(playback, consumer, locals, "candidate", key_lock);
    let current = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &v2.complete_set_identity,
        &current.complete_set_identity
    ));
    for (old, next) in v2.stems.iter().zip(&current.stems) {
        assert!(Arc::ptr_eq(&old.samples, &next.samples));
    }
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).1,
        0b0101
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    // V2 files swap vocals/drums: mask0101 selects original drums+bass1100.
    assert!(
        playback
            .reference
            .set_stem_enabled_mask(0, 0b1100, full_stems.source_version_hash)
    );
    playback.select(Some(0b1100));
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert candidate.claimed() and candidate.is_current() and not candidate.withdraw()
assert candidate.effective_mode_status() == 'transitioning'
assert controller.stem_residency_status(0).effective == 'transitioning'
assert nonempty_selection.ticket is nonempty_registration
"#,
    );
    if key_lock {
        settle_wet(playback, locals, "candidate");
    } else {
        settle_varispeed(playback, locals, "candidate");
    }
    controller_python(
        locals,
        r#"
assert live.attempt is None and live.control is nonempty_selection
assert live.disk.selection is nonempty_selection
assert nonempty_selection.ticket is nonempty_registration and nonempty_registration.is_current()
assert nonempty_selection.receipt is candidate
assert nonempty_selection.receipt_intent == ('all_stems', 0b0101)
assert candidate.is_current() and candidate.effective_mode_status() == 'effective'
status = controller.stem_residency_status(0)
assert status.registration == 'accepted' and status.residency == 'accepted' and status.effective == 'effective'
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert project.stem_cache[0].pair == disk_v2 and live.desired == ('all_stems', 0b0101)
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(consumer.is_empty());
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
}

fn repeated_sets_after_empty_claim(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    full_stems: &PreparedStemSet,
    predecessor: &PreparedStemSet,
    key_lock: bool,
) {
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    assert_eq!(
        transition.required_from_selection(),
        Some(
            crate::audio_engine::source_reader::StemRenderSelection::for_stems(predecessor, 0b1010)
        )
    );
    let v2 = playback.callback.mixer.stems_for_measurement()[0]
        .clone()
        .unwrap();
    assert!(!Arc::ptr_eq(
        &predecessor.complete_set_identity,
        &v2.complete_set_identity
    ));
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    controller_python(
        locals,
        r#"
v2_control = live.control
assert live.attempt is v2_control and v2_control.receipt is candidate
assert v2_control.receipt_intent == ('all_stems', 0)
for mask in (0b0101, 0, 0b1010):
    assert controller.set_stem_enabled_mask(0, mask)
    assert project.pad_stem_intent[0].enabled_mask == mask
    assert live.desired == ('all_stems', mask)
    assert live.control is v2_control and live.attempt is v2_control
    assert v2_control.receipt is candidate and v2_control.receipt_intent == ('all_stems', 0)
    assert candidate.claimed() and candidate.is_current()
    assert candidate.effective_mode_status() == 'transitioning'

disk_v3 = StemPairSelection.model_validate_json(third_json)
assert len({disk_v1.stem_set_identity, disk_v2.stem_set_identity, disk_v3.stem_set_identity}) == 3
assert len({disk_v1.wav_generation, disk_v2.wav_generation, disk_v3.wav_generation}) == 3
entry_v3 = StemCacheEntry(source_version=fixture['source_version'], cache_dir=disk_v3.wav_generation,
    stems=expected_stem_files(disk_v3.wav_generation), pair=disk_v3, available=True)
owner_v3 = audio.acquire_project_asset_lease(str(Path(entry_v3.cache_dir).absolute()))
persistence.commit_stem_selection(0, entry_v2, entry_v3,
    lambda: project.stem_cache[0] is entry_v2 and source_v3.same_source_request(
        audio.capture_prepared_source(0, fixture['source_version'])))
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None and reopened.project.stem_cache[0].pair == disk_v3
project.stem_cache[0] = entry_v3
prepared_v3.select()
controller._register_committed_pair(0, prepared_v3, source_v3)
assert controller.set_stem_mix_mode(0, 'all_stems')
for _ in range(16):
    controller.on_frame_render()
    assert live.disk.selection is None and live.disk.registration == 'deferred'
    assert live.control is v2_control and live.attempt is v2_control
    assert live.desired == ('all_stems', 0b1010)
    assert candidate.claimed() and candidate.is_current() and not candidate.withdraw()
    status = controller.stem_residency_status(0)
    assert status.disk_stem_set_identity == disk_v3.stem_set_identity
    assert status.control_stem_set_identity == disk_v2.stem_set_identity
    assert status.attempt_stem_set_identity == disk_v2.stem_set_identity
    assert status.effective_stem_set_identity == disk_v1.stem_set_identity
    assert status.residency == 'accepted' and status.effective == 'transitioning'
    assert status.registration == 'deferred'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(
        consumer.is_empty(),
        "no V3 callback command while V2 is claimed"
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    let current = voice(&playback.callback.mixer);
    assert!(current.source_playback.matches_exact(&clock));
    assert_eq!(current.generation, generation);
    assert_eq!(current.stretch.native_state_address(), native);
    assert_eq!(current.stretch.pending_fifo_frames(), fifo);
    let held = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &held.complete_set_identity,
        &v2.complete_set_identity
    ));
    for (before, after) in v2.stems.iter().zip(&held.stems) {
        assert!(Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert_eq!(playback.callback.mixer.stem_demand_for_measurement(0).1, 0);
    settle_empty(playback, consumer, locals, key_lock);
    assert!(consumer.is_empty());
    controller_python(
        locals,
        r#"
assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'effective'
assert live.disk.selection is None and live.control is v2_control
controller.on_frame_render()
third_control = live.disk.selection
third = live.attempt.receipt
assert third_control is not v2_control and third is not candidate
assert third_control.receipt_intent == ('all_stems', 0b1010)
assert third_control.ticket.publication_status() == 'accepted' and third.is_current()
assert third.residency_status() in {'captured', 'pending'} and third.effective_mode_status() == 'waiting'
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v3.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.attempt_stem_set_identity == disk_v3.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert status.effective == 'waiting'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert!(
        Arc::ptr_eq(
            &playback.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .complete_set_identity,
            &v2.complete_set_identity
        ),
        "V2 ACK cannot install V3"
    );
    own_adoption(playback, consumer, locals, "third", key_lock);
    let v3 = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(!Arc::ptr_eq(
        &v2.complete_set_identity,
        &v3.complete_set_identity
    ));
    assert!(!Arc::ptr_eq(
        &predecessor.complete_set_identity,
        &v3.complete_set_identity
    ));
    for (before, after) in v2.stems.iter().zip(&v3.stems) {
        assert!(!Arc::ptr_eq(&before.samples, &after.samples));
    }
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).1,
        0b1010
    );
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    // Real V3 swaps melody/bass: actual1010 means independent V1 bass+drums.
    assert!(
        playback
            .reference
            .set_stem_enabled_mask(0, 0b1100, full_stems.source_version_hash)
    );
    playback.select(Some(0b1100));
    controller_python(
        locals,
        r#"
controller.on_frame_render()
status = controller.stem_residency_status(0)
assert live.control is third_control and live.attempt is third_control
assert status.control_stem_set_identity == disk_v3.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert status.effective == 'transitioning'
assert third.claimed() and not third.withdraw()
"#,
    );
    if key_lock {
        settle_wet(playback, locals, "third");
    } else {
        settle_varispeed(playback, locals, "third");
    }
    controller_python(
        locals,
        r#"
status = controller.stem_residency_status(0)
assert status.registration == 'accepted' and status.residency == 'accepted'
assert status.effective == 'effective' and live.attempt is None
assert status.disk_stem_set_identity == disk_v3.stem_set_identity
assert status.control_stem_set_identity == disk_v3.stem_set_identity
assert status.effective_stem_set_identity == disk_v3.stem_set_identity
assert project.stem_cache[0].pair == disk_v3
assert project.pad_stem_intent[0].enabled_mask == 0b1010
assert not session.stem_generation_errors, session.stem_generation_errors
owner_v3.release()
"#,
    );
}

fn other_voice(mixer: &RtMixer) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(215))
        .unwrap()
}

fn delete_during_claimed_handover(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    old_owner: std::sync::Weak<[u8; 32]>,
    new_owner: std::sync::Weak<[u8; 32]>,
    paused: bool,
) {
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    assert!(transition.is_active());
    if paused {
        for id in [0, 215] {
            playback
                .callback
                .mixer
                .pause_sample_at_output_frame(id, playback.frame);
        }
        playback
            .reference
            .pause_sample_at_output_frame(0, playback.frame);
        playback
            .full_mix
            .pause_sample_at_output_frame(0, playback.frame);
    }
    controller_python(
        locals,
        r#"
assert candidate.claimed() and candidate.effective_mode_status() == 'transitioning'
assert controller.delete_stems(0)
assert project.stem_cache[0] is None and project.pad_stem_mix_mode[0] == 'full_mix'
assert not controller._committed_residency.contains(0)
assert controller.stem_residency_status(0) is None
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    assert_eq!(
        playback.callback.drain(consumer),
        1,
        "real ordered Python delete"
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    let after = voice(&playback.callback.mixer);
    assert!(after.source_playback.matches_exact(&clock));
    assert_eq!(after.generation, generation);
    assert_eq!(after.stretch.native_state_address(), native);
    assert_eq!(after.stretch.pending_fifo_frames(), fifo);
    playback.callback.retirement.slots = 0;
    let held_before = playback.callback.retirement.retired.len();
    if paused {
        // These callbacks consume no source frames. Keep the independent source-feed
        // clock fixed, then resume the same voice and its unmodified Native/FIFOs.
        for frames in [1, 127, 384] {
            let mut output = vec![0.0; frames * 2];
            playback.callback.mixer.render_rt(
                &mut output,
                &mut [0.0; NUM_SAMPLES],
                &mut playback.callback.retirement,
            );
            assert!(output.iter().all(|sample| *sample == 0.0));
            let current = voice(&playback.callback.mixer);
            assert!(current.source_playback.matches_exact(&clock));
            assert_eq!(current.generation, generation);
            assert_eq!(current.stretch.native_state_address(), native);
            assert_eq!(current.stretch.pending_fifo_frames(), fifo);
            assert!(
                playback
                    .callback
                    .mixer
                    .stem_transition_for_measurement(0)
                    .matches_exact(transition)
            );
            assert_eq!(
                playback.callback.mixer.stem_demand_for_measurement(0).0,
                StemMixMode::AllStems
            );
            assert!(old_owner.upgrade().is_some() && new_owner.upgrade().is_some());
        }
        for id in [0, 215] {
            playback
                .callback
                .mixer
                .resume_sample_at_output_frame(id, playback.frame);
        }
        playback
            .reference
            .resume_sample_at_output_frame(0, playback.frame);
        playback
            .full_mix
            .resume_sample_at_output_frame(0, playback.frame);
    }
    let ramp_frames = (128.0 / playback.ratio).ceil() as usize;
    let mut elapsed = 0;
    for frames in [1_usize, 31, 17, 7].into_iter().cycle() {
        let count = frames.min(ramp_frames - elapsed);
        playback.render(count, false);
        elapsed += count;
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        if elapsed == ramp_frames {
            break;
        }
        assert_eq!(
            playback.callback.mixer.stem_demand_for_measurement(0).0,
            StemMixMode::AllStems
        );
    }
    // Finishing V1 -> V2 arms the retained delete intent's own V2 -> FullMix
    // ramp. Neither ownership pressure nor Python cancellation may erase it.
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert_eq!(playback.callback.retirement.retired.len(), held_before);
    assert!(
        playback
            .reference
            .set_stem_mix_mode(0, StemMixMode::FullMix, 0)
    );
    playback.select(None);
    let mut elapsed = 0;
    for frames in [7_usize, 1, 43, 17].into_iter().cycle() {
        let count = frames.min(ramp_frames - elapsed);
        playback.render(count, false);
        elapsed += count;
        assert_eq!(voice(&playback.callback.mixer).generation, generation);
        assert_eq!(
            voice(&playback.callback.mixer)
                .stretch
                .native_state_address(),
            native
        );
        if elapsed == ramp_frames {
            break;
        }
        assert!(
            playback
                .callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
    }
    assert!(
        !playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_some());
    assert_eq!(playback.callback.retirement.retired.len(), held_before);
    assert!(old_owner.upgrade().is_some() && new_owner.upgrade().is_some());
    playback.callback.retirement.slots = 2;
    playback.render(1, false);
    assert!(playback.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(playback.callback.retirement.retired.len(), held_before + 2);
    for owner in [&old_owner, &new_owner] {
        assert!(
            playback.callback.retirement.retired[held_before..]
                .iter()
                .any(|retired| {
                    matches!(retired, RetiredAudioBuffer::PreparedStems(stems)
                if Arc::ptr_eq(&stems.complete_set_identity, &owner.upgrade().unwrap()))
                })
        );
    }
    playback.callback.retirement.slots = usize::MAX;
    playback.render(13, true);
    let (actual, _) = playback.adopt_native(false);
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&playback.callback.mixer).generation, generation);
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::FullMix
    );
    controller_python(
        locals,
        "controller.on_frame_render()\nassert project.stem_cache[0] is None\nassert controller.stem_residency_status(0) is None\nassert not session.stem_generation_errors, session.stem_generation_errors",
    );
    // Transfer both exact component owners through the existing production
    // retirement worker. Its join drains the queue outside rendering.
    let held_counts = (old_owner.strong_count(), new_owner.strong_count());
    let (mut off_rt, worker) =
        crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
    for retired in std::mem::take(&mut playback.callback.retirement.retired) {
        match retired {
            RetiredAudioBuffer::PreparedStems(stems) => off_rt.retire_prepared_stems(stems),
            other => playback.callback.retirement.retired.push(other),
        }
    }
    drop(off_rt);
    drop(worker);
    assert!(old_owner.strong_count() < held_counts.0);
    assert!(new_owner.strong_count() < held_counts.1);
}

fn stopped_adopt(
    engine: &AudioEngine,
    locals: &Py<PyDict>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    name: &str,
) {
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
    assert_eq!(callback.drain(consumer), 1, "stopped own Capture");
    wait_until(|| {
        consumer.peek().is_ok()
            || controller_ticket_status(locals, name, "residency_status") == "rejected"
    });
    assert_eq!(callback.drain(consumer), 1, "stopped own Adopt");
    callback.mixer.finish_committed_stem_mode(0);
    reconcile(engine).unwrap();
    assert_eq!(
        controller_ticket_status(locals, name, "residency_status"),
        "accepted"
    );
    assert_eq!(
        controller_ticket_status(locals, name, "effective_mode_status"),
        "waiting"
    );
    assert!(!callback.mixer.voices.iter().any(|voice| voice.active));
}

fn stopped_candidate_play(
    locals: &Py<PyDict>,
    callback: &mut Callback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    before_poll: bool,
    key_lock: bool,
    complete_partial: bool,
    ratio: f64,
) -> (u64, u64, LoopPhaseReference) {
    let source = callback.mixer.bank_for_measurement()[0].clone().unwrap();
    let set = callback.mixer.stems_for_measurement()[0].clone().unwrap();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("before_poll", before_poll)
            .unwrap();
        locals
            .bind(py)
            .set_item("stopped_key_lock", key_lock)
            .unwrap();
    });
    controller_python(
        locals,
        r#"
assert stopped.residency_status() == 'accepted' and stopped.is_current()
assert stopped.effective_mode_status() == 'waiting' and stopped.claimed()
assert stopped.effective_failure_reason() is None and stopped.error() is None
if not before_poll:
    controller.on_frame_render()
    assert live.control is live.disk.selection and live.attempt is None
else:
    assert live.control is previous_control and live.attempt is live.disk.selection
from flitzis_looper.controller.transport import TransportController
class ReadyTransportAudio(CapturedTransportAudio):
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def play_sample(self, *args, **kwargs):
        raise AssertionError('ordinary Play must use its own resident window ACK')
    play_sample_exclusive = play_sample
project.pad_loop_auto[0] = False
project.pad_loop_start_s[0] = 17 / fixture['rate']
project.pad_loop_end_s[0] = 1517 / fixture['rate']
project.pad_key_lock[0] = stopped_key_lock
transport = TransportController(project, session, ReadyTransportAudio())
ordinary_timestamp = captured_audio.capture_input_timestamp_ns()
transport.playback.trigger_pad_keep_others(0, received_at_ns=ordinary_timestamp)
ordinary_launch = next(iter(transport.residency._captured_starts.values()))
ordinary_start = ordinary_launch.start
ready_start = ordinary_launch.ticket
assert ready_start is not None
assert ready_start.publication_status() in ({'preparing', 'pending'} if complete_partial else {'pending'})
"#,
    );
    // A changed physical window remains a proposal during heavy preparation.
    // Its actual queued transaction proves the worker armed the new intent;
    // only that boundary can revoke the accepted stopped observation.
    wait_until(|| {
        Python::attach(|py| {
            locals
                .bind(py)
                .get_item("ready_start")
                .unwrap()
                .unwrap()
                .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
                .unwrap()
                .control_published_for_test()
        })
    });
    assert_original_target_at_front(consumer, locals);
    controller_python(
        locals,
        r#"
assert ready_start.is_current() and ready_start.publication_status() == 'pending'
assert 0 not in session.active_sample_ids
assert not stopped.is_current() and stopped.effective_mode_status() == 'rejected', (
    ready_start.publication_status(), stopped.is_current(), stopped.effective_mode_status(),
    session.stem_generation_errors, session.sample_load_errors)
controller.on_frame_render()
assert live.control is live.disk.selection and live.attempt is None, (
    controller.stem_residency_status(0), session.stem_generation_errors, session.sample_load_errors)
assert not live.control.candidate and live.control.ticket.is_current()
assert live.control.receipt is None and live.control.receipt_intent is None
assert live.control.desired is None and live.desired is None
status = controller.stem_residency_status(0)
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective is None and status.effective_stem_set_identity is None
assert project.pad_stem_mix_mode[0] == 'all_stems'
assert not session.stem_generation_errors, session.stem_generation_errors
"#,
    );
    let capture_frame = callback.transport.output_frame();
    assert_eq!(
        drain_callback_at_current_frame(callback, consumer),
        2,
        "original target then ordinary Play window ACK"
    );
    assert_eq!(
        ordinary_start_from_python(locals).original_frame(),
        Some(capture_frame)
    );
    controller_python(
        locals,
        "assert ready_start.is_current() and ready_start.publication_status() == 'accepted'\nassert ready_start.error() is None\nassert 0 not in session.active_sample_ids",
    );
    if !complete_partial {
        assert!(Arc::ptr_eq(
            &source.samples,
            &callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap()
                .samples
        ));
    } else {
        let current = callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
        assert_eq!(
            (current.resident_start(), current.resident_end()),
            (LOOP_START, LOOP_END)
        );
        assert_eq!(current.source_address(), source.source_address());
        assert_eq!(
            current.residency.as_ref().unwrap().context,
            if key_lock {
                crate::messages::ResidentContext::KeyLockFiniteLoop
            } else {
                crate::messages::ResidentContext::FiniteLoop
            }
        );
    }
    let actual = callback.mixer.stems_for_measurement()[0].as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &actual.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&actual.stems) {
        if !complete_partial {
            assert!(Arc::ptr_eq(&before.samples, &after.samples));
        } else {
            assert_eq!(
                (after.resident_start(), after.resident_end()),
                (LOOP_START, LOOP_END)
            );
            assert_eq!(after.samples.len(), (LOOP_END - LOOP_START) * 2);
        }
    }
    Python::attach(|py| {
        let ready = locals.bind(py).get_item("ready_start").unwrap().unwrap();
        let ready = ready
            .extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>()
            .unwrap();
        assert_eq!(
            ready.read_observation_for_test().is_none(),
            !complete_partial,
            "ordinary Play either reuses its ready window or owns its new finite preparation"
        );
    });
    let launch_timeline = callback.transport;
    let start = admit_original_public_start(callback, consumer, locals);
    let execution = start.execution_frame().unwrap();
    let original = start.original_frame().unwrap();
    let launch = LoopPhaseReference::capture(Some(launch_timeline), original);
    assert!(execution >= original);
    while callback.transport.output_frame() < execution {
        let frame = callback.transport.output_frame();
        let frames = (execution - frame).min(257) as usize;
        let mut output = vec![0.0; frames * 2];
        crate::audio_engine::audio_stream::render_scheduled_audio(
            &mut callback.mixer,
            &mut callback.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            frame,
            2,
            &mut callback.transport,
            &mut callback.feedback,
            &mut callback.retirement,
        );
        callback.transport.advance_by_rendered_frames(frames);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert!(callback.mixer.voices.iter().all(|voice| !voice.active));
        assert!(start.effective_frame().is_none());
    }
    execute_public_start_at_frame(callback, &start);
    assert!(
        voice(&callback.mixer)
            .source_playback
            .phase_reference_current(&launch_timeline)
    );
    if key_lock {
        assert_eq!(
            voice(&callback.mixer).stretch.adopted_request_id(),
            Some(start.sequence())
        );
        assert_eq!(
            voice(&callback.mixer)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            // Declared default pitch, KeyLock ON and the independent physical rate.
            ControllerRawNative::constant_history_frames(1.0 / ratio) as u64
        );
    }
    let started = callback.feedback.events.last().unwrap().clone();
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("ordinary_started_feedback", Py::new(py, started).unwrap())
            .unwrap()
    });
    controller_python(
        locals,
        r#"
ordinary_started_slot = captured_audio._resolve_pad_feedback(ordinary_started_feedback)
assert ordinary_started_slot == 0
transport.playback.handle_sample_started_message(ordinary_started_feedback, sample_id=ordinary_started_slot)
assert session.active_sample_ids == {0}
for _ in range(16):
    controller.on_frame_render()
assert live.control.receipt is None
assert controller.stem_residency_status(0).effective is None
assert not session.stem_generation_errors, session.stem_generation_errors
assert audio.worker_call_count() == 1
"#,
    );
    assert!(
        consumer.is_empty(),
        "poll cannot implicitly retry obsolete ALL"
    );
    assert!(
        callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    let actual = callback.mixer.stems_for_measurement()[0].as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &set.complete_set_identity,
        &actual.complete_set_identity
    ));
    for (before, after) in set.stems.iter().zip(&actual.stems) {
        if !complete_partial {
            assert!(Arc::ptr_eq(&before.samples, &after.samples));
        } else {
            assert_eq!(
                (after.resident_start(), after.resident_end()),
                (LOOP_START, LOOP_END)
            );
        }
    }
    (original, execution, launch)
}

fn public_explicit_seek(
    engine: &Arc<AudioEngine>,
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    key_lock: bool,
    target_kind: PublicSeekTarget,
    accepted: Option<crate::audio_engine::constant_timing::AcceptedTimingProjection>,
) {
    use crate::audio_engine::source_reader::ExplicitSeekMode;
    let complete_resident = playback.complete_resident;
    let (target, mode, repeats) = match target_kind {
        PublicSeekTarget::Intro => (0, ExplicitSeekMode::BeforeLoop, 1),
        PublicSeekTarget::Tail => (LOOP_END, ExplicitSeekMode::AfterLoop, 1),
        PublicSeekTarget::InLoop { equal } => {
            (64, ExplicitSeekMode::Normal, if equal { 2 } else { 1 })
        }
    };
    let intro = mode == ExplicitSeekMode::BeforeLoop;
    if intro && key_lock {
        let block = voice(&playback.callback.mixer)
            .stretch
            .rubberband_block_size();
        assert!(
            (8673 + 16 * block) as f64 * playback.ratio < playback.loop_start as f64,
            "both complete Native observations must fit inside the declared Intro"
        );
    }
    let other = other_voice(&playback.callback.mixer);
    let other_clock = other.source_playback;
    let other_generation = other.generation;
    Python::attach(|py| locals.bind(py).set_item("seek_target", target).unwrap());
    let consumed = playback.source_frame;
    for attempt in 0..repeats {
        let before = voice(&playback.callback.mixer);
        let generation = before.generation;
        let history_epoch = before.stretch.source_epoch_owner().load(Ordering::Acquire);
        let source_playback_before = before.source_playback;
        let source = playback.callback.mixer.bank_for_measurement()[0]
            .clone()
            .unwrap();
        if attempt == 1 {
            assert_eq!(before.source_playback.position().frame, target);
            assert_eq!(before.source_playback.position().fraction, 0.0);
            assert_eq!(before.explicit_seek_mode, mode);
            assert_eq!(
                playback.source_frame, consumed,
                "equal-phase Seek has no intervening source consumption"
            );
            controller_python(
                locals,
                "transport.on_frame_render()\nassert 0 not in transport.residency._pending\nprevious_seek = tail_seek\nassert previous_seek.is_current()\nassert session.pad_playhead_s[0] == seek_target / fixture['rate']",
            );
            assert!(
                consumer.is_empty(),
                "polling the first ACK consumes no audio or new control request"
            );
        } else if mode == ExplicitSeekMode::Normal {
            let position = before.source_playback.position();
            assert!(
                position.frame != target || position.fraction != 0.0,
                "changed in-loop Seek starts from a different exact phase"
            );
        }
        controller_python(
            locals,
            r#"
from flitzis_looper.controller.transport import TransportController
if not musical_explicit:
    transport = TransportController(project, session, playback_audio)
else:
    # Keep the own existing public timing/playback facade and accepted anchor.
    assert transport.bpm.current_timing(0) == timing_before_bpm
assert project.resolve_pad_key_lock(0) is tail_key_lock
transport.playback.seek_pad(0, seek_target / fixture['rate'])
tail_seek = transport.residency._pending[0].ticket
assert tail_seek is not None
assert transport.residency._pending[0].seek_only
assert tail_seek.key_lock_request_id is None
"#,
        );
        wait_until(|| {
            if matches!(consumer.peek(), Ok(ControlMessage::CaptureResidentSeek(_))) {
                assert_eq!(playback.drain(consumer), 1, "public own Seek Capture");
            }
            let status = controller_ticket_status(locals, "tail_seek", "publication_status");
            status == "accepted"
                || (status == "pending"
                    && matches!(consumer.peek(), Ok(ControlMessage::RelocateResident(_))))
        });
        assert_eq!(
            controller_ticket_status(locals, "tail_seek", "publication_status"),
            "pending",
            "capture-only proposal cannot acknowledge the Seek"
        );
        assert!(matches!(
            consumer.peek(),
            Ok(ControlMessage::RelocateResident(_))
        ));
        let captured = voice(&playback.callback.mixer);
        assert_eq!(captured.generation, generation);
        assert!(
            captured
                .source_playback
                .matches_exact(&source_playback_before)
        );
        assert_eq!(playback.drain(consumer), 1, "public own Seek Relocate");
        assert_eq!(
            controller_ticket_status(locals, "tail_seek", "publication_status"),
            "accepted"
        );
        reconcile(engine).unwrap();
        if complete_resident || mode == ExplicitSeekMode::Normal {
            let current = playback.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap();
            assert!(Arc::ptr_eq(&source.samples, &current.samples));
            assert_eq!(source.resident_binding(), current.resident_binding());
            assert_eq!(
                (source.resident_start(), source.resident_end()),
                (current.resident_start(), current.resident_end())
            );
            Python::attach(|py| {
                let ticket = locals.bind(py).get_item("tail_seek").unwrap().unwrap();
                let ticket = ticket.extract::<PyRef<crate::audio_engine::resident_relocation::ResidentWindowTicket>>().unwrap();
                if let Some(read) = ticket.read_observation_for_test() {
                    assert_eq!(read.source.read_bytes, 0, "covered Seek source I/O");
                    assert_eq!(read.source.allocated_bytes, 0, "covered Seek source PCM");
                    assert_eq!(read.stems.read_bytes, [0; 5], "covered Seek stem I/O");
                    assert_eq!(read.stems.allocated_bytes, 0, "covered Seek stem PCM");
                    assert_eq!(read.stems.fresh_opens, 0, "covered Seek stem opens");
                    assert_eq!(
                        read.stems.integrity_bytes, 0,
                        "covered Seek stem integrity I/O"
                    );
                }
            });
            assert!(
                consumer.is_empty(),
                "covered Seek has exactly one own Relocate"
            );
        }
        assert_eq!(
            controller_ticket_status(locals, "tail_seek", "publication_status"),
            "accepted"
        );
        let current = voice(&playback.callback.mixer);
        assert_ne!(
            current.generation, generation,
            "each own Seek cuts its old chronology, including an exactly equal phase"
        );
        assert!(current.stretch.source_epoch_owner().load(Ordering::Acquire) > history_epoch);
        assert_eq!(current.explicit_seek_mode, mode);
        assert_eq!(current.source_playback.position().frame, target);
        assert_eq!(current.source_playback.position().fraction, 0.0);
        assert_eq!(current.stretch.pending_fifo_frames(), (0, 0));
        assert_eq!(current.source_timing.accepted, accepted);
        // Seek clears the old configured domain even when its ACK preserves
        // immutable resident PCM. The exact accepted projection remains owned;
        // the next productive render configures its musical domain from it.
        assert_eq!(current.source_playback.loop_period(), None);
        assert_eq!(playback.source_frame, consumed);
        if attempt == 1 {
            controller_python(
                locals,
                "assert not previous_seek.is_current() and tail_seek.is_current()",
            );
        }
    }
    controller_python(
        locals,
        r#"
transport.on_frame_render()
assert 0 not in transport.residency._pending
assert tail_seek.is_current() and tail_seek.error() is None
assert tail_seek.effective_seek_seconds == seek_target / fixture['rate']
assert session.pad_playhead_s[0] == seek_target / fixture['rate']
assert transport.residency._current_error(0) is None
assert not session.paused_sample_ids
if selected_v1:
    assert not first.is_current()
"#,
    );
    let source = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_eq!(source.frame_count(), 16_384);
    assert_eq!(
        (source.resident_start(), source.resident_end()),
        (
            if complete_resident {
                0
            } else if mode == ExplicitSeekMode::Normal {
                playback.loop_start
            } else if key_lock && !intro {
                LOOP_START
            } else {
                0
            },
            if complete_resident {
                16_384
            } else if mode == ExplicitSeekMode::Normal || (key_lock && intro) {
                playback.loop_end
            } else {
                16_384
            }
        )
    );
    assert_eq!(
        source.residency.as_ref().unwrap().context,
        if complete_resident && key_lock {
            crate::messages::ResidentContext::KeyLockFullTrack
        } else if mode == ExplicitSeekMode::Normal && !key_lock {
            crate::messages::ResidentContext::FiniteLoop
        } else if key_lock {
            crate::messages::ResidentContext::KeyLockFiniteLoop
        } else {
            crate::messages::ResidentContext::FullTrack
        }
    );
    assert_eq!(
        crate::audio_engine::input_runtime_binding::capture(engine, 0)
            .unwrap()
            .unwrap()
            .binding
            .accepted,
        accepted
    );
    assert_eq!(
        engine
            .current_timing_acknowledgements
            .current_for_slot(&engine.input_runtime_ownership, 0),
        accepted.map_or(0, |projection| projection.publication_epoch)
    );
    let current = voice(&playback.callback.mixer);
    assert_eq!(current.explicit_seek_mode, mode);
    assert_eq!(current.source_playback.position().frame, target);
    assert_eq!(current.source_playback.position().fraction, 0.0);
    assert_eq!(current.stretch.pending_fifo_frames(), (0, 0));
    assert_eq!(current.source_timing.accepted, accepted);
    let other = other_voice(&playback.callback.mixer);
    assert!(other.source_playback.matches_exact(&other_clock));
    assert_eq!(other.generation, other_generation);
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        assert!(reference.seek_sample_at_output_frame(
            0,
            target as f64 / f64::from(RATE),
            playback.frame
        ));
    }
    playback.seek_epoch = Some(ControllerSeekEpoch {
        consumed_at: playback.source_frame,
        acknowledged_at: playback.frame,
        target,
    });
    playback.raw = key_lock.then(|| ControllerRawNative::new(playback.ratio, false));
    playback.candidate = None;
    // The authorized Seek resets fixed filter storage and snaps persistent
    // parameters to their targets. Immediate OFF output exposes this state;
    // the initial ON Wet silence used to mask an unnecessarily smoothed oracle.
    playback.filter = controller_filter();
    playback.previous = playback.selection;
    let actual = if key_lock {
        playback.render(13, true);
        assert_eq!(
            voice(&playback.callback.mixer)
                .source_playback
                .loop_period()
                .map(f64::to_bits),
            accepted
                .map(|projection| (projection.period_seconds * f64::from(RATE) / 16.0).to_bits()),
            "first productive post-Seek render uses the retained own timing ACK"
        );
        playback.adopt_native(true).0
    } else {
        let mut actual = Vec::new();
        for frames in [1, 31, 137, 257, 511] {
            actual.extend(render_varispeed(playback, frames).0);
        }
        actual
    };
    assert!(actual.iter().any(|sample| sample.abs() > 0.001));
    assert_eq!(voice(&playback.callback.mixer).explicit_seek_mode, mode);
    assert_eq!(
        voice(&playback.callback.mixer)
            .source_playback
            .loop_period()
            .map(f64::to_bits),
        accepted.map(|projection| (projection.period_seconds * f64::from(RATE) / 16.0).to_bits())
    );
    assert_eq!(
        voice(&playback.callback.mixer)
            .stretch
            .wet_current_source_output_ready(),
        key_lock
    );
    assert!(consumer.is_empty());
}

fn prove_public_loop_return(
    playback: &mut ControllerPlayback,
    key_lock: bool,
    accepted: Option<crate::audio_engine::constant_timing::AcceptedTimingProjection>,
) {
    use crate::audio_engine::source_reader::ExplicitSeekMode;
    let current = voice(&playback.callback.mixer);
    let native = current.stretch.native_state_address();
    let generation = current.generation;
    let (consumed_at, one_shot, initial_phase) =
        playback.seek_epoch.as_ref().map_or((0, 0, 0), |epoch| {
            if epoch.target < playback.loop_start {
                (epoch.consumed_at, playback.loop_start - epoch.target, 0)
            } else if epoch.target >= playback.loop_end {
                (
                    epoch.consumed_at,
                    playback.source.frame_count() - epoch.target,
                    0,
                )
            } else {
                (epoch.consumed_at, 0, epoch.target - playback.loop_start)
            }
        });
    let return_at = consumed_at + (one_shot as f64 / playback.ratio).ceil() as u64;
    // Observe three complete future seams even when the own Native checkpoint
    // has already advanced an in-loop Seek through earlier musical cycles.
    let begin = playback.source_frame.max(return_at);
    let base_cycle = (((begin - consumed_at) as f64 * playback.ratio - one_shot as f64
        + initial_phase as f64)
        / playback.loop_period)
        .floor() as usize;
    let finish = begin
        + if accepted.is_some() {
            (4.0 * playback.loop_period / playback.ratio).ceil() as u64 + 512
        } else {
            4096
        };
    let mut seams = [false; 3];
    for frames in [1_usize, 31, 137, 257, 511].into_iter().cycle() {
        let count = frames.min((finish - playback.source_frame) as usize);
        if accepted.is_some() {
            let last = (playback.loop_period.ceil() as usize - 1)
                .min(playback.loop_end - playback.loop_start - 1);
            for frame in playback.source_frame..playback.source_frame + count as u64 {
                let (whole, fraction, mode) = if playback.seek_epoch.is_some() {
                    playback.seek_position(frame)
                } else {
                    let phase = (frame as f64 * playback.ratio).rem_euclid(playback.loop_period);
                    (
                        playback.loop_start + phase.floor() as usize,
                        phase.fract(),
                        ExplicitSeekMode::Normal,
                    )
                };
                let phase = whole.saturating_sub(playback.loop_start) as f64 + fraction;
                if mode == ExplicitSeekMode::Normal && phase >= last as f64 && fraction != 0.0 {
                    assert!(playback.loop_start + last < playback.loop_end);
                    let gain = (phase - last as f64) / (playback.loop_period - last as f64);
                    assert!((0.0..1.0).contains(&gain));
                    assert_ne!(
                        gain.to_bits(),
                        fraction.to_bits(),
                        "virtual seam uses its accepted duration"
                    );
                    let distance = (frame - consumed_at) as f64 * playback.ratio - one_shot as f64
                        + initial_phase as f64;
                    let cycle = (distance / playback.loop_period).floor() as usize;
                    if let Some(observed) = cycle
                        .checked_sub(base_cycle)
                        .and_then(|cycle| seams.get_mut(cycle))
                    {
                        *observed = true;
                    }
                }
            }
        }
        if key_lock {
            playback.render(count, true);
        } else {
            render_varispeed(playback, count);
        }
        let current = voice(&playback.callback.mixer);
        assert_eq!(current.generation, generation);
        assert_eq!(current.stretch.native_state_address(), native);
        assert_eq!(current.source_timing.accepted, accepted);
        if playback.source_frame == finish {
            break;
        }
    }
    assert_eq!(
        voice(&playback.callback.mixer).explicit_seek_mode,
        ExplicitSeekMode::Normal
    );
    if accepted.is_some() {
        assert_ne!(
            playback.loop_period.to_bits(),
            ((playback.loop_end - playback.loop_start) as f64).to_bits()
        );
        assert!(
            seams.into_iter().all(|observed| observed),
            "independent complete PCM/Native/EQ must cross three accepted virtual seams: observed={seams:?}, outputs={begin}..{finish}, ratio={}, period={}",
            playback.ratio,
            playback.loop_period
        );
    }
}

pub(super) fn remember_public_pause_admissions(locals: &Py<PyDict>, paused: bool) {
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("expected_ordinary_paused", paused)
            .unwrap();
    });
    controller_python(
        locals,
        r#"
original_pause_admissions = {}
for pad in (0, 215):
    content = project.pad_content[pad]
    assert content is not None
    owner = transport.playback._pause_intent._owners[content.instance_id]
    admission = owner.admission
    assert owner.pending and admission is not None
    assert admission.requested_paused is expected_ordinary_paused
    assert admission.current_control_slot() == pad
    binding = transport._audio.current_input_runtime_pad_binding(pad)
    assert binding is not None and binding.is_current()
    assert admission.content_lifetime == binding.metadata()['content_lifetime']
    before = admission.current_observation()
    assert before is not None and before.current_control_slot() == pad and before.active
    assert before.ordinary_paused is not expected_ordinary_paused
    assert not before.settles_admission(admission)
    original_pause_admissions[pad] = (admission, before.observation_sequence)
assert original_pause_admissions[0][0].content_lifetime != original_pause_admissions[215][0].content_lifetime
assert all(transport.playback._pause_intent.requested_paused(pad) is expected_ordinary_paused for pad in (0, 215))
"#,
    );
}

pub(super) fn assert_public_pause_effect(locals: &Py<PyDict>) {
    controller_python(
        locals,
        r#"
for pad, (admission, before_sequence) in original_pause_admissions.items():
    assert admission.current_control_slot() == pad
    observation = admission.current_observation()
    assert observation is not None and observation.current_control_slot() == pad
    assert observation.content_lifetime == admission.content_lifetime
    assert observation.observation_sequence > before_sequence
    assert observation.matches_admission(admission) and observation.settles_admission(admission)
    assert observation.outcome == 'applied' and observation.active
    assert observation.ordinary_paused is expected_ordinary_paused
transport.playback.poll_pause_observations()
assert session.active_sample_ids == {0, 215}
assert session.paused_sample_ids == ({0, 215} if expected_ordinary_paused else set())
for pad in (0, 215):
    owner = transport.playback._pause_intent._owners[project.pad_content[pad].instance_id]
    assert not owner.pending and owner.admission is None
original_pause_admissions.clear()
"#,
    );
}

fn public_explicit_pause(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
) {
    let before = voice(&playback.callback.mixer);
    assert!(!before.paused);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let other_clock = other_voice(&playback.callback.mixer).source_playback;
    let consumed = playback.source_frame;
    controller_python(
        locals,
        r#"
assert session.active_sample_ids == {0, 215}
transport.playback.pause_pad(0)
transport.playback.pause_pad(215)
assert not session.paused_sample_ids
assert all(transport.playback._pause_intent.requested_paused(pad) for pad in (0, 215))
"#,
    );
    remember_public_pause_admissions(locals, true);
    assert_eq!(
        playback.drain(consumer),
        2,
        "own public explicit Seek Pause commands"
    );
    assert_public_pause_effect(locals);
    controller_python(
        locals,
        "transport.playback.poll_pause_observations()\nassert session.paused_sample_ids == {0, 215}",
    );
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        reference.pause_sample_at_output_frame(0, playback.frame);
    }
    for frames in [1, 127, 385] {
        playback.render_paused(frames);
    }
    let paused = voice(&playback.callback.mixer);
    assert!(paused.paused && paused.source_playback.matches_exact(&clock));
    assert_eq!(paused.explicit_seek_mode, clock.position().seek_mode);
    assert_eq!(paused.generation, generation);
    assert_eq!(paused.stretch.native_state_address(), native);
    assert_eq!(paused.stretch.pending_fifo_frames(), fifo);
    if let Some(history) = history {
        let actual = paused.stretch.productive_history().unwrap();
        assert_eq!(actual.binding, history.binding);
        assert_eq!(actual.next_position, history.next_position);
        assert_eq!(actual.fed_output_frames, history.fed_output_frames);
    } else {
        assert!(paused.stretch.productive_history().is_none());
    }
    let actual_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(actual_dsp.binding, dsp.binding);
    assert_eq!(actual_dsp.next_position, dsp.next_position);
    assert_eq!(actual_dsp.fed_output_frames, dsp.fed_output_frames);
    assert_eq!(playback.source_frame, consumed);
    let other = other_voice(&playback.callback.mixer);
    assert!(other.paused && other.source_playback.matches_exact(&other_clock));
    assert!(consumer.is_empty());
}

fn public_busy_handover_pause_resume(
    playback: &mut ControllerPlayback,
    consumer: &mut rtrb::Consumer<ControlMessage>,
    locals: &Py<PyDict>,
    old: &FinalReaderProbe,
    current: &FinalReaderProbe,
) {
    controller_python(
        locals,
        r#"
from flitzis_looper.controller.transport import TransportController
if 'transport' not in locals():
    transport = TransportController(project, session, playback_audio)
controller.on_frame_render()
assert candidate.is_current() and candidate.claimed()
assert candidate.effective_mode_status() == 'transitioning'
assert controller.stem_residency_status(0).effective == 'transitioning'
"#,
    );
    let before = voice(&playback.callback.mixer);
    let clock = before.source_playback;
    let generation = before.generation;
    let native = before.stretch.native_state_address();
    let fifo = before.stretch.pending_fifo_frames();
    let history = before.stretch.productive_history().unwrap();
    let dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    let transition = playback.callback.mixer.stem_transition_for_measurement(0);
    let consumed = playback.source_frame;
    let other = other_voice(&playback.callback.mixer);
    let other_clock = other.source_playback;
    let other_generation = other.generation;
    let other_native = other.stretch.native_state_address();
    let other_fifo = other.stretch.pending_fifo_frames();
    let incoming_pcm = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| stem.samples.as_ptr() as usize);
    let outgoing_pcm = playback
        .callback
        .mixer
        .outgoing_stems_for_measurement(0)
        .unwrap()
        .stems
        .each_ref()
        .map(|stem| stem.samples.as_ptr() as usize);
    public_explicit_pause(playback, consumer, locals);
    old.assert_retained();
    current.assert_retained();
    assert_eq!(playback.source_frame, consumed);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    controller_python(
        locals,
        r#"
controller.on_frame_render()
assert candidate.is_current() and candidate.claimed()
assert candidate.effective_mode_status() == 'transitioning'
assert controller.stem_residency_status(0).effective == 'transitioning'
transport.playback.resume_pad(0)
transport.playback.resume_pad(215)
assert session.paused_sample_ids == {0, 215}
assert not any(transport.playback._pause_intent.requested_paused(pad) for pad in (0, 215))
"#,
    );
    remember_public_pause_admissions(locals, false);
    assert_eq!(
        playback.drain(consumer),
        2,
        "own busy-handover public Resume commands"
    );
    assert_public_pause_effect(locals);
    controller_python(
        locals,
        "transport.playback.poll_pause_observations()\nassert not session.paused_sample_ids",
    );
    assert!(consumer.is_empty());
    for reference in [&mut playback.reference, &mut playback.full_mix] {
        reference.resume_sample_at_output_frame(0, playback.frame);
    }
    playback.resumed_at = Some(playback.frame);
    let resumed = voice(&playback.callback.mixer);
    assert!(!resumed.paused && resumed.source_playback.matches_exact(&clock));
    assert_eq!(resumed.generation, generation);
    assert_eq!(resumed.stretch.native_state_address(), native);
    assert_eq!(resumed.stretch.pending_fifo_frames(), fifo);
    let resumed_history = resumed.stretch.productive_history().unwrap();
    assert_eq!(resumed_history.binding, history.binding);
    assert_eq!(resumed_history.next_position, history.next_position);
    assert_eq!(resumed_history.fed_output_frames, history.fed_output_frames);
    let resumed_dsp = playback
        .callback
        .mixer
        .dsp_source_history_for_test(0)
        .unwrap();
    assert_eq!(resumed_dsp.binding, dsp.binding);
    assert_eq!(resumed_dsp.next_position, dsp.next_position);
    assert_eq!(resumed_dsp.fed_output_frames, dsp.fed_output_frames);
    assert!(
        playback
            .callback
            .mixer
            .stem_transition_for_measurement(0)
            .matches_exact(transition)
    );
    let incoming = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap();
    let outgoing = playback
        .callback
        .mixer
        .outgoing_stems_for_measurement(0)
        .unwrap();
    assert!(
        current
            .identity
            .ptr_eq(&Arc::downgrade(&incoming.complete_set_identity))
    );
    assert!(
        old.identity
            .ptr_eq(&Arc::downgrade(&outgoing.complete_set_identity))
    );
    assert_eq!(
        incoming
            .stems
            .each_ref()
            .map(|stem| stem.samples.as_ptr() as usize),
        incoming_pcm
    );
    assert_eq!(
        outgoing
            .stems
            .each_ref()
            .map(|stem| stem.samples.as_ptr() as usize),
        outgoing_pcm
    );
    old.assert_retained();
    current.assert_retained();
    let other = other_voice(&playback.callback.mixer);
    assert!(!other.paused && other.source_playback.matches_exact(&other_clock));
    assert_eq!(other.generation, other_generation);
    assert_eq!(other.stretch.native_state_address(), other_native);
    assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
    assert_eq!(playback.source_frame, consumed);
    assert!(playback.callback.retirement.retired.is_empty());
}

fn round_trip(
    selected_v1: bool,
    delete_pause: Option<bool>,
    key_lock: bool,
    stopped_before_poll: Option<bool>,
    paused_first: bool,
    trace: CandidateTrace,
) {
    round_trip_with_lifecycle(
        selected_v1,
        delete_pause,
        key_lock,
        stopped_before_poll,
        paused_first,
        trace,
        CandidateLifecycle::Existing,
    );
}

fn round_trip_with_lifecycle(
    selected_v1: bool,
    delete_pause: Option<bool>,
    key_lock: bool,
    stopped_before_poll: Option<bool>,
    paused_first: bool,
    trace: CandidateTrace,
    lifecycle: CandidateLifecycle,
) {
    let domain_final_reader = lifecycle == CandidateLifecycle::CleanupAfterClaim;
    let frozen_final_reader = matches!(
        lifecycle,
        CandidateLifecycle::FrozenUnload
            | CandidateLifecycle::FrozenResumeReaders
            | CandidateLifecycle::FrozenPausedUnload
    );
    assert!(!frozen_final_reader || matches!(trace, CandidateTrace::Ordinary));
    let full_track = matches!(trace, CandidateTrace::FullTrack);
    let complete_partial = matches!(trace, CandidateTrace::CompletePartial { .. });
    let stopped_partial = matches!(trace, CandidateTrace::CompletePartial { stopped: true, .. });
    let stopped_before_poll = stopped_before_poll.or(stopped_partial.then_some(false));
    let complete_resident = full_track || complete_partial;
    let seek_target = match trace {
        CandidateTrace::Explicit { intro: true, .. }
        | CandidateTrace::CompletePartial {
            seek: Some(true), ..
        } => Some(PublicSeekTarget::Intro),
        CandidateTrace::Explicit { intro: false, .. }
        | CandidateTrace::CompletePartial {
            seek: Some(false), ..
        } => Some(PublicSeekTarget::Tail),
        CandidateTrace::NormalSeek { equal, .. } => Some(PublicSeekTarget::InLoop { equal }),
        _ => None,
    };
    let explicit = seek_target.is_some() || complete_partial;
    let intro = matches!(seek_target, Some(PublicSeekTarget::Intro));
    let tail = matches!(seek_target, Some(PublicSeekTarget::Tail));
    let normal_explicit = explicit && !intro && !tail;
    let paused_explicit = matches!(
        trace,
        CandidateTrace::Explicit { paused: true, .. }
            | CandidateTrace::NormalSeek { paused: true, .. }
            | CandidateTrace::CompletePartial { paused: true, .. }
    );
    let musical_explicit = matches!(
        trace,
        CandidateTrace::Explicit { musical: true, .. }
            | CandidateTrace::NormalSeek { musical: true, .. }
            | CandidateTrace::CompletePartial { musical: true, .. }
    );
    // A phase-zero OFF musical loop at150 BPM has an exactly1200-output
    // period whose regular sample mesh skips the virtual tail. This declared
    // public target keeps the complete-partial Normal proof nonneutral and
    // samples each seam; existing explicit-Seek150 BPM meshes remain unchanged.
    let target_bpm = if musical_explicit && (key_lock || (complete_partial && normal_explicit)) {
        87.6
    } else {
        150.0
    };
    assert!(
        !explicit
            || (delete_pause.is_none()
                && (stopped_before_poll.is_none() || stopped_partial)
                && !paused_first)
    );
    let final_reader = matches!(
        trace,
        CandidateTrace::FinalReader
            | CandidateTrace::UnloadFinalReader
            | CandidateTrace::SavedCopyFinalReader
            | CandidateTrace::PostAdmitPcmFinalReader
            | CandidateTrace::SharedJobFinalReader
            | CandidateTrace::DiskSettlementFinalReader
    ) || lifecycle != CandidateLifecycle::Existing;
    let unload_final_reader = matches!(trace, CandidateTrace::UnloadFinalReader)
        || lifecycle == CandidateLifecycle::QueuedUnloadCleanup;
    let saved_copy = matches!(trace, CandidateTrace::SavedCopyFinalReader);
    let preserve_pcm = matches!(trace, CandidateTrace::PostAdmitPcmFinalReader);
    let shared_job = matches!(trace, CandidateTrace::SharedJobFinalReader);
    let disk_settlement = matches!(trace, CandidateTrace::DiskSettlementFinalReader);
    assert!(
        !final_reader
            || (selected_v1
                && key_lock
                && delete_pause.is_none()
                && stopped_before_poll.is_none()
                && !paused_first)
    );
    let repeated_sets = matches!(trace, CandidateTrace::RepeatedSets);
    let claimed_masks = matches!(trace, CandidateTrace::ClaimedMasks);
    let preclaim_masks = matches!(trace, CandidateTrace::PreclaimMasks);
    let retrigger = matches!(trace, CandidateTrace::Retrigger);
    let window_change = matches!(trace, CandidateTrace::WindowChange);
    let source_stems_off = matches!(trace, CandidateTrace::SourceStemsOff);
    let unload = matches!(
        trace,
        CandidateTrace::Unload | CandidateTrace::UnloadFinalReader
    ) || lifecycle == CandidateLifecycle::QueuedUnloadCleanup;
    let frozen_old = matches!(trace, CandidateTrace::FrozenOld);
    let source_stems_on = matches!(trace, CandidateTrace::SourceStemsOn);
    let source_key_lock = matches!(
        trace,
        CandidateTrace::SourceKeyLockOn | CandidateTrace::SourceStemsOn
    );
    assert!(!(source_stems_off || source_key_lock) || !key_lock);
    let source_play = matches!(
        trace,
        CandidateTrace::SourcePlay
            | CandidateTrace::SourceStemsOff
            | CandidateTrace::SourceKeyLockOn
            | CandidateTrace::SourceStemsOn
    );
    let source_replacement = matches!(
        trace,
        CandidateTrace::SourceReplacement
            | CandidateTrace::SourcePlay
            | CandidateTrace::SourceStemsOff
            | CandidateTrace::SourceKeyLockOn
            | CandidateTrace::SourceStemsOn
    );
    let source_controls = source_replacement
        || unload
        || saved_copy
        || shared_job
        || disk_settlement
        || domain_final_reader
        || frozen_final_reader;
    assert!(
        !(claimed_masks
            || preclaim_masks
            || retrigger
            || window_change
            || source_controls
            || frozen_old)
            || (!paused_first && delete_pause.is_none() && stopped_before_poll.is_none())
    );
    let withdraw_preclaim = matches!(trace, CandidateTrace::WithdrawPreclaim);
    let admission_pressure = matches!(trace, CandidateTrace::AdmissionPressure);
    let worker_post_pressure = matches!(trace, CandidateTrace::WorkerPostPressure);
    let queue_pressure = admission_pressure || worker_post_pressure;
    let capture_deadline = matches!(trace, CandidateTrace::CaptureDeadline);
    let paused_live_bpm = matches!(trace, CandidateTrace::PausedLiveBpm);
    let public_rate_adoption = matches!(trace, CandidateTrace::PublicRateAdoption);
    let mut public_rate_start = None;
    let paused_replacement = matches!(trace, CandidateTrace::PausedReplacement);
    let mut paused_other_clock = None;
    assert!(
        !paused_replacement
            || (selected_v1
                && !paused_first
                && delete_pause.is_none()
                && stopped_before_poll.is_none())
    );
    let live_bpm = matches!(
        trace,
        CandidateTrace::LiveBpm | CandidateTrace::PausedLiveBpm
    );
    let public_bpm = matches!(
        trace,
        CandidateTrace::PublicBpm(_)
            | CandidateTrace::LiveBpm
            | CandidateTrace::PausedLiveBpm
            | CandidateTrace::PublicRateAdoption
    );
    let mut live_other_ledger = None;
    let mut retriggered_at = None;
    let mut adoption_generation = None;
    let mut adopted_window = None;
    let mut final_current = None;
    assert!(
        !public_bpm
            || ((key_lock || live_bpm || public_rate_adoption)
                && (selected_v1 || public_rate_adoption)
                && !paused_first
                && delete_pause.is_none()
                && stopped_before_poll.is_none())
    );
    assert!(key_lock || delete_pause.is_none());
    assert!(
        !(repeated_sets || withdraw_preclaim || queue_pressure || capture_deadline)
            || (selected_v1
                && delete_pause.is_none()
                && stopped_before_poll.is_none()
                && !paused_first)
    );
    let previous_cwd = std::env::current_dir().unwrap();
    let mut h = if public_bpm || explicit {
        l3_native_tests::bpm_harness()
    } else {
        long_harness()
    };
    let _retirement_worker = final_reader.then(|| {
        let (actual, worker) =
            crate::audio_engine::buffer_retirement::create_audio_buffer_retirement();
        // End setup-only collecting owners on this control thread before render.
        h.callback.retirement.retired.clear();
        h.callback.retirement.actual = Some(actual);
        worker
    });
    let accepted =
        (public_bpm || musical_explicit).then(|| l3_native_tests::accepted_bpm_timing(&mut h));
    // Only the exact child owns this cwd and its project config/artifact paths.
    std::env::set_current_dir(h.root.parent().unwrap()).unwrap();
    let source = h.ticket(0);
    let oracle_pair = h.prepare(0, &source, true, None);
    assert_eq!(
        oracle_pair
            .pair_reader
            .descriptor
            .content
            .alignment
            .offset_frames,
        0
    );
    let saved_selection = selection(&oracle_pair);
    let mut saved = h.save(&oracle_pair);
    let full = h.material.sample.clone();
    let loop_region = crate::audio_engine::source_reader::FrameRange {
        start: if full_track {
            0
        } else if intro {
            14_501
        } else {
            LOOP_START
        },
        end: if full_track {
            full.frame_count()
        } else if intro {
            16_001
        } else {
            LOOP_END
        },
    };
    let original_before = final_reader.then(|| fs::read(&h.material.lease.original_path).unwrap());
    let full_stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x74; 32]),
        reference_samples: full.samples.clone(),
        reference_residency: full.residency.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: None,
        source_version_hash: crate::audio_engine::stem_cache::source_version_hash(&h.version),
        sample_rate_hz: RATE,
        channels: 2,
        frame_count: full.frame_count(),
        available_mask: 0b1111,
        stems: oracle_pair
            .component_views_for_test()
            .unwrap()
            .each_ref()
            .map(|component| {
                let mut oracle = component.clone();
                if final_reader {
                    // Expected audio has its own complete PCM allocation. It must not
                    // hold the registered generation's actual final native readers.
                    oracle.samples = Arc::from(component.samples.as_ref().to_vec());
                }
                oracle
            }),
    };
    assert!(full_stems.stems.iter().all(|component| {
        component.samples[loop_region.start * 2..loop_region.end * 2]
            .iter()
            .any(|value| value.abs() > 0.04)
    }));
    drop(oracle_pair);
    let window = prepare_window_with_producer(
        &h.engine,
        0,
        WindowRequest {
            key_lock: Some(key_lock && !complete_resident),
            loop_region: Some((
                if complete_partial {
                    0.0
                } else {
                    loop_region.start as f64 / f64::from(RATE)
                },
                Some(if complete_partial {
                    full.frame_count() as f64 / f64::from(RATE)
                } else {
                    loop_region.end as f64 / f64::from(RATE)
                }),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| h.consumer.peek().is_ok() || window.error().unwrap().is_some());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(window.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    if complete_partial {
        // Scalar intent declares the partial loop before voice birth while the
        // genuinely acknowledged OFF complete source remains resident.
        h.callback
            .mixer
            .set_loop_region_frames(0, loop_region.start, Some(loop_region.end));
    }
    if complete_resident && key_lock {
        let mode = prepare_window_with_producer(
            &h.engine,
            0,
            WindowRequest {
                key_lock: Some(true),
                ..WindowRequest::default()
            },
            h.producer.clone(),
        )
        .unwrap();
        wait_until(|| h.consumer.peek().is_ok() || mode.error().unwrap().is_some());
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(mode.publication_status(), "accepted", "{:?}", mode.error());
        reconcile(&h.engine).unwrap();
    }
    let finite = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
    if complete_resident {
        assert_eq!(finite.resident_start(), 0);
        assert_eq!(finite.resident_end(), full.frame_count());
        assert_eq!(finite.samples.len(), full.frame_count() * 2);
        assert_eq!(
            finite.residency.as_ref().unwrap().context,
            if key_lock {
                crate::messages::ResidentContext::KeyLockFullTrack
            } else {
                crate::messages::ResidentContext::FullTrack
            }
        );
    }
    let other_source = h.callback.mixer.bank_for_measurement()[215]
        .clone()
        .unwrap();
    let other_ticket = h.ticket(215);
    assert_eq!(other_ticket.id, 215);
    assert!(other_ticket.sample.same_source(&other_source));
    assert!(other_source.same_source(&full));
    assert!(!other_ticket.same_source_request(&source));
    let other_key = other_ticket
        .admission
        .binding
        .placement
        .unwrap()
        .content_key()
        .unwrap();
    assert_ne!(
        Some(other_key),
        source.admission.binding.placement.unwrap().content_key()
    );
    assert_eq!(other_ticket.source_version, h.version);
    let two_active_pads = stopped_before_poll.is_none();
    let metadata = h.material.metadata();
    let fixture = json!({
        "original_reference": metadata["new_reference"], "material_id": metadata["material_id"],
        "source_version": h.version, "pair": saved_selection, "frames": full.frame_count(), "rate": RATE,
        "peer": {
            "original_reference": metadata["new_reference"], "material_id": metadata["material_id"],
            "frames": other_ticket.sample.frame_count(), "rate": other_ticket.sample_rate_hz,
            "content_lifetime": other_key.lifetime(),
        },
    });
    let (parameter_producer, mut parameter_consumer) = rtrb::RingBuffer::new(8);
    let parameter_producer = Arc::new(Mutex::new(parameter_producer));
    {
        let (_feedback, feedback_rx) = rtrb::RingBuffer::new(8);
        h.engine.test_control_channels =
            Some((h.producer.clone(), Arc::new(Mutex::new(feedback_rx))));
        h.engine.test_parameter_producer = Some(parameter_producer.clone());
        h.engine.test_output_format = Some((RATE, 2));
    }
    let engine = Arc::new(h.engine);
    let locals = Python::attach(|py| {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let locals = PyDict::new(py);
        locals
            .set_item("input_json", serde_json::to_string(&fixture).unwrap())
            .unwrap();
        locals.set_item("selected_v1", selected_v1).unwrap();
        locals.set_item("public_bpm", public_bpm).unwrap();
        locals
            .set_item("musical_explicit", musical_explicit)
            .unwrap();
        locals.set_item("tail", tail).unwrap();
        locals.set_item("explicit", explicit).unwrap();
        locals
            .set_item(
                "domain_final_reader",
                lifecycle != CandidateLifecycle::Existing,
            )
            .unwrap();
        locals
            .set_item(
                "declared_domain",
                explicit || (full_track && domain_final_reader),
            )
            .unwrap();
        locals
            .set_item("seek_requested", seek_target.is_some())
            .unwrap();
        locals
            .set_item("complete_partial", complete_partial)
            .unwrap();
        locals
            .set_item("seek_target", if intro { 0 } else { LOOP_END })
            .unwrap();
        locals
            .set_item("declared_loop_start", loop_region.start)
            .unwrap();
        locals
            .set_item("declared_loop_end", loop_region.end)
            .unwrap();
        locals.set_item("paused_explicit", paused_explicit).unwrap();
        locals.set_item("two_active_pads", two_active_pads).unwrap();
        locals.set_item("tail_key_lock", key_lock).unwrap();
        locals.set_item("live_bpm", live_bpm).unwrap();
        locals
            .set_item("paused_replacement", paused_replacement)
            .unwrap();
        if public_bpm || musical_explicit {
            locals
                .set_item(
                    "bpm_audio",
                    Py::new(
                        py,
                        PublicBpmAudio {
                            engine: engine.clone(),
                            producer: h.producer.clone(),
                            parameter_producer: parameter_producer.clone(),
                            original_rate_revision: AtomicU64::new(0),
                        },
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        locals.set_item("repeated_sets", repeated_sets).unwrap();
        locals
            .set_item(
                "captured_audio",
                Py::new(
                    py,
                    PublicCapturedStartAudio {
                        engine: engine.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        locals.set_item("preclaim_masks", preclaim_masks).unwrap();
        locals.set_item("retrigger", retrigger).unwrap();
        locals.set_item("window_change", window_change).unwrap();
        if source_controls {
            locals
                .set_item(
                    "load_audio",
                    Py::new(
                        py,
                        PublicLoadAudio {
                            engine: engine.clone(),
                            producer: h.producer.clone(),
                            parameters: parameter_producer.clone(),
                        },
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        locals.set_item("queue_pressure", queue_pressure).unwrap();
        locals
            .set_item("capture_deadline", capture_deadline)
            .unwrap();
        locals
            .set_item("worker_post_pressure", worker_post_pressure)
            .unwrap();
        locals
            .set_item("withdraw_preclaim", withdraw_preclaim)
            .unwrap();
        locals
            .set_item(
                "playback_audio",
                Py::new(
                    py,
                    ReadyPlaybackAudio {
                        engine: engine.clone(),
                        producer: h.producer.clone(),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        locals
            .set_item(
                "audio",
                Py::new(
                    py,
                    PairWorkerAudio {
                        engine: engine.clone(),
                        root: h.root.clone(),
                        producer: h.producer.clone(),
                        control_thread: std::thread::current().id(),
                        worker_calls: AtomicUsize::new(0),
                        only_pool_threads: AtomicBool::new(true),
                    },
                )
                .unwrap(),
            )
            .unwrap();
        worker_python(
            py,
            &locals,
            r#"
import json
from pathlib import Path
from flitzis_looper.models import ProjectState, SessionState, StemCacheEntry, PadContentIdentity, PadStemIntent
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.controller.stems import StemController
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.stem_pair_selection import StemPairSelection
captured_start_calls, captured_dependency_calls = [], []
class CapturedTransportAudio:
    def __getattr__(self, name):
        return getattr(playback_audio, name)
    def capture_pad_start(self, *args, **kwargs):
        original = playback_audio.capture_pad_start(*args, **kwargs)
        captured_start_calls.append((original, kwargs.get('received_at_ns')))
        return original
    def prepare_captured_pad_start(self, original, window=None):
        captured_dependency_calls.append((original, window))
        return captured_audio.prepare_captured_pad_start(original, window)
fixture = json.loads(input_json)
project = ProjectState()
project.sample_paths[0] = fixture['original_reference']
project.sample_durations[0] = fixture['frames'] / fixture['rate']
project.pad_content[0] = PadContentIdentity(instance_id='8' * 32, material_id=fixture['material_id'])
project.pad_timing_intent[0] = 'automatic' if public_bpm or musical_explicit else 'legacy'
project.pad_stem_mix_mode[0] = 'full_mix'
if two_active_pads:
    # Both pads were independently adopted and acknowledged by Native. Their
    # shared immutable material does not make their mutable content/intent one.
    project.sample_paths[215] = fixture['peer']['original_reference']
    project.sample_durations[215] = fixture['peer']['frames'] / fixture['peer']['rate']
    project.pad_content[215] = PadContentIdentity(instance_id='a' * 32, material_id=fixture['peer']['material_id'])
    assert project.pad_content[215].instance_id != project.pad_content[0].instance_id
    peer_binding = playback_audio.current_input_runtime_pad_binding(215)
    assert peer_binding is not None and peer_binding.is_current()
    assert peer_binding.metadata()['content_lifetime'] == fixture['peer']['content_lifetime']
    assert peer_binding.metadata()['intent'] == 'legacy'
    project.pad_timing_intent[215] = 'legacy'
    project.pad_key_lock[215] = False
    project.pad_loop_auto[215] = False
    project.pad_loop_start_s[215] = 17 / fixture['peer']['rate']
    project.pad_loop_end_s[215] = 1517 / fixture['peer']['rate']
    del peer_binding
if declared_domain:
    project.pad_loop_auto[0] = False
    project.pad_loop_start_s[0] = declared_loop_start / fixture['rate']
    project.pad_loop_end_s[0] = declared_loop_end / fixture['rate']
    project.pad_key_lock[0] = tail_key_lock
disk_v1 = StemPairSelection.model_validate(fixture['pair'])
project.stem_cache[0] = StemCacheEntry(source_version=fixture['source_version'],
    cache_dir=disk_v1.wav_generation, stems=expected_stem_files(disk_v1.wav_generation), pair=disk_v1, available=True)
session = SessionState()
project.pad_stem_intent[0] = PadStemIntent(enabled_mask=0b1010, display_mode='custom')
persistence = ProjectPersistence(project)
controller = StemController(project, session, audio, persistence.mark_dirty)
assert isinstance(controller._stem_worker_pool, StemWorkerPool)
assert controller._stem_worker_pool._executor._max_workers == 2
assert controller._committed_residency.pending_supported
controller.restore_stem_cache_from_project_state()
assert controller.publish_restored_stem_cache_if_available(0)
"#,
        );
        locals.unbind()
    });
    wait_until(|| {
        let own_registration_pending = Python::attach(|py| {
            worker_python(
                py,
                locals.bind(py),
                r#"
controller.on_frame_render()
assert not session.stem_generation_errors, session.stem_generation_errors
initial_registration_pending = (
    0 in controller._committed_residency._selections
    and controller._committed_residency._selections[0].ticket.is_current()
    and controller._committed_residency._selections[0].ticket.publication_status() == 'pending'
)
"#,
            );
            locals
                .bind(py)
                .get_item("initial_registration_pending")
                .unwrap()
                .unwrap()
                .extract::<bool>()
                .unwrap()
        });
        own_registration_pending && h.consumer.peek().is_ok()
    });
    assert!(
        matches!(
            h.consumer.peek().unwrap(),
            ControlMessage::CommittedStemWork(work)
                if matches!(
                    work.as_ref(),
                    crate::audio_engine::committed_stem_residency::CommittedStemRtWork::Register {
                        id: 0,
                        ..
                    }
                )
        ),
        "initial restore must queue its own stopped source registration"
    );
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
previous_control = controller._committed_residency._selections[0]
assert previous_control.ticket.publication_status() == 'accepted'
assert previous_control.ticket.is_current()
assert controller.stem_residency_status(0).effective is None
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
persistence.mark_dirty()
persistence.flush()
"#,
    );
    let ratio = if let Some(projection) = accepted {
        public_bpm_before_voice_birth(
            &locals,
            &mut h.callback,
            &mut h.consumer,
            &mut parameter_consumer,
            projection,
            if let CandidateTrace::PublicBpm(bpm) = trace {
                bpm
            } else {
                assert!(live_bpm || public_rate_adoption || musical_explicit);
                target_bpm
            },
        )
    } else {
        let ratio = if explicit && key_lock {
            0.73
        } else if key_lock {
            0.75
        } else {
            1.25
        };
        h.callback.mixer.set_speed(ratio);
        ratio
    };
    assert_eq!(h.callback.mixer.key_lock_for_measurement(0), key_lock);
    h.callback.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    if let Some(before_poll) = stopped_before_poll {
        let other_stopped_demand = h.callback.mixer.stem_demand_for_measurement(215);
        assert!(!paused_first);
        assert!((!selected_v1 || stopped_partial) && delete_pause.is_none());
        let install_stopped_v1 = !stopped_partial || selected_v1;
        let v1 = if install_stopped_v1 {
            controller_python(
                &locals,
                "assert controller.set_stem_mix_mode(0, 'all_stems')\nv1_stopped = previous_control.receipt",
            );
            stopped_adopt(
                &engine,
                &locals,
                &mut h.callback,
                &mut h.consumer,
                "v1_stopped",
            );
            controller_python(
                &locals,
                "controller.on_frame_render()\nassert controller.stem_residency_status(0).effective_stem_set_identity is None",
            );
            Some(h.callback.mixer.stems_for_measurement()[0].clone().unwrap())
        } else {
            assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
            None
        };
        commit_candidate_v2(
            &engine,
            &h.root,
            &h.version,
            &h.wav_reference,
            full.frame_count(),
            &locals,
        );
        controller_python(
            &locals,
            "assert controller.set_stem_mix_mode(0, 'all_stems')\nstopped = live.attempt.receipt\nassert stopped is not globals().get('v1_stopped')",
        );
        stopped_adopt(
            &engine,
            &locals,
            &mut h.callback,
            &mut h.consumer,
            "stopped",
        );
        let v2 = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
        if let Some(v1) = v1 {
            assert!(!Arc::ptr_eq(
                &v1.complete_set_identity,
                &v2.complete_set_identity
            ));
            for (before, after) in v1.stems.iter().zip(&v2.stems) {
                assert!(!Arc::ptr_eq(&before.samples, &after.samples));
            }
        }
        if stopped_partial {
            let source = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
            assert_eq!(
                (source.resident_start(), source.resident_end()),
                (0, full.frame_count())
            );
            assert_eq!(
                source.residency.as_ref().unwrap().context,
                if key_lock {
                    crate::messages::ResidentContext::KeyLockFullTrack
                } else {
                    crate::messages::ResidentContext::FullTrack
                }
            );
            assert_eq!(v2.accepted_timing, accepted);
            assert!(
                v2.stems
                    .iter()
                    .all(|component| component.resident_start() == 0
                        && component.resident_end() == full.frame_count()
                        && component.samples.len() == full.frame_count() * 2)
            );
            assert!(Arc::ptr_eq(
                &other_source.samples,
                &h.callback.mixer.bank_for_measurement()[215]
                    .as_ref()
                    .unwrap()
                    .samples
            ));
            assert!(h.callback.mixer.stems_for_measurement()[215].is_none());
            controller_python(
                &locals,
                "controller.on_frame_render()\nassert stopped.is_current() and stopped.effective_mode_status() == 'waiting'\nassert not session.active_sample_ids\nassert controller.stem_residency_status(0).effective_stem_set_identity is None",
            );
        }
        let (original, birth, launch) = stopped_candidate_play(
            &locals,
            &mut h.callback,
            &mut h.consumer,
            before_poll,
            key_lock,
            stopped_partial,
            ratio,
        );
        let played_set = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
        assert!(Arc::ptr_eq(
            &v2.complete_set_identity,
            &played_set.complete_set_identity
        ));
        let oracle_timing =
            accepted.map(|projection| (projection, projection.period_seconds / ratio));
        let mut reference = complete_reference_at_loop(
            &full,
            &full_stems,
            ratio,
            key_lock,
            oracle_timing,
            loop_region,
        );
        // Stopped V2 is already selected before this new ordinary voice starts.
        // Physical V2 mask1010 equals complete independent V1 mask0011.
        reference.stop_sample(0);
        assert!(reference.set_stem_enabled_mask(0, 0b0011, full_stems.source_version_hash));
        assert!(reference.set_stem_mix_mode(
            0,
            StemMixMode::AllStems,
            full_stems.source_version_hash
        ));
        assert!(reference.play_sample_at_output_frame(0, 1.0, birth));
        install_public_start_reference(&mut reference, launch, &h.callback.transport);
        let mut full_mix = complete_reference_at_loop(
            &full,
            &full_stems,
            ratio,
            key_lock,
            oracle_timing,
            loop_region,
        );
        assert!(full_mix.play_sample_at_output_frame(0, 1.0, birth));
        install_public_start_reference(&mut full_mix, launch, &h.callback.transport);
        if key_lock {
            prepare_public_start_reference(&mut reference, birth);
            prepare_public_start_reference(&mut full_mix, birth);
        }
        let mut playback = ControllerPlayback {
            callback: h.callback,
            reference,
            full_mix,
            frame: birth,
            source_frame: 0,
            resumed_at: Some(birth),
            seek_epoch: None,
            complete_resident: false,
            source: full.clone(),
            components: full_stems.stems.clone(),
            ratio,
            target_bpm,
            loop_period: accepted.map_or((LOOP_END - LOOP_START) as f64, |projection| {
                projection.period_seconds * f64::from(RATE) / 16.0
            }),
            loop_start: LOOP_START,
            loop_end: LOOP_END,
            rate_ledger: None,
            scheduled: false,
            selection: Some(0b0011),
            previous: Some(0b0011),
            ramp_start: 0,
            raw: None,
            candidate: None,
            filter: controller_filter(),
        };
        playback.raw = key_lock.then(|| prepared_public_start_raw(&playback));
        // E is the source birth for all independent references; their complete
        // initial source/Native/EQ output remains unconsumed until emission.
        playback.advance_expected_birth(original, birth);
        let (actual, counterfactual) = if key_lock {
            render_physical_birth_prefix(&mut playback)
        } else {
            let mut actual = Vec::new();
            let mut counterfactual = Vec::new();
            for frames in [13, 31, 137, 257, 511] {
                let (output, full_mix) = render_varispeed(&mut playback, frames);
                actual.extend(output);
                counterfactual.extend(full_mix);
            }
            (actual, counterfactual)
        };
        assert!(actual.iter().any(|sample| sample.abs() > 0.001));
        assert!(
            actual
                .iter()
                .zip(counterfactual)
                .any(|(actual, full)| (actual - full).abs() > 0.005)
        );
        controller_python(
            &locals,
            r#"
controller.on_frame_render()
assert controller.stem_residency_status(0).effective is None
assert controller.set_stem_mix_mode(0, 'all_stems')
fresh = live.control.receipt
assert fresh is not stopped and fresh.is_current()
assert fresh.residency_status() in {'captured', 'pending'}
assert fresh.effective_mode_status() == 'waiting'
"#,
        );
        own_adoption(&mut playback, &mut h.consumer, &locals, "fresh", key_lock);
        let after = playback.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(
            &played_set.complete_set_identity,
            &after.complete_set_identity
        ));
        for (before, after) in played_set.stems.iter().zip(&after.stems) {
            assert!(Arc::ptr_eq(&before.samples, &after.samples));
        }
        let mut output = Vec::new();
        for frames in [1, 31, 137, 257, if key_lock { 777 } else { 511 }] {
            output.extend(if key_lock {
                playback.render(frames, true).0
            } else {
                render_varispeed(&mut playback, frames).0
            });
        }
        assert!(output.iter().any(|sample| sample.abs() > 0.001));
        if stopped_partial {
            prove_public_loop_return(&mut playback, key_lock, accepted);
            let current = engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap();
            assert_eq!(current.binding.accepted, accepted);
            assert_eq!(
                engine
                    .current_timing_acknowledgements
                    .current_for_slot(&engine.input_runtime_ownership, 0),
                accepted.map_or(0, |projection| projection.publication_epoch)
            );
            assert!(Arc::ptr_eq(
                &other_source.samples,
                &playback.callback.mixer.bank_for_measurement()[215]
                    .as_ref()
                    .unwrap()
                    .samples
            ));
            assert!(playback.callback.mixer.stems_for_measurement()[215].is_none());
            assert_eq!(
                playback.callback.mixer.stem_demand_for_measurement(215),
                other_stopped_demand
            );
            controller_python(
                &locals,
                r#"
persistence.mark_dirty()
persistence.flush()
stopped_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert stopped_reopened.load_error is None
assert stopped_reopened.project.stem_cache[0].pair == disk_v2
assert stopped_reopened.project.pad_loop_start_s[0] == declared_loop_start / fixture['rate']
assert stopped_reopened.project.pad_loop_end_s[0] == declared_loop_end / fixture['rate']
assert stopped_reopened.project.pad_key_lock[0] is tail_key_lock
assert stopped_reopened.project.pad_timing_intent[0] == ('automatic' if musical_explicit else 'legacy')
if musical_explicit:
    assert stopped_reopened.project.bpm_lock
    assert stopped_reopened.project.speed == accepted_period * (target_bpm / 60)
    assert stopped_reopened.project.manual_bpm[0] is None
"#,
            );
        }
        controller_python(
            &locals,
            r#"
controller.on_frame_render()
assert fresh.is_current() and fresh.effective_mode_status() == 'effective'
assert fresh.effective_failure_reason() is None and fresh.error() is None
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert status.effective == 'effective' and not stopped.is_current()
assert not session.stem_generation_errors, session.stem_generation_errors
assert audio.worker_call_count() == 1
controller.shut_down()
controller._assets.release_saved_assignments()
owner_v1.release()
owner_v2.release()
"#,
        );
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    assert!(h.callback.mixer.play_sample_at_output_frame(0, 1.0, 0));
    // A genuinely active other pad contributes zero gain, so the independent pad-0
    // raw oracle can still assert every output sample without hiding other-pad state.
    h.callback
        .mixer
        .set_loop_region_frames(215, LOOP_START, Some(LOOP_END));
    assert!(h.callback.mixer.play_sample_at_output_frame(215, 0.0, 0));
    let other_generation = other_voice(&h.callback.mixer).generation;
    let other_native = other_voice(&h.callback.mixer)
        .stretch
        .native_state_address();
    let other_fifo = other_voice(&h.callback.mixer).stretch.pending_fifo_frames();
    let other_demand = h.callback.mixer.stem_demand_for_measurement(215);
    let oracle_timing = accepted.map(|projection| {
        let bpm = if let CandidateTrace::PublicBpm(bpm) = trace {
            bpm
        } else {
            target_bpm
        };
        (
            projection,
            projection.period_seconds / (projection.period_seconds * (bpm / 60.0)),
        )
    });
    let mut playback = ControllerPlayback {
        callback: h.callback,
        reference: complete_reference_at_loop(
            &full,
            &full_stems,
            ratio,
            key_lock,
            oracle_timing,
            loop_region,
        ),
        full_mix: complete_reference_at_loop(
            &full,
            &full_stems,
            ratio,
            key_lock,
            oracle_timing,
            loop_region,
        ),
        frame: 0,
        source_frame: 0,
        resumed_at: None,
        seek_epoch: None,
        complete_resident,
        source: full.clone(),
        components: full_stems.stems.clone(),
        ratio,
        target_bpm,
        loop_period: accepted.map_or((loop_region.end - loop_region.start) as f64, |projection| {
            projection.period_seconds * f64::from(RATE) / 16.0
        }),
        loop_start: loop_region.start,
        loop_end: loop_region.end,
        rate_ledger: (live_bpm || public_rate_adoption).then(|| {
            ControllerRateLedger::new(
                ratio,
                accepted.unwrap().period_seconds * f64::from(RATE) / 16.0,
            )
        }),
        scheduled: live_bpm || public_rate_adoption,
        selection: None,
        previous: None,
        ramp_start: 0,
        raw: key_lock.then(|| ControllerRawNative::new(ratio, false)),
        candidate: None,
        filter: controller_filter(),
    };
    if key_lock {
        playback.render(13, true);
        let (actual, _) = playback.adopt_native(true);
        if public_bpm {
            assert!(actual.iter().any(|sample| sample.abs() > 0.001));
            assert_ne!(
                playback.loop_period.to_bits(),
                ((LOOP_END - LOOP_START) as f64).to_bits()
            );
            assert!(playback.source_frame as f64 * playback.ratio > playback.loop_period);
        }
        if paused_first {
            assert!(actual.iter().any(|sample| sample.abs() > 0.001));
        }
    } else {
        let (actual, _) = render_varispeed(&mut playback, 13);
        if paused_first {
            assert!(actual.iter().any(|sample| sample.abs() > 0.001));
        }
    }
    controller_python(&locals, "session.active_sample_ids.update({0, 215})");
    if paused_first {
        assert!(!selected_v1 && delete_pause.is_none());
        if key_lock {
            assert!(voice(&playback.callback.mixer).stretch.wet_render_ready());
            assert!(
                voice(&playback.callback.mixer)
                    .stretch
                    .wet_current_source_output_ready()
            );
        } else {
            assert!(playback.raw.is_none() && playback.candidate.is_none());
            assert!(!voice(&playback.callback.mixer).stretch.wet_render_ready());
            assert!(
                voice(&playback.callback.mixer)
                    .stretch
                    .productive_history()
                    .is_none()
            );
        }
        controller_python(
            &locals,
            r#"
from flitzis_looper.controller.transport import TransportController
transport = TransportController(project, session, playback_audio)
transport.playback.pause_pad(0)
transport.playback.pause_pad(215)
assert not session.paused_sample_ids
assert all(transport.playback._pause_intent.requested_paused(pad) for pad in (0, 215))
"#,
        );
        remember_public_pause_admissions(&locals, true);
        assert_eq!(
            playback.drain(&mut h.consumer),
            2,
            "actual public Pause commands"
        );
        assert_public_pause_effect(&locals);
        controller_python(
            &locals,
            "transport.playback.poll_pause_observations()\nassert session.paused_sample_ids == {0, 215}",
        );
        playback
            .reference
            .pause_sample_at_output_frame(0, playback.frame);
        playback
            .full_mix
            .pause_sample_at_output_frame(0, playback.frame);
        playback.render_paused(513);
    }
    if selected_v1 {
        controller_python(
            &locals,
            "assert controller.set_stem_mix_mode(0, 'all_stems')\nfirst = controller._committed_residency._selections[0].receipt",
        );
        own_adoption(&mut playback, &mut h.consumer, &locals, "first", key_lock);
        assert!(playback.reference.set_stem_enabled_mask(
            0,
            0b1010,
            full_stems.source_version_hash
        ));
        assert!(playback.reference.set_stem_mix_mode(
            0,
            StemMixMode::AllStems,
            full_stems.source_version_hash
        ));
        playback.select(Some(0b1010));
        if key_lock {
            settle_wet(&mut playback, &locals, "first");
        } else {
            settle_varispeed(&mut playback, &locals, "first");
        }
    }
    if paused_live_bpm || paused_replacement {
        let before = voice(&playback.callback.mixer);
        let clock = before.source_playback;
        let generation = before.generation;
        let native = before.stretch.native_state_address();
        let fifo = before.stretch.pending_fifo_frames();
        let other_clock = other_voice(&playback.callback.mixer).source_playback;
        paused_other_clock = Some(other_clock);
        if paused_replacement {
            controller_python(
                &locals,
                "from flitzis_looper.controller.transport import TransportController\ntransport = TransportController(project, session, playback_audio)",
            );
        }
        controller_python(
            &locals,
            r#"
assert first.is_current() and first.effective_mode_status() == 'effective'
transport.playback.pause_pad(0)
transport.playback.pause_pad(215)
assert not session.paused_sample_ids
assert all(transport.playback._pause_intent.requested_paused(pad) for pad in (0, 215))
"#,
        );
        remember_public_pause_admissions(&locals, true);
        assert_eq!(
            playback.drain(&mut h.consumer),
            2,
            "actual public V1 Pause before V2 Capture"
        );
        assert_public_pause_effect(&locals);
        controller_python(
            &locals,
            "transport.playback.poll_pause_observations()\nassert session.paused_sample_ids == {0, 215}",
        );
        for reference in [&mut playback.reference, &mut playback.full_mix] {
            reference.pause_sample_at_output_frame(0, playback.frame);
        }
        playback.render_paused(513);
        let paused = voice(&playback.callback.mixer);
        assert!(paused.paused && paused.source_playback.matches_exact(&clock));
        assert_eq!(paused.generation, generation);
        assert_eq!(paused.stretch.native_state_address(), native);
        assert_eq!(paused.stretch.pending_fifo_frames(), fifo);
        let other = other_voice(&playback.callback.mixer);
        assert!(other.paused && other.source_playback.matches_exact(&other_clock));
        assert!(h.consumer.is_empty());
    }
    if public_rate_adoption {
        let start = playback.source_frame;
        live_other_ledger = Some(public_rate_before_candidate(
            &engine,
            &mut playback,
            &mut h.consumer,
            &mut parameter_consumer,
            &locals,
            accepted.unwrap(),
            key_lock,
        ));
        public_rate_start = Some(start);
    }
    if let Some(target_kind) = seek_target {
        public_explicit_seek(
            &engine,
            &mut playback,
            &mut h.consumer,
            &locals,
            key_lock,
            target_kind,
            accepted,
        );
        adopted_window = playback.callback.mixer.bank_for_measurement()[0].clone();
    }
    if explicit && paused_explicit {
        if !musical_explicit && seek_target.is_none() {
            controller_python(
                &locals,
                "from flitzis_looper.controller.transport import TransportController\ntransport = TransportController(project, session, playback_audio)",
            );
        }
        paused_other_clock = Some(other_voice(&playback.callback.mixer).source_playback);
        public_explicit_pause(&mut playback, &mut h.consumer, &locals);
    }
    let before = voice(&playback.callback.mixer);
    let initial_clock = before.source_playback;
    let initial_generation = before.generation;
    let initial_native = before.stretch.native_state_address();
    let initial_fifo = before.stretch.pending_fifo_frames();
    let initial_history = before.stretch.productive_history();
    assert_eq!(initial_history.is_some(), key_lock);
    let predecessor = playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .map(|stems| stems.complete_set_identity.clone());
    let final_old = final_reader.then(|| {
        FinalReaderProbe::new(
            &h.root,
            &fixture["pair"],
            playback.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap(),
        )
    });
    if disk_settlement {
        drop(predecessor);
        saved.release();
        l4_disk_settlement_tests::deferred_after_v1(
            &engine,
            &h.root,
            (&h.version, &h.wav_reference, full.frame_count()),
            &mut playback,
            (&mut h.consumer, &mut parameter_consumer),
            &locals,
            final_old.as_ref().unwrap(),
        );
        assert_eq!(
            fs::read(&h.material.lease.original_path).unwrap(),
            *original_before.as_ref().unwrap()
        );
        controller_python(
            &locals,
            "controller.shut_down()\ncontroller._assets.release_saved_assignments()",
        );
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    if saved_copy {
        controller_python(
            &locals,
            r#"
copy_id = 37
project.sample_paths[215] = project.sample_paths[0]
project.sample_durations[215] = project.sample_durations[0]
project.pad_content[215] = PadContentIdentity(instance_id='a' * 32, material_id=fixture['material_id'])
project.sample_paths[copy_id] = project.sample_paths[0]
project.sample_durations[copy_id] = project.sample_durations[0]
project.pad_content[copy_id] = PadContentIdentity(instance_id='9' * 32, material_id=fixture['material_id'])
project.stem_cache[copy_id] = project.stem_cache[0].model_copy(deep=True)
project.pad_stem_mix_mode[copy_id] = 'all_stems'
copy_content = project.pad_content[copy_id]
copy_entry = project.stem_cache[copy_id]
assert copy_content != project.pad_content[0]
assert copy_entry is not project.stem_cache[0] and copy_entry.pair == disk_v1
controller._assets.sync_assignments()
assert ('original', copy_id) in controller._assets._assignments
assert ('stems', copy_id) in controller._assets._assignments
assert not controller._committed_residency.contains(copy_id)
assert copy_id not in session.active_sample_ids
persistence.mark_dirty()
persistence.flush()
"#,
        );
    }
    let predecessor_set = (repeated_sets || (claimed_masks && selected_v1)).then(|| {
        playback.callback.mixer.stems_for_measurement()[0]
            .clone()
            .unwrap()
    });
    let (v2, source_ticket) = v2_files(
        &engine,
        &h.root,
        &h.version,
        &h.wav_reference,
        full.frame_count(),
    );
    let candidate_json = v2.selection_json().to_owned();
    assert_ne!(
        serde_json::from_str::<Value>(&candidate_json).unwrap(),
        fixture["pair"]
    );
    Python::attach(|py| {
        locals
            .bind(py)
            .set_item("candidate_json", candidate_json)
            .unwrap();
        locals
            .bind(py)
            .set_item("prepared_v2", Py::new(py, v2).unwrap())
            .unwrap();
        locals
            .bind(py)
            .set_item("source_v2", Py::new(py, source_ticket).unwrap())
            .unwrap();
    });
    controller_python(
        &locals,
        r#"
disk_v2 = StemPairSelection.model_validate_json(candidate_json)
entry_v1 = project.stem_cache[0]
entry_v2 = StemCacheEntry(source_version=fixture['source_version'], cache_dir=disk_v2.wav_generation,
    stems=expected_stem_files(disk_v2.wav_generation), pair=disk_v2, available=True)
# Both real complete assignment owners survive the actual fenced config replacement.
owner_v1 = audio.acquire_project_asset_lease(str(Path(entry_v1.cache_dir).absolute()))
owner_v2 = audio.acquire_project_asset_lease(str(Path(entry_v2.cache_dir).absolute()))
persistence.commit_stem_selection(0, entry_v1, entry_v2,
    lambda: project.stem_cache[0] is entry_v1 and source_v2.same_source_request(
        audio.capture_prepared_source(0, fixture['source_version'])))
reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert reopened.load_error is None
assert reopened.project.stem_cache[0].pair == disk_v2
project.stem_cache[0] = entry_v2
prepared_v2.select()
controller._register_committed_pair(0, prepared_v2, source_v2)
live = controller._committed_residency._live_selections[0]
assert live.control is previous_control and live.attempt is None
assert live.disk.selection.ticket.publication_status() == 'accepted'
assert previous_control.ticket.is_current()
status = controller.stem_residency_status(0)
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v1.stem_set_identity
assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if selected_v1 and not seek_requested else None)
"#,
    );
    if capture_deadline {
        pending_capture_deadline_then_explicit_retry(
            &engine,
            &mut playback,
            &mut h.consumer,
            &locals,
        );
    } else if queue_pressure {
        full_queue_then_explicit_retry(
            &engine,
            &h.producer,
            &mut playback,
            &mut h.consumer,
            &locals,
            key_lock,
            worker_post_pressure,
        );
    }
    controller_python(
        &locals,
        r#"
if repeated_sets:
    assert controller.set_stem_enabled_mask(0, 0)
if not queue_pressure and not capture_deadline:
    assert controller.set_stem_mix_mode(0, 'all_stems')
    assert controller._publish_all_stems_mode_if_preferred(0, fixture['source_version'])
candidate = live.attempt.receipt
assert candidate is not None and candidate.effective_mode_status() == 'waiting'
for _ in range(16):
    controller.on_frame_render()
assert candidate.effective_mode_status() == 'waiting'
assert previous_control.ticket.is_current()
"#,
    );
    if let Some(old) = &final_old {
        saved.release();
        controller_python(
            &locals,
            "controller._assets.sync_assignments()\naudio.retire_project_asset(str(Path(disk_v1.wav_generation).absolute()), True)\nowner_v1.release()",
        );
        old.assert_retained();
    }
    if !(queue_pressure || capture_deadline) {
        let current = voice(&playback.callback.mixer);
        assert!(current.source_playback.matches_exact(&initial_clock));
        assert_eq!(current.generation, initial_generation);
        assert_eq!(current.stretch.native_state_address(), initial_native);
        assert_eq!(current.stretch.pending_fifo_frames(), initial_fifo);
        if let Some(initial_history) = initial_history {
            let history = current.stretch.productive_history().unwrap();
            assert_eq!(history.binding, initial_history.binding);
            assert_eq!(history.next_position, initial_history.next_position);
            assert_eq!(history.fed_output_frames, initial_history.fed_output_frames);
        } else {
            assert!(current.stretch.productive_history().is_none());
        }
    }
    if withdraw_preclaim {
        withdraw_queued_candidate_for_full_mix(&mut playback, &mut h.consumer, &locals, key_lock);
    }
    if live_bpm {
        live_other_ledger = Some(live_bpm_then_explicit_retry(
            &engine,
            &mut playback,
            &mut h.consumer,
            &mut parameter_consumer,
            &locals,
            accepted.unwrap(),
            key_lock,
        ));
    }
    if retrigger {
        let (original, execution, generation) =
            public_play_then_explicit_retry(&mut playback, &mut h.consumer, &locals, key_lock);
        retriggered_at = Some((original, execution));
        adoption_generation = Some(generation);
    }
    if window_change {
        adopted_window = Some(public_window_then_explicit_retry(
            &mut playback,
            &mut h.consumer,
            &locals,
            key_lock,
        ));
    }
    if frozen_old {
        current_bank_candidate_cannot_borrow_frozen_voice(
            &engine,
            &h.root,
            &h.producer,
            &mut playback,
            &mut h.consumer,
            &locals,
            key_lock,
        );
        controller_python(
            &locals,
            "controller.shut_down()\ncontroller._assets.release_saved_assignments()\nowner_v1.release()\nowner_v2.release()",
        );
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    if unload {
        if unload_final_reader {
            // End this identity-only comparison alias before the real Unload.
            drop(predecessor);
        }
        public_unload_queued_candidate(
            &mut playback,
            &mut h.consumer,
            &mut parameter_consumer,
            &locals,
            key_lock,
            unload_final_reader.then(|| (h.root.as_path(), final_old.as_ref().unwrap())),
        );
        if unload_final_reader {
            assert_eq!(
                fs::read(&h.material.lease.original_path).unwrap(),
                *original_before.as_ref().unwrap()
            );
        }
        controller_python(
            &locals,
            "controller.shut_down()\ncontroller._assets.release_saved_assignments()\nowner_v1.release()\nowner_v2.release()",
        );
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    if source_replacement {
        let (expected_source, predecessor_generation, historical_adoption) =
            public_source_replacement(
                &engine,
                &mut playback,
                &mut h.consumer,
                &mut parameter_consumer,
                &locals,
                key_lock,
            );
        if source_play {
            let reference_owners = public_replaced_source_play(
                &mut playback,
                &mut h.consumer,
                &locals,
                expected_source,
                predecessor_generation,
                historical_adoption,
                trace,
            );
            if source_stems_off {
                public_replaced_source_stems(
                    &h.root,
                    &mut playback,
                    &mut h.consumer,
                    &locals,
                    false,
                );
            }
            if source_key_lock {
                public_replaced_source_key_lock_on(
                    &mut playback,
                    &mut h.consumer,
                    &locals,
                    reference_owners.unwrap(),
                );
            }
            if source_stems_on {
                public_replaced_source_stems(
                    &h.root,
                    &mut playback,
                    &mut h.consumer,
                    &locals,
                    true,
                );
            }
        }
        controller_python(
            &locals,
            "controller.shut_down()\ncontroller._assets.release_saved_assignments()\nowner_v1.release()\nowner_v2.release()",
        );
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    if full_track {
        // Live callbacks move the queued candidate near the complete-source seam.
        // Own Capture/Adopt still rechecks the continuously advancing real cursor.
        let phase =
            (playback.source_frame as f64 * playback.ratio).rem_euclid(playback.loop_period);
        let distance = (playback.loop_period - 64.0 - phase).rem_euclid(playback.loop_period);
        let mut remaining = (distance / playback.ratio).floor() as usize;
        while remaining != 0 {
            let count = remaining.min(511);
            if key_lock {
                playback.render(count, false);
            } else {
                render_varispeed(&mut playback, count);
            }
            remaining -= count;
        }
    }
    if public_rate_adoption {
        assert_eq!(
            playback.drain(&mut h.consumer),
            1,
            "fresh own V2 Capture during glide"
        );
        wait_until(|| h.consumer.peek().is_ok());
        for frames in [257, 1, 198] {
            if key_lock {
                playback.render(frames, false);
            } else {
                render_varispeed(&mut playback, frames);
            }
            assert_eq!(
                controller_ticket_status(&locals, "candidate", "residency_status"),
                "pending"
            );
            controller_python(
                &locals,
                "controller.on_frame_render()\nassert not candidate.claimed() and candidate.is_current()\nassert not session.stem_generation_errors, session.stem_generation_errors",
            );
        }
        own_adoption_after_capture(
            &mut playback,
            &mut h.consumer,
            &locals,
            "candidate",
            key_lock,
        );
        assert_eq!(playback.source_frame - public_rate_start.unwrap(), 500);
        assert_ne!(
            voice(&playback.callback.mixer)
                .source_playback
                .tempo_ratio()
                .to_bits(),
            playback.ratio.to_bits()
        );
    } else if preclaim_masks {
        supersede_queued_candidate_mask(
            &h.producer,
            &mut playback,
            &mut h.consumer,
            &locals,
            key_lock,
        );
        own_adoption_after_capture(
            &mut playback,
            &mut h.consumer,
            &locals,
            "candidate",
            key_lock,
        );
    } else {
        own_adoption(
            &mut playback,
            &mut h.consumer,
            &locals,
            "candidate",
            key_lock,
        );
    }
    assert_eq!(
        playback.callback.mixer.key_lock_for_measurement(0),
        key_lock
    );
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(0).1,
        if repeated_sets {
            0
        } else if preclaim_masks {
            0b0101
        } else {
            0b1010
        }
    );
    let current_source = playback.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    let expected_window = adopted_window.as_ref().unwrap_or(&finite);
    assert!(Arc::ptr_eq(
        &current_source.samples,
        &expected_window.samples
    ));
    assert_eq!(
        current_source.resident_binding(),
        expected_window.resident_binding()
    );
    let old_owner = predecessor.as_ref().map(Arc::downgrade);
    if explicit {
        assert_eq!(
            voice(&playback.callback.mixer).explicit_seek_mode,
            if intro {
                crate::audio_engine::source_reader::ExplicitSeekMode::BeforeLoop
            } else if tail {
                crate::audio_engine::source_reader::ExplicitSeekMode::AfterLoop
            } else {
                crate::audio_engine::source_reader::ExplicitSeekMode::Normal
            }
        );
        assert_eq!(
            voice(&playback.callback.mixer).source_timing.accepted,
            accepted
        );
        assert_eq!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, 0),
            accepted.map_or(0, |projection| projection.publication_epoch)
        );
        for set in [
            playback.callback.mixer.stems_for_measurement()[0].as_ref(),
            playback.callback.mixer.outgoing_stems_for_measurement(0),
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(set.accepted_timing, accepted);
        }
    }
    if complete_partial || (explicit && !normal_explicit && (!key_lock || intro)) {
        for set in [
            playback.callback.mixer.stems_for_measurement()[0].as_ref(),
            playback.callback.mixer.outgoing_stems_for_measurement(0),
        ]
        .into_iter()
        .flatten()
        {
            assert!(set.stems.iter().all(|component| {
                component.resident_start() == 0
                    && component.resident_end()
                        == (if key_lock && !complete_partial {
                            loop_region.end
                        } else {
                            full.frame_count()
                        })
                    && component.samples.len()
                        == (if key_lock && !complete_partial {
                            loop_region.end
                        } else {
                            full.frame_count()
                        }) * 2
            }));
        }
        if selected_v1 {
            assert!(
                playback
                    .callback
                    .mixer
                    .outgoing_stems_for_measurement(0)
                    .is_some()
            );
        }
    }
    let adopted_owner = &playback.callback.mixer.stems_for_measurement()[0]
        .as_ref()
        .unwrap()
        .complete_set_identity;
    let new_owner = Arc::downgrade(adopted_owner);
    if let Some(old) = predecessor {
        assert!(!Arc::ptr_eq(&old, adopted_owner));
    }
    assert_eq!(
        voice(&playback.callback.mixer).generation,
        adoption_generation.unwrap_or(initial_generation)
    );
    // Real V2 swaps vocals/drums: mask1010 maps to original0011 and0101 to1100.
    // An empty ALL mask stays Some(0), not FullMix.
    let oracle_mask = if repeated_sets {
        0
    } else if preclaim_masks {
        0b1100
    } else {
        0b0011
    };
    assert!(playback.reference.set_stem_enabled_mask(
        0,
        oracle_mask,
        full_stems.source_version_hash
    ));
    assert!(playback.reference.set_stem_mix_mode(
        0,
        StemMixMode::AllStems,
        full_stems.source_version_hash
    ));
    playback.select(Some(oracle_mask));
    if full_track {
        let phase = (playback.ramp_start as f64 * playback.ratio).rem_euclid(playback.loop_period);
        assert!(
            phase < playback.loop_period && phase + 128.0 > playback.loop_period,
            "public own handover crosses the complete-source seam"
        );
    }
    if public_rate_adoption {
        let boundary = public_rate_start.unwrap() + 512;
        assert_eq!(playback.ramp_start + 12, boundary);
        let ledger = playback.rate_ledger.as_ref().unwrap();
        assert_eq!(ledger.epoch(boundary).start, boundary);
        assert_ne!(
            ledger.epoch(boundary - 1).rate.to_bits(),
            ledger.epoch(boundary).rate.to_bits()
        );
        assert!(ledger.distance(boundary) - ledger.distance(playback.ramp_start) < 128.0);
    }
    controller_python(
        &locals,
        r#"
controller.on_frame_render()
status = controller.stem_residency_status(0)
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if selected_v1 and not withdraw_preclaim and not window_change and not seek_requested else None)
assert status.effective == 'transitioning'
assert candidate.claimed() and not candidate.withdraw()
assert not previous_control.ticket.is_current()
if queue_pressure:
    assert session.stem_generation_errors == {0: pressure_error}
if capture_deadline:
    assert session.stem_generation_errors == {0: deadline_error}
if live_bpm:
    assert session.stem_generation_errors == {0: bpm_error}
if retrigger:
    assert session.stem_generation_errors == {0: play_error}
if window_change:
    assert session.stem_generation_errors == {0: window_error}
"#,
    );
    if frozen_final_reader {
        let candidate: Value = Python::attach(|py| {
            serde_json::from_str(
                &locals
                    .bind(py)
                    .get_item("candidate_json")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
            )
            .unwrap()
        });
        let current = FinalReaderProbe::new(
            &h.root,
            &candidate,
            playback.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap(),
        );
        frozen_source_final_readers_then_public_unload(
            &engine,
            (&h.root, &h.producer),
            &mut playback,
            (&mut h.consumer, &mut parameter_consumer),
            &locals,
            (final_old.as_ref().unwrap(), &current),
            lifecycle,
        );
        assert_eq!(
            fs::read(&h.material.lease.original_path).unwrap(),
            *original_before.as_ref().unwrap()
        );
        controller_python(
            &locals,
            "controller.shut_down()\ncontroller._assets.release_saved_assignments()\nowner_v1.release()\nowner_v2.release()",
        );
        std::env::set_current_dir(previous_cwd).unwrap();
        return;
    }
    if repeated_sets {
        let (v3, source_ticket) = version_files(
            &engine,
            &h.root,
            &h.version,
            &h.wav_reference,
            full.frame_count(),
            FixtureSet::Third,
        );
        Python::attach(|py| {
            locals
                .bind(py)
                .set_item("third_json", v3.selection_json().to_owned())
                .unwrap();
            locals
                .bind(py)
                .set_item("prepared_v3", Py::new(py, v3).unwrap())
                .unwrap();
            locals
                .bind(py)
                .set_item("source_v3", Py::new(py, source_ticket).unwrap())
                .unwrap();
        });
        repeated_sets_after_empty_claim(
            &mut playback,
            &mut h.consumer,
            &locals,
            &full_stems,
            &predecessor_set.unwrap(),
            key_lock,
        );
    } else if claimed_masks {
        latest_masks_after_nonempty_claim(
            &mut playback,
            &mut h.consumer,
            &locals,
            &full_stems,
            predecessor_set.as_ref(),
            key_lock,
        );
    } else if let Some(paused) = delete_pause {
        delete_during_claimed_handover(
            &mut playback,
            &mut h.consumer,
            &locals,
            old_owner.expect("delete case starts with selected V1"),
            new_owner,
            paused,
        );
    } else {
        if paused_first || paused_replacement || paused_explicit {
            let source_frame = playback.source_frame;
            let fifo = voice(&playback.callback.mixer)
                .stretch
                .pending_fifo_frames();
            let transition = playback.callback.mixer.stem_transition_for_measurement(0);
            let reader_snapshot = |set: Option<&PreparedStemSet>| {
                set.map(|set| {
                    (
                        Arc::as_ptr(&set.complete_set_identity) as *const () as usize,
                        set.stems
                            .each_ref()
                            .map(|component| Arc::as_ptr(&component.samples) as *const () as usize),
                    )
                })
            };
            let paused_readers = paused_explicit.then(|| {
                [
                    playback.callback.mixer.stems_for_measurement()[0].as_ref(),
                    playback.callback.mixer.outgoing_stems_for_measurement(0),
                ]
                .map(reader_snapshot)
            });
            for frames in [1, 127, 384, 512] {
                playback.render_paused(frames);
                assert_eq!(playback.source_frame, source_frame);
                assert_eq!(
                    controller_ticket_status(&locals, "candidate", "effective_mode_status"),
                    "transitioning"
                );
                controller_python(
                    &locals,
                    r#"
controller.on_frame_render()
assert not session.stem_generation_errors, session.stem_generation_errors
if paused_replacement or paused_explicit:
    status = controller.stem_residency_status(0)
    assert status.registration == 'accepted' and status.residency == 'accepted'
    assert status.effective == 'transitioning'
    assert status.disk_stem_set_identity == disk_v2.stem_set_identity
    assert status.control_stem_set_identity == disk_v2.stem_set_identity
    assert status.effective_stem_set_identity == (disk_v1.stem_set_identity if paused_replacement or (complete_partial and selected_v1 and not seek_requested) else None)
    assert candidate.claimed() and candidate.is_current()
    assert audio.worker_call_count() == 1
"#,
                );
                assert!(
                    playback
                        .callback
                        .mixer
                        .stem_transition_for_measurement(0)
                        .matches_exact(transition)
                );
                if let Some(readers) = paused_readers {
                    assert_eq!(
                        [
                            playback.callback.mixer.stems_for_measurement()[0].as_ref(),
                            playback.callback.mixer.outgoing_stems_for_measurement(0),
                        ]
                        .map(reader_snapshot),
                        readers
                    );
                    assert_eq!(readers[1].is_some(), selected_v1);
                }
                if paused_replacement || paused_explicit {
                    let other = other_voice(&playback.callback.mixer);
                    assert!(
                        other.paused
                            && other
                                .source_playback
                                .matches_exact(&paused_other_clock.unwrap())
                    );
                    assert_eq!(other.generation, other_generation);
                    assert_eq!(other.stretch.native_state_address(), other_native);
                    assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
                    if selected_v1 {
                        assert!(
                            old_owner
                                .as_ref()
                                .expect("selected V1 retains its predecessor owner")
                                .upgrade()
                                .is_some()
                        );
                    } else {
                        assert!(old_owner.is_none());
                    }
                    assert!(Arc::ptr_eq(
                        &new_owner.upgrade().unwrap(),
                        &playback.callback.mixer.stems_for_measurement()[0]
                            .as_ref()
                            .unwrap()
                            .complete_set_identity
                    ));
                    assert!(h.consumer.is_empty());
                }
            }
            assert!(h.consumer.is_empty());
            controller_python(
                &locals,
                r#"
assert candidate.claimed() and candidate.is_current()
assert controller.stem_residency_status(0).effective == 'transitioning'
transport.playback.resume_pad(0)
transport.playback.resume_pad(215)
assert session.paused_sample_ids == {0, 215}
assert not any(transport.playback._pause_intent.requested_paused(pad) for pad in (0, 215))
"#,
            );
            remember_public_pause_admissions(&locals, false);
            assert_eq!(
                playback.drain(&mut h.consumer),
                2,
                "actual public Resume commands"
            );
            assert_public_pause_effect(&locals);
            controller_python(
                &locals,
                "transport.playback.poll_pause_observations()\nassert not session.paused_sample_ids",
            );
            playback
                .reference
                .resume_sample_at_output_frame(0, playback.frame);
            playback
                .full_mix
                .resume_sample_at_output_frame(0, playback.frame);
            playback.resumed_at = Some(playback.frame);
            let resumed = voice(&playback.callback.mixer);
            assert!(!resumed.paused);
            assert!(resumed.source_playback.matches_exact(&initial_clock));
            assert_eq!(resumed.generation, initial_generation);
            assert_eq!(resumed.stretch.native_state_address(), initial_native);
            assert_eq!(resumed.stretch.pending_fifo_frames(), fifo);
            if paused_replacement || paused_explicit {
                let other = other_voice(&playback.callback.mixer);
                assert!(
                    !other.paused
                        && other
                            .source_playback
                            .matches_exact(&paused_other_clock.unwrap())
                );
            }
        }
        if let Some(old) = &final_old {
            let candidate: Value = Python::attach(|py| {
                serde_json::from_str(
                    &locals
                        .bind(py)
                        .get_item("candidate_json")
                        .unwrap()
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                )
                .unwrap()
            });
            let current = FinalReaderProbe::new(
                &h.root,
                &candidate,
                playback.callback.mixer.stems_for_measurement()[0]
                    .as_ref()
                    .unwrap(),
            );
            productive_final_reader_after_claim(
                &engine,
                &mut playback,
                &locals,
                old,
                &current,
                domain_final_reader.then_some(&mut h.consumer),
                if saved_copy {
                    FinalReaderOutcome::SavedCopy
                } else if preserve_pcm {
                    FinalReaderOutcome::PreserveUnknownPcm
                } else {
                    FinalReaderOutcome::Cleanup
                },
            );
            if preserve_pcm {
                pcm_preservation_after_readers(
                    &engine,
                    &h.root,
                    &mut playback,
                    &locals,
                    (old, &current),
                );
            }
            if shared_job {
                l4_shared_job_tests::shared_job_after_origin_readers(
                    &engine,
                    &h.root,
                    &mut playback,
                    &mut h.consumer,
                    &mut parameter_consumer,
                    &locals,
                    &current,
                );
            }
            assert_eq!(
                fs::read(&h.material.lease.original_path).unwrap(),
                *original_before.as_ref().unwrap()
            );
            if saved_copy {
                saved_copy_after_origin_readers(
                    &engine,
                    &h.root,
                    &mut playback,
                    &mut h.consumer,
                    &mut parameter_consumer,
                    &locals,
                    (old, &current),
                );
                assert_eq!(
                    fs::read(&h.material.lease.original_path).unwrap(),
                    *original_before.as_ref().unwrap()
                );
                controller_python(
                    &locals,
                    "controller.shut_down()\ncontroller._assets.release_saved_assignments()\nowner_v1.release()\nowner_v2.release()",
                );
                std::env::set_current_dir(previous_cwd).unwrap();
                return;
            }
            if domain_final_reader {
                final_current = Some(current);
            }
        } else if key_lock {
            if public_rate_adoption {
                let own_adopt_frame = playback.frame;
                settle_wet_after(&mut playback, &locals, "candidate", Some(own_adopt_frame));
            } else {
                settle_wet(&mut playback, &locals, "candidate");
            }
        } else {
            settle_varispeed(&mut playback, &locals, "candidate");
        }
        controller_python(
            &locals,
            r#"
status = controller.stem_residency_status(0)
assert status.registration == 'accepted' and status.residency == 'accepted'
assert status.effective == 'effective'
assert status.disk_stem_set_identity == disk_v2.stem_set_identity
assert status.control_stem_set_identity == disk_v2.stem_set_identity
assert status.effective_stem_set_identity == disk_v2.stem_set_identity
assert project.stem_cache[0].pair == disk_v2
assert controller._stem_worker_pool._executor._max_workers == 2
if queue_pressure:
    assert live.control is pressure_selection and live.disk.selection is pressure_selection
    assert pressure_selection.ticket is pressure_registration and pressure_selection.receipt is candidate
    assert not session.stem_generation_errors, session.stem_generation_errors
    if worker_post_pressure:
        assert not failed_post.is_current() and not failed_post.claimed()
        assert failed_post.residency_status() == 'rejected' and failed_post.effective_mode_status() == 'rejected'
        assert failed_post.error() == pressure_error and failed_post.effective_failure_reason() is None
if window_change:
    assert live.control is fresh_window_selection and live.disk.selection is fresh_window_selection
    assert fresh_window_selection.ticket is fresh_window_registration and fresh_window_registration.is_current()
    assert fresh_window_selection.receipt is candidate and candidate.is_current() and candidate.claimed()
    assert not failed_window.is_current() and not failed_window.claimed()
    assert failed_window.residency_status() == 'rejected' and failed_window.error() == window_ownership_error
    assert failed_window.effective_failure_reason() is None
    assert window_selection.receipt is failed_window and not window_registration.is_current()
    assert not session.stem_generation_errors, session.stem_generation_errors
    persistence.mark_dirty()
    persistence.flush()
    window_reopened = ProjectPersistence.from_config_path(persistence.config_path)
    assert window_reopened.load_error is None
    assert window_reopened.project.stem_cache[0].pair == disk_v2
    assert window_reopened.project.pad_loop_start_s[0] == 17 / fixture['rate']
    assert window_reopened.project.pad_loop_end_s[0] == 1581 / fixture['rate']
    assert window_reopened.project.pad_key_lock[0] is window_key_lock
if withdraw_preclaim:
    assert not withdrawn.claimed() and not withdrawn.is_current()
    assert withdrawn.residency_status() == 'rejected' and withdrawn.effective_mode_status() == 'rejected'
    assert withdrawn.effective_failure_reason() is None
if capture_deadline:
    assert live.control is deadline_selection and live.disk.selection is deadline_selection
    assert deadline_selection.ticket is deadline_registration and deadline_selection.receipt is candidate
    assert not failed_capture.is_current() and not failed_capture.claimed()
    assert failed_capture.residency_status() == 'rejected' and failed_capture.error() == deadline_error
    assert not session.stem_generation_errors, session.stem_generation_errors
if preclaim_masks:
    assert live.control is latest_selection and live.disk.selection is latest_selection
    assert latest_selection.ticket is fresh_registration and fresh_registration.is_current()
    assert latest_selection.receipt is candidate and candidate.is_current() and candidate.claimed()
    assert latest_selection.receipt_intent == ('all_stems', 0b0101)
    assert live.desired == ('all_stems', 0b0101) and project.pad_stem_intent[0].enabled_mask == 0b0101
    assert superseded_selection.receipt is superseded
    assert not superseded.is_current() and not superseded.claimed()
    assert superseded.residency_status() == 'rejected' and superseded.effective_mode_status() == 'rejected'
    assert superseded.effective_failure_reason() is None
    assert not superseded_registration.is_current() and superseded_registration.error() is None
    assert not session.stem_generation_errors, session.stem_generation_errors
if retrigger:
    assert live.control is fresh_play_selection and live.disk.selection is fresh_play_selection
    assert fresh_play_selection.ticket is fresh_play_registration and fresh_play_registration.is_current()
    assert fresh_play_selection.receipt is candidate and candidate.is_current() and candidate.claimed()
    assert not failed_play.is_current() and not failed_play.claimed()
    assert failed_play.residency_status() == 'rejected' and failed_play.error() == play_error
    assert failed_play.effective_failure_reason() is None
    assert play_selection.receipt is failed_play and not play_registration.is_current()
    assert not session.stem_generation_errors, session.stem_generation_errors
"#,
        );
    }
    if explicit {
        prove_public_loop_return(&mut playback, key_lock, accepted);
        controller_python(
            &locals,
            r#"
controller.on_frame_render()
assert candidate.is_current() and candidate.effective_mode_status() == 'effective'
assert candidate.error() is None and candidate.effective_failure_reason() is None
assert not session.stem_generation_errors, session.stem_generation_errors
persistence.mark_dirty()
persistence.flush()
tail_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert tail_reopened.load_error is None
assert tail_reopened.project.stem_cache[0].pair == disk_v2
assert tail_reopened.project.pad_loop_start_s[0] == declared_loop_start / fixture['rate']
assert tail_reopened.project.pad_loop_end_s[0] == declared_loop_end / fixture['rate']
assert tail_reopened.project.pad_key_lock[0] is tail_key_lock
assert tail_reopened.project.pad_timing_intent[0] == ('automatic' if musical_explicit else 'legacy')
assert audio.worker_call_count() == 1 and audio.prepared_only_on_pool_threads()
"#,
        );
    }
    if matches!(trace, CandidateTrace::PublicBpm(bpm) if bpm == 149.9) {
        render_public_bpm_tail_sample(&mut playback);
    }
    if public_bpm || musical_explicit {
        let current = engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap();
        assert_eq!(current.binding.accepted, accepted);
        assert_eq!(
            engine
                .current_timing_acknowledgements
                .current_for_slot(&engine.input_runtime_ownership, 0),
            accepted.unwrap().publication_epoch
        );
        controller_python(
            &locals,
            r#"
assert transport.bpm.current_timing(0) == timing_before_bpm
assert session.bpm_lock_anchor_revision == timing_before_bpm.accepted_revision
assert session.master_period_seconds == accepted_period / project.speed
persistence.mark_dirty()
persistence.flush()
bpm_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert bpm_reopened.load_error is None
assert bpm_reopened.project.bpm_lock and bpm_reopened.project.speed == project.speed
if musical_explicit:
    assert project.speed == accepted_period * (target_bpm / 60)
    assert bpm_reopened.project.speed == accepted_period * (target_bpm / 60)
assert bpm_reopened.project.manual_bpm[0] is None
assert bpm_reopened.project.pad_timing_intent[0] == 'automatic'
assert bpm_reopened.project.stem_cache[0].pair == disk_v2
"#,
        );
    }
    let other = other_voice(&playback.callback.mixer);
    assert_eq!(other.generation, other_generation);
    assert_eq!(other.stretch.native_state_address(), other_native);
    assert_eq!(other.stretch.pending_fifo_frames(), other_fifo);
    if let Some(ledger) = live_other_ledger {
        let (whole, fraction) = ledger.position(playback.source_frame);
        let position = other.source_playback.position();
        assert_eq!(position.frame, LOOP_START + whole);
        assert_eq!(position.fraction.to_bits(), fraction.to_bits());
        assert_eq!(
            other.source_playback.tempo_ratio().to_bits(),
            ledger.epoch(playback.source_frame - 1).rate.to_bits()
        );
        assert_eq!(other.source_playback.loop_period(), None);
        controller_python(
            &locals,
            r#"
assert not session.stem_generation_errors, session.stem_generation_errors
if live_bpm:
    assert not failed_bpm.is_current() and not failed_bpm.claimed()
    assert failed_bpm.residency_status() == 'rejected' and failed_bpm.error() == bpm_error
    assert failed_bpm.effective_failure_reason() is None
    assert live.control is bpm_selection and live.disk.selection is bpm_selection
    assert bpm_selection.ticket is bpm_registration and bpm_selection.receipt is candidate
else:
    assert candidate.is_current() and candidate.claimed()
    assert candidate.residency_status() == 'accepted' and candidate.effective_mode_status() == 'effective'
    assert candidate.error() is None and candidate.effective_failure_reason() is None
    assert live.control is live.disk.selection and live.control.receipt is candidate
    assert audio.worker_call_count() == 1
"#,
        );
    } else if let CandidateTrace::PublicBpm(target_bpm) = trace {
        // The other pad has no accepted timing and keeps its own physical loop.
        let other_ratio = accepted.unwrap().period_seconds * (target_bpm / 60.0);
        let distance = playback.source_frame as f64 * other_ratio;
        let phase = distance.rem_euclid((LOOP_END - LOOP_START) as f64);
        let position = other.source_playback.position();
        assert_eq!(
            other.source_playback.tempo_ratio().to_bits(),
            other_ratio.to_bits()
        );
        assert_eq!(other.source_playback.loop_period(), None);
        assert_eq!(position.frame, LOOP_START + phase.floor() as usize);
        assert_eq!(position.fraction.to_bits(), distance.fract().to_bits());
    } else if paused_replacement || paused_explicit || domain_final_reader {
        assert!(playback.frame > playback.source_frame);
        let phase = (playback.source_frame as f64 * playback.ratio)
            .rem_euclid((LOOP_END - LOOP_START) as f64);
        assert_eq!(
            other.source_playback.position().frame,
            LOOP_START + phase.floor() as usize
        );
        assert_eq!(
            other.source_playback.position().fraction.to_bits(),
            phase.fract().to_bits()
        );
        assert_eq!(
            other.source_playback.tempo_ratio().to_bits(),
            playback.ratio.to_bits()
        );
    } else if window_change || retriggered_at.is_some() || full_track || explicit {
        // The independent source counter starts with actual emitted birth E.
        // Requested T remains separately retained by the original public owner.
        assert_eq!(
            playback.frame - playback.source_frame,
            retriggered_at.map_or(0, |(_original, execution)| execution)
        );
        let phase =
            (playback.frame as f64 * playback.ratio).rem_euclid((LOOP_END - LOOP_START) as f64);
        let position = other.source_playback.position();
        assert_eq!(position.frame, LOOP_START + phase.floor() as usize);
        assert_eq!(position.fraction.to_bits(), phase.fract().to_bits());
        assert_eq!(
            other.source_playback.tempo_ratio().to_bits(),
            playback.ratio.to_bits()
        );
    } else {
        assert!(
            other
                .source_playback
                .matches_exact(&voice(&playback.callback.mixer).source_playback)
        );
    }
    assert!(Arc::ptr_eq(
        &other.sample.as_ref().unwrap().samples,
        &other_source.samples
    ));
    assert_eq!(
        playback.callback.mixer.stem_demand_for_measurement(215),
        other_demand
    );
    assert!(playback.callback.mixer.stems_for_measurement()[215].is_none());
    if domain_final_reader {
        if full_track {
            controller_python(
                &locals,
                r#"
persistence.mark_dirty()
persistence.flush()
full_reopened = ProjectPersistence.from_config_path(persistence.config_path)
assert full_reopened.load_error is None
assert full_reopened.project.stem_cache[0].pair == disk_v2
assert full_reopened.project.pad_loop_start_s[0] == declared_loop_start / fixture['rate']
assert full_reopened.project.pad_loop_end_s[0] == declared_loop_end / fixture['rate']
assert full_reopened.project.pad_key_lock[0] is True
assert full_reopened.project.pad_timing_intent[0] == 'legacy'
"#,
            );
        }
        public_unload_effective_pair(
            &engine,
            &mut playback,
            &mut h.consumer,
            &mut parameter_consumer,
            &locals,
            final_current.as_ref().unwrap(),
            None,
        );
        let old = final_old.as_ref().unwrap();
        assert_eq!(old.identity.strong_count(), 0);
        assert!(old.pcm.iter().all(|pcm| pcm.strong_count() == 0));
        assert!(old.areas.iter().all(|path| !path.exists()));
        assert_eq!(
            fs::read(&h.material.lease.original_path).unwrap(),
            *original_before.as_ref().unwrap()
        );
    }
    controller_python(
        &locals,
        "controller.shut_down()\ncontroller._assets.release_saved_assignments()\nowner_v1.release()\nowner_v2.release()",
    );
    std::env::set_current_dir(previous_cwd).unwrap();
}

#[test]
fn actual_paired_pcm_preserves_unknown_child_after_native_final_readers() {
    if std::env::var_os(PCM_PRESERVATION_CHILD).is_some() {
        round_trip(
            true,
            None,
            true,
            None,
            false,
            CandidateTrace::PostAdmitPcmFinalReader,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PCM_PRESERVATION_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PCM_PRESERVATION_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L4 post-admission PCM child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_saved_copy_retains_v1_after_origin_final_readers_and_unload() {
    if std::env::var_os(SAVED_COPY_CHILD).is_some() {
        round_trip(
            true,
            None,
            true,
            None,
            false,
            CandidateTrace::SavedCopyFinalReader,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            SAVED_COPY_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SAVED_COPY_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L4 saved-copy child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_unload_waits_for_queued_candidate_and_final_reader_cleanup() {
    if std::env::var_os(UNLOAD_FINAL_READER_CHILD).is_some() {
        round_trip(
            true,
            None,
            true,
            None,
            false,
            CandidateTrace::UnloadFinalReader,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            UNLOAD_FINAL_READER_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(UNLOAD_FINAL_READER_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L4 Unload final-reader child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_productive_replacement_waits_for_last_native_reader_and_asset_worker() {
    if std::env::var_os(FINAL_READER_CHILD).is_some() {
        round_trip(true, None, true, None, false, CandidateTrace::FinalReader);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            FINAL_READER_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(FINAL_READER_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L4 final-reader child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_unload_rejects_queued_v2_before_own_stop_with_keylock_on() {
    unload_child(true, UNLOAD_ON_CHILD, UNLOAD_ON_TEST);
}

#[test]
fn actual_native_current_bank_candidate_cannot_borrow_frozen_old_voice_with_keylock_on() {
    frozen_old_child(true, FROZEN_ON_CHILD, FROZEN_ON_TEST);
}

#[test]
fn actual_native_current_bank_candidate_cannot_borrow_frozen_old_voice_with_keylock_off() {
    frozen_old_child(false, FROZEN_OFF_CHILD, FROZEN_OFF_TEST);
}

fn frozen_old_child(key_lock: bool, child: &str, test: &str) {
    if std::env::var_os(child).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::FrozenOld,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(child, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 Native frozen old voice child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_unload_rejects_queued_v2_before_own_stop_with_keylock_off() {
    unload_child(false, UNLOAD_OFF_CHILD, UNLOAD_OFF_TEST);
}

fn unload_child(key_lock: bool, child: &str, test: &str) {
    if std::env::var_os(child).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::Unload,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(child, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public Unload child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_all_of_replaced_source_requires_own_wet_stem_effect() {
    if std::env::var_os(SOURCE_STEMS_ON_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::SourceStemsOn,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            SOURCE_STEMS_ON_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOURCE_STEMS_ON_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 replaced-source own wet stems child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_keylock_on_of_replaced_source_requires_own_wet_confirmation() {
    if std::env::var_os(SOURCE_KEY_LOCK_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::SourceKeyLockOn,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            SOURCE_KEY_LOCK_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOURCE_KEY_LOCK_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 replaced-source own KEY LOCK ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_all_of_replaced_source_requires_own_dry_stem_effect() {
    if std::env::var_os(SOURCE_STEMS_OFF_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::SourceStemsOff,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            SOURCE_STEMS_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOURCE_STEMS_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 replaced-source own dry stems child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_play_of_replaced_source_requires_own_voice_after_keylock_on() {
    if std::env::var_os(SOURCE_PLAY_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::SourcePlay,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            SOURCE_PLAY_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOURCE_PLAY_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 replaced-source public Play after ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_play_of_replaced_source_requires_own_voice_after_keylock_off() {
    if std::env::var_os(SOURCE_PLAY_OFF_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::SourcePlay,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            SOURCE_PLAY_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOURCE_PLAY_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 replaced-source public Play after OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_source_replacement_fences_queued_v2_before_own_ack_with_keylock_on() {
    if std::env::var_os(SOURCE_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::SourceReplacement,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", SOURCE_TEST, "--nocapture", "--test-threads=1"])
        .env(SOURCE_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public source replacement ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_source_replacement_fences_queued_v2_before_own_ack_with_keylock_off() {
    if std::env::var_os(SOURCE_OFF_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::SourceReplacement,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            SOURCE_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SOURCE_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public source replacement OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_window_rejects_queued_v2_then_requires_fresh_wet_preparation() {
    if std::env::var_os(WINDOW_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::WindowChange,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", WINDOW_TEST, "--nocapture", "--test-threads=1"])
        .env(WINDOW_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public changed window ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_window_rejects_queued_v2_then_requires_fresh_dry_preparation() {
    if std::env::var_os(WINDOW_OFF_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::WindowChange,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            WINDOW_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(WINDOW_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public changed window child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_play_rejects_queued_v2_then_new_voice_requires_own_wet_ack() {
    if std::env::var_os(RETRIGGER_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::Retrigger,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", RETRIGGER_TEST, "--nocapture", "--test-threads=1"])
        .env(RETRIGGER_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public Play ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_play_rejects_queued_v2_then_new_voice_requires_own_dry_ack() {
    if std::env::var_os(RETRIGGER_OFF_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::Retrigger,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            RETRIGGER_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(RETRIGGER_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public Play OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_preclaim_mask_supersession_requires_own_wet_ack() {
    if std::env::var_os(PRECLAIM_MASKS_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::PreclaimMasks,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PRECLAIM_MASKS_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PRECLAIM_MASKS_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 preclaim mask ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_preclaim_mask_supersession_requires_own_dry_ack() {
    if std::env::var_os(PRECLAIM_MASKS_OFF_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::PreclaimMasks,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PRECLAIM_MASKS_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PRECLAIM_MASKS_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 preclaim mask OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_latest_masks_during_nonempty_v2_claim_require_own_wet_ack() {
    if std::env::var_os(CLAIMED_MASKS_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::ClaimedMasks,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CLAIMED_MASKS_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CLAIMED_MASKS_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 claimed latest masks ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_latest_masks_during_nonempty_v2_claim_require_own_dry_ack() {
    if std::env::var_os(CLAIMED_MASKS_OFF_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::ClaimedMasks,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CLAIMED_MASKS_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CLAIMED_MASKS_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 claimed latest masks OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_live_bpm_paused_v1_rejects_then_resumes_and_retries_with_keylock_on() {
    if std::env::var_os(PAUSED_LIVE_BPM_CHILD).is_some() {
        round_trip(true, None, true, None, false, CandidateTrace::PausedLiveBpm);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PAUSED_LIVE_BPM_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PAUSED_LIVE_BPM_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public paused live BPM ON child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_live_bpm_paused_v1_rejects_then_resumes_and_retries_with_keylock_off() {
    if std::env::var_os(PAUSED_LIVE_BPM_OFF_CHILD).is_some() {
        round_trip(
            true,
            None,
            false,
            None,
            false,
            CandidateTrace::PausedLiveBpm,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PAUSED_LIVE_BPM_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PAUSED_LIVE_BPM_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public paused live BPM OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_live_bpm_keylock_off_rejects_queued_v2_then_own_dry_retry() {
    if std::env::var_os(LIVE_BPM_OFF_CHILD).is_some() {
        round_trip(true, None, false, None, false, CandidateTrace::LiveBpm);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            LIVE_BPM_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(LIVE_BPM_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public live BPM OFF child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_live_bpm_rejects_queued_v2_then_explicit_retry_keeps_wet_chronology() {
    if std::env::var_os(LIVE_BPM_CHILD).is_some() {
        round_trip(true, None, true, None, false, CandidateTrace::LiveBpm);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", LIVE_BPM_TEST, "--nocapture", "--test-threads=1"])
        .env(LIVE_BPM_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public live BPM child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_bpm_target_then_own_replacement_preserves_musical_wet_chronology() {
    if std::env::var_os(PUBLIC_BPM_CHILD).is_some() {
        for bpm in [150.0, 149.9] {
            round_trip(
                true,
                None,
                true,
                None,
                false,
                CandidateTrace::PublicBpm(bpm),
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PUBLIC_BPM_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PUBLIC_BPM_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public BPM child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_first_and_replacement_candidates_keep_own_wet_chronology() {
    if std::env::var_os(CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                true,
                None,
                false,
                CandidateTrace::Ordinary,
            );
        }
        for paused in [false, true] {
            round_trip(
                true,
                Some(paused),
                true,
                None,
                false,
                CandidateTrace::Ordinary,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 productive controller child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_keylock_off_first_and_replacement_keep_varispeed_feed_and_eq() {
    if std::env::var_os(VARISPEED_CHILD).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                false,
                None,
                false,
                CandidateTrace::Ordinary,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", VARISPEED_TEST, "--nocapture", "--test-threads=1"])
        .env(VARISPEED_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 Varispeed controller child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_stopped_replacement_play_then_fresh_all_uses_own_dry_receipt() {
    if std::env::var_os(STOPPED_NEW_SET_OFF_CHILD).is_some() {
        for before_poll in [false, true] {
            round_trip(
                false,
                None,
                false,
                Some(before_poll),
                false,
                CandidateTrace::Ordinary,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            STOPPED_NEW_SET_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(STOPPED_NEW_SET_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 stopped OFF replacement child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_paused_v1_replacement_requires_own_resumed_wet_effect() {
    paused_replacement_child(
        true,
        PAUSED_REPLACEMENT_ON_CHILD,
        PAUSED_REPLACEMENT_ON_TEST,
    );
}

#[test]
fn actual_public_bpm_glide_adoption_requires_own_wet_effect() {
    public_rate_adoption_child(true, PUBLIC_RATE_ON_CHILD, PUBLIC_RATE_ON_TEST);
}

#[test]
fn actual_public_bpm_glide_adoption_requires_own_dry_effect() {
    public_rate_adoption_child(false, PUBLIC_RATE_OFF_CHILD, PUBLIC_RATE_OFF_TEST);
}

fn public_rate_adoption_child(key_lock: bool, child: &str, test: &str) {
    if std::env::var_os(child).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::PublicRateAdoption,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(child, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public rate adoption child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_paused_v1_replacement_requires_own_resumed_dry_effect() {
    paused_replacement_child(
        false,
        PAUSED_REPLACEMENT_OFF_CHILD,
        PAUSED_REPLACEMENT_OFF_TEST,
    );
}

fn paused_replacement_child(key_lock: bool, child: &str, test: &str) {
    if std::env::var_os(child).is_some() {
        round_trip(
            true,
            None,
            key_lock,
            None,
            false,
            CandidateTrace::PausedReplacement,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(child, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 public paused replacement child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_stopped_replacement_play_then_fresh_all_uses_own_wet_receipt() {
    if std::env::var_os(STOPPED_NEW_SET_CHILD).is_some() {
        for before_poll in [false, true] {
            round_trip(
                false,
                None,
                true,
                Some(before_poll),
                false,
                CandidateTrace::Ordinary,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            STOPPED_NEW_SET_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(STOPPED_NEW_SET_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 stopped replacement child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_wet_full_mix_paused_before_first_candidate_keeps_consumed_clock() {
    if std::env::var_os(PAUSED_FIRST_CHILD).is_some() {
        round_trip(false, None, true, None, true, CandidateTrace::Ordinary);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PAUSED_FIRST_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PAUSED_FIRST_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 paused-first child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_empty_handover_defers_v3_and_uses_latest_mask_with_own_ack() {
    if std::env::var_os(REPEATED_SETS_CHILD).is_some() {
        for key_lock in [true, false] {
            round_trip(
                true,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::RepeatedSets,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            REPEATED_SETS_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(REPEATED_SETS_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 repeated sets child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_full_mix_withdraws_queued_v2_adopt_then_all_uses_fresh_authority() {
    if std::env::var_os(WITHDRAW_PRECLAIM_CHILD).is_some() {
        for key_lock in [true, false] {
            round_trip(
                true,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::WithdrawPreclaim,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            WITHDRAW_PRECLAIM_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(WITHDRAW_PRECLAIM_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 preclaim withdrawal child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_queue_full_keeps_v1_then_explicit_all_admits_own_v2() {
    if std::env::var_os(ADMISSION_PRESSURE_CHILD).is_some() {
        for key_lock in [true, false] {
            round_trip(
                true,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::AdmissionPressure,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            ADMISSION_PRESSURE_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ADMISSION_PRESSURE_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 admission pressure child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_worker_adopt_queue_failure_keeps_v1_then_own_explicit_retry() {
    if std::env::var_os(WORKER_POST_PRESSURE_CHILD).is_some() {
        for key_lock in [true, false] {
            round_trip(
                true,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::WorkerPostPressure,
            );
        }
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            WORKER_POST_PRESSURE_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(WORKER_POST_PRESSURE_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 worker post pressure child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_pending_capture_deadline_keeps_v1_then_fresh_own_retry() {
    if std::env::var_os(CAPTURE_DEADLINE_CHILD).is_some() {
        round_trip(
            true,
            None,
            true,
            None,
            false,
            CandidateTrace::CaptureDeadline,
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CAPTURE_DEADLINE_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CAPTURE_DEADLINE_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 Capture deadline child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_python_controller_varispeed_paused_before_first_candidate_keeps_consumed_clock() {
    if std::env::var_os(PAUSED_FIRST_OFF_CHILD).is_some() {
        round_trip(false, None, false, None, true, CandidateTrace::Ordinary);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            PAUSED_FIRST_OFF_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PAUSED_FIRST_OFF_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "L3 OFF paused-first child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_full_track_first_and_replacement_require_own_wet_effect() {
    full_track_public_child(true);
}

#[test]
fn actual_public_frozen_source_final_reader_unfinished_handover_unload_cleans_both_pairs() {
    residual_domain_child(
        "actual_public_frozen_source_final_reader_unfinished_handover_unload_cleans_both_pairs",
        || {
            round_trip_with_lifecycle(
                true,
                None,
                true,
                None,
                false,
                CandidateTrace::Ordinary,
                CandidateLifecycle::FrozenUnload,
            )
        },
    );
}

#[test]
fn actual_public_frozen_source_final_reader_resume_retires_outgoing_before_current_unload() {
    residual_domain_child(
        "actual_public_frozen_source_final_reader_resume_retires_outgoing_before_current_unload",
        || {
            round_trip_with_lifecycle(
                true,
                None,
                true,
                None,
                false,
                CandidateTrace::Ordinary,
                CandidateLifecycle::FrozenResumeReaders,
            )
        },
    );
}

#[test]
fn actual_public_paused_frozen_source_readers_refuse_current_bank_resume_until_own_unload() {
    residual_domain_child(
        "actual_public_paused_frozen_source_readers_refuse_current_bank_resume_until_own_unload",
        || {
            round_trip_with_lifecycle(
                true,
                None,
                true,
                None,
                false,
                CandidateTrace::Ordinary,
                CandidateLifecycle::FrozenPausedUnload,
            )
        },
    );
}

#[test]
fn actual_public_domain_final_reader_replacement_cleans_after_busy_pause_and_own_unload() {
    residual_domain_child(
        "actual_public_domain_final_reader_replacement_cleans_after_busy_pause_and_own_unload",
        || {
            let cases = [
                (
                    "finite musical Intro",
                    CandidateTrace::Explicit {
                        intro: true,
                        paused: false,
                        musical: true,
                    },
                ),
                (
                    "finite musical Tail",
                    CandidateTrace::Explicit {
                        intro: false,
                        paused: false,
                        musical: true,
                    },
                ),
                (
                    "changed finite musical Normal Seek",
                    CandidateTrace::NormalSeek {
                        equal: false,
                        paused: false,
                        musical: true,
                    },
                ),
                (
                    "equal finite musical Normal Seek",
                    CandidateTrace::NormalSeek {
                        equal: true,
                        paused: false,
                        musical: true,
                    },
                ),
                (
                    "complete proper-partial musical Normal",
                    CandidateTrace::CompletePartial {
                        seek: None,
                        paused: false,
                        stopped: false,
                        musical: true,
                    },
                ),
                (
                    "actual complete ON musical Intro",
                    CandidateTrace::CompletePartial {
                        seek: Some(true),
                        paused: false,
                        stopped: false,
                        musical: true,
                    },
                ),
                (
                    "actual complete ON musical Tail",
                    CandidateTrace::CompletePartial {
                        seek: Some(false),
                        paused: false,
                        stopped: false,
                        musical: true,
                    },
                ),
                (
                    "complete full-source physical Normal",
                    CandidateTrace::FullTrack,
                ),
            ];
            assert_eq!(cases.len(), 8);
            for (name, trace) in cases {
                eprintln!("domain final reader replacement: {name}");
                round_trip_with_lifecycle(
                    true,
                    None,
                    true,
                    None,
                    false,
                    trace,
                    CandidateLifecycle::CleanupAfterClaim,
                );
            }
        },
    );
}

#[test]
fn actual_public_domain_final_reader_queued_unload_cleans_finite_tail_and_complete_intro() {
    residual_domain_child(
        "actual_public_domain_final_reader_queued_unload_cleans_finite_tail_and_complete_intro",
        || {
            let cases = [
                (
                    "finite musical Tail",
                    CandidateTrace::Explicit {
                        intro: false,
                        paused: false,
                        musical: true,
                    },
                ),
                (
                    "actual complete ON musical Intro",
                    CandidateTrace::CompletePartial {
                        seek: Some(true),
                        paused: false,
                        stopped: false,
                        musical: true,
                    },
                ),
            ];
            assert_eq!(cases.len(), 2);
            for (name, trace) in cases {
                eprintln!("domain final reader queued Unload: {name}");
                round_trip_with_lifecycle(
                    true,
                    None,
                    true,
                    None,
                    false,
                    trace,
                    CandidateLifecycle::QueuedUnloadCleanup,
                );
            }
        },
    );
}

#[test]
fn actual_public_residual_domain_in_loop_and_equal_seek_requires_own_new_set_effect() {
    residual_domain_child(
        "actual_public_residual_domain_in_loop_and_equal_seek_requires_own_new_set_effect",
        || {
            let mut histories = 0;
            for equal in [false, true] {
                for key_lock in [false, true] {
                    for musical in [false, true] {
                        for paused in [false, true] {
                            for selected_v1 in [false, true] {
                                round_trip(
                                    selected_v1,
                                    None,
                                    key_lock,
                                    None,
                                    false,
                                    CandidateTrace::NormalSeek {
                                        equal,
                                        paused,
                                        musical,
                                    },
                                );
                                histories += 1;
                            }
                        }
                    }
                }
            }
            assert_eq!(histories, 32);
        },
    );
}

#[test]
fn actual_public_residual_domain_complete_partial_normal_requires_own_new_set_effect() {
    residual_domain_child(
        "actual_public_residual_domain_complete_partial_normal_requires_own_new_set_effect",
        || {
            let mut histories = 0;
            for key_lock in [false, true] {
                for musical in [false, true] {
                    for state in 0..3 {
                        for selected_v1 in [false, true] {
                            round_trip(
                                selected_v1,
                                None,
                                key_lock,
                                None,
                                false,
                                CandidateTrace::CompletePartial {
                                    seek: None,
                                    paused: state == 1,
                                    stopped: state == 2,
                                    musical,
                                },
                            );
                            histories += 1;
                        }
                    }
                }
            }
            assert_eq!(histories, 24);
        },
    );
}

#[test]
fn actual_public_residual_domain_keylock_full_track_intro_tail_requires_own_effect_and_return() {
    residual_domain_child(
        "actual_public_residual_domain_keylock_full_track_intro_tail_requires_own_effect_and_return",
        || {
            let mut histories = 0;
            for intro in [false, true] {
                for musical in [false, true] {
                    for paused in [false, true] {
                        for selected_v1 in [false, true] {
                            round_trip(
                                selected_v1,
                                None,
                                true,
                                None,
                                false,
                                CandidateTrace::CompletePartial {
                                    seek: Some(intro),
                                    paused,
                                    stopped: false,
                                    musical,
                                },
                            );
                            histories += 1;
                        }
                    }
                }
            }
            assert_eq!(histories, 16);
        },
    );
}

fn residual_domain_child(name: &str, run: impl FnOnce()) {
    const RESIDUAL_CHILD: &str = "FLITZI_L3_PUBLIC_RESIDUAL_DOMAIN_CHILD";
    if std::env::var_os(RESIDUAL_CHILD).is_some() {
        run();
        return;
    }
    let test = format!(
        "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::{name}"
    );
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(RESIDUAL_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public residual domain child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn productive_saved_key_lock_public_canonical_restore_play_preserves_own_native_chronology() {
    residual_domain_child(
        "productive_saved_key_lock_public_canonical_restore_play_preserves_own_native_chronology",
        saved_keylock_controller_tests::run,
    );
}

#[test]
fn actual_public_full_track_first_and_replacement_require_own_dry_effect() {
    full_track_public_child(false);
}

fn full_track_public_child(key_lock: bool) {
    let (child, name) = if key_lock {
        (
            "FLITZI_L3_FULL_TRACK_ON_CHILD",
            "actual_public_full_track_first_and_replacement_require_own_wet_effect",
        )
    } else {
        (
            "FLITZI_L3_FULL_TRACK_OFF_CHILD",
            "actual_public_full_track_first_and_replacement_require_own_dry_effect",
        )
    };
    if std::env::var_os(child).is_some() {
        for selected_v1 in [false, true] {
            round_trip(
                selected_v1,
                None,
                key_lock,
                None,
                false,
                CandidateTrace::FullTrack,
            );
        }
        return;
    }
    let test = format!(
        "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::{name}"
    );
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(child, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "FullTrack public child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn actual_public_tail_first_and_replacement_require_own_wet_effect_and_return() {
    explicit_public_child(true, false, false);
}

#[test]
fn actual_public_tail_first_and_replacement_require_own_dry_effect_and_return() {
    explicit_public_child(false, false, false);
}

#[test]
fn actual_public_paused_tail_first_and_replacement_require_own_resumed_wet_effect() {
    explicit_public_child(true, true, false);
}

#[test]
fn actual_public_paused_tail_first_and_replacement_require_own_resumed_dry_effect() {
    explicit_public_child(false, true, false);
}

#[test]
fn actual_public_intro_first_and_replacement_require_own_wet_effect_and_return() {
    explicit_public_child(true, false, true);
}

#[test]
fn actual_public_intro_first_and_replacement_require_own_dry_effect_and_return() {
    explicit_public_child(false, false, true);
}

#[test]
fn actual_public_paused_intro_first_and_replacement_require_own_resumed_wet_effect() {
    explicit_public_child(true, true, true);
}

#[test]
fn actual_public_paused_intro_first_and_replacement_require_own_resumed_dry_effect() {
    explicit_public_child(false, true, true);
}

#[test]
fn actual_public_musical_intro_first_and_replacement_require_own_wet_effect_and_return() {
    musical_explicit_public_child(true, true);
}

#[test]
fn actual_public_musical_intro_first_and_replacement_require_own_dry_effect_and_return() {
    musical_explicit_public_child(false, true);
}

#[test]
fn actual_public_musical_tail_first_and_replacement_require_own_wet_effect_and_return() {
    musical_explicit_public_child(true, false);
}

#[test]
fn actual_public_musical_tail_first_and_replacement_require_own_dry_effect_and_return() {
    musical_explicit_public_child(false, false);
}

fn explicit_public_child(key_lock: bool, paused: bool, intro: bool) {
    let (child, name) = if intro {
        match (key_lock, paused) {
            (true, false) => (
                "FLITZI_L3_PUBLIC_INTRO_ON_CHILD",
                "actual_public_intro_first_and_replacement_require_own_wet_effect_and_return",
            ),
            (false, false) => (
                "FLITZI_L3_PUBLIC_INTRO_OFF_CHILD",
                "actual_public_intro_first_and_replacement_require_own_dry_effect_and_return",
            ),
            (true, true) => (
                "FLITZI_L3_PUBLIC_PAUSED_INTRO_ON_CHILD",
                "actual_public_paused_intro_first_and_replacement_require_own_resumed_wet_effect",
            ),
            (false, true) => (
                "FLITZI_L3_PUBLIC_PAUSED_INTRO_OFF_CHILD",
                "actual_public_paused_intro_first_and_replacement_require_own_resumed_dry_effect",
            ),
        }
    } else {
        match (key_lock, paused) {
            (true, false) => (
                "FLITZI_L3_PUBLIC_TAIL_ON_CHILD",
                "actual_public_tail_first_and_replacement_require_own_wet_effect_and_return",
            ),
            (false, false) => (
                "FLITZI_L3_PUBLIC_TAIL_OFF_CHILD",
                "actual_public_tail_first_and_replacement_require_own_dry_effect_and_return",
            ),
            (true, true) => (
                "FLITZI_L3_PUBLIC_PAUSED_TAIL_ON_CHILD",
                "actual_public_paused_tail_first_and_replacement_require_own_resumed_wet_effect",
            ),
            (false, true) => (
                "FLITZI_L3_PUBLIC_PAUSED_TAIL_OFF_CHILD",
                "actual_public_paused_tail_first_and_replacement_require_own_resumed_dry_effect",
            ),
        }
    };
    explicit_public_cases(key_lock, intro, false, &[paused], child, name);
}

fn musical_explicit_public_child(key_lock: bool, intro: bool) {
    let (child, name) = match (key_lock, intro) {
        (true, true) => (
            "FLITZI_L3_PUBLIC_MUSICAL_INTRO_ON_CHILD",
            "actual_public_musical_intro_first_and_replacement_require_own_wet_effect_and_return",
        ),
        (false, true) => (
            "FLITZI_L3_PUBLIC_MUSICAL_INTRO_OFF_CHILD",
            "actual_public_musical_intro_first_and_replacement_require_own_dry_effect_and_return",
        ),
        (true, false) => (
            "FLITZI_L3_PUBLIC_MUSICAL_TAIL_ON_CHILD",
            "actual_public_musical_tail_first_and_replacement_require_own_wet_effect_and_return",
        ),
        (false, false) => (
            "FLITZI_L3_PUBLIC_MUSICAL_TAIL_OFF_CHILD",
            "actual_public_musical_tail_first_and_replacement_require_own_dry_effect_and_return",
        ),
    };
    explicit_public_cases(key_lock, intro, true, &[false, true], child, name);
}

fn explicit_public_cases(
    key_lock: bool,
    intro: bool,
    musical: bool,
    pause_states: &[bool],
    child: &str,
    name: &str,
) {
    if std::env::var_os(child).is_some() {
        for &paused in pause_states {
            for selected_v1 in [false, true] {
                round_trip(
                    selected_v1,
                    None,
                    key_lock,
                    None,
                    false,
                    CandidateTrace::Explicit {
                        intro,
                        paused,
                        musical,
                    },
                );
            }
        }
        return;
    }
    let test = format!(
        "audio_engine::cold_load::tests::stem_pair_publication_tests::committed_stem_residency_tests::l3_controller_productive_tests::{name}"
    );
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(child, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public explicit Seek child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
}
