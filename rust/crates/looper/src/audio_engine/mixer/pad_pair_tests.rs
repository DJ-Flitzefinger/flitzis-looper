use super::super::*;
use crate::audio_engine::committed_stem_residency::SelectedStemIdentity;
use crate::audio_engine::content_placement::PlacementObservation;
use crate::audio_engine::input_runtime_binding::{InputPadBinding, InputRuntimeOwnership};
use crate::audio_engine::pad_pair::{PadPairRtWork, PairParticipantGuard, PairRtGuard};
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use crate::audio_engine::source_playback::FractionalSourcePosition;
use crate::messages::{STEM_MASK_DRUMS, STEM_MASK_MELODY, STEM_MASK_VOCALS};
use std::sync::atomic::Ordering;

fn guarded_pair_work(mixer: &RtMixer) -> Box<PadPairRtWork> {
    let ownership = mixer.input_runtime_ownership.clone();
    let participants = [SOURCE, TARGET].map(|slot| {
        let snapshot = ownership.slot_snapshot(slot).unwrap();
        let binding = mixer.sample_bank[slot]
            .as_ref()
            .map(|sample| InputPadBinding {
                placement: Some(snapshot),
                resident: sample.resident_binding(),
                source_address: sample.source_address(),
                sample_count: sample.source_sample_count(),
                channels: sample.channels,
                sample_rate_hz: RATE,
                authority_revision: ownership.authority_revision(slot),
                runtime_revision: ownership.runtime_revision(slot),
                accepted: mixer.pad_accepted_timing[slot],
            });
        PairParticipantGuard::capture(
            &ownership,
            &mixer.current_timing_acknowledgements,
            snapshot,
            binding,
            mixer.prepared_source_epochs[snapshot.cell().index()].clone(),
            (1..=3)
                .map(|epoch| (Arc::new(AtomicU64::new(epoch)), epoch))
                .collect(),
        )
        .unwrap()
    });
    let snapshots = participants
        .each_ref()
        .map(|participant| participant.placement());
    let reservation = crate::audio_engine::content_lifetime::reserve().unwrap();
    let guard = PairRtGuard::new(
        participants,
        ownership.clone(),
        mixer.current_timing_acknowledgements.clone(),
        reservation,
    )
    .unwrap();
    ownership.reserve_pair(&snapshots, reservation).unwrap();
    PadPairRtWork::new(guard, RATE, 1).unwrap()
}

struct RetainingPairRetirement {
    pairs: Vec<Box<PadPairRtWork>>,
    available: usize,
}

fn assert_pair_history_retained(
    actual: Option<ProductiveSourceHistory>,
    expected: Option<ProductiveSourceHistory>,
) {
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            assert_eq!(actual.binding, expected.binding);
            assert_eq!(actual.next_position, expected.next_position);
            assert_eq!(actual.domain, expected.domain);
            assert_eq!(actual.fed_output_frames, expected.fed_output_frames);
        }
        (None, None) => {}
        _ => panic!("Pair replaced actual source history"),
    }
}

impl RetainingPairRetirement {
    fn new(available: usize) -> Self {
        Self {
            pairs: Vec::with_capacity(1),
            available,
        }
    }
}

impl AudioBufferRetirement for RetainingPairRetirement {
    fn retire_pad_pair(&mut self, work: Box<PadPairRtWork>) {
        self.pairs.push(work);
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<super::super::super::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_sample(&mut self, _: SampleBuffer) {}
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {}
    fn retire_constant_timing(&mut self, _: PreparedConstantTiming) {}
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<super::super::super::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<super::super::super::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.available
    }
}

const SOURCE: usize = 3;
const TARGET: usize = 178;
const RATE: u32 = 8_000;

fn sample(amplitude: f32, frequency: f32) -> SampleBuffer {
    SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from(
            (0..RATE * 4)
                .map(|frame| amplitude * (frame as f32 * frequency).sin())
                .collect::<Vec<_>>(),
        ),
    }
}

fn stems(reference: &SampleBuffer, identity: u8) -> PreparedStemSet {
    PreparedStemSet {
        complete_set_identity: Arc::new([identity; 32]),
        accepted_timing: None,
        reference_samples: reference.samples.clone(),
        reference_residency: reference.residency.clone(),
        publication: PreparedSourcePermit::accepted_fixed(),
        source_version_hash: u64::from(identity),
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: reference.frame_count(),
        available_mask: STEM_COMPONENT_MASK,
        stems: std::array::from_fn(|index| SampleBuffer {
            samples: Arc::from(
                reference
                    .samples
                    .iter()
                    .map(|value| value * (index + 1) as f32 / 10.0)
                    .collect::<Vec<_>>(),
            ),
            ..reference.clone()
        }),
    }
}

fn source_stems(
    mixer: &RtMixer,
    id: usize,
    reference: &SampleBuffer,
    identity: u8,
) -> PreparedStemSet {
    let ownership = &mixer.input_runtime_ownership;
    let snapshot = ownership.slot_snapshot(id).unwrap();
    let binding = InputPadBinding {
        placement: Some(snapshot),
        resident: reference.resident_binding(),
        source_address: reference.source_address(),
        sample_count: reference.source_sample_count(),
        channels: reference.channels,
        sample_rate_hz: RATE,
        authority_revision: ownership.authority_revision(id),
        runtime_revision: ownership.runtime_revision(id),
        accepted: mixer.pad_accepted_timing[id],
    };
    let mut prepared = stems(reference, identity);
    prepared.accepted_timing = binding.accepted;
    prepared.publication = PreparedSourcePermit::for_native_binding_for_test(
        id,
        binding,
        ownership.clone(),
        mixer.current_timing_acknowledgements.clone(),
        mixer.prepared_source_epochs[snapshot.cell().index()].clone(),
    )
    .unwrap();
    prepared
}

