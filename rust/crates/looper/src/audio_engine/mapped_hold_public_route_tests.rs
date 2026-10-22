use super::*;
use crate::audio_engine::hold_action::HoldActionRtWork;
use crate::messages::{PreparedStemSet, SampleBuffer};

#[derive(Default)]
struct HoldGate {
    capacity: usize,
    held: [Option<Box<HoldActionRtWork>>; 2],
}

impl AudioBufferRetirement for HoldGate {
    fn retire_hold_action(&mut self, work: Box<HoldActionRtWork>) {
        let slot = self
            .held
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("bounded test retirement slots");
        *slot = Some(work);
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
    }
    fn retire_resident_cancellation(&mut self, _: Arc<std::sync::atomic::AtomicBool>) {}
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {}
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {}
    fn retire_sample(&mut self, _: SampleBuffer) {}
    fn retire_prepared_stems(&mut self, _: PreparedStemSet) {}
    fn retire_constant_timing(
        &mut self,
        _: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.capacity
    }
}

fn drain_gate(path: &mut LaunchPath, gate: &mut HoldGate) {
    drain_control_messages(
        &mut path.source.consumer,
        &mut path.scheduler,
        path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.feedback,
        gate,
    );
}

#[test]
fn actual_mapped_hold_retirement_pressure_defers_exact_owner_before_each_effect() {
    let (mut path, _job, _captured) = qualified();
    path.render(7);
    let before = path.voice().source_playback;
    let revision = path.voice().source_feed_history.continuity_revision();
    let ticket = pause(&path, 1);
    let mut gate = HoldGate {
        capacity: 1,
        ..Default::default()
    };
    drain_gate(&mut path, &mut gate);
    assert_eq!(ticket.action.state(), HoldActionState::Admitted);
    assert!(!path.voice().paused);
    assert!(path.voice().source_playback.matches_exact(&before));
    assert_eq!(
        path.voice().source_feed_history.continuity_revision(),
        revision
    );
    assert!(gate.held.iter().all(Option::is_none));
    gate.capacity = 2;
    drain_gate(&mut path, &mut gate);
    assert_eq!(ticket.action.state(), HoldActionState::PausedOwned);
    path.render(113);
    release(&path, &ticket);
    let before = path.voice().source_playback;
    let revision = path.voice().source_feed_history.continuity_revision();
    gate.capacity = 1;
    drain_gate(&mut path, &mut gate);
    assert_eq!(ticket.action.state(), HoldActionState::PausedOwned);
    assert!(path.voice().paused);
    assert!(path.voice().source_playback.matches_exact(&before));
    assert_eq!(
        path.voice().source_feed_history.continuity_revision(),
        revision
    );
    gate.capacity = 2;
    drain_gate(&mut path, &mut gate);
    assert_eq!(ticket.action.state(), HoldActionState::Resumed);
    assert!(!path.voice().paused);
    assert_eq!(gate.held.iter().flatten().count(), 2);
    assert!(
        gate.held
            .iter()
            .flatten()
            .all(|work| Arc::ptr_eq(&work.action, &ticket.action))
    );
}

#[test]
fn actual_python_waveform_controller_owns_mapped_hold_until_callback_ack() {
    let (mut path, _job, _captured) = qualified();
    path.render(7);
    let saved = path.voice().source_playback;
    let source = path.voice().sample.as_ref().unwrap().clone();
    let LaunchPath {
        source: custody,
        mut timeline,
        mut scheduler,
        mut feedback,
    } = path;
    let ColdCustody {
        engine,
        mut mixer,
        mut consumer,
        directory,
        ..
    } = custody;
    let (engine, controller) = Python::attach(|py| {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        py.import("sys")
            .unwrap()
            .getattr("path")
            .unwrap()
            .call_method1(
                "insert",
                (0, repository.join("src").to_string_lossy().as_ref()),
            )
            .unwrap();
        let engine = Py::new(py, engine).unwrap();
        let code = std::ffi::CString::new(
            r#"
from types import SimpleNamespace
from flitzis_looper.models import SessionState
from flitzis_looper.controller.transport.waveform import WaveformController

def make(audio):
    session = SessionState(active_sample_ids={0}, paused_sample_ids=set())
    controller = WaveformController(SimpleNamespace(_audio=audio, _session=session))
    controller.press_pause_hold(0)
    assert controller.has_pending_holds
    assert controller.hold_status() == ('pause_pending', None)
    return controller
"#,
        )
        .unwrap();
        let module = pyo3::types::PyModule::from_code(
            py,
            &code,
            c"mapped_hold_probe.py",
            c"mapped_hold_probe",
        )
        .unwrap();
        let controller = module
            .getattr("make")
            .unwrap()
            .call1((engine.clone_ref(py),))
            .unwrap()
            .unbind();
        (engine, controller)
    });
    let mut quantization = TriggerQuantization::Grid { step_64ths: 16 };
    drain_control_messages(
        &mut consumer,
        &mut scheduler,
        timeline.output_frame(),
        &mut quantization,
        &mut timeline,
        &mut mixer,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    let index = mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(0))
        .unwrap();
    assert!(
        mixer.voices[index].paused,
        "actual Hold is immediate even with trigger quantization"
    );
    assert!(mixer.voices[index].source_playback.matches_exact(&saved));
    Python::attach(|py| {
        controller.call_method0(py, "poll_holds").unwrap();
        let status: (Option<String>, Option<String>) = controller
            .call_method0(py, "hold_status")
            .unwrap()
            .extract(py)
            .unwrap();
        assert_eq!(status, (Some("paused_owned".into()), None));
        controller.call_method0(py, "request_hold_release").unwrap();
        controller.call_method0(py, "poll_holds").unwrap();
        assert!(
            controller
                .bind(py)
                .getattr("has_pending_holds")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
    });
    let mut silence = vec![0.0; 113 * 2];
    let start = timeline.output_frame();
    render_scheduled_audio(
        &mut mixer,
        &mut scheduler,
        &mut silence,
        &mut [0.0; NUM_SAMPLES],
        start,
        2,
        &mut timeline,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    timeline.advance_by_rendered_frames(113);
    assert!(silence.iter().all(|sample| *sample == 0.0));
    drain_control_messages(
        &mut consumer,
        &mut scheduler,
        timeline.output_frame(),
        &mut quantization,
        &mut timeline,
        &mut mixer,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert!(!mixer.voices[index].paused);
    exact(
        saved.position(),
        mixer.voices[index].source_playback.position(),
    );
    Python::attach(|py| {
        controller.call_method0(py, "poll_holds").unwrap();
        assert!(
            !controller
                .bind(py)
                .getattr("has_pending_holds")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        let status: (Option<String>, Option<String>) = controller
            .call_method0(py, "hold_status")
            .unwrap()
            .extract(py)
            .unwrap();
        assert_eq!(status, (None, None));
    });
    let mut output = vec![0.0; 97 * 2];
    let start = timeline.output_frame();
    render_scheduled_audio(
        &mut mixer,
        &mut scheduler,
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        start,
        2,
        &mut timeline,
        &mut feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    assert_pcm(
        &output,
        (7..104).map(|active| pcm_at(&source, coordinate(active as f64 / 64.0))),
    );
    drop(controller);
    drop(engine);
    drop(mixer);
    drop(directory);
}
