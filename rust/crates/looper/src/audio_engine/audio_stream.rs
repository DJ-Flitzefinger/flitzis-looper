//! Audio Stream Module
//!
//! This module handles CPAL audio stream management including:
//! - Stream initialization and configuration
//! - Audio callback setup
//! - Real-time message processing
//! - Error handling for audio stream operations

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Stream, StreamConfig};
use env_logger::{Builder, Env};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, AudioBufferRetirementWorker, CallbackCloseOwner,
    RtAudioBufferRetirement, create_audio_buffer_retirement_with_callback,
};
use crate::audio_engine::constant_timing::CurrentTimingAcknowledgements;
use crate::audio_engine::constants::{MAX_VOICES, NUM_SAMPLES};
use crate::audio_engine::content_placement::PlacementError;
use crate::audio_engine::content_placement::SlotSnapshot;
use crate::audio_engine::mixer::{RtMixer, RtRenderPadActivity};
use crate::audio_engine::scheduler::{
    DueEvent, FixedCapacityScheduler, ScheduledCommand, TransportScheduler,
};
use crate::audio_engine::timing::{
    InputClock, OutputClockMapper, OutputClockSnapshot, SharedOutputClock,
};
use crate::audio_engine::transport::{QuantizeGrid, TransportTimeline};
use crate::messages::{AudioMessage, ControlMessage, ControlParameterMessage, TriggerQuantization};
use std::cell::RefCell;

pub(crate) const MAX_CONTROL_MESSAGES_PER_CALLBACK: usize = 64;
pub(crate) const MAX_PARAMETER_MESSAGES_PER_CALLBACK: usize = 64;

/// Handle to the audio stream with associated message channels
pub struct AudioStreamHandle {
    // Declaration order is intentional: stream closure hands its box to the still-live
    // worker before the control thread requests worker drain/join.
    pub stream: Stream,
    _retirement_worker: AudioBufferRetirementWorker,
    pub producer: Arc<Mutex<Producer<ControlMessage>>>,
    pub(crate) parameter_producer: Arc<Mutex<Producer<ControlParameterMessage>>>,
    pub consumer: Arc<Mutex<Consumer<AudioMessage>>>,
    pub output_channels: usize,
    pub output_sample_rate: u32,
    /// Sticky stream-lifecycle failure; control can observe it without another callback.
    pub(crate) stream_failed: Arc<AtomicBool>,
    pub(crate) output_clock: Arc<SharedOutputClock>,
    pub(crate) device_descriptor: OutputDeviceDescriptor,
}

impl AudioStreamHandle {
    /// Close the real CPAL stream before joining its existing retirement worker.
    /// No capture owner may be taken over on the strength of an unsuccessful join.
    pub(super) fn close_checked(self) -> std::io::Result<()> {
        let Self {
            stream,
            _retirement_worker: retirement_worker,
            ..
        } = self;
        super::buffer_retirement::close_callback_and_join(stream, retirement_worker)
    }
}

/// The device builder failed after owning a callback, and its actual close
/// retirement could not be verified. This typed cause survives the pre-handle
/// startup boundary without inventing a CPAL failure bit or parsing messages.
#[derive(Debug)]
pub(super) struct UncertainStreamStartup {
    failed: Arc<AtomicBool>,
    build: Box<dyn std::error::Error>,
    cleanup: std::io::Error,
}

impl UncertainStreamStartup {
    pub(super) fn failure_signal(&self) -> Arc<AtomicBool> {
        self.failed.clone()
    }
}

impl std::fmt::Display for UncertainStreamStartup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}; {}", self.build, self.cleanup)
    }
}

impl std::error::Error for UncertainStreamStartup {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.build.as_ref())
    }
}

/// The builder took ownership of the supplied callback closure. On failure,
/// verify its actual disposal through the original checked close lane before
/// returning an ordinary startup error. Headless proofs reuse this exact seam.
fn finish_stream_build<T>(
    result: Result<T, Box<dyn std::error::Error>>,
    retirement_worker: AudioBufferRetirementWorker,
    failed: Arc<AtomicBool>,
) -> Result<(T, AudioBufferRetirementWorker), Box<dyn std::error::Error>> {
    match result {
        Ok(stream) => Ok((stream, retirement_worker)),
        Err(build) => match retirement_worker.join_checked() {
            Ok(()) => Err(build),
            Err(cleanup) => Err(Box::new(UncertainStreamStartup {
                failed,
                build,
                cleanup,
            })),
        },
    }
}

/// Strings are acquired before callback construction and never enter realtime work.
pub(crate) struct OutputDeviceDescriptor {
    pub(crate) host: String,
    pub(crate) endpoint_name: Option<String>,
    pub(crate) endpoint_description_error: Option<String>,
    pub(crate) default_sample_format: String,
}

/// Setup and configure the logger for audio operations
pub fn setup_logger() {
    // Default to `info` to avoid extremely expensive debug/trace logging during analysis.
    // Users can override via `RUST_LOG`, e.g. `RUST_LOG=debug` when troubleshooting.
    Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .try_init()
        .unwrap_or(()); // Ignore initialization errors
}

pub(super) trait AudioMessageSink {
    fn push_audio_message(&mut self, message: AudioMessage);
    fn available_audio_message_slots(&mut self) -> usize;

    /// An effect owns these tags already. Publication must never recapture current intent.
    fn push_captured_pad_feedback(
        &mut self,
        captured: Option<AudioMessage>,
        _diagnostic: AudioMessage,
    ) {
        if let Some(message) = captured {
            self.push_audio_message(message);
        }
    }
}

fn content_feedback_message(
    id: usize,
    owner: usize,
    key: super::content_placement::ContentKey,
    transport_revision: u64,
    kind: u8,
    value: f64,
) -> AudioMessage {
    AudioMessage::ContentFeedback {
        id,
        owner,
        cell: key.cell().index(),
        lifetime: key.lifetime(),
        transport_revision,
        kind,
        value,
    }
}

/// Inside an effect marker these scalar reads belong to that exact effect. Telemetry
/// uses the same bounded capture at observation and fails closed on a concurrent edit.
fn capture_pad_feedback(
    ownership: &super::input_runtime_binding::InputRuntimeOwnership,
    id: usize,
    kind: u8,
    value: f64,
) -> Option<AudioMessage> {
    if !ownership.is_source_tracked() {
        return Some(match kind {
            1 => AudioMessage::SampleStarted { id },
            2 => AudioMessage::SampleStopped { id },
            3 => AudioMessage::PadPeak {
                id,
                peak: value as f32,
            },
            4 => AudioMessage::PadPlayhead {
                id,
                position_s: value,
            },
            _ => return None,
        });
    }
    let key = ownership.callback_feedback_key(id)?;
    let transport = ownership.transport_intent_for_cell(key.cell());
    let revision = transport.load(Ordering::Acquire);
    (ownership.callback_feedback_key(id) == Some(key)
        && transport.load(Ordering::Acquire) == revision)
        .then(|| {
            content_feedback_message(
                id,
                ownership.owner_identity().diagnostic(),
                key,
                revision,
                kind,
                value,
            )
        })
}

impl AudioMessageSink for Producer<AudioMessage> {
    fn push_audio_message(&mut self, message: AudioMessage) {
        let _ = self.push(message);
    }

    fn available_audio_message_slots(&mut self) -> usize {
        self.slots()
    }
}

/// Allocated with the callback owner; tagging takes fixed atomic reads only.
struct ContentFeedbackProducer {
    producer: Producer<AudioMessage>,
    ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,
}

impl AudioMessageSink for ContentFeedbackProducer {
    fn push_audio_message(&mut self, message: AudioMessage) {
        let (id, kind, value) = match message {
            AudioMessage::SampleStarted { id } => (id, 1, 0.0),
            AudioMessage::SampleStopped { id } => (id, 2, 0.0),
            AudioMessage::PadPeak { id, peak } => (id, 3, f64::from(peak)),
            AudioMessage::PadPlayhead { id, position_s } => (id, 4, position_s),
            other => {
                let _ = self.producer.push(other);
                return;
            }
        };
        if let Some(message) = capture_pad_feedback(&self.ownership, id, kind, value) {
            let _ = self.producer.push(message);
        }
    }

    fn available_audio_message_slots(&mut self) -> usize {
        self.producer.slots()
    }
}

fn schedule_immediate_command<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    command: ScheduledCommand,
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    if !scheduler.is_full() {
        let _ = scheduler.schedule(callback_start_frame, command);
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
    } else {
        execute_due_event(
            scheduler,
            callback_start_frame,
            DueEvent {
                target_frame: callback_start_frame,
                execution_frame: callback_start_frame,
                was_late: false,
                command,
            },
            mixer,
            transport,
            audio_messages,
            retirement,
        );
    }
}

// Keep callback hot-path state borrows explicit instead of hiding them in a context struct.
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn schedule_play_sample_command<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &mut TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
    received_at_ns: Option<u64>,
) {
    schedule_play_sample_command_with_source(
        scheduler,
        callback_start_frame,
        trigger_quantization,
        transport,
        id,
        volume,
        mixer,
        audio_messages,
        retirement,
        received_at_ns,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn schedule_play_sample_command_with_source<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &mut TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
    received_at_ns: Option<u64>,
    expected_source_generation: Option<u64>,
) {
    if !mixer.expected_source_generation_current(id, expected_source_generation) {
        return;
    }
    let command = ScheduledCommand::PlaySample {
        id,
        volume,
        received_at_ns,
        expected_source_generation,
    };

    let Some(target_frame) = quantized_target_frame(transport, trigger_quantization) else {
        schedule_immediate_command(
            scheduler,
            callback_start_frame,
            command,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
        return;
    };

    if scheduler.schedule(target_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
    }
}

// Keep callback hot-path state borrows explicit instead of hiding them in a context struct.
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn schedule_exclusive_play_sample_command<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &mut TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
    received_at_ns: Option<u64>,
) {
    schedule_exclusive_play_sample_command_with_source(
        scheduler,
        callback_start_frame,
        trigger_quantization,
        transport,
        id,
        volume,
        mixer,
        audio_messages,
        retirement,
        received_at_ns,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn schedule_exclusive_play_sample_command_with_source<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: TriggerQuantization,
    transport: &mut TransportTimeline,
    id: usize,
    volume: f32,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
    received_at_ns: Option<u64>,
    expected_source_generation: Option<u64>,
) {
    if !mixer.expected_source_generation_current(id, expected_source_generation) {
        return;
    }
    let command = ScheduledCommand::StopAllThenPlaySample {
        id,
        volume,
        received_at_ns,
        expected_source_generation,
    };

    let Some(target_frame) = quantized_target_frame(transport, trigger_quantization) else {
        schedule_immediate_command(
            scheduler,
            callback_start_frame,
            command,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
        return;
    };

    if scheduler.schedule(target_frame, command).is_ok() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
    }
}

fn quantized_target_frame(
    transport: &TransportTimeline,
    trigger_quantization: TriggerQuantization,
) -> Option<u64> {
    match trigger_quantization {
        TriggerQuantization::Immediate => None,
        TriggerQuantization::Grid { step_64ths } => {
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(step_64ths)?)
        }
    }
}

fn anchor_transport_phase_from_pad(
    mixer: &RtMixer,
    transport: &mut TransportTimeline,
    id: usize,
) -> bool {
    anchor_transport_phase_from_pad_at_frame(mixer, transport, id, transport.output_frame())
}

fn anchor_transport_phase_from_pad_at_frame(
    mixer: &RtMixer,
    transport: &mut TransportTimeline,
    id: usize,
    output_frame: u64,
) -> bool {
    if mixer.has_mapped_voice() {
        return false;
    }
    let Some(source_beat) = mixer.active_pad_beat_position(id) else {
        return false;
    };
    let Some(period) = mixer.transport_reference_period_for_sample_id(id) else {
        return false;
    };

    let anchored = transport.set_master_period_and_anchor_beat_position_at_frame(
        period,
        source_beat,
        output_frame,
    );
    if anchored {
        transport.complete_bootstrap();
    }
    anchored
}

/// Called after both command/parameter batches and at render segment boundaries.
/// The source position and output frame therefore describe the same instant.
fn try_bootstrap_transport_from_reference(
    mixer: &RtMixer,
    transport: &mut TransportTimeline,
    output_frame: u64,
) -> bool {
    if mixer.has_pending_global_rate() || mixer.has_mapped_voice() {
        return false;
    }
    let Some(id) = transport.bootstrap_reference() else {
        return false;
    };
    let Some(source_beat) = mixer.active_pad_beat_position(id) else {
        return false;
    };
    let Some(period) = mixer.transport_reference_period_for_sample_id(id) else {
        return false;
    };
    transport.bootstrap_from_source_period_at_frame(period, source_beat, output_frame)
}

fn drain_scheduler_due_at_callback_start<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    while let Some(event) = scheduler.pop_due_at_callback_start(callback_start_frame) {
        if !execute_due_event(
            scheduler,
            callback_start_frame,
            event,
            mixer,
            transport,
            audio_messages,
            retirement,
        ) {
            break;
        }
    }
}

pub(super) enum ScheduledExecution {
    Complete,
    Pending(ScheduledCommand),
    Replanned {
        command: ScheduledCommand,
        frame: u64,
    },
}

enum CallbackEffectResult {
    Settled(bool),
    Pending,
}

// Feedback is published after the complete marker closes. The callback reserves its queue
// capacity before claim and captures only bounded scalar STOP events while applying effects.
struct StoppedFeedback {
    events: [Option<(Option<AudioMessage>, usize)>; MAX_VOICES],
    len: usize,
}

impl StoppedFeedback {
    fn new() -> Self {
        Self {
            events: std::array::from_fn(|_| None),
            len: 0,
        }
    }

    fn publish(&mut self, sink: &mut impl AudioMessageSink) {
        for event in &mut self.events[..self.len] {
            if let Some((captured, id)) = event.take() {
                sink.push_captured_pad_feedback(captured, AudioMessage::SampleStopped { id });
            }
        }
    }
}

impl AudioMessageSink for StoppedFeedback {
    fn push_audio_message(&mut self, message: AudioMessage) {
        let (captured, id) = match message {
            AudioMessage::ContentFeedback { id, kind: 2, .. } => (Some(message), id),
            // A diagnostic id alone must never acquire a later occupant's receipt.
            AudioMessage::SampleStopped { id } => (None, id),
            _ => return,
        };
        debug_assert!(self.len < MAX_VOICES);
        self.events[self.len] = Some((captured, id));
        self.len += 1;
    }

    fn push_captured_pad_feedback(
        &mut self,
        captured: Option<AudioMessage>,
        diagnostic: AudioMessage,
    ) {
        if let AudioMessage::SampleStopped { id } = diagnostic {
            debug_assert!(self.len < MAX_VOICES);
            self.events[self.len] = Some((captured, id));
            self.len += 1;
        }
    }

    fn available_audio_message_slots(&mut self) -> usize {
        MAX_VOICES - self.len
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_due_event<const CAPACITY: usize, S: AudioMessageSink, R: AudioBufferRetirement>(
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_frame: u64,
    event: DueEvent,
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) -> bool {
    let complete = match execute_scheduled_command(
        mixer,
        transport,
        event.execution_frame,
        event.command,
        audio_messages,
        retirement,
    ) {
        ScheduledExecution::Complete => true,
        ScheduledExecution::Pending(command) => {
            scheduler.defer_due(DueEvent { command, ..event }, callback_frame);
            false
        }
        ScheduledExecution::Replanned { command, frame } => {
            // The same original action retains its slot and sequence. Popping its
            // previous event reserved this bounded reinsertion capacity.
            let scheduled = scheduler.schedule(frame, command);
            debug_assert!(scheduled.is_ok());
            true
        }
    };
    if !complete {
        // A completed earlier event at S must leave the rest of that same-frame
        // ordering intact. Only Pending has prevented this due pass from reaching
        // the still-uncommitted group's own start/stop pair.
        mixer.settle_group_capture_stop_through(event.execution_frame);
    }
    mixer.service_mix_capture(scheduler, retirement);
    complete
}

fn execute_admitted_trigger<S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    output_frame: u64,
    command: &ScheduledCommand,
    audio_messages: &mut S,
    retirement: &mut R,
) -> CallbackEffectResult {
    let ScheduledCommand::TriggerInputPad {
        id,
        start_s,
        end_s,
        exclusive,
        binding,
        resident_control,
        launch_revision,
        ..
    } = command
    else {
        unreachable!("trigger command")
    };
    let ownership = mixer.input_ownership_for_effect();
    if ownership.placement_pending() {
        return CallbackEffectResult::Pending;
    }
    let Some(id) = trigger_binding_slot(
        mixer,
        *id,
        *binding,
        resident_control.as_ref(),
        *launch_revision,
    ) else {
        return CallbackEffectResult::Settled(false);
    };
    let retirement_needed =
        if *exclusive { 3 * MAX_VOICES } else { 3 } + mixer.mapped_owner_retirement_slots();
    let feedback_needed = if *exclusive {
        mixer.voices.iter().filter(|voice| voice.active).count() + 1
    } else {
        1
    };
    if retirement.available_retirement_slots() < retirement_needed
        || audio_messages.available_audio_message_slots() < feedback_needed
    {
        return CallbackEffectResult::Pending;
    }
    if !mixer.admitted_trigger_loop_context_available(id, *start_s, *end_s, *binding)
        || (!*exclusive
            && !mixer
                .voices
                .iter()
                .any(|voice| !voice.active || voice.sample_id == id))
    {
        return CallbackEffectResult::Settled(false);
    }
    let Some(prepared) =
        mixer.prepare_admitted_play_sample_rt(id, 1.0, output_frame, *binding, retirement)
    else {
        return if ownership.placement_pending() {
            CallbackEffectResult::Pending
        } else {
            CallbackEffectResult::Settled(false)
        };
    };
    let Some(snapshot) = ownership.slot_snapshot(id) else {
        return CallbackEffectResult::Pending;
    };
    let write = match ownership.prepare_slot_write_fixed(snapshot, snapshot.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending | PlacementError::Stale) => {
            return CallbackEffectResult::Pending;
        }
        Err(_) => return CallbackEffectResult::Settled(false),
    };
    let prepared = RefCell::new(Some(prepared));
    let mixer = RefCell::new(mixer);
    let mut stopped = StoppedFeedback::new();
    let mut started_feedback = None;
    #[cfg(test)]
    launch_callback_tests::before_claim();
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            let mixer = mixer.borrow();
            trigger_binding_slot(
                &mixer,
                id,
                *binding,
                resident_control.as_ref(),
                *launch_revision,
            ) == Some(id)
                && mixer.admit_prepared_source_birth(
                    prepared.borrow_mut().as_mut().unwrap(),
                    *binding,
                    resident_control.is_none(),
                )
        },
        |_| {
            let mut mixer = mixer.borrow_mut();
            let mut config = prepared.borrow_mut().take().unwrap();
            mixer.apply_prepared_loop_region(&mut config, *start_s, *end_s);
            if *exclusive {
                stop_all_samples(&mut mixer, &mut stopped, retirement);
            }
            let started = mixer.play_prepared_sample_rt(config, retirement);
            debug_assert!(started, "complete trigger capacity was prepared");
            started_feedback = capture_pad_feedback(&ownership, id, 1, 0.0);
        },
    );
    match result {
        Ok(_) => {
            #[cfg(test)]
            content_feedback_tests::after_effect();
            stopped.publish(audio_messages);
            audio_messages
                .push_captured_pad_feedback(started_feedback, AudioMessage::SampleStarted { id });
            CallbackEffectResult::Settled(true)
        }
        Err(PlacementError::Pending | PlacementError::Stale) => CallbackEffectResult::Pending,
        Err(_) => CallbackEffectResult::Settled(false),
    }
}

