use super::*;
use crate::audio_engine::audio_stream::{drain_control_messages, drain_parameter_messages};
use crate::audio_engine::committed_stem_residency::SelectedStemIdentity;
use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use crate::audio_engine::pad_copy::{CopyRtGuard, CopyRtIntent, CopySelectionFences};
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::ResidentSourceView;
use crate::messages::{AudioMessage, ControlMessage, ControlParameterMessage, TriggerQuantization};
use pyo3::Python;
use rtrb::{Producer, RingBuffer};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8};

pub(super) const SOURCE: usize = 0;
pub(super) const TARGET: usize = 36;
pub(super) const RATE: u32 = 48_000;

pub(super) struct HoldingRetirement {
    pub(super) slots: usize,
    pub(super) held: Option<Box<PadCopyRtWork>>,
    pub(super) samples: Vec<SampleBuffer>,
    pub(super) stems: Vec<PreparedStemSet>,
    // Preserve the actual prepared boxes transferred by the production retirement
    // interface; unboxing here would destroy their allocation inside the callback.
    #[allow(clippy::vec_box)]
    pub(super) holds: Vec<Box<crate::audio_engine::hold_action::HoldActionRtWork>>,
}

macro_rules! unexpected_retirement {
    ($method:ident, $value:ty) => {
        fn $method(&mut self, _: $value) {
            panic!("copy must retain this owner inside its whole payload");
        }
    };
}

impl AudioBufferRetirement for HoldingRetirement {
    fn retire_pad_copy(&mut self, work: Box<PadCopyRtWork>) {
        assert!(self.held.is_none());
        self.held = Some(work);
    }

    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }

    fn retire_sample(&mut self, sample: SampleBuffer) {
        self.samples.push(sample);
    }
    fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
        self.stems.push(stems);
    }
    fn retire_hold_action(
        &mut self,
        hold: Box<crate::audio_engine::hold_action::HoldActionRtWork>,
    ) {
        self.holds.push(hold);
    }
    unexpected_retirement!(retire_constant_timing, PreparedConstantTiming);
    unexpected_retirement!(retire_cold_adoption, Arc<AtomicU8>);
    unexpected_retirement!(retire_resident_cancellation, Arc<AtomicBool>);
    unexpected_retirement!(
        retire_resident_transaction,
        Box<crate::messages::ResidentTransaction>
    );
    unexpected_retirement!(
        retire_resident_capture,
        Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>
    );
    unexpected_retirement!(
        retire_global_playback_batch,
        Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>
    );
    unexpected_retirement!(
        retire_accepted_timing_refresh,
        Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>
    );
    unexpected_retirement!(
        retire_committed_stem_work,
        Box<crate::audio_engine::committed_stem_residency::CommittedStemRtWork>
    );
}