fn fixture(wet: bool) -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    mixer.set_input_runtime_ownership(ownership.clone());
    for (id, amplitude, frequency, identity, generation) in
        [(SOURCE, 0.6, 0.13, 17, 11), (TARGET, 0.3, 0.07, 29, 23)]
    {
        let source = sample(amplitude, frequency);
        ownership.publish_test_source(id, &source, RATE, generation);
        mixer.load_sample(id, source.clone());
        assert!(mixer.publish_prepared_stems_rt(
            id,
            source_stems(&mixer, id, &source, identity),
            &mut ImmediateAudioBufferRetirement,
        ));
        assert!(mixer.set_stem_mix_mode(id, StemMixMode::AllStems, u64::from(identity)));
        mixer.set_pad_key_lock(id, wet);
    }
    mixer.set_pad_bpm(SOURCE, Some(119.75));
    mixer.set_pad_bpm(TARGET, Some(83.25));
    mixer.set_pad_loop_region(SOURCE, 0.031_25, Some(1.5));
    mixer.set_pad_loop_region(TARGET, 0.125, Some(2.125));
    mixer.set_speed(1.371_234_567_890_123);
    mixer.set_pad_gain(SOURCE, -2.0);
    mixer.set_pad_gain(TARGET, -7.0);
    mixer.set_pad_eq(SOURCE, -8.0, 2.0, -1.0);
    mixer.set_pad_eq(TARGET, 1.0, -3.0, -9.0);
    assert!(mixer.play_sample(SOURCE, 0.9));
    assert!(mixer.play_sample(TARGET, 0.7));
    mixer
}

fn render(mixer: &mut RtMixer, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames];
    mixer.render(&mut output, &mut [0.0; NUM_SAMPLES]);
    output
}

fn lane(mixer: &RtMixer, id: usize) -> usize {
    mixer
        .voices
        .iter()
        .position(|voice| voice.active && voice.sample_id == id)
        .unwrap()
}

fn swap(mixer: &mut RtMixer) {
    let ownership = mixer.input_runtime_ownership.clone();
    let prepared = ownership
        .prepare_placement_pair(
            ownership.slot_snapshot(SOURCE).unwrap(),
            ownership.slot_snapshot(TARGET).unwrap(),
        )
        .unwrap();
    ownership
        .commit_placement_pair(
            &prepared,
            true,
            || true,
            || mixer.swap_occupied_pad_images_rt(SOURCE, TARGET),
        )
        .unwrap();
}

struct LaneState {
    index: usize,
    generation: u64,
    history_lifetime: Arc<AtomicU64>,
    history_generation: u64,
    source_epoch: Arc<AtomicU64>,
    source_epoch_value: u64,
    native: usize,
    fifo: (usize, usize),
    position: FractionalSourcePosition,
    sample: SampleBuffer,
}

impl LaneState {
    fn capture(mixer: &RtMixer, id: usize) -> Self {
        let index = lane(mixer, id);
        let voice = &mixer.voices[index];
        Self {
            index,
            generation: voice.generation,
            history_lifetime: voice.history_lifetime.clone(),
            history_generation: voice.history_lifetime.load(Ordering::Acquire),
            source_epoch: voice.stretch.source_epoch_owner().clone(),
            source_epoch_value: voice.stretch.source_epoch_owner().load(Ordering::Acquire),
            native: voice.stretch.native_state_address(),
            fifo: voice.stretch.pending_fifo_frames(),
            position: voice.source_playback.position(),
            sample: voice.sample.as_ref().unwrap().clone(),
        }
    }

    fn assert_retained(&self, mixer: &RtMixer, id: usize) {
        let voice = &mixer.voices[self.index];
        assert!(voice.active);
        assert_eq!(voice.sample_id, id);
        assert_eq!(voice.generation, self.generation);
        assert!(Arc::ptr_eq(&voice.history_lifetime, &self.history_lifetime));
        assert_eq!(
            voice.history_lifetime.load(Ordering::Acquire),
            self.history_generation
        );
        assert!(Arc::ptr_eq(
            voice.stretch.source_epoch_owner(),
            &self.source_epoch
        ));
        assert_eq!(
            voice.stretch.source_epoch_owner().load(Ordering::Acquire),
            self.source_epoch_value
        );
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.stretch.pending_fifo_frames(), self.fifo);
        assert_eq!(voice.source_playback.position(), self.position);
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &self.sample.samples
        ));
    }
}