fn trigger_binding_slot(
    mixer: &RtMixer,
    id: usize,
    binding: super::input_runtime_binding::InputPadBinding,
    resident_control: Option<&super::resident_relocation::ResidentLaunchGuard>,
    launch_revision: u64,
) -> Option<usize> {
    let id = mixer.timing_effect_slot(id, binding.placement)?;
    if !mixer.admitted_launch_current(id, binding, launch_revision) {
        return None;
    }
    let current = match resident_control {
        Some(guard) => guard.current() && mixer.admitted_source_binding_current(id, binding),
        None => mixer.admitted_input_binding_current(id, binding),
    };
    current.then_some(id)
}

fn execute_admitted_stop<S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    placement: SlotSnapshot,
    audio_messages: &mut S,
    retirement: &mut R,
) -> CallbackEffectResult {
    let ownership = mixer.input_ownership_for_effect();
    if ownership.placement_pending() {
        return CallbackEffectResult::Pending;
    }
    let Some(id) = mixer.timing_effect_slot(placement.slot(), Some(placement)) else {
        return CallbackEffectResult::Settled(false);
    };
    if retirement.available_retirement_slots()
        < 3 * MAX_VOICES + mixer.mapped_owner_retirement_slots()
        || audio_messages.available_audio_message_slots() < 1
    {
        return CallbackEffectResult::Pending;
    }
    let Some(snapshot) = ownership.slot_snapshot(id) else {
        return CallbackEffectResult::Pending;
    };
    let write = match ownership.prepare_slot_write_fixed(snapshot, snapshot.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending | PlacementError::Stale) => {
            return CallbackEffectResult::Pending;
        }
        Err(_) => return CallbackEffectResult::Settled(false),
    };
    let mixer = RefCell::new(mixer);
    let mut stopped_feedback = None;
    #[cfg(test)]
    launch_callback_tests::before_claim();
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            mixer
                .borrow()
                .timing_effect_slot(placement.slot(), Some(placement))
                == Some(id)
        },
        |_| {
            stopped_feedback = capture_pad_feedback(&ownership, id, 2, 0.0);
            mixer.borrow_mut().stop_sample_rt(id, retirement);
        },
    );
    match result {
        Ok(_) => {
            #[cfg(test)]
            content_feedback_tests::after_effect();
            audio_messages
                .push_captured_pad_feedback(stopped_feedback, AudioMessage::SampleStopped { id });
            CallbackEffectResult::Settled(true)
        }
        Err(PlacementError::Pending | PlacementError::Stale) => CallbackEffectResult::Pending,
        Err(_) => CallbackEffectResult::Settled(false),
    }
}

pub(super) fn execute_scheduled_command<S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
    output_frame: u64,
    command: ScheduledCommand,
    audio_messages: &mut S,
    retirement: &mut R,
) -> ScheduledExecution {
    match command {
        ScheduledCommand::CaptureStart { session_id } => {
            mixer.execute_mix_capture_start(session_id, output_frame);
        }
        ScheduledCommand::CaptureStop { session_id } => {
            mixer.execute_mix_capture_stop(session_id, output_frame);
        }
        ScheduledCommand::PitchAction {
            slot,
            action_sequence,
        } => {
            return super::pitch_runtime::execute(
                mixer,
                slot,
                action_sequence,
                output_frame,
                Some(*transport),
                audio_messages,
                retirement,
            );
        }
        ScheduledCommand::RefreshAcceptedTiming(refresh) => {
            let accepted = execute_accepted_timing_refresh(
                mixer,
                transport,
                output_frame,
                &refresh,
                retirement,
            );
            refresh.finish(accepted);
            retirement.retire_accepted_timing_refresh(refresh);
        }
        ScheduledCommand::GlobalPlaybackBatch(batch) => {
            if retirement.available_retirement_slots() == 0 {
                // Even a genuinely stale event retains its final owner until the
                // retirement lane can accept the rejection payload.
                return ScheduledExecution::Pending(ScheduledCommand::GlobalPlaybackBatch(batch));
            }
            let result = execute_global_playback_batch(
                mixer,
                output_frame,
                &batch,
                audio_messages,
                retirement,
            );
            let CallbackEffectResult::Settled(accepted) = result else {
                return ScheduledExecution::Pending(ScheduledCommand::GlobalPlaybackBatch(batch));
            };
            batch.finish(accepted);
            retirement.retire_global_playback_batch(batch);
        }
        command @ ScheduledCommand::TriggerInputPad { .. } => {
            if matches!(
                execute_admitted_trigger(mixer, output_frame, &command, audio_messages, retirement),
                CallbackEffectResult::Pending
            ) {
                return ScheduledExecution::Pending(command);
            }
        }
        ScheduledCommand::PlaySample {
            id,
            volume,
            received_at_ns: _,
            expected_source_generation,
        } => {
            if !mixer.expected_source_generation_current(id, expected_source_generation) {
                return ScheduledExecution::Complete;
            }
            let ownership = mixer.input_ownership_for_effect();
            let started_feedback = capture_pad_feedback(&ownership, id, 1, 0.0);
            let stopped_feedback = capture_pad_feedback(&ownership, id, 2, 0.0);
            let started =
                mixer.play_sample_at_output_frame_rt(id, volume, output_frame, retirement);
            #[cfg(test)]
            content_feedback_tests::after_effect();
            if started {
                audio_messages.push_captured_pad_feedback(
                    started_feedback,
                    AudioMessage::SampleStarted { id },
                );
            } else {
                audio_messages.push_captured_pad_feedback(
                    stopped_feedback,
                    AudioMessage::SampleStopped { id },
                );
            }
        }
        ScheduledCommand::StopAllThenPlaySample {
            id,
            volume,
            received_at_ns: _,
            expected_source_generation,
        } => {
            if !mixer.expected_source_generation_current(id, expected_source_generation) {
                return ScheduledExecution::Complete;
            }
            if retirement.available_retirement_slots()
                < 3 * MAX_VOICES + mixer.mapped_owner_retirement_slots()
            {
                return ScheduledExecution::Complete;
            }
            let Some(prepared) = mixer.prepare_play_sample_rt(id, volume, output_frame, retirement)
            else {
                return ScheduledExecution::Complete;
            };
            let ownership = mixer.input_ownership_for_effect();
            let started_feedback = capture_pad_feedback(&ownership, id, 1, 0.0);
            stop_all_samples(mixer, audio_messages, retirement);
            let started = mixer.play_prepared_sample_rt(prepared, retirement);
            #[cfg(test)]
            content_feedback_tests::after_effect();
            if started {
                audio_messages.push_captured_pad_feedback(
                    started_feedback,
                    AudioMessage::SampleStarted { id },
                );
            }
        }
        #[cfg(test)]
        ScheduledCommand::StopSample { id } => {
            let ownership = mixer.input_ownership_for_effect();
            let stopped_feedback = capture_pad_feedback(&ownership, id, 2, 0.0);
            mixer.stop_sample_rt(id, retirement);
            content_feedback_tests::after_effect();
            audio_messages
                .push_captured_pad_feedback(stopped_feedback, AudioMessage::SampleStopped { id });
        }
        command @ ScheduledCommand::StopSampleForPlacement { placement } => {
            if matches!(
                execute_admitted_stop(mixer, placement, audio_messages, retirement),
                CallbackEffectResult::Pending
            ) {
                return ScheduledExecution::Pending(command);
            }
        }
        ScheduledCommand::StopAll => {
            stop_all_samples(mixer, audio_messages, retirement);
        }
    }
    ScheduledExecution::Complete
}

fn execute_accepted_timing_refresh<R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
    output_frame: u64,
    refresh: &super::accepted_timing_refresh::AcceptedTimingRefresh,
    retirement: &mut R,
) -> bool {
    if refresh.master_period_seconds.is_some()
        && (mixer.has_mapped_voice() || mixer.refuse_legacy_global_rate_if_pending())
    {
        return false;
    }
    if retirement.available_retirement_slots() < 1
        || !refresh.global_current()
        || !mixer.refresh_binding_current(refresh.id, refresh.binding)
        || !mixer.loop_intent_context_available(refresh.id, refresh.start_s, refresh.end_s)
    {
        return false;
    }
    let mut candidate = *transport;
    if let Some(period) = refresh.master_period_seconds {
        if !mixer.accepted_refresh_master_matches(refresh.binding, period)
            || !candidate.set_master_period_preserving_beat_position_at_frame(period, output_frame)
        {
            return false;
        }
        // This coupled master belongs to the acknowledged selected source. A still-pending
        // foreign reference cannot later anchor it from another pad's source beat position.
        // Completed one-time bootstrap remains complete; ordinary requests retain their policy.
        if let Some(previous) = candidate.bootstrap_reference() {
            candidate.clear_pending_bootstrap_for_pad(previous);
        }
        candidate.request_bootstrap(refresh.id);
    }
    // Recheck immediately before the first mutation; thereafter no fallible work remains.
    if !refresh.global_current() || !mixer.refresh_binding_current(refresh.id, refresh.binding) {
        return false;
    }
    mixer.set_pad_loop_region(refresh.id, refresh.start_s, refresh.end_s);
    if let Some(period) = refresh.master_period_seconds {
        *transport = candidate;
        mixer.set_master_period(period);
    }
    true
}

pub(super) fn global_playback_entry_slot(
    mixer: &RtMixer,
    entry: &super::global_playback_batch::GlobalPlaybackEntry,
    start: bool,
) -> Option<usize> {
    let id = mixer.timing_effect_slot(entry.id, entry.binding.placement)?;
    (mixer.admitted_source_binding_current(id, entry.binding)
        && (!start || mixer.admitted_launch_current(id, entry.binding, entry.launch_revision)))
    .then_some(id)
}

fn execute_global_playback_batch<S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    output_frame: u64,
    batch: &super::global_playback_batch::GlobalPlaybackBatch,
    audio_messages: &mut S,
    retirement: &mut R,
) -> CallbackEffectResult {
    // Captured input time is diagnostic context; transport owns execution quantization.
    let _received_at_ns = batch.received_at_ns;
    let ownership = mixer.input_ownership_for_effect();
    if ownership.placement_pending() {
        return CallbackEffectResult::Pending;
    }
    if batch.entries.len() > NUM_SAMPLES || (batch.start && batch.entries.len() > MAX_VOICES) {
        return CallbackEffectResult::Settled(false);
    }
    let mut resolved = [usize::MAX; NUM_SAMPLES];
    for (index, entry) in batch.entries.iter().enumerate() {
        let Some(id) = global_playback_entry_slot(mixer, entry, batch.start) else {
            return CallbackEffectResult::Settled(false);
        };
        resolved[index] = id;
    }
    // One fixed retirement reservation covers every old pin plus this immutable payload.
    let retirement_slots = retirement.available_retirement_slots();
    if retirement_slots < 3 * MAX_VOICES + mixer.mapped_owner_retirement_slots() + 1 {
        // Preserve the existing atomic capacity rejection. Only an unavailable
        // payload-retirement slot must retain the event to avoid a final RT drop.
        return if retirement_slots == 0 {
            CallbackEffectResult::Pending
        } else {
            CallbackEffectResult::Settled(false)
        };
    }
    let feedback_needed = if batch.start {
        batch.entries.len()
    } else {
        mixer.voices.iter().filter(|voice| voice.active).count()
    };
    // The callback is the only writer. The consumer can only increase this capacity
    // until all transaction feedback is published, so no accepted effect loses its state events.
    if audio_messages.available_audio_message_slots() < feedback_needed {
        return CallbackEffectResult::Settled(false);
    }
    if !batch.start && !mixer.global_stop_bindings_current(&batch.entries) {
        return CallbackEffectResult::Settled(false);
    }
    if batch.start
        && !batch.entries.iter().enumerate().all(|(index, entry)| {
            mixer.loop_intent_context_available(resolved[index], entry.start_s, entry.end_s)
        })
    {
        return CallbackEffectResult::Settled(false);
    }
    let free = mixer.voices.iter().filter(|voice| !voice.active).count();
    let new = batch
        .entries
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            !mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == resolved[*index])
        })
        .count();
    if batch.start && new > free {
        return CallbackEffectResult::Settled(false);
    }
    // Preparing clones immutable PCM pins and fixed configs only; no active history mutates.
    let mut prepared: [Option<super::voice_slot::VoiceStartConfig>; MAX_VOICES] =
        std::array::from_fn(|_| None);
    if batch.start {
        for (index, entry) in batch.entries.iter().enumerate() {
            let Some(config) = mixer.prepare_admitted_play_sample_rt(
                resolved[index],
                1.0,
                output_frame,
                entry.binding,
                retirement,
            ) else {
                return CallbackEffectResult::Settled(false);
            };
            prepared[index] = Some(config);
        }
    }
    // The complete original-source guard is rechecked under the one mutation marker below.
    let anchor = if batch.entries.is_empty() {
        0
    } else {
        resolved[0]
    };
    let Some(snapshot) = ownership.slot_snapshot(anchor) else {
        return CallbackEffectResult::Pending;
    };
    let write = match ownership.prepare_slot_write_fixed(snapshot, snapshot.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending | PlacementError::Stale) => {
            return CallbackEffectResult::Pending;
        }
        Err(_) => return CallbackEffectResult::Settled(false),
    };
    let prepared = RefCell::new(prepared);
    let mixer = RefCell::new(mixer);
    let mut stopped = StoppedFeedback::new();
    let mut started_feedback: [Option<AudioMessage>; MAX_VOICES] = std::array::from_fn(|_| None);
    #[cfg(test)]
    launch_callback_tests::before_claim();
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            let mixer = mixer.borrow();
            if !batch.entries.iter().enumerate().all(|(index, entry)| {
                global_playback_entry_slot(&mixer, entry, batch.start) == Some(resolved[index])
            }) {
                return false;
            }
            if !batch.start {
                return mixer.global_stop_bindings_current(&batch.entries);
            }
            let mut prepared = prepared.borrow_mut();
            batch.entries.iter().enumerate().all(|(index, entry)| {
                mixer.admit_prepared_source_birth(
                    prepared[index].as_mut().unwrap(),
                    entry.binding,
                    false,
                )
            })
        },
        |_| {
            let mut mixer = mixer.borrow_mut();
            if !batch.start {
                stop_all_samples(&mut mixer, &mut stopped, retirement);
                return;
            }
            let mut prepared = prepared.borrow_mut();
            for (index, entry) in batch.entries.iter().enumerate() {
                let mut config = prepared[index]
                    .take()
                    .expect("validated prepared batch entry");
                mixer.apply_prepared_loop_region(&mut config, entry.start_s, entry.end_s);
                let started = mixer.play_prepared_sample_rt(config, retirement);
                debug_assert!(
                    started,
                    "capacity was reserved for the complete global batch"
                );
                started_feedback[index] = capture_pad_feedback(&ownership, resolved[index], 1, 0.0);
            }
        },
    );
    match result {
        Ok(_) => {
            #[cfg(test)]
            content_feedback_tests::after_effect();
            stopped.publish(audio_messages);
            if batch.start {
                for (index, id) in resolved[..batch.entries.len()].iter().enumerate() {
                    audio_messages.push_captured_pad_feedback(
                        started_feedback[index].take(),
                        AudioMessage::SampleStarted { id: *id },
                    );
                }
            }
            CallbackEffectResult::Settled(true)
        }
        Err(PlacementError::Pending | PlacementError::Stale) => CallbackEffectResult::Pending,
        Err(_) => CallbackEffectResult::Settled(false),
    }
}

fn stop_all_samples<S: AudioMessageSink, R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    for index in 0..MAX_VOICES {
        if mixer.voices[index].active {
            let id = mixer.voices[index].sample_id;
            let ownership = mixer.input_ownership_for_effect();
            let stopped_feedback = capture_pad_feedback(&ownership, id, 2, 0.0);
            mixer.stop_sample_rt(id, retirement);
            audio_messages
                .push_captured_pad_feedback(stopped_feedback, AudioMessage::SampleStopped { id });
        }
    }
}

// Keep scheduler, transport, mixer, output, and retirement ownership visible in the render path.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn render_scheduled_audio<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    mixer: &mut RtMixer,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    callback_start_frame: u64,
    channels: usize,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    let mut pad_activity = RtRenderPadActivity::default();
    render_scheduled_audio_tracking_pads(
        mixer,
        scheduler,
        output,
        pad_peaks,
        &mut pad_activity,
        callback_start_frame,
        channels,
        transport,
        audio_messages,
        retirement,
    );
}

// Keep scheduler, transport, mixer, output, telemetry, and retirement ownership visible.
#[allow(clippy::too_many_arguments)]
fn render_scheduled_audio_tracking_pads<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    mixer: &mut RtMixer,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    pad_activity: &mut RtRenderPadActivity,
    callback_start_frame: u64,
    channels: usize,
    transport: &mut TransportTimeline,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    output.fill(0.0);
    pad_peaks.fill(0.0);
    pad_activity.clear();

    mixer.service_mix_capture(scheduler, retirement);
    super::pitch_runtime::service_endpoint_reserve(mixer);

    drain_scheduler_due_at_callback_start(
        scheduler,
        callback_start_frame,
        mixer,
        transport,
        audio_messages,
        retirement,
    );
    mixer.settle_group_capture_stop_through(callback_start_frame);
    mixer.service_mix_capture(scheduler, retirement);

    if scheduler.deferred_due().is_none() {
        mixer.service_global_rate(callback_start_frame, transport, retirement);
    }

    if channels == 0 {
        return;
    }

    let frames = output.len() / channels;
    if frames == 0 {
        return;
    }

    let callback_end_frame = callback_start_frame.saturating_add(frames as u64);
    let mut rendered_until_frame = callback_start_frame;
    let mut segment_peaks = [0.0_f32; NUM_SAMPLES];

    while rendered_until_frame < callback_end_frame {
        try_bootstrap_transport_from_reference(mixer, transport, rendered_until_frame);
        let next_target_frame = scheduler
            .peek_next_target_frame()
            .unwrap_or(callback_end_frame)
            .min(mixer.next_global_rate_frame().unwrap_or(callback_end_frame));

        if next_target_frame >= callback_end_frame {
            render_mixer_segment(
                mixer,
                output,
                pad_peaks,
                &mut segment_peaks,
                pad_activity,
                callback_start_frame,
                rendered_until_frame,
                callback_end_frame,
                channels,
                retirement,
            );
            mixer.service_mix_capture(scheduler, retirement);
            break;
        }

        if next_target_frame > rendered_until_frame {
            render_mixer_segment(
                mixer,
                output,
                pad_peaks,
                &mut segment_peaks,
                pad_activity,
                callback_start_frame,
                rendered_until_frame,
                next_target_frame,
                channels,
                retirement,
            );
            mixer.service_mix_capture(scheduler, retirement);
            rendered_until_frame = next_target_frame;
        }

        while let Some(event) =
            scheduler.pop_due_through(callback_start_frame, rendered_until_frame)
        {
            if !execute_due_event(
                scheduler,
                callback_start_frame,
                event,
                mixer,
                transport,
                audio_messages,
                retirement,
            ) {
                break;
            }
        }
        if scheduler.deferred_due().is_some() {
            render_mixer_segment(
                mixer,
                output,
                pad_peaks,
                &mut segment_peaks,
                pad_activity,
                callback_start_frame,
                rendered_until_frame,
                callback_end_frame,
                channels,
                retirement,
            );
            // The rendered interval is half-open. A boundary at the next
            // callback remains eligible for that callback's ordinary due pass.
            mixer.settle_group_capture_stop_through(callback_end_frame - 1);
            mixer.service_mix_capture(scheduler, retirement);
            break;
        }
        mixer.service_global_rate(rendered_until_frame, transport, retirement);
    }
    mixer.finish_key_lock_start_callback(
        scheduler.peek_next_target_frame().is_none() && scheduler.deferred_due().is_none(),
    );
    mixer.service_mix_capture(scheduler, retirement);
}