pub(super) fn fixture() -> (Box<RtMixer>, Box<PadCopyRtWork>, HoldingRetirement) {
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    // Use the exact production boxed startup path. Returning an inline RtMixer
    // through this fixture and occupied_fixture multiplies its fixed arrays in
    // unoptimized caller frames; the production callback owns only one box.
    let mut mixer = crate::audio_engine::audio_stream::create_stream_mixer(
        1,
        RATE,
        Arc::default(),
        ownership.clone(),
        (0..NUM_SAMPLES).map(|_| Arc::default()).collect(),
    )
    .unwrap();
    let source = SampleBuffer {
        channels: 1,
        samples: Arc::from(vec![0.25; 256]),
        residency: None,
    }
    .with_complete_source(RATE);
    ownership.publish_test_source(SOURCE, &source, RATE, 1);
    mixer.load_sample(SOURCE, source.clone());
    let mut target = source.clone();
    let view = source.residency.as_ref().unwrap();
    target.residency = Some(Arc::new(ResidentSourceView {
        source: view.source.clone(),
        start_frame: view.start_frame,
        window_revision: 2,
        context: view.context,
    }));
    let source_binding = InputPadBinding {
        placement: ownership.slot_snapshot(SOURCE),
        resident: source.resident_binding(),
        source_address: source.source_address(),
        sample_count: source.source_sample_count(),
        channels: source.channels,
        sample_rate_hz: RATE,
        authority_revision: ownership.authority_revision(SOURCE),
        runtime_revision: ownership.runtime_revision(SOURCE),
        accepted: None,
    };
    ownership.reserve_copy(TARGET, 17).unwrap();
    let guard = CopyRtGuard {
        source: SOURCE,
        target: TARGET,
        source_binding,
        source_generation: 1,
        source_epoch: mixer.prepared_source_epochs[SOURCE].clone(),
        captured_source_epoch: 0,
        source_selection_fences: None,
        target_epoch: mixer.prepared_source_epochs[TARGET].clone(),
        captured_target_epoch: 0,
        target_authority: ownership.authority_revision(TARGET),
        target_runtime: ownership.runtime_revision(TARGET),
        target_placement: ownership.slot_snapshot(TARGET).unwrap(),
        target_binding: None,
        target_source_generation: 0,
        target_selection_fences: std::array::from_fn(|_| None),
        reservation: 17,
        phase: Arc::new(AtomicU8::new(0)),
        ownership,
        acknowledgements: mixer.current_timing_acknowledgements.clone(),
        source_reservation: 0,
    };
    let content_lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
    let work = Box::new(PadCopyRtWork {
        content_key: guard
            .target_placement
            .for_lifetime(content_lifetime)
            .unwrap()
            .content_key()
            .unwrap(),
        target: TARGET,
        sample: target,
        stems: None,
        selection: None,
        selection_fences: None,
        projection: None,
        source_generation: 2,
        content_lifetime,
        prepared_epoch: 1,
        authority_revision: 2,
        publication: PreparedSourcePermit::new(guard.target_epoch.clone(), 1),
        guard,
        intent: CopyRtIntent {
            pitch_intent: crate::audio_engine::pitch_intent::PitchIntent::default(),
            period_seconds: Some(0.5),
            origin_frame: -3.25,
            loop_start_frame: 8,
            loop_end_frame: Some(128),
            key_lock: false,
            timing_intent: TimingIntent::Tap,
            stem_mix_mode: StemMixMode::FullMix,
            source_version_hash: 0,
            enabled_mask: 0,
            gain_db: -3.0,
            eq_low_db: -6.0,
            eq_mid_db: 1.0,
            eq_high_db: 0.0,
        },
        retired_receipt: None,
        retired_sample: None,
        retired_stems: None,
        retired_outgoing: None,
    });
    (
        mixer,
        work,
        HoldingRetirement {
            slots: 1,
            held: None,
            samples: Vec::new(),
            stems: Vec::new(),
            holds: Vec::new(),
        },
    )
}

fn prepared_set(sample: &SampleBuffer, publication: PreparedSourcePermit) -> PreparedStemSet {
    PreparedStemSet {
        complete_set_identity: Arc::new([7; 32]),
        accepted_timing: None,
        reference_samples: sample.samples.clone(),
        reference_residency: sample.residency.clone(),
        publication,
        source_version_hash: 47,
        sample_rate_hz: RATE,
        channels: sample.channels,
        frame_count: sample.frame_count(),
        available_mask: STEM_COMPONENT_MASK,
        stems: std::array::from_fn(|_| sample.clone()),
    }
}

