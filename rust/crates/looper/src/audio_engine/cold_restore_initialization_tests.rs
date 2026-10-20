//! Original restore controls and genuine Cold acknowledgements, without devices.
#![cfg(windows)]

use super::*;
use crate::audio_engine::audio_stream::{drain_control_messages, drain_parameter_messages};
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, ControlParameterMessage, TriggerQuantization};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Fixture {
    _directory: tempfile::TempDir,
    source: PathBuf,
    samples: PathBuf,
    engine: AudioEngine,
    producer: Arc<Mutex<Producer<ControlMessage>>>,
    consumer: rtrb::Consumer<ControlMessage>,
    parameters: rtrb::Consumer<ControlParameterMessage>,
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "Cold initialization probe timed out"
        );
        std::thread::yield_now();
    }
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let samples = directory.path().join("samples");
        std::fs::create_dir(&samples).unwrap();
        let source = samples.join("restore.wav");
        let values = [0_i16, 8192, -16384, 32767, 0, -4096, 16384];
        let data = (values.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&96_000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        std::fs::write(&source, bytes).unwrap();
        let mut engine = AudioEngine::new().unwrap();
        let (producer, consumer) = rtrb::RingBuffer::new(32);
        let producer = Arc::new(Mutex::new(producer));
        let (_feedback, feedback_rx) = rtrb::RingBuffer::new(8);
        engine.test_control_channels = Some((producer.clone(), Arc::new(Mutex::new(feedback_rx))));
        let (parameters, parameter_consumer) = rtrb::RingBuffer::new(32);
        engine.test_parameter_producer = Some(Arc::new(Mutex::new(parameters)));
        Self {
            _directory: directory,
            source,
            samples,
            engine,
            producer,
            consumer,
            parameters: parameter_consumer,
        }
    }

    fn admit(&self, mix: Option<SourceMixInitialization>) -> u64 {
        admit_for_format_selected_intent_with_pitch(
            &self.engine,
            0,
            self.source.to_string_lossy().into_owned(),
            (false, false, true, None),
            self.producer.clone(),
            (2, 48_000, self.samples.clone()),
            "restore",
            SourcePitchInitialization::RestoredTuple {
                pitch: super::super::pitch_intent::PitchIntent::default(),
                key_lock: Some(false),
            },
            mix,
        )
        .unwrap()
    }

    fn wait_published(&self) {
        wait_until(|| self.engine.sample_cache.lock().unwrap()[0].is_some());
    }

    fn drain(&mut self, mixer: &mut RtMixer, transport: &mut TransportTimeline) {
        let mut scheduler = FixedCapacityScheduler::<32>::new();
        let mut mode = TriggerQuantization::Immediate;
        let mut feedback: Vec<AudioMessage> = Vec::new();
        assert!(
            drain_control_messages(
                &mut self.consumer,
                &mut scheduler,
                0,
                &mut mode,
                transport,
                mixer,
                &mut feedback,
                &mut ImmediateAudioBufferRetirement,
            ) > 0
        );
    }

    fn terminal(&self, request: u64) -> LoaderEvent {
        let mut terminal = None;
        wait_until(|| {
            while let Ok(event) = self.engine.loader_rx.lock().unwrap().try_recv() {
                let event = event.into_test_payload();
                if matches!(&event, LoaderEvent::Success { request_id, .. } | LoaderEvent::Error { request_id, .. } if *request_id == request)
                {
                    terminal = Some(event);
                }
            }
            terminal.is_some()
        });
        terminal.unwrap()
    }

    fn mixer(&self) -> RtMixer {
        let mut mixer = RtMixer::new(2, 48_000.0);
        mixer.set_input_runtime_ownership(self.engine.input_runtime_ownership.clone());
        mixer
    }
}