// Keep segment frame bounds and realtime state explicit for in-buffer scheduling tests.
#[allow(clippy::too_many_arguments)]
fn render_mixer_segment<R: AudioBufferRetirement>(
    mixer: &mut RtMixer,
    output: &mut [f32],
    pad_peaks: &mut [f32; NUM_SAMPLES],
    segment_peaks: &mut [f32; NUM_SAMPLES],
    pad_activity: &mut RtRenderPadActivity,
    callback_start_frame: u64,
    segment_start_frame: u64,
    segment_end_frame: u64,
    channels: usize,
    retirement: &mut R,
) {
    if segment_end_frame <= segment_start_frame {
        return;
    }

    let start_frame = (segment_start_frame - callback_start_frame) as usize;
    let end_frame = (segment_end_frame - callback_start_frame) as usize;
    let start = start_frame * channels;
    let end = end_frame * channels;

    mixer.render_rt_at_output_frame_tracking_pads(
        &mut output[start..end],
        segment_peaks,
        segment_start_frame,
        pad_activity,
        retirement,
    );
    mixer.append_mix_capture(segment_start_frame, &output[start..end]);

    for id in pad_activity.iter() {
        pad_peaks[id] = pad_peaks[id].max(segment_peaks[id]);
    }
}

fn master_output_peak(output: &[f32]) -> f32 {
    output.iter().fold(0.0_f32, |peak, sample| {
        if sample.is_finite() {
            peak.max(sample.abs())
        } else {
            peak
        }
    })
}

fn publish_master_peak_telemetry<S: AudioMessageSink>(
    audio_messages: &mut S,
    master_peak: f32,
    frame_clock: u64,
    emit_interval_frames: u64,
    last_master_emit_frame: &mut u64,
) {
    if frame_clock.wrapping_sub(*last_master_emit_frame) < emit_interval_frames {
        return;
    }

    *last_master_emit_frame = frame_clock;

    if master_peak > 0.0 && master_peak.is_finite() {
        audio_messages.push_audio_message(AudioMessage::MasterPeak { peak: master_peak });
    }
}

fn publish_pad_telemetry<S: AudioMessageSink>(
    audio_messages: &mut S,
    mixer: &RtMixer,
    pad_peaks: &[f32; NUM_SAMPLES],
    pad_activity: &RtRenderPadActivity,
    frame_clock: u64,
    emit_interval_frames: u64,
    last_pad_emit_frame: &mut u64,
) {
    if frame_clock.wrapping_sub(*last_pad_emit_frame) < emit_interval_frames {
        return;
    }

    *last_pad_emit_frame = frame_clock;

    for id in pad_activity.iter() {
        let ownership = mixer.input_ownership_for_effect();
        let peak = pad_peaks[id];
        if peak > 0.0 && peak.is_finite() {
            let peak = peak.clamp(0.0, 1.0);
            let observed = capture_pad_feedback(&ownership, id, 3, f64::from(peak));
            #[cfg(test)]
            content_feedback_tests::after_effect();
            audio_messages.push_captured_pad_feedback(observed, AudioMessage::PadPeak { id, peak });
        }

        if let Some(position_s) = mixer.pad_playhead_seconds(id)
            && position_s.is_finite()
        {
            let observed = capture_pad_feedback(&ownership, id, 4, position_s);
            #[cfg(test)]
            content_feedback_tests::after_effect();
            audio_messages
                .push_captured_pad_feedback(observed, AudioMessage::PadPlayhead { id, position_s });
        }
    }
}

fn control_message_retirement_slots_needed(
    message: &ControlMessage,
    mapped_owners: usize,
) -> usize {
    match message {
        ControlMessage::StartMixCapture { .. } => 2,
        ControlMessage::GlobalPlaybackBatch(batch) => {
            3 * MAX_VOICES + mapped_owners + 1 + usize::from(batch.capture_publication.is_some())
        }
        ControlMessage::RelocateResident(_) => 3 * MAX_VOICES + 3,
        ControlMessage::CaptureResidentSeek(_) => 3,
        ControlMessage::CommittedStemWork(_) => 3,
        ControlMessage::PadCopy(work) => work.retirement_slots_needed() + mapped_owners,
        ControlMessage::BankMutation(_) => {
            3 * MAX_VOICES + mapped_owners + super::constants::NUM_PADS + 1
        }
        ControlMessage::VerifyRecoveredBank(_) => 1,
        ControlMessage::PadPair(_) => 1,
        ControlMessage::HoldAction(_) => 2,
        ControlMessage::PitchAction(_) => 2,
        ControlMessage::PreparedStart(_) => 2,
        ControlMessage::CapturePadStartTarget(_) => 1,
        ControlMessage::CaptureGlobalStartTarget(_) => 1,
        ControlMessage::TransportForPlacement { .. } => 1,
        ControlMessage::LoadColdSample {
            replace_assignment: true,
            ..
        } => 3 * MAX_VOICES + mapped_owners + 5,
        ControlMessage::LoadColdSample {
            replace_assignment: false,
            ..
        } => 2 * MAX_VOICES + 5,
        ControlMessage::LoadSample { .. } => 2 * MAX_VOICES + 3,
        ControlMessage::PublishPreparedStems { .. } => 2,
        ControlMessage::SetStemPairFullMix { .. } => 3,
        ControlMessage::PublishConstantTiming { .. } | ControlMessage::RefreshAcceptedTiming(_) => {
            1
        }
        ControlMessage::PlaySample { .. }
        | ControlMessage::TriggerInputPad {
            exclusive: false, ..
        } => 3 + mapped_owners,
        #[cfg(test)]
        ControlMessage::StopSample { .. } => 3 * MAX_VOICES + mapped_owners,
        ControlMessage::StopSampleForPlacement { .. } => 3 * MAX_VOICES + mapped_owners,
        ControlMessage::UnloadSample { .. } => 3 * MAX_VOICES + mapped_owners + 3,
        ControlMessage::StopAll()
        | ControlMessage::PlaySampleExclusive { .. }
        | ControlMessage::TriggerInputPad {
            exclusive: true, ..
        } => 3 * MAX_VOICES + mapped_owners,
        _ => 0,
    }
}

fn apply_hold_action_message(
    mixer: &mut RtMixer,
    work: Box<super::hold_action::HoldActionRtWork>,
    output_frame: u64,
    transport: &TransportTimeline,
    retirement: &mut impl AudioBufferRetirement,
) {
    use super::hold_action::{HoldActionState, HoldWorkKind};
    let ownership = mixer.input_ownership_for_effect();
    if retirement.available_retirement_slots() < 2 || ownership.placement_pending() {
        mixer.defer_control(ControlMessage::HoldAction(work));
        return;
    }
    if !Arc::ptr_eq(&ownership, work.action.ownership()) {
        work.action.settle(HoldActionState::Rejected, output_frame);
        retirement.retire_hold_action(work);
        return;
    }
    let key = work.action.key();
    let Some(id) = ownership.callback_key_slot(key) else {
        mixer.retire_content_actions_rt(key, HoldActionState::Retired, output_frame, retirement);
        work.action.settle(HoldActionState::Retired, output_frame);
        retirement.retire_hold_action(work);
        return;
    };
    let Some(placement) = ownership.slot_snapshot(id) else {
        mixer.defer_control(ControlMessage::HoldAction(work));
        return;
    };
    let Ok(write) = ownership.prepare_slot_write_fixed(placement, placement.lifetime()) else {
        mixer.defer_control(ControlMessage::HoldAction(work));
        return;
    };
    let mut pending = Some(work);
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            ownership.slot_snapshot_fields_current(placement)
                && placement.content_key() == Some(key)
        },
        |_| {
            let work = pending.take().unwrap();
            match work.kind {
                HoldWorkKind::Pause => {
                    mixer.apply_pause_hold_with_timeline_rt(
                        work,
                        output_frame,
                        Some(transport),
                        retirement,
                    );
                }
                HoldWorkKind::Release => {
                    mixer.apply_release_hold_with_timeline_rt(
                        work,
                        output_frame,
                        Some(transport),
                        retirement,
                    );
                }
            }
        },
    );
    if result.is_err()
        && let Some(work) = pending
    {
        // The exact owner retries under the next current placement; no resynthesized action.
        mixer.defer_control(ControlMessage::HoldAction(work));
    }
}

fn same_parameter_participant(
    old_id: usize,
    old: Option<SlotSnapshot>,
    id: usize,
    current: Option<SlotSnapshot>,
) -> bool {
    match (old, current) {
        (Some(old), Some(current)) => match (old.content_key(), current.content_key()) {
            (Some(old), Some(current)) => old == current,
            (None, None) => old.same_projection(current),
            _ => false,
        },
        (None, None) => old_id == id,
        _ => false,
    }
}

fn apply_transport_content_message(
    mixer: &mut RtMixer,
    command: ControlMessage,
    output_frame: u64,
    transport: &TransportTimeline,
    audio_messages: &mut impl AudioMessageSink,
    retirement: &mut impl AudioBufferRetirement,
) {
    let ControlMessage::TransportForPlacement {
        placement: original,
        transport_revision: revision,
        source_epoch,
        effect,
    } = command
    else {
        return;
    };
    let ownership = mixer.input_ownership_for_effect();
    if revision == 0 || original.owner() != ownership.owner_identity() {
        return;
    }
    if retirement.available_retirement_slots() == 0
        || ownership.placement_pending()
        || audio_messages.available_audio_message_slots() == 0
    {
        mixer.defer_control(ControlMessage::TransportForPlacement {
            placement: original,
            transport_revision: revision,
            source_epoch,
            effect,
        });
        return;
    }
    let key = original.content_key();
    let target = match key {
        Some(key) => ownership.callback_key_slot(key),
        None => ownership
            .slot_snapshot_fields_current(original)
            .then_some(original.slot()),
    };
    let Some(target) = target else {
        return;
    };
    if ownership
        .transport_intent_for_cell(original.cell())
        .load(Ordering::Acquire)
        < revision
    {
        return;
    }
    let Some(current) = ownership.slot_snapshot(target) else {
        mixer.defer_control(ControlMessage::TransportForPlacement {
            placement: original,
            transport_revision: revision,
            source_epoch,
            effect,
        });
        return;
    };
    let Ok(write) = ownership.prepare_slot_write_fixed(current, current.lifetime()) else {
        mixer.defer_control(ControlMessage::TransportForPlacement {
            placement: original,
            transport_revision: revision,
            source_epoch,
            effect,
        });
        return;
    };
    let mut completed = None;
    let result = ownership.commit_slot_write_fixed(
        &write,
        || {
            ownership.slot_snapshot_fields_current(current)
                && current.cell() == original.cell()
                && current.content_key() == key
                && ownership
                    .transport_intent_for_cell(original.cell())
                    .load(Ordering::Acquire)
                    >= revision
        },
        |_| {
            if let Some(key) = key {
                // Preflight next publication before any trajectory or Hold-cause
                // mutation. Terminal unavailability is an explicit query/admission
                // error; this queued control must never report false success.
                if ownership.ordinary_available(original.cell()) {
                    if ownership.ordinary_source_epoch(original.cell()) == Some(source_epoch) {
                        let (outcome, active, paused) = mixer
                            .apply_transport_content_with_timeline_rt(
                                key,
                                effect,
                                output_frame,
                                Some(transport),
                                Some(revision),
                                retirement,
                            );
                        completed = ownership.publish_ordinary_effect(
                            key, revision, effect, outcome, active, paused,
                        );
                    } else if mixer.consume_ordinary_hold_cause_rt(
                        key,
                        revision,
                        output_frame,
                        retirement,
                    ) {
                        completed = ownership.publish_ordinary_effect_for_source(
                            key,
                            source_epoch,
                            revision,
                            effect,
                            super::ordinary_transport::REFUSED,
                            true,
                            true,
                        );
                    }
                }
                ownership.finish_pending_ordinary_origin(key, revision);
            }
        },
    );
    if result.is_ok() {
        #[cfg(test)]
        content_feedback_tests::after_effect();
        if let Some(completed) = completed {
            audio_messages.push_audio_message(completed);
        }
    }
    if matches!(
        result,
        Err(super::content_placement::PlacementError::Pending)
            | Err(super::content_placement::PlacementError::Stale)
    ) {
        mixer.defer_control(ControlMessage::TransportForPlacement {
            placement: original,
            transport_revision: revision,
            source_epoch,
            effect,
        });
    }
}

#[cfg(test)]
#[path = "constant_timing_callback_tests.rs"]
mod constant_timing_callback_tests;

