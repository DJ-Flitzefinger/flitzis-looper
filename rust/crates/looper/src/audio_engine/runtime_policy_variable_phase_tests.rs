//! Real Cold/ACK/map-worker and original policy claims for variable dry phase.
//! Generated PCM and numerical phase capture do not certify hearing or public grid phase.

#[path = "runtime_policy_variable_phase_lifecycle_tests.rs"]
mod lifecycle_tests;

use super::*;
use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
use crate::audio_engine::mapped_source_clock::{
    MappedClockEpoch, MappedLoopGeometry, MappedSourceSpan,
};
use crate::audio_engine::source_reader::SourceLoopDomain;
use pyo3::types::PyModule;
use std::ffi::CString;
use std::sync::Mutex;

const SMALL_ANCHORS: &[(f64, f64)] = &[(0., 0.), (1. / 64., 8.), (5. / 64., 24.), (9. / 64., 32.)];
const LARGE_ANCHORS: &[(f64, f64)] = &[(0., 0.), (2., 8.), (10., 24.), (18., 32.)];
const LONG_ANCHORS: &[(f64, f64)] = &[(0., 0.), (16., 64.), (80., 192.), (144., 256.)];

struct VariablePath {
    dry: DryMappedPath,
    anchors: Vec<Vec<(f64, f64)>>,
    ends: Vec<usize>,
    starts: Vec<usize>,
    quarters: Vec<Option<f64>>,
    fpb: f64,
    phase_origins: Vec<Option<f64>>,
}

impl VariablePath {
    fn small(lanes: usize, initial: usize) -> Self {
        Self::new(
            &vec![(SMALL_ANCHORS, 6_750, Some(32.)); lanes],
            187.5,
            initial,
        )
    }

    fn new(shapes: &[(&[(f64, f64)], usize, Option<f64>)], fpb: f64, initial: usize) -> Self {
        Self::with_details(shapes, fpb, initial, &vec![0; shapes.len()], true, true)
    }

    fn with_details(
        shapes: &[(&[(f64, f64)], usize, Option<f64>)],
        fpb: f64,
        initial: usize,
        starts: &[usize],
        verified_meter: bool,
        accepted: bool,
    ) -> Self {
        Self::with_accepted_quarters(shapes, fpb, initial, starts, verified_meter, accepted, None)
    }

    /// Private fixture only: legacy accepted timing and selected-map Q retain
    /// their separate original owners. Publication and ACK still precede birth.
    fn with_accepted_quarters(
        shapes: &[(&[(f64, f64)], usize, Option<f64>)],
        fpb: f64,
        initial: usize,
        starts: &[usize],
        verified_meter: bool,
        accepted: bool,
        accepted_quarters: Option<&[f64]>,
    ) -> Self {
        assert!(!shapes.is_empty() && shapes.len() <= 32);
        assert_eq!(starts.len(), shapes.len());
        if let Some(quarters) = accepted_quarters {
            assert_eq!(quarters.len(), shapes.len());
        }
        let source_frames = shapes
            .iter()
            .map(|(anchors, end, _)| {
                (*end).max((anchors.last().unwrap().0 * f64::from(RATE)).ceil() as usize)
            })
            .max()
            .unwrap()
            + 16;
        let pcm = (0..source_frames)
            .map(|frame| ((frame * 73 % 28001) as i32 - 14000) as i16)
            .collect::<Vec<_>>();
        let mut source = ColdCustody::new_selected_with_signal(None, &pcm);
        for id in 2..shapes.len() {
            source.load_and_ack_selected_with_frames(id, None, source_frames);
        }
        source.engine.test_output_format = Some((RATE, 2));
        source.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            source.producer.clone(),
            source.engine.input_clock,
            source.engine.input_runtime_ownership.clone(),
        ));
        source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_render_format(RATE, 2)
            .unwrap();
        let (producer, parameters) = rtrb::RingBuffer::new(128);
        source.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
        source.mixer.set_bpm_lock(true);
        source.mixer.set_master_period(fpb / f64::from(RATE));
        source.mixer.set_speed(1.);
        source.mixer.set_volume(1. / shapes.len() as f32);
        let mut timeline = TransportTimeline::new(RATE);
        assert!(timeline.set_master_period(fpb / f64::from(RATE)));
        timeline.complete_bootstrap();
        let dry = DryMappedPath {
            path: Path {
                source,
                timeline,
                scheduler: FixedCapacityScheduler::new(),
                events: Vec::with_capacity(8192),
            },
            jobs: Vec::with_capacity(shapes.len()),
            deliveries: Vec::with_capacity(shapes.len()),
            lanes: shapes.len(),
            intent_revision: 7,
            tiny: false,
            parameters,
        };
        let mut fixture = Self {
            dry,
            anchors: shapes.iter().map(|(a, _, _)| a.to_vec()).collect(),
            ends: shapes.iter().map(|(_, end, _)| *end).collect(),
            starts: starts.to_vec(),
            quarters: shapes.iter().map(|(_, _, q)| *q).collect(),
            fpb,
            phase_origins: vec![None; shapes.len()],
        };
        for (id, (anchors, end, q)) in shapes.iter().enumerate() {
            fixture
                .dry
                .path
                .source
                .engine
                .set_pad_timing_intent(id, if accepted { "automatic" } else { "manual" })
                .unwrap();
            // This is the original accepted-timing publication and ACK route,
            // not a fabricated scalar-domain witness passed to a new clock.
            if accepted {
                let timing_quarters =
                    accepted_quarters.map_or(q.unwrap_or(32.), |quarters| quarters[id]);
                let period = (*end - starts[id]) as f64 / timing_quarters / f64::from(RATE);
                let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
                    &fixture.dry.path.source.engine,
                    &fixture.dry.path.source.producer,
                    id,
                    period,
                );
                fixture.drain();
                assert_eq!(timing.publication_status().unwrap(), "accepted");
            } else {
                fixture.drain();
            }
            fixture.dry.path.source.mixer.set_pad_key_lock(id, false);
            let (job, delivery) =
                fixture
                    .dry
                    .map_anchors_with_metadata(id, anchors, None, verified_meter);
            fixture.dry.jobs.push(job);
            fixture.dry.deliveries.push(delivery);
        }
        fixture.publish(Some(wire(false, false, 1)), true, true, None, None);
        for id in 0..shapes.len() {
            fixture.start(id);
        }
        fixture.dry.path.render(initial);
        fixture
    }

    fn drain(&mut self) {
        self.dry.path.drain();
        drain_parameter_messages(
            &mut self.dry.parameters,
            &mut self.dry.path.source.mixer,
            &mut self.dry.path.timeline,
        );
    }

    fn start(&mut self, id: usize) -> CapturedPadStart {
        let birth = self
            .dry
            .path
            .source
            .engine
            .capture_pad_start(
                id,
                self.starts[id] as f64 / f64::from(RATE),
                Some(self.ends[id] as f64 / f64::from(RATE)),
                1.,
                false,
                None,
                None,
            )
            .unwrap();
        self.drain();
        assert!(
            self.dry
                .path
                .source
                .engine
                .publish_pad_start(&birth, None)
                .unwrap()
        );
        self.drain();
        assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
        birth
    }

    fn publish(
        &self,
        policy: Option<Value>,
        multi: bool,
        parent: bool,
        optout: Option<usize>,
        key_lock: Option<usize>,
    ) {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < self.dry.lanes).then(|| {
                    self.dry
                        .path
                        .source
                        .engine
                        .current_input_runtime_pad_binding(id)
                        .unwrap()
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let rows = (0..NUM_SAMPLES)
            .map(|id| {
                if id < self.dry.lanes {
                    SourceMapRow {
                        phase_origin_seconds: self.phase_origins[id],
                        operational_four_quarter_bars: false,
                        sync_requested: optout != Some(id),
                        intent_revision: self.dry.intent_revision,
                        delivery: Some(self.dry.deliveries[id].clone()),
                        ordinary_pending: false,
                        exact_quarters: self.quarters[id],
                    }
                } else {
                    SourceMapRow::default()
                }
            })
            .collect();
        let encoded = policy.map(|value| value.to_string());
        self.dry
            .path
            .source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_policy(
                multi,
                (0..NUM_SAMPLES).map(|id| id < self.dry.lanes).collect(),
                (0..NUM_SAMPLES)
                    .map(|id| self.starts.get(id).copied().unwrap_or(0) as f64 / f64::from(RATE))
                    .collect(),
                (0..NUM_SAMPLES)
                    .map(|id| self.ends.get(id).map(|end| *end as f64 / f64::from(RATE)))
                    .collect(),
                bindings.iter().map(Option::as_ref).collect(),
                Some(
                    (0..NUM_SAMPLES)
                        .map(|id| PitchPadProjection {
                            sync: parent,
                            key_lock: key_lock == Some(id),
                            ..Default::default()
                        })
                        .collect(),
                ),
                Some(rows),
                GlobalTimingPolicy::parse(encoded.as_deref()).unwrap(),
            )
            .unwrap();
    }

    fn effect(&mut self) -> Arc<RuntimePolicySnapshot> {
        let result = request(&self.dry.path);
        self.drain();
        self.dry.path.render(0);
        result
    }

    fn mode(&mut self) -> Arc<RuntimePolicySnapshot> {
        self.publish(Some(wire(true, true, 2)), true, true, None, None);
        let snapshot = self.effect();
        assert_eq!(
            snapshot.observation().unwrap().state,
            "applied",
            "{:?}",
            snapshot.observation()
        );
        for id in 0..self.dry.lanes {
            assert!(self.dry.path.voice(id).source_playback.is_mapped());
            assert!(!self.dry.path.voice(id).source_playback.is_mapped_phase());
            assert_eq!(snapshot.map_preparation(id).class, RuntimeMapClass::Ready);
            assert!(
                snapshot
                    .prepared_map(id)
                    .unwrap()
                    .geometry()
                    .has_common_phase()
            );
        }
        snapshot
    }

    fn install(&mut self) -> (Arc<RuntimePolicySnapshot>, Vec<OracleAnchor>) {
        let mode = self.mode();
        // Actual ordinary mapped reads precede the separate PHASE claim.
        self.dry.path.render(129);
        let anchors = (0..self.dry.lanes)
            .map(|id| OracleAnchor::capture(self, id))
            .collect::<Vec<_>>();
        let snapshot = self.effect();
        assert!(Arc::ptr_eq(&mode, &snapshot));
        assert_eq!(
            snapshot.observation().unwrap().state,
            "applied",
            "{:?}",
            snapshot.observation()
        );
        for (id, anchor) in anchors.iter().enumerate() {
            let actual = self.dry.path.voice(id).source_playback;
            assert!(actual.is_mapped_phase());
            assert_position(actual.position(), anchor.raw);
        }
        (snapshot, anchors)
    }

    fn capture_pitch(
        &self,
        action: &str,
    ) -> crate::audio_engine::pitch_actions::CapturedPitchAction {
        self.capture_pitch_for(0, action)
    }

    fn capture_pitch_for(
        &self,
        id: usize,
        action: &str,
    ) -> crate::audio_engine::pitch_actions::CapturedPitchAction {
        use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};
        let clock = Arc::new(SharedOutputClock::new());
        let now = self.dry.path.source.engine.input_clock.capture_ns();
        clock.publish(OutputClockSnapshot {
            valid: true,
            observed_at_ns: now,
            audible_at_ns: now,
            output_frame: self.dry.path.timeline.output_frame(),
            sample_rate_hz: RATE,
            master_period_seconds: self.dry.path.timeline.master_period_seconds(),
            downbeat_frame: self.dry.path.timeline.downbeat_frame(),
            master_beat: self.dry.path.timeline.beat_position().unwrap(),
            freshness_ns: 2_000_000_000,
        });
        self.dry
            .path
            .source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .attach_pitch_clock(clock)
            .unwrap();
        self.dry
            .path
            .source
            .engine
            .capture_pad_pitch_action(id, action, None)
            .unwrap()
    }
}