fn assert_continuation(wet: bool) {
    let mut control = fixture(wet);
    let mut moved = fixture(wet);
    assert_eq!(render(&mut moved, 1657), render(&mut control, 1657));
    for mixer in [&mut control, &mut moved] {
        // Retain a partially consumed gain/filter ramp and an unfinished source-selection ramp.
        mixer.set_pad_gain(SOURCE, -13.0);
        mixer.set_pad_eq(TARGET, -17.0, 5.0, -2.0);
        assert!(mixer.set_stem_enabled_mask(SOURCE, STEM_MASK_DRUMS, 17));
        assert!(mixer.set_stem_enabled_mask(TARGET, STEM_MASK_MELODY | STEM_MASK_VOCALS, 29));
    }
    assert_eq!(render(&mut moved, 13), render(&mut control, 13));
    assert!(moved.stem_transitions[SOURCE].is_active());
    assert!(moved.stem_transitions[TARGET].is_active());
    let source = LaneState::capture(&moved, SOURCE);
    let target = LaneState::capture(&moved, TARGET);
    let source_filter = moved.pad_dsp_chains[SOURCE].source_history().unwrap();
    let target_filter = moved.pad_dsp_chains[TARGET].source_history().unwrap();
    let source_gain = moved.pad_gain_smoothers[SOURCE].current();
    let target_gain = moved.pad_gain_smoothers[TARGET].current();
    let source_transition = moved.stem_transitions[SOURCE];
    let target_transition = moved.stem_transitions[TARGET];
    let source_content = moved
        .input_runtime_ownership
        .slot_snapshot(SOURCE)
        .unwrap()
        .content_key();
    let target_content = moved
        .input_runtime_ownership
        .slot_snapshot(TARGET)
        .unwrap()
        .content_key();
    let epochs = moved.prepared_source_epochs.clone();
    if wet {
        assert_ne!(source.native, 0);
        assert_ne!(target.native, 0);
        for index in [source.index, target.index] {
            assert!(moved.voices[index].stretch.wet_render_ready());
            assert!(moved.voices[index].stretch.productive_history().is_some());
        }
        assert!(source.fifo.0 + source.fifo.1 > 0);
        assert!(target.fifo.0 + target.fifo.1 > 0);
    }

    swap(&mut moved);

    source.assert_retained(&moved, TARGET);
    target.assert_retained(&moved, SOURCE);
    assert_eq!(moved.pad_gain_smoothers[TARGET].current(), source_gain);
    assert_eq!(moved.pad_gain_smoothers[SOURCE].current(), target_gain);
    assert!(moved.stem_transitions[TARGET].matches_exact(source_transition));
    assert!(moved.stem_transitions[SOURCE].matches_exact(target_transition));
    let filter = moved.pad_dsp_chains[TARGET].source_history().unwrap();
    assert_eq!(filter.binding, source_filter.binding);
    assert_eq!(filter.next_position, source_filter.next_position);
    assert_eq!(filter.fed_output_frames, source_filter.fed_output_frames);
    let filter = moved.pad_dsp_chains[SOURCE].source_history().unwrap();
    assert_eq!(filter.binding, target_filter.binding);
    assert_eq!(filter.next_position, target_filter.next_position);
    assert_eq!(filter.fed_output_frames, target_filter.fed_output_frames);
    assert_eq!(
        moved
            .input_runtime_ownership
            .slot_snapshot(TARGET)
            .unwrap()
            .content_key(),
        source_content
    );
    assert_eq!(
        moved
            .input_runtime_ownership
            .slot_snapshot(SOURCE)
            .unwrap()
            .content_key(),
        target_content
    );
    for (before, after) in epochs.iter().zip(&moved.prepared_source_epochs) {
        assert!(Arc::ptr_eq(before, after));
    }
    for frames in [1, 37, 257, 1024, 19] {
        assert_eq!(render(&mut moved, frames), render(&mut control, frames));
    }
    // The whole moved chain accepts its new slot's EQ parameter addresses without a reset.
    assert!(moved.set_pad_dsp_parameter(TARGET, DspParameterSlot::Slot0, 0.2));
    assert!(control.set_pad_dsp_parameter(SOURCE, DspParameterSlot::Slot0, 0.2));
    assert!(moved.set_pad_dsp_parameter(SOURCE, DspParameterSlot::Slot2, 0.8));
    assert!(control.set_pad_dsp_parameter(TARGET, DspParameterSlot::Slot2, 0.8));
    let actual = render(&mut moved, 257);
    assert_eq!(actual, render(&mut control, 257));
    assert!(actual.iter().any(|value| value.abs() > 1.0e-5));
}

#[test]
fn occupied_swap_preserves_dry_audio_gain_filter_and_stem_ramps() {
    assert_continuation(false);
}

#[test]
fn guarded_swap_own_ack_preserves_actual_dry_and_wet_owners_and_audio() {
    for wet in [false, true] {
        let mut control = fixture(wet);
        let mut moved = fixture(wet);
        assert_eq!(render(&mut moved, 1657), render(&mut control, 1657));
        let source = LaneState::capture(&moved, SOURCE);
        let target = LaneState::capture(&moved, TARGET);
        let source_filter = moved.pad_dsp_chains[SOURCE].source_history();
        let target_filter = moved.pad_dsp_chains[TARGET].source_history();
        let source_gain = moved.pad_gain_smoothers[SOURCE];
        let target_gain = moved.pad_gain_smoothers[TARGET];
        let work = guarded_pair_work(&moved);
        let guard = work.guard.clone();
        let before = guard
            .participants
            .each_ref()
            .map(|participant| participant.placement());
        let mut retirement = RetainingPairRetirement::new(1);
        assert!(moved.pair_pad_rt(work, &mut retirement));
        source.assert_retained(&moved, TARGET);
        target.assert_retained(&moved, SOURCE);
        assert_eq!(moved.pad_gain_smoothers[TARGET], source_gain);
        assert_eq!(moved.pad_gain_smoothers[SOURCE], target_gain);
        assert_pair_history_retained(moved.pad_dsp_chains[TARGET].source_history(), source_filter);
        assert_pair_history_retained(moved.pad_dsp_chains[SOURCE].source_history(), target_filter);
        let receipt = guard.native_acknowledgement().unwrap();
        assert_eq!(
            receipt.kind,
            crate::audio_engine::content_placement::PairPlacementKind::Swap
        );
        assert!(
            receipt
                .source_after
                .matches(moved.input_runtime_ownership.slot_snapshot(SOURCE).unwrap())
        );
        assert!(
            receipt
                .target_after
                .matches(moved.input_runtime_ownership.slot_snapshot(TARGET).unwrap())
        );
        assert_eq!(
            receipt.source_after.binding_epoch,
            before[0].binding_epoch() + 1
        );
        assert_eq!(
            receipt.target_after.binding_epoch,
            before[1].binding_epoch() + 1
        );
        assert_eq!(retirement.pairs.len(), 1);
        assert!(retirement.pairs[0].retired_stems.is_none());
        assert!(retirement.pairs[0].retired_outgoing.is_none());
        assert!(retirement.pairs[0].retired_receipt.is_none());
        assert_eq!(
            moved.input_runtime_ownership.pair_reservation(SOURCE),
            guard.reservation
        );
        assert_eq!(
            moved.input_runtime_ownership.pair_reservation(TARGET),
            guard.reservation
        );
        for frames in [17, 409, 1301] {
            assert_eq!(render(&mut moved, frames), render(&mut control, frames));
        }
    }
}