#[test]
fn stopped_complete_copy_keeps_running_source_and_owns_fresh_target_image() {
    let (mut mixer, work, mut retirement) = fixture();
    assert!(mixer.play_sample(SOURCE, 1.0));
    let source_voice = mixer.voices.iter().position(|voice| voice.active).unwrap();
    let position = mixer.voices[source_voice].source_playback.position();
    let generation = mixer.voices[source_voice].generation;
    let source_authority = mixer.input_runtime_ownership.authority_revision(SOURCE);
    let source_sample = mixer.sample_bank[SOURCE].as_ref().unwrap().clone();
    let phase = work.guard.phase.clone();

    assert!(mixer.copy_pad_rt(work, &mut retirement));
    assert_eq!(phase.load(Ordering::Acquire), 2);
    assert!(retirement.held.is_some());
    assert_eq!(
        mixer.voices[source_voice].source_playback.position(),
        position
    );
    assert_eq!(mixer.voices[source_voice].generation, generation);
    assert!(mixer.voices[source_voice].active);
    assert!(
        !mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == TARGET)
    );
    assert_eq!(
        mixer.input_runtime_ownership.authority_revision(SOURCE),
        source_authority
    );
    let target = mixer.sample_bank[TARGET].as_ref().unwrap();
    assert!(same_copy_window(&source_sample, target));
    assert!(!source_sample.same_window(target));
    assert_eq!(target.window_revision(), 2);
    assert_eq!(
        mixer
            .input_runtime_ownership
            .source_generation(TARGET, target, RATE),
        Some(2)
    );
    assert_eq!(
        mixer.prepared_source_epochs[TARGET].load(Ordering::Acquire),
        1
    );
    assert_eq!(mixer.pad_loop_start_frame[TARGET], 8);
    assert_eq!(mixer.pad_loop_end_frame[TARGET], Some(128));
    assert_eq!(mixer.pad_period_seconds[TARGET], Some(0.5));
    assert_eq!(mixer.pad_phase_anchor_frame[TARGET], -3.25);
    assert_eq!(mixer.pad_gain_db[TARGET], -3.0);
    assert_eq!(mixer.pad_gain_smoothers[TARGET].frames_remaining, 0);
    assert_eq!(
        mixer.pad_gain_smoothers[TARGET].current(),
        gain_db_to_linear(-3.0)
    );
    assert_eq!(
        mixer.pad_dsp_chains[TARGET].loop_acceptance_parameters(),
        (
            [
                pad_eq_db_to_normalized(-6.0),
                pad_eq_db_to_normalized(1.0),
                0.5
            ],
            [
                pad_eq_db_to_normalized(-6.0),
                pad_eq_db_to_normalized(1.0),
                0.5
            ]
        )
    );
    assert_eq!(mixer.pad_playhead_frame[TARGET], None);
    assert_eq!(mixer.stem_enabled_mask[TARGET], 0);
    assert_eq!(mixer.stem_mix_mode[TARGET], StemMixMode::FullMix);
    assert_eq!(
        mixer
            .current_timing_acknowledgements
            .current_for_slot(&mixer.input_runtime_ownership, TARGET),
        0
    );
    assert_eq!(mixer.input_runtime_ownership.copy_reservation(TARGET), 17);
}