#[derive(Clone, Copy)]
struct OracleAnchor {
    raw: FractionalSourcePosition,
    first: u64,
    master: f64,
    timeline: TransportTimeline,
}
impl OracleAnchor {
    fn capture(fixture: &VariablePath, id: usize) -> Self {
        let first = fixture.dry.path.timeline.output_frame();
        Self {
            raw: fixture.dry.path.voice(id).source_playback.position(),
            first,
            master: fixture
                .dry
                .path
                .timeline
                .beat_position_at_frame(first)
                .unwrap(),
            timeline: fixture.dry.path.timeline,
        }
    }
}

fn oracle_pairs(
    fixture: &VariablePath,
    id: usize,
    anchor: OracleAnchor,
    frames: &[u64],
) -> Vec<(usize, u64, usize, Option<usize>, u32)> {
    let masters = frames
        .iter()
        .map(|frame| anchor.timeline.beat_position_at_frame(*frame).unwrap())
        .collect::<Vec<_>>();
    Python::attach(|py| {
        let code = CString::new(FRACTION_ORACLE).unwrap();
        let module = PyModule::from_code(
            py,
            &code,
            c"variable_phase_fraction_oracle.py",
            c"variable_phase_fraction_oracle",
        )
        .unwrap();
        module
            .getattr("evaluate")
            .unwrap()
            .call1((
                fixture.anchors[id].clone(),
                RATE,
                fixture.starts[id],
                fixture.ends[id],
                fixture.quarters[id],
                anchor.raw.frame,
                anchor.raw.fraction,
                anchor.master,
                masters,
                fixture.fpb,
                0,
                fixture.phase_origins[id].map(|seconds| (seconds * f64::from(RATE)).round()),
            ))
            .unwrap()
            .extract()
            .unwrap()
    })
}

fn verify_reads(fixture: &VariablePath, id: usize, anchor: OracleAnchor) -> usize {
    let voice = fixture.dry.path.voice(id);
    let sample = voice.sample.as_ref().unwrap();
    let mut history = SourceFeedSnapshot::new();
    assert!(voice.source_feed_history.capture_into(&mut history));
    let mut checked = 0;
    for span in history.spans().filter(|span| {
        span.playback.is_mapped_phase()
            && span.playback.mapped_output_frame().unwrap() >= anchor.first
    }) {
        let first = span.playback.mapped_output_frame().unwrap();
        let expected = oracle_pairs(
            fixture,
            id,
            anchor,
            &(first..=first + span.frames as u64).collect::<Vec<_>>(),
        );
        let mut pcm = [vec![0.; span.frames], vec![0.; span.frames]];
        for (offset, (whole, bits, left, right, gain)) in expected.iter().copied().enumerate() {
            let actual = span.playback.position_at(offset).unwrap();
            assert_eq!(
                (actual.frame, actual.fraction.to_bits()),
                (whole, bits),
                "id={id}, frame={}",
                first + offset as u64
            );
            let plan = SourceReadPlan {
                frame_pos: whole,
                ..span.plan
            };
            let taps = plan.try_fractional_taps(f64::from_bits(bits)).unwrap();
            assert_eq!(
                (taps.left, taps.right, taps.right_gain.to_bits()),
                (left, right, gain)
            );
            if offset < span.frames {
                for channel in 0..2 {
                    let a = sample.samples[(left - sample.resident_start()) * 2 + channel];
                    pcm[channel][offset] = right.map_or(a, |right| {
                        let b = sample.samples[(right - sample.resident_start()) * 2 + channel];
                        a + (b - a) * f32::from_bits(gain)
                    });
                }
            }
        }
        for checked_reader in [false, true] {
            let mut actual = [vec![17.; span.frames], vec![19.; span.frames]];
            if checked_reader {
                span.plan.fill_fractional_buffers_checked_with_readers(
                    sample,
                    StemReadSets::default(),
                    &span.playback,
                    &mut actual,
                    span.frames,
                )
            } else {
                span.plan.fill_fractional_buffers_with_readers(
                    sample,
                    StemReadSets::default(),
                    &span.playback,
                    &mut actual,
                    span.frames,
                )
            }
            .unwrap();
            assert_eq!(actual, pcm);
        }
        if span.frames > 2 {
            let nested = span.slice(1, span.frames - 1).unwrap().slice(1, 1).unwrap();
            let direct = span.slice(2, 1).unwrap();
            assert!(nested.playback.matches_exact(&direct.playback));
            assert!(nested.plan.matches_exact(direct.plan));
        }
        checked += span.frames;
    }
    checked
}

#[test]
fn variable_mode_then_same_snapshot_phase_keeps_actual_pair_pcm_and_original_master() {
    let mut fixture = VariablePath::small(2, 23);
    let mode = fixture.mode();
    fixture.dry.path.render(129);
    let original = scalar_state(&fixture.dry.path);
    let anchor = OracleAnchor::capture(&fixture, 0);
    let master = fixture.dry.path.timeline;
    let snapshot = fixture.effect();
    assert!(Arc::ptr_eq(&mode, &snapshot));
    assert_eq!(
        snapshot.observation().unwrap().execution_frame,
        Some(anchor.first)
    );
    assert!(fixture.dry.path.timeline.matches_clock_epoch(&master));
    for (id, _, old, _) in &original {
        assert_position(
            fixture.dry.path.voice(*id).source_playback.position(),
            old.position(),
        );
    }
    fixture.dry.path.render(17);
    assert_eq!(verify_reads(&fixture, 0, anchor), 17);
    assert!(fixture.dry.path.feedback(0)["grid_phase_confirmed"].is_null());
}

