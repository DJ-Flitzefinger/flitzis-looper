//! Public complete tickets retain their captured authority until actual due rendering.
//! The isolated child owns cwd; no installed App, device or facade creates pad 0's voice.

use super::*;
use crate::audio_engine::audio_stream::render_scheduled_audio;
use crate::audio_engine::input_runtime_binding::InputPadBinding;
use crate::audio_engine::resident_relocation::{ResidentLaunchGuard, ResidentWindowTicket};

const SCHEDULE_CHILD: &str = "FLITZI_ORDINARY_PUBLIC_SCHEDULE_CHILD";
const SCHEDULE_OFF_TEST: &str = "audio_engine::ordinary_loader_incoming_tests::ordinary_loader_public_schedule_tests::public_complete_scheduled_start_rejects_supersession_off";
const SCHEDULE_ON_TEST: &str = "audio_engine::ordinary_loader_incoming_tests::ordinary_loader_public_schedule_tests::public_complete_scheduled_start_rejects_supersession_on";
const COMPLETE_FRAMES: usize = 997;
const FINITE_START: usize = 17;
const FINITE_END: usize = 863;

#[derive(Clone, Copy, Debug)]
enum Supersession {
    SourceReplacement,
    CompleteIntent,
    FiniteWindow,
    TicketCancelled,
    PadLaunchCancelled,
}

#[test]
fn public_complete_scheduled_start_rejects_supersession_off() {
    isolated_schedule_history(false, SCHEDULE_OFF_TEST);
}

#[test]
fn public_complete_scheduled_start_rejects_supersession_on() {
    isolated_schedule_history(true, SCHEDULE_ON_TEST);
}

fn isolated_schedule_history(key_lock: bool, test: &str) {
    if std::env::var_os(SCHEDULE_CHILD).is_some() {
        for exclusive in [false, true] {
            current_authority_due_history(key_lock, exclusive);
            for supersession in [
                Supersession::SourceReplacement,
                Supersession::CompleteIntent,
                Supersession::FiniteWindow,
                Supersession::TicketCancelled,
                Supersession::PadLaunchCancelled,
            ] {
                scheduled_history(key_lock, exclusive, supersession);
            }
        }
        eprintln!("public complete scheduled child PID={}", std::process::id());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(SCHEDULE_CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "public complete scheduled child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("public complete scheduled child PID=")
    );
}

fn complete_wav(replacement: bool) -> Vec<u8> {
    let words = (0..COMPLETE_FRAMES)
        .map(|frame| {
            if replacement {
                [
                    -4096 - (frame * 43 % 8191) as i16,
                    2048 + (frame * 61 % 4093) as i16,
                ]
            } else {
                [
                    4096 + (frame * 37 % 8191) as i16,
                    -2048 - (frame * 53 % 4093) as i16,
                ]
            }
        })
        .collect::<Vec<_>>();
    wav(&words)
}

fn current_binding(fixture: &Fixture, id: usize) -> (u64, InputPadBinding) {
    Python::attach(|py| {
        let binding = fixture
            .engine
            .bind(py)
            .call_method1("current_input_runtime_pad_binding", (id,))
            .unwrap()
            .extract::<Py<input_runtime_binding::InputRuntimePadBinding>>()
            .unwrap();
        let binding = binding.borrow(py);
        assert!(binding.is_current(), "actual public binding for pad {id}");
        (binding.source_generation, binding.binding)
    })
}

fn assert_no_bpm_receipt(event: &Py<PyAny>) {
    Python::attach(|py| {
        let analysis = event.bind(py).get_item("analysis").unwrap();
        assert_eq!(
            analysis.get_item("bpm").unwrap().extract::<f64>().unwrap(),
            0.0
        );
        assert_eq!(
            analysis
                .get_item("key")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            "unknown"
        );
        let grid = analysis.get_item("beat_grid").unwrap();
        for field in ["beats", "downbeats", "bars"] {
            assert!(
                grid.get_item(field)
                    .unwrap()
                    .extract::<Vec<f32>>()
                    .unwrap()
                    .is_empty()
            );
        }
    });
}

fn assert_complete_source(sample: &SampleBuffer, expected: &SampleBuffer, key_lock: bool) {
    assert_eq!(sample.frame_count(), COMPLETE_FRAMES);
    assert_eq!(
        (sample.resident_start(), sample.resident_end()),
        (0, COMPLETE_FRAMES)
    );
    assert_eq!(sample.samples.as_ref(), expected.samples.as_ref());
    assert_eq!(sample.samples.len() * size_of::<f32>(), COMPLETE_FRAMES * 8);
    assert_eq!(
        sample.residency.as_ref().unwrap().context,
        if key_lock {
            ResidentContext::KeyLockFullTrack
        } else {
            ResidentContext::FullTrack
        }
    );
}

fn prepare_ticket(
    fixture: &Fixture,
    generation: u64,
    start: usize,
    end: usize,
    key_lock: Option<bool>,
) -> Py<ResidentWindowTicket> {
    Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs
            .set_item("expected_source_generation", generation)
            .unwrap();
        let ticket = fixture
            .engine
            .bind(py)
            .call_method(
                "prepare_resident_control",
                (
                    0,
                    Some(start as f64 / f64::from(RATE)),
                    Some(end as f64 / f64::from(RATE)),
                    None::<f64>,
                    key_lock,
                ),
                Some(&kwargs),
            )
            .unwrap()
            .extract::<Py<ResidentWindowTicket>>()
            .unwrap();
        for (field, expected) in [
            ("start_frame", start),
            ("end_frame", end),
            ("full_frame_count", COMPLETE_FRAMES),
        ] {
            assert_eq!(
                ticket
                    .bind(py)
                    .getattr(field)
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                expected,
                "real public ticket {field}"
            );
        }
        assert_ne!(ticket.borrow(py).publication_status(), "accepted");
        ticket
    })
}

