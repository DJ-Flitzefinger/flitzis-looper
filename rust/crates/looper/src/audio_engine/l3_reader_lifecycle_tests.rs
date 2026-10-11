// Actual mixer reading and owner transfer after an admitted two-set handover.
// Admission is tested through the separate committed-generation productive path.

use super::*;
use std::sync::Weak;

const RATE: u32 = 44_100;
const FRAMES: usize = 1024;
const OLD_VALUE: f32 = 0.1;
const NEW_VALUE: f32 = 0.6;

struct CapacityRetirement {
    slots: usize,
    held: CollectingRetirement,
}

impl CapacityRetirement {
    fn new(slots: usize) -> Self {
        Self {
            slots,
            held: CollectingRetirement::default(),
        }
    }

    fn consume_slot(&mut self) {
        assert!(self.slots > 0, "an owner was transferred without retirement capacity");
        self.slots -= 1;
    }
}

impl AudioBufferRetirement for CapacityRetirement {
    fn retire_resident_capture(
        &mut self,
        capture: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        self.held.retire_resident_capture(capture);
    }

    fn retire_resident_cancellation(&mut self, cancelled: Arc<std::sync::atomic::AtomicBool>) {
        self.held.retire_resident_cancellation(cancelled);
    }

    fn retire_resident_transaction(&mut self, transaction: Box<crate::messages::ResidentTransaction>) {
        self.held.retire_resident_transaction(transaction);
    }

    fn retire_cold_adoption(&mut self, adoption: Arc<std::sync::atomic::AtomicU8>) {
        self.held.retire_cold_adoption(adoption);
    }

    fn retire_sample(&mut self, sample: SampleBuffer) {
        self.consume_slot();
        self.held.retire_sample(sample);
    }

    fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
        self.consume_slot();
        self.held.retire_prepared_stems(stems);
    }

    fn retire_constant_timing(&mut self, timing: PreparedConstantTiming) {
        self.consume_slot();
        self.held.retire_constant_timing(timing);
    }

    fn retire_global_playback_batch(
        &mut self,
        batch: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.held.retire_global_playback_batch(batch);
    }

    fn retire_accepted_timing_refresh(
        &mut self,
        refresh: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.held.retire_accepted_timing_refresh(refresh);
    }

    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }
}

fn handover(ratio: f64) -> (RtMixer, Weak<[u8; 32]>, Weak<[u8; 32]>) {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let source = create_test_sample(1, FRAMES, 0.9);
    mixer.load_sample(0, source.clone());
    let v1 = create_test_prepared_stems_with_values(
        &source,
        1,
        RATE,
        FRAMES,
        [OLD_VALUE, 0.02, 0.03, 0.04],
    );
    let v2 = create_test_prepared_stems_with_values(
        &source,
        1,
        RATE,
        FRAMES,
        [NEW_VALUE, 0.12, 0.13, 0.14],
    );
    let old_owner = Arc::downgrade(&v1.complete_set_identity);
    let new_owner = Arc::downgrade(&v2.complete_set_identity);
    assert!(mixer.publish_prepared_stems(0, v1));
    assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
    assert!(mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS, 42));
    mixer.set_speed(ratio);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    // Install the exact already-admitted state without bypassing admission in product code.
    let predecessor = mixer.prepared_stems[0].replace(v2).unwrap();
    mixer.stem_transitions[0] = StemTransition::start(
        StemRenderSelection::for_stems(&predecessor, STEM_MASK_VOCALS),
        STEM_TRANSITION_RAMP_FRAMES,
    );
    mixer.outgoing_stems[0] = Some(predecessor);
    (mixer, old_owner, new_owner)
}

fn render(
    mixer: &mut RtMixer,
    start: u64,
    frames: usize,
    retirement: &mut CapacityRetirement,
) -> Vec<f32> {
    let mut output = vec![0.0; frames];
    mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        start,
        retirement,
    );
    output
}

fn expected_ramp(frame: usize, ratio: f64) -> f32 {
    let to_gain = (frame as f64 * ratio / 128.0).min(1.0) as f32;
    OLD_VALUE * (1.0 - to_gain) + NEW_VALUE * to_gain
}