// Keep queue, scheduler, transport, mixer, telemetry, and retirement state explicit in the callback.
#[allow(clippy::too_many_arguments)]
pub(super) fn drain_control_messages<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    consumer: &mut Consumer<ControlMessage>,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: &mut TriggerQuantization,
    transport: &mut TransportTimeline,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
) -> usize {
    mixer.service_mix_capture(scheduler, retirement);
    let drain_revision = mixer.begin_global_timing_drain();
    mixer.begin_key_lock_start_control_drain();
    mixer.set_global_commands_drained(false);
    let mut processed = 0;

    // An already due payload gets its one bounded retry before later queued effects.
    if scheduler.deferred_due().is_some() {
        drain_scheduler_due_at_callback_start(
            scheduler,
            callback_start_frame,
            mixer,
            transport,
            audio_messages,
            retirement,
        );
        if scheduler.deferred_due().is_some() {
            return 0;
        }
    }

    while processed < MAX_CONTROL_MESSAGES_PER_CALLBACK {
        if scheduler.deferred_due().is_some() {
            break;
        }
        if mixer.placement_pending() {
            break;
        }
        let next = mixer.deferred_control().or_else(|| consumer.peek().ok());
        let (needed_retirement_slots, needed_feedback_slots) = match next {
            Some(message) => {
                let source = match message {
                    ControlMessage::LoadSample { publication, .. }
                    | ControlMessage::UnloadSample { publication, .. } => publication.as_ref(),
                    ControlMessage::LoadColdSample { publication, .. } => Some(publication),
                    _ => None,
                };
                if source.is_some_and(|source| mixer.source_publication_pending(source)) {
                    break;
                }
                (
                    control_message_retirement_slots_needed(
                        message,
                        mixer.mapped_owner_retirement_slots(),
                    ) + usize::from(source.is_some()),
                    usize::from(matches!(
                        message,
                        ControlMessage::TransportForPlacement { .. }
                            | ControlMessage::UnloadSample { .. }
                            | ControlMessage::LoadColdSample {
                                replace_assignment: true,
                                ..
                            }
                    )),
                )
            }
            None => break,
        };

        if (needed_retirement_slots > 0
            && retirement.available_retirement_slots() < needed_retirement_slots)
            || (needed_feedback_slots > 0
                && audio_messages.available_audio_message_slots() < needed_feedback_slots)
        {
            break;
        }

        let message = if let Some(message) = mixer.take_deferred_control() {
            message
        } else {
            let Ok(message) = consumer.pop() else {
                break;
            };
            message
        };

        process_control_message(
            message,
            scheduler,
            callback_start_frame,
            trigger_quantization,
            transport,
            mixer,
            audio_messages,
            retirement,
        );
        if mixer.deferred_control().is_some() {
            break;
        }
        processed += 1;
    }

    mixer.finish_global_control_drain(
        drain_revision,
        consumer.is_empty()
            && mixer.deferred_control().is_none()
            && scheduler.deferred_due().is_none(),
    );
    mixer.finish_key_lock_start_control_drain(
        consumer.is_empty()
            && mixer.deferred_control().is_none()
            && scheduler.deferred_due().is_none(),
    );
    processed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ParameterDrainResult {
    messages_drained: usize,
    parameters_applied: usize,
}

#[derive(Debug, Clone, Copy)]
struct PendingPadBpm {
    placement: Option<SlotSnapshot>,
    id: usize,
    bpm: Option<f64>,
    through_epoch: Option<u64>,
    expected_source_generation: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct PendingPadGain {
    placement: Option<SlotSnapshot>,
    id: usize,
    gain_db: f32,
    expected_epoch: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct PendingPadEq {
    placement: Option<SlotSnapshot>,
    id: usize,
    low_db: f32,
    mid_db: f32,
    high_db: f32,
    expected_epoch: Option<u64>,
}

struct PendingControlParameters {
    volume: Option<f32>,
    rate_revision: u64,
    speed: Option<f64>,
    master_period_seconds: Option<f64>,
    pad_bpm: [PendingPadBpm; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
    pad_bpm_count: usize,
    pad_gain: [PendingPadGain; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
    pad_gain_count: usize,
    pad_eq: [PendingPadEq; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
    pad_eq_count: usize,
}

impl Default for PendingControlParameters {
    fn default() -> Self {
        Self {
            volume: None,
            rate_revision: 0,
            speed: None,
            master_period_seconds: None,
            pad_bpm: [PendingPadBpm {
                placement: None,
                id: 0,
                bpm: None,
                through_epoch: None,
                expected_source_generation: None,
            }; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
            pad_bpm_count: 0,
            pad_gain: [PendingPadGain {
                placement: None,
                id: 0,
                gain_db: 0.0,
                expected_epoch: None,
            }; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
            pad_gain_count: 0,
            pad_eq: [PendingPadEq {
                placement: None,
                id: 0,
                low_db: 0.0,
                mid_db: 0.0,
                high_db: 0.0,
                expected_epoch: None,
            }; MAX_PARAMETER_MESSAGES_PER_CALLBACK],
            pad_eq_count: 0,
        }
    }
}

impl PendingControlParameters {
    fn record(&mut self, message: ControlParameterMessage, mixer: &RtMixer) {
        match message {
            ControlParameterMessage::SetVolume(volume) => self.volume = Some(volume),
            ControlParameterMessage::SetRateIntent {
                revision,
                speed,
                period_seconds,
            } => {
                self.rate_revision = revision;
                if let Some(speed) = speed {
                    self.speed = Some(speed);
                }
                if let Some(period) = period_seconds {
                    self.master_period_seconds = Some(period);
                }
            }
            #[cfg(test)]
            ControlParameterMessage::SetSpeed(speed) => self.speed = Some(speed),
            #[cfg(test)]
            ControlParameterMessage::SetMasterBpm(bpm) => {
                self.master_period_seconds = Some(60.0 / bpm);
            }
            #[cfg(test)]
            ControlParameterMessage::SetMasterPeriod(period) => {
                self.master_period_seconds = Some(period);
            }
            #[cfg(test)]
            ControlParameterMessage::SetSpeedAndMasterPeriod {
                speed,
                period_seconds,
            } => {
                self.speed = Some(speed);
                self.master_period_seconds = Some(period_seconds);
            }
            #[cfg(test)]
            ControlParameterMessage::SetPadBpm { id, bpm } => {
                self.record_pad_bpm(id, bpm, None, None, None);
            }
            ControlParameterMessage::SetLegacyPadBpm {
                placement,
                id,
                bpm,
                through_epoch,
                expected_source_generation,
            } => {
                // Reject before coalescing: an old gesture must not replace a
                // valid pending edit for the newly adopted source in this pad.
                if let Some(target) = mixer.timing_effect_slot(id, placement)
                    && mixer.expected_source_generation_current(target, expected_source_generation)
                    && mixer.legacy_parameter_after_copy(target, through_epoch)
                {
                    self.record_pad_bpm(
                        id,
                        bpm,
                        Some(through_epoch),
                        expected_source_generation,
                        placement,
                    );
                }
            }
            #[cfg(test)]
            ControlParameterMessage::SetPadGain { id, gain_db } => {
                self.record_pad_gain(id, gain_db, None, None);
            }
            ControlParameterMessage::SetPadGainGuarded {
                placement,
                id,
                gain_db,
                expected_epoch,
            } => {
                if let Some(target) = mixer.timing_effect_slot(id, Some(placement))
                    && mixer.pad_parameter_epoch_current(target, expected_epoch)
                {
                    self.record_pad_gain(id, gain_db, Some(expected_epoch), Some(placement));
                }
            }
            #[cfg(test)]
            ControlParameterMessage::SetPadEq {
                id,
                low_db,
                mid_db,
                high_db,
            } => {
                self.record_pad_eq(id, low_db, mid_db, high_db, None, None);
            }
            ControlParameterMessage::SetPadEqGuarded {
                placement,
                id,
                low_db,
                mid_db,
                high_db,
                expected_epoch,
            } => {
                if let Some(target) = mixer.timing_effect_slot(id, Some(placement))
                    && mixer.pad_parameter_epoch_current(target, expected_epoch)
                {
                    self.record_pad_eq(
                        id,
                        low_db,
                        mid_db,
                        high_db,
                        Some(expected_epoch),
                        Some(placement),
                    );
                }
            }
        }
    }

    fn record_pad_bpm(
        &mut self,
        id: usize,
        bpm: Option<f64>,
        through_epoch: Option<u64>,
        expected_source_generation: Option<u64>,
        placement: Option<SlotSnapshot>,
    ) {
        if id >= NUM_SAMPLES {
            return;
        }
        if let Some(pending) = self.pad_bpm[..self.pad_bpm_count]
            .iter_mut()
            .find(|pending| pending.id == id && pending.placement == placement)
        {
            pending.bpm = bpm;
            pending.through_epoch = through_epoch;
            pending.expected_source_generation = expected_source_generation;
            return;
        }
        if self.pad_bpm_count < self.pad_bpm.len() {
            self.pad_bpm[self.pad_bpm_count] = PendingPadBpm {
                placement,
                id,
                bpm,
                through_epoch,
                expected_source_generation,
            };
            self.pad_bpm_count += 1;
        }
    }

    fn record_pad_gain(
        &mut self,
        id: usize,
        gain_db: f32,
        expected_epoch: Option<u64>,
        placement: Option<SlotSnapshot>,
    ) {
        if id >= NUM_SAMPLES {
            return;
        }
        if let Some(pending) = self.pad_gain[..self.pad_gain_count]
            .iter_mut()
            .find(|pending| {
                same_parameter_participant(pending.id, pending.placement, id, placement)
                    && pending.expected_epoch == expected_epoch
            })
        {
            pending.gain_db = gain_db;
            pending.expected_epoch = expected_epoch;
            return;
        }
        if self.pad_gain_count < self.pad_gain.len() {
            self.pad_gain[self.pad_gain_count] = PendingPadGain {
                placement,
                id,
                gain_db,
                expected_epoch,
            };
            self.pad_gain_count += 1;
        }
    }

    fn record_pad_eq(
        &mut self,
        id: usize,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
        expected_epoch: Option<u64>,
        placement: Option<SlotSnapshot>,
    ) {
        if id >= NUM_SAMPLES {
            return;
        }
        if let Some(pending) = self.pad_eq[..self.pad_eq_count].iter_mut().find(|pending| {
            same_parameter_participant(pending.id, pending.placement, id, placement)
                && pending.expected_epoch == expected_epoch
        }) {
            pending.low_db = low_db;
            pending.mid_db = mid_db;
            pending.high_db = high_db;
            pending.expected_epoch = expected_epoch;
            return;
        }
        if self.pad_eq_count < self.pad_eq.len() {
            self.pad_eq[self.pad_eq_count] = PendingPadEq {
                placement,
                id,
                low_db,
                mid_db,
                high_db,
                expected_epoch,
            };
            self.pad_eq_count += 1;
        }
    }

    fn apply_to(self, mixer: &mut RtMixer, transport: &mut TransportTimeline) -> usize {
        let mut applied = 0;

        if let Some(volume) = self.volume {
            mixer.set_volume(volume);
            applied += 1;
        }
        if self.speed.is_some() || self.master_period_seconds.is_some() {
            mixer.stage_global_rate(
                self.rate_revision,
                self.speed,
                self.master_period_seconds,
                transport,
            );
            applied += usize::from(self.speed.is_some())
                + usize::from(self.master_period_seconds.is_some());
        }
        for pending in self.pad_bpm[..self.pad_bpm_count].iter().copied() {
            let target = if pending.through_epoch.is_none() {
                // Only the cfg(test) unguarded SetPadBpm constructor has no retirement epoch.
                Some(pending.id)
            } else {
                mixer.timing_effect_slot(pending.id, pending.placement)
            };
            let Some(target) = target else {
                continue;
            };
            if !mixer.expected_source_generation_current(target, pending.expected_source_generation)
                || pending
                    .through_epoch
                    .is_some_and(|epoch| !mixer.legacy_parameter_after_copy(target, epoch))
            {
                continue;
            }
            mixer.set_pad_bpm(target, pending.bpm);
            if let Some(epoch) = pending.through_epoch {
                mixer.clear_constant_timing_for_placement(pending.id, epoch, pending.placement);
            }
            applied += 1;
        }
        for pending in self.pad_gain[..self.pad_gain_count].iter().copied() {
            let target = if pending.expected_epoch.is_none() {
                Some(pending.id)
            } else {
                mixer.timing_effect_slot(pending.id, pending.placement)
            };
            let Some(target) = target else {
                continue;
            };
            if pending
                .expected_epoch
                .is_some_and(|epoch| !mixer.pad_parameter_epoch_current(target, epoch))
            {
                continue;
            }
            mixer.set_pad_gain(target, pending.gain_db);
            applied += 1;
        }
        for pending in self.pad_eq[..self.pad_eq_count].iter().copied() {
            let target = if pending.expected_epoch.is_none() {
                Some(pending.id)
            } else {
                mixer.timing_effect_slot(pending.id, pending.placement)
            };
            let Some(target) = target else {
                continue;
            };
            if pending
                .expected_epoch
                .is_some_and(|epoch| !mixer.pad_parameter_epoch_current(target, epoch))
            {
                continue;
            }
            mixer.set_pad_eq(target, pending.low_db, pending.mid_db, pending.high_db);
            applied += 1;
        }

        applied
    }
}

pub(super) fn drain_parameter_messages(
    consumer: &mut Consumer<ControlParameterMessage>,
    mixer: &mut RtMixer,
    transport: &mut TransportTimeline,
) -> ParameterDrainResult {
    let drain_revision = mixer.begin_global_timing_drain();
    let mut pending = PendingControlParameters::default();
    let mut messages_drained = 0;

    while messages_drained < MAX_PARAMETER_MESSAGES_PER_CALLBACK {
        let Ok(message) = consumer.pop() else {
            break;
        };

        pending.record(message, mixer);
        messages_drained += 1;
    }

    mixer.finish_global_parameter_drain(drain_revision, consumer.is_empty());
    ParameterDrainResult {
        messages_drained,
        parameters_applied: pending.apply_to(mixer, transport),
    }
}

// Keep ordered command effects explicit at the realtime boundary.
#[allow(clippy::too_many_arguments)]
pub(super) fn process_control_message<
    const CAPACITY: usize,
    S: AudioMessageSink,
    R: AudioBufferRetirement,
>(
    message: ControlMessage,
    scheduler: &mut FixedCapacityScheduler<CAPACITY>,
    callback_start_frame: u64,
    trigger_quantization: &mut TriggerQuantization,
    transport: &mut TransportTimeline,
    mixer: &mut RtMixer,
    audio_messages: &mut S,
    retirement: &mut R,
) {
    if mixer.placement_pending() || scheduler.deferred_due().is_some() {
        mixer.defer_control(message);
        return;
    }
    match message {
        ControlMessage::StartMixCapture {
            publication,
            target_frame,
        } => {
            mixer.accept_mix_capture(
                publication,
                target_frame,
                callback_start_frame,
                scheduler,
                retirement,
            );
        }
        ControlMessage::StopMixCapture {
            session_id,
            target_frame,
        } => {
            mixer.schedule_mix_capture_stop(
                session_id,
                target_frame,
                callback_start_frame,
                scheduler,
            );
        }
        ControlMessage::RefreshAcceptedTiming(refresh) => {
            // The next frame is after this callback's bounded parameter drain. Existing
            // immediate commands may drain frame-zero events, so frame zero cannot be used.
            let target = callback_start_frame.checked_add(1).filter(|_| {
                !scheduler.is_full()
                    && refresh.global_current()
                    && mixer.refresh_binding_current(refresh.id, refresh.binding)
            });
            if let Some(target) = target {
                let _ =
                    scheduler.schedule(target, ScheduledCommand::RefreshAcceptedTiming(refresh));
            } else {
                refresh.finish(false);
                retirement.retire_accepted_timing_refresh(refresh);
            }
        }
        ControlMessage::GlobalPlaybackBatch(batch) => {
            let needed = if batch.capture_publication.is_some() && !batch.prepared_transferred() {
                2
            } else {
                1
            };
            // Only the attached capture introduces a second possible heavy
            // retirement owner here. Ordinary groups retain their established
            // scalar admission/deferred-due behavior at zero retirement space.
            if batch.capture_publication.is_some()
                && retirement.available_retirement_slots() < needed
            {
                mixer.defer_control(ControlMessage::GlobalPlaybackBatch(batch));
                return;
            }
            if batch.prepared_transferred() {
                // This Arc no longer owns Work, capture or receipt authority.
                retirement.retire_global_playback_batch(batch);
                return;
            }
            // Admission and execution both check current actual source/authority/projection.
            if scheduler.is_full()
                || !batch
                    .entries
                    .iter()
                    .all(|entry| global_playback_entry_slot(mixer, entry, batch.start).is_some())
            {
                batch.finish(false);
                mixer.reject_unadmitted_group_capture(&batch, retirement);
                retirement.retire_global_playback_batch(batch);
                return;
            }
            let target = if let Some(captured) = batch.captured_start.as_ref() {
                let Some(target) = captured.target_frame() else {
                    batch.finish(false);
                    mixer.reject_unadmitted_group_capture(&batch, retirement);
                    retirement.retire_global_playback_batch(batch);
                    return;
                };
                target
            } else if batch.start {
                match *trigger_quantization {
                    TriggerQuantization::Immediate => callback_start_frame,
                    TriggerQuantization::Grid { .. } => {
                        let Some(target) = quantized_target_frame(transport, *trigger_quantization)
                            .filter(|target| *target < u64::MAX)
                        else {
                            batch.finish(false);
                            mixer.reject_unadmitted_group_capture(&batch, retirement);
                            retirement.retire_global_playback_batch(batch);
                            return;
                        };
                        target
                    }
                }
            } else {
                callback_start_frame
            };
            if batch.start {
                super::pitch_runtime::accept_group(
                    mixer,
                    batch,
                    scheduler,
                    callback_start_frame,
                    target,
                    Some(*transport),
                    retirement,
                );
            } else {
                let _ = scheduler.schedule(target, ScheduledCommand::GlobalPlaybackBatch(batch));
            }
            drain_scheduler_due_at_callback_start(
                scheduler,
                callback_start_frame,
                mixer,
                transport,
                audio_messages,
                retirement,
            );
        }
        ControlMessage::Ping() => {
            audio_messages.push_audio_message(AudioMessage::Pong());
        }
        ControlMessage::LoadSample {
            id,
            sample,
            publication,
        } => {
            let Some(publication) = publication else {
                if !mixer.source_tracking() {
                    mixer.load_sample_rt(id, sample, retirement);
                } else {
                    retirement.retire_sample(sample);
                }
                return;
            };
            let ownership = publication.ownership.clone();
            let target = mixer
                .source_publication_target(&publication, &sample)
                .filter(|target| {
                    mixer.cold_source_current_with_defaults(
                        *target,
                        &sample,
                        publication.source_generation,
                        None,
                    )
                });
            let write = match ownership.observe_source_publication(&publication) {
                super::content_placement::PlacementObservation::Current(snapshot)
                    if target.is_some() =>
                {
                    ownership.prepare_slot_write_fixed(snapshot, snapshot.lifetime())
                }
                super::content_placement::PlacementObservation::Pending
                    if mixer.owns_source_publication(&publication) =>
                {
                    mixer.defer_control(ControlMessage::LoadSample {
                        id,
                        sample,
                        publication: Some(publication),
                    });
                    return;
                }
                _ => Err(super::content_placement::PlacementError::ClaimRejected),
            };
            let mut pending_sample = Some(sample);
            let rate = mixer.sample_rate_hz_for_publication();
            let binding =
                publication.binding(pending_sample.as_ref().expect("unclaimed source"), rate);
            let result = match write {
                Ok(write) => ownership.commit_slot_write_fixed(
                    &write,
                    || ownership.source_publication_binding_current(&publication, binding),
                    |_| {
                        mixer.load_sample_rt(
                            target.expect("validated source target"),
                            pending_sample.take().expect("single source adoption"),
                            retirement,
                        );
                        if let Some(key) = publication.key() {
                            ownership.acknowledge_callback_source(key);
                        }
                    },
                ),
                Err(error) => Err(error),
            };
            if matches!(
                result,
                Err(super::content_placement::PlacementError::Pending
                    | super::content_placement::PlacementError::Stale)
            ) && pending_sample.is_some()
            {
                mixer.defer_control(ControlMessage::LoadSample {
                    id,
                    sample: pending_sample.take().expect("unclaimed source"),
                    publication: Some(publication),
                });
                return;
            }
            if let Some(sample) = pending_sample {
                retirement.retire_sample(sample);
            }
            if result.is_ok()
                && let Some(target) = target
            {
                // Source-load telemetry is valid only after its callback-source
                // ACK. A stopped replacement has no voice render to refresh it.
                mixer.publish_source_sync_status(target);
            }
            drop(ownership);
            retirement.retire_source_publication(publication);
        }
        ControlMessage::LoadColdSample {
            publication,
            id: original_id,
            sample,
            source_generation,
            content_lifetime,
            adoption,
            epoch,
            captured_epoch,
            replace_assignment,
            loop_region,
            resident_cancelled,
            incoming_defaults,
            initial_pitch,
            initial_mix,
        } => {
            let ownership = publication.ownership.clone();
            let snapshot = match ownership.observe_source_publication(&publication) {
                super::content_placement::PlacementObservation::Current(snapshot) => Some(snapshot),
                super::content_placement::PlacementObservation::Pending
                    if mixer.owns_source_publication(&publication) =>
                {
                    mixer.defer_control(ControlMessage::LoadColdSample {
                        publication,
                        id: original_id,
                        sample,
                        source_generation,
                        content_lifetime,
                        adoption,
                        epoch,
                        captured_epoch,
                        replace_assignment,
                        loop_region,
                        resident_cancelled,
                        incoming_defaults,
                        initial_pitch,
                        initial_mix,
                    });
                    return;
                }
                _ => None,
            };
            let target = mixer.cold_publication_target(&publication, &sample, epoch.is_some());
            let id = target.unwrap_or(original_id);
            let rate = mixer.sample_rate_hz_for_publication();
            let geometry = target.is_some()
                && publication.source_generation == source_generation
                && publication.placement.lifetime() == content_lifetime
                && mixer.cold_source_current_with_initial_pitch(
                    id,
                    &sample,
                    source_generation,
                    incoming_defaults,
                    initial_pitch,
                );
            let write = match snapshot
                .filter(|_| geometry)
                .map(|snapshot| ownership.prepare_slot_write_fixed(snapshot, snapshot.lifetime()))
                .transpose()
            {
                Ok(write) => write,
                Err(super::content_placement::PlacementError::Pending) => {
                    mixer.defer_control(ControlMessage::LoadColdSample {
                        publication,
                        id: original_id,
                        sample,
                        source_generation,
                        content_lifetime,
                        adoption,
                        epoch,
                        captured_epoch,
                        replace_assignment,
                        loop_region,
                        resident_cancelled,
                        incoming_defaults,
                        initial_pitch,
                        initial_mix,
                    });
                    return;
                }
                Err(_) => None,
            };
            let epoch_current = || {
                resident_cancelled
                    .as_ref()
                    .is_none_or(|cancelled| !cancelled.load(std::sync::atomic::Ordering::Acquire))
                    && epoch.as_ref().is_none_or(|epoch| {
                        epoch.load(std::sync::atomic::Ordering::Acquire) == captured_epoch
                    })
            };
            let source_binding = publication.binding(&sample, rate);
            let mut pending_sample = Some(sample);
            let accepted = std::cell::Cell::new(false);
            let mut stopped_feedback = None;
            let result = if let Some(write) = &write {
                ownership.commit_slot_write_fixed(
                    write,
                    || {
                        if !geometry
                            || !epoch_current()
                            || !ownership.source_publication_identity_binding_current(
                                &publication,
                                source_binding,
                            )
                            || (epoch.is_some()
                                && !ownership.source_publication_binding_current(
                                    &publication,
                                    source_binding,
                                ))
                        {
                            return false;
                        }
                        if adoption
                            .compare_exchange(
                                0,
                                1,
                                std::sync::atomic::Ordering::AcqRel,
                                std::sync::atomic::Ordering::Acquire,
                            )
                            .is_err()
                        {
                            return false;
                        }
                        #[cfg(test)]
                        super::incoming_load::observe_callback_claim();
                        if !ownership.claim_cold_publication(&publication) || !epoch_current() {
                            ownership.finish_cold_publication(&publication, true, false);
                            adoption.store(3, std::sync::atomic::Ordering::Release);
                            return false;
                        }
                        true
                    },
                    |_| {
                        if replace_assignment {
                            transport.clear_pending_bootstrap_for_pad(id);
                            mixer.stop_sample_rt(id, retirement);
                        }
                        mixer.load_sample_with_initial_pitch_rt(
                            id,
                            pending_sample
                                .take()
                                .expect("single claimed source payload"),
                            incoming_defaults,
                            initial_pitch,
                            retirement,
                        );
                        if let Some(mixing) = initial_mix {
                            mixer.set_pad_gain(id, mixing.gain_db);
                            mixer.set_pad_eq(id, mixing.eq_db[0], mixing.eq_db[1], mixing.eq_db[2]);
                        }
                        if let Some((start, end)) = loop_region {
                            mixer.set_loop_region_frames(id, start, Some(end));
                        }
                        accepted.set(ownership.finish_cold_publication(&publication, true, true));
                        if replace_assignment && accepted.get() {
                            stopped_feedback = capture_pad_feedback(&ownership, id, 2, 0.0);
                        }
                    },
                )
            } else {
                Err(super::content_placement::PlacementError::ClaimRejected)
            };
            if matches!(
                result,
                Err(super::content_placement::PlacementError::Pending
                    | super::content_placement::PlacementError::Stale)
            ) && pending_sample.is_some()
            {
                mixer.defer_control(ControlMessage::LoadColdSample {
                    publication,
                    id: original_id,
                    sample: pending_sample.take().expect("unclaimed source payload"),
                    source_generation,
                    content_lifetime,
                    adoption,
                    epoch,
                    captured_epoch,
                    replace_assignment,
                    loop_region,
                    resident_cancelled,
                    incoming_defaults,
                    initial_pitch,
                    initial_mix,
                });
                return;
            }
            if accepted.get() {
                mixer.publish_source_sync_status(id);
                adoption.store(2, std::sync::atomic::Ordering::Release);
                if replace_assignment {
                    #[cfg(test)]
                    content_feedback_tests::after_effect();
                    audio_messages.push_captured_pad_feedback(
                        stopped_feedback,
                        AudioMessage::SampleStopped { id },
                    );
                }
            } else if pending_sample.is_some() {
                // No native image was changed. A concurrent writer is retried with this exact
                // payload; cancellation itself must never race a new incarnation's row.
                if mixer.owns_source_publication(&publication)
                    && matches!(
                        ownership.cancel_cold_publication_fixed(&publication),
                        Err(super::content_placement::PlacementError::Pending
                            | super::content_placement::PlacementError::Stale)
                    )
                {
                    mixer.defer_control(ControlMessage::LoadColdSample {
                        publication,
                        id: original_id,
                        sample: pending_sample.take().expect("unclaimed source payload"),
                        source_generation,
                        content_lifetime,
                        adoption,
                        epoch,
                        captured_epoch,
                        replace_assignment,
                        loop_region,
                        resident_cancelled,
                        incoming_defaults,
                        initial_pitch,
                        initial_mix,
                    });
                    return;
                }
                if incoming_defaults.is_some() {
                    adoption.swap(3, std::sync::atomic::Ordering::AcqRel);
                } else {
                    let _ = adoption.compare_exchange(
                        0,
                        3,
                        std::sync::atomic::Ordering::Release,
                        std::sync::atomic::Ordering::Acquire,
                    );
                }
            }
            if let Some(sample) = pending_sample {
                retirement.retire_sample(sample);
            }
            drop(ownership);
            retirement.retire_source_publication(publication);
            retirement.retire_cold_adoption(adoption);
            if let Some(cancelled) = resident_cancelled {
                retirement.retire_resident_cancellation(cancelled);
            }
        }
        ControlMessage::PublishPreparedStems { id, stems } => {
            mixer.publish_prepared_stems_rt(id, stems, retirement);
        }
        ControlMessage::RelocateResident(transaction) => {
            mixer.relocate_resident_at_output_frame(transaction, retirement, callback_start_frame);
        }
        ControlMessage::CaptureResidentSeek(capture) => {
            mixer.capture_resident_seek_rt(capture, retirement, callback_start_frame);
        }
        ControlMessage::BankMutation(publication) => {
            if let Some(mut work) = publication.take() {
                work.publication = Some(publication);
                mixer.mutate_bank_rt(work, callback_start_frame, retirement);
            } else {
                retirement.retire_bank_publication(publication);
            }
        }
        ControlMessage::VerifyRecoveredBank(probe) => {
            mixer.verify_recovered_bank_rt(probe, callback_start_frame, retirement);
        }
        ControlMessage::PadCopy(work) => {
            mixer.copy_pad_at_output_frame_rt(work, callback_start_frame, retirement);
        }
        ControlMessage::PadPair(work) => {
            mixer.pair_pad_rt(work, retirement);
        }
        ControlMessage::HoldAction(work) => {
            apply_hold_action_message(mixer, work, callback_start_frame, transport, retirement);
        }
        ControlMessage::PitchAction(publication) => {
            if let Some(work) = publication.take() {
                super::pitch_runtime::accept(
                    mixer,
                    work,
                    scheduler,
                    callback_start_frame,
                    retirement,
                );
            }
            retirement.retire_pitch_publication(publication);
        }
        ControlMessage::CapturePadStartTarget(owner) => {
            let original = quantized_target_frame(transport, *trigger_quantization)
                .unwrap_or(callback_start_frame);
            if let Err(reason) = mixer
                .source_control_revision(owner.content_key())
                .and_then(|revision| owner.capture_target(original, revision))
            {
                owner.reject(reason);
            }
            retirement.retire_pad_start_target(owner.into());
        }
        ControlMessage::CaptureGlobalStartTarget(owner) => {
            let original = quantized_target_frame(transport, *trigger_quantization)
                .unwrap_or(callback_start_frame);
            owner.capture_target(original, mixer);
            retirement.retire_global_start_target(owner);
        }
        ControlMessage::PreparedStart(publication) => {
            if let Some(work) = publication.take() {
                super::pitch_runtime::accept_ordinary(
                    mixer,
                    work,
                    scheduler,
                    callback_start_frame,
                    Some(*transport),
                    retirement,
                );
            }
            retirement.retire_ordinary_start_publication(publication);
            drain_scheduler_due_at_callback_start(
                scheduler,
                callback_start_frame,
                mixer,
                transport,
                audio_messages,
                retirement,
            );
        }
        ControlMessage::CommittedStemWork(work) => {
            mixer.apply_committed_stem_work(work, retirement);
        }
        ControlMessage::PublishConstantTiming { id, timing } => {
            mixer.publish_constant_timing_rt(id, timing, retirement);
        }
        ControlMessage::ClearPadConstantTiming {
            placement,
            id,
            through_epoch,
            expected_source_generation,
        } => {
            if let Some(target) = mixer.timing_effect_slot(id, placement)
                && mixer.expected_source_generation_current(target, expected_source_generation)
            {
                mixer.clear_constant_timing_for_placement(id, through_epoch, placement);
            }
        }
        ControlMessage::SetStemMixMode {
            id,
            mode,
            source_version_hash,
        } => {
            mixer.set_stem_mix_mode(id, mode, source_version_hash);
        }
        ControlMessage::SetStemPairFullMix { id, retired } => {
            for stems in retired.into_iter().flatten() {
                retirement.retire_prepared_stems(stems);
            }
            mixer.set_stem_pair_full_mix_rt(id, retirement);
        }
        ControlMessage::SetStemEnabledMask {
            id,
            enabled_stem_mask,
            source_version_hash,
        } => {
            mixer.set_stem_enabled_mask(id, enabled_stem_mask, source_version_hash);
        }
        ControlMessage::PlaySample {
            id,
            volume,
            received_at_ns,
            expected_source_generation,
        } => {
            schedule_play_sample_command_with_source(
                scheduler,
                callback_start_frame,
                *trigger_quantization,
                transport,
                id,
                volume,
                mixer,
                audio_messages,
                retirement,
                received_at_ns,
                expected_source_generation,
            );
        }
        ControlMessage::TriggerInputPad {
            id,
            start_s,
            end_s,
            exclusive,
            binding,
            received_at_ns,
            resident_control,
            launch_revision,
        } => {
            if trigger_binding_slot(
                mixer,
                id,
                binding,
                resident_control.as_ref(),
                launch_revision,
            )
            .is_none()
            {
                return;
            }
            let command = ScheduledCommand::TriggerInputPad {
                id,
                start_s,
                end_s,
                exclusive,
                binding,
                received_at_ns,
                resident_control,
                launch_revision,
            };
            if let Some(target) = quantized_target_frame(transport, *trigger_quantization) {
                if scheduler.schedule(target, command).is_ok() {
                    drain_scheduler_due_at_callback_start(
                        scheduler,
                        callback_start_frame,
                        mixer,
                        transport,
                        audio_messages,
                        retirement,
                    );
                }
            } else {
                schedule_immediate_command(
                    scheduler,
                    callback_start_frame,
                    command,
                    mixer,
                    transport,
                    audio_messages,
                    retirement,
                );
            }
        }
        ControlMessage::PlaySampleExclusive {
            id,
            volume,
            received_at_ns,
            expected_source_generation,
        } => {
            schedule_exclusive_play_sample_command_with_source(
                scheduler,
                callback_start_frame,
                *trigger_quantization,
                transport,
                id,
                volume,
                mixer,
                audio_messages,
                retirement,
                received_at_ns,
                expected_source_generation,
            );
        }
        #[cfg(test)]
        ControlMessage::StopSample { id } => {
            schedule_immediate_command(
                scheduler,
                callback_start_frame,
                ScheduledCommand::StopSample { id },
                mixer,
                transport,
                audio_messages,
                retirement,
            );
        }
        ControlMessage::StopSampleForPlacement { placement } => {
            schedule_immediate_command(
                scheduler,
                callback_start_frame,
                ScheduledCommand::StopSampleForPlacement { placement },
                mixer,
                transport,
                audio_messages,
                retirement,
            );
        }
        ControlMessage::StopAll() => {
            schedule_immediate_command(
                scheduler,
                callback_start_frame,
                ScheduledCommand::StopAll,
                mixer,
                transport,
                audio_messages,
                retirement,
            );
        }
        ControlMessage::UnloadSample { id, publication } => {
            let Some(publication) = publication else {
                if !mixer.source_tracking() {
                    mixer.unload_sample_rt(id, retirement);
                    transport.clear_pending_bootstrap_for_pad(id);
                    audio_messages.push_audio_message(AudioMessage::SampleStopped { id });
                }
                return;
            };
            let ownership = publication.ownership.clone();
            let target = mixer.source_retirement_target(&publication);
            let write = if target.is_some() {
                ownership.prepare_slot_write_fixed(publication.placement, 0)
            } else if mixer.source_publication_pending(&publication) {
                Err(super::content_placement::PlacementError::Pending)
            } else {
                Err(super::content_placement::PlacementError::ClaimRejected)
            };
            let result = match write {
                Ok(write) => ownership.commit_slot_write_fixed(
                    &write,
                    || ownership.source_retirement_fields_current(&publication),
                    |_| {
                        let id = target.expect("validated empty source target");
                        mixer.unload_sample_rt(id, retirement);
                        transport.clear_pending_bootstrap_for_pad(id);
                    },
                ),
                Err(error) => Err(error),
            };
            if matches!(
                result,
                Err(super::content_placement::PlacementError::Pending
                    | super::content_placement::PlacementError::Stale)
            ) {
                mixer.defer_control(ControlMessage::UnloadSample {
                    id,
                    publication: Some(publication),
                });
                return;
            }
            if result.is_ok() {
                if let Some((key, transport_revision)) = publication.retired_feedback {
                    audio_messages.push_audio_message(content_feedback_message(
                        target.expect("accepted retirement target"),
                        ownership.owner_identity().diagnostic(),
                        key,
                        transport_revision,
                        2,
                        0.0,
                    ));
                }
            }
            drop(ownership);
            retirement.retire_source_publication(publication);
        }
        ControlMessage::SetBpmLock(enabled) => {
            mixer.set_bpm_lock(enabled);
        }
        ControlMessage::SetKeyLock(enabled) => {
            mixer.set_key_lock(enabled);
        }
        #[cfg(test)]
        ControlMessage::SetPadKeyLock { id, enabled } => {
            mixer.set_pad_key_lock(id, enabled);
        }
        ControlMessage::SetPadKeyLockSavedIntent { placement, enabled } => {
            mixer.set_pad_key_lock_saved_intent(placement, enabled);
        }
        ControlMessage::SetPadKeyLockRequest {
            id,
            enabled,
            request_id,
            binding,
            source_generation,
        } => {
            mixer.set_pad_key_lock_request(id, enabled, request_id, binding, source_generation);
        }
        #[cfg(test)]
        ControlMessage::SetPadTimingMetadata { id, metadata } => {
            mixer.set_pad_timing_metadata(id, metadata);
        }
        ControlMessage::SetLegacyPadTimingMetadata {
            placement,
            id,
            metadata,
            through_epoch,
            expected_source_generation,
        } => {
            if let Some(target) = mixer.timing_effect_slot(id, placement)
                && mixer.expected_source_generation_current(target, expected_source_generation)
            {
                mixer.set_pad_timing_metadata(target, metadata);
                mixer.clear_constant_timing_for_placement(id, through_epoch, placement);
            }
        }
        ControlMessage::BootstrapTransportFromPad { id } => {
            if id < NUM_SAMPLES && !mixer.refuse_legacy_global_rate_if_pending() {
                transport.request_bootstrap(id);
            }
        }
        ControlMessage::AnchorTransportPhaseFromPad { id } => {
            if !mixer.refuse_legacy_global_rate_if_pending() {
                anchor_transport_phase_from_pad(mixer, transport, id);
            }
        }
        ControlMessage::SetPadLoopRegion {
            id,
            start_s,
            end_s,
            expected_source_generation,
        } => {
            if mixer.expected_source_generation_current(id, expected_source_generation) {
                mixer.set_pad_loop_region(id, start_s, end_s);
            }
        }
        ControlMessage::SetTriggerQuantization(mode) => {
            *trigger_quantization = mode;
        }
        #[cfg(test)]
        ControlMessage::PauseSample { id } => {
            mixer.pause_sample_at_output_frame(id, callback_start_frame);
        }
        #[cfg(test)]
        ControlMessage::ResumeSample { id } => {
            mixer.resume_sample_at_output_frame(id, callback_start_frame);
        }
        #[cfg(test)]
        ControlMessage::SeekSample {
            id,
            position_s,
            expected_source_generation,
        } => {
            if mixer.expected_source_generation_current(id, expected_source_generation) {
                mixer.seek_sample_at_output_frame(id, position_s, callback_start_frame);
            }
        }
        message @ ControlMessage::TransportForPlacement { .. } => {
            apply_transport_content_message(
                mixer,
                message,
                callback_start_frame,
                transport,
                audio_messages,
                retirement,
            );
        }
    }
    mixer.settle_invalid_hold_actions_rt(callback_start_frame, retirement);
    mixer.service_mix_capture(scheduler, retirement);
}

#[cfg(test)]
#[path = "launch_callback_tests.rs"]
mod launch_callback_tests;

/// Construct the callback's unique mixer owner on the control thread. Keeping the owner
/// boxed prevents CPAL's generic callback setup from moving copies of its fixed pad/voice
/// storage through the startup stack; rendering only borrows this preallocated owner.
pub(super) fn create_stream_mixer(
    channels: usize,
    sample_rate_hz: u32,
    current_timing_acknowledgements: Arc<CurrentTimingAcknowledgements>,
    input_runtime_ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,
    prepared_source_epochs: Vec<Arc<std::sync::atomic::AtomicU64>>,
) -> Result<Box<RtMixer>, super::key_lock_preparation::KeyLockPreparationError> {
    let mut mixer = Box::new(RtMixer::try_new(channels, sample_rate_hz as f32)?);
    mixer.set_current_timing_acknowledgements(current_timing_acknowledgements);
    mixer.set_input_runtime_ownership(input_runtime_ownership);
    mixer.set_prepared_source_epochs(prepared_source_epochs);
    Ok(mixer)
}

/// Every unique owner captured by the device callback leaves that callback as one box.
/// In particular, pending commands, the deferred payload and the full retirement backlog
/// cannot be destroyed by an unspecified CPAL closure-destructor thread.
struct AudioCallbackState {
    consumer_in: Consumer<ControlMessage>,
    parameter_consumer_in: Consumer<ControlParameterMessage>,
    producer_out: ContentFeedbackProducer,
    mixer: Box<RtMixer>,
    transport: TransportTimeline,
    scheduler: TransportScheduler,
    trigger_quantization: TriggerQuantization,
    callback_clock: Arc<SharedOutputClock>,
    clock_mapper: OutputClockMapper,
    loop_acceptance: Arc<super::loop_acceptance::SharedLoopAcceptance>,
    pad_peaks: [f32; NUM_SAMPLES],
    pad_activity: RtRenderPadActivity,
    last_pad_emit_frame: u64,
    last_master_emit_frame: u64,
    retired_buffers: RtAudioBufferRetirement,
}

/// Build the same complete boxed callback owner before any device registration. The
/// device-free startup regression uses this exact path on its fixed two-MiB stack.
fn create_stream_callback_owner(
    consumer_in: Consumer<ControlMessage>,
    parameter_consumer_in: Consumer<ControlParameterMessage>,
    producer_out: Producer<AudioMessage>,
    mixer: Box<RtMixer>,
    sample_rate_hz: u32,
    callback_clock: Arc<SharedOutputClock>,
    loop_acceptance: Arc<super::loop_acceptance::SharedLoopAcceptance>,
) -> std::io::Result<(
    CallbackCloseOwner<AudioCallbackState>,
    AudioBufferRetirementWorker,
)> {
    create_audio_buffer_retirement_with_callback(|retired_buffers| {
        boxed_stream_callback_state(
            (consumer_in, parameter_consumer_in, producer_out),
            mixer,
            sample_rate_hz,
            callback_clock,
            loop_acceptance,
            retired_buffers,
        )
    })
}

fn boxed_stream_callback_state(
    queues: (
        Consumer<ControlMessage>,
        Consumer<ControlParameterMessage>,
        Producer<AudioMessage>,
    ),
    mixer: Box<RtMixer>,
    sample_rate_hz: u32,
    callback_clock: Arc<SharedOutputClock>,
    loop_acceptance: Arc<super::loop_acceptance::SharedLoopAcceptance>,
    retired_buffers: RtAudioBufferRetirement,
) -> Box<AudioCallbackState> {
    let (consumer_in, parameter_consumer_in, producer_out) = queues;
    let producer_out = ContentFeedbackProducer {
        producer: producer_out,
        ownership: mixer.input_ownership_for_effect(),
    };
    Box::new(AudioCallbackState {
        consumer_in,
        parameter_consumer_in,
        producer_out,
        mixer,
        transport: TransportTimeline::new(sample_rate_hz),
        scheduler: TransportScheduler::new(),
        trigger_quantization: TriggerQuantization::Immediate,
        callback_clock,
        clock_mapper: OutputClockMapper::new(),
        loop_acceptance,
        pad_peaks: [0.0; NUM_SAMPLES],
        pad_activity: RtRenderPadActivity::default(),
        last_pad_emit_frame: 0,
        last_master_emit_frame: 0,
        retired_buffers,
    })
}

#[cfg(all(test, windows))]
fn create_stream_callback_owner_with_start(
    queues: (
        Consumer<ControlMessage>,
        Consumer<ControlParameterMessage>,
        Producer<AudioMessage>,
    ),
    mixer: Box<RtMixer>,
    sample_rate_hz: u32,
    callback_clock: Arc<SharedOutputClock>,
    loop_acceptance: Arc<super::loop_acceptance::SharedLoopAcceptance>,
    worker_start: impl FnOnce() + Send + 'static,
) -> std::io::Result<(
    CallbackCloseOwner<AudioCallbackState>,
    AudioBufferRetirementWorker,
)> {
    super::buffer_retirement::create_audio_buffer_retirement_with_callback_paused(
        |retired_buffers| {
            boxed_stream_callback_state(
                queues,
                mixer,
                sample_rate_hz,
                callback_clock,
                loop_acceptance,
                retired_buffers,
            )
        },
        worker_start,
    )
}

/// Create and configure the audio stream
///
/// This function:
/// 1. Sets up the default audio device
/// 2. Configures the stream with appropriate parameters
/// 3. Creates ring buffers for message passing
/// 4. Initializes the mixer
/// 5. Builds and returns the audio stream
pub fn create_audio_stream(
    input_clock: InputClock,
    current_timing_acknowledgements: Arc<CurrentTimingAcknowledgements>,
    input_runtime_ownership: Arc<super::input_runtime_binding::InputRuntimeOwnership>,
    prepared_source_epochs: Vec<Arc<std::sync::atomic::AtomicU64>>,
    loop_acceptance: Arc<super::loop_acceptance::SharedLoopAcceptance>,
) -> Result<AudioStreamHandle, Box<dyn std::error::Error>> {
    setup_logger();

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio device found")?;

    let config = device.default_output_config()?;
    let description = device.description();
    let device_descriptor = OutputDeviceDescriptor {
        host: host.id().name().to_owned(),
        endpoint_name: description
            .as_ref()
            .ok()
            .map(|value| value.name().to_owned()),
        endpoint_description_error: description.err().map(|error| error.to_string()),
        default_sample_format: config.sample_format().to_string(),
    };
    let sample_rate = config.sample_rate();
    let sample_rate_hz = sample_rate;
    let channels = config.channels();

    log::info!(
        "Starting AudioEngine... ({} ch@{} Hz)",
        channels,
        sample_rate_hz
    );

    // Create ring buffer for incoming messages (Python->Rust)
    let (producer_in, consumer_in) = RingBuffer::new(1024);

    // Create ring buffer for fast parameter updates (Python->Rust)
    let (parameter_producer_in, parameter_consumer_in) = RingBuffer::new(1024);

    // Create ring buffer for outgoing messages (Rust->Python)
    let (producer_out, consumer_out) = RingBuffer::new(1024);

    let mixer = create_stream_mixer(
        channels as usize,
        sample_rate_hz,
        current_timing_acknowledgements,
        input_runtime_ownership,
        prepared_source_epochs,
    )?;
    let output_clock = Arc::new(SharedOutputClock::new());
    let callback_clock = output_clock.clone();
    let stream_failed = Arc::new(AtomicBool::new(false));
    let callback_stream_failed = stream_failed.clone();
    let error_stream_failed = stream_failed.clone();

    let emit_interval_frames: u64 = (sample_rate_hz as u64 / 10).max(1);
    let (mut callback_owner, retirement_worker) = create_stream_callback_owner(
        consumer_in,
        parameter_consumer_in,
        producer_out,
        mixer,
        sample_rate_hz,
        callback_clock,
        loop_acceptance,
    )?;

    // Create stream config
    let stream_config = StreamConfig {
        channels,
        sample_rate,
        buffer_size: BufferSize::Fixed(512),
    };

    // Create audio stream with callback
    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                let state = callback_owner.state_mut();
                let observed_at_ns = input_clock.capture_ns();
                let buffer_start_frame = state.transport.output_frame();
                if callback_stream_failed.load(Ordering::Acquire) {
                    state.mixer.fail_mix_capture_stream();
                }

                let snapshot = state.clock_mapper.observe_callback(
                    info,
                    OutputClockSnapshot {
                        valid: true,
                        observed_at_ns,
                        audible_at_ns: observed_at_ns,
                        output_frame: buffer_start_frame,
                        sample_rate_hz,
                        master_period_seconds: state.transport.master_period_seconds(),
                        downbeat_frame: state.transport.downbeat_frame(),
                        master_beat: state
                            .transport
                            .beat_position_at_frame(buffer_start_frame)
                            .unwrap_or(0.0),
                        freshness_ns: 0,
                    },
                    data.len() / channels as usize,
                );

                drain_control_messages(
                    &mut state.consumer_in,
                    &mut state.scheduler,
                    buffer_start_frame,
                    &mut state.trigger_quantization,
                    &mut state.transport,
                    &mut state.mixer,
                    &mut state.producer_out,
                    &mut state.retired_buffers,
                );
                if callback_stream_failed.load(Ordering::Acquire) {
                    state.mixer.fail_mix_capture_stream();
                    state
                        .mixer
                        .service_mix_capture(&mut state.scheduler, &mut state.retired_buffers);
                }

                drain_parameter_messages(
                    &mut state.parameter_consumer_in,
                    &mut state.mixer,
                    &mut state.transport,
                );

                try_bootstrap_transport_from_reference(
                    &state.mixer,
                    &mut state.transport,
                    buffer_start_frame,
                );

                // Publish the same clock estimate with the accepted current grid.
                state.callback_clock.publish(OutputClockSnapshot {
                    master_period_seconds: state.transport.master_period_seconds(),
                    downbeat_frame: state.transport.downbeat_frame(),
                    master_beat: state
                        .transport
                        .beat_position_at_frame(buffer_start_frame)
                        .unwrap_or(0.0),
                    ..snapshot
                });

                // Render audio + compute per-pad peaks.
                render_scheduled_audio_tracking_pads(
                    &mut state.mixer,
                    &mut state.scheduler,
                    data,
                    &mut state.pad_peaks,
                    &mut state.pad_activity,
                    buffer_start_frame,
                    channels as usize,
                    &mut state.transport,
                    &mut state.producer_out,
                    &mut state.retired_buffers,
                );
                let master_peak = master_output_peak(data);

                let frames = data.len() / channels as usize;
                state.transport.advance_by_rendered_frames(frames);
                let frame_clock = state.transport.output_frame();
                state
                    .loop_acceptance
                    .observe(&state.mixer, observed_at_ns, frame_clock, frames);

                publish_pad_telemetry(
                    &mut state.producer_out,
                    &state.mixer,
                    &state.pad_peaks,
                    &state.pad_activity,
                    frame_clock,
                    emit_interval_frames,
                    &mut state.last_pad_emit_frame,
                );

                publish_master_peak_telemetry(
                    &mut state.producer_out,
                    master_peak,
                    frame_clock,
                    emit_interval_frames,
                    &mut state.last_master_emit_frame,
                );
            },
            move |err| {
                error_stream_failed.store(true, Ordering::Release);
                log::error!("Audio stream error: {}", err);
            },
            None,
        )
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>);
    let (stream, retirement_worker) =
        finish_stream_build(stream, retirement_worker, stream_failed.clone())?;

    Ok(AudioStreamHandle {
        stream,
        _retirement_worker: retirement_worker,
        producer: Arc::new(Mutex::new(producer_in)),
        parameter_producer: Arc::new(Mutex::new(parameter_producer_in)),
        consumer: Arc::new(Mutex::new(consumer_out)),
        output_channels: channels as usize,
        output_sample_rate: sample_rate_hz,
        stream_failed,
        output_clock,
        device_descriptor,
    })
}

/// Start playing the audio stream
pub fn start_stream(stream: &Stream) -> Result<(), Box<dyn std::error::Error>> {
    stream.play()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
    use crate::audio_engine::constants::PAD_EQ_DB_MIN;
    use crate::messages::{PadTimingMetadata, SampleBuffer};
    use std::sync::Arc;

    impl AudioMessageSink for Vec<AudioMessage> {
        fn available_audio_message_slots(&mut self) -> usize {
            usize::MAX
        }
        fn push_audio_message(&mut self, message: AudioMessage) {
            self.push(message);
        }
        fn push_captured_pad_feedback(
            &mut self,
            _captured: Option<AudioMessage>,
            diagnostic: AudioMessage,
        ) {
            // These standalone PCM/reference fixtures inspect diagnostic signals. Ownership
            // projections are tested with the actual production ContentFeedbackProducer.
            self.push(diagnostic);
        }
    }

    fn create_test_sample(channels: usize, frames: usize, value: f32) -> SampleBuffer {
        let samples = vec![value; channels * frames];
        SampleBuffer {
            residency: None,
            channels,
            samples: Arc::from(samples.into_boxed_slice()),
        }
    }

    #[test]
    fn stamped_commands_keep_legacy_quantization_and_retain_capture_in_scheduler() {
        for exclusive in [false, true] {
            for received_at_ns in [None, Some(0), Some(42), Some(u64::MAX)] {
                let mut mixer = RtMixer::new(1, 48_000.0);
                mixer.load_sample(0, create_test_sample(1, 100, 0.5));
                let mut transport = TransportTimeline::new(48_000);
                transport.advance_by_rendered_frames(6_001);
                let mut scheduler = FixedCapacityScheduler::<8>::new();
                let mut mode = TriggerQuantization::Grid { step_64ths: 4 };
                let command = if exclusive {
                    ControlMessage::PlaySampleExclusive {
                        expected_source_generation: None,
                        id: 0,
                        volume: 1.0,
                        received_at_ns,
                    }
                } else {
                    ControlMessage::PlaySample {
                        expected_source_generation: None,
                        id: 0,
                        volume: 1.0,
                        received_at_ns,
                    }
                };
                let mut messages = Vec::new();
                process_control_message(
                    command,
                    &mut scheduler,
                    6_001,
                    &mut mode,
                    &mut transport,
                    &mut mixer,
                    &mut messages,
                    &mut ImmediateAudioBufferRetirement,
                );

                assert!(messages.is_empty());
                assert_eq!(scheduler.peek_next_target_frame(), Some(12_000));
                let event = scheduler.pop_due_through(6_001, 12_000).unwrap();
                let retained = match event.command {
                    ScheduledCommand::PlaySample { received_at_ns, .. }
                    | ScheduledCommand::StopAllThenPlaySample { received_at_ns, .. } => {
                        received_at_ns
                    }
                    _ => panic!("unexpected scheduled command"),
                };
                assert_eq!(retained, received_at_ns);
                execute_scheduled_command(
                    &mut mixer,
                    &mut transport,
                    event.execution_frame,
                    event.command,
                    &mut messages,
                    &mut ImmediateAudioBufferRetirement,
                );
                let voice = mixer.voices.iter().find(|voice| voice.active).unwrap();
                assert_eq!(voice.frame_pos, 0);
            }
        }
    }

    fn assert_started(messages: &[AudioMessage], index: usize, expected_id: usize) {
        assert!(matches!(
            messages.get(index),
            Some(AudioMessage::SampleStarted { id }) if *id == expected_id
        ));
    }

    fn assert_stopped(messages: &[AudioMessage], index: usize, expected_id: usize) {
        assert!(matches!(
            messages.get(index),
            Some(AudioMessage::SampleStopped { id }) if *id == expected_id
        ));
    }

    fn active_voice_frame(mixer: &RtMixer, id: usize) -> Option<usize> {
        mixer
            .voices
            .iter()
            .find(|voice| voice.active && voice.sample_id == id)
            .map(|voice| voice.frame_pos)
    }

    #[test]
    fn control_drain_respects_per_callback_budget() {
        let total_messages = MAX_CONTROL_MESSAGES_PER_CALLBACK + 3;
        let (mut producer, mut consumer) = RingBuffer::new(total_messages + 1);
        for _ in 0..total_messages {
            producer.push(ControlMessage::Ping()).unwrap();
        }
        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        let mut trigger_quantization = TriggerQuantization::Immediate;
        let mut messages = Vec::new();

        let processed = drain_control_messages(
            &mut consumer,
            &mut scheduler,
            0,
            &mut trigger_quantization,
            &mut transport,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert_eq!(processed, MAX_CONTROL_MESSAGES_PER_CALLBACK);
        assert_eq!(messages.len(), MAX_CONTROL_MESSAGES_PER_CALLBACK);
        assert_eq!(consumer.slots(), 3);
    }

    #[test]
    fn parameter_drain_coalesces_latest_value_per_identity() {
        let (mut producer, mut consumer) = RingBuffer::new(8);
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 0,
                gain_db: -6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 0,
                gain_db: 6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetVolume(0.5))
            .unwrap();

        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut transport = TransportTimeline::new(44_100);
        mixer.load_sample(0, create_test_sample(1, 8, 1.0));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 3,
                parameters_applied: 2
            }
        );

        let mut output = vec![0.0; 1];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        assert!(mixer.play_sample(0, 1.0));
        mixer.render(&mut output, &mut pad_peaks);

        let expected = 10.0_f32.powf(6.0 / 20.0) * 0.5;
        assert!((output[0] - expected).abs() < 1e-5);
    }

    #[test]
    fn parameter_drain_coalesces_latest_pad_eq_target() {
        let (mut producer, mut consumer) = RingBuffer::new(4);
        producer
            .push(ControlParameterMessage::SetPadEq {
                id: 0,
                low_db: PAD_EQ_DB_MIN,
                mid_db: PAD_EQ_DB_MIN,
                high_db: PAD_EQ_DB_MIN,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadEq {
                id: 0,
                low_db: 0.0,
                mid_db: 0.0,
                high_db: 0.0,
            })
            .unwrap();

        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut transport = TransportTimeline::new(44_100);
        mixer.load_sample(0, create_test_sample(1, 8, 0.5));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 2,
                parameters_applied: 1
            }
        );

        assert!(mixer.play_sample(0, 1.0));
        let mut output = vec![0.0; 1];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!((output[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn parameter_drain_ignores_invalid_pad_ids_while_coalescing_touched_ids() {
        let (mut producer, mut consumer) = RingBuffer::new(8);
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: NUM_SAMPLES,
                gain_db: 12.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 1,
                gain_db: -6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadGain {
                id: 1,
                gain_db: 6.0,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadBpm {
                id: 2,
                bpm: Some(90.0),
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetPadBpm { id: 2, bpm: None })
            .unwrap();

        let mut mixer = RtMixer::new(1, 44_100.0);
        let mut transport = TransportTimeline::new(44_100);
        mixer.load_sample(1, create_test_sample(1, 8, 1.0));
        mixer.set_pad_bpm(2, Some(100.0));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 5,
                parameters_applied: 2
            }
        );
        assert_eq!(mixer.output_bpm_for_sample_id(2), None);

        assert!(mixer.play_sample(1, 1.0));
        let mut output = vec![0.0; 1];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        let expected = 10.0_f32.powf(6.0 / 20.0);
        assert!((output[0] - expected).abs() < 1e-5);
    }

    #[test]
    fn master_bpm_parameter_updates_mixer_and_transport_clock() {
        let (mut producer, mut consumer) = RingBuffer::new(4);
        producer
            .push(ControlParameterMessage::SetMasterBpm(120.0))
            .unwrap();

        let mut mixer = RtMixer::new(1, 10.0);
        mixer.set_bpm_lock(true);
        mixer.set_pad_bpm(0, Some(100.0));

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        assert_eq!(transport.bar_phase_beats(), Some(1.0));

        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);

        assert_eq!(
            result,
            ParameterDrainResult {
                messages_drained: 1,
                parameters_applied: 1
            }
        );
        assert_eq!(transport.master_bpm(), Some(120.0));
        assert_eq!(transport.downbeat_frame(), 5);
        assert_eq!(transport.bar_phase_beats(), Some(1.0));
        let output_bpm = mixer.output_bpm_for_sample_id(0).unwrap();
        assert!((output_bpm - 120.0).abs() < 1e-4);
    }

    #[test]
    fn master_parameter_period_is_precise_and_failure_keeps_mixer_transport_coupled() {
        let mut mixer = RtMixer::new(1, 48_000.0);
        mixer.set_bpm_lock(true);
        mixer.set_pad_bpm(0, Some(93.123_456_789_012_34));
        let mut transport = TransportTimeline::new(48_000);
        transport.advance_by_rendered_frames(13_001);
        let before_beat = transport.beat_position().unwrap();
        let bpm = 129.987_654_321_098_76;
        let period = 60.0 / bpm;
        assert_ne!(period, 60.0 / f64::from(bpm as f32));
        let (mut producer, mut consumer) = RingBuffer::new(1);
        producer
            .push(ControlParameterMessage::SetMasterBpm(bpm))
            .unwrap();
        drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
        assert_eq!(transport.master_period_seconds(), Some(period));
        assert_eq!(
            mixer.transport_reference_period_for_sample_id(0),
            Some(period)
        );
        assert_eq!(transport.beat_position(), Some(before_beat));
        let before_output_bpm = mixer.output_bpm_for_sample_id(0);
        // Finite positive BPM can still imply an overflowing output-frame period.
        producer
            .push(ControlParameterMessage::SetMasterBpm(60.0 / f64::MAX))
            .unwrap();
        drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
        assert_eq!(transport.master_period_seconds(), Some(period));
        assert_eq!(
            mixer.transport_reference_period_for_sample_id(0),
            Some(period)
        );
        assert_eq!(mixer.output_bpm_for_sample_id(0), before_output_bpm);
        assert_eq!(transport.beat_position(), Some(before_beat));
    }

    #[test]
    fn direct_master_period_and_bpm_share_last_admitted_parameter_order() {
        let period = 0.224_504_832_555_644_9_f64;
        assert_ne!(period.to_bits(), (60.0 / (60.0 / period)).to_bits());
        for (first, last, expected) in [
            (
                ControlParameterMessage::SetMasterBpm(120.0),
                ControlParameterMessage::SetMasterPeriod(period),
                period,
            ),
            (
                ControlParameterMessage::SetMasterPeriod(period),
                ControlParameterMessage::SetMasterBpm(123.5),
                60.0 / 123.5,
            ),
        ] {
            let (mut producer, mut consumer) = RingBuffer::new(2);
            producer.push(first).unwrap();
            producer.push(last).unwrap();
            let mut mixer = RtMixer::new(1, 48_000.0);
            mixer.set_bpm_lock(true);
            mixer.set_pad_bpm(0, Some(93.123_456_789));
            let mut transport = TransportTimeline::new(48_000);
            transport.advance_by_rendered_frames(13_001);
            let before_beat = transport.beat_position();
            let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
            assert_eq!(result.messages_drained, 2);
            assert_eq!(result.parameters_applied, 1);
            assert_eq!(
                transport.master_period_seconds().unwrap().to_bits(),
                expected.to_bits()
            );
            assert_eq!(
                mixer
                    .transport_reference_period_for_sample_id(0)
                    .unwrap()
                    .to_bits(),
                expected.to_bits()
            );
            assert_eq!(transport.beat_position(), before_beat);
        }
    }

    #[test]
    fn invalid_direct_master_period_or_full_ring_keeps_effective_clock() {
        let period = 0.224_504_832_555_644_9_f64;
        let mut mixer = RtMixer::new(1, 48_000.0);
        mixer.set_bpm_lock(true);
        mixer.set_pad_bpm(0, Some(93.123_456_789));
        let mut transport = TransportTimeline::new(48_000);
        transport.advance_by_rendered_frames(13_001);
        let (mut producer, mut consumer) = RingBuffer::new(1);
        producer
            .push(ControlParameterMessage::SetMasterPeriod(period))
            .unwrap();
        assert!(
            producer
                .push(ControlParameterMessage::SetMasterPeriod(0.75))
                .is_err()
        );
        assert_eq!(transport.master_period_seconds(), Some(0.5));
        drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
        let beat_before = transport.beat_position();
        for invalid in [f64::NAN, f64::INFINITY, 0.0, -1.0, f64::MAX] {
            producer
                .push(ControlParameterMessage::SetMasterPeriod(invalid))
                .unwrap();
            drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
            assert_eq!(
                transport.master_period_seconds().unwrap().to_bits(),
                period.to_bits()
            );
            assert_eq!(
                mixer
                    .transport_reference_period_for_sample_id(0)
                    .unwrap()
                    .to_bits(),
                period.to_bits()
            );
            assert_eq!(transport.beat_position(), beat_before);
        }
    }

    #[test]
    fn coupled_speed_master_period_keeps_component_order_and_exact_bits() {
        let period = 0.224_504_832_555_644_9_f64;
        let batch = ControlParameterMessage::SetSpeedAndMasterPeriod {
            speed: 1.25,
            period_seconds: period,
        };
        for (messages, expected_speed, expected_period) in [
            (
                vec![
                    ControlParameterMessage::SetSpeed(0.75),
                    ControlParameterMessage::SetMasterBpm(120.0),
                    batch,
                ],
                1.25,
                period,
            ),
            (
                vec![batch, ControlParameterMessage::SetSpeed(0.75)],
                0.75,
                period,
            ),
            (
                vec![batch, ControlParameterMessage::SetMasterBpm(123.5)],
                1.25,
                60.0 / 123.5,
            ),
            (
                vec![batch, ControlParameterMessage::SetMasterPeriod(0.6)],
                1.25,
                0.6,
            ),
        ] {
            let (mut producer, mut consumer) = RingBuffer::new(messages.len());
            for message in messages {
                producer.push(message).unwrap();
            }
            let mut mixer = RtMixer::new(1, 48_000.0);
            mixer.set_pad_bpm(0, Some(120.0));
            let mut transport = TransportTimeline::new(48_000);
            transport.advance_by_rendered_frames(13_001);
            let beat_before = transport.beat_position();
            let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
            assert_eq!(result.parameters_applied, 2);
            assert_eq!(
                mixer.output_bpm_for_sample_id(0),
                Some(60.0 / (0.5 / expected_speed))
            );
            assert_eq!(
                transport.master_period_seconds().unwrap().to_bits(),
                expected_period.to_bits()
            );
            assert_eq!(transport.beat_position(), beat_before);
            mixer.set_bpm_lock(true);
            assert_eq!(
                mixer
                    .transport_reference_period_for_sample_id(0)
                    .unwrap()
                    .to_bits(),
                expected_period.to_bits()
            );
        }
    }

    #[test]
    fn coupled_speed_master_period_is_not_split_at_callback_drain_budget() {
        let period = 0.224_504_832_555_644_9_f64;
        let (mut producer, mut consumer) = RingBuffer::new(MAX_PARAMETER_MESSAGES_PER_CALLBACK + 1);
        for _ in 1..MAX_PARAMETER_MESSAGES_PER_CALLBACK {
            producer
                .push(ControlParameterMessage::SetVolume(0.5))
                .unwrap();
        }
        producer
            .push(ControlParameterMessage::SetSpeedAndMasterPeriod {
                speed: 1.25,
                period_seconds: period,
            })
            .unwrap();
        producer
            .push(ControlParameterMessage::SetVolume(0.75))
            .unwrap();
        let mut mixer = RtMixer::new(1, 48_000.0);
        mixer.set_pad_bpm(0, Some(120.0));
        let mut transport = TransportTimeline::new(48_000);
        let result = drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
        assert_eq!(result.messages_drained, MAX_PARAMETER_MESSAGES_PER_CALLBACK);
        assert_eq!(result.parameters_applied, 3);
        assert_eq!(mixer.output_bpm_for_sample_id(0), Some(150.0));
        assert_eq!(
            transport.master_period_seconds().unwrap().to_bits(),
            period.to_bits()
        );
        mixer.set_bpm_lock(true);
        assert_eq!(
            mixer.transport_reference_period_for_sample_id(0),
            Some(period)
        );
        assert_eq!(
            consumer.pop().unwrap(),
            ControlParameterMessage::SetVolume(0.75)
        );
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn full_parameter_queue_does_not_consume_command_queue_capacity() {
        let (mut command_producer, mut command_consumer) = RingBuffer::<ControlMessage>::new(1);
        let (mut parameter_producer, _parameter_consumer) =
            RingBuffer::<ControlParameterMessage>::new(1);

        parameter_producer
            .push(ControlParameterMessage::SetVolume(0.25))
            .unwrap();
        assert!(
            parameter_producer
                .push(ControlParameterMessage::SetVolume(0.5))
                .is_err()
        );

        assert!(command_producer.push(ControlMessage::StopAll()).is_ok());
        assert!(matches!(
            command_consumer.pop(),
            Ok(ControlMessage::StopAll())
        ));
    }

    #[test]
    fn master_output_peak_is_post_sum_and_post_master_volume() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 16, 0.8));
        mixer.load_sample(1, create_test_sample(1, 16, 0.6));
        mixer.set_volume(0.5);
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));

        let mut output = vec![0.0; 16];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        assert!(output.iter().all(|sample| (*sample - 0.7).abs() < 1e-5));
        assert!((master_output_peak(&output) - 0.7).abs() < 1e-5);
        assert!((pad_peaks[0] - 0.8).abs() < 1e-5);
        assert!((pad_peaks[1] - 0.6).abs() < 1e-5);
    }

    #[test]
    fn master_peak_telemetry_preserves_unclamped_value() {
        let (mut producer, mut consumer) = RingBuffer::<AudioMessage>::new(2);
        let mut last_master_emit_frame = 0;

        publish_master_peak_telemetry(
            &mut producer,
            1.25,
            4_410,
            4_410,
            &mut last_master_emit_frame,
        );

        assert_eq!(last_master_emit_frame, 4_410);
        assert!(matches!(
            consumer.pop(),
            Ok(AudioMessage::MasterPeak { peak }) if (peak - 1.25).abs() < 1e-5
        ));
    }

    #[test]
    fn full_audio_message_queue_drops_master_peak_without_blocking() {
        let (mut producer, mut consumer) = RingBuffer::<AudioMessage>::new(1);
        producer.push(AudioMessage::Pong()).unwrap();
        let mut last_master_emit_frame = 0;

        publish_master_peak_telemetry(
            &mut producer,
            0.75,
            4_410,
            4_410,
            &mut last_master_emit_frame,
        );

        assert_eq!(last_master_emit_frame, 4_410);
        assert!(matches!(consumer.pop(), Ok(AudioMessage::Pong())));
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn pad_telemetry_publishes_only_touched_pads_on_shared_interval() {
        let mixer = RtMixer::new(1, 44_100.0);
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        pad_peaks[0] = 0.5;
        pad_peaks[1] = 0.75;
        let mut activity = RtRenderPadActivity::default();
        activity.record(0);
        let mut messages = Vec::new();
        let mut last_pad_emit_frame = 0;

        publish_pad_telemetry(
            &mut messages,
            &mixer,
            &pad_peaks,
            &activity,
            9,
            10,
            &mut last_pad_emit_frame,
        );

        assert!(messages.is_empty());
        assert_eq!(last_pad_emit_frame, 0);

        publish_pad_telemetry(
            &mut messages,
            &mixer,
            &pad_peaks,
            &activity,
            10,
            10,
            &mut last_pad_emit_frame,
        );

        assert_eq!(last_pad_emit_frame, 10);
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages[0],
            AudioMessage::PadPeak { id: 0, peak } if (peak - 0.5).abs() < 1e-5
        ));

        messages.clear();
        activity.clear();
        publish_pad_telemetry(
            &mut messages,
            &mixer,
            &pad_peaks,
            &activity,
            20,
            10,
            &mut last_pad_emit_frame,
        );

        assert_eq!(last_pad_emit_frame, 20);
        assert!(messages.is_empty());
    }

    #[test]
    fn popped_cold_payload_defers_pending_then_owns_one_ack_before_later_commands() {
        use crate::audio_engine::content_placement::PlacementError;
        use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
        use std::sync::atomic::{AtomicU8, Ordering};
        let ownership = Arc::new(InputRuntimeOwnership::tracked());
        let sample = create_test_sample(1, 128, 0.25);
        let lifetime = crate::audio_engine::content_lifetime::reserve().unwrap();
        let original = ownership.slot_snapshot(0).unwrap();
        let prepared = ownership.prepare_slot_write(original, lifetime).unwrap();
        let publication = ownership
            .publish_source_write(&prepared, &sample, 48_000, 7, 2, None, true, || true)
            .unwrap();
        let adoption = Arc::new(AtomicU8::new(0));
        let mut mixer = RtMixer::new(1, 48_000.0);
        mixer.set_input_runtime_ownership(ownership.clone());
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut mode = TriggerQuantization::Immediate;
        let mut transport = TransportTimeline::new(48_000);
        let mut feedback = Vec::new();
        let mut retirement = ImmediateAudioBufferRetirement;
        let (mut producer, mut consumer) = RingBuffer::new(2);
        producer
            .push(ControlMessage::LoadColdSample {
                initial_mix: None,
                publication: publication.clone(),
                id: 0,
                sample: sample.clone(),
                source_generation: 7,
                content_lifetime: lifetime,
                adoption: adoption.clone(),
                epoch: None,
                captured_epoch: 0,
                replace_assignment: false,
                loop_region: None,
                resident_cancelled: None,
                incoming_defaults: None,
                initial_pitch: super::super::cold_load::SourcePitchInitialization::RestoredTuple {
                    pitch: super::super::pitch_intent::PitchIntent::new(6, 18).unwrap(),
                    key_lock: Some(false),
                },
            })
            .unwrap();
        // The message has actually left the queue before another source writer starts.
        let popped = consumer.pop().unwrap();
        producer.push(ControlMessage::Ping()).unwrap();
        let unrelated = ownership.slot_snapshot(215).unwrap();
        let other_write = ownership.prepare_slot_write_fixed(unrelated, 0).unwrap();
        assert_eq!(ownership.commit_slot_write_fixed(&other_write,|| {
            process_control_message(popped,&mut scheduler,0,&mut mode,&mut transport,&mut mixer,&mut feedback,&mut retirement);
            assert_eq!(adoption.load(Ordering::Acquire),0);
            assert!(!mixer.can_play_sample(0,1.0));
            assert!(feedback.is_empty());
            assert!(matches!(mixer.deferred_control(),Some(ControlMessage::LoadColdSample { publication:held,sample:held_sample,adoption:held_adoption,.. })
                if held.key()==publication.key() && Arc::ptr_eq(&held_sample.samples,&sample.samples) && Arc::ptr_eq(held_adoption,&adoption)));
            false
        },|_|panic!("rejected unrelated writer must not apply")),Err(PlacementError::ClaimRejected));
        assert_eq!(
            drain_control_messages(
                &mut consumer,
                &mut scheduler,
                0,
                &mut mode,
                &mut transport,
                &mut mixer,
                &mut feedback,
                &mut retirement
            ),
            2
        );
        assert!(mixer.deferred_control().is_none());
        assert_eq!(adoption.load(Ordering::Acquire), 2);
        assert_eq!(ownership.cold_status(0, 7), Some(2));
        assert_eq!(
            mixer.pad_pitch_intent(0),
            Some(super::super::pitch_intent::PitchIntent::new(6, 18).unwrap())
        );
        assert!(mixer.can_play_sample(0, 1.0));
        assert_eq!(feedback.len(), 1);
        assert!(matches!(feedback[0], AudioMessage::Pong()));
        assert_eq!(
            drain_control_messages(
                &mut consumer,
                &mut scheduler,
                0,
                &mut mode,
                &mut transport,
                &mut mixer,
                &mut feedback,
                &mut retirement
            ),
            0
        );
        assert_eq!(adoption.load(Ordering::Acquire), 2);
        assert_eq!(feedback.len(), 1);
    }

    #[test]
    fn retirement_slot_estimate_covers_polyphonic_stop_paths() {
        assert_eq!(
            control_message_retirement_slots_needed(&ControlMessage::StopSample { id: 0 }, 0),
            3 * MAX_VOICES
        );
        assert_eq!(
            control_message_retirement_slots_needed(
                &ControlMessage::UnloadSample {
                    id: 0,
                    publication: None
                },
                0
            ),
            3 * MAX_VOICES + 3
        );
        assert_eq!(
            control_message_retirement_slots_needed(&ControlMessage::StopAll(), 0),
            3 * MAX_VOICES
        );
        assert_eq!(
            control_message_retirement_slots_needed(
                &ControlMessage::PlaySampleExclusive {
                    expected_source_generation: None,
                    id: 0,
                    volume: 1.0,
                    received_at_ns: None
                },
                0
            ),
            3 * MAX_VOICES
        );
    }

    #[test]
    fn test_logger_setup() {
        // This test just verifies that logger setup doesn't panic
        // Multiple calls should be safe (though only the first takes effect)
        setup_logger();
        setup_logger(); // Should not panic
    }

    #[test]
    fn production_stream_mixer_constructor_and_callback_capture_fit_two_mib_without_device() {
        use crate::audio_engine::input_runtime_binding::{
            InputPadBinding, InputRuntimeOwnership, KeyLockStatus,
        };
        use crate::audio_engine::stretch_processor::StretchProcessor;
        use crate::audio_engine::voice_slot::VoiceSlot;
        use crate::messages::PreparedStemSet;
        use std::mem::{size_of, size_of_val};

        let mode_arrays_bytes = size_of::<[u64; NUM_SAMPLES]>()
            + 2 * size_of::<[bool; NUM_SAMPLES]>()
            + size_of::<[Option<(u64, usize)>; NUM_SAMPLES]>()
            + size_of::<[Option<KeyLockStatus>; NUM_SAMPLES]>();
        eprintln!(
            "stream startup layout: mixer={} voice={} stretch={} input_ownership={} key_lock_status={} added_mixer_mode_arrays={} control_message={} mode_request_payload={} two_stem_payload={} scheduler={}",
            size_of::<RtMixer>(),
            size_of::<VoiceSlot>(),
            size_of::<StretchProcessor>(),
            size_of::<InputRuntimeOwnership>(),
            size_of::<KeyLockStatus>(),
            mode_arrays_bytes,
            size_of::<ControlMessage>(),
            size_of::<(usize, bool, u64, InputPadBinding, u64)>(),
            size_of::<(usize, [Option<PreparedStemSet>; 2])>(),
            size_of::<TransportScheduler>(),
        );

        // This is the same constructor and ownership binding used by create_audio_stream.
        // A fixed thread stack keeps the test independent of the runner's RUST_MIN_STACK.
        // The fixture never asks CPAL for a host, device, configuration, or stream.
        std::thread::Builder::new()
            .name("stream-mixer-startup-without-device".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                for (channels, sample_rate_hz) in [(1, 44_100), (2, 48_000)] {
                    let mixer = create_stream_mixer(
                        channels,
                        sample_rate_hz,
                        Arc::default(),
                        Arc::default(),
                        (0..NUM_SAMPLES).map(|_| Arc::default()).collect(),
                    )
                    .unwrap();
                    let storage = mixer.as_ref() as *const RtMixer as usize;

                    let (_commands, consumer_in) = RingBuffer::new(1);
                    let (_parameters, parameter_consumer_in) = RingBuffer::new(1);
                    let (producer_out, _feedback) = RingBuffer::new(1);
                    let (mut owner, worker) = create_stream_callback_owner(
                        consumer_in,
                        parameter_consumer_in,
                        producer_out,
                        mixer,
                        sample_rate_hz,
                        Arc::new(SharedOutputClock::new()),
                        Arc::default(),
                    )
                    .unwrap();
                    let state_storage = owner.state_mut() as *mut AudioCallbackState as usize;

                    // CPAL captures this actual complete-state owner. Unique Native images
                    // stay boxed; its destructor forwards the box to the retirement worker.
                    fn capture<F: FnMut(&mut [f32]) -> (usize, usize)>(callback: F) -> F {
                        callback
                    }
                    let mut callback = capture(move |data: &mut [f32]| {
                        let state = owner.state_mut();
                        let state_storage = state as *mut AudioCallbackState as usize;
                        state.mixer.render_rt_at_output_frame(
                            data,
                            &mut state.pad_peaks,
                            0,
                            &mut state.retired_buffers,
                        );
                        (
                            state.mixer.as_ref() as *const RtMixer as usize,
                            state_storage,
                        )
                    });
                    assert_eq!(
                        size_of_val(&callback),
                        size_of::<CallbackCloseOwner<AudioCallbackState>>()
                    );
                    assert!(size_of_val(&callback) < size_of::<AudioCallbackState>());
                    let mut output = [1.0_f32; 64];
                    for _ in 0..3 {
                        assert_eq!(callback(&mut output), (storage, state_storage));
                        assert!(output.iter().all(|value| *value == 0.0));
                    }
                    drop(callback);
                    drop(worker);
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn test_audio_stream_creation() {
        // This is a basic smoke test to ensure the function signature is correct
        // Actual stream creation requires audio hardware
        if cpal::default_host().default_output_device().is_none() {
            return; // Skip test if no audio device available
        }

        let result = create_audio_stream(
            InputClock::new(),
            Arc::default(),
            Arc::default(),
            (0..NUM_SAMPLES).map(|_| Arc::default()).collect(),
            Arc::default(),
        );
        // We expect this to potentially fail in test environments,
        // but we want to ensure the function exists and has the right signature
        match result {
            Ok(_) => {
                // If it works, that's great
            }
            Err(_) => {
                // Expected in many test environments
            }
        }
    }

    #[test]
    fn immediate_command_uses_current_frame_scheduler_path() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            12,
            ScheduledCommand::PlaySample {
                expected_source_generation: None,
                id: 0,
                volume: 1.0,
                received_at_ns: None,
            },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_eq!(active_voice_frame(&mixer, 0), Some(0));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn immediate_command_falls_back_when_scheduler_is_full() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut scheduler = FixedCapacityScheduler::<0>::new();
        let mut transport = TransportTimeline::new(44_100);
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            12,
            ScheduledCommand::PlaySample {
                expected_source_generation: None,
                id: 0,
                volume: 1.0,
                received_at_ns: None,
            },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_schedules_supported_grid_and_renders_at_target_offset() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(5));
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert!(messages.is_empty());

        let mut output = vec![0.0; 4];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert_eq!(output[0], 0.0);
        assert!(
            output[1..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_schedules_selected_subdivision_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(7);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert!(messages.is_empty());
    }

    #[test]
    fn quantized_play_starts_at_loop_start_even_with_phase_metadata() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_loop_region(0, 0.7, Some(5.0));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert_eq!(active_voice_frame(&mixer, 0), Some(7));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_without_pad_metadata_falls_back_to_loop_start() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_loop_region(0, 0.7, Some(5.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert_eq!(active_voice_frame(&mixer, 0), Some(7));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_schedules_future_sixteenth_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(5));
        assert!(mixer.voices.iter().all(|voice| !voice.active));
        assert!(messages.is_empty());
    }

    #[test]
    fn quantized_play_on_grid_boundary_executes_at_current_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(10);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_without_master_bpm_falls_back_to_immediate() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut transport = TransportTimeline::new(10);
        transport.clear_master_bpm();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn quantized_play_uses_global_masterclock_not_active_pad_phase() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.load_sample(1, create_test_sample(1, 64, 0.25));
        for id in [0, 1] {
            mixer.set_pad_bpm(id, Some(60.0));
            mixer.set_pad_timing_metadata(
                id,
                PadTimingMetadata {
                    phase_anchor_s: 0.0,
                },
            );
        }
        assert!(mixer.play_sample(0, 1.0));

        let mut rendered = vec![0.0; 6];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut rendered, &mut pad_peaks);

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(6);
        transport.set_downbeat_frame(0);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(active_voice_frame(&mixer, 1), None);
        assert!(messages.is_empty());
    }

    #[test]
    fn quantized_late_click_waits_for_future_grid_and_starts_at_loop_start() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.25));
        mixer.set_pad_loop_region(0, 0.7, Some(5.0));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.0,
            },
        );
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(6);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(active_voice_frame(&mixer, 0), None);

        let mut output = vec![0.0; 3];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(output[..2].iter().all(|sample| *sample == 0.0));
        assert!((output[2] - 0.25).abs() < 1e-5);
        assert_eq!(active_voice_frame(&mixer, 0), Some(8));
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn stopping_first_started_pad_keeps_masterclock_phase_for_future_triggers() {
        let mut mixer = RtMixer::new(1, 10.0);
        for id in 0..3 {
            mixer.load_sample(id, create_test_sample(1, 64, 0.25));
        }
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.set_downbeat_frame(0);
        transport.advance_by_rendered_frames(6);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            callback_start_frame,
            ScheduledCommand::StopSample { id: 0 },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(active_voice_frame(&mixer, 0), None);
        assert!(active_voice_frame(&mixer, 1).is_some());

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            2,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        assert_eq!(active_voice_frame(&mixer, 2), None);
    }

    #[test]
    fn quantized_two_bar_later_trigger_preserves_global_offset() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.load_sample(1, create_test_sample(1, 64, 0.25));
        mixer.set_pad_loop_region(1, 0.7, Some(5.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            0,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            0,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert_eq!(active_voice_frame(&mixer, 0), Some(0));

        transport.advance_by_rendered_frames(80);
        let callback_start_frame = transport.output_frame();
        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(callback_start_frame, 80);
        assert!(scheduler.is_empty());
        assert!(active_voice_frame(&mixer, 0).is_some());
        assert_eq!(active_voice_frame(&mixer, 1), Some(7));
    }

    #[test]
    fn multi_loop_remains_stable_when_any_one_of_five_pads_stops() {
        let mut mixer = RtMixer::new(1, 10.0);
        for id in 0..6 {
            mixer.load_sample(id, create_test_sample(1, 64, 0.25));
        }
        for id in 0..5 {
            assert!(mixer.play_sample(id, 1.0));
        }

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(6);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_immediate_command(
            &mut scheduler,
            callback_start_frame,
            ScheduledCommand::StopSample { id: 2 },
            &mut mixer,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );
        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            5,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(8));
        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
        for id in [0, 1, 3, 4] {
            assert!(active_voice_frame(&mixer, id).is_some());
        }
        assert_eq!(active_voice_frame(&mixer, 2), None);
        assert_eq!(active_voice_frame(&mixer, 5), None);
    }

    #[test]
    fn bootstrap_retains_accepted_master_bpm_even_when_reference_rate_is_clipped() {
        for master_bpm in [123.45, 400.0] {
            let mut mixer = RtMixer::new(1, 10.0);
            mixer.load_sample(0, create_test_sample(1, 200, 0.5));
            mixer.set_pad_bpm(0, Some(87.65));
            mixer.set_master_bpm(master_bpm);
            mixer.set_bpm_lock(true);
            assert!(mixer.play_sample(0, 1.0));
            let mut transport = TransportTimeline::new(10);
            transport.request_bootstrap(0);
            assert!(try_bootstrap_transport_from_reference(
                &mixer,
                &mut transport,
                0
            ));
            assert_eq!(transport.master_bpm(), Some(master_bpm));
        }
    }

    #[test]
    fn deliberate_sync_retains_complete_source_beat_and_consumes_bootstrap() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_loop_region(0, 18.5, Some(19.0));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        transport.advance_by_rendered_frames(100);
        transport.request_bootstrap(1);
        assert!(anchor_transport_phase_from_pad(&mixer, &mut transport, 0));
        assert_eq!(transport.beat_position(), Some(18.5));
        assert_eq!(transport.bar_phase_beats(), Some(2.5));
        transport.request_bootstrap(0);
        assert_eq!(transport.bootstrap_reference(), None);
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
    }

    #[test]
    fn reference_bootstrap_uses_same_loop_edit_position_as_renderer() {
        for (rendered, expected_start) in [(30, 30), (80, 20)] {
            let mut mixer = RtMixer::new(1, 10.0);
            mixer.load_sample(0, create_test_sample(1, 200, 0.5));
            mixer.set_master_bpm(60.0);
            mixer.set_pad_bpm(0, Some(60.0));
            mixer.set_bpm_lock(true);
            assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
            let mut scheduler = FixedCapacityScheduler::<8>::new();
            let mut transport = TransportTimeline::new(10);
            transport.set_master_bpm(60.0);
            let mut messages = Vec::new();
            let mut peaks = [0.0; NUM_SAMPLES];
            render_scheduled_audio(
                &mut mixer,
                &mut scheduler,
                &mut vec![0.0; rendered],
                &mut peaks,
                0,
                1,
                &mut transport,
                &mut messages,
                &mut ImmediateAudioBufferRetirement,
            );
            assert_eq!(active_voice_frame(&mixer, 0), Some(rendered));
            mixer.set_pad_loop_region(0, 2.0, Some(6.0));
            transport.request_bootstrap(0);
            assert!(try_bootstrap_transport_from_reference(
                &mixer,
                &mut transport,
                rendered as u64
            ));
            assert_eq!(
                transport.beat_position_at_frame(rendered as u64),
                Some(expected_start as f64 / 10.0)
            );
            render_scheduled_audio(
                &mut mixer,
                &mut scheduler,
                &mut [0.0; 1],
                &mut peaks,
                rendered as u64,
                1,
                &mut transport,
                &mut messages,
                &mut ImmediateAudioBufferRetirement,
            );
            assert_eq!(active_voice_frame(&mixer, 0), Some(expected_start + 1));
        }
    }

    #[test]
    fn selected_bootstrap_waits_for_parameter_batch_and_keeps_other_playheads() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.load_sample(1, create_test_sample(1, 200, 0.25));
        mixer.set_pad_loop_region(0, 1.0, Some(9.0));
        mixer.set_pad_loop_region(1, 2.0, Some(10.0));
        assert!(mixer.play_sample(0, 1.0));
        assert!(mixer.play_sample(1, 1.0));
        let mut transport = TransportTimeline::new(10);
        transport.advance_by_rendered_frames(100);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut quantization = TriggerQuantization::Immediate;
        let mut messages = Vec::new();
        for command in [
            ControlMessage::SetBpmLock(true),
            ControlMessage::SetPadTimingMetadata {
                id: 0,
                metadata: PadTimingMetadata {
                    phase_anchor_s: -0.5,
                },
            },
            ControlMessage::BootstrapTransportFromPad { id: 0 },
        ] {
            process_control_message(
                command.clone(),
                &mut scheduler,
                100,
                &mut quantization,
                &mut transport,
                &mut mixer,
                &mut messages,
                &mut ImmediateAudioBufferRetirement,
            );
        }
        assert_eq!(transport.master_bpm(), Some(120.0));
        assert_eq!(transport.beat_position(), Some(20.0));
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
        let (mut parameters, mut consumer) = RingBuffer::new(8);
        parameters
            .push(ControlParameterMessage::SetPadBpm {
                id: 0,
                bpm: Some(60.0),
            })
            .unwrap();
        parameters
            .push(ControlParameterMessage::SetMasterBpm(90.0))
            .unwrap();
        drain_parameter_messages(&mut consumer, &mut mixer, &mut transport);
        assert!(try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
        assert_eq!(transport.master_bpm(), Some(90.0));
        assert_eq!(transport.beat_position(), Some(1.5));
        assert_eq!(active_voice_frame(&mixer, 0), Some(10));
        assert_eq!(active_voice_frame(&mixer, 1), Some(20));
        // Subsequent edits, silence and restarts cannot consume another bootstrap.
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 8.0,
            },
        );
        mixer.stop_sample(0);
        mixer.stop_sample(1);
        assert!(mixer.play_sample(0, 1.0));
        transport.request_bootstrap(0);
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            100
        ));
        assert_eq!(transport.beat_position(), Some(1.5));
    }

    #[test]
    fn stopped_selected_reference_bootstraps_at_its_scheduled_start_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_master_bpm(60.0);
        mixer.set_bpm_lock(true);
        mixer.set_pad_loop_region(0, 3.0, Some(11.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: -0.5,
            },
        );
        let mut transport = TransportTimeline::new(10);
        transport.set_master_bpm(60.0);
        transport.advance_by_rendered_frames(4);
        transport.request_bootstrap(0);
        assert!(!try_bootstrap_transport_from_reference(
            &mixer,
            &mut transport,
            4
        ));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        scheduler
            .schedule(
                10,
                ScheduledCommand::PlaySample {
                    expected_source_generation: None,
                    id: 0,
                    volume: 1.0,
                    received_at_ns: Some(123),
                },
            )
            .unwrap();
        let mut output = [0.0; 8];
        let mut peaks = [0.0; NUM_SAMPLES];
        let mut messages = Vec::new();
        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut peaks,
            4,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(transport.beat_position_at_frame(10), Some(3.5));
        assert_eq!(transport.output_frame(), 4);
        assert_eq!(active_voice_frame(&mixer, 0), Some(32));
        assert_eq!(transport.bootstrap_reference(), None);
    }

    #[test]
    fn pending_start_retains_target_and_capture_across_bpm_and_grid_edits() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 200, 0.5));
        mixer.set_pad_loop_region(0, 2.0, Some(10.0));
        let mut transport = TransportTimeline::new(10);
        transport.set_master_bpm(60.0);
        transport.advance_by_rendered_frames(4);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let command = ScheduledCommand::PlaySample {
            expected_source_generation: None,
            id: 0,
            volume: 1.0,
            received_at_ns: Some(123),
        };
        let accepted = scheduler
            .schedule(
                transport
                    .next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap())
                    .unwrap(),
                command.clone(),
            )
            .unwrap();
        assert_eq!(accepted.target_frame, 10);
        assert!(transport.set_master_bpm_preserving_beat_position_at_frame(120.0, 4));
        mixer.set_master_bpm(120.0);
        mixer.set_pad_bpm(0, Some(80.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: -0.5,
            },
        );
        mixer.set_pad_loop_region(0, 1.0, Some(9.0));
        assert_eq!(scheduler.peek_next_target_frame(), Some(10));
        assert_eq!(
            transport.next_grid_frame(QuantizeGrid::from_step_64ths(16).unwrap()),
            Some(7)
        );
        let due = scheduler.pop_due_through(4, 10).unwrap();
        assert_eq!(due.target_frame, accepted.target_frame);
        assert_eq!(due.command, command);
        let mut messages = Vec::new();
        execute_scheduled_command(
            &mut mixer,
            &mut transport,
            due.execution_frame,
            due.command,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );
        assert_eq!(active_voice_frame(&mixer, 0), Some(10));
    }

    #[test]
    fn bpm_lock_phase_anchor_updates_transport_downbeat_from_active_pad() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        mixer.set_pad_timing_metadata(
            0,
            PadTimingMetadata {
                phase_anchor_s: 0.5,
            },
        );
        assert!(mixer.play_sample(0, 1.0));

        let mut output = vec![0.0; 30];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        mixer.render(&mut output, &mut pad_peaks);

        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(100);

        assert!(anchor_transport_phase_from_pad(&mixer, &mut transport, 0));

        assert_eq!(transport.downbeat_frame(), 75);
        assert_eq!(transport.bar_phase_beats(), Some(2.5));
    }

    #[test]
    fn bpm_lock_phase_anchor_keeps_transport_downbeat_when_anchor_is_inactive() {
        let mixer = RtMixer::new(1, 10.0);
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.set_downbeat_frame(12);
        transport.advance_by_rendered_frames(100);

        assert!(!anchor_transport_phase_from_pad(&mixer, &mut transport, 0));

        assert_eq!(transport.downbeat_frame(), 12);
    }

    #[test]
    fn phase_anchor_establishes_transport_clock_without_existing_master_bpm() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.set_pad_bpm(0, Some(60.0));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        transport.clear_master_bpm();
        transport.set_downbeat_frame(12);

        assert!(anchor_transport_phase_from_pad(&mixer, &mut transport, 0));

        assert_eq!(transport.master_bpm(), Some(60.0));
        assert_eq!(transport.downbeat_frame(), 0);
    }

    #[test]
    fn scheduler_full_quantized_play_leaves_current_playback_unchanged() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<0>::new();
        let mut messages = Vec::new();

        schedule_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());
    }

    #[test]
    fn immediate_exclusive_play_stops_all_then_starts_at_current_frame() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        mixer.set_pad_bpm(1, Some(60.0));
        mixer.set_pad_timing_metadata(
            1,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Immediate,
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        assert_eq!(active_voice_frame(&mixer, 1), Some(0));
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }

    #[test]
    fn quantized_exclusive_play_starts_target_at_loop_start() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 64, 0.5));
        mixer.load_sample(1, create_test_sample(1, 64, 0.25));
        mixer.set_pad_loop_region(1, 0.7, Some(5.0));
        mixer.set_pad_bpm(1, Some(60.0));
        mixer.set_pad_timing_metadata(
            1,
            PadTimingMetadata {
                phase_anchor_s: 2.0,
            },
        );
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(40);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert_eq!(active_voice_frame(&mixer, 1), Some(7));
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }

    #[test]
    fn quantized_exclusive_play_switches_pads_at_target_offset() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(4);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert_eq!(scheduler.peek_next_target_frame(), Some(5));
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());

        let mut output = vec![0.0; 4];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            callback_start_frame,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!((output[0] - 0.5).abs() < 1e-5);
        assert!(
            output[1..]
                .iter()
                .all(|sample| (*sample - 0.25).abs() < 1e-5)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }

    #[test]
    fn scheduler_full_quantized_exclusive_play_leaves_current_playback_unchanged() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        assert!(transport.set_master_bpm(60.0));
        transport.advance_by_rendered_frames(5);
        let callback_start_frame = transport.output_frame();
        let mut scheduler = FixedCapacityScheduler::<0>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            callback_start_frame,
            TriggerQuantization::Grid { step_64ths: 4 },
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());
    }

    #[test]
    fn exclusive_play_rejects_missing_target_without_stopping_current_playback() {
        let mut mixer = RtMixer::new(1, 10.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        assert!(mixer.play_sample(0, 1.0));
        let mut transport = TransportTimeline::new(10);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut messages = Vec::new();

        schedule_exclusive_play_sample_command(
            &mut scheduler,
            transport.output_frame(),
            TriggerQuantization::Immediate,
            &mut transport,
            1,
            1.0,
            &mut mixer,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
            None,
        );

        assert!(scheduler.is_empty());
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 1)
        );
        assert!(messages.is_empty());
    }

    #[test]
    fn scheduled_start_inside_buffer_renders_at_target_offset() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(
                4,
                ScheduledCommand::PlaySample {
                    expected_source_generation: None,
                    id: 0,
                    volume: 1.0,
                    received_at_ns: None,
                },
            )
            .unwrap();
        let mut output = vec![0.0; 8];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(output[..4].iter().all(|sample| *sample == 0.0));
        assert!(
            output[4..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert!((pad_peaks[0] - 0.5).abs() < 1e-5);
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn scheduled_start_inside_oversized_buffer_preserves_target_offset() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 1_000, 0.5));
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(
                600,
                ScheduledCommand::PlaySample {
                    expected_source_generation: None,
                    id: 0,
                    volume: 1.0,
                    received_at_ns: None,
                },
            )
            .unwrap();
        let mut output = vec![0.0; 700];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(output[..600].iter().all(|sample| *sample == 0.0));
        assert!(
            output[600..]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert_started(&messages, 0, 0);
    }

    #[test]
    fn scheduled_stop_inside_buffer_silences_after_target_offset() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(4, ScheduledCommand::StopSample { id: 0 })
            .unwrap();
        let mut output = vec![0.0; 8];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(
            output[..4]
                .iter()
                .all(|sample| (*sample - 0.5).abs() < 1e-5)
        );
        assert!(output[4..].iter().all(|sample| *sample == 0.0));
        assert!((pad_peaks[0] - 0.5).abs() < 1e-5);
        assert_stopped(&messages, 0, 0);
    }

    #[test]
    fn scheduled_render_tracks_pad_activity_across_split_segments() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.5));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler
            .schedule(4, ScheduledCommand::StopSample { id: 0 })
            .unwrap();
        let mut output = vec![0.0; 8];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut pad_activity = RtRenderPadActivity::default();
        let mut messages = Vec::new();

        render_scheduled_audio_tracking_pads(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            &mut pad_activity,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(pad_activity.contains(0));
        assert_eq!(pad_activity.len(), 1);
        assert!((pad_peaks[0] - 0.5).abs() < 1e-5);
        assert_eq!(mixer.pad_playhead_seconds(0), None);
        assert_stopped(&messages, 0, 0);
    }

    #[test]
    fn same_frame_stop_all_and_start_preserve_stable_order() {
        let mut mixer = RtMixer::new(1, 44_100.0);
        mixer.load_sample(0, create_test_sample(1, 32, 0.8));
        mixer.load_sample(1, create_test_sample(1, 32, 0.25));
        mixer.play_sample(0, 1.0);
        let mut scheduler = FixedCapacityScheduler::<8>::new();
        let mut transport = TransportTimeline::new(44_100);
        scheduler.schedule(0, ScheduledCommand::StopAll).unwrap();
        scheduler
            .schedule(
                0,
                ScheduledCommand::PlaySample {
                    expected_source_generation: None,
                    id: 1,
                    volume: 1.0,
                    received_at_ns: None,
                },
            )
            .unwrap();
        let mut output = vec![0.0; 4];
        let mut pad_peaks = [0.0_f32; NUM_SAMPLES];
        let mut messages = Vec::new();

        render_scheduled_audio(
            &mut mixer,
            &mut scheduler,
            &mut output,
            &mut pad_peaks,
            0,
            1,
            &mut transport,
            &mut messages,
            &mut ImmediateAudioBufferRetirement,
        );

        assert!(
            mixer
                .voices
                .iter()
                .all(|voice| !voice.active || voice.sample_id != 0)
        );
        assert!(
            mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 1)
        );
        assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 1e-5));
        assert_stopped(&messages, 0, 0);
        assert_started(&messages, 1, 1);
    }
}

#[cfg(test)]
#[path = "source_callback_close_tests.rs"]
mod source_callback_close_tests;

#[cfg(all(test, windows))]
#[path = "pitch_callback_lifecycle_tests.rs"]
mod pitch_callback_lifecycle_tests;

#[cfg(test)]
#[path = "parameter_placement_tests.rs"]
mod parameter_placement_tests;

#[cfg(test)]
#[path = "content_feedback_tests.rs"]
mod content_feedback_tests;

#[cfg(test)]
pub(super) fn after_pad_feedback_effect_for_test() {
    content_feedback_tests::after_effect();
}

#[cfg(test)]
#[path = "pair_saved_timing_tests.rs"]
mod pair_saved_timing_tests;

#[cfg(all(test, windows))]
#[path = "held_lifecycle_tests.rs"]
mod held_lifecycle_tests;

#[cfg(all(test, windows))]
#[path = "failed_stream_tests.rs"]
mod failed_stream_tests;