fn acknowledge_ticket(fixture: &mut Fixture, ticket: &Py<ResidentWindowTicket>) {
    wait_until(|| fixture.callback.consumer.peek().is_ok());
    assert!(matches!(
        fixture.callback.consumer.peek().unwrap(),
        ControlMessage::RelocateResident(transaction) if transaction.id == 0
    ));
    assert_eq!(fixture.callback.drain(), 1, "actual own resident ACK");
    Python::attach(|py| {
        let ticket = ticket.borrow(py);
        assert_eq!(ticket.publication_status(), "accepted");
        assert!(ticket.is_current());
    });
    assert!(fixture.transport_events().is_empty());
    assert_unborn(fixture);
}

fn launch_ticket(fixture: &Fixture, ticket: &Py<ResidentWindowTicket>, exclusive: bool) -> bool {
    Python::attach(|py| {
        fixture
            .engine
            .bind(py)
            .call_method1("play_resident_control", (ticket.bind(py), exclusive))
            .unwrap()
            .extract()
            .unwrap()
    })
}

struct CapturedLaunch {
    source_generation: u64,
    binding: InputPadBinding,
    guard: ResidentLaunchGuard,
    launch_revision: u64,
}

fn capture_queued_launch(fixture: &Fixture, exclusive: bool) -> CapturedLaunch {
    let ControlMessage::TriggerInputPad {
        id,
        start_s,
        end_s,
        exclusive: actual_exclusive,
        binding,
        resident_control: Some(guard),
        launch_revision,
        ..
    } = fixture.callback.consumer.peek().unwrap()
    else {
        panic!("actual public ticket must enqueue its captured TriggerInputPad");
    };
    assert_eq!((*id, *actual_exclusive), (0, exclusive));
    assert_eq!(*start_s, 0.0);
    assert_eq!(*end_s, Some(COMPLETE_FRAMES as f64 / f64::from(RATE)));
    let (source_generation, current) = current_binding(fixture, 0);
    assert_eq!(*binding, current);
    assert!(guard.current());
    Python::attach(|py| {
        let engine = fixture.engine.borrow(py);
        assert_eq!(
            engine
                .input_runtime_ownership
                .binding_source_generation(0, *binding),
            Some(source_generation)
        );
        assert!(
            engine
                .input_runtime_ownership
                .launch_current(0, *launch_revision)
        );
    });
    CapturedLaunch {
        source_generation,
        binding: *binding,
        guard: guard.clone(),
        launch_revision: *launch_revision,
    }
}