fn move_fixture(wet: bool) -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    mixer.set_input_runtime_ownership(ownership.clone());
    let source = sample(0.6, 0.13);
    ownership.publish_test_source(SOURCE, &source, RATE, 11);
    mixer.load_sample(SOURCE, source.clone());
    assert!(mixer.publish_prepared_stems_rt(
        SOURCE,
        source_stems(&mixer, SOURCE, &source, 17),
        &mut ImmediateAudioBufferRetirement
    ));
    assert!(mixer.set_stem_mix_mode(SOURCE, StemMixMode::AllStems, 17));
    mixer.set_pad_key_lock(SOURCE, wet);
    mixer.set_pad_bpm(SOURCE, Some(119.75));
    mixer.set_pad_loop_region(SOURCE, 0.031_25, Some(1.5));
    mixer.set_speed(1.371_234_567_890_123);
    mixer.set_pad_gain(SOURCE, -2.0);
    mixer.set_pad_eq(SOURCE, -8.0, 2.0, -1.0);
    assert!(mixer.play_sample(SOURCE, 0.9));
    mixer
}

#[test]
fn guarded_move_installs_independent_empty_defaults_and_retires_residuals_as_one_owner() {
    let mut control = move_fixture(true);
    let mut moved = move_fixture(true);
    assert_eq!(render(&mut moved, 1657), render(&mut control, 1657));
    let source = LaneState::capture(&moved, SOURCE);
    let source_sample = moved.sample_bank[SOURCE].as_ref().unwrap().clone();
    let residual = stems(&source_sample, 71);
    let residual_identity = residual.complete_set_identity.clone();
    let outgoing = stems(&source_sample, 73);
    let outgoing_identity = outgoing.complete_set_identity.clone();
    moved.prepared_stems[TARGET] = Some(residual);
    moved.outgoing_stems[TARGET] = Some(outgoing);
    moved.set_pad_gain(TARGET, -29.0);
    moved.set_pad_eq(TARGET, -60.0, -11.0, 3.0);
    moved.set_pad_bpm(TARGET, Some(67.0));
    moved.pad_loop_start_frame[TARGET] = 53;
    moved.pad_loop_end_frame[TARGET] = Some(127);
    moved.pad_playhead_frame[TARGET] = Some(97);
    let work = guarded_pair_work(&moved);
    let guard = work.guard.clone();
    let mut retirement = RetainingPairRetirement::new(1);
    assert!(moved.pair_pad_rt(work, &mut retirement));
    source.assert_retained(&moved, TARGET);
    assert!(moved.sample_bank[SOURCE].is_none());
    assert!(moved.prepared_stems[SOURCE].is_none());
    assert!(moved.outgoing_stems[SOURCE].is_none());
    assert_eq!(moved.pad_gain_db[SOURCE], PAD_GAIN_DB_DEFAULT);
    assert_eq!(moved.pad_gain_smoothers[SOURCE], SmoothedGain::default());
    assert!(moved.pad_dsp_chains[SOURCE].source_history().is_none());
    assert_eq!(moved.pad_period_seconds[SOURCE], None);
    assert_eq!(moved.pad_phase_anchor_frame[SOURCE], 0.0);
    assert_eq!(moved.pad_accepted_timing[SOURCE], None);
    assert_eq!(moved.pad_loop_start_frame[SOURCE], 0);
    assert_eq!(moved.pad_loop_end_frame[SOURCE], None);
    assert_eq!(moved.pad_playhead_frame[SOURCE], None);
    assert_eq!(moved.stem_mix_mode[SOURCE], StemMixMode::FullMix);
    assert_eq!(retirement.pairs.len(), 1);
    assert!(Arc::ptr_eq(
        &retirement.pairs[0]
            .retired_stems
            .as_ref()
            .unwrap()
            .complete_set_identity,
        &residual_identity
    ));
    assert!(Arc::ptr_eq(
        &retirement.pairs[0]
            .retired_outgoing
            .as_ref()
            .unwrap()
            .complete_set_identity,
        &outgoing_identity
    ));
    let receipt = guard.native_acknowledgement().unwrap();
    assert_eq!(
        receipt.kind,
        crate::audio_engine::content_placement::PairPlacementKind::Move
    );
    assert_eq!(receipt.source_after.lifetime, 0);
    assert!(
        receipt
            .target_after
            .matches(moved.input_runtime_ownership.slot_snapshot(TARGET).unwrap())
    );
    for frames in [13, 257, 779] {
        assert_eq!(render(&mut moved, frames), render(&mut control, frames));
    }
}

#[test]
fn guarded_move_preserves_paused_native_voice_and_exact_resume_continuation() {
    let mut control = move_fixture(true);
    let mut moved = move_fixture(true);
    assert_eq!(render(&mut moved, 1657), render(&mut control, 1657));
    control.pause_sample(SOURCE);
    moved.pause_sample(SOURCE);
    let retained = LaneState::capture(&moved, SOURCE);
    let work = guarded_pair_work(&moved);
    let guard = work.guard.clone();
    let mut retirement = RetainingPairRetirement::new(1);
    assert!(moved.pair_pad_rt(work, &mut retirement));
    retained.assert_retained(&moved, TARGET);
    assert!(moved.voices[retained.index].paused);
    assert_eq!(render(&mut moved, 137), render(&mut control, 137));
    assert!(guard.native_acknowledgement().is_some());
    control.resume_sample(SOURCE);
    moved.resume_sample(TARGET);
    assert_eq!(render(&mut moved, 777), render(&mut control, 777));
}