#[test]
fn original_unarmed_restore_still_rejects_genuine_later_timing_epoch_changes() {
    for edit in 0..3 {
        let mut fixture = Fixture::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        super::pending_cancellation_tests::probe_next_preparation(move |work| {
            assert!(!work.proposal.armed());
            assert!(!work.is_cancelled());
            entered_tx.send(work.base_epoch).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            assert!(work.is_cancelled());
        });
        let request = fixture.admit(None);
        let original_epoch = entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        match edit {
            0 => fixture.engine.set_pad_bpm(0, Some(120.0)).unwrap(),
            1 => fixture.engine.set_pad_timing_metadata(0, 0.125).unwrap(),
            2 => fixture.engine.set_pad_timing_intent(0, "manual").unwrap(),
            _ => unreachable!(),
        }
        assert!(fixture.engine.prepared_source_epochs[0].load(Ordering::Acquire) > original_epoch);
        release_tx.send(()).unwrap();
        assert!(matches!(
            fixture.terminal(request),
            LoaderEvent::Error { .. }
        ));
        assert!(fixture.engine.sample_cache.lock().unwrap()[0].is_none());
        assert_eq!(fixture.engine.pad_request_ids.lock().unwrap()[0], 0);
        assert_eq!(
            fixture.engine.loaded_source_generations.lock().unwrap()[0],
            (0, 0)
        );
    }
}

#[test]
fn initialization_before_admission_keeps_original_restore_startable_after_real_ack() {
    let mut fixture = Fixture::new();
    fixture.engine.set_pad_bpm(0, Some(120.0)).unwrap();
    fixture.engine.set_pad_timing_metadata(0, 0.125).unwrap();
    fixture.engine.set_pad_timing_intent(0, "manual").unwrap();
    let initialized_epoch = fixture.engine.prepared_source_epochs[0].load(Ordering::Acquire);
    let request = fixture.admit(Some(SourceMixInitialization {
        gain_db: -6.0,
        eq_db: [-3.0, 0.0, 2.0],
    }));
    fixture.wait_published();
    assert_eq!(
        fixture.engine.prepared_source_epochs[0].load(Ordering::Acquire),
        initialized_epoch + 1
    );
    let mut mixer = fixture.mixer();
    let mut transport = TransportTimeline::new(48_000);
    fixture.drain(&mut mixer, &mut transport);
    assert!(matches!(
        fixture.terminal(request),
        LoaderEvent::Success { .. }
    ));
    assert_eq!(
        fixture.engine.timing_intents.lock().unwrap()[0],
        TimingIntent::Manual
    );
    assert!(mixer.play_sample(0, 1.0));
}

#[test]
fn saved_mix_survives_source_publication_before_empty_parameter_delivery_and_real_ack() {
    for mix in [
        SourceMixInitialization {
            gain_db: -6.0,
            eq_db: [-3.0, 2.0, -9.0],
        },
        SourceMixInitialization {
            gain_db: 0.0,
            eq_db: [0.0; 3],
        },
    ] {
        let mut fixture = Fixture::new();
        fixture.engine.set_pad_gain(0, -42.0).unwrap();
        fixture.engine.set_pad_eq(0, -18.0, -17.0, -16.0).unwrap();
        let request = fixture.admit(Some(mix));
        fixture.wait_published();
        // Neither parameter was consumed before the source lifetime changed.
        assert_eq!(fixture.parameters.slots(), 2);
        let mut mixer = fixture.mixer();
        mixer.set_pad_gain(0, 11.0);
        mixer.set_pad_eq(0, 5.0, 4.0, 3.0);
        let mut reference = RtMixer::new(2, 48_000.0);
        reference.set_pad_gain(0, mix.gain_db);
        reference.set_pad_eq(0, mix.eq_db[0], mix.eq_db[1], mix.eq_db[2]);
        let expected = reference.parameter_targets_for_test(0);
        let mut transport = TransportTimeline::new(48_000);
        fixture.drain(&mut mixer, &mut transport);
        assert!(matches!(
            fixture.terminal(request),
            LoaderEvent::Success { .. }
        ));
        assert_eq!(mixer.parameter_targets_for_test(0), expected);
        drain_parameter_messages(&mut fixture.parameters, &mut mixer, &mut transport);
        assert!(fixture.parameters.is_empty());
        assert_eq!(mixer.parameter_targets_for_test(0), expected);
        assert!(mixer.play_sample(0, 1.0));
    }
}

