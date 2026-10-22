//! Real public load admission in an owned child cwd, with the existing cold worker
//! and callback fixture. No App, audio device, or repository samples are involved.
use super::*;
use crate::audio_engine::hold_action::{HoldActionState, HoldActionTicket};

fn public_load(fixture: &Fixture, replace: bool) -> u64 {
    fixture
        .engine
        .load_sample_async_with_pitch(
            0,
            fixture.source.to_string_lossy().into_owned(),
            Some(false),
            false,
            replace,
            None,
            None,
            false,
            "auto",
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap()
}

fn public_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let (_, feedback) = rtrb::RingBuffer::<AudioMessage>::new(8);
    fixture.engine.test_control_channels =
        Some((fixture.producer.clone(), Arc::new(Mutex::new(feedback))));
    let request = public_load(&fixture, true);
    fixture.pending(0, request);
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    assert!(matches!(
        terminal(&fixture.engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(|| fixture.engine.cold_loading[0].load(Ordering::Acquire) == 0);
    assert!(fixture.callback.mixer.play_sample(0, 1.0));
    fixture
}

fn public_hold(fixture: &Fixture) -> HoldActionTicket {
    Python::attach(|py| {
        let values = pyo3::types::PyDict::new(py);
        values.set_item("press", 1).unwrap();
        fixture
            .engine
            .pause_waveform_hold(0, &values.get_item("press").unwrap().unwrap(), None)
            .unwrap()
    })
}

fn warm_retained_pause(ordinary_pause: bool) {
    let mut fixture = public_fixture();
    let key = fixture
        .engine
        .input_runtime_ownership
        .slot_snapshot(0)
        .unwrap()
        .content_key()
        .unwrap();
    let monitor = if ordinary_pause {
        fixture.engine.pause_sample(0).unwrap()
    } else {
        fixture.engine.resume_sample(0).unwrap()
    };
    fixture.callback.drain(&mut fixture.consumer);
    let receipt = fixture
        .callback
        .feedback
        .events
        .iter()
        .rev()
        .find(|message| matches!(message, AudioMessage::OrdinaryTransportFeedback { .. }))
        .unwrap()
        .clone();
    let held = (!ordinary_pause).then(|| public_hold(&fixture));
    fixture.callback.drain(&mut fixture.consumer);
    let retained = fixture
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .sample
        .as_ref()
        .unwrap()
        .clone();
    assert!(
        fixture
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0) && voice.paused)
    );
    let source = fixture
        .engine
        .input_runtime_ownership
        .ordinary_source_epoch(key.cell())
        .unwrap();
    let request = public_load(&fixture, false);
    fixture.pending(0, request);
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .slot_snapshot(0)
            .unwrap()
            .content_key(),
        Some(key)
    );
    assert_ne!(
        fixture
            .engine
            .input_runtime_ownership
            .ordinary_source_epoch(key.cell()),
        Some(source)
    );
    assert!(monitor.current_control_slot().unwrap().is_none());
    assert!(monitor.current_observation().unwrap().is_none());
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    assert!(matches!(
        terminal(&fixture.engine, request),
        LoaderEvent::Success { .. }
    ));
    wait_until(|| fixture.engine.cold_loading[0].load(Ordering::Acquire) == 0);
    let current = fixture
        .engine
        ._current_ordinary_transport_observation(0)
        .unwrap()
        .unwrap();
    assert_eq!(current.status_kind(), "source_adopted");
    assert_eq!(current.transport_revision(), 0);
    assert_eq!(current.requested_effect(), None);
    assert_eq!(current.outcome(), None);
    assert!(current.active());
    assert_eq!(current.ordinary_paused(), ordinary_pause);
    assert!(!current.settles_admission(&monitor));
    assert!(
        fixture
            .engine
            ._resolve_ordinary_transport_feedback(&receipt)
            .unwrap()
            .is_none()
    );
    let voice = fixture
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert!(voice.paused);
    assert!(Arc::ptr_eq(
        &voice.sample.as_ref().unwrap().samples,
        &retained.samples
    ));
    let bank = fixture.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert!(!Arc::ptr_eq(&bank.samples, &retained.samples));
    assert_eq!(
        bank.samples.as_ref(),
        retained.samples.as_ref(),
        "genuine same-digest warm load"
    );
    if let Some(held) = held {
        assert!(fixture.engine.release_waveform_hold(&held).unwrap());
        fixture.callback.drain(&mut fixture.consumer);
        assert!(
            fixture
                .callback
                .mixer
                .voices
                .iter()
                .any(|voice| voice.is_playing_sample(0) && !voice.paused)
        );
    }
    let generation = fixture.engine.loaded_source_generations.lock().unwrap()[0].0;
    fixture
        .engine
        .seek_sample_with_source(0, 0.0, Some(generation))
        .unwrap();
    fixture.callback.drain(&mut fixture.consumer);
    let seek = fixture
        .engine
        ._current_ordinary_transport_observation(0)
        .unwrap()
        .unwrap();
    assert_eq!(seek.requested_effect(), Some("seek"));
    assert_eq!(seek.outcome(), Some("applied"));
    assert_eq!(seek.ordinary_paused(), ordinary_pause);
    assert!(!seek.settles_admission(&monitor));
    let voice = fixture
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert_eq!(voice.paused, ordinary_pause);
    assert!(Arc::ptr_eq(
        &voice.sample.as_ref().unwrap().samples,
        &retained.samples
    ));
    assert_eq!(fs::read(&fixture.source).unwrap(), fixture.original);
}