#[test]
fn requested_grid_phase_real_late_birth_keeps_first_read_then_converges_without_allocations() {
    use crate::test_memory::{ThreadAllocations, observe_current_thread};
    for requested_origin in [None, Some(1. / 1024.), Some(-1. / 1024.)] {
        let mut fixture = VariablePath::small(1, 0);
        fixture.dry.path.source.engine.stop_sample(0).unwrap();
        fixture.drain();
        fixture.phase_origins[0] = requested_origin; // ±g rounds to±47 frames; exact B(g)=±188/375.
        fixture.publish(Some(wire(false, false, 1)), true, true, None, None);
        let birth = fixture
            .dry
            .path
            .source
            .engine
            .capture_pad_start(0, 0., Some(6750. / f64::from(RATE)), 1., false, None, None)
            .unwrap();
        fixture.drain();
        assert_eq!(birth.original_frame(), Some(0));
        let mut silence = [0.; 34];
        fixture
            .dry
            .render_into(&mut silence, &mut ImmediateAudioBufferRetirement);
        assert!(silence.iter().all(|value| *value == 0.));
        assert_eq!(fixture.dry.path.timeline.output_frame(), 17);
        assert!(
            fixture
                .dry
                .path
                .source
                .engine
                .publish_pad_start(&birth, None)
                .unwrap()
        );
        fixture.drain();
        assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
        assert_eq!(birth.original_frame(), Some(0));
        assert_eq!(birth.execution_frame(), Some(17));
        let initial = fixture.dry.path.voice(0).source_playback;
        assert_eq!(
            initial
                .diagnostic_launch_reference()
                .map(|(_, frame, master)| (frame, master)),
            Some((0, 0.))
        );
        assert_position(
            initial.position(),
            FractionalSourcePosition {
                frame: 0,
                fraction: 0.,
                seek_mode: ExplicitSeekMode::Normal,
            },
        );
        assert!(!initial.is_mapped_phase());
        let mut onset = [0.; 2];
        fixture
            .dry
            .render_into(&mut onset, &mut ImmediateAudioBufferRetirement);
        assert!(onset.iter().any(|value| *value != 0.));
        let mut history = SourceFeedSnapshot::new();
        assert!(
            fixture
                .dry
                .path
                .voice(0)
                .source_feed_history
                .capture_into(&mut history)
        );
        let first = history.spans().next().unwrap();
        assert_eq!(first.frames, 1);
        assert_position(first.playback.position_at(0).unwrap(), initial.position());

        let mode = fixture.mode();
        fixture.dry.path.render(129);
        let anchor = OracleAnchor::capture(&fixture, 0);
        let phase = request(&fixture.dry.path);
        fixture.drain();
        let (_, constructor_allocations) = observe_current_thread(|| {
            fixture
                .dry
                .render_into(&mut [], &mut ImmediateAudioBufferRetirement)
        });
        assert_eq!(constructor_allocations, ThreadAllocations::default());
        assert!(Arc::ptr_eq(&mode, &phase));
        assert_position(
            fixture.dry.path.voice(0).source_playback.position(),
            anchor.raw,
        );
        assert!(!fixture.dry.path.voice(0).phase_launch_qualified);
        if requested_origin.is_none() {
            assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
            assert_eq!(
                phase.observation().unwrap().reason,
                Some(crate::audio_engine::input_mapping::RuntimePolicyError::Unsupported)
            );
            continue;
        }
        assert!(
            fixture.dry.path.voice(0).source_playback.is_mapped_phase(),
            "observation={:?} feedback={:?} launch={:?} qualified={}",
            phase.observation(),
            fixture.dry.path.feedback(0),
            fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .diagnostic_launch_reference(),
            fixture.dry.path.voice(0).phase_launch_qualified
        );
        assert_eq!(fixture.dry.path.feedback(0)["phase_read_confirmed"], false);
        let witness = phase.prepared_map(0).unwrap().sync_witness();
        let origin = witness.phase_origin.unwrap();
        let g_frame = (requested_origin.unwrap() * f64::from(RATE)).round();
        assert_eq!(origin.frame_bits, g_frame.to_bits());
        assert_eq!(
            origin.authority_revision,
            fixture
                .dry
                .path
                .source
                .engine
                .current_input_runtime_pad_binding(0)
                .unwrap()
                .unwrap()
                .binding
                .authority_revision
        );
        let mut output = [0.; 512];
        let mut rendered = 0;
        let mut correcting = false;
        loop {
            let (_, allocations) = observe_current_thread(|| {
                fixture
                    .dry
                    .render_into(&mut output, &mut ImmediateAudioBufferRetirement)
            });
            assert_eq!(allocations, ThreadAllocations::default());
            assert!(output.iter().all(|value| value.is_finite()));
            assert!(output.iter().any(|value| *value != 0.));
            rendered += output.len() / 2;
            let settled = fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .mapped_phase_settled(phase.prepared_map(0).unwrap().geometry())
                .unwrap();
            correcting |= !settled;
            if rendered == output.len() / 2 {
                assert!(verify_reads(&fixture, 0, anchor) > 0);
            }
            if fixture.dry.path.feedback(0)["phase_read_confirmed"] == true {
                assert!(settled);
                break;
            }
            assert!(
                rendered < 100_000,
                "fixed synthetic g-debt did not converge"
            );
        }
        assert!(correcting);
        assert!(verify_reads(&fixture, 0, anchor) > 0);
        assert_eq!(
            fixture.dry.path.feedback(0)["phase_origin_frame_bits"],
            json!(g_frame.to_bits())
        );
        assert_eq!(
            fixture.dry.path.feedback(0)["phase_origin_authority_revision"],
            json!(origin.authority_revision)
        );
        assert_eq!(birth.original_frame(), Some(0));
        assert!(
            !fixture.dry.path.voice(0).phase_launch_qualified,
            "absolute phase never creates intentional launch proof"
        );
        println!(
            "REQUESTED_GRID_PHASE_REAL T=0 E=17 g_frame={g_frame} rendered={rendered} correcting=true settled_read=true rust_allocations=0; synthetic PCM/source proof, not hearing acceptance"
        );
    }
}

#[test]
fn requested_grid_phase_after_real_loop_edit_uses_actual_cursor_without_launch_reference() {
    for requested_origin in [None, Some(47. / f64::from(RATE))] {
        let mut fixture = VariablePath::small(1, 23);
        let generation = fixture
            .dry
            .path
            .source
            .engine
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap()
            .source_generation;
        let voice_generation = fixture.dry.path.voice(0).generation;
        let sample = fixture.dry.path.voice(0).sample.as_ref().unwrap().clone();
        for end in [6751, 6750] {
            fixture
                .dry
                .path
                .source
                .engine
                .set_pad_loop_region_with_source(
                    0,
                    0.,
                    Some(end as f64 / f64::from(RATE)),
                    Some(generation),
                )
                .unwrap();
            fixture.drain();
            assert_eq!(
                fixture.dry.path.source.mixer.loop_region_frames(0),
                (0, Some(end))
            );
            fixture.dry.path.render(1);
        }
        assert_eq!(fixture.dry.path.timeline.output_frame(), 25);
        assert!(!fixture.dry.path.voice(0).phase_launch_qualified);
        assert!(
            fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .diagnostic_launch_reference()
                .is_none()
        );
        let (job, delivery) =
            fixture
                .dry
                .map_anchors_with_metadata(0, &fixture.anchors[0], None, true);
        fixture.dry.jobs.push(job);
        fixture.dry.deliveries[0] = delivery;
        fixture.phase_origins[0] = requested_origin;
        let mode = fixture.mode();
        fixture.dry.path.render(129);
        let anchor = OracleAnchor::capture(&fixture, 0);
        let phase = fixture.effect();
        assert!(Arc::ptr_eq(&mode, &phase));
        assert_position(
            fixture.dry.path.voice(0).source_playback.position(),
            anchor.raw,
        );
        assert_eq!(fixture.dry.path.voice(0).generation, voice_generation);
        assert!(
            fixture
                .dry
                .path
                .voice(0)
                .sample
                .as_ref()
                .unwrap()
                .same_source(&sample)
        );
        assert!(!fixture.dry.path.voice(0).phase_launch_qualified);
        assert!(
            fixture
                .dry
                .path
                .voice(0)
                .source_playback
                .diagnostic_launch_reference()
                .is_none()
        );
        if requested_origin.is_none() {
            assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
            assert_eq!(
                phase.observation().unwrap().reason,
                Some(crate::audio_engine::input_mapping::RuntimePolicyError::Unsupported)
            );
        } else {
            assert!(
                fixture.dry.path.voice(0).source_playback.is_mapped_phase(),
                "{:?}",
                phase.observation()
            );
            fixture.dry.path.render(17);
            assert_eq!(verify_reads(&fixture, 0, anchor), 17);
        }
    }
}

#[test]
fn variable_real_global_b64_b256_births_keep_own_q8_q256_without_artificial_phase_debt() {
    let anchors = &[
        (0., 0.),
        (1. / 64., 64.),
        (5. / 64., 256.),
        (9. / 64., 512.),
    ][..];
    let mut fixture = VariablePath::with_details(
        &[(anchors, 875, Some(8.)), (anchors, 6750, Some(256.))],
        15.625,
        0,
        &[750, 3750],
        true,
        true,
    );
    assert!(
        fixture
            .dry
            .path
            .timeline
            .anchor_beat_position_at_frame(64., 0)
    );
    let ticket = Python::attach(|py| {
        let entries = [0, 1]
            .into_iter()
            .map(|id| {
                let binding = fixture
                    .dry
                    .path
                    .source
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (
                    Py::new(py, binding).unwrap(),
                    fixture.starts[id] as f64 / f64::from(RATE),
                    Some(fixture.ends[id] as f64 / f64::from(RATE)),
                )
            })
            .collect();
        fixture
            .dry
            .path
            .source
            .engine
            .start_global_playback_batch(py, entries, None)
            .unwrap()
    });
    fixture.drain();
    assert_eq!(ticket.publication_status(), "accepted");
    assert_eq!(fixture.dry.path.timeline.output_frame(), 0);
    for id in [0, 1] {
        let voice = fixture.dry.path.voice(id);
        assert!(voice.phase_launch_qualified && voice.phase_birth_unadvanced);
        assert_eq!(voice.source_playback.position().frame, fixture.starts[id]);
    }
    let mode = fixture.mode();
    fixture.dry.path.render(129);
    let raw = [0, 1].map(|id| fixture.dry.path.voice(id).source_playback.position());
    let phase = fixture.effect();
    assert!(Arc::ptr_eq(&mode, &phase));
    assert_eq!(
        phase.observation().unwrap().state,
        "applied",
        "{:?}",
        phase.observation()
    );
    for id in [0, 1] {
        let playback = fixture.dry.path.voice(id).source_playback;
        let geometry = phase.prepared_map(id).unwrap().geometry();
        assert!(playback.is_mapped_phase());
        assert_position(playback.position(), raw[id]);
        assert!(
            playback.mapped_phase_settled(&geometry).unwrap(),
            "id={id}: already aligned global birth"
        );
        assert_eq!(fixture.dry.path.feedback(id)["phase_read_confirmed"], false);
    }
    // Exercise the small loop seam while the 256-quarter source remains within
    // its own loop. Only the complete settled dry reads qualify the new read witness.
    fixture.dry.path.render(125);
    for id in [0, 1] {
        let position = fixture.dry.path.voice(id).source_playback.position();
        assert!((fixture.starts[id]..fixture.ends[id]).contains(&position.frame));
        assert!(fixture.dry.path.feedback(id)["grid_phase_confirmed"].is_null());
        assert_eq!(fixture.dry.path.feedback(id)["phase_read_confirmed"], true);
    }
}