#[test]
fn guarded_pair_capacity_retry_preserves_complete_payload_before_later_control() {
    let mut mixer = fixture(false);
    let source_before = mixer.input_runtime_ownership.slot_snapshot(SOURCE).unwrap();
    let work = guarded_pair_work(&mixer);
    let guard = work.guard.clone();
    let mut retirement = RetainingPairRetirement::new(0);
    assert!(!mixer.pair_pad_rt(work, &mut retirement));
    assert_eq!(guard.phase(), 0);
    assert!(guard.native_acknowledgement().is_none());
    assert!(
        source_before.same_projection(mixer.input_runtime_ownership.slot_snapshot(SOURCE).unwrap())
    );
    assert!(retirement.pairs.is_empty());
    let Some(crate::messages::ControlMessage::PadPair(work)) = mixer.take_deferred_control() else {
        panic!("whole Pair payload was not retained");
    };
    retirement.available = 1;
    assert!(mixer.pair_pad_rt(work, &mut retirement));
    assert!(guard.native_acknowledgement().is_some());
    assert_eq!(retirement.pairs.len(), 1);
}

#[test]
fn guarded_pair_rejects_each_obsolete_participant_without_changing_live_audio_image() {
    for index in 0..2 {
        let mut mixer = fixture(false);
        render(&mut mixer, 137);
        let source = LaneState::capture(&mixer, SOURCE);
        let target = LaneState::capture(&mixer, TARGET);
        let work = guarded_pair_work(&mixer);
        let guard = work.guard.clone();
        let source_before = mixer.input_runtime_ownership.slot_snapshot(SOURCE).unwrap();
        let target_before = mixer.input_runtime_ownership.slot_snapshot(TARGET).unwrap();
        guard.participants[index]
            .preparation_epoch
            .fetch_add(1, Ordering::AcqRel);
        let mut retirement = RetainingPairRetirement::new(1);
        assert!(!mixer.pair_pad_rt(work, &mut retirement));
        assert_eq!(guard.phase(), 3);
        assert!(guard.native_acknowledgement().is_none());
        assert_eq!(
            mixer.input_runtime_ownership.slot_snapshot(SOURCE),
            Some(source_before)
        );
        assert_eq!(
            mixer.input_runtime_ownership.slot_snapshot(TARGET),
            Some(target_before)
        );
        source.assert_retained(&mixer, SOURCE);
        target.assert_retained(&mixer, TARGET);
        assert_eq!(retirement.pairs.len(), 1);
    }
}

fn pair_owned_pause(
    mixer: &mut RtMixer,
) -> Arc<crate::audio_engine::hold_action::NativeHoldAction> {
    use crate::audio_engine::hold_action::{HoldActionRtWork, NativeHoldAction};
    let ownership = mixer.input_runtime_ownership.clone();
    let snapshot = ownership.slot_snapshot(SOURCE).unwrap();
    let intent = ownership.prepare_transport_intent(snapshot).unwrap();
    let action =
        NativeHoldAction::prepare(ownership.clone(), snapshot, intent.revision(), 173, 99).unwrap();
    ownership
        .commit_transport_intent(
            intent,
            || true,
            || {
                assert!(action.mark_admitted());
            },
        )
        .unwrap();
    let snapshot = ownership.slot_snapshot(SOURCE).unwrap();
    let write = ownership
        .prepare_slot_write_fixed(snapshot, snapshot.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                let pause = mixer.apply_pause_hold_rt(
                    HoldActionRtWork::pause(action.clone()),
                    991,
                    &mut ImmediateAudioBufferRetirement,
                );
                assert!(pause.effect_current);
                assert_eq!(
                    pause.state,
                    crate::audio_engine::hold_action::HoldActionState::PausedOwned
                );
            },
        )
        .unwrap();
    action
}

fn pair_owned_release(
    mixer: &mut RtMixer,
    action: &Arc<crate::audio_engine::hold_action::NativeHoldAction>,
) {
    use crate::audio_engine::hold_action::{HoldActionRtWork, HoldActionState};
    assert!(action.request_release());
    assert!(action.mark_release_admitted());
    let ownership = mixer.input_runtime_ownership.clone();
    let snapshot = match ownership.observe_content_key(action.key()) {
        PlacementObservation::Current(snapshot) => snapshot,
        other => panic!("retained Hold content was not current: {other:?}"),
    };
    let write = ownership
        .prepare_slot_write_fixed(snapshot, snapshot.lifetime())
        .unwrap();
    ownership
        .commit_slot_write_fixed(
            &write,
            || true,
            |_| {
                let release = mixer.apply_release_hold_rt(
                    HoldActionRtWork::release(action.clone()),
                    1003,
                    &mut ImmediateAudioBufferRetirement,
                );
                assert_eq!(release.state, HoldActionState::Resumed);
            },
        )
        .unwrap();
}