#[test]
fn superseded_cold_claim_cannot_install_original_saved_mixing() {
    let mut fixture = Fixture::new();
    let request = fixture.admit(Some(SourceMixInitialization {
        gain_db: -6.0,
        eq_db: [-3.0; 3],
    }));
    fixture.wait_published();
    let replacement = SampleBuffer {
        samples: Arc::from([0.5_f32; 32]),
        channels: 2,
        residency: None,
    };
    fixture
        .engine
        .input_runtime_ownership
        .publish_test_source(0, &replacement, 48_000, 77);
    let mut mixer = fixture.mixer();
    mixer.set_pad_gain(0, 11.0);
    mixer.set_pad_eq(0, 5.0, 4.0, 3.0);
    let before = mixer.parameter_targets_for_test(0);
    fixture.drain(&mut mixer, &mut TransportTimeline::new(48_000));
    assert!(matches!(
        fixture.terminal(request),
        LoaderEvent::Error { .. }
    ));
    assert_eq!(mixer.parameter_targets_for_test(0), before);
    assert!(mixer.bank_for_measurement()[0].is_none());
}

#[test]
fn malformed_public_initial_mixing_rejects_before_admission_or_source_mutation() {
    Python::initialize();
    Python::attach(|py| {
        let engine = AudioEngine::new().unwrap();
        engine.pad_request_ids.lock().unwrap()[0] = 17;
        engine.loaded_source_generations.lock().unwrap()[0] = (13, 48_000);
        let epoch = engine.prepared_source_epochs[0].load(Ordering::Acquire);
        for (source, gain, eq) in [
            ("auto", c"0", c"(0, 0, 0)"),
            ("import", c"0", c"(0, 0, 0)"),
            ("restore", c"0", c"None"),
            ("restore", c"None", c"(0, 0, 0)"),
            ("restore", c"True", c"(0, 0, 0)"),
            ("restore", c"float('nan')", c"(0, 0, 0)"),
            ("restore", c"float('inf')", c"(0, 0, 0)"),
            ("restore", c"13", c"(0, 0, 0)"),
            ("restore", c"-61", c"(0, 0, 0)"),
            ("restore", c"0", c"[0, 0, 0]"),
            ("restore", c"0", c"(0, 0)"),
            ("restore", c"0", c"(0, 0, 0, 0)"),
            ("restore", c"0", c"(0, False, 0)"),
            ("restore", c"0", c"(0, '2', 0)"),
            ("restore", c"0", c"(0, float('inf'), 0)"),
            ("restore", c"0", c"(0, float('nan'), 0)"),
            ("restore", c"0", c"(0, 7, 0)"),
            ("restore", c"0", c"(0, -61, 0)"),
        ] {
            let gain = py.eval(gain, None, None).unwrap();
            let eq = py.eval(eq, None, None).unwrap();
            let error = engine
                .load_sample_async_with_pitch(
                    0,
                    "not-opened.wav".into(),
                    Some(false),
                    false,
                    true,
                    None,
                    None,
                    false,
                    source,
                    None,
                    None,
                    None,
                    Some(&gain),
                    Some(&eq),
                )
                .unwrap_err();
            assert!(error.is_instance_of::<PyValueError>(py));
            assert_eq!(engine.pad_request_ids.lock().unwrap()[0], 17);
            assert_eq!(
                engine.loaded_source_generations.lock().unwrap()[0],
                (13, 48_000)
            );
            assert_eq!(
                engine.prepared_source_epochs[0].load(Ordering::Acquire),
                epoch
            );
            assert_eq!(engine.cold_jobs.counts_for_test(), (0, 0, 0));
            assert_eq!(engine.cold_loading[0].load(Ordering::Acquire), 0);
            assert!(engine.loading_sample_ids.lock().unwrap().is_empty());
            assert!(engine.sample_cache.lock().unwrap()[0].is_none());
        }
        let gain = py.eval(c"-60", None, None).unwrap();
        let eq = py.eval(c"(-60, 0.0, 6)", None, None).unwrap();
        assert_eq!(
            parse_initial_mix("restore", Some(&gain), Some(&eq)).unwrap(),
            Some(SourceMixInitialization {
                gain_db: -60.0,
                eq_db: [-60.0, 0.0, 6.0]
            })
        );
        assert!(parse_initial_mix("import", None, None).unwrap().is_none());
    });
}