#[test]
fn outgoing_owner_survives_completed_ramp_under_pressure_then_transfers_on_retry() {
    let (mut mixer, old_owner, new_owner) = handover(0.73);
    let mut retirement = CapacityRetirement::new(0);
    let mut output = render(&mut mixer, 0, 137, &mut retirement);
    assert!(mixer.stem_transitions[0].is_active());
    assert!(mixer.outgoing_stems[0].is_some());
    output.extend(render(&mut mixer, 137, 39, &mut retirement));
    for (frame, actual) in output.iter().enumerate() {
        assert!((*actual - expected_ramp(frame, 0.73)).abs() < 1.0e-5);
    }
    assert!(!mixer.stem_transitions[0].is_active());
    assert!(mixer.outgoing_stems[0].is_some());
    assert!(retirement.held.stems.is_empty());
    assert!(old_owner.upgrade().is_some());
    assert!(new_owner.upgrade().is_some());
    let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
    assert_eq!(voice.source_playback.position().frame, (176.0_f64 * 0.73).floor() as usize);
    assert!((voice.source_playback.position().fraction - (176.0_f64 * 0.73).fract()).abs() < 1.0e-12);

    retirement.slots = 1;
    assert!((render(&mut mixer, 176, 1, &mut retirement)[0] - NEW_VALUE).abs() < 1.0e-5);
    assert!(mixer.outgoing_stems[0].is_none());
    assert_eq!(retirement.held.stems.len(), 1);
    assert!(Arc::ptr_eq(
        &retirement.held.stems[0].complete_set_identity,
        &old_owner.upgrade().unwrap(),
    ));
    // The sink still owns V1; releasing the transfer outside rendering ends its final use.
    retirement.held.stems.clear();
    assert!(old_owner.upgrade().is_none());
    assert!(new_owner.upgrade().is_some());
}

#[test]
fn scalar_controls_and_unaccepted_publication_cannot_replace_an_occupied_outgoing_slot() {
    let (mut mixer, old_owner, new_owner) = handover(1.0);
    let mut retirement = CapacityRetirement::new(8);
    let transition = mixer.stem_transitions[0];
    let selection = mixer.stem_render_selection(0);
    assert!(!mixer.set_stem_enabled_mask(0, 0, 42));
    assert!(!mixer.set_stem_enabled_mask(0, STEM_MASK_DRUMS, 42));
    assert!(!mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
    mixer.set_stem_pair_full_mix_rt(0, &mut retirement);
    assert!(mixer.release_pair_components[0]);
    assert!(mixer.set_stem_enabled_mask(0, STEM_MASK_VOCALS, 42));
    assert!(mixer.set_stem_mix_mode(0, StemMixMode::AllStems, 42));
    assert!(!mixer.release_pair_components[0]);
    let v3 = create_test_prepared_stems_with_values(
        mixer.sample_bank[0].as_ref().unwrap(),
        1,
        RATE,
        FRAMES,
        [0.8, 0.0, 0.0, 0.0],
    );
    assert!(!mixer.publish_prepared_stems(0, v3));
    assert!(mixer.stem_transitions[0].matches_exact(transition));
    assert_eq!(mixer.stem_render_selection(0), selection);
    assert!(Arc::ptr_eq(
        &mixer.outgoing_stems[0].as_ref().unwrap().complete_set_identity,
        &old_owner.upgrade().unwrap(),
    ));
    assert!(Arc::ptr_eq(
        &mixer.prepared_stems[0].as_ref().unwrap().complete_set_identity,
        &new_owner.upgrade().unwrap(),
    ));
    let output = render(&mut mixer, 0, 17, &mut retirement);
    for (frame, actual) in output.iter().enumerate() {
        assert!((*actual - expected_ramp(frame, 1.0)).abs() < 1.0e-5);
    }
    assert!(retirement.held.stems.is_empty());
}

#[test]
fn actual_source_replacement_freezes_both_readers_and_paused_or_running_voice_selection() {
    for paused in [false, true] {
        let (mut mixer, old_owner, new_owner) = handover(0.73);
        let mut retirement = CapacityRetirement::new(16);
        render(&mut mixer, 0, 19, &mut retirement);
        if paused {
            mixer.pause_sample(0);
        }
        let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
        let clock = voice.source_playback;
        let generation = voice.generation;
        let selection = mixer.stem_render_selection(0);
        let transition = mixer.stem_transitions[0];
        let replacement = create_test_sample(1, FRAMES, -0.8);
        assert!(mixer.load_sample_rt(0, replacement, &mut retirement));
        let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
        assert!(voice.source_playback.matches_exact(&clock));
        assert_eq!(voice.generation, generation);
        let frozen = voice.frozen_stems.as_ref().unwrap();
        assert_eq!(frozen.selection, selection);
        assert!(frozen.transition.matches_exact(transition));
        assert!(Arc::ptr_eq(
            &frozen.set.as_ref().unwrap().complete_set_identity,
            &new_owner.upgrade().unwrap(),
        ));
        assert!(Arc::ptr_eq(
            &frozen.outgoing.as_ref().unwrap().complete_set_identity,
            &old_owner.upgrade().unwrap(),
        ));
        assert!(mixer.prepared_stems[0].is_none());
        assert!(mixer.outgoing_stems[0].is_none());
        assert_eq!(retirement.held.stems.len(), 2);
        retirement.held.stems.clear();
        assert!(old_owner.upgrade().is_some());
        assert!(new_owner.upgrade().is_some());
        assert!(mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0));
        if paused {
            assert!(render(&mut mixer, 19, 31, &mut retirement).iter().all(|value| *value == 0.0));
            let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
            assert!(voice.source_playback.matches_exact(&clock));
            assert!(voice.frozen_stems.as_ref().unwrap().transition.matches_exact(transition));
            mixer.resume_sample(0);
        }
        let output = render(&mut mixer, 50, 48, &mut retirement);
        for (offset, actual) in output.iter().enumerate() {
            assert!((*actual - expected_ramp(19 + offset, 0.73)).abs() < 1.0e-5);
        }
        // The old bank owners have been released; the old voice still owns both actual sides.
        assert!(retirement.held.stems.is_empty());
        assert!(old_owner.upgrade().is_some());
        assert!(new_owner.upgrade().is_some());
        mixer.stop_sample_rt(0, &mut retirement);
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert_eq!(retirement.held.stems.len(), 2);
        assert!(old_owner.upgrade().is_some());
        assert!(new_owner.upgrade().is_some());
        retirement.held.stems.clear();
        assert!(old_owner.upgrade().is_none());
        assert!(new_owner.upgrade().is_none());
    }
}