#[test]
fn own_pair_ack_carries_actual_pause_effect_and_both_legal_release_layout_orders() {
    for release_first in [false, true] {
        let mut mixer = fixture(true);
        render(&mut mixer, 1657);
        let action = pair_owned_pause(&mut mixer);
        let source_lane = lane(&mixer, SOURCE);
        let target_lane = lane(&mixer, TARGET);
        let original_effect = mixer.capture_hold_projection_rt(action.key()).unwrap();
        let mut retained = LaneState::capture(&mixer, SOURCE);
        let work = guarded_pair_work(&mixer);
        let prepared_revision = work.prepared.source().layout_revision();
        let guard = work.guard.clone();
        if release_first {
            // Release is already admitted to this exact effect/cohort. It advances the shared
            // marker but preserves every original Pair participant's authority/counter guard.
            pair_owned_release(&mut mixer, &action);
            assert!(guard.stable_current());
            assert!(!mixer.voices[source_lane].paused);
            let resumed = LaneState::capture(&mixer, SOURCE);
            assert_eq!(resumed.source_epoch_value, retained.source_epoch_value + 1);
            assert_eq!(resumed.generation, retained.generation);
            assert_eq!(resumed.native, retained.native);
            assert_eq!(resumed.position, retained.position);
            retained = resumed;
        }
        let mut retirement = RetainingPairRetirement::new(1);
        assert!(mixer.pair_pad_rt(work, &mut retirement));
        retained.assert_retained(&mixer, TARGET);
        let receipt = guard.native_acknowledgement().unwrap();
        if release_first {
            assert!(receipt.layout_revision > prepared_revision + 2);
            assert_eq!(receipt.holds, [None, None]);
        } else {
            assert_eq!(receipt.holds, [Some(original_effect), None]);
            assert!(receipt.holds[0].unwrap().effect_current);
            assert!(mixer.voices[source_lane].paused);
            assert_eq!(
                mixer.capture_hold_projection_rt(action.key()),
                Some(original_effect)
            );
            pair_owned_release(&mut mixer, &action);
        }
        assert!(!mixer.voices[source_lane].paused);
        assert!(!mixer.voices[target_lane].paused);
        assert_eq!(mixer.voices[source_lane].sample_id, TARGET);
        assert_eq!(mixer.voices[target_lane].sample_id, SOURCE);
        assert_eq!(mixer.voices[source_lane].generation, retained.generation);
        assert_eq!(
            mixer.input_runtime_ownership.pair_reservation(SOURCE),
            guard.reservation
        );
        assert_eq!(
            mixer.input_runtime_ownership.pair_reservation(TARGET),
            guard.reservation
        );
        // Later release cannot overwrite the immutable own Pair receipt observed at its claim.
        assert_eq!(guard.native_acknowledgement(), Some(receipt));
    }
}

#[test]
fn occupied_swap_preserves_wet_audio_native_fifo_and_filter_history() {
    assert_continuation(true);
}

#[test]
fn occupied_swap_preserves_paused_frozen_voice_lane_and_admitted_source() {
    let mut control = fixture(true);
    let mut moved = fixture(true);
    assert_eq!(render(&mut moved, 1657), render(&mut control, 1657));
    for mixer in [&mut control, &mut moved] {
        let replacement = sample(0.2, 0.19);
        mixer
            .input_runtime_ownership
            .publish_test_source(SOURCE, &replacement, RATE, 31);
        assert!(mixer.load_sample_rt(SOURCE, replacement, &mut ImmediateAudioBufferRetirement));
        mixer.pause_sample(SOURCE);
    }
    let retained = LaneState::capture(&moved, SOURCE);
    let voice = &moved.voices[retained.index];
    assert!(voice.paused);
    let frozen = voice.frozen_stems.as_ref().unwrap();
    let frozen_set = frozen.set.as_ref().unwrap().complete_set_identity.clone();
    let frozen_selection = frozen.selection;
    let frozen_transition = frozen.transition;
    let source_region = voice.source_loop_region;
    let permit = NativeVoiceHistoryContext {
        admission: voice.source_admission.as_ref().unwrap(),
        lifetime: &voice.history_lifetime,
        generation: voice.generation,
        preparation_epoch: voice.stretch.source_epoch_owner(),
        accepted: voice.source_timing.accepted,
    }
    .capture(voice.sample.as_ref().unwrap(), RATE)
    .unwrap();
    let bank = moved.sample_bank[SOURCE].as_ref().unwrap().clone();

    swap(&mut moved);

    retained.assert_retained(&moved, TARGET);
    let voice = &moved.voices[retained.index];
    assert!(voice.paused);
    assert_eq!(voice.source_loop_region, source_region);
    assert!(voice.source_admission.is_some());
    assert!(permit.current(voice.sample.as_ref().unwrap(), RATE));
    let frozen = voice.frozen_stems.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &frozen.set.as_ref().unwrap().complete_set_identity,
        &frozen_set
    ));
    assert_eq!(frozen.selection, frozen_selection);
    assert!(frozen.transition.matches_exact(frozen_transition));
    assert!(Arc::ptr_eq(
        &moved.sample_bank[TARGET].as_ref().unwrap().samples,
        &bank.samples
    ));
    assert!(!bank.same_source(voice.sample.as_ref().unwrap()));
    assert_eq!(render(&mut moved, 127), render(&mut control, 127));
    moved.resume_sample(TARGET);
    control.resume_sample(SOURCE);
    assert_eq!(render(&mut moved, 777), render(&mut control, 777));
}

#[test]
fn occupied_swap_keeps_selection_and_feedback_on_their_content_without_epoch_replacement() {
    let mut mixer = fixture(false);
    render(&mut mixer, 37);
    let source_selection = SelectedStemIdentity {
        identity: [17; 32],
        source_generation: 11,
        epoch: 41,
    };
    let target_selection = SelectedStemIdentity {
        identity: [29; 32],
        source_generation: 23,
        epoch: 43,
    };
    mixer.committed_stem_selection[SOURCE] = Some(source_selection);
    mixer.committed_stem_selection[TARGET] = Some(target_selection);
    mixer.pad_key_lock_request_id[SOURCE] = 47;
    mixer.pad_key_lock_request_id[TARGET] = 53;
    mixer.pad_key_lock_rejected[SOURCE] = true;
    mixer.pad_key_lock_pending_on[TARGET] = true;
    let source_ack = mixer.pad_key_lock_ack_source[SOURCE];
    let target_ack = mixer.pad_key_lock_ack_source[TARGET];
    let source_status = mixer.pad_key_lock_status[SOURCE];
    let target_status = mixer.pad_key_lock_status[TARGET];
    let ownership = mixer.input_runtime_ownership.clone();
    let acknowledgements = mixer.current_timing_acknowledgements.clone();
    let source_authority = ownership.authority_revision(SOURCE);
    let target_authority = ownership.authority_revision(TARGET);

    swap(&mut mixer);

    assert_eq!(
        mixer.committed_stem_selection[TARGET],
        Some(source_selection)
    );
    assert_eq!(
        mixer.committed_stem_selection[SOURCE],
        Some(target_selection)
    );
    assert_eq!(mixer.pad_key_lock_request_id[TARGET], 47);
    assert_eq!(mixer.pad_key_lock_request_id[SOURCE], 53);
    assert!(mixer.pad_key_lock_rejected[TARGET]);
    assert!(mixer.pad_key_lock_pending_on[SOURCE]);
    assert_eq!(mixer.pad_key_lock_ack_source[TARGET], source_ack);
    assert_eq!(mixer.pad_key_lock_ack_source[SOURCE], target_ack);
    assert!(mixer.pad_key_lock_status[TARGET] == source_status);
    assert!(mixer.pad_key_lock_status[SOURCE] == target_status);
    assert!(Arc::ptr_eq(&mixer.input_runtime_ownership, &ownership));
    assert!(Arc::ptr_eq(
        &mixer.current_timing_acknowledgements,
        &acknowledgements
    ));
    assert_eq!(ownership.authority_revision(TARGET), source_authority);
    assert_eq!(ownership.authority_revision(SOURCE), target_authority);
}

