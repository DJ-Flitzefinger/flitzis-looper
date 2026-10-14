//! Actual Hold/Pair control composition with the production boxed callback mixer.
//! This bounded complete FullMix fixture does not certify selected ALL, wet adoption,
//! musical/resource acceptance, controller gestures or the whole R2 parent.

use super::{PadPairRtWork, PadPairTicket, control};
use crate::audio_engine::audio_stream::{
    create_stream_mixer, drain_control_messages, render_scheduled_audio,
};
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, AudioBufferRetirementWorker, RtAudioBufferRetirement,
    create_audio_buffer_retirement,
};
use crate::audio_engine::content_placement::{ContentKey, PairPlacementKind};
use crate::audio_engine::hold_action::{HoldActionRtWork, HoldActionState, HoldActionTicket};
use crate::audio_engine::input_mapping::launch_admission_tests::{LaunchFixture, OTHER, RATE};
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::native_history_permit::NativeVoiceHistoryContext;
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::source_playback::FractionalSourcePosition;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, PreparedStemSet, SampleBuffer, TriggerQuantization};
use pyo3::Python;
use pyo3::types::PyInt;
use rtrb::RingBuffer;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const SOURCE: usize = 0;
const EMPTY: usize = 2;

/// Restrict actual callback admission while all accepted owners still use the real
/// off-callback retirement worker. No work is popped or reconstructed to create pressure.
struct RetirementGate {
    available: usize,
    inner: RtAudioBufferRetirement,
}

impl AudioBufferRetirement for RetirementGate {
    fn retire_pad_pair(&mut self, work: Box<PadPairRtWork>) {
        self.inner.retire_pad_pair(work);
    }
    fn retire_hold_action(&mut self, work: Box<HoldActionRtWork>) {
        self.inner.retire_hold_action(work);
    }
    fn retire_resident_capture(
        &mut self,
        capture: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        self.inner.retire_resident_capture(capture);
    }
    fn retire_resident_cancellation(&mut self, cancellation: Arc<AtomicBool>) {
        self.inner.retire_resident_cancellation(cancellation);
    }
    fn retire_resident_transaction(&mut self, work: Box<crate::messages::ResidentTransaction>) {
        self.inner.retire_resident_transaction(work);
    }
    fn retire_cold_adoption(&mut self, adoption: Arc<AtomicU8>) {
        self.inner.retire_cold_adoption(adoption);
    }
    fn retire_sample(&mut self, sample: SampleBuffer) {
        self.inner.retire_sample(sample);
    }
    fn retire_prepared_stems(&mut self, stems: PreparedStemSet) {
        self.inner.retire_prepared_stems(stems);
    }
    fn retire_constant_timing(
        &mut self,
        timing: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
        self.inner.retire_constant_timing(timing);
    }
    fn retire_global_playback_batch(
        &mut self,
        batch: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.inner.retire_global_playback_batch(batch);
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        refresh: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.inner.retire_accepted_timing_refresh(refresh);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.available.min(self.inner.available_retirement_slots())
    }
}

struct CallbackFixture {
    control: LaunchFixture,
    mixer: Box<RtMixer>,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    quantization: TriggerQuantization,
    feedback: Vec<AudioMessage>,
    retirement: RetirementGate,
    _worker: AudioBufferRetirementWorker,
}

impl CallbackFixture {
    fn new(capacity: usize) -> Self {
        let mut control = LaunchFixture::new(capacity);
        let (_, feedback) = RingBuffer::<AudioMessage>::new(32);
        control.engine.test_control_channels =
            Some((control.producer.clone(), Arc::new(Mutex::new(feedback))));
        let mut mixer = create_stream_mixer(
            1,
            RATE,
            control.engine.current_timing_acknowledgements.clone(),
            control.engine.input_runtime_ownership.clone(),
            control.engine.prepared_source_epochs.clone(),
        )
        .unwrap();
        for id in [SOURCE, OTHER] {
            mixer.load_sample(
                id,
                control.engine.sample_cache.lock().unwrap()[id]
                    .clone()
                    .unwrap(),
            );
            mixer.set_pad_loop_region(id, 16.0 / f64::from(RATE), Some(96.0 / f64::from(RATE)));
        }
        mixer.set_pad_eq(SOURCE, -8.0, 2.0, -1.0);
        mixer.set_pad_eq(OTHER, 1.0, -3.0, -9.0);
        let (retirement, worker) = create_audio_buffer_retirement();
        let mut fixture = Self {
            control,
            mixer,
            scheduler: FixedCapacityScheduler::new(),
            transport: TransportTimeline::new(RATE),
            quantization: TriggerQuantization::Immediate,
            feedback: Vec::new(),
            retirement: RetirementGate {
                available: usize::MAX,
                inner: retirement,
            },
            _worker: worker,
        };
        for id in [SOURCE, OTHER] {
            fixture.control.engine.play_sample(id, 1.0, None).unwrap();
            assert_eq!(fixture.drain(), 1);
        }
        fixture.render(19);
        // B is intentionally paused by an independent accepted transport gesture.
        // A's hold and release must never acquire or undo B's pause.
        fixture.control.engine.pause_sample(OTHER).unwrap();
        assert_eq!(fixture.drain(), 1);
        fixture
    }