struct PeerHistory {
    source_generation: u64,
    binding: InputPadBinding,
    generation: u64,
    native: usize,
    fifo: (usize, usize),
    sample: SampleBuffer,
    independent: SampleBuffer,
    region: (usize, Option<usize>),
    key_lock: bool,
    output_bpm: Option<f64>,
}

impl PeerHistory {
    fn capture(fixture: &Fixture) -> Self {
        let peer = voice(&fixture.callback.mixer, 215);
        let (source_generation, binding) = current_binding(fixture, 215);
        let words = (0..8192)
            .map(|frame| [frame as i16 - 4096, 2048 - frame as i16 / 2])
            .collect::<Vec<_>>();
        Self {
            source_generation,
            binding,
            generation: peer.generation,
            native: peer.stretch.native_state_address(),
            fifo: peer.stretch.pending_fifo_frames(),
            sample: peer.sample.as_ref().unwrap().clone(),
            independent: independent_pcm(&wav(&words)),
            region: fixture.callback.mixer.loop_region_frames(215),
            key_lock: fixture.callback.mixer.key_lock_for_measurement(215),
            output_bpm: fixture.callback.mixer.output_bpm_for_sample_id(215),
        }
    }

    fn assert_unchanged(&self, fixture: &Fixture) {
        let peer = voice(&fixture.callback.mixer, 215);
        let sample = peer.sample.as_ref().unwrap();
        assert_eq!(peer.generation, self.generation, "peer voice generation");
        assert_eq!(peer.stretch.native_state_address(), self.native);
        assert_eq!(peer.stretch.pending_fifo_frames(), self.fifo);
        assert!(Arc::ptr_eq(&sample.samples, &self.sample.samples));
        assert!(sample.same_window(&self.sample));
        assert_eq!(sample.samples.as_ref(), self.independent.samples.as_ref());
        assert_eq!(
            (sample.resident_start(), sample.resident_end()),
            (0, self.independent.frame_count())
        );
        assert_eq!(
            current_binding(fixture, 215),
            (self.source_generation, self.binding)
        );
        assert_eq!(fixture.callback.mixer.loop_region_frames(215), self.region);
        assert_eq!(
            fixture.callback.mixer.key_lock_for_measurement(215),
            self.key_lock
        );
        assert_eq!(
            fixture.callback.mixer.output_bpm_for_sample_id(215),
            self.output_bpm
        );
        assert_eq!(peer.source_playback.tempo_ratio(), RATIO);
        assert_eq!(peer.source_playback.rate_target(), RATIO);
        fixture.assert_other();
    }
}

struct CurrentPad {
    source_generation: u64,
    binding: InputPadBinding,
    bank: SampleBuffer,
    region: (usize, Option<usize>),
    key_lock: bool,
    output_bpm: Option<f64>,
    voice_generations: Vec<u64>,
}

impl CurrentPad {
    fn capture(fixture: &Fixture) -> Self {
        let (source_generation, binding) = current_binding(fixture, 0);
        Self {
            source_generation,
            binding,
            bank: fixture.callback.mixer.bank_for_measurement()[0]
                .clone()
                .unwrap(),
            region: fixture.callback.mixer.loop_region_frames(0),
            key_lock: fixture.callback.mixer.key_lock_for_measurement(0),
            output_bpm: fixture.callback.mixer.output_bpm_for_sample_id(0),
            voice_generations: fixture
                .callback
                .mixer
                .voices
                .iter()
                .map(|voice| voice.generation)
                .collect(),
        }
    }

    fn assert_unchanged(&self, fixture: &Fixture) {
        assert_unborn(fixture);
        let bank = fixture.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap();
        assert!(Arc::ptr_eq(&bank.samples, &self.bank.samples));
        assert!(bank.same_window(&self.bank));
        assert_eq!(bank.samples.as_ref(), self.bank.samples.as_ref());
        assert_eq!(
            current_binding(fixture, 0),
            (self.source_generation, self.binding)
        );
        assert_eq!(fixture.callback.mixer.loop_region_frames(0), self.region);
        assert_eq!(
            fixture.callback.mixer.key_lock_for_measurement(0),
            self.key_lock
        );
        assert_eq!(
            fixture.callback.mixer.output_bpm_for_sample_id(0),
            self.output_bpm
        );
        assert_eq!(
            fixture
                .callback
                .mixer
                .voices
                .iter()
                .map(|voice| voice.generation)
                .collect::<Vec<_>>(),
            self.voice_generations,
            "stale launch must neither birth nor stop any voice"
        );
    }
}