#[test]
fn all_stems_copy_accepts_mask_zero_and_retires_former_empty_slot_readers_together() {
    let (mut mixer, mut work, mut retirement) = fixture();
    let source = mixer.sample_bank[SOURCE].as_ref().unwrap();
    let source_stems = prepared_set(source, PreparedSourcePermit::accepted_fixed());
    mixer.prepared_stems[SOURCE] = Some(source_stems.clone());
    let mut target_stems = source_stems.clone();
    target_stems.reference_residency = work.sample.residency.clone();
    for stem in &mut target_stems.stems {
        stem.residency = work.sample.residency.clone();
    }
    target_stems.publication = work.publication.clone();
    let permit = target_stems.publication.clone();
    mixer.committed_stem_selection[SOURCE] = Some(SelectedStemIdentity {
        identity: [7; 32],
        source_generation: 1,
        epoch: 3,
    });
    work.selection = Some(SelectedStemIdentity {
        identity: [7; 32],
        source_generation: work.source_generation,
        epoch: 5,
    });
    let selection_epoch = Arc::new(AtomicU64::new(2));
    let registration_epoch = Arc::new(AtomicU64::new(4));
    work.selection_fences = Some(CopySelectionFences {
        selection: selection_epoch.clone(),
        registration: registration_epoch.clone(),
        expected: 5,
        publication: PreparedSourcePermit::new(registration_epoch.clone(), 5),
    });
    work.stems = Some(target_stems);
    work.intent.stem_mix_mode = StemMixMode::AllStems;
    work.intent.source_version_hash = 47;
    mixer.prepared_stems[TARGET] = Some(source_stems.clone());
    mixer.outgoing_stems[TARGET] = Some(source_stems);

    assert!(mixer.copy_pad_rt(work, &mut retirement));
    assert_eq!(permit.status(), "accepted");
    assert!(permit.current());
    assert_eq!(selection_epoch.load(Ordering::Acquire), 5);
    assert_eq!(registration_epoch.load(Ordering::Acquire), 5);
    assert_eq!(
        mixer.committed_stem_selection[TARGET]
            .unwrap()
            .source_generation,
        2
    );
    assert_eq!(mixer.committed_stem_selection[TARGET].unwrap().epoch, 5);
    assert_eq!(mixer.stem_mix_mode[TARGET], StemMixMode::AllStems);
    assert_eq!(mixer.stem_enabled_mask[TARGET], 0);
    assert!(mixer.outgoing_stems[TARGET].is_none());
    let retired = retirement.held.as_ref().unwrap();
    assert!(retired.retired_stems.is_some());
    assert!(retired.retired_outgoing.is_some());
    let sample = mixer.sample_bank[TARGET].as_ref().unwrap();
    let stems = mixer.prepared_stems[TARGET].as_ref().unwrap();
    assert_eq!(render_source_sample(sample, Some(stems), 0, 8, 1, 0), 0.0);
    assert!(Arc::ptr_eq(
        &mixer.prepared_stems[SOURCE].as_ref().unwrap().stems[0].samples,
        &stems.stems[0].samples
    ));
}

#[test]
fn automatic_copy_acknowledges_only_the_fresh_target_publication() {
    let (mut mixer, mut work, mut retirement) = fixture();
    let source = AcceptedTimingProjection {
        revision: [3; 32],
        period_seconds: 0.5,
        origin_seconds: -0.125,
        sample_rate_hz: RATE,
        publication_epoch: 7,
    };
    mixer.pad_accepted_timing[SOURCE] = Some(source);
    assert!(
        mixer.current_timing_acknowledgements.acknowledge_for_key(
            &mixer.input_runtime_ownership,
            work.guard
                .source_binding
                .placement
                .unwrap()
                .content_key()
                .unwrap(),
            7
        )
    );
    work.guard.source_binding.accepted = Some(source);
    work.projection = Some(AcceptedTimingProjection {
        publication_epoch: 11,
        ..source
    });
    work.intent.timing_intent = TimingIntent::Automatic;
    assert!(mixer.copy_pad_rt(work, &mut retirement));
    assert_eq!(
        mixer
            .current_timing_acknowledgements
            .current_for_slot(&mixer.input_runtime_ownership, SOURCE),
        7
    );
    assert_eq!(
        mixer
            .current_timing_acknowledgements
            .current_for_slot(&mixer.input_runtime_ownership, TARGET),
        11
    );
    assert_eq!(
        mixer.pad_accepted_timing[TARGET].unwrap().publication_epoch,
        11
    );
}

#[test]
fn missing_retirement_capacity_retains_exact_copy_before_any_image_change() {
    let (mut mixer, work, mut retirement) = fixture();
    retirement.slots = 0;
    let phase = work.guard.phase.clone();
    assert!(!mixer.copy_pad_rt(work, &mut retirement));
    assert_eq!(phase.load(Ordering::Acquire), 0);
    assert!(retirement.held.is_none());
    assert!(mixer.sample_bank[TARGET].is_none());
    assert_eq!(mixer.input_runtime_ownership.authority_revision(TARGET), 1);
    assert_eq!(
        mixer.prepared_source_epochs[TARGET].load(Ordering::Acquire),
        0
    );
    assert_eq!(mixer.pad_loop_start_frame[TARGET], 0);
    assert_eq!(mixer.pad_gain_db[TARGET], PAD_GAIN_DB_DEFAULT);
    let Some(ControlMessage::PadCopy(work)) = mixer.take_deferred_control() else {
        panic!("capacity must retain the exact Copy payload");
    };
    assert!(Arc::ptr_eq(&phase, &work.guard.phase));
    retirement.slots = 1;
    assert!(mixer.copy_pad_rt(work, &mut retirement));
    assert_eq!(phase.load(Ordering::Acquire), 2);
}

