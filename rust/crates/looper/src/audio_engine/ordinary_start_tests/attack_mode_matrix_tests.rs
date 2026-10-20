//! Device-free policy switches with two real sources, original starts and workers.
#![cfg(windows)]

use super::*;
use crate::audio_engine::audio_stream::render_scheduled_audio;
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirementWorker, RtAudioBufferRetirement,
};
use crate::audio_engine::captured_global_start::CapturedGlobalStart;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::input_runtime_binding::{self, InputPadBinding};
use crate::audio_engine::key_lock_start_policy::KeyLockStartBehavior;
use crate::audio_engine::mixer::RtMixer;
use crate::audio_engine::ordinary_endpoint_reserve_tests::adopt_current_sources;
use crate::audio_engine::pitch_actions::PitchKeyMode;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::audio_engine::resident_relocation::{
    self, ResidentWindowTicket, WindowRequest, prepare_window_with_producer,
};
use crate::audio_engine::scheduler::FixedCapacityScheduler;
use crate::audio_engine::source_reader::StemRenderSelection;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{
    PreparedStemSet, ResidentContext, STEM_BUFFER_COUNT, SampleBuffer, StemMixMode,
};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;
use std::sync::Arc;

const WAIT: Duration = Duration::from_secs(5);
const STEM_HASH: u64 = 0x6832_7377;
const STEM_MASK: u8 = 0b0101;
const POLICIES: [&str; 2] = ["first_sound_at_grid", "attack_at_grid"];

#[derive(Clone, Copy, Debug, Default)]
struct Case {
    speed: f64,
    shifts: [i32; 2],
    separate_tempos: bool,
    warm_reserve: bool,
    edited_loop: bool,
    cold: bool,
    stems: bool,
}

impl Case {
    fn neutral() -> Self {
        Self {
            speed: 1.0,
            ..Self::default()
        }
    }

    fn loops(self) -> [(f64, Option<f64>); 2] {
        [
            (if self.edited_loop { 0.2 } else { 0.1 }, Some(2.1)),
            (0.1, Some(2.1)),
        ]
    }
}

fn complete_tone(hz: f64) -> SampleBuffer {
    SampleBuffer {
        channels: 1,
        samples: (0..3 * RATE as usize)
            .map(|frame| {
                (0.25 * (frame as f64 * hz * std::f64::consts::TAU / f64::from(RATE)).sin()) as f32
            })
            .collect::<Vec<_>>()
            .into(),
        residency: None,
    }
    .with_complete_source(RATE)
}

fn refresh_rows(fixture: &Fixture, case: Case) {
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    for id in [0, 1] {
        loaded[id] = true;
        (starts[id], ends[id]) = case.loops()[id];
        bindings[id] = input_runtime_binding::capture(&fixture.control.engine, id).unwrap();
        rows[id] = PitchPadProjection {
            intent: PitchIntent::new(0, case.shifts[id]).unwrap(),
            source_root: Some(0),
            source_mode: Some(PitchKeyMode::Major),
            ..PitchPadProjection::default()
        };
    }
    fixture
        .control
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_pitch(
            true,
            loaded,
            starts,
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            rows,
        )
        .unwrap();
}