    fn drain(&mut self) -> usize {
        drain_control_messages(
            &mut self.control.consumer,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut self.quantization,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        )
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        let frame = self.transport.output_frame();
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
            frame,
            1,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        output
    }

    fn hold(&self) -> HoldActionTicket {
        Python::attach(|py| {
            self.control
                .engine
                .pause_waveform_hold(SOURCE, PyInt::new(py, 701).as_any(), None)
                .unwrap()
        })
    }

    fn prepare_pair(&self, target: usize) -> PadPairTicket {
        let captured = self
            .control
            .engine
            ._capture_pad_pair(SOURCE, target)
            .unwrap();
        // Internal preparation has the actual fixture's mono format; the public
        // device-free default is stereo and cannot describe this callback.
        control::prepare(&self.control.engine, &captured, RATE, 1).unwrap()
    }

    fn finish(&self, ticket: &PadPairTicket) {
        let ack = ticket.own_acknowledgement().unwrap();
        let receipt = ticket.native_acknowledgement().unwrap();
        assert!(self.control.engine._reconcile_pad_pair(ticket).unwrap());
        assert!(
            self.control
                .engine
                ._validate_pad_pair_acknowledgement(ticket, &ack)
                .unwrap()
        );
        self.control.engine._finish_pad_pair(ticket, &ack).unwrap();
        for slot in [receipt.source_after.slot, receipt.target_after.slot] {
            assert_eq!(
                self.control
                    .engine
                    .input_runtime_ownership
                    .pair_reservation(slot),
                0
            );
        }
    }
}

struct LaneEvidence {
    lane: usize,
    key: ContentKey,
    generation: u64,
    lifetime: Arc<AtomicU64>,
    lifetime_value: u64,
    epoch: Arc<AtomicU64>,
    epoch_value: u64,
    native: usize,
    fifo: (usize, usize),
    position: FractionalSourcePosition,
    sample: SampleBuffer,
    accepted: Option<crate::audio_engine::constant_timing::AcceptedTimingProjection>,
    legacy_period: Option<f64>,
    legacy_origin: f64,
    history: Option<ProductiveSourceHistory>,
    dsp: ProductiveSourceHistory,
}

impl LaneEvidence {
    fn capture(mixer: &RtMixer, id: usize) -> Self {
        let lane = mixer
            .voices
            .iter()
            .position(|voice| voice.is_playing_sample(id))
            .unwrap();
        let voice = &mixer.voices[lane];
        let sample = voice.sample.as_ref().unwrap().clone();
        let admission = voice.source_admission.as_ref().unwrap();
        let permit = NativeVoiceHistoryContext {
            admission,
            lifetime: &voice.history_lifetime,
            generation: voice.generation,
            preparation_epoch: voice.stretch.source_epoch_owner(),
            accepted: voice.source_timing.accepted,
        }
        .capture(&sample, RATE)
        .unwrap();
        assert!(permit.current_effective_source(&sample, RATE));
        Self {
            lane,
            key: admission.content_key().unwrap(),
            generation: voice.generation,
            lifetime: voice.history_lifetime.clone(),
            lifetime_value: voice.history_lifetime.load(Ordering::Acquire),
            epoch: voice.stretch.source_epoch_owner().clone(),
            epoch_value: voice.stretch.source_epoch_owner().load(Ordering::Acquire),
            native: voice.stretch.native_state_address(),
            fifo: voice.stretch.pending_fifo_frames(),
            position: voice.source_playback.position(),
            sample,
            accepted: voice.source_timing.accepted,
            legacy_period: voice.source_timing.legacy_period_seconds,
            legacy_origin: voice.source_timing.legacy_origin_frame,
            history: voice.stretch.productive_history(),
            dsp: mixer.dsp_source_history_for_test(id).unwrap(),
        }
    }