#[test]
fn variable_actual_q32_q256_share_one_master_and_keep_own_wraps_under_unequal_partitions() {
    let long = &[(0., 0.), (1. / 8., 64.), (5. / 8., 192.), (9. / 8., 256.)][..];
    let mut results = Vec::new();
    for partitions in [&[512][..], &[31, 257, 1, 96, 777][..]] {
        let mut fixture = VariablePath::new(
            &[(SMALL_ANCHORS, 6750, Some(32.)), (long, 54000, Some(256.))],
            187.5,
            23,
        );
        let (_, anchors) = fixture.install();
        let master = fixture.dry.path.timeline;
        let mut pcm = Vec::new();
        let mut remaining = 100_000;
        let mut part = 0;
        while remaining > 0 {
            let frames = partitions[part % partitions.len()].min(remaining);
            pcm.extend(fixture.dry.path.render(frames));
            remaining -= frames;
            part += 1;
        }
        for (id, anchor) in anchors.iter().enumerate() {
            assert_eq!(verify_reads(&fixture, id, *anchor), 100_000);
            let expected = oracle_pairs(
                &fixture,
                id,
                *anchor,
                &[fixture.dry.path.timeline.output_frame()],
            );
            let actual = fixture.dry.path.voice(id).source_playback.position();
            assert_eq!(
                (actual.frame, actual.fraction.to_bits()),
                (expected[0].0, expected[0].1)
            );
        }
        assert!(fixture.dry.path.timeline.matches_clock_epoch(&master));
        results.push((
            pcm,
            fixture.dry.path.voice(0).source_playback,
            fixture.dry.path.voice(1).source_playback,
        ));
    }
    assert_eq!(results[0].0, results[1].0);
    assert!(results[0].1.matches_exact(&results[1].1));
    assert!(results[0].2.matches_exact(&results[1].2));
}

#[test]
fn variable_real_48k_source_axes_and_first_actual_phase_reads_use_observed_e_m0_and_fraction() {
    let mut fixture = VariablePath::new(
        &[
            (LARGE_ANCHORS, 864000, Some(32.)),
            (LONG_ANCHORS, 6912000, Some(256.)),
        ],
        24000.,
        23,
    );
    let (_, anchors) = fixture.install();
    fixture.dry.path.render(513);
    for (id, anchor) in anchors.iter().enumerate() {
        assert_eq!(verify_reads(&fixture, id, *anchor), 513);
    }
}

#[test]
fn variable_identical_publications_and_same_snapshot_do_not_restart_the_frozen_programme() {
    let mut fixture = VariablePath::small(2, 23);
    let (old, _) = fixture.install();
    fixture.dry.path.render(79);
    let clocks = scalar_state(&fixture.dry.path);
    let timeline = fixture.dry.path.timeline;
    let ownership = fixture
        .dry
        .path
        .source
        .engine
        .input_runtime_ownership
        .clone();
    let old_fence = ownership
        .global_timing_revision
        .load(std::sync::atomic::Ordering::Acquire);
    let custody = clocks
        .iter()
        .map(|(id, ..)| fixture.dry.path.voice(*id).phase_policy_revision.unwrap())
        .collect::<Vec<_>>();
    assert!(custody.iter().all(|(_, fence)| *fence == old_fence));
    for _ in 0..8 {
        assert!(Arc::ptr_eq(&old, &fixture.effect()));
        unchanged(&fixture.dry.path, &clocks);
        for ((id, ..), expected) in clocks.iter().zip(&custody) {
            assert_eq!(
                fixture.dry.path.voice(*id).phase_policy_revision,
                Some(*expected)
            );
        }
    }
    fixture.publish(Some(wire(true, true, 3)), true, true, None, None);
    // Full policy admission publishes a new independent global timing fence,
    // even when the effective Transport evaluator and numeric programme agree.
    let published_fence = ownership
        .global_timing_revision
        .load(std::sync::atomic::Ordering::Acquire);
    assert_eq!(published_fence, old_fence.checked_add(1).unwrap());
    let new = fixture.effect();
    assert!(!Arc::ptr_eq(&old, &new));
    assert_eq!(new.observation().unwrap().state, "applied");
    // The original claim refreshes request/service custody. Only the numeric
    // programme and voice generation stay exactly identical across publication.
    assert!(fixture.dry.path.timeline.matches_clock_epoch(&timeline));
    assert_eq!(
        fixture.dry.path.timeline.output_frame(),
        timeline.output_frame()
    );
    for (id, generation, clock, _) in &clocks {
        let voice = fixture.dry.path.voice(*id);
        assert_eq!(voice.generation, *generation);
        assert!(voice.source_playback.matches_exact(clock));
        assert_eq!(voice.sync_request, new.row(*id).unwrap().request);
        assert_eq!(voice.sync_request.global_revision, Some(3));
        assert_eq!(
            voice.phase_policy_revision,
            Some((new.revision(), published_fence))
        );
    }
    let refreshed = scalar_state(&fixture.dry.path);
    let applied_attempt = new.observation().unwrap().attempt;
    for _ in 0..8 {
        assert!(Arc::ptr_eq(&new, &fixture.effect()));
        unchanged(&fixture.dry.path, &refreshed);
        assert_eq!(new.observation().unwrap().attempt, applied_attempt);
        assert_eq!(
            ownership
                .global_timing_revision
                .load(std::sync::atomic::Ordering::Acquire),
            published_fence
        );
        for (id, ..) in &clocks {
            assert_eq!(
                fixture.dry.path.voice(*id).phase_policy_revision,
                Some((new.revision(), published_fence))
            );
        }
    }
}

#[test]
fn variable_explicit_partial_parent_off_local_optout_and_nullable_legacy_keep_distinct_phase_rules()
{
    for (policy, multi, parent, optout, expected_phase) in [
        (Some(wire(true, false, 3)), true, true, None, false),
        (Some(wire(false, true, 3)), true, true, None, false),
        (Some(wire(true, true, 3)), false, true, None, false),
        (Some(wire(true, true, 3)), true, false, None, false),
        (Some(wire(true, true, 3)), true, true, Some(0), false),
        (None, true, true, None, false),
    ] {
        let mut fixture = VariablePath::small(1, 23);
        fixture.install();
        fixture.publish(policy, multi, parent, optout, None);
        fixture.effect();
        assert_eq!(
            fixture.dry.path.voice(0).source_playback.is_mapped_phase(),
            expected_phase
        );
        assert!(fixture.dry.path.feedback(0)["grid_phase_confirmed"].is_null());
    }
    for (sync, realtime, expected_phase) in [
        (Some(true), None, true),
        (None, Some(true), true),
        (None, None, false),
    ] {
        let mut fixture = VariablePath::small(1, 23);
        fixture.publish(
            Some(json!({"schema_version":1,"sync":sync,"realtime_bpm":realtime,"revision":9})),
            true,
            true,
            None,
            None,
        );
        let mode = fixture.effect();
        assert_eq!(
            mode.observation().unwrap().state,
            "applied",
            "{:?}",
            mode.observation()
        );
        let requested = mode.row(0).unwrap().request;
        assert_eq!(requested.mode, SourceSyncMode::Mapped);
        assert_eq!(
            (
                requested.policy_sync,
                requested.policy_realtime_bpm,
                requested.global_revision
            ),
            (sync, realtime, Some(9))
        );
        assert_eq!(fixture.dry.path.voice(0).sync_request, requested);
        assert!(fixture.dry.path.voice(0).source_playback.is_mapped());
        assert!(!fixture.dry.path.voice(0).source_playback.is_mapped_phase());
        let ordinary = fixture.dry.path.voice(0).source_playback;
        let phase = fixture.effect();
        assert!(Arc::ptr_eq(&mode, &phase));
        assert_eq!(
            phase.observation().unwrap().state,
            "applied",
            "{:?}",
            phase.observation()
        );
        assert_eq!(
            fixture.dry.path.voice(0).source_playback.is_mapped_phase(),
            expected_phase
        );
        assert_position(
            fixture.dry.path.voice(0).source_playback.position(),
            ordinary.position(),
        );
        if !expected_phase {
            assert!(
                fixture
                    .dry
                    .path
                    .voice(0)
                    .source_playback
                    .matches_exact(&ordinary)
            );
        }
    }
}

fn pending_phase(lanes: usize) -> (VariablePath, Arc<RuntimePolicySnapshot>) {
    pending_phase_at(lanes, 23)
}

fn pending_phase_at(lanes: usize, initial: usize) -> (VariablePath, Arc<RuntimePolicySnapshot>) {
    let mut fixture = VariablePath::small(lanes, initial);
    let snapshot = fixture.mode();
    fixture.dry.path.render(129);
    let current = request(&fixture.dry.path);
    assert!(Arc::ptr_eq(&snapshot, &current));
    (fixture, snapshot)
}

fn consume_pending(fixture: &mut VariablePath, output: &mut [f32]) {
    fixture.drain();
    fixture
        .dry
        .render_into(output, &mut ImmediateAudioBufferRetirement);
}

fn request_composed_rate(fixture: &mut VariablePath, speed: f64) {
    fixture
        .dry
        .path
        .source
        .engine
        .set_speed_and_master_period(speed, fixture.fpb / f64::from(RATE) / speed)
        .unwrap();
}

fn assert_composed_rate_applied(fixture: &VariablePath) {
    let effect = fixture
        .dry
        .path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
        .effect;
    assert_eq!(
        effect.state,
        crate::audio_engine::global_rate_status::RateState::Applied,
        "{:?}",
        effect.error
    );
    for id in 0..fixture.dry.lanes {
        assert!(
            fixture
                .dry
                .path
                .voice(id)
                .source_playback
                .mapped_epoch()
                .unwrap()
                .is_composed_ordinary()
        );
        assert!(!fixture.dry.path.voice(id).source_playback.is_mapped_phase());
        assert!(fixture.dry.path.feedback(id)["grid_phase_confirmed"].is_null());
    }
}