fn assert_unborn(fixture: &Fixture) {
    assert!(
        fixture
            .callback
            .mixer
            .voices
            .iter()
            .all(|voice| !voice.is_playing_sample(0)),
        "pad 0 must have no voice before or after a stale public due start"
    );
}

/// Same output-frame advancement as the actual callback, including in-buffer due splitting.
fn render_scheduled(fixture: &mut Fixture, frames: usize, peer: &PeerHistory) {
    assert!(frames > 0);
    let callback = &mut fixture.callback;
    assert_eq!(callback.transport.output_frame(), callback.frame);
    let before = callback.frame;
    let mut output = vec![0.0; frames * 2];
    reset_tap_observation_for_test();
    render_scheduled_audio(
        &mut callback.mixer,
        &mut callback.scheduler,
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        callback.frame,
        2,
        &mut callback.transport,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    callback.transport.advance_by_rendered_frames(frames);
    callback.frame = callback.transport.output_frame();
    callback.other_consumed += frames as u64;
    assert_eq!(callback.frame, before + frames as u64);
    assert_eq!(tap_observation_for_test().missing_reads, 0);
    // Fixture's already running peer is muted. Any public pad-0 birth would emit
    // independent nonzero PCM as well as changing the retained voice generations.
    assert!(output.iter().all(|sample| *sample == 0.0));
    assert!(
        fixture.transport_events().is_empty(),
        "no due START or other STOP"
    );
    assert_unborn(fixture);
    peer.assert_unchanged(fixture);
}

fn replace_source(fixture: &mut Fixture, key_lock: bool) {
    let encoded = complete_wav(true);
    let complete = independent_pcm(&encoded);
    let path = fixture.path.with_file_name("replacement.wav");
    fs::write(&path, &encoded).unwrap();
    let prior = fixture.callback.mixer.bank_for_measurement()[0]
        .clone()
        .unwrap();
    let request = Python::attach(|py| {
        fixture
            .engine
            .bind(py)
            .call_method1(
                "load_incoming_sample_async",
                (0, path.to_string_lossy().as_ref(), key_lock),
            )
            .unwrap()
            .extract::<u64>()
            .unwrap()
    });
    wait_until(|| fixture.callback.consumer.peek().is_ok());
    let ControlMessage::LoadColdSample {
        id,
        sample,
        source_generation,
        incoming_defaults: Some(defaults),
        adoption,
        resident_cancelled: Some(cancelled),
        ..
    } = fixture.callback.consumer.peek().unwrap()
    else {
        panic!("real incoming source replacement must reach its own callback ACK");
    };
    assert_eq!((*id, *source_generation), (0, request));
    assert_eq!((defaults.start_frame, defaults.end_frame), (0, None));
    assert_eq!(defaults.period_seconds, None);
    assert_eq!(defaults.key_lock, key_lock);
    assert_eq!(adoption.load(Ordering::Acquire), 0);
    assert!(!cancelled.load(Ordering::Acquire));
    assert_complete_source(sample, &complete, key_lock);
    let adoption = adoption.clone();
    assert!(
        fixture.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap()
            .same_window(&prior)
    );
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(adoption.load(Ordering::Acquire), 2);
    assert_eq!(fixture.transport_events(), vec![("stopped".to_owned(), 0)]);
    let event = fixture.terminal(request, true);
    assert_no_bpm_receipt(&event);
    let bank = fixture.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert_complete_source(bank, &complete, key_lock);
    assert!(!bank.same_source(&prior));
    assert_eq!(current_binding(fixture, 0).0, request);
    assert_eq!(fs::read(&path).unwrap(), encoded);
    assert_unborn(fixture);
}

fn assert_public_mode(
    fixture: &Fixture,
    ticket: &Py<ResidentWindowTicket>,
    state: &str,
    effective: bool,
    ready: bool,
) {
    Python::attach(|py| {
        let status = fixture
            .engine
            .bind(py)
            .call_method1("pad_key_lock_status", (0,))
            .unwrap();
        assert_eq!(
            status
                .get_item("state")
                .unwrap()
                .extract::<String>()
                .unwrap(),
            state
        );
        assert_eq!(
            status
                .get_item("effective")
                .unwrap()
                .extract::<bool>()
                .unwrap(),
            effective
        );
        assert_eq!(
            status.get_item("ready").unwrap().extract::<bool>().unwrap(),
            ready
        );
        assert_eq!(
            status
                .get_item("request_id")
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            ticket
                .bind(py)
                .getattr("key_lock_request_id")
                .unwrap()
                .extract::<u64>()
                .unwrap()
        );
        assert_eq!(
            status
                .get_item("source_generation")
                .unwrap()
                .extract::<u64>()
                .unwrap(),
            current_binding(fixture, 0).0
        );
        assert!(status.get_item("error").unwrap().is_none());
    });
}

fn current_authority_due_history(key_lock: bool, exclusive: bool) {
    eprintln!("public complete current due: ON={key_lock}, exclusive={exclusive}");
    let mut fixture = Fixture::new(complete_wav(false), false, false);
    let request = fixture.start(key_lock);
    let incoming = fixture.pending(request);
    let receipt = fixture.accept(request, &incoming);
    assert_no_bpm_receipt(&receipt);
    assert_complete_source(&incoming.sample, &fixture.complete, key_lock);
    let peer = PeerHistory::capture(&fixture);
    let ticket = prepare_ticket(&fixture, request, 0, COMPLETE_FRAMES, Some(key_lock));
    acknowledge_ticket(&mut fixture, &ticket);
    assert_public_mode(
        &fixture,
        &ticket,
        if key_lock { "armed" } else { "dry" },
        key_lock,
        true,
    );
    let own = CurrentPad::capture(&fixture);
    render_scheduled(&mut fixture, 1, &peer);
    assert!(fixture.callback.transport.set_master_bpm(120.0));
    fixture.callback.quantization = TriggerQuantization::Grid { step_64ths: 4 };
    assert!(launch_ticket(&fixture, &ticket, exclusive));
    let captured = capture_queued_launch(&fixture, exclusive);
    assert_eq!(captured.source_generation, request);
    assert_eq!(fixture.callback.drain(), 1);
    assert_eq!(fixture.callback.scheduler.len(), 1);
    let due = fixture.callback.scheduler.peek_next_target_frame().unwrap();
    assert!(due > fixture.callback.frame);
    for frames in [31, 137, 512, 777].into_iter().cycle() {
        let remaining = due - fixture.callback.frame;
        if remaining == 0 {
            break;
        }
        render_scheduled(&mut fixture, frames.min(remaining as usize), &peer);
        own.assert_unchanged(&fixture);
        assert_eq!(fixture.callback.scheduler.len(), 1);
    }
    assert_eq!(fixture.callback.frame, due);
    // The peer has consumed precisely the prefix before this boundary. In the
    // exclusive case its STOP occurs before the next eight output frames.
    peer.assert_unchanged(&fixture);
    let prefix_phase = voice(&fixture.callback.mixer, 215)
        .source_playback
        .position();
    let peer_index = fixture
        .callback
        .mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(215))
        .unwrap();
    let retired_before = fixture.callback.retirement.owners.len();
    let mut output = vec![0.0; 8 * 2];
    reset_tap_observation_for_test();
    let callback = &mut fixture.callback;
    render_scheduled_audio(
        &mut callback.mixer,
        &mut callback.scheduler,
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        callback.frame,
        2,
        &mut callback.transport,
        &mut callback.feedback,
        &mut callback.retirement,
    );
    callback.transport.advance_by_rendered_frames(8);
    callback.frame = callback.transport.output_frame();
    if !exclusive {
        callback.other_consumed += 8;
    }
    assert_eq!(callback.frame, due + 8);
    assert!(callback.scheduler.is_empty());
    assert_eq!(tap_observation_for_test().missing_reads, 0);
    assert_eq!(
        fixture.transport_events(),
        if exclusive {
            vec![("stopped".to_owned(), 215), ("started".to_owned(), 0)]
        } else {
            vec![("started".to_owned(), 0)]
        }
    );
    assert!(captured.guard.current());
    assert!(Python::attach(|py| ticket.borrow(py).is_current()));
    assert_eq!(current_binding(&fixture, 0), (request, captured.binding));
    let bank = fixture.callback.mixer.bank_for_measurement()[0]
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(&bank.samples, &own.bank.samples));
    assert!(bank.same_window(&own.bank));
    assert_complete_source(bank, &fixture.complete, key_lock);
    assert_eq!(
        fixture.callback.mixer.loop_region_frames(0),
        (0, Some(COMPLETE_FRAMES))
    );
    assert_eq!(fixture.callback.mixer.key_lock_for_measurement(0), key_lock);

    // Independent PCM is allowed to create only its own reference voice after
    // the productive public voice already exists. Both modes choose dry PCM
    // during these eight frames; ON has not yet completed its first Native block.
    let mut reference = Box::new(RtMixer::new(2, RATE as f32));
    reference.load_sample(0, fixture.complete.clone());
    reference.set_loop_region_frames(0, 0, Some(COMPLETE_FRAMES));
    reference.set_speed(RATIO);
    reference.set_pad_key_lock(0, false);
    assert!(reference.play_sample_at_output_frame(0, 1.0, due));
    let mut expected = vec![0.0; 8 * 2];
    reference.render(&mut expected, &mut [0.0; NUM_SAMPLES]);
    assert!(expected.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(
        output, expected,
        "actual due birth begins at independent complete frame zero"
    );
    let current = voice(&fixture.callback.mixer, 0);
    assert_eq!(
        (
            current.source_playback.position().frame,
            current.source_playback.position().fraction
        ),
        (6, 0.0)
    );
    assert!(
        current
            .source_playback
            .matches_exact(&voice(&reference, 0).source_playback)
    );
    if key_lock {
        let mut raw = RawNative::new(false);
        assert!(raw.native.block_size() > 8);
        let feed = std::array::from_fn(|channel| {
            expected
                .chunks_exact(2)
                .map(|frame| frame[channel])
                .collect::<Vec<_>>()
        });
        assert_eq!(raw.render(&feed, true), expected);
        assert!(!raw.used);
        assert_eq!(
            current.stretch.pending_fifo_frames(),
            (raw.input[0].len(), raw.output[0].len())
        );
    }
    let own_index = fixture
        .callback
        .mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap();
    for (index, voice) in fixture.callback.mixer.voices.iter().enumerate() {
        let births = u64::from(index == own_index);
        let stops = u64::from(exclusive && index == peer_index);
        assert_eq!(
            voice.generation,
            own.voice_generations[index] + births + stops
        );
    }
    if exclusive {
        assert!(
            fixture
                .callback
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.is_playing_sample(215))
        );
        assert_eq!(fixture.callback.other_consumed, due);
        let phase = (due as f64 * RATIO).rem_euclid(peer.independent.frame_count() as f64);
        assert_eq!(prefix_phase.frame, phase.floor() as usize);
        assert_eq!(prefix_phase.fraction.to_bits(), phase.fract().to_bits());
        assert_eq!(
            current_binding(&fixture, 215),
            (peer.source_generation, peer.binding)
        );
        assert!(
            fixture.callback.mixer.bank_for_measurement()[215]
                .as_ref()
                .unwrap()
                .same_window(&peer.sample)
        );
        assert!(fixture.callback.retirement.owners[retired_before..].iter().any(|retired| matches!(
            retired,
            RetiredAudioBuffer::Sample(sample) if Arc::ptr_eq(&sample.samples, &peer.sample.samples)
        )), "exclusive due STOP retires the peer's actual PCM pin");
    } else {
        peer.assert_unchanged(&fixture);
    }
    assert_public_mode(
        &fixture,
        &ticket,
        if key_lock { "waiting" } else { "dry" },
        false,
        !key_lock,
    );
    assert!(fixture.callback.consumer.is_empty());
    assert!(fixture.callback.parameters.is_empty());
    fixture.assert_disk(false);
}