fn warm_source_invalidates_deferred_ordinary() {
    let mut fixture = public_fixture();
    let held = public_hold(&fixture);
    fixture.callback.drain(&mut fixture.consumer);
    let key = held.action.key();
    let original_source = fixture
        .engine
        .input_runtime_ownership
        .ordinary_source_epoch(key.cell())
        .unwrap();
    let resume = fixture.engine.resume_sample(0).unwrap();
    fixture.callback.feedback.slots = 0;
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 0);
    let mut output = [0.0; 32];
    crate::audio_engine::audio_stream::render_scheduled_audio(
        &mut fixture.callback.mixer,
        &mut fixture.callback.scheduler,
        &mut output,
        &mut [0.0; crate::audio_engine::constants::NUM_SAMPLES],
        0,
        2,
        &mut fixture.callback.transport,
        &mut fixture.callback.feedback,
        &mut fixture.callback.retirement,
    );
    assert!(output.iter().all(|value| *value == 0.0));
    assert!(
        fixture
            .callback
            .mixer
            .capture_hold_projection_rt(key)
            .is_some()
    );
    let request = public_load(&fixture, false);
    fixture.pending(0, request);
    assert!(resume.current_control_slot().unwrap().is_none());
    fixture.callback.feedback.slots = usize::MAX;
    let original = fixture.consumer.pop().unwrap();
    assert!(
        matches!(&original, ControlMessage::TransportForPlacement { transport_revision, .. } if *transport_revision == resume.transport_revision())
    );
    crate::audio_engine::audio_stream::process_control_message(
        original,
        &mut fixture.callback.scheduler,
        0,
        &mut fixture.callback.quantization,
        &mut fixture.callback.transport,
        &mut fixture.callback.mixer,
        &mut fixture.callback.feedback,
        &mut fixture.callback.retirement,
    );
    let raw = fixture
        .engine
        .input_runtime_ownership
        .ordinary_recorded_snapshot(key)
        .unwrap();
    assert_eq!(
        raw.source, original_source,
        "refusal keeps the command's original source"
    );
    assert_eq!(raw.revision, resume.transport_revision());
    assert_eq!(
        raw.outcome,
        crate::audio_engine::ordinary_transport::REFUSED
    );
    assert!(raw.active && raw.paused);
    assert_eq!(held.action.state(), HoldActionState::Superseded);
    assert!(
        fixture
            .engine
            ._current_ordinary_transport_observation(0)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .input_runtime_ownership
            .pending_ordinary_origin(key.cell()),
        0
    );
    assert_eq!(fixture.callback.drain(&mut fixture.consumer), 1);
    assert!(matches!(
        terminal(&fixture.engine, request),
        LoaderEvent::Success { .. }
    ));
    let current = fixture
        .engine
        ._current_ordinary_transport_observation(0)
        .unwrap()
        .unwrap();
    assert_eq!(current.status_kind(), "source_adopted");
    assert!(current.active() && current.ordinary_paused());
    assert!(!current.settles_admission(&resume));
    assert_eq!(current.transport_revision(), 0);
    assert_eq!(current.requested_effect(), None);
    assert_eq!(current.outcome(), None);
    assert!(resume.current_observation().unwrap().is_none());
    assert!(
        fixture
            .callback
            .mixer
            .voices
            .iter()
            .any(|voice| voice.is_playing_sample(0) && voice.paused)
    );
}

#[test]
#[cfg(windows)]
fn ordinary_transport_public_warm_source_adoption_retains_only_confirmed_pause_cause() {
    const CHILD: &str = "FLITZI_ORDINARY_SOURCE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        Python::initialize();
        warm_retained_pause(true);
        warm_retained_pause(false);
        warm_source_invalidates_deferred_ordinary();
        eprintln!(
            "ordinary source-adoption child completed PID={}",
            std::process::id()
        );
        return;
    }
    let root = tempfile::Builder::new()
        .prefix("ordinary-source-")
        .tempdir()
        .unwrap()
        .keep();
    fs::write(root.join("OWNER.md"), "Owner: isolated Native ordinary-transport public warm-source callback regression. Reproducible test output; preserve results before authorized cleanup.\n").unwrap();
    let (_, modules) = module_path!().split_once("::").unwrap();
    let test = format!(
        "{modules}::ordinary_transport_public_warm_source_adoption_retains_only_confirmed_pause_cause"
    );
    let stdout = fs::File::create(root.join("stdout.log")).unwrap();
    let stderr = fs::File::create(root.join("stderr.log")).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test, "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .current_dir(&root)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!(
                "owned ordinary source child timed out; evidence {}",
                root.display()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = fs::read_to_string(root.join("stdout.log")).unwrap();
    let stderr = fs::read_to_string(root.join("stderr.log")).unwrap();
    eprintln!(
        "ordinary source child evidence: {}\n{stderr}",
        root.display()
    );
    assert!(
        status.success(),
        "ordinary source child failed:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("1 passed; 0 failed"));
    assert!(stderr.contains("ordinary source-adoption child completed PID="));
}