// Public own-ACK Hold/Release, with real silent callback progress. The reference
// copy checks the entire resumed programme, including its immutable launch ref.
fn hold_composed_fixture(fixture: &mut VariablePath, frames: usize) {
    let before = (0..fixture.dry.lanes)
        .map(|id| {
            let voice = fixture.dry.path.voice(id);
            (
                voice.generation,
                voice.source_playback,
                voice.sample.as_ref().unwrap().clone(),
                voice
                    .mapped_start
                    .as_ref()
                    .unwrap()
                    .effective_source()
                    .unwrap()
                    .clone(),
            )
        })
        .collect::<Vec<_>>();
    let paused_at = fixture.dry.path.timeline.output_frame();
    let mut tickets = Vec::with_capacity(fixture.dry.lanes);
    for id in 0..fixture.dry.lanes {
        let ticket = Python::attach(|py| {
            let press = paused_at * 32 + id as u64 + 1;
            fixture
                .dry
                .path
                .source
                .engine
                .pause_waveform_hold(id, press.into_pyobject(py).unwrap().as_any(), None)
                .unwrap()
        });
        fixture.drain();
        assert!(ticket.is_current());
        assert!(fixture.dry.path.voice(id).paused);
        assert_eq!(fixture.dry.path.timeline.output_frame(), paused_at);
        tickets.push(ticket);
    }
    assert_eq!(fixture.dry.path.timeline.output_frame(), paused_at);
    assert!(tickets.iter().all(|ticket| ticket.is_current()));
    assert!((0..fixture.dry.lanes).all(|id| fixture.dry.path.voice(id).paused));
    assert!(
        fixture
            .dry
            .path
            .render(frames)
            .iter()
            .all(|sample| *sample == 0.)
    );
    for (id, (_, prior, _, _)) in before.iter().enumerate() {
        assert!(
            fixture
                .dry
                .path
                .voice(id)
                .source_playback
                .matches_exact(prior)
        );
    }
    let resumed_at = fixture.dry.path.timeline.output_frame();
    assert_eq!(resumed_at, paused_at + frames as u64);
    for ticket in &tickets {
        assert!(
            fixture
                .dry
                .path
                .source
                .engine
                .release_waveform_hold(ticket)
                .unwrap()
        );
        fixture.drain();
        assert_eq!(ticket.publication_status(), "resumed");
        assert_eq!(fixture.dry.path.timeline.output_frame(), resumed_at);
    }
    assert_eq!(fixture.dry.path.timeline.output_frame(), resumed_at);
    assert!(
        tickets
            .iter()
            .all(|ticket| ticket.publication_status() == "resumed")
    );
    let execution = fixture.dry.path.timeline.output_frame();
    for (id, (generation, prior, sample, owner)) in before.iter().enumerate() {
        let voice = fixture.dry.path.voice(id);
        assert!(!voice.paused);
        assert_eq!(voice.generation, *generation);
        assert!(voice.sample.as_ref().unwrap().same_window(sample));
        assert!(Arc::ptr_eq(
            voice
                .mapped_start
                .as_ref()
                .unwrap()
                .effective_source()
                .unwrap(),
            owner
        ));
        assert_position(voice.source_playback.position(), prior.position());
        assert_eq!(
            voice.source_playback.original_scalar_domain(),
            prior.original_scalar_domain()
        );
        let mut expected = *prior;
        expected
            .resume_mapped_at(
                voice
                    .mapped_start
                    .as_ref()
                    .unwrap()
                    .mapped_geometry()
                    .unwrap(),
                &fixture.dry.path.timeline,
                execution,
            )
            .unwrap();
        assert!(voice.source_playback.matches_exact(&expected));
        assert!(
            voice
                .source_playback
                .phase_reference_current(&fixture.dry.path.timeline)
        );
    }
}

fn pending_composed_rate(lanes: usize, full_fraction: bool) -> VariablePath {
    let anchors = &[(0., 0.), (0.37, 1.), (0.91, 2.)][..];
    let mut fixture = VariablePath::new(&vec![(anchors, 43680, Some(2.)); lanes], 24000., 12000);
    fixture.mode();
    if full_fraction {
        let phase = fixture.effect();
        assert_eq!(phase.observation().unwrap().state, "applied");
        assert!((0..lanes).all(|id| fixture.dry.path.voice(id).source_playback.is_mapped_phase()));
        // First actual Hold exits PHASE through its retained RN-pair recovery.
        hold_composed_fixture(&mut fixture, 17);
        assert!((0..lanes).all(|id| {
            fixture
                .dry
                .path
                .voice(id)
                .source_playback
                .mapped_epoch()
                .unwrap()
                .is_full_fraction_general()
        }));
    }
    // The ordinary Hold creates the address translation that requires exact
    // basis composition. Untranslated old classes deliberately remain old.
    hold_composed_fixture(&mut fixture, 29);
    request_composed_rate(&mut fixture, 0.99);
    fixture
}

fn assert_composed_consumed(fixture: &VariablePath) {
    assert_composed_rate_applied(fixture);
    for id in 0..fixture.dry.lanes {
        let voice = fixture.dry.path.voice(id);
        let mut history = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut history));
        let frames: usize = history
            .spans()
            .filter(|span| {
                span.playback
                    .mapped_epoch()
                    .is_some_and(|epoch| epoch.is_composed_ordinary())
            })
            .map(|span| span.frames)
            .sum();
        assert_eq!(frames, 512);
    }
}

#[test]
fn composed_ordinary_claim_reader_history_fit_two_mib_without_rust_requests() {
    std::thread::Builder::new()
        .name("composed-ordinary-two-mib".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for full_fraction in [false, true] {
                let mut fixture = pending_composed_rate(6, full_fraction);
                let mut output = [0.; 1024];
                let (_, allocations) =
                    observe_current_thread(|| consume_pending(&mut fixture, &mut output));
                assert_eq!(allocations, ThreadAllocations::default());
                assert_composed_consumed(&fixture);
                assert_eq!(
                    fixture
                        .dry
                        .path
                        .voice(0)
                        .source_playback
                        .mapped_epoch()
                        .unwrap()
                        .is_composed_full_fraction_for_test(),
                    full_fraction
                );
                assert!(output.iter().all(|v| v.is_finite()));
                assert!(output.iter().any(|v| *v != 0.));
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "descriptive composed ordinary consuming profile; Root runs serially after frozen POST"]
fn descriptive_composed_actual_constructor_renderer_cohorts_report_median_max_and_limits() {
    for full_fraction in [false, true] {
        for lanes in [2, 4, 6, 10] {
            let mut elapsed = Vec::new();
            for repeat in 0..4 {
                let mut fixture = pending_composed_rate(lanes, full_fraction);
                let mut output = [0.; 1024];
                let started = Instant::now();
                let (_, allocations) =
                    observe_current_thread(|| consume_pending(&mut fixture, &mut output));
                let millis = started.elapsed().as_secs_f64() * 1000.;
                assert_eq!(allocations, ThreadAllocations::default());
                assert_composed_consumed(&fixture);
                assert_eq!(
                    fixture
                        .dry
                        .path
                        .voice(0)
                        .source_playback
                        .mapped_epoch()
                        .unwrap()
                        .is_composed_full_fraction_for_test(),
                    full_fraction
                );
                assert!(output.iter().any(|v| *v != 0.));
                // Inspect the actual installed tag outside the measured region.
                if repeat == 0 {
                    let mut selected_widths = Vec::new();
                    for id in 0..lanes {
                        let voice = fixture.dry.path.voice(id);
                        let geometry = voice
                            .mapped_start
                            .as_ref()
                            .unwrap()
                            .mapped_geometry()
                            .unwrap();
                        let mut history = SourceFeedSnapshot::new();
                        assert!(voice.source_feed_history.capture_into(&mut history));
                        for span in history.spans().filter(|span| {
                            span.playback
                                .mapped_epoch()
                                .is_some_and(|e| e.is_composed_ordinary())
                        }) {
                            let epoch = span.playback.mapped_epoch().unwrap();
                            let frame = span.playback.mapped_output_frame().unwrap();
                            let canonical = geometry.refresh_span(epoch, frame).unwrap();
                            let (small, bits) = canonical
                                .composed_split_for_test(geometry.domain())
                                .unwrap();
                            selected_widths.push((id, frame, span.frames, small, bits));
                        }
                    }
                    println!(
                        "composed_profile_actual_epoch full_fraction={full_fraction} selected_history_widths_pad_frame_frames_small_bits={selected_widths:?} {:?}",
                        fixture
                            .dry
                            .path
                            .voice(0)
                            .source_playback
                            .mapped_epoch()
                            .unwrap()
                    );
                }
                if repeat > 0 {
                    elapsed.push(millis);
                }
            }
            elapsed.sort_by(f64::total_cmp);
            let nominal = 512. / f64::from(RATE) * 1000.;
            println!(
                "composed_productive_profile full_fraction={full_fraction} active={lanes} fixed_capacity=32 frames_per_lane=512 warmups=1 repeats=3 median_ms={:.4} max_ms={:.4} nominal_ms={nominal:.7} within_nominal={}; includes actual-E global claim, full dry renderer/history/copied advance; zero Rust requests asserted; setup excluded; prior6-PAD11.2481ms is not proof of this new class; no Native/device/music/Human acceptance",
                elapsed[1],
                elapsed[2],
                elapsed[2] <= nominal
            );
        }
    }
}

fn pending_full_fraction(lanes: usize) -> (VariablePath, Arc<RuntimePolicySnapshot>) {
    let anchors = &[(0., 0.), (0.37, 1.), (0.91, 2.)][..];
    let mut fixture = VariablePath::new(&vec![(anchors, 43680, Some(2.)); lanes], 24000., 12000);
    let snapshot = fixture.mode();
    fixture.dry.path.render(129);
    assert!(Arc::ptr_eq(&snapshot, &request(&fixture.dry.path)));
    (fixture, snapshot)
}

fn assert_full_fraction_consumed(fixture: &VariablePath) {
    for id in 0..fixture.dry.lanes {
        let voice = fixture.dry.path.voice(id);
        assert!(voice.source_playback.is_mapped_phase());
        assert!(
            voice
                .source_playback
                .mapped_epoch()
                .unwrap()
                .has_full_fraction_recovery()
        );
        let mut history = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut history));
        let spans = history
            .spans()
            .filter(|span| span.playback.is_mapped_phase())
            .collect::<Vec<_>>();
        assert_eq!(spans.iter().map(|span| span.frames).sum::<usize>(), 512);
        for span in spans {
            assert!(
                span.playback
                    .mapped_epoch()
                    .unwrap()
                    .has_full_fraction_recovery()
            );
            assert!(
                span.playback.tempo_ratio() < 0.74,
                "the entire first segment must consume the negative global correction"
            );
        }
        assert!(fixture.dry.path.feedback(id)["grid_phase_confirmed"].is_null());
    }
}