    fn assert_retained(&self, mixer: &RtMixer, id: usize, pause_changes: u64) {
        let voice = &mixer.voices[self.lane];
        assert!(voice.active);
        assert_eq!(voice.sample_id, id);
        assert_eq!(voice.generation, self.generation);
        assert_eq!(
            voice.source_admission.as_ref().unwrap().content_key(),
            Some(self.key)
        );
        assert!(Arc::ptr_eq(&voice.history_lifetime, &self.lifetime));
        assert_eq!(
            voice.history_lifetime.load(Ordering::Acquire),
            self.lifetime_value
        );
        assert!(Arc::ptr_eq(voice.stretch.source_epoch_owner(), &self.epoch));
        assert_eq!(
            voice.stretch.source_epoch_owner().load(Ordering::Acquire),
            self.epoch_value + pause_changes
        );
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.stretch.pending_fifo_frames(), self.fifo);
        assert_eq!(voice.source_playback.position(), self.position);
        assert!(voice.sample.as_ref().unwrap().same_window(&self.sample));
        assert!(Arc::ptr_eq(
            &voice.sample.as_ref().unwrap().samples,
            &self.sample.samples
        ));
        assert_eq!(voice.source_timing.accepted, self.accepted);
        assert_eq!(
            voice.source_timing.legacy_period_seconds,
            self.legacy_period
        );
        assert_eq!(voice.source_timing.legacy_origin_frame, self.legacy_origin);
        assert_history(voice.stretch.productive_history(), self.history);
        assert_history(mixer.dsp_source_history_for_test(id), Some(self.dsp));
    }
}

fn assert_history(
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
        _ => panic!("living source/filter history was replaced"),
    }
}

fn assert_hold_pair_matrix(target: usize) {
    for early in [false, true] {
        for release_first in [false, true] {
            let mut fixture = CallbackFixture::new(8);
            let hold = fixture.hold();
            assert_eq!(hold.publication_status(), "pause_pending");
            assert!(hold.action.effect().is_none());
            if !early {
                assert_eq!(fixture.drain(), 1);
                assert_eq!(hold.publication_status(), "paused_owned");
            }
            let source = LaneEvidence::capture(&fixture.mixer, SOURCE);
            let other = LaneEvidence::capture(&fixture.mixer, OTHER);
            assert_eq!(source.key, hold.action.key());
            assert!(fixture.mixer.voices[other.lane].paused);
            let ticket = fixture.prepare_pair(target);
            assert!(ticket.own_acknowledgement().is_none());
            if release_first {
                assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
            }
            fixture.control.engine._publish_pad_pair(&ticket).unwrap();
            if !release_first {
                assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
            }
            assert!(hold.release_requested());
            assert!(hold.release_admitted());
            assert_eq!(fixture.drain(), if early { 3 } else { 2 });
            assert_eq!(hold.action.state(), HoldActionState::Resumed);
            let effect = hold.action.effect().unwrap();
            assert_eq!(effect.key, source.key);
            assert_eq!(effect.cohort_mask, 1 << source.lane);
            assert_eq!(effect.changed_mask, 1 << source.lane);
            assert_eq!(effect.generations[source.lane], source.generation);
            let receipt = ticket.native_acknowledgement().unwrap();
            assert_eq!(
                receipt.kind,
                if target == EMPTY {
                    PairPlacementKind::Move
                } else {
                    PairPlacementKind::Swap
                }
            );
            assert_eq!(receipt.target_after.lifetime, source.key.lifetime());
            if release_first {
                assert!(receipt.holds[0].is_none());
            } else {
                let carried = receipt.holds[0].unwrap();
                assert_eq!(carried.action_id, hold.action.action_id());
                assert_eq!(carried.effect, Some(effect));
                assert!(carried.effect_current);
                assert_eq!(carried.state, HoldActionState::PausedOwned);
            }
            assert!(receipt.holds[1].is_none());
            source.assert_retained(&fixture.mixer, target, if early { 2 } else { 1 });
            other.assert_retained(
                &fixture.mixer,
                if target == EMPTY { OTHER } else { SOURCE },
                0,
            );
            assert!(!fixture.mixer.voices[source.lane].paused);
            assert!(fixture.mixer.voices[other.lane].paused);
            fixture.finish(&ticket);
            assert_eq!(hold.current_control_slot(), None);
            let terminal = hold.action.projection();
            assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
            assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
            assert_eq!(fixture.drain(), 0);
            assert_eq!(hold.action.projection(), terminal);
            assert_eq!(ticket.native_acknowledgement(), Some(receipt));
        }
    }
}

