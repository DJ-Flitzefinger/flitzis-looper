use super::*;
use crate::audio_engine::content_placement::ContentKey;
use crate::audio_engine::pitch_actions::PitchKeyMode;
use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};
use rtrb::RingBuffer;

const INPUT_TIME_NS: u64 = 10_000_000;

struct SelectionProbe {
    state: Arc<Mutex<RuntimeState>>,
    ownership: Arc<InputRuntimeOwnership>,
    initial_revision: u64,
    keys: [ContentKey; 2],
}

struct DispatcherFixture {
    dispatcher: InputDispatcher,
    input_tx: SyncSender<NormalizedMidiEvent>,
    event_rx: Receiver<InputRuntimeEvent>,
    selection: SelectionProbe,
}

fn fixture() -> DispatcherFixture {
    let ownership = Arc::new(InputRuntimeOwnership::default());
    let output_clock = Arc::new(SharedOutputClock::new());
    output_clock.publish(OutputClockSnapshot {
        valid: true,
        observed_at_ns: INPUT_TIME_NS,
        audible_at_ns: INPUT_TIME_NS,
        output_frame: 24_000,
        sample_rate_hz: 48_000,
        master_period_seconds: Some(0.5),
        downbeat_frame: 0,
        master_beat: 1.0,
        freshness_ns: 100_000_000,
    });
    let mut state = RuntimeState {
        pitch_clock: Some(output_clock),
        ..RuntimeState::default()
    };
    for id in [0, 1] {
        let native = InputRuntimePadBinding::for_test(id, ownership.clone());
        state.pads[id] = RuntimePadState {
            loaded: true,
            binding: Some(native.binding),
            acknowledgements: Some(native.acknowledgements.clone()),
            pitch: PitchPadProjection {
                source_root: Some(4),
                source_mode: Some(PitchKeyMode::Minor),
                ..PitchPadProjection::default()
            },
            ..RuntimePadState::default()
        };
    }
    let keys = [0, 1].map(|id| {
        state.pads[id]
            .binding
            .unwrap()
            .placement
            .unwrap()
            .content_key()
            .unwrap()
    });
    assert_ne!(keys[0], keys[1]);
    let state = Arc::new(Mutex::new(state));
    let initial_revision = pitch::select_pad(0, &state, &ownership).unwrap();
    let (input_tx, input_rx) = sync_channel(INPUT_QUEUE_CAPACITY);
    let (event_tx, event_rx) = sync_channel(INPUT_QUEUE_CAPACITY);
    let (audio_producer, _audio_consumer) = RingBuffer::<ControlMessage>::new(2);
    let action_key = "ui.select_pad:1".to_string();
    let dispatcher = InputDispatcher {
        clock: InputClock::new(),
        input_rx,
        event_tx,
        mappings: Arc::new(Mutex::new(vec![InputMapping {
            binding_key: "midi:note:1:61".to_string(),
            action: parse_input_action(&action_key),
            action_key,
            owner: Arc::new(AtomicBool::new(true)),
            pitch_step_state: Arc::new(Mutex::new(pitch_steps::PitchStepState::default())),
        }])),
        runtime_state: state.clone(),
        ownership: ownership.clone(),
        audio_producer: Arc::new(Mutex::new(audio_producer)),
        enabled: Arc::new(AtomicBool::new(true)),
        learn_capture_active: Arc::new(AtomicBool::new(false)),
        stop_admission_revision: Arc::new(AtomicU64::new(0)),
        running: Arc::new(AtomicBool::new(true)),
    };
    DispatcherFixture {
        dispatcher,
        input_tx,
        event_rx,
        selection: SelectionProbe {
            state,
            ownership,
            initial_revision,
            keys,
        },
    }
}

fn note(number: u16, received_at_ns: u64) -> NormalizedMidiEvent {
    NormalizedMidiEvent {
        binding: MidiBinding {
            kind: MidiBindingKind::Note,
            channel: 1,
            number,
        },
        value: 100,
        received_at_ns,
    }
}