fn wait_window(fixture: &mut Fixture, ticket: &ResidentWindowTicket) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WAIT;
    loop {
        fixture.drain();
        resident_relocation::reconcile(&fixture.control.engine).unwrap();
        if ticket.publication_status() == "accepted" {
            assert!(ticket.is_current());
            break;
        }
        assert!(
            !matches!(
                ticket.publication_status(),
                "failed" | "rejected" | "cancelled"
            ),
            "finite ACK failed: {:?}",
            ticket.error()
        );
        assert!(Instant::now() < deadline, "finite ACK timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn selected_stems(fixture: &mut Fixture, complete: &SampleBuffer) -> PreparedStemSet {
    let engine = &fixture.control.engine;
    let source = engine.sample_cache.lock().unwrap()[0].clone().unwrap();
    let captured = input_runtime_binding::capture(engine, 0).unwrap().unwrap();
    let binding = InputPadBinding {
        runtime_revision: engine.input_runtime_ownership.runtime_revision(0),
        ..captured.binding
    };
    assert!(engine.input_runtime_ownership.current(0, binding));
    let publication = PreparedSourcePermit::for_native_binding_for_test(
        0,
        binding,
        engine.input_runtime_ownership.clone(),
        engine.current_timing_acknowledgements.clone(),
        engine.prepared_source_epoch(0).unwrap().clone(),
    )
    .unwrap();
    let stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x68; 32]),
        accepted_timing: binding.accepted,
        reference_samples: complete.samples.clone(),
        reference_residency: complete.residency.clone(),
        publication,
        source_version_hash: STEM_HASH,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: complete.frame_count(),
        available_mask: (1 << STEM_BUFFER_COUNT) - 1,
        stems: std::array::from_fn(|component| {
            let mut stem = complete_tone(197.0 + 83.0 * component as f64);
            // Complete component PCM is rebound by window_for to this exact
            // reference source/window, as in the productive stem publication.
            stem.residency = None;
            stem
        }),
    }
    .window_for(&source)
    .unwrap();
    assert!(
        crate::audio_engine::source_reader::prepared_stem_set_matches_sample(
            &stems,
            &source,
            1,
            RATE as f32,
            source.frame_count(),
        )
    );
    assert!(fixture.mixer.publish_prepared_stems(0, stems.clone()));
    assert_eq!(stems.publication.status(), "accepted");
    {
        let mut producer = fixture.control.producer.lock().unwrap();
        producer
            .push(ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: STEM_MASK,
                source_version_hash: STEM_HASH,
            })
            .unwrap();
        producer
            .push(ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::AllStems,
                source_version_hash: STEM_HASH,
            })
            .unwrap();
    }
    fixture.drain();
    stems
}

fn fully_stopped(fixture: &mut Fixture) {
    fixture.drain();
    assert!(fixture.render(1).iter().all(|sample| *sample == 0.0));
    assert!(fixture.scheduler.is_empty());
    assert!(fixture.mixer.voices.iter().all(|voice| !voice.active));
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
}