#[test]
fn full_fraction_claim_consuming_reader_and_copied_history_fit_two_mib_without_rust_requests() {
    std::thread::Builder::new()
        .name("full-fraction-two-mib".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let (mut fixture, snapshot) = pending_full_fraction(6);
            let mut output = [0.; 1024];
            let (_, allocations) =
                observe_current_thread(|| consume_pending(&mut fixture, &mut output));
            assert_eq!(allocations, ThreadAllocations::default());
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            assert!(output.iter().all(|v| v.is_finite()));
            assert!(output.iter().any(|v| *v != 0.));
            assert_full_fraction_consumed(&fixture);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
#[ignore = "descriptive FullFraction consuming profile; Root runs serially after frozen POST"]
fn descriptive_full_fraction_actual_constructor_renderer_cohorts_report_median_max_and_limits() {
    // This decimal map admits global correction. It has no waiting-window
    // region; all512 measured frames must consume its actual negative correction.
    for lanes in [2, 4, 6, 10] {
        let mut elapsed = Vec::new();
        for repeat in 0..4 {
            let (mut fixture, snapshot) = pending_full_fraction(lanes);
            let mut output = [0.; 1024];
            let started = Instant::now();
            let (_, allocations) =
                observe_current_thread(|| consume_pending(&mut fixture, &mut output));
            let millis = started.elapsed().as_secs_f64() * 1000.;
            assert_eq!(allocations, ThreadAllocations::default());
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            assert_full_fraction_consumed(&fixture);
            assert!(output.iter().any(|v| *v != 0.));
            if repeat > 0 {
                elapsed.push(millis);
            }
        }
        elapsed.sort_by(f64::total_cmp);
        let nominal = 512. / f64::from(RATE) * 1000.;
        println!(
            "full_fraction_productive_profile region=global_negative_correction active={lanes} fixed_capacity=32 frames_per_lane=512 warmups=1 repeats=3 median_ms={:.4} max_ms={:.4} nominal_ms={nominal:.7} within_nominal={}; includes original drains, actual-E FullFraction construction/all32 claim, consuming dry renderer/history/copied advance; zero Rust requests asserted; setup excluded; no Native/device/music/worstcase or Human acceptance",
            elapsed[1],
            elapsed[2],
            elapsed[2] <= nominal
        );
    }
}

fn assert_consumed_region(fixture: &VariablePath, correcting: bool) -> (usize, usize) {
    let mut history = SourceFeedSnapshot::new();
    assert!(
        fixture
            .dry
            .path
            .voice(0)
            .source_feed_history
            .capture_into(&mut history)
    );
    let phase = history
        .spans()
        .filter(|span| span.playback.is_mapped_phase())
        .collect::<Vec<_>>();
    let total = phase.iter().map(|span| span.frames).sum::<usize>();
    let corrected = phase
        .iter()
        .filter(|span| span.playback.tempo_ratio().to_bits() == (63_f64 / 32.).to_bits())
        .map(|span| span.frames)
        .sum::<usize>();
    assert_eq!(total, 512);
    if correcting {
        assert!(
            corrected > 256,
            "most actual reads must exercise the correcting MAX-window programme"
        );
    } else {
        assert_eq!(
            corrected, 0,
            "the retained wait fixture must remain before correction"
        );
    }
    (total, corrected)
}

#[test]
fn variable_consuming_constructor_claim_reader_history_fits_explicit_two_mib_without_rust_requests()
{
    fn copy<T: Copy>() {}
    copy::<SourcePlayback>();
    copy::<MappedClockEpoch>();
    copy::<MappedSourceSpan>();
    assert!(!std::mem::needs_drop::<SourcePlayback>());
    assert!(std::mem::size_of::<MappedClockEpoch>() <= 176);
    assert!(std::mem::size_of::<MappedSourceSpan>() <= 288);
    assert!(std::mem::size_of::<SourcePlayback>() <= 512);
    assert!(std::mem::size_of::<[Option<SourcePlayback>; 32]>() <= 16 * 1024);
    let phase_bytes =
        std::mem::size_of::<crate::audio_engine::mapped_source_clock::PhaseLocalSpanView>();
    let general_bytes =
        std::mem::size_of::<crate::audio_engine::mapped_source_clock::GeneralLocalSpanView>();
    let playback_view_bytes =
        std::mem::size_of::<crate::audio_engine::source_playback::CompactLocalPlaybackView>();
    let (grid_bytes, full_phase_bytes) = MappedClockEpoch::phase_local_payload_sizes_for_test();
    let (declared_bytes, physical_bytes, full_general_bytes) =
        MappedLoopGeometry::general_local_payload_sizes_for_test();
    let (composed_ordinary_bytes, composed_full_bytes, composed_bytes) =
        MappedLoopGeometry::composed_local_payload_sizes_for_test();
    let affine_bytes =
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::RationalAffine>();
    // Original numeric payloads and local-view caps remain unchanged. The full
    // grid target grows the copied epoch by24 bytes; physical and composed views
    // avoid retaining unused span geometry after complete validation. Keep all
    // historical local caps explicit, not size-of-derived moving limits.
    assert!(
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::Integer>() <= 257
    );
    assert!(std::mem::size_of::<SourceLoopDomain>() <= 96);
    assert!(
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::Affine<256>>()
            <= 928
    );
    assert!(affine_bytes <= 944);
    assert!(grid_bytes <= 1184 + 32);
    assert!(
        declared_bytes <= 1184 + 64 && physical_bytes <= 1184 + 64,
        "temporary_bytes grid={grid_bytes} declared={declared_bytes} physical={physical_bytes} full_phase={full_phase_bytes} full_general={full_general_bytes} composed_ordinary={composed_ordinary_bytes} composed_full={composed_full_bytes} composed_enum={composed_bytes} phase_enum={phase_bytes} general_enum={general_bytes} playback_view={playback_view_bytes}"
    );
    assert!(full_phase_bytes <= 1200 + 32 && full_general_bytes <= 1200 + 32);
    // The original composed wide payload keeps three signed2048 integers<=771
    // bytes, two i128 masters32, raw8, prepared reader72 and one class byte.
    // Small holds three signed512 integers<=195, masters32, raw8, reader72,
    // its existing flag and two1024-bit bounds256, all aligned to16.
    assert!(
        std::mem::size_of::<
            crate::audio_engine::full_fraction_source_numerics::ComposedWideAffine<256>,
        >() <= 896
    );
    assert!(
        std::mem::size_of::<
            crate::audio_engine::full_fraction_source_numerics::ComposedWideAffine<128>,
        >() <= 512
    );
    assert!(
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::SmallAffine>()
            <= 576
    );
    assert!(
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::ComposedAffine<128>>(
        ) <= 592
    );
    assert!(
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::ComposedAffine<256>>(
        ) <= 912
    );
    // The selected enum adds at most one16-byte tag/alignment layer. Composed
    // observation keeps the original epoch, bounds and grid after validation;
    // LocalView adds one more layer. Historical payload caps remain unchanged.
    assert!(composed_ordinary_bytes <= 864);
    assert!(composed_full_bytes <= 1184 && composed_bytes <= 1200);
    println!(
        "composed_affine_temporary_layout small_bytes={} wide1024_bytes={} wide2048_bytes={} selected_ordinary_bytes={} selected_full_bytes={}; fixed payload caps576/512/896/592/912, outer caps1280/1312 unchanged",
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::SmallAffine>(),
        std::mem::size_of::<
            crate::audio_engine::full_fraction_source_numerics::ComposedWideAffine<128>,
        >(),
        std::mem::size_of::<
            crate::audio_engine::full_fraction_source_numerics::ComposedWideAffine<256>,
        >(),
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::ComposedAffine<128>>(
        ),
        std::mem::size_of::<crate::audio_engine::full_fraction_source_numerics::ComposedAffine<256>>(
        )
    );
    println!(
        "variable_phase_temporary_layout grid_bytes={grid_bytes} declared_bytes={declared_bytes} physical_bytes={physical_bytes} rational_affine_bytes={affine_bytes} full_phase_bytes={full_phase_bytes} full_general_bytes={full_general_bytes} composed_ordinary_bytes={composed_ordinary_bytes} composed_full_bytes={composed_full_bytes} composed_enum_bytes={composed_bytes} phase_local_bytes={phase_bytes} general_local_bytes={general_bytes} compact_local_playback_bytes={playback_view_bytes} epoch_bytes={} span_bytes={} playback_bytes={} cohort_bytes={} historical_old_class_inner_max=1184 historical_old_outer_max=1200 new_inner_max=1280 new_outer_max=1312 phase_needs_drop={} playback_view_needs_drop={}; copied fixed caps176/288/512 (historical152/264) and unchanged16KiB cohort; this is layout, not measured peak stack",
        std::mem::size_of::<MappedClockEpoch>(),
        std::mem::size_of::<MappedSourceSpan>(),
        std::mem::size_of::<SourcePlayback>(),
        std::mem::size_of::<[Option<SourcePlayback>; 32]>(),
        std::mem::needs_drop::<crate::audio_engine::mapped_source_clock::PhaseLocalSpanView>(),
        std::mem::needs_drop::<crate::audio_engine::source_playback::CompactLocalPlaybackView>()
    );
    assert!(
        phase_bytes <= 1280,
        "PhaseLocalSpanView actual_bytes={phase_bytes} exceeds separately bounded containing enum"
    );
    assert!(
        general_bytes <= 1280,
        "GeneralLocalSpanView actual_bytes={general_bytes} exceeds separately bounded containing enum"
    );
    // One additional local-span tag and the frame index each need at most one
    // 16-byte alignment layer beyond the independently bounded inner payload.
    assert!(
        playback_view_bytes <= 1312,
        "CompactLocalPlaybackView actual_bytes={playback_view_bytes} exceeds containing-view bound1312"
    );
    assert!(!std::mem::needs_drop::<
        crate::audio_engine::mapped_source_clock::PhaseLocalSpanView,
    >());
    assert!(!std::mem::needs_drop::<
        crate::audio_engine::source_playback::CompactLocalPlaybackView,
    >());
    let child = std::thread::Builder::new()
        .name("variable-productive-two-mib".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for (initial, correcting) in [(23, false), (3200, true)] {
                let (mut fixture, snapshot) = pending_phase_at(6, initial);
                let mut output = [0.; 1024];
                let (_, allocations) =
                    observe_current_thread(|| consume_pending(&mut fixture, &mut output));
                assert_eq!(
                    snapshot.observation().unwrap().state,
                    "applied",
                    "{:?}",
                    snapshot.observation()
                );
                assert!(
                    (0..6).all(|id| fixture.dry.path.voice(id).source_playback.is_mapped_phase())
                );
                assert!(output.iter().all(|sample| sample.is_finite()));
                assert!(output.iter().any(|sample| *sample != 0.));
                assert_eq!(allocations, ThreadAllocations::default());
                assert_consumed_region(&fixture, correcting);
            }
        })
        .unwrap();
    child.join().unwrap();
}

#[test]
#[ignore = "descriptive consuming profile; Root runs serially after actual combined POST review"]
fn descriptive_variable_actual_constructor_renderer_cohorts_report_median_max_and_limits() {
    for (region, initial, correcting) in [("wait", 23, false), ("correcting_window", 3200, true)] {
        for lanes in [2, 4, 6, 10] {
            let mut elapsed = Vec::new();
            for repeat in 0..4 {
                let (mut fixture, snapshot) = pending_phase_at(lanes, initial);
                let mut output = [0.; 1024];
                let start = Instant::now();
                let (_, allocations) =
                    observe_current_thread(|| consume_pending(&mut fixture, &mut output));
                let millis = start.elapsed().as_secs_f64() * 1000.;
                assert_eq!(allocations, ThreadAllocations::default());
                assert_eq!(snapshot.observation().unwrap().state, "applied");
                assert!(
                    (0..lanes).all(|id| fixture
                        .dry
                        .path
                        .voice(id)
                        .source_playback
                        .is_mapped_phase())
                );
                let (frames, corrected) = assert_consumed_region(&fixture, correcting);
                if repeat == 0 {
                    println!(
                        "variable_productive_region region={region} actual_phase_frames={frames} actual_correcting_frames={corrected} expected_correcting_rate=63/32"
                    );
                }
                if repeat > 0 {
                    elapsed.push(millis);
                }
            }
            elapsed.sort_by(f64::total_cmp);
            let nominal = 512. / f64::from(RATE) * 1000.;
            println!(
                "variable_productive_profile region={region} active={lanes} fixed_capacity=32 frames_per_lane=512 warmups=1 repeats=3 median_ms={:.4} max_ms={:.4} nominal_ms={nominal:.7} within_nominal={}; includes original drains, actual-E phase construction/all32 claim, consuming full dry renderer/history/copied advance; Rust requests observed zero; setup precedes measurement; no Native/device/music/worstcase acceptance; earlier performance negatives remain open",
                elapsed[1],
                elapsed[2],
                elapsed[2] <= nominal
            );
        }
    }
}

#[test]
#[ignore = "descriptive settled phase-read profile; Root runs after frozen POST"]
fn descriptive_settled_phase_read_renderer_reports_affected_productive_cost() {
    for lanes in [2, 4, 6, 10] {
        let mut elapsed = Vec::new();
        for repeat in 0..4 {
            let mut fixture = VariablePath::small(lanes, 0);
            let mode = fixture.mode();
            let phase = fixture.effect();
            assert!(Arc::ptr_eq(&mode, &phase));
            fixture.dry.path.render(1);
            for id in 0..lanes {
                let geometry = phase.prepared_map(id).unwrap().geometry();
                assert!(
                    fixture
                        .dry
                        .path
                        .voice(id)
                        .source_playback
                        .mapped_phase_settled(&geometry)
                        .unwrap()
                );
                assert_eq!(fixture.dry.path.feedback(id)["phase_read_confirmed"], true);
            }
            fixture.drain();
            let mut output = [0.; 1024];
            let start = Instant::now();
            let (_, allocations) = observe_current_thread(|| {
                fixture
                    .dry
                    .render_into(&mut output, &mut ImmediateAudioBufferRetirement)
            });
            let millis = start.elapsed().as_secs_f64() * 1000.;
            assert_eq!(allocations, ThreadAllocations::default());
            assert!(output.iter().all(|value| value.is_finite()));
            assert!(output.iter().any(|value| *value != 0.));
            for id in 0..lanes {
                assert_eq!(fixture.dry.path.feedback(id)["phase_read_confirmed"], true);
            }
            if repeat > 0 {
                elapsed.push(millis);
            }
        }
        elapsed.sort_by(f64::total_cmp);
        println!(
            "variable_settled_read_profile active={lanes} frames=512 warmups=1 repeats=3 median_ms={:.4} max_ms={:.4} nominal_ms=10.6666667 rust_allocations=0; actual settled dry read before/after; no device or worst-case acceptance",
            elapsed[1], elapsed[2]
        );
    }
}

// Independently retained stdlib Fraction source is inserted below after fixture generation.
const FRACTION_ORACLE: &str = r####""""Independent original-seconds common-phase reference, using only the stdlib.

The input master values are actual binary64 Transport evaluations. Source axes,
inverse/forward maps, debt, window count and reader fractions use Fraction.
This is bounded numerical evidence, not source custody or musical acceptance.
"""

import json
import struct
from fractions import Fraction as F


def bits(value):
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def master(frame, fpb=24000.0, anchor_frame=0, anchor=0.0):
    return anchor + (float(frame) - float(anchor_frame)) / fpb


def axes(anchors, rate):
    return [(F(seconds) * rate, F(beat)) for seconds, beat in anchors]


def affine(points, value, inverse=False):
    if inverse:
        points = [(beat, source) for source, beat in points]
    for left, right in zip(points, points[1:]):
        if left[0] <= value <= right[0]:
            return left[1] + (value-left[0]) * (right[1]-left[1]) / (right[0]-left[0])
    raise ValueError("original trusted coverage is required")


def seed_for(points, beginning, quarters, slope):
    seeds = []
    for left, right in zip(points, points[1:]):
        if (right[0]-left[0]) / (right[1]-left[1]) != slope:
            continue
        low, high = max(beginning, left[1]), min(beginning+quarters, right[1])
        if high <= low:
            continue
        for exponent in range(47, -127, -1):
            width = F(2) ** exponent
            aligned = -((-low) // width) * width
            if aligned + width <= high:
                seeds.append((exponent, aligned))
                break
    return max(seeds, key=lambda item: (item[0], -item[1])) if seeds else None


def choose(points, beginning, quarters, beta0, m0, fpb, launch_master=0, grid_origin=None):
    slopes = [(r[0]-l[0])/(r[1]-l[1]) for l, r in zip(points, points[1:])
              if r[1] > beginning and l[1] < beginning+quarters]
    if grid_origin is None:
        target = beginning-F(launch_master)
    elif grid_origin < 0:
        # This fixture's original first usable segment touches physical zero.
        # Extend only the explicit phase target; ordinary affine stays strict.
        left, right = points[:2]
        assert left[0] == 0
        target = left[1]+(F(grid_origin)-left[0])*(right[1]-left[1])/(right[0]-left[0])
    else:
        target = affine(points,F(grid_origin))
    residue = (target+F(m0)-beta0) % quarters
    debts = [residue] if residue == 0 else sorted([residue, residue-quarters], key=lambda x: (abs(x), x < 0))
    for debt in debts:
        if debt == 0:
            return (debt, 6, None)
        sigma = 1 if debt > 0 else -1
        for k in range(6, 65):
            h = 1 << k
            if all(F(1, 2) <= slope/F(fpb)*(1+F(sigma, h)) <= 2 for slope in slopes):
                return (debt, k, None)
            selected = min(slopes) if sigma > 0 else max(slopes)
            seed = seed_for(points, beginning, quarters, selected)
            if seed is not None and F(1, 2) <= selected/F(fpb)*(1+F(sigma, h)) <= 2:
                w, a = seed
                delta = F(2) ** (w-k-(1 if sigma > 0 else 0))
                return (debt, k, (a, delta))
    raise ValueError("no bounded rate candidate")


def beta_at(beta0, elapsed, quarters, programme):
    debt, k, window = programme
    if not debt:
        return beta0 + elapsed
    sigma, h = (1 if debt > 0 else -1), 1 << k
    if window is None:
        correction = min(elapsed/h, abs(debt))
    else:
        seed, delta = window
        # Exact ceil, also for nonintegral beta0 and signed beat axes.
        entry = seed + (-((seed-beta0)//quarters))*quarters
        if entry < beta0:
            entry += quarters
        v = elapsed - (entry-beta0)
        if v < 0:
            return beta0 + elapsed
        cycle, remainder = divmod(v, quarters-sigma*delta)
        warped = (h+sigma)*remainder/h if remainder <= h*delta else remainder+sigma*delta
        unbounded = entry + cycle*quarters + warped - beta0-elapsed
        assert sigma*unbounded >= 0
        correction = min(abs(unbounded), abs(debt))
    return beta0+elapsed+sigma*correction


def evaluate(anchors, rate, start, end, quarters, raw_frame, raw_fraction, master0, masters, fpb, launch_master=0, grid_origin=None):
    points = axes(anchors, rate)
    beginning = affine(points, F(start))
    quarters = affine(points, F(end))-beginning if quarters is None else F(quarters)
    beta0 = affine(points, F(raw_frame)+F(raw_fraction))
    programme = choose(points, beginning, quarters, beta0, master0, fpb, launch_master, grid_origin)
    result = []
    for observed in masters:
        beta = beta_at(beta0, F(observed)-F(master0), quarters, programme)
        exact = affine(points, beginning+(beta-beginning) % quarters, True)
        whole = exact.numerator // exact.denominator
        remainder = exact-whole
        fraction = raw_fraction if remainder == F(raw_fraction) else float(remainder)
        if fraction == 1:
            whole, fraction = whole+1, 0.0
        if F(whole)+F(fraction) >= end:
            whole, fraction = start, 0.0
        gain = struct.unpack("<I", struct.pack("<f", fraction))[0]
        result.append((whole, bits(fraction), whole, None if fraction == 0 else (whole+1 if whole+1 < end else start), gain))
    return result


def main():
    vectors = []
    for anchors, quarters, end, capture in [
        ([(0., 0.), (2., 8.), (10., 24.), (18., 32.)], 32., 864000, 577639),
        ([(0., 0.), (16., 64.), (80., 192.), (144., 256.)], 256., 6912000, 4609638),
    ]:
        observations = [capture-1, capture]
        pairs = evaluate(anchors, 48000, 0, end, quarters, 13, 0., 0., [master(n) for n in observations], 24000.)
        vectors.append({"quarters": quarters, "frames": observations, "pairs": pairs})
    expected = [(483275, bits(1-3213/2**46)), (483278, bits(11/2**39)),
                (3843274, bits(1/32-1953/2**44)), (3843276, bits(81/2**38))]
    assert [(row[0], row[1]) for case in vectors for row in case["pairs"]] == expected
    print(json.dumps({"status": "PASS", "actual_binary64_capture_vectors": vectors}, indent=2))
    # A constant-rate shortcut and an ideal rational master disagree at capture.
    assert F(master(577638)) != F(577638, 24000)
    print("NEGATIVE: ideal rational time cannot replace actual binary64 Transport time")


if __name__ == "__main__":
    main()
"####;

#[test]
#[ignore = "descriptive component attribution; Root runs only after the frozen systems review"]
fn descriptive_full_fraction_claim_and_installed_renderer_components() {
    let mut constructors = Vec::with_capacity(3);
    let mut renders = Vec::with_capacity(3);
    for repeat in 0..4 {
        let (mut fixture, snapshot) = pending_full_fraction(6);
        fixture.drain();
        let actual_e = fixture.dry.path.timeline.output_frame();
        let original_transport = fixture.dry.path.timeline;
        let mut retirement = ImmediateAudioBufferRetirement;
        let started = Instant::now();
        let (_, constructor_requests) =
            observe_current_thread(|| fixture.dry.render_into(&mut [], &mut retirement));
        let constructor_ms = started.elapsed().as_secs_f64() * 1000.;
        assert_eq!(constructor_requests, ThreadAllocations::default());
        let observation = snapshot.observation().unwrap();
        assert_eq!(observation.state, "applied", "{observation:?}");
        assert_eq!(observation.execution_frame, Some(actual_e));
        assert_eq!(fixture.dry.path.timeline.output_frame(), actual_e);
        assert!(
            fixture
                .dry
                .path
                .timeline
                .matches_clock_epoch(&original_transport)
        );
        let attempt = observation.attempt;
        // Selection and view setup are inspected outside both timed callbacks.
        let widths = (0..6)
            .map(|id| {
                fixture
                    .dry
                    .path
                    .voice(id)
                    .source_playback
                    .local_compact_view()
                    .unwrap()
                    .unwrap()
                    .full_fraction_phase_split_bits_for_test()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(widths.iter().all(|bits| *bits == 1024));
        if repeat == 0 {
            println!(
                "full_fraction_component_split_widths active=6 selected_bits={widths:?}; inspected outside timed regions"
            );
        }
        let mut output = [0.; 1024];
        fixture.drain();
        let started = Instant::now();
        let (_, render_requests) =
            observe_current_thread(|| fixture.dry.render_into(&mut output, &mut retirement));
        let render_ms = started.elapsed().as_secs_f64() * 1000.;
        assert_eq!(render_requests, ThreadAllocations::default());
        assert_eq!(snapshot.observation().unwrap().state, "applied");
        assert_eq!(snapshot.observation().unwrap().attempt, attempt);
        assert!(output.iter().all(|v| v.is_finite()));
        assert!(output.iter().any(|v| *v != 0.));
        assert_full_fraction_consumed(&fixture);
        if repeat > 0 {
            constructors.push(constructor_ms);
            renders.push(render_ms);
        }
    }
    constructors.sort_by(f64::total_cmp);
    renders.sort_by(f64::total_cmp);
    println!(
        "full_fraction_components active=6 fixed_capacity=32 frames_per_lane=512 warmups=1 repeats=3 constructor_median_ms={:.4} constructor_max_ms={:.4} installed_render_median_ms={:.4} installed_render_max_ms={:.4}; empty callback executes original actual-E PHASE claim; following callback consumes512 correcting frames without re-admission; both assert zero Rust requests; setup and class inspection excluded; these component times are not summed as an exact decomposition; unchanged combined2/4/6/10 profile remains budget authority; no Native/device/music/Human acceptance",
        constructors[1], constructors[2], renders[1], renders[2]
    );
}

#[test]
#[ignore = "descriptive component attribution; Root runs only after the frozen systems review"]
fn descriptive_variable_phase_constructor_and_installed_renderer_components() {
    for (region, initial, correcting) in [("wait", 23, false), ("correcting_window", 3200, true)] {
        let mut constructors = Vec::with_capacity(3);
        let mut renders = Vec::with_capacity(3);
        for repeat in 0..4 {
            let (mut fixture, snapshot) = pending_phase_at(6, initial);
            // Original setup, publication and both drains precede measurement.
            // This empty-output callback still executes the SAME original
            // snapshot's pending all32 PHASE claim at actual E.
            fixture.drain();
            let actual_e = fixture.dry.path.timeline.output_frame();
            let original_transport = fixture.dry.path.timeline;
            let mut retirement = ImmediateAudioBufferRetirement;
            let started = Instant::now();
            let (_, constructor_requests) = observe_current_thread(|| {
                fixture.dry.render_into(&mut [], &mut retirement);
            });
            let constructor_ms = started.elapsed().as_secs_f64() * 1000.;
            assert_eq!(constructor_requests, ThreadAllocations::default());
            assert_eq!(
                snapshot.observation().unwrap().state,
                "applied",
                "{:?}",
                snapshot.observation()
            );
            assert_eq!(
                snapshot.observation().unwrap().execution_frame,
                Some(actual_e)
            );
            assert_eq!(fixture.dry.path.timeline.output_frame(), actual_e);
            assert!(
                fixture
                    .dry
                    .path
                    .timeline
                    .matches_clock_epoch(&original_transport)
            );
            let applied_attempt = snapshot.observation().unwrap().attempt;
            assert!((0..6).all(|id| fixture.dry.path.voice(id).source_playback.is_mapped_phase()));

            // No publication or new phase request occurs between the paired
            // measurements. The renderer consumes an ALREADY installed programme.
            let mut output = [0.; 1024];
            fixture.drain();
            let started = Instant::now();
            let (_, render_requests) = observe_current_thread(|| {
                fixture.dry.render_into(&mut output, &mut retirement);
            });
            let render_ms = started.elapsed().as_secs_f64() * 1000.;
            assert_eq!(render_requests, ThreadAllocations::default());
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            assert_eq!(
                snapshot.observation().unwrap().attempt,
                applied_attempt,
                "already installed rendering must not start a hidden new PHASE construction"
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
            assert!(output.iter().any(|sample| *sample != 0.));
            let (frames, corrected) = assert_consumed_region(&fixture, correcting);
            assert_eq!((frames, corrected), (512, if correcting { 491 } else { 0 }));
            if repeat > 0 {
                constructors.push(constructor_ms);
                renders.push(render_ms);
            }
        }
        constructors.sort_by(f64::total_cmp);
        renders.sort_by(f64::total_cmp);
        println!(
            "variable_phase_components region={region} active=6 fixed_capacity=32 frames_per_lane=512 warmups=1 repeats=3 constructor_median_ms={:.4} constructor_max_ms={:.4} installed_render_median_ms={:.4} installed_render_max_ms={:.4}; constructor is the actual empty-output original PHASE claim; render consumes its already installed programme and records512 actual frames with {} correcting frames; both measured callbacks observe zero Rust requests; setup/drains and assertions precede/follow measurement; paired component timings are NOT summed as an exact decomposition; unchanged combined cohort profile remains the budget authority; no Native/device/music/worstcase acceptance",
            constructors[1],
            constructors[2],
            renders[1],
            renders[2],
            if correcting { 491 } else { 0 }
        );
    }
}