#[test]
fn changed_source_epoch_and_paused_target_reject_complete_work_without_restart() {
    for stale_source in [false, true] {
        let (mut mixer, work, mut retirement) = fixture();
        if stale_source {
            mixer.prepared_source_epochs[SOURCE].store(1, Ordering::Release);
        } else {
            mixer.voices[0].sample_id = TARGET;
            mixer.voices[0].paused = true;
        }
        let phase = work.guard.phase.clone();
        assert!(!mixer.copy_pad_rt(work, &mut retirement));
        assert_eq!(phase.load(Ordering::Acquire), 3);
        assert!(mixer.sample_bank[TARGET].is_none());
        assert!(mixer.sample_bank[SOURCE].is_some());
        assert_eq!(mixer.pad_loop_start_frame[TARGET], 0);
        assert!(retirement.held.is_some());
    }
}

#[test]
fn malformed_copy_geometry_backing_and_historical_timing_never_install_a_partial_image() {
    for invalid in 0..4 {
        let (mut mixer, mut work, mut retirement) = fixture();
        match invalid {
            0 => work.intent.enabled_mask = 16,
            1 => work.intent.loop_start_frame = 256,
            2 => work.sample.samples = Arc::from(vec![0.25; 256]),
            _ => {
                work.intent.timing_intent = TimingIntent::Automatic;
                work.projection = Some(AcceptedTimingProjection {
                    revision: [3; 32],
                    period_seconds: 0.5,
                    origin_seconds: 0.0,
                    sample_rate_hz: RATE,
                    publication_epoch: 9,
                });
            }
        }
        let phase = work.guard.phase.clone();
        assert!(!mixer.copy_pad_rt(work, &mut retirement));
        assert_eq!(phase.load(Ordering::Acquire), 3);
        assert!(mixer.sample_bank[TARGET].is_none());
        assert_eq!(
            mixer
                .current_timing_acknowledgements
                .current_for_slot(&mixer.input_runtime_ownership, TARGET),
            0
        );
        assert_eq!(mixer.input_runtime_ownership.authority_revision(TARGET), 1);
        assert!(retirement.held.is_some());
    }
}

fn parameter_engine(
    mixer: &RtMixer,
    parameter_producer: Producer<ControlParameterMessage>,
    control_producer: Arc<Mutex<Producer<ControlMessage>>>,
) -> crate::audio_engine::AudioEngine {
    let mut engine = crate::audio_engine::AudioEngine::new().unwrap();
    engine.input_runtime_ownership = mixer.input_runtime_ownership.clone();
    engine.prepared_source_epochs = mixer.prepared_source_epochs.clone();
    engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameter_producer)));
    let (_, feedback) = RingBuffer::<AudioMessage>::new(8);
    engine.test_control_channels = Some((control_producer, Arc::new(Mutex::new(feedback))));
    engine
}