fn warm_selected_reserve(fixture: &mut Fixture) -> usize {
    fixture.control.engine.select_input_pad(0).unwrap();
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WAIT;
    loop {
        fixture.render(0);
        let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap();
        if runtime.selected_endpoint_ready() {
            assert_eq!(fixture.transport.output_frame(), frame);
            let native = runtime.selected_endpoint_native_address();
            assert_ne!(native, 0);
            return native;
        }
        assert!(
            Instant::now() < deadline,
            "selected reserve did not become ready"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_starts(fixture: &mut Fixture, starts: &[CapturedPadStart], global: bool) {
    let frame = fixture.transport.output_frame();
    let deadline = Instant::now() + WAIT;
    loop {
        let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        let ready = if global {
            starts.iter().all(|start| start.state() == "applied")
                || runtime.ordinary_group_ready(starts.len())
        } else {
            starts.iter().all(|start| {
                start.state() == "applied" || runtime.has_ready_action(start.sequence())
            })
        };
        if ready {
            break;
        }
        for start in starts {
            assert_ne!(
                start.state(),
                "rejected",
                "worker reason={:?}",
                start.reason()
            );
        }
        assert!(
            Instant::now() < deadline,
            "ordinary matrix worker timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fixture.transport.output_frame(), frame);
}

fn run_case(case: Case, initial: &str, final_policy: &str, global: bool) {
    let mut fixture = Fixture::new(true);
    let _files = case
        .cold
        .then(|| install_synthetic_complete_lease(&mut fixture));
    let complete = case.stems.then(|| {
        let complete = complete_tone(SOURCE_HZ);
        let sample = complete
            .window(LOOP_START, LOOP_END, 2, ResidentContext::KeyLockFiniteLoop)
            .unwrap();
        publish_tone(&fixture.control, 0, sample, true);
        complete
    });
    adopt_current_sources(&mut fixture, true);
    let _window = if case.cold {
        let ticket = prepare_window_with_producer(
            &fixture.control.engine,
            0,
            WindowRequest {
                loop_region: Some(case.loops()[0]),
                key_lock: Some(true),
                ..WindowRequest::default()
            },
            fixture.control.producer.clone(),
        )
        .unwrap();
        wait_window(&mut fixture, &ticket);
        Some(ticket)
    } else {
        None
    };
    for id in [0, 1] {
        fixture
            .mixer
            .set_pad_pitch_intent(id, PitchIntent::new(0, case.shifts[id]).unwrap())
            .unwrap();
        let (start, end) = case.loops()[id];
        if !(case.cold && id == 0) {
            fixture
                .control
                .engine
                .set_pad_loop_region(id, start, end)
                .unwrap();
        }
    }
    fixture.drain();
    fixture.mixer.set_speed(case.speed);
    if case.separate_tempos {
        fixture.mixer.set_bpm_lock(true);
        fixture.mixer.set_master_bpm(120.0);
        fixture.mixer.set_pad_bpm(0, Some(120.0));
        fixture.mixer.set_pad_bpm(1, Some(90.0));
    }
    refresh_rows(&fixture, case);
    let stems = complete
        .as_ref()
        .map(|complete| selected_stems(&mut fixture, complete));
    fully_stopped(&mut fixture);
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(initial)
            .unwrap()
    );
    let cached_native = case
        .warm_reserve
        .then(|| warm_selected_reserve(&mut fixture));
    assert!(fixture.control.engine.can_change_key_lock_start_behavior());
    assert!(
        fixture
            .control
            .engine
            .set_key_lock_start_behavior(final_policy)
            .unwrap()
    );
    assert_eq!(
        fixture.control.engine.key_lock_start_behavior(),
        final_policy
    );
    let policy = KeyLockStartBehavior::parse(final_policy).unwrap();
    let frame = fixture.transport.output_frame();

    let mut group: Option<CapturedGlobalStart> = None;
    let starts = if global {
        let captured = Python::attach(|py| {
            let entries = [0, 1]
                .into_iter()
                .map(|id| {
                    let binding = input_runtime_binding::capture(&fixture.control.engine, id)
                        .unwrap()
                        .unwrap();
                    let (start, end) = case.loops()[id];
                    (Py::new(py, binding).unwrap(), start, end, None)
                })
                .collect();
            fixture
                .control
                .engine
                .capture_global_start(py, entries, None)
                .unwrap()
        });
        fixture.drain();
        let starts = captured
            .resolve()
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|member| member.start)
            .collect::<Vec<_>>();
        assert_eq!(captured.original_frame(), Some(frame));
        let ticket = fixture
            .control
            .engine
            .publish_captured_global_start(&captured)
            .unwrap();
        assert!(ticket.is_some());
        group = Some(captured);
        starts
    } else {
        let starts = [0, 1]
            .into_iter()
            .map(|id| {
                let (start, end) = case.loops()[id];
                fixture
                    .control
                    .engine
                    .capture_pad_start(id, start, end, 1.0, false, None, None)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        fixture.drain();
        for start in &starts {
            let own_window = if case.cold && start.diagnostic_slot() == Some(0) {
                let ticket = fixture
                    .control
                    .engine
                    .prepare_captured_pad_start(start)
                    .unwrap();
                wait_window(&mut fixture, &ticket);
                Some(ticket)
            } else {
                None
            };
            assert!(
                fixture
                    .control
                    .engine
                    .publish_pad_start(start, own_window.as_ref())
                    .unwrap()
            );
        }
        starts
    };
    let originals = starts
        .iter()
        .map(|start| {
            assert_eq!(start.start_behavior(), policy);
            assert_eq!(start.original_frame(), Some(frame));
            (
                start.sequence(),
                start.content_key(),
                start.received_timestamp(),
            )
        })
        .collect::<Vec<_>>();
    fixture.drain();
    for start in &starts {
        assert!(
            start.accepted(),
            "admission {case:?} {initial}->{final_policy} global={global}: {:?}",
            start.reason()
        );
    }
    wait_starts(&mut fixture, &starts, global);
    let mut executions = starts
        .iter()
        .map(|start| start.execution_frame().unwrap())
        .collect::<Vec<_>>();
    executions.sort_unstable();
    executions.dedup();
    let mut pcm = Vec::new();
    for execution in executions {
        pcm.extend(fixture.render_to(execution));
        for start in &starts {
            if start.execution_frame() == Some(execution) {
                // Immediate unity starts and FirstSound reserve hits can have
                // already applied in the actual control drain at this frame.
                // They have consumed no output sample and cannot move future E.
                if execution > frame {
                    assert_ne!(start.state(), "applied");
                } else if start.state() == "applied" {
                    assert_eq!(execution, frame);
                    assert_eq!(start.effective_frame(), Some(execution));
                }
            }
        }
        pcm.extend(fixture.render(1));
    }
    pcm.extend(fixture.pcm(4096));
    assert!(pcm.iter().all(|sample| sample.is_finite()));
    assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
    for (id, start) in starts.iter().enumerate() {
        assert_eq!(
            start.state(),
            "applied",
            "effect {case:?} {initial}->{final_policy} global={global} id={id}: {:?}",
            start.reason()
        );
        assert_eq!(
            (
                start.sequence(),
                start.content_key(),
                start.received_timestamp()
            ),
            originals[id]
        );
        assert_eq!(start.original_frame(), Some(frame));
        assert_eq!(start.effective_frame(), start.execution_frame());
        assert_eq!(fixture.starts(id), 1);
        assert_eq!(start.control_guard().unwrap().id, id);
        let voice = fixture.voice(id);
        assert_eq!(
            fixture.mixer.pad_pitch_intent(id),
            Some(PitchIntent::new(0, case.shifts[id]).unwrap())
        );
        let native_scale = PitchIntent::new(0, case.shifts[id])
            .unwrap()
            .native_pitch_scale(voice.source_playback.tempo_ratio(), true)
            .unwrap();
        if (native_scale - 1.0).abs() > 1.0e-9 {
            assert_ne!(voice.stretch.native_state_address(), 0);
            assert_eq!(voice.stretch.adopted_request_id(), Some(start.sequence()));
            if final_policy == "attack_at_grid" && id == 0 {
                if let Some(cached) = cached_native {
                    assert_ne!(
                        voice.stretch.native_state_address(),
                        cached,
                        "AttackAtGrid must retain its measured original worker"
                    );
                }
            }
        }
    }
    if case.separate_tempos {
        assert!(
            (fixture.voice(0).source_playback.tempo_ratio()
                - fixture.voice(1).source_playback.tempo_ratio())
            .abs()
                > 0.1
        );
    }
    if let Some(group) = group {
        assert_eq!(
            group.state(),
            "applied",
            "global reason={:?}",
            group.reason()
        );
    }
    if let Some(stems) = stems {
        let snapshot = fixture
            .mixer
            .capture_pitch_start_snapshot(0, fixture.voice(0).frame_pos)
            .unwrap();
        assert_eq!(
            snapshot.plan.selection,
            StemRenderSelection::for_stems(&stems, STEM_MASK)
        );
        assert!(Arc::ptr_eq(
            &snapshot.stems.unwrap().complete_set_identity,
            &stems.complete_set_identity
        ));
    }
}

fn both_directions(case: Case) {
    for (initial, final_policy) in [(POLICIES[0], POLICIES[1]), (POLICIES[1], POLICIES[0])] {
        for global in [false, true] {
            run_case(case, initial, final_policy, global);
        }
    }
}

#[test]
fn stopped_two_pad_policy_switch_neutral_fullmix() {
    both_directions(Case::neutral());
}

#[test]
fn stopped_two_pad_policy_switch_keeps_separate_rates_and_pitch() {
    both_directions(Case {
        speed: 1.25,
        shifts: [7, -5],
        separate_tempos: true,
        ..Case::neutral()
    });
}

#[test]
fn stopped_two_pad_policy_switch_keeps_warm_reserve_and_loop_edit() {
    both_directions(Case {
        speed: 0.8,
        shifts: [7, -5],
        warm_reserve: true,
        edited_loop: true,
        ..Case::neutral()
    });
}

#[test]
fn stopped_two_pad_policy_switch_keeps_real_cold_finite_ack() {
    both_directions(Case {
        speed: 1.25,
        shifts: [7, -5],
        cold: true,
        edited_loop: true,
        ..Case::neutral()
    });
}

#[test]
fn stopped_two_pad_policy_switch_keeps_selected_stems_and_original_worker() {
    both_directions(Case {
        speed: 1.25,
        shifts: [7, -5],
        stems: true,
        warm_reserve: true,
        ..Case::neutral()
    });
}

const CONTROLLER_PROBE: &str = r#"
from pathlib import Path

from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.control_diagnostics import ConsoleDiagnostics
from flitzis_looper.input_mapping.controller import InputMappingController
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from flitzis_looper.ui.context import PadAudioActions, SettingsActions


class PolicyStartProbe:
    def __init__(self, audio, source_root):
        self._audio = audio
        self.project = ProjectState()
        self.diagnostics = ConsoleDiagnostics()
        self.project.input_mapping_enabled = False
        self.project.multi_loop = True
        self.project.bpm_lock = False
        self.project.key_lock = True
        self.project.trigger_quantization_enabled = False
        self.session = SessionState()
        for slot, shift in ((0, 7), (1, -5)):
            self.project.sample_paths[slot] = str(Path(source_root) / f"synthetic-policy-{slot}.wav")
            self.project.sample_durations[slot] = 3.0
            self.project.pad_content[slot] = PadContentIdentity(instance_id=f"{slot + 1:032x}")
            self.project.pad_key_intent[slot] = PadKeyIntent(
                source=SourceKeyVersion(raw_key="C"), extra_shift=shift)
            self.project.manual_bpm[slot] = 120.0
            self.project.pad_loop_auto[slot] = False
            self.project.pad_loop_start_s[slot] = 0.1
            self.project.pad_loop_end_s[slot] = 2.1
        self.transport = TransportController(self.project, self.session, audio)
        self.settings = SettingsController(self.project, self.session, audio)
        # Storage is outside this probe. The actual disabled-mapping branch uses
        # only these two model fields and executes the real UI/controller action.
        self.input_mapping = InputMappingController.__new__(InputMappingController)
        self.input_mapping._project = self.project
        self.input_mapping._session = self.session
        self.pad_actions = PadAudioActions(self)
        self.settings_actions = SettingsActions(self)

    def switch(self, initial, final):
        assert self.settings_actions.can_change_key_lock_start_behavior()
        assert self.settings_actions.set_key_lock_start_behavior(initial)
        assert self.settings_actions.can_change_key_lock_start_behavior()
        assert self.settings_actions.set_key_lock_start_behavior(final)
        assert self.project.key_lock_start_behavior == self._audio.key_lock_start_behavior() == final

    def start(self):
        for slot in (0, 1):
            self.pad_actions.trigger_pad(slot, received_at_ns=self._audio.capture_input_timestamp_ns())
        captures = [pending.start for pending in self.transport.residency._captured_starts.values()]
        assert len(captures) == 2, [self.transport.residency.status(slot) for slot in (0, 1)]
        return captures

    def poll(self):
        self.transport.residency.poll()
        for slot in (0, 1):
            status = self.transport.residency.status(slot)
            assert status is None or status[1] is None, (slot, status)

    def applied(self):
        self.poll()
        assert not self.transport.residency._captured_starts
        assert self.session.input_mapping_error is None
"#;

fn install_controller_complete_leases(fixture: &mut Fixture) -> tempfile::TempDir {
    use crate::audio_engine::cold_store::{ColdTransaction, PcmArtifactInput};
    use serde_json::json;

    let temp = tempfile::tempdir().unwrap();
    for (id, hz) in [(0, SOURCE_HZ), (1, 320.0)] {
        let source = temp.path().join(format!("synthetic-policy-{id}.wav"));
        std::fs::write(&source, format!("owned synthetic policy source {id}")).unwrap();
        let pcm = complete_tone(hz);
        let mut transaction =
            ColdTransaction::capture(&temp.path().join("samples"), &source, true, &|| false)
                .unwrap();
        let manifest = transaction
            .write_pcm_artifacts(
                PcmArtifactInput {
                    samples: &pcm.samples,
                    rate_hz: RATE,
                    channels: 1,
                    provenance: json!({"decoder":"synthetic-policy-test-v1"}),
                },
                PcmArtifactInput {
                    samples: &pcm.samples,
                    rate_hz: RATE,
                    channels: 1,
                    provenance: json!({"processing":"identity-test-v1"}),
                },
                json!({"kind":"synthetic-test-identity","from_rate":RATE,"to_rate":RATE}),
                &|| false,
            )
            .unwrap();
        transaction.commit(&|| false).unwrap();
        let lease = transaction.into_lease();
        let mut complete = SampleBuffer {
            channels: 1,
            samples: pcm.samples,
            residency: None,
        };
        crate::audio_engine::cold_residency::attach(&mut complete, &manifest).unwrap();
        let sample = complete
            .window(LOOP_START, LOOP_END, 2, ResidentContext::KeyLockFiniteLoop)
            .unwrap();
        lease.verify_reference(&sample).unwrap();
        lease.bind_sample(&sample).unwrap();
        fixture
            .control
            .engine
            .project_assets
            .retain_cold(
                lease.clone(),
                &sample,
                Arc::downgrade(&fixture.control.engine.project_asset_engine_owner),
            )
            .unwrap();
        publish_tone(&fixture.control, id, sample, true);
        fixture.control.engine.cold_leases.lock().unwrap()[id] = Some(lease);
    }
    temp
}

struct ControllerRoute {
    engine: Py<crate::audio_engine::AudioEngine>,
    probe: Py<PyAny>,
    consumer: rtrb::Consumer<ControlMessage>,
    mixer: Box<RtMixer>,
    scheduler: FixedCapacityScheduler<16>,
    transport: TransportTimeline,
    feedback: Vec<AudioMessage>,
    retirement: RtAudioBufferRetirement,
    _retirement_worker: AudioBufferRetirementWorker,
    _synthetic_files: tempfile::TempDir,
}

impl ControllerRoute {
    fn new(initial: &str, final_policy: &str) -> Self {
        let mut fixture = Fixture::new(true);
        let files = install_controller_complete_leases(&mut fixture);
        adopt_current_sources(&mut fixture, true);
        fixture
            .mixer
            .set_pad_pitch_intent(0, PitchIntent::new(0, 7).unwrap())
            .unwrap();
        fixture
            .mixer
            .set_pad_pitch_intent(1, PitchIntent::new(0, -5).unwrap())
            .unwrap();
        fully_stopped(&mut fixture);
        let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let (engine, probe) = Python::attach(|py| {
            py.import("sys")
                .unwrap()
                .getattr("path")
                .unwrap()
                .call_method1(
                    "insert",
                    (0, repository.join("src").to_string_lossy().as_ref()),
                )
                .unwrap();
            let engine = Py::new(py, fixture.control.engine).unwrap();
            let module = PyModule::from_code(
                py,
                &CString::new(CONTROLLER_PROBE).unwrap(),
                c"attack_mode_controller_probe.py",
                c"attack_mode_controller_probe",
            )
            .unwrap();
            let probe = module
                .getattr("PolicyStartProbe")
                .unwrap()
                .call1((
                    engine.clone_ref(py),
                    files.path().to_string_lossy().as_ref(),
                ))
                .unwrap()
                .unbind();
            probe
                .call_method1(py, "switch", (initial, final_policy))
                .unwrap();
            (engine, probe)
        });
        Self {
            engine,
            probe,
            consumer: fixture.control.consumer,
            mixer: fixture.mixer,
            scheduler: fixture.scheduler,
            transport: fixture.transport,
            feedback: fixture.feedback,
            retirement: fixture.retirement,
            _retirement_worker: fixture._retirement_worker,
            _synthetic_files: files,
        }
    }

    fn drain(&mut self) {
        drain_control_messages(
            &mut self.consumer,
            &mut self.scheduler,
            self.transport.output_frame(),
            &mut TriggerQuantization::Immediate,
            &mut self.transport,
            &mut self.mixer,
            &mut self.feedback,
            &mut self.retirement,
        );
    }

    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        render_scheduled_audio(
            &mut self.mixer,
            &mut self.scheduler,
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            self.transport.output_frame(),
            1,
            &mut self.transport,
            &mut self.feedback,
            &mut self.retirement,
        );
        self.transport.advance_by_rendered_frames(frames);
        output
    }
}

#[test]
fn actual_ui_settings_and_pad_actions_start_both_loaded_pads_after_policy_switch() {
    Python::initialize();
    for (initial, final_policy) in [(POLICIES[0], POLICIES[1]), (POLICIES[1], POLICIES[0])] {
        let mut route = ControllerRoute::new(initial, final_policy);
        let starts = Python::attach(|py| {
            let starts: Vec<Py<CapturedPadStart>> = route
                .probe
                .call_method0(py, "start")
                .unwrap()
                .extract(py)
                .unwrap();
            starts
                .into_iter()
                .map(|start| start.borrow(py).clone())
                .collect::<Vec<_>>()
        });
        let original_frame = route.transport.output_frame();
        let original_owners = starts
            .iter()
            .map(|start| {
                (
                    start.sequence(),
                    start.content_key(),
                    start.received_timestamp(),
                )
            })
            .collect::<Vec<_>>();
        let deadline = Instant::now() + WAIT;
        loop {
            route.drain();
            Python::attach(|py| route.probe.call_method0(py, "poll").unwrap());
            let runtime = route.mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            if starts
                .iter()
                .all(|start| runtime.has_ready_action(start.sequence()))
            {
                break;
            }
            for start in &starts {
                assert_ne!(
                    start.state(),
                    "rejected",
                    "UI policy switch {initial}->{final_policy}: {:?}",
                    start.reason()
                );
            }
            assert!(
                Instant::now() < deadline,
                "UI ordinary worker did not become ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(route.transport.output_frame(), original_frame);
        let execution = starts[0].execution_frame().unwrap();
        assert!(
            starts
                .iter()
                .all(|start| start.execution_frame() == Some(execution))
        );
        while route.transport.output_frame() < execution {
            route.render((execution - route.transport.output_frame()).min(512) as usize);
        }
        let mut pcm = route.render(1);
        pcm.extend(route.render(4_095));
        assert!(pcm.iter().all(|sample| sample.is_finite()));
        let raw = [(SOURCE_HZ, 7), (320.0, -5)]
            .into_iter()
            .map(|(source, shift)| {
                crate::audio_engine::pitch_runtime_tests::physical_birth_tone_reference(
                    4_096, source, 0, shift, 1.0, true,
                )
            })
            .collect::<Vec<_>>();
        let maximum = pcm
            .iter()
            .enumerate()
            .map(|(frame, actual)| (*actual - (raw[0][frame] + raw[1][frame])).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            maximum < 4.0e-5,
            "UI full physical-birth prefix mismatch: {maximum}"
        );
        assert!(pcm.iter().any(|sample| sample.abs() > 0.01));
        for (id, start) in starts.iter().enumerate() {
            assert_eq!(
                start.state(),
                "applied",
                "UI pad {id}: {:?}",
                start.reason()
            );
            assert_eq!(start.original_frame(), Some(original_frame));
            assert_eq!(start.effective_frame(), Some(execution));
            assert_eq!(
                (
                    start.sequence(),
                    start.content_key(),
                    start.received_timestamp()
                ),
                original_owners[id]
            );
            let voice = route
                .mixer
                .voices
                .iter()
                .find(|voice| voice.active && voice.sample_id == id)
                .unwrap();
            assert_eq!(voice.stretch.adopted_request_id(), Some(start.sequence()));
            assert_ne!(voice.stretch.native_state_address(), 0);
        }
        Python::attach(|py| {
            assert_eq!(
                route.engine.borrow(py).key_lock_start_behavior(),
                final_policy
            );
            route.probe.call_method0(py, "applied").unwrap();
        });
    }
}