#[test]
fn occupied_swap_preserves_actual_accepted_timing_ack_and_admitted_history_permission() {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    mixer.set_input_runtime_ownership(ownership.clone());
    let source = sample(0.6, 0.13);
    let target = sample(0.3, 0.07);
    ownership.publish_test_source(SOURCE, &source, RATE, 11);
    ownership.publish_test_source(TARGET, &target, RATE, 23);
    mixer.load_sample(SOURCE, source.clone());
    mixer.load_sample(TARGET, target);
    let source_key = ownership
        .slot_snapshot(SOURCE)
        .unwrap()
        .content_key()
        .unwrap();
    let projection = AcceptedTimingProjection {
        revision: [71; 32],
        period_seconds: 0.500_123_456_789_012_3,
        origin_seconds: -0.031_251_234,
        sample_rate_hz: RATE,
        publication_epoch: 71,
    };
    let publication = PreparedSourcePermit::for_epoch(Arc::new(AtomicU64::new(71)), 71);
    publication.mark_pending().unwrap();
    assert!(mixer.publish_constant_timing_rt(
        SOURCE,
        PreparedConstantTiming {
            reference: source.clone(),
            publication: publication.clone(),
            projection,
            content_key: Some(source_key),
        },
        &mut ImmediateAudioBufferRetirement,
    ));
    assert_eq!(publication.status(), "accepted");
    let history = NativeHistoryContext {
        id: SOURCE,
        ownership: &ownership,
        acknowledgements: &mixer.current_timing_acknowledgements,
        preparation_epoch: &mixer.prepared_source_epochs[SOURCE],
    }
    .capture(&source, RATE, Some(projection))
    .unwrap();

    swap(&mut mixer);

    assert_eq!(mixer.pad_accepted_timing[TARGET], Some(projection));
    assert_eq!(mixer.pad_accepted_timing[SOURCE], None);
    assert_eq!(
        mixer
            .current_timing_acknowledgements
            .current_for_key(&ownership, source_key),
        71
    );
    assert_eq!(
        mixer
            .current_timing_acknowledgements
            .current_for_slot(&ownership, TARGET),
        71
    );
    assert_eq!(
        mixer
            .current_timing_acknowledgements
            .current_for_slot(&ownership, SOURCE),
        0
    );
    assert!(history.current(&source, RATE));
    assert_eq!(ownership.source_generation(TARGET, &source, RATE), Some(11));
}