#[test]
fn real_control_copy_then_parameter_drain_rejects_prior_gain_eq_and_empty_target_bpm() {
    Python::initialize();
    Python::attach(|_| {
        let (mut mixer, mut work, mut retirement) = fixture();
        let (controls, mut control_consumer) = RingBuffer::<ControlMessage>::new(8);
        let controls = Arc::new(Mutex::new(controls));
        let (parameters, mut parameter_consumer) = RingBuffer::<ControlParameterMessage>::new(8);
        let mut engine = parameter_engine(&mixer, parameters, controls.clone());
        engine
            .input_runtime_ownership
            .release_copy(TARGET, 17)
            .unwrap();
        engine.set_pad_gain(TARGET, -18.0).unwrap();
        engine.set_pad_eq(TARGET, -10.0, -5.0, 2.0).unwrap();
        engine.set_pad_bpm(TARGET, Some(90.0)).unwrap();

        // Capture after those actual predecessor admissions, exactly as Copy preparation does.
        work.guard.captured_target_epoch =
            engine.prepared_source_epochs[TARGET].load(Ordering::Acquire);
        work.prepared_epoch = work.guard.captured_target_epoch + 1;
        work.guard.target_authority = engine.input_runtime_ownership.authority_revision(TARGET);
        work.authority_revision = work.guard.target_authority + 1;
        work.publication =
            PreparedSourcePermit::new(work.guard.target_epoch.clone(), work.prepared_epoch);
        engine
            .input_runtime_ownership
            .reserve_copy(TARGET, 17)
            .unwrap();
        let phase = work.guard.phase.clone();
        let publication = work.publication.clone();
        controls
            .lock()
            .unwrap()
            .push(ControlMessage::PadCopy(work))
            .unwrap();
        let mut transport = TransportTimeline::new(RATE);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut quantization = TriggerQuantization::Immediate;
        let (mut feedback, _) = RingBuffer::<AudioMessage>::new(8);
        assert_eq!(
            drain_control_messages(
                &mut control_consumer,
                &mut scheduler,
                0,
                &mut quantization,
                &mut transport,
                &mut mixer,
                &mut feedback,
                &mut retirement,
            ),
            1
        );
        assert_eq!(phase.load(Ordering::Acquire), 2);
        assert_eq!(publication.status(), "accepted");
        engine
            .input_runtime_ownership
            .release_copy(TARGET, 17)
            .unwrap();

        drain_parameter_messages(&mut parameter_consumer, &mut mixer, &mut transport);
        assert_eq!(mixer.pad_gain_db[TARGET], -3.0);
        assert_eq!(mixer.pad_period_seconds[TARGET], Some(0.5));
        let (eq, _) = mixer.pad_dsp_chains[TARGET].loop_acceptance_parameters();
        assert_eq!(
            eq,
            [
                pad_eq_db_to_normalized(-6.0),
                pad_eq_db_to_normalized(1.0),
                0.5
            ]
        );
        assert!(retirement.held.is_some());
    });
}

#[test]
fn later_actual_gain_eq_admissions_survive_stale_entries_before_coalescing() {
    Python::initialize();
    Python::attach(|_| {
        let (mut mixer, work, mut retirement) = fixture();
        let (controls, _) = RingBuffer::<ControlMessage>::new(8);
        let controls = Arc::new(Mutex::new(controls));
        let (parameters, mut parameter_consumer) = RingBuffer::<ControlParameterMessage>::new(8);
        let mut engine = parameter_engine(&mixer, parameters, controls);
        assert!(mixer.copy_pad_rt(work, &mut retirement));
        assert_eq!(
            retirement
                .held
                .as_ref()
                .unwrap()
                .guard
                .phase
                .load(Ordering::Acquire),
            2
        );
        engine
            .input_runtime_ownership
            .release_copy(TARGET, 17)
            .unwrap();
        engine.set_pad_gain(TARGET, -9.0).unwrap();
        engine.set_pad_eq(TARGET, -3.0, 0.0, 2.0).unwrap();
        // A delayed predecessor after valid pending edits may not replace their coalesced values.
        let mut producer = engine
            .test_parameter_producer
            .as_ref()
            .unwrap()
            .lock()
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadGainGuarded {
                placement: engine
                    .input_runtime_ownership
                    .slot_snapshot(TARGET)
                    .unwrap(),
                id: TARGET,
                gain_db: -18.0,
                expected_epoch: 0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadEqGuarded {
                placement: engine
                    .input_runtime_ownership
                    .slot_snapshot(TARGET)
                    .unwrap(),
                id: TARGET,
                low_db: -10.0,
                mid_db: -5.0,
                high_db: 0.0,
                expected_epoch: 0,
            })
            .unwrap();
        drop(producer);
        let mut transport = TransportTimeline::new(RATE);
        drain_parameter_messages(&mut parameter_consumer, &mut mixer, &mut transport);
        assert_eq!(mixer.pad_gain_db[TARGET], -9.0);
        let (_, target) = mixer.pad_dsp_chains[TARGET].loop_acceptance_parameters();
        assert_eq!(
            target,
            [
                pad_eq_db_to_normalized(-3.0),
                0.5,
                pad_eq_db_to_normalized(2.0)
            ]
        );
    });
}