#[test]
fn actual_move_hold_before_after_pause_ack_and_both_release_layout_orders() {
    assert_hold_pair_matrix(EMPTY);
}

#[test]
fn actual_swap_hold_before_after_pause_ack_and_both_release_layout_orders_preserves_paused_b() {
    assert_hold_pair_matrix(OTHER);
}

#[test]
fn actual_pair_then_release_producer_pressure_retries_original_action_once() {
    for target in [EMPTY, OTHER] {
        let mut fixture = CallbackFixture::new(1);
        let hold = fixture.hold();
        assert_eq!(fixture.drain(), 1);
        let source = LaneEvidence::capture(&fixture.mixer, SOURCE);
        let other = LaneEvidence::capture(&fixture.mixer, OTHER);
        let effect = hold.action.effect().unwrap();
        let ticket = fixture.prepare_pair(target);
        fixture.control.engine._publish_pad_pair(&ticket).unwrap();
        for _ in 0..3 {
            assert!(!fixture.control.engine.release_waveform_hold(&hold).unwrap());
            assert!(hold.release_requested());
            assert!(!hold.release_admitted());
            assert_eq!(hold.action.effect(), Some(effect));
        }
        assert_eq!(fixture.drain(), 1);
        source.assert_retained(&fixture.mixer, target, 0);
        other.assert_retained(
            &fixture.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
        assert_eq!(hold.action.state(), HoldActionState::PausedOwned);
        assert_eq!(hold.current_control_slot(), None);
        assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
        assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
        assert_eq!(fixture.drain(), 1);
        source.assert_retained(&fixture.mixer, target, 1);
        other.assert_retained(
            &fixture.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
        assert!(fixture.mixer.voices[other.lane].paused);
        assert_eq!(hold.action.state(), HoldActionState::Resumed);
        fixture.finish(&ticket);
        assert_eq!(fixture.drain(), 0);
    }
}

#[test]
fn actual_pair_then_release_callback_retirement_pressure_preserves_own_effect_until_retry() {
    for target in [EMPTY, OTHER] {
        let mut fixture = CallbackFixture::new(4);
        let hold = fixture.hold();
        assert_eq!(fixture.drain(), 1);
        let source = LaneEvidence::capture(&fixture.mixer, SOURCE);
        let other = LaneEvidence::capture(&fixture.mixer, OTHER);
        let effect = hold.action.effect().unwrap();
        let ticket = fixture.prepare_pair(target);
        fixture.control.engine._publish_pad_pair(&ticket).unwrap();
        assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
        fixture.retirement.available = 1;
        assert_eq!(fixture.drain(), 1);
        let receipt = ticket.native_acknowledgement().unwrap();
        assert_eq!(receipt.holds[0].unwrap().effect, Some(effect));
        source.assert_retained(&fixture.mixer, target, 0);
        other.assert_retained(
            &fixture.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
        assert_eq!(hold.action.state(), HoldActionState::PausedOwned);
        assert!(hold.release_admitted());
        for _ in 0..3 {
            assert!(fixture.control.engine.release_waveform_hold(&hold).unwrap());
            assert_eq!(fixture.drain(), 0);
            assert_eq!(hold.action.effect(), Some(effect));
            assert!(fixture.mixer.voices[source.lane].paused);
        }
        fixture.finish(&ticket);
        assert_eq!(hold.current_control_slot(), Some(target));
        fixture.retirement.available = usize::MAX;
        assert_eq!(fixture.drain(), 1);
        source.assert_retained(&fixture.mixer, target, 1);
        other.assert_retained(
            &fixture.mixer,
            if target == EMPTY { OTHER } else { SOURCE },
            0,
        );
        assert!(fixture.mixer.voices[other.lane].paused);
        assert_eq!(hold.action.state(), HoldActionState::Resumed);
        assert_eq!(ticket.native_acknowledgement(), Some(receipt));
        assert_eq!(fixture.drain(), 0);
    }
}