fn capture_selected(selection: &SelectionProbe) -> Result<CapturedPitchAction, PitchActionReject> {
    pitch::capture_selected(
        PitchActionSpec::parse("pitch.extra:2:set_retrigger").unwrap(),
        INPUT_TIME_NS,
        INPUT_TIME_NS,
        &selection.state,
        &selection.ownership,
        None,
    )
}

fn assert_selection_unchanged(selection: &SelectionProbe) {
    assert_eq!(
        selection.state.lock().unwrap().selection_revision,
        selection.initial_revision,
        "an undelivered selection must not advance Native revision"
    );
    assert_eq!(
        selection.ownership.endpoint_selection(),
        Some((selection.initial_revision, Some(selection.keys[0])))
    );
    let pitch = capture_selected(selection).unwrap();
    assert_eq!(pitch.content_key(), selection.keys[0]);
    assert_eq!(pitch.control_guard(None).unwrap().id, 0);
}

#[test]
fn full_receipt_queue_keeps_selection_revision_and_original_pitch_source() {
    let DispatcherFixture {
        dispatcher,
        input_tx,
        event_rx,
        selection,
    } = fixture();
    let feeder = thread::spawn(move || {
        // Real unmapped input fills every slot of the production receipt channel.
        for index in 0..INPUT_QUEUE_CAPACITY {
            input_tx.send(note(60, index as u64)).unwrap();
        }
        input_tx.send(note(61, INPUT_TIME_NS)).unwrap();
        // Disconnection makes run return only after it processed the selection.
    });
    dispatcher.run();
    feeder.join().unwrap();

    let receipts: Vec<_> = event_rx.try_iter().collect();
    assert_eq!(receipts.len(), INPUT_QUEUE_CAPACITY);
    for (index, receipt) in receipts.iter().enumerate() {
        assert_eq!(receipt.binding_key, "midi:note:1:60");
        assert_eq!(receipt.received_at_ns, index as u64);
        assert!(receipt.action_key.is_none());
        assert!(receipt.selection_revision.is_none());
        assert!(!receipt.dispatched);
    }
    assert_selection_unchanged(&selection);

    // A later replacement of A must reject its original source, never fall through to B.
    selection
        .ownership
        .publish_content_lifetime(0, crate::audio_engine::content_lifetime::reserve().unwrap());
    assert!(matches!(
        capture_selected(&selection),
        Err(PitchActionReject::StaleContent)
    ));
}

#[test]
fn disconnected_receipt_queue_keeps_selection_revision_and_original_pitch_source() {
    let DispatcherFixture {
        dispatcher,
        input_tx,
        event_rx,
        selection,
    } = fixture();
    drop(event_rx);
    input_tx.send(note(61, INPUT_TIME_NS)).unwrap();
    drop(input_tx);
    dispatcher.run();

    assert_selection_unchanged(&selection);
}

#[test]
fn delivered_selection_receipt_matches_native_revision_under_runtime_exclusion() {
    let DispatcherFixture {
        dispatcher,
        input_tx,
        event_rx,
        selection,
    } = fixture();
    input_tx.send(note(61, INPUT_TIME_NS)).unwrap();
    drop(input_tx);
    let dispatch = thread::spawn(move || dispatcher.run());
    let receipt = event_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(receipt.binding_key, "midi:note:1:61");
    assert_eq!(receipt.action_key.as_deref(), Some("ui.select_pad:1"));
    assert_eq!(receipt.value, 100);
    assert_eq!(receipt.received_at_ns, INPUT_TIME_NS);
    assert!(receipt.dispatched);
    assert!(!receipt.direct);
    let revision = receipt.selection_revision.unwrap();
    assert_eq!(revision, selection.initial_revision + 1);
    {
        // The real receipt consumer validates only after taking this existing mutex.
        let state = selection.state.lock().unwrap();
        assert_eq!(state.selection_revision, revision);
        assert!(state.selected.is_some());
        assert_eq!(
            selection.ownership.endpoint_selection(),
            Some((revision, Some(selection.keys[1])))
        );
    }
    let pitch = capture_selected(&selection).unwrap();
    assert_eq!(pitch.content_key(), selection.keys[1]);
    assert_eq!(pitch.control_guard(None).unwrap().id, 1);
    dispatch.join().unwrap();
    assert!(event_rx.try_recv().is_err());
}