#[test]
fn ordinary_timing_preparation_does_not_reject_preceding_actual_gain_eq_edits() {
    Python::initialize();
    Python::attach(|_| {
        let (mut mixer, work, _) = fixture();
        let (controls, _) = RingBuffer::<ControlMessage>::new(8);
        let (parameters, mut parameter_consumer) = RingBuffer::<ControlParameterMessage>::new(8);
        let mut engine = parameter_engine(&mixer, parameters, Arc::new(Mutex::new(controls)));
        engine
            .input_runtime_ownership
            .release_copy(TARGET, 17)
            .unwrap();
        engine.set_pad_gain(TARGET, -9.0).unwrap();
        engine.set_pad_eq(TARGET, -3.0, 0.0, 2.0).unwrap();
        engine.set_pad_bpm(TARGET, Some(120.0)).unwrap();
        assert_eq!(
            engine.prepared_source_epochs[TARGET].load(Ordering::Acquire),
            1
        );
        assert_eq!(
            engine.input_runtime_ownership.copy_parameter_epoch(TARGET),
            0
        );
        let mut transport = TransportTimeline::new(RATE);
        drain_parameter_messages(&mut parameter_consumer, &mut mixer, &mut transport);
        assert_eq!(mixer.pad_gain_db[TARGET], -9.0);
        assert_eq!(mixer.pad_period_seconds[TARGET], Some(0.5));
        let (_, target) = mixer.pad_dsp_chains[TARGET].loop_acceptance_parameters();
        assert_eq!(
            target,
            [
                pad_eq_db_to_normalized(-3.0),
                0.5,
                pad_eq_db_to_normalized(2.0)
            ]
        );
        assert_eq!(work.guard.phase.load(Ordering::Acquire), 0);
    });
}

#[test]
fn pending_copy_rejects_direct_admissions_without_queue_or_epoch_changes() {
    Python::initialize();
    Python::attach(|_| {
        let (mixer, work, _) = fixture();
        let (controls, mut control_consumer) = RingBuffer::<ControlMessage>::new(8);
        let controls = Arc::new(Mutex::new(controls));
        let (parameters, mut parameter_consumer) = RingBuffer::<ControlParameterMessage>::new(8);
        let mut engine = parameter_engine(&mixer, parameters, controls);
        let requests = engine.pad_request_ids.lock().unwrap().clone();
        let authority = engine.input_runtime_ownership.authority_revision(TARGET);
        let prepared = engine.prepared_source_epochs[TARGET].load(Ordering::Acquire);
        assert!(
            engine
                .set_pad_gain(TARGET, -9.0)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .set_pad_eq(TARGET, -3.0, 0.0, 2.0)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .pause_sample(TARGET)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .resume_sample(TARGET)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .set_stem_mix_mode(TARGET, "full_mix", None)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .set_stem_enabled_mask(TARGET, 5, "owned-source".into())
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .set_key_lock(false)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .unload_sample(TARGET)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(
            engine
                .load_incoming_sample_async(TARGET, "copy-pending-source.wav".into(), false)
                .unwrap_err()
                .to_string()
                .contains("copy")
        );
        assert!(parameter_consumer.pop().is_err());
        assert!(control_consumer.pop().is_err());
        assert_eq!(*engine.pad_request_ids.lock().unwrap(), requests);
        assert_eq!(
            engine.input_runtime_ownership.authority_revision(TARGET),
            authority
        );
        assert_eq!(
            engine.prepared_source_epochs[TARGET].load(Ordering::Acquire),
            prepared
        );
        assert_eq!(
            engine.input_runtime_ownership.copy_parameter_epoch(TARGET),
            0
        );
        assert_eq!(work.guard.phase.load(Ordering::Acquire), 0);
    });
}