fn scheduled_history(key_lock: bool, exclusive: bool, supersession: Supersession) {
    eprintln!("public complete due: ON={key_lock}, exclusive={exclusive}, {supersession:?}");
    let mut fixture = Fixture::new(complete_wav(false), false, false);
    let request = fixture.start(key_lock);
    let incoming = fixture.pending(request);
    assert_eq!(
        (incoming.defaults.start_frame, incoming.defaults.end_frame),
        (0, None)
    );
    assert_eq!(incoming.defaults.period_seconds, None);
    assert_eq!(incoming.defaults.key_lock, key_lock);
    assert_complete_source(&incoming.sample, &fixture.complete, key_lock);
    let receipt = fixture.accept(request, &incoming);
    assert_no_bpm_receipt(&receipt);
    fixture.assert_disk(false);
    assert_unborn(&fixture);

    let peer = PeerHistory::capture(&fixture);
    peer.assert_unchanged(&fixture);
    let ticket = prepare_ticket(&fixture, request, 0, COMPLETE_FRAMES, Some(key_lock));
    assert!(
        !launch_ticket(&fixture, &ticket, exclusive),
        "unacknowledged public ticket"
    );
    acknowledge_ticket(&mut fixture, &ticket);
    assert_eq!(
        fixture.callback.mixer.loop_region_frames(0),
        (0, Some(COMPLETE_FRAMES))
    );
    assert_complete_source(
        fixture.callback.mixer.bank_for_measurement()[0]
            .as_ref()
            .unwrap(),
        &fixture.complete,
        key_lock,
    );
    let own = CurrentPad::capture(&fixture);

    // Render the first frame through the same kernel so quantization targets a
    // future boundary. The clock never jumps to the queued event's target.
    render_scheduled(&mut fixture, 1, &peer);
    assert!(fixture.callback.transport.set_master_bpm(120.0));
    fixture.callback.quantization = TriggerQuantization::Grid { step_64ths: 4 };
    assert!(launch_ticket(&fixture, &ticket, exclusive));
    let captured = capture_queued_launch(&fixture, exclusive);
    assert_eq!(captured.source_generation, request);
    assert_eq!(captured.binding, own.binding);
    assert_eq!(fixture.callback.drain(), 1);
    assert!(fixture.callback.consumer.is_empty());
    assert_eq!(fixture.callback.scheduler.len(), 1);
    assert!(fixture.transport_events().is_empty());
    own.assert_unchanged(&fixture);
    let due = fixture.callback.scheduler.peek_next_target_frame().unwrap();
    assert!(due > fixture.callback.frame + 3);

    let newer_ticket = match supersession {
        Supersession::SourceReplacement => {
            replace_source(&mut fixture, key_lock);
            assert_ne!(current_binding(&fixture, 0).0, captured.source_generation);
            assert!(!captured.guard.current());
            assert!(!Python::attach(|py| ticket.borrow(py).is_current()));
            None
        }
        Supersession::CompleteIntent | Supersession::FiniteWindow => {
            let (start, end) = match supersession {
                Supersession::CompleteIntent => (0, COMPLETE_FRAMES),
                _ => (FINITE_START, FINITE_END),
            };
            let newer = prepare_ticket(&fixture, request, start, end, None);
            // Admission already invalidates the old captured resident permit;
            // the bank remains the acknowledged original until the real ACK.
            assert!(!captured.guard.current());
            assert!(!Python::attach(|py| ticket.borrow(py).is_current()));
            assert!(
                fixture.callback.mixer.bank_for_measurement()[0]
                    .as_ref()
                    .unwrap()
                    .same_window(&own.bank)
            );
            acknowledge_ticket(&mut fixture, &newer);
            assert_eq!(current_binding(&fixture, 0).0, captured.source_generation);
            assert_eq!(
                fixture.callback.mixer.loop_region_frames(0),
                (start, Some(end))
            );
            let bank = fixture.callback.mixer.bank_for_measurement()[0]
                .as_ref()
                .unwrap();
            assert!(bank.same_source(&own.bank));
            if matches!(supersession, Supersession::CompleteIntent) {
                assert!(Arc::ptr_eq(&bank.samples, &own.bank.samples));
                assert_eq!(bank.window_revision(), own.bank.window_revision());
                assert_eq!(current_binding(&fixture, 0).1, captured.binding);
                assert_complete_source(bank, &fixture.complete, key_lock);
            } else {
                assert_eq!(bank.window_revision(), own.bank.window_revision() + 1);
                assert_eq!(
                    (bank.resident_start(), bank.resident_end()),
                    (FINITE_START, FINITE_END)
                );
                assert_eq!(
                    bank.samples.as_ref(),
                    &fixture.complete.samples[FINITE_START * 2..FINITE_END * 2]
                );
                assert_eq!(
                    bank.residency.as_ref().unwrap().context,
                    if key_lock {
                        ResidentContext::KeyLockFiniteLoop
                    } else {
                        ResidentContext::FiniteLoop
                    }
                );
                assert_ne!(
                    current_binding(&fixture, 0).1.resident,
                    captured.binding.resident
                );
            }
            Some(newer)
        }
        Supersession::TicketCancelled => {
            Python::attach(|py| {
                assert!(
                    ticket
                        .bind(py)
                        .call_method0("cancel_launch")
                        .unwrap()
                        .extract::<bool>()
                        .unwrap()
                );
                assert!(
                    !ticket
                        .bind(py)
                        .call_method0("cancel_launch")
                        .unwrap()
                        .extract::<bool>()
                        .unwrap()
                );
                assert_eq!(ticket.borrow(py).publication_status(), "accepted");
                assert!(ticket.borrow(py).is_current());
            });
            assert!(!captured.guard.current());
            own.assert_unchanged(&fixture);
            None
        }
        Supersession::PadLaunchCancelled => {
            Python::attach(|py| {
                assert!(
                    fixture
                        .engine
                        .bind(py)
                        .call_method1("cancel_pad_launches", (0,))
                        .unwrap()
                        .extract::<bool>()
                        .unwrap()
                );
                let engine = fixture.engine.borrow(py);
                assert!(
                    !engine
                        .input_runtime_ownership
                        .launch_current(0, captured.launch_revision)
                );
                assert_eq!(
                    engine.input_runtime_ownership.launch_revision(0),
                    captured.launch_revision + 1
                );
                assert_eq!(ticket.borrow(py).publication_status(), "accepted");
                assert!(ticket.borrow(py).is_current());
            });
            // This public revision fence preserves the accepted resident permit.
            // Re-capturing a fresh revision at due would wrongly start this ticket.
            assert!(captured.guard.current());
            own.assert_unchanged(&fixture);
            None
        }
    };
    assert!(fixture.callback.consumer.is_empty());
    assert!(fixture.callback.parameters.is_empty());
    assert_eq!(
        fixture.callback.scheduler.peek_next_target_frame(),
        Some(due)
    );
    assert_eq!(fixture.callback.scheduler.len(), 1);
    assert_eq!(captured.source_generation, request);
    assert_eq!(captured.binding, own.binding);
    assert_eq!(
        Python::attach(|py| ticket.borrow(py).publication_status()),
        "accepted"
    );
    peer.assert_unchanged(&fixture);
    let current = CurrentPad::capture(&fixture);

    for frames in [31, 137, 512, 777].into_iter().cycle() {
        let remaining = due - 3 - fixture.callback.frame;
        if remaining == 0 {
            break;
        }
        render_scheduled(&mut fixture, frames.min(remaining as usize), &peer);
        assert_eq!(
            fixture.callback.scheduler.len(),
            1,
            "before actual due render"
        );
        current.assert_unchanged(&fixture);
    }
    assert_eq!(fixture.callback.frame, due - 3);
    render_scheduled(&mut fixture, 11, &peer);
    assert_eq!(fixture.callback.frame, due + 8);
    assert!(
        fixture.callback.scheduler.is_empty(),
        "actual in-buffer due consumed the stale start"
    );
    current.assert_unchanged(&fixture);
    for frames in [1, 31, 137, 512, 777] {
        render_scheduled(&mut fixture, frames, &peer);
        current.assert_unchanged(&fixture);
    }
    assert!(fixture.callback.consumer.is_empty());
    assert!(fixture.callback.parameters.is_empty());
    assert!(fixture.callback.scheduler.is_empty());
    assert_eq!(fs::read(&fixture.path).unwrap(), fixture.encoded);
    for (path, identity) in &fixture.file_identities {
        assert_eq!(&project_assets::capture_identity(path).unwrap(), identity);
    }
    if let Some(newer) = newer_ticket {
        assert!(Python::attach(|py| newer.borrow(py).is_current()));
        assert!(
            !captured.guard.current(),
            "old complete launch permit stays stale"
        );
    }
    if !matches!(supersession, Supersession::SourceReplacement) {
        fixture.assert_disk(false);
    }
}