#[test]
fn unload_transfers_current_and_outgoing_owners_without_rendering_them_again() {
    let (mut mixer, old_owner, new_owner) = handover(1.0);
    let mut retirement = CapacityRetirement::new(8);
    render(&mut mixer, 0, 7, &mut retirement);
    assert!(mixer.unload_sample_rt(0, &mut retirement));
    assert!(mixer.sample_bank[0].is_none());
    assert!(mixer.prepared_stems[0].is_none());
    assert!(mixer.outgoing_stems[0].is_none());
    assert!(!mixer.stem_transitions[0].is_active());
    assert!(mixer.voices.iter().all(|voice| !voice.active));
    assert_eq!(retirement.held.stems.len(), 2);
    assert_eq!(retirement.held.samples.len(), 2);
    assert!(old_owner.upgrade().is_some());
    assert!(new_owner.upgrade().is_some());
    assert!(render(&mut mixer, 7, 32, &mut retirement).iter().all(|value| *value == 0.0));
    assert_eq!(retirement.held.stems.len(), 2);
    retirement.held.stems.clear();
    assert!(old_owner.upgrade().is_none());
    assert!(new_owner.upgrade().is_none());
}

#[test]
fn frozen_paused_outgoing_owner_waits_for_consumption_and_capacity_before_transfer() {
    let (mut mixer, old_owner, new_owner) = handover(0.73);
    let mut retirement = CapacityRetirement::new(16);
    render(&mut mixer, 0, 19, &mut retirement);
    mixer.pause_sample(0);
    assert!(mixer.load_sample_rt(0, create_test_sample(1, FRAMES, -0.8), &mut retirement));
    retirement.held.stems.clear();
    let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
    let paused_clock = voice.source_playback;
    let paused_transition = voice.frozen_stems.as_ref().unwrap().transition;
    retirement.slots = 0;
    assert!(render(&mut mixer, 19, 177, &mut retirement).iter().all(|value| *value == 0.0));
    let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
    assert!(voice.source_playback.matches_exact(&paused_clock));
    assert!(voice.frozen_stems.as_ref().unwrap().transition.matches_exact(paused_transition));
    assert!(voice.frozen_stems.as_ref().unwrap().outgoing.is_some());
    assert!(old_owner.upgrade().is_some());
    assert!(retirement.held.stems.is_empty());
    mixer.resume_sample(0);
    let output = render(&mut mixer, 196, 177, &mut retirement);
    for (offset, actual) in output.iter().enumerate() {
        assert!((*actual - expected_ramp(19 + offset, 0.73)).abs() < 1.0e-5);
    }
    let frozen = mixer.voices.iter().find(|voice| voice.active).unwrap().frozen_stems.as_ref().unwrap();
    assert!(!frozen.transition.is_active());
    assert!(frozen.outgoing.is_some());
    assert!(frozen.set.is_some());
    assert!(old_owner.upgrade().is_some());
    assert!(new_owner.upgrade().is_some());
    assert!(retirement.held.stems.is_empty());
    retirement.slots = 1;
    assert!((render(&mut mixer, 373, 1, &mut retirement)[0] - NEW_VALUE).abs() < 1.0e-5);
    let frozen = mixer.voices.iter().find(|voice| voice.active).unwrap().frozen_stems.as_ref().unwrap();
    assert!(frozen.outgoing.is_none());
    assert!(frozen.set.is_some());
    assert_eq!(retirement.held.stems.len(), 1);
    assert!(Arc::ptr_eq(&retirement.held.stems[0].complete_set_identity, &old_owner.upgrade().unwrap()));
    retirement.held.stems.clear();
    assert!(old_owner.upgrade().is_none());
    assert!(new_owner.upgrade().is_some());
}
