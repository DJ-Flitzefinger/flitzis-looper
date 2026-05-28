use crate::audio_engine::constants::NUM_SAMPLES;
use crate::messages::ControlMessage;
use midir::{Ignore, MidiInput, MidiInputConnection};
use rtrb::Producer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const INPUT_QUEUE_CAPACITY: usize = 1024;
const MIDI_NRPN_MSB_CC: u8 = 99;
const MIDI_NRPN_LSB_CC: u8 = 98;
const MIDI_RPN_MSB_CC: u8 = 101;
const MIDI_RPN_LSB_CC: u8 = 100;
const MIDI_DATA_INCREMENT_CC: u8 = 96;
const MIDI_DATA_DECREMENT_CC: u8 = 97;
const MIDI_RELATIVE_INCREMENT_VALUE: u8 = 65;
const MIDI_RELATIVE_DECREMENT_VALUE: u8 = 63;
const MIDI_INC_DEC_VALUE_KEY_MIN: u8 = 2;
const MIDI_NRPN_ACTIVE_TTL_NS: u64 = 250_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MidiBindingKind {
    Note,
    ControlChange,
    Nrpn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MidiBinding {
    pub kind: MidiBindingKind,
    pub channel: u8,
    pub number: u16,
}

impl MidiBinding {
    pub fn key(&self) -> String {
        let kind = match self.kind {
            MidiBindingKind::Note => "note",
            MidiBindingKind::ControlChange => "cc",
            MidiBindingKind::Nrpn => "nrpn",
        };
        format!("midi:{kind}:{}:{}", self.channel, self.number)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputAction {
    TriggerPad { id: usize },
    StopPad { id: usize },
    StopAll,
    Python { action_key: String },
}

#[derive(Debug, Clone)]
struct InputMapping {
    binding_key: String,
    action_key: String,
    action: InputAction,
}

#[derive(Debug, Clone)]
struct RuntimePadState {
    loaded: bool,
    loop_start_s: f32,
    loop_end_s: Option<f32>,
}

impl Default for RuntimePadState {
    fn default() -> Self {
        Self {
            loaded: false,
            loop_start_s: 0.0,
            loop_end_s: None,
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeState {
    multi_loop: bool,
    pads: Vec<RuntimePadState>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            multi_loop: false,
            pads: vec![RuntimePadState::default(); NUM_SAMPLES],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedMidiEvent {
    pub binding: MidiBinding,
    pub value: u8,
    pub received_at_ns: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct InputRuntimeEvent {
    pub binding_key: String,
    pub value: u8,
    pub received_at_ns: u64,
    pub action_key: Option<String>,
    pub dispatched: bool,
    pub direct: bool,
}

pub(crate) struct InputRuntime {
    origin: Instant,
    enabled: Arc<AtomicBool>,
    input_tx: SyncSender<NormalizedMidiEvent>,
    event_rx: Mutex<Receiver<InputRuntimeEvent>>,
    mappings: Arc<Mutex<Vec<InputMapping>>>,
    runtime_state: Arc<Mutex<RuntimeState>>,
    test_normalizer: Mutex<MidiNormalizer>,
    midi_connections: Mutex<Vec<MidiInputConnection<()>>>,
    running: Arc<AtomicBool>,
    dispatcher: Mutex<Option<JoinHandle<()>>>,
}

impl InputRuntime {
    pub fn new(audio_producer: Arc<Mutex<Producer<ControlMessage>>>) -> Self {
        let origin = Instant::now();
        let enabled = Arc::new(AtomicBool::new(false));
        let mappings = Arc::new(Mutex::new(Vec::new()));
        let runtime_state = Arc::new(Mutex::new(RuntimeState::default()));
        let running = Arc::new(AtomicBool::new(true));
        let (input_tx, input_rx) = sync_channel(INPUT_QUEUE_CAPACITY);
        let (event_tx, event_rx) = sync_channel(INPUT_QUEUE_CAPACITY);

        let dispatcher = {
            let enabled = enabled.clone();
            let mappings = mappings.clone();
            let runtime_state = runtime_state.clone();
            let running = running.clone();
            thread::spawn(move || {
                input_dispatch_loop(
                    input_rx,
                    event_tx,
                    mappings,
                    runtime_state,
                    audio_producer,
                    enabled,
                    running,
                );
            })
        };

        Self {
            origin,
            enabled,
            input_tx,
            event_rx: Mutex::new(event_rx),
            mappings,
            runtime_state,
            test_normalizer: Mutex::new(MidiNormalizer::default()),
            midi_connections: Mutex::new(Vec::new()),
            running,
            dispatcher: Mutex::new(Some(dispatcher)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    pub fn replace_mappings(&self, mappings: Vec<(String, String)>) {
        let parsed = mappings
            .into_iter()
            .map(|(binding_key, action_key)| {
                let action = parse_input_action(&action_key);
                InputMapping {
                    binding_key,
                    action_key,
                    action,
                }
            })
            .collect();

        if let Ok(mut guard) = self.mappings.lock() {
            *guard = parsed;
        }
    }

    pub fn set_runtime_state(
        &self,
        multi_loop: bool,
        loaded: Vec<bool>,
        loop_starts: Vec<f32>,
        loop_ends: Vec<Option<f32>>,
    ) -> Result<(), String> {
        if loaded.len() != NUM_SAMPLES
            || loop_starts.len() != NUM_SAMPLES
            || loop_ends.len() != NUM_SAMPLES
        {
            return Err(format!(
                "input runtime state arrays must have length {NUM_SAMPLES}"
            ));
        }

        let mut pads = Vec::with_capacity(NUM_SAMPLES);
        for idx in 0..NUM_SAMPLES {
            let start_s = loop_starts[idx];
            let end_s = loop_ends[idx];
            if !start_s.is_finite() || start_s < 0.0 {
                return Err("loop start out of range".to_string());
            }
            if end_s.is_some_and(|value| !value.is_finite() || value < 0.0) {
                return Err("loop end out of range".to_string());
            }
            pads.push(RuntimePadState {
                loaded: loaded[idx],
                loop_start_s: start_s,
                loop_end_s: end_s,
            });
        }

        if let Ok(mut guard) = self.runtime_state.lock() {
            *guard = RuntimeState { multi_loop, pads };
        }
        Ok(())
    }

    pub fn start_midi_input(&self) -> Result<usize, String> {
        let port_count = {
            let probe =
                MidiInput::new("flitzis-looper-midi-probe").map_err(|err| err.to_string())?;
            probe.ports().len()
        };

        let mut connections = self
            .midi_connections
            .lock()
            .map_err(|_| "failed to acquire MIDI connection lock".to_string())?;
        connections.clear();

        for index in 0..port_count {
            let mut midi_in = MidiInput::new(&format!("flitzis-looper-midi-{index}"))
                .map_err(|err| err.to_string())?;
            midi_in.ignore(Ignore::None);
            let ports = midi_in.ports();
            let Some(port) = ports.get(index) else {
                continue;
            };

            let input_tx = self.input_tx.clone();
            let origin = self.origin;
            let mut normalizer = MidiNormalizer::default();
            let connection = midi_in
                .connect(
                    port,
                    &format!("flitzis-looper-input-{index}"),
                    move |_backend_stamp, message, _| {
                        let received_at_ns = monotonic_ns_since(origin);
                        if let Some(event) = normalizer.normalize(message, received_at_ns) {
                            let _ = input_tx.try_send(event);
                        }
                    },
                    (),
                )
                .map_err(|err| err.to_string())?;
            connections.push(connection);
        }

        Ok(connections.len())
    }

    pub fn stop_midi_input(&self) {
        if let Ok(mut connections) = self.midi_connections.lock() {
            connections.clear();
        }
    }

    pub fn inject_midi_message(&self, message: &[u8]) -> bool {
        let received_at_ns = monotonic_ns_since(self.origin);
        let Ok(mut normalizer) = self.test_normalizer.lock() else {
            return false;
        };
        let Some(event) = normalizer.normalize(message, received_at_ns) else {
            return false;
        };
        self.input_tx.try_send(event).is_ok()
    }

    pub fn poll_event(&self) -> Option<InputRuntimeEvent> {
        let Ok(rx) = self.event_rx.lock() else {
            return None;
        };
        rx.try_recv().ok()
    }
}

impl Drop for InputRuntime {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Ok(mut connections) = self.midi_connections.lock() {
            connections.clear();
        }
        if let Ok(mut guard) = self.dispatcher.lock()
            && let Some(dispatcher) = guard.take()
        {
            let _ = dispatcher.join();
        }
    }
}

fn input_dispatch_loop(
    input_rx: Receiver<NormalizedMidiEvent>,
    event_tx: SyncSender<InputRuntimeEvent>,
    mappings: Arc<Mutex<Vec<InputMapping>>>,
    runtime_state: Arc<Mutex<RuntimeState>>,
    audio_producer: Arc<Mutex<Producer<ControlMessage>>>,
    enabled: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    while running.load(Ordering::Acquire) {
        let event = match input_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };

        let binding_key = event.binding.key();
        let mapping = if enabled.load(Ordering::Acquire) {
            lookup_mapping(&mappings, &binding_key)
        } else {
            None
        };

        let mut dispatched = false;
        let mut direct = false;
        let action_key = mapping.as_ref().map(|mapping| mapping.action_key.clone());

        if let Some(mapping) = mapping {
            let result = dispatch_action(&mapping.action, &runtime_state, &audio_producer);
            dispatched = result.dispatched;
            direct = result.direct;
        }

        let _ = event_tx.try_send(InputRuntimeEvent {
            binding_key,
            value: event.value,
            received_at_ns: event.received_at_ns,
            action_key,
            dispatched,
            direct,
        });
    }
}

fn lookup_mapping(
    mappings: &Arc<Mutex<Vec<InputMapping>>>,
    binding_key: &str,
) -> Option<InputMapping> {
    let Ok(guard) = mappings.lock() else {
        return None;
    };
    guard
        .iter()
        .find(|mapping| mapping.binding_key == binding_key)
        .cloned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DispatchResult {
    dispatched: bool,
    direct: bool,
}

fn dispatch_action(
    action: &InputAction,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    audio_producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> DispatchResult {
    match action {
        InputAction::TriggerPad { id } => dispatch_trigger_pad(*id, runtime_state, audio_producer),
        InputAction::StopPad { id } => {
            dispatch_audio_messages(audio_producer, [ControlMessage::StopSample { id: *id }])
        }
        InputAction::StopAll => {
            dispatch_audio_messages(audio_producer, [ControlMessage::StopAll()])
        }
        InputAction::Python { action_key } => {
            let _ = action_key.len();
            DispatchResult {
                dispatched: true,
                direct: false,
            }
        }
    }
}

fn dispatch_trigger_pad(
    id: usize,
    runtime_state: &Arc<Mutex<RuntimeState>>,
    audio_producer: &Arc<Mutex<Producer<ControlMessage>>>,
) -> DispatchResult {
    let Ok(state) = runtime_state.lock() else {
        return DispatchResult {
            dispatched: false,
            direct: true,
        };
    };
    let Some(pad) = state.pads.get(id) else {
        return DispatchResult {
            dispatched: false,
            direct: true,
        };
    };
    if !pad.loaded {
        return DispatchResult {
            dispatched: false,
            direct: true,
        };
    }

    let play = if state.multi_loop {
        ControlMessage::PlaySample { id, volume: 1.0 }
    } else {
        ControlMessage::PlaySampleExclusive { id, volume: 1.0 }
    };
    let loop_region = ControlMessage::SetPadLoopRegion {
        id,
        start_s: pad.loop_start_s,
        end_s: pad.loop_end_s,
    };
    drop(state);

    dispatch_audio_messages(audio_producer, [loop_region, play])
}

fn dispatch_audio_messages<const N: usize>(
    audio_producer: &Arc<Mutex<Producer<ControlMessage>>>,
    messages: [ControlMessage; N],
) -> DispatchResult {
    let Ok(mut producer) = audio_producer.try_lock() else {
        return DispatchResult {
            dispatched: false,
            direct: true,
        };
    };

    if producer.slots() < N {
        return DispatchResult {
            dispatched: false,
            direct: true,
        };
    }

    for message in messages {
        if producer.push(message).is_err() {
            return DispatchResult {
                dispatched: false,
                direct: true,
            };
        }
    }

    DispatchResult {
        dispatched: true,
        direct: true,
    }
}

fn parse_input_action(action_key: &str) -> InputAction {
    if let Some(id) = action_key
        .strip_prefix("pad.trigger:")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return InputAction::TriggerPad { id };
    }
    if let Some(id) = action_key
        .strip_prefix("pad.stop:")
        .and_then(|value| value.parse::<usize>().ok())
    {
        return InputAction::StopPad { id };
    }
    if action_key == "global.stop_all" {
        return InputAction::StopAll;
    }
    InputAction::Python {
        action_key: action_key.to_string(),
    }
}

#[cfg(test)]
fn normalize_midi_message(message: &[u8], received_at_ns: u64) -> Option<NormalizedMidiEvent> {
    MidiNormalizer::default().normalize(message, received_at_ns)
}

#[derive(Debug, Clone, Default)]
struct MidiNormalizer {
    nrpn_parameters: [NrpnParameter; 16],
}

impl MidiNormalizer {
    fn normalize(&mut self, message: &[u8], received_at_ns: u64) -> Option<NormalizedMidiEvent> {
        let status = *message.first()?;
        let message_type = status & 0xF0;
        let channel = (status & 0x0F) + 1;
        let channel_index = usize::from(channel - 1);

        match message_type {
            0x90 => {
                if message.len() < 3 || message[2] == 0 {
                    return None;
                }
                Some(NormalizedMidiEvent {
                    binding: MidiBinding {
                        kind: MidiBindingKind::Note,
                        channel,
                        number: u16::from(message[1]),
                    },
                    value: message[2],
                    received_at_ns,
                })
            }
            0xB0 => {
                if message.len() < 3 {
                    return None;
                }

                let controller = message[1];
                let data_value = message[2];
                if controller == MIDI_NRPN_MSB_CC {
                    self.nrpn_parameters[channel_index].set_msb(data_value, received_at_ns);
                    return None;
                }
                if controller == MIDI_NRPN_LSB_CC {
                    self.nrpn_parameters[channel_index].set_lsb(data_value, received_at_ns);
                    return None;
                }
                if controller == MIDI_RPN_MSB_CC || controller == MIDI_RPN_LSB_CC {
                    self.nrpn_parameters[channel_index].clear();
                    return None;
                }
                if controller == MIDI_DATA_INCREMENT_CC || controller == MIDI_DATA_DECREMENT_CC {
                    let value = if controller == MIDI_DATA_INCREMENT_CC {
                        MIDI_RELATIVE_INCREMENT_VALUE
                    } else {
                        MIDI_RELATIVE_DECREMENT_VALUE
                    };
                    if data_value < MIDI_INC_DEC_VALUE_KEY_MIN
                        && let Some(parameter) =
                            self.nrpn_parameters[channel_index].active_number(received_at_ns)
                    {
                        self.nrpn_parameters[channel_index].touch(received_at_ns);
                        return Some(NormalizedMidiEvent {
                            binding: MidiBinding {
                                kind: MidiBindingKind::Nrpn,
                                channel,
                                number: parameter,
                            },
                            value,
                            received_at_ns,
                        });
                    }

                    return Some(NormalizedMidiEvent {
                        binding: MidiBinding {
                            kind: MidiBindingKind::ControlChange,
                            channel,
                            number: u16::from(data_value),
                        },
                        value,
                        received_at_ns,
                    });
                }

                self.nrpn_parameters[channel_index].clear();
                Some(NormalizedMidiEvent {
                    binding: MidiBinding {
                        kind: MidiBindingKind::ControlChange,
                        channel,
                        number: u16::from(controller),
                    },
                    value: data_value,
                    received_at_ns,
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct NrpnParameter {
    msb: Option<u8>,
    lsb: Option<u8>,
    selected_at_ns: Option<u64>,
}

impl NrpnParameter {
    fn set_msb(&mut self, value: u8, received_at_ns: u64) {
        self.msb = Some(value);
        self.selected_at_ns = Some(received_at_ns);
    }

    fn set_lsb(&mut self, value: u8, received_at_ns: u64) {
        self.lsb = Some(value);
        self.selected_at_ns = Some(received_at_ns);
    }

    fn number(&self) -> Option<u16> {
        Some(u16::from(self.msb?) * 128 + u16::from(self.lsb?))
    }

    fn active_number(&self, received_at_ns: u64) -> Option<u16> {
        let selected_at_ns = self.selected_at_ns?;
        if received_at_ns.saturating_sub(selected_at_ns) > MIDI_NRPN_ACTIVE_TTL_NS {
            return None;
        }
        self.number()
    }

    fn touch(&mut self, received_at_ns: u64) {
        self.selected_at_ns = Some(received_at_ns);
    }

    fn clear(&mut self) {
        self.msb = None;
        self.lsb = None;
        self.selected_at_ns = None;
    }
}

fn monotonic_ns_since(origin: Instant) -> u64 {
    let nanos = origin.elapsed().as_nanos();
    nanos.min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    #[test]
    fn note_on_velocity_positive_normalizes_with_timestamp() {
        let event = normalize_midi_message(&[0x90, 60, 100], 42).expect("normalized");

        assert_eq!(
            event.binding,
            MidiBinding {
                kind: MidiBindingKind::Note,
                channel: 1,
                number: 60
            }
        );
        assert_eq!(event.binding.key(), "midi:note:1:60");
        assert_eq!(event.value, 100);
        assert_eq!(event.received_at_ns, 42);
    }

    #[test]
    fn control_change_normalizes_with_channel() {
        let event = normalize_midi_message(&[0xB3, 7, 99], 100).expect("normalized");

        assert_eq!(
            event.binding,
            MidiBinding {
                kind: MidiBindingKind::ControlChange,
                channel: 4,
                number: 7
            }
        );
        assert_eq!(event.binding.key(), "midi:cc:4:7");
        assert_eq!(event.value, 99);
    }

    #[test]
    fn nrpn_increment_decrement_normalizes_to_stable_binding() {
        let mut normalizer = MidiNormalizer::default();

        assert!(normalizer.normalize(&[0xB0, 99, 0], 10).is_none());
        assert!(normalizer.normalize(&[0xB0, 98, 2], 11).is_none());

        let increment = normalizer
            .normalize(&[0xB0, 96, 1], 12)
            .expect("normalized increment");
        assert_eq!(
            increment.binding,
            MidiBinding {
                kind: MidiBindingKind::Nrpn,
                channel: 1,
                number: 2
            }
        );
        assert_eq!(increment.binding.key(), "midi:nrpn:1:2");
        assert_eq!(increment.value, 65);

        let decrement = normalizer
            .normalize(&[0xB0, 97, 1], 13)
            .expect("normalized decrement");
        assert_eq!(decrement.binding.key(), "midi:nrpn:1:2");
        assert_eq!(decrement.value, 63);
    }

    #[test]
    fn standalone_inc_dec_messages_use_data_byte_as_relative_cc_identity() {
        let mut normalizer = MidiNormalizer::default();

        let increment = normalizer
            .normalize(&[0xB0, 96, 1], 1)
            .expect("normalized increment");
        assert_eq!(increment.binding.key(), "midi:cc:1:1");
        assert_eq!(increment.value, 65);

        let decrement = normalizer
            .normalize(&[0xB0, 97, 1], 2)
            .expect("normalized decrement");
        assert_eq!(decrement.binding.key(), "midi:cc:1:1");
        assert_eq!(decrement.value, 63);

        let second_knob = normalizer
            .normalize(&[0xB0, 96, 2], 3)
            .expect("normalized second knob");
        assert_eq!(second_knob.binding.key(), "midi:cc:1:2");
        assert_eq!(second_knob.value, 65);
    }

    #[test]
    fn stale_nrpn_selection_does_not_collapse_standalone_inc_dec_knobs() {
        let mut normalizer = MidiNormalizer::default();

        assert!(normalizer.normalize(&[0xB0, 99, 0], 10).is_none());
        assert!(normalizer.normalize(&[0xB0, 98, 0], 11).is_none());

        let event = normalizer
            .normalize(&[0xB0, 96, 3], 11 + MIDI_NRPN_ACTIVE_TTL_NS + 1)
            .expect("normalized standalone increment");
        assert_eq!(event.binding.key(), "midi:cc:1:3");
        assert_eq!(event.value, 65);
    }

    #[test]
    fn value_keyed_inc_dec_messages_do_not_collapse_even_after_fresh_nrpn_setup() {
        let mut normalizer = MidiNormalizer::default();

        assert!(normalizer.normalize(&[0xB0, 99, 0], 10).is_none());
        assert!(normalizer.normalize(&[0xB0, 98, 0], 11).is_none());

        let event = normalizer
            .normalize(&[0xB0, 96, 2], 12)
            .expect("normalized value-keyed increment");
        assert_eq!(event.binding.key(), "midi:cc:1:2");
        assert_eq!(event.value, 65);

        let event = normalizer
            .normalize(&[0xB0, 97, 2], 13)
            .expect("normalized value-keyed decrement");
        assert_eq!(event.binding.key(), "midi:cc:1:2");
        assert_eq!(event.value, 63);
    }

    #[test]
    fn rpn_selection_clears_pending_nrpn_parameter() {
        let mut normalizer = MidiNormalizer::default();

        assert!(normalizer.normalize(&[0xB0, 99, 0], 10).is_none());
        assert!(normalizer.normalize(&[0xB0, 98, 0], 11).is_none());
        assert!(normalizer.normalize(&[0xB0, 101, 0], 12).is_none());

        let event = normalizer
            .normalize(&[0xB0, 96, 4], 13)
            .expect("normalized standalone increment");
        assert_eq!(event.binding.key(), "midi:cc:1:4");
        assert_eq!(event.value, 65);
    }

    #[test]
    fn unsupported_midi_messages_are_dropped() {
        let messages = [
            &[0x80, 60, 0][..],
            &[0x90, 60, 0],
            &[0xF8],
            &[0xFE],
            &[0xF0, 1, 2, 0xF7],
            &[0xC0, 1],
            &[0xE0, 0, 64],
            &[0xD0, 1],
            &[0xA0, 60, 1],
        ];

        for message in messages {
            assert!(normalize_midi_message(message, 1).is_none());
        }
    }

    #[test]
    fn trigger_pad_dispatch_sends_loop_region_and_exclusive_play() {
        let (producer, mut consumer) = RingBuffer::<ControlMessage>::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        {
            let mut guard = state.lock().unwrap();
            guard.multi_loop = false;
            guard.pads[3] = RuntimePadState {
                loaded: true,
                loop_start_s: 1.25,
                loop_end_s: Some(4.0),
            };
        }

        let result = dispatch_trigger_pad(3, &state, &producer);

        assert_eq!(
            result,
            DispatchResult {
                dispatched: true,
                direct: true
            }
        );
        assert!(matches!(
            consumer.pop().unwrap(),
            ControlMessage::SetPadLoopRegion {
                id: 3,
                start_s,
                end_s: Some(4.0)
            } if start_s == 1.25
        ));
        assert!(matches!(
            consumer.pop().unwrap(),
            ControlMessage::PlaySampleExclusive { id: 3, volume: 1.0 }
        ));
    }

    #[test]
    fn trigger_pad_dispatch_uses_multiloop_play_when_enabled() {
        let (producer, mut consumer) = RingBuffer::<ControlMessage>::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        {
            let mut guard = state.lock().unwrap();
            guard.multi_loop = true;
            guard.pads[1].loaded = true;
        }

        let result = dispatch_trigger_pad(1, &state, &producer);

        assert!(result.dispatched);
        assert!(matches!(
            consumer.pop().unwrap(),
            ControlMessage::SetPadLoopRegion { id: 1, .. }
        ));
        assert!(matches!(
            consumer.pop().unwrap(),
            ControlMessage::PlaySample { id: 1, volume: 1.0 }
        ));
    }

    #[test]
    fn trigger_pad_dispatch_rejects_partial_loop_and_play_sequence() {
        let (producer, mut consumer) = RingBuffer::<ControlMessage>::new(1);
        let producer = Arc::new(Mutex::new(producer));
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        {
            let mut guard = state.lock().unwrap();
            guard.multi_loop = true;
            guard.pads[1].loaded = true;
        }

        let result = dispatch_trigger_pad(1, &state, &producer);

        assert_eq!(
            result,
            DispatchResult {
                dispatched: false,
                direct: true
            }
        );
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn trigger_pad_dispatch_rejects_unloaded_pad_without_message() {
        let (producer, mut consumer) = RingBuffer::<ControlMessage>::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let state = Arc::new(Mutex::new(RuntimeState::default()));

        let result = dispatch_trigger_pad(1, &state, &producer);

        assert_eq!(
            result,
            DispatchResult {
                dispatched: false,
                direct: true
            }
        );
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn parser_keeps_unknown_actions_for_python_dispatch() {
        let action = parse_input_action("ui.select_bank:2");

        assert!(
            matches!(action, InputAction::Python { action_key } if action_key == "ui.select_bank:2")
        );
    }

    #[test]
    fn future_dsp_parameter_actions_do_not_use_direct_dispatch() {
        let (producer, mut consumer) = RingBuffer::<ControlMessage>::new(8);
        let producer = Arc::new(Mutex::new(producer));
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let action = parse_input_action("dsp.pad.parameter.delta:0:filter.cutoff");

        let result = dispatch_action(&action, &state, &producer);

        assert_eq!(
            result,
            DispatchResult {
                dispatched: true,
                direct: false
            }
        );
        assert!(consumer.pop().is_err());
    }
}