#[test]
fn unrelated_pending_slot_write_preserves_current_adopted_wet_history_and_audio() {
    let mut control = fixture(true);
    let mut actual = fixture(true);
    let render_at = |mixer: &mut RtMixer, frame: u64, frames: usize| {
        let mut output = vec![0.0; frames];
        mixer.render_at_output_frame(frame, &mut output, &mut [0.0; NUM_SAMPLES]);
        output
    };
    assert_eq!(
        render_at(&mut actual, 0, 13),
        render_at(&mut control, 0, 13)
    );
    let ready = |mixer: &mut RtMixer| {
        [SOURCE, TARGET].into_iter().all(|id| {
            let index = lane(mixer, id);
            mixer.voices[index].stretch.source_preparation_ready()
        })
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !(ready(&mut actual) && ready(&mut control)) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(ready(&mut actual) && ready(&mut control));
    let prepared_native = [SOURCE, TARGET].map(|id| {
        let index = lane(&actual, id);
        let reference_index = lane(&control, id);
        assert_eq!(
            actual.voices[index].stretch.prepared_target_output_frame(),
            control.voices[reference_index]
                .stretch
                .prepared_target_output_frame()
        );
        actual.voices[index].stretch.prepared_native_address()
    });
    let first_lane = lane(&actual, SOURCE);
    let adoption_frame = actual.voices[first_lane]
        .stretch
        .prepared_target_output_frame()
        .unwrap();
    let second_lane = lane(&actual, TARGET);
    assert_eq!(
        actual.voices[second_lane]
            .stretch
            .prepared_target_output_frame(),
        Some(adoption_frame)
    );
    let mut frame = 13;
    let mut partitions = [31_usize, 777, 1, 257, 512].into_iter().cycle();
    while frame < adoption_frame {
        let frames = partitions
            .next()
            .unwrap()
            .min((adoption_frame - frame) as usize);
        assert_eq!(
            render_at(&mut actual, frame, frames),
            render_at(&mut control, frame, frames)
        );
        frame += frames as u64;
    }
    // Both real worker results adopt at their own exact output-frame boundary.
    assert_eq!(
        render_at(&mut actual, frame, 137),
        render_at(&mut control, frame, 137)
    );
    frame += 137;
    for (id, native) in [SOURCE, TARGET].into_iter().zip(prepared_native) {
        let index = lane(&actual, id);
        assert_ne!(native, 0);
        assert_eq!(actual.voices[index].stretch.native_state_address(), native);
        assert!(actual.voices[index].stretch.adopted_request_id().is_some());
    }
    let mut productive_signal = false;
    for _ in 0..16 {
        let output = render_at(&mut actual, frame, 257);
        assert_eq!(output, render_at(&mut control, frame, 257));
        productive_signal |= output.iter().any(|value| value.abs() > 1.0e-5);
        frame += 257;
    }
    assert!(productive_signal);

    let ownership = actual.input_runtime_ownership.clone();
    let snapshots = [SOURCE, TARGET].map(|id| ownership.slot_snapshot(id).unwrap());
    let permits = [SOURCE, TARGET].map(|id| {
        let index = lane(&actual, id);
        let voice = &actual.voices[index];
        let cell = ownership.slot_snapshot(id).unwrap().cell();
        NativeHistoryContext {
            id,
            ownership: &ownership,
            acknowledgements: &actual.current_timing_acknowledgements,
            preparation_epoch: &actual.prepared_source_epochs[cell.index()],
        }
        .capture(
            voice.sample.as_ref().unwrap(),
            RATE,
            voice.source_timing.accepted,
        )
        .unwrap()
    });
    let before = [SOURCE, TARGET].map(|id| {
        let state = LaneState::capture(&actual, id);
        let voice = &actual.voices[state.index];
        assert!(voice.stretch.wet_current_source_output_ready());
        assert!(voice.frozen_stems.is_none());
        assert!(voice.source_loop_region.is_none());
        assert!(
            actual.sample_bank[id]
                .as_ref()
                .unwrap()
                .same_source(voice.sample.as_ref().unwrap())
        );
        (
            state,
            voice.stretch.productive_history().unwrap(),
            voice.stretch.adopted_request_id(),
        )
    });
    let unrelated = ownership.slot_snapshot(215).unwrap();
    assert_eq!(unrelated.lifetime(), 0);
    let write = ownership.prepare_slot_write(unrelated, 0).unwrap();
    let continued_frames = 37;
    let result = ownership.commit_slot_write(
        &write,
        || {
            for (index, id) in [SOURCE, TARGET].into_iter().enumerate() {
                let key = snapshots[index].content_key().unwrap();
                assert!(matches!(
                    ownership.observe_content_key(key),
                    PlacementObservation::Pending
                ));
                assert!(ownership.content_key_current(key));
                assert!(permits[index].current(&before[index].0.sample, RATE));
                assert!(permits[index].current_effective_source(&before[index].0.sample, RATE));
                // Fresh current-bank capture defers while the global layout marker is odd.
                // The unchanged bank must not switch to retained-voice authority to compensate.
                assert!(
                    NativeHistoryContext {
                        id,
                        ownership: &ownership,
                        acknowledgements: &actual.current_timing_acknowledgements,
                        preparation_epoch: &actual.prepared_source_epochs[key.cell().index()],
                    }
                    .capture(
                        &before[index].0.sample,
                        RATE,
                        before[index].1.binding.accepted,
                    )
                    .is_none()
                );
            }
            assert!(
                ownership
                    .prepare_slot_write(snapshots[0], snapshots[0].lifetime())
                    .is_err()
            );
            assert_eq!(
                render_at(&mut actual, frame, continued_frames),
                render_at(&mut control, frame, continued_frames)
            );
            for (index, id) in [SOURCE, TARGET].into_iter().enumerate() {
                let (state, history, adopted) = &before[index];
                let voice = &actual.voices[state.index];
                let reference = &control.voices[lane(&control, id)];
                assert!(voice.active && !voice.paused);
                assert_eq!(voice.sample_id, id);
                assert_eq!(voice.generation, state.generation);
                assert!(Arc::ptr_eq(
                    &voice.history_lifetime,
                    &state.history_lifetime
                ));
                assert_eq!(
                    voice.history_lifetime.load(Ordering::Acquire),
                    state.history_generation
                );
                assert!(Arc::ptr_eq(
                    voice.stretch.source_epoch_owner(),
                    &state.source_epoch
                ));
                assert_eq!(
                    voice.stretch.source_epoch_owner().load(Ordering::Acquire),
                    state.source_epoch_value
                );
                assert_eq!(voice.stretch.native_state_address(), state.native);
                assert_eq!(voice.stretch.adopted_request_id(), *adopted);
                assert_eq!(
                    voice.stretch.pending_fifo_frames(),
                    reference.stretch.pending_fifo_frames()
                );
                assert!(
                    voice
                        .source_playback
                        .matches_exact(&reference.source_playback)
                );
                assert!(voice.stretch.wet_current_source_output_ready());
                assert!(voice.frozen_stems.is_none());
                let continued = voice.stretch.productive_history().unwrap();
                assert_eq!(continued.binding, history.binding);
                assert_eq!(
                    continued.fed_output_frames,
                    history.fed_output_frames + continued_frames as u64
                );
                assert!(
                    permits[index].current_effective_source(voice.sample.as_ref().unwrap(), RATE)
                );
            }
            false
        },
        |_| panic!("a rejected unrelated write must not apply"),
    );
    assert!(result.is_err());
    assert_eq!(ownership.slot_snapshot(215), Some(unrelated));
    for (snapshot, id) in snapshots.into_iter().zip([SOURCE, TARGET]) {
        assert_eq!(ownership.slot_snapshot(id), Some(snapshot));
        assert!(ownership.content_key_current(snapshot.content_key().unwrap()));
    }
    frame += continued_frames as u64;
    assert_eq!(
        render_at(&mut actual, frame, 257),
        render_at(&mut control, frame, 257)
    );
}
