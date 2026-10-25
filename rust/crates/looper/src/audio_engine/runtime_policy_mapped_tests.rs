//! Actual cold-source/ACK/map-worker admission, due-frame conversion and dry rendering.
//! Synthetic timing labels and generated PCM do not establish device or musical acceptance.
//! Public scalar births expose +0; raw -0 remains covered by the separate numerical fixtures.

#[path = "runtime_policy_general_tests.rs"]
mod general_tests;

#[path = "runtime_policy_mapped_stage_tests.rs"]
mod static_stage_tests;

use super::*;
use crate::audio_engine::constant_timing::l3_constant_timing_fixture;
use crate::audio_engine::mapped_source_clock::observe_compact_queries_for_test;
use crate::audio_engine::pad_start::PadStartCustody;
use crate::audio_engine::source_map_admission::{RuntimeMapClass, SourceMapRow};
use crate::audio_engine::source_reader::{
    SourceReadPlan, StemReadSets, StemRenderSelection, StemTransition,
};
use crate::test_memory::{ThreadAllocations, observe_current_thread};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const DYADIC_END: usize = 1500;
const DYADIC_PERIOD: f64 = 1.0 / 64.0;
const BLOCK: usize = 512;
const PCM_FRAMES: usize = 4096;

#[path = "runtime_policy_variable_phase_tests.rs"]
mod variable_phase_tests;

struct DryMappedPath {
    path: Path,
    jobs: Vec<OfflineAnalysisJob>,
    deliveries: Vec<LoaderEventDelivery>,
    lanes: usize,
    intent_revision: u64,
    tiny: bool,
    parameters: rtrb::Consumer<crate::messages::ControlParameterMessage>,
}

impl DryMappedPath {
    fn new(lanes: usize, initial_frames: usize) -> Self {
        Self::new_with_shape(lanes, initial_frames, false)
    }

    fn new_with_shape(lanes: usize, initial_frames: usize, tiny: bool) -> Self {
        Self::new_with_shape_and_pending_birth(lanes, initial_frames, tiny, false)
    }

    fn new_with_shape_and_pending_birth(
        lanes: usize,
        initial_frames: usize,
        tiny: bool,
        pending_birth: bool,
    ) -> Self {
        assert!((1..=32).contains(&lanes));
        let pcm = (0..PCM_FRAMES)
            .map(|frame| ((frame * 73 % 28001) as i32 - 14000) as i16)
            .collect::<Vec<_>>();
        let mut source = ColdCustody::new_selected_with_signal(None, &pcm);
        for id in 2..lanes {
            source.load_and_ack_selected_with_frames(id, None, PCM_FRAMES);
        }
        source.engine.test_output_format = Some((RATE, 2));
        source.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            source.producer.clone(),
            InputClock::new(),
            source.engine.input_runtime_ownership.clone(),
        ));
        source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_render_format(RATE, 2)
            .unwrap();
        source.mixer.set_bpm_lock(true);
        let master = if tiny { 2.0 / f64::from(RATE) } else { MASTER };
        let period = if tiny {
            36.0 / f64::from(RATE)
        } else {
            DYADIC_PERIOD
        };
        source.mixer.set_master_period(master);
        source.mixer.set_speed(1.0);
        source.mixer.set_volume(1.0);
        let mut timeline = TransportTimeline::new(RATE);
        assert!(timeline.set_master_period(master));
        timeline.complete_bootstrap();
        let path = Path {
            source,
            timeline,
            scheduler: FixedCapacityScheduler::new(),
            events: Vec::with_capacity(8192),
        };
        let (_producer, parameters) = rtrb::RingBuffer::new(1);
        let mut fixture = Self {
            path,
            jobs: Vec::with_capacity(lanes),
            deliveries: Vec::with_capacity(lanes),
            lanes,
            intent_revision: 7,
            tiny,
            parameters,
        };
        for id in 0..lanes {
            fixture
                .path
                .source
                .engine
                .set_pad_timing_intent(id, "automatic")
                .unwrap();
            let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
                &fixture.path.source.engine,
                &fixture.path.source.producer,
                id,
                period,
            );
            fixture.path.drain();
            assert_eq!(timing.publication_status().unwrap(), "accepted");
            fixture.path.source.mixer.set_pad_key_lock(id, false);
            let binding = fixture
                .path
                .source
                .engine
                .current_input_runtime_pad_binding(id)
                .unwrap()
                .unwrap();
            assert_eq!(
                binding.binding.accepted.unwrap().period_seconds.to_bits(),
                period.to_bits()
            );
            let (job, delivery) = if tiny {
                fixture.map_anchors(id, &[(0.0, 0.0), (3.0 / 128.0, 500.0)])
            } else {
                fixture.map(id, 0.75, 1.0 / 128.0)
            };
            fixture.jobs.push(job);
            fixture.deliveries.push(delivery);
        }
        if pending_birth {
            fixture.publish_with_selection(true, 1, None, true, true);
        } else {
            fixture.publish(false, 1);
        }
        for id in 0..lanes {
            let start = fixture.start(id);
            assert_eq!(
                start.sync_request().mode,
                if tiny || pending_birth {
                    SourceSyncMode::Legacy
                } else {
                    SourceSyncMode::Fixed
                }
            );
            if pending_birth {
                assert_eq!(start.requested_sync_request().mode, SourceSyncMode::Mapped);
            }
            assert!(fixture.path.voice(id).mapped_start.is_none());
        }
        fixture.path.render(initial_frames);
        assert_eq!(
            fixture
                .path
                .timeline
                .master_period_seconds()
                .map(f64::to_bits),
            Some(master.to_bits())
        );
        fixture
    }

    fn map(
        &self,
        id: usize,
        middle_beat: f64,
        middle_seconds: f64,
    ) -> (OfflineAnalysisJob, LoaderEventDelivery) {
        self.map_anchors(
            id,
            &[(0.0, 0.0), (middle_seconds, middle_beat), (1.0 / 32.0, 2.0)],
        )
    }

    fn map_anchors(
        &self,
        id: usize,
        anchors: &[(f64, f64)],
    ) -> (OfflineAnalysisJob, LoaderEventDelivery) {
        self.map_anchors_with_metadata(id, anchors, None, true)
    }

    fn map_anchors_with_metadata(
        &self,
        id: usize,
        anchors: &[(f64, f64)],
        coverage: Option<&[&str]>,
        verified_meter: bool,
    ) -> (OfflineAnalysisJob, LoaderEventDelivery) {
        let manifest: Value = serde_json::from_str(
            &self
                .path
                .source
                .engine
                .cold_source_manifest(id)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let map = SourceBeatmap::manual(
            manifest["descriptor"]["decoder"].clone(),
            0.0,
            DYADIC_PERIOD,
        )
        .unwrap();
        let mut value: Value = serde_json::from_str(map.canonical_record()).unwrap();
        let template = value["record"]["accepted"]["anchors"][0].clone();
        value["record"]["accepted"]["anchors"] = Value::Array(
            anchors
                .iter()
                .copied()
                .enumerate()
                .map(|(index, (seconds, beat))| {
                    let mut anchor = template.clone();
                    anchor["id"] = json!(format!("dyadic-productive-{index}"));
                    anchor["source_seconds_bits"] = json!(format!("{:016x}", seconds.to_bits()));
                    anchor["beat_bits"] = json!(format!("{:016x}", beat.to_bits()));
                    anchor
                })
                .collect(),
        );
        let interval = value["record"]["accepted"]["intervals"][0].clone();
        if let Some(coverage) = coverage {
            assert_eq!(coverage.len(), anchors.len() - 1);
        }
        value["record"]["accepted"]["intervals"] = Value::Array(
            (0..anchors.len() - 1)
                .map(|index| {
                    let mut interval = interval.clone();
                    if let Some(coverage) = coverage {
                        interval["coverage"] = json!(coverage[index]);
                    }
                    if !verified_meter {
                        interval["meter"]["status"] = json!("unknown");
                        interval["meter"]["downbeat_label"] = Value::Null;
                    }
                    interval
                })
                .collect(),
        );
        let map = editor::seal(value).unwrap();
        let job = self
            .path
            .source
            .engine
            .begin_source_beatmap_store_async(id, map.canonical_record().into())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let terminal = Python::attach(|py| {
                job.source_beatmap_async_status(py)
                    .unwrap()
                    .unwrap()
                    .bind(py)
                    .get_item("terminal")
                    .unwrap()
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            });
            if terminal {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "genuine dyadic map worker timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let delivery = Python::attach(|py| {
            while let Some(event) = self.path.source.engine.poll_loader_events(py).unwrap() {
                let event = event.bind(py).cast::<PyDict>().unwrap();
                if event
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .extract::<String>()
                    .unwrap()
                    == "source_beatmap_completed"
                {
                    let owner = event.get_item("_native_event_owner").unwrap().unwrap();
                    let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
                    assert!(
                        job.resolve_source_beatmap_artifact_completion(&delivery)
                            .unwrap()
                            .is_some()
                    );
                    return delivery.clone();
                }
            }
            panic!("original private delivery required");
        });
        (job, delivery)
    }

    fn publish(&self, mapped: bool, revision: u64) {
        self.publish_with_rows(mapped, revision, None);
    }

    fn publish_with_rows(&self, mapped: bool, revision: u64, bad_domain: Option<usize>) {
        self.publish_with_parent(mapped, revision, bad_domain, true);
    }

    fn publish_with_parent(
        &self,
        mapped: bool,
        revision: u64,
        bad_domain: Option<usize>,
        parent: bool,
    ) {
        self.publish_with_selection(mapped, revision, bad_domain, parent, false);
    }

    fn publish_with_selection(
        &self,
        mapped: bool,
        revision: u64,
        bad_domain: Option<usize>,
        parent: bool,
        missing_map: bool,
    ) {
        let bindings = (0..NUM_SAMPLES)
            .map(|id| {
                (id < self.lanes).then(|| {
                    self.path
                        .source
                        .engine
                        .current_input_runtime_pad_binding(id)
                        .unwrap()
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
        for (id, row) in rows.iter_mut().enumerate().take(self.lanes) {
            *row = SourceMapRow {
                sync_requested: true,
                intent_revision: self.intent_revision,
                delivery: (!missing_map).then(|| self.deliveries[id].clone()),
                ordinary_pending: missing_map,
                exact_quarters: self.tiny.then_some(1.0),
            };
        }
        let mut ends = vec![Some(self.end_seconds()); NUM_SAMPLES];
        if let Some(id) = bad_domain {
            ends[id] = Some(1.0 / 64.0);
        }
        self.path
            .source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_policy(
                true,
                (0..NUM_SAMPLES).map(|id| id < self.lanes).collect(),
                vec![0.0; NUM_SAMPLES],
                ends,
                bindings.iter().map(Option::as_ref).collect(),
                Some(vec![
                    PitchPadProjection {
                        sync: parent,
                        key_lock: false,
                        ..Default::default()
                    };
                    NUM_SAMPLES
                ]),
                Some(rows),
                GlobalTimingPolicy::parse(Some(
                    &wire(mapped || !self.tiny, mapped, revision).to_string(),
                ))
                .unwrap(),
            )
            .unwrap();
    }

    fn start(&mut self, id: usize) -> CapturedPadStart {
        let start = self
            .path
            .source
            .engine
            .capture_pad_start(id, 0.0, Some(self.end_seconds()), 1.0, false, None, None)
            .unwrap();
        self.path.drain();
        assert!(
            self.path
                .source
                .engine
                .publish_pad_start(&start, None)
                .unwrap()
        );
        self.path.drain();
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        start
    }

    fn adopt(&mut self, mapped: bool, revision: u64) -> Arc<RuntimePolicySnapshot> {
        self.publish(mapped, revision);
        let snapshot = request(&self.path);
        service(&mut self.path);
        assert_eq!(
            snapshot.observation().unwrap().state,
            "applied",
            "{:?}",
            snapshot.observation()
        );
        snapshot
    }

    fn assert_domain(&self, snapshot: &RuntimePolicySnapshot) {
        for id in 0..self.lanes {
            assert_eq!(snapshot.map_preparation(id).class, RuntimeMapClass::Ready);
            let domain = snapshot.map_preparation(id).domain.unwrap();
            assert_eq!(domain.sample_frames, PCM_FRAMES);
            assert_eq!(
                domain.region,
                FrameRange {
                    start: 0,
                    end: if self.tiny { 2 } else { DYADIC_END }
                }
            );
            assert_eq!(
                domain.period.scalar_period().map(f64::to_bits),
                Some(
                    if self.tiny {
                        2.25_f64
                    } else {
                        DYADIC_END as f64
                    }
                    .to_bits()
                )
            );
            let voice = self.path.voice(id);
            assert!(
                voice
                    .source_playback
                    .matches_domain(snapshot.prepared_map(id).unwrap().geometry().domain())
            );
        }
    }

    fn end_seconds(&self) -> f64 {
        if self.tiny {
            2.0 / f64::from(RATE)
        } else {
            1.0 / 32.0
        }
    }

    fn render_into(&mut self, output: &mut [f32], retirement: &mut impl AudioBufferRetirement) {
        let start = self.path.timeline.output_frame();
        render_scheduled_audio(
            &mut self.path.source.mixer,
            &mut self.path.scheduler,
            output,
            &mut [0.0; NUM_SAMPLES],
            start,
            2,
            &mut self.path.timeline,
            &mut self.path.events,
            retirement,
        );
        self.path
            .timeline
            .advance_by_rendered_frames(output.len() / 2);
    }
}

fn assert_position(actual: FractionalSourcePosition, expected: FractionalSourcePosition) {
    assert_eq!(actual.frame, expected.frame);
    assert_eq!(actual.fraction.to_bits(), expected.fraction.to_bits());
    assert_eq!(actual.seek_mode, expected.seek_mode);
}

fn latest_plan(path: &Path, id: usize) -> SourceReadPlan {
    let mut history = SourceFeedSnapshot::new();
    assert!(
        path.voice(id)
            .source_feed_history
            .capture_into(&mut history)
    );
    if let Some(span) = history.spans().last() {
        return span.plan;
    }
    // At E=0 no read receipt exists yet. This independent oracle uses the actual
    // admitted voice's accepted domain, without manufacturing a source permit.
    let voice = path.voice(id);
    let domain = voice.source_timing.loop_domain(
        voice.sample.as_ref().unwrap().frame_count(),
        FrameRange {
            start: 0,
            end: DYADIC_END,
        },
    );
    SourceReadPlan {
        channels: 2,
        sample_frames: domain.sample_frames,
        frame_pos: voice.source_playback.position().frame,
        loop_region: domain.region,
        loop_period: domain.musical_period,
        seek_mode: voice.source_playback.position().seek_mode,
        selection: StemRenderSelection::full_mix(),
        transition: StemTransition::default(),
    }
}

fn wide_oracle(path: &Path, snapshot: &RuntimePolicySnapshot, id: usize) -> SourcePlayback {
    let voice = path.voice(id);
    let source = snapshot.prepared_map(id).unwrap();
    let wide = SourcePlayback::mapped_anchored_continuation(
        source.geometry(),
        snapshot.revision(),
        path.timeline,
        latest_plan(path, id).domain(),
        voice.source_playback.position(),
        path.timeline.output_frame(),
    )
    .unwrap();
    assert!(!wide.is_compact_mapped());
    wide
}

fn assert_mapped_reads(path: &Path, mut wide: SourcePlayback, first: u64, id: usize) {
    let mut history = SourceFeedSnapshot::new();
    assert!(
        path.voice(id)
            .source_feed_history
            .capture_into(&mut history)
    );
    let geometry = path
        .voice(id)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .geometry();
    let sample = path.voice(id).sample.as_ref().unwrap();
    let mut checked_frames = 0;
    for span in history.spans().filter(|span| span.active_start >= first) {
        assert!(span.playback.is_compact_mapped());
        for offset in 0..span.frames {
            wide.project_mapped_to(geometry, first + checked_frames + offset as u64)
                .unwrap();
            let actual = span.playback.position_at(offset).unwrap();
            assert_position(actual, wide.position());
            // Compare the original reader's physical interpolation taps as well as coordinates.
            let mut actual_plan = span.plan;
            actual_plan.frame_pos = actual.frame;
            let mut expected_plan = span.plan;
            expected_plan.frame_pos = wide.position().frame;
            assert_eq!(
                actual_plan.fractional_taps(actual.fraction),
                expected_plan.fractional_taps(wide.position().fraction)
            );
        }
        let mut actual_pcm = [vec![0.0; span.frames], vec![0.0; span.frames]];
        let mut expected_pcm = [vec![0.0; span.frames], vec![0.0; span.frames]];
        wide.project_mapped_to(geometry, first + checked_frames)
            .unwrap();
        span.plan
            .fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &span.playback,
                &mut actual_pcm,
                span.frames,
            )
            .unwrap();
        span.plan
            .fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &wide,
                &mut expected_pcm,
                span.frames,
            )
            .unwrap();
        assert_eq!(actual_pcm, expected_pcm);
        checked_frames += span.frames as u64;
    }
    assert!(checked_frames > 0);
}

#[test]
fn missing_map_birth_adopts_compatible_arrival_without_retoggle_or_region_edit() {
    let automatic_request = |path: &Path| {
        path.source
            .engine
            .request_input_runtime_policy_adoption(true)
            .unwrap();
        path.source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .runtime_policy_snapshot()
            .unwrap()
            .unwrap()
    };
    for compatible in [true, false] {
        // Original end=1/32s gives exactly1500 source frames; accepted P=1/64s
        // and both original map intervals already have an exact continuation proof.
        let mut fixture = DryMappedPath::new_with_shape_and_pending_birth(1, 17, false, true);
        let path = &mut fixture.path;
        assert_eq!(path.voice(0).sync_request.mode, SourceSyncMode::Legacy);
        let unavailable = automatic_request(path);
        assert_eq!(
            unavailable.row(0).unwrap().request.mode,
            SourceSyncMode::Mapped
        );
        assert_eq!(
            unavailable.map_preparation(0).class,
            RuntimeMapClass::Unavailable
        );
        let desired = unavailable.row(0).unwrap().request;
        service(path);
        assert_eq!(
            unavailable.observation().unwrap().reason,
            Some(RuntimePolicyError::Unsupported)
        );
        let original = path.voice(0).source_playback;
        let original_domain = original.effective_domain().unwrap();
        let original_plan = SourceReadPlan {
            frame_pos: original.position().frame,
            ..latest_plan(path, 0)
        };
        let original_taps = original_plan
            .try_fractional_taps(original.position().fraction)
            .unwrap();
        let original_source = path.voice(0).sample.as_ref().unwrap().source_address();
        let generation = path.voice(0).generation;
        let execution = path.timeline.output_frame();
        assert_eq!(
            original_domain.region,
            FrameRange {
                start: 0,
                end: DYADIC_END
            }
        );

        // The map arrives under the unchanged desired request. The negative row
        // requests750 frames while the actual original audio still owns1500.
        fixture.publish_with_rows(true, 1, (!compatible).then_some(0));
        let arrival = automatic_request(&fixture.path);
        assert_eq!(arrival.row(0).unwrap().request, desired);
        assert_eq!(arrival.map_preparation(0).class, RuntimeMapClass::Ready);
        assert!(arrival.map_current(0));
        service(&mut fixture.path);
        let voice = fixture.path.voice(0);
        assert_eq!(voice.generation, generation);
        assert_eq!(
            voice.sample.as_ref().unwrap().source_address(),
            original_source
        );
        assert_eq!(voice.source_playback.position(), original.position());
        let current_domain = voice.source_playback.effective_domain().unwrap();
        assert_eq!(current_domain.sample_frames, original_domain.sample_frames);
        assert_eq!(current_domain.region, original_domain.region);
        let observed = arrival.observation().unwrap();
        assert_eq!(observed.execution_frame, Some(execution));
        if compatible {
            assert_eq!(observed.state, "applied", "{observed:?}");
            assert_eq!(voice.sync_request, desired);
            assert!(voice.mapped_start.is_some());
            assert!(
                voice
                    .source_playback
                    .matches_domain(arrival.prepared_map(0).unwrap().geometry().domain())
            );
            assert_eq!(
                original_plan
                    .with_effective_domain(current_domain)
                    .unwrap()
                    .try_fractional_taps(voice.source_playback.position().fraction)
                    .unwrap(),
                original_taps
            );
        } else {
            assert_eq!(observed.state, "refused");
            assert_eq!(observed.reason, Some(RuntimePolicyError::Unsupported));
            assert_eq!(voice.sync_request.mode, SourceSyncMode::Legacy);
            assert!(voice.source_playback.matches_exact(&original));
            assert!(voice.mapped_start.is_none());
        }
    }
}

#[test]
fn genuine_scalar_to_compact_and_reverse_preserve_actual_e_first_pcm_and_original_domain() {
    for initial_frames in [1024, 23] {
        let mut fixture = DryMappedPath::new(2, initial_frames);
        let mut reference = DryMappedPath::new(2, initial_frames);
        let before = fixture.path.voice(0).source_playback;
        let first = fixture.path.timeline.output_frame();
        fixture.publish(true, 2);
        let snapshot = request(&fixture.path);
        fixture.assert_domain(&snapshot);
        let wide = wide_oracle(&fixture.path, &snapshot, 0);
        service(&mut fixture.path);
        assert_eq!(snapshot.observation().unwrap().execution_frame, Some(first));
        assert_eq!(snapshot.observation().unwrap().state, "applied");
        assert_position(
            fixture.path.voice(0).source_playback.position(),
            before.position(),
        );
        assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
        assert!(
            fixture
                .path
                .voice(0)
                .mapped_start
                .as_ref()
                .unwrap()
                .original_launch()
                .is_none()
        );
        assert_eq!(fixture.path.render(1), reference.path.render(1));
        fixture.path.render(1537);
        assert_mapped_reads(&fixture.path, wide, first, 0);
        let before = fixture.path.voice(0).source_playback;
        assert!(before.phase_reference_current(&fixture.path.timeline));
        assert!(fixture.path.voice(0).phase_launch_qualified);
        let plan = latest_plan(&fixture.path, 0);
        let first_pcm = fixture.path.render(1);
        // A separate freshly converted path reaches the same state through public execution.
        let mut reverse = DryMappedPath::new(2, initial_frames);
        reverse.adopt(true, 2);
        reverse.path.render(1538);
        assert_position(
            reverse.path.voice(0).source_playback.position(),
            before.position(),
        );
        assert!(
            reverse
                .path
                .voice(0)
                .source_playback
                .phase_reference_current(&reverse.path.timeline)
        );
        assert!(reverse.path.voice(0).phase_launch_qualified);
        let source = reverse
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap()
            .clone();
        let ratio = before.tempo_ratio();
        let mut expected = SourcePlayback::scalar_musical_continuation(
            before.position(),
            plan.domain(),
            ratio,
            ratio,
        )
        .unwrap();
        expected.carry_launch_reference_from(&before);
        let target = DYADIC_PERIOD / MASTER;
        if target.to_bits() != expected.rate_target().to_bits() {
            assert!(expected.set_target(target));
            expected.chunk(1024);
        }
        reverse.adopt(false, 3);
        assert!(
            reverse
                .path
                .voice(0)
                .source_playback
                .matches_exact(&expected)
        );
        assert!(!reverse.path.voice(0).source_playback.is_mapped());
        assert!(Arc::ptr_eq(
            reverse
                .path
                .voice(0)
                .mapped_start
                .as_ref()
                .unwrap()
                .effective_source()
                .unwrap(),
            &source
        ));
        assert_eq!(reverse.path.render(1), first_pcm);
        assert!(
            reverse
                .path
                .voice(0)
                .source_playback
                .matches_domain(plan.domain())
        );
    }
}

#[test]
fn equal_map_raw_revisions_preserve_full_trajectory_but_refresh_genuine_intent_witness() {
    let mut fixture = DryMappedPath::new(2, 23);
    let first = fixture.adopt(true, 2);
    fixture.path.render(37);
    let before = scalar_state(&fixture.path);
    let source = fixture
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let next = fixture.adopt(true, 3);
    assert!(!Arc::ptr_eq(&first, &next));
    for (id, _, playback, _) in &before {
        assert!(
            fixture
                .path
                .voice(*id)
                .source_playback
                .matches_exact(playback)
        );
    }
    assert!(Arc::ptr_eq(
        fixture
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap(),
        &source
    ));
    fixture.intent_revision += 1;
    fixture.publish(true, 4);
    let next = request(&fixture.path);
    let mut retired = HeldRetirement::new(64);
    drain_and_render(&mut fixture, &mut [], &mut retired);
    assert_eq!(next.observation().unwrap().state, "applied");
    for (id, _, playback, _) in &before {
        assert!(
            fixture
                .path
                .voice(*id)
                .source_playback
                .matches_exact(playback)
        );
        let current = fixture
            .path
            .voice(*id)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap();
        assert_eq!(
            current.sync_witness().intent_revision,
            fixture.intent_revision
        );
        assert!(current.same_source(first.prepared_map(*id).unwrap()));
    }
    assert_eq!(retired.maps.len(), 2);
    assert!(Arc::ptr_eq(
        retired.maps[0].effective_source().unwrap(),
        &source
    ));
    assert_eq!(fixture.path.feedback(0)["policy_revision"], 4);
}

// This callback sink reserves fixed capacity before execution, then retains
// displaced owners until the control/test thread explicitly releases it.
struct HeldRetirement {
    maps: Vec<PadStartCustody>,
    work: Vec<crate::audio_engine::input_mapping::RuntimePolicyWork>,
    slots: usize,
}

impl HeldRetirement {
    fn new(slots: usize) -> Self {
        Self {
            maps: Vec::with_capacity(64),
            work: Vec::with_capacity(64),
            slots,
        }
    }
}

impl AudioBufferRetirement for HeldRetirement {
    fn retire_runtime_policy(
        &mut self,
        work: crate::audio_engine::input_mapping::RuntimePolicyWork,
    ) {
        assert!(self.slots > 0 && self.work.len() < self.work.capacity());
        self.slots -= 1;
        self.work.push(work);
    }
    fn retire_pad_start_target(&mut self, owner: PadStartCustody) {
        assert!(self.slots > 0 && self.maps.len() < self.maps.capacity());
        self.slots -= 1;
        self.maps.push(owner);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.slots
    }
    fn retire_resident_capture(
        &mut self,
        _: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        panic!("unexpected resident capture");
    }
    fn retire_resident_cancellation(&mut self, _: Arc<std::sync::atomic::AtomicBool>) {
        panic!("unexpected resident cancellation");
    }
    fn retire_resident_transaction(&mut self, _: Box<crate::messages::ResidentTransaction>) {
        panic!("unexpected resident transaction");
    }
    fn retire_cold_adoption(&mut self, _: Arc<std::sync::atomic::AtomicU8>) {
        panic!("unexpected cold adoption");
    }
    fn retire_sample(&mut self, _: crate::messages::SampleBuffer) {
        panic!("unexpected sample retirement");
    }
    fn retire_prepared_stems(&mut self, _: crate::messages::PreparedStemSet) {
        panic!("unexpected stem retirement");
    }
    fn retire_constant_timing(
        &mut self,
        _: crate::audio_engine::constant_timing::PreparedConstantTiming,
    ) {
        panic!("unexpected timing retirement");
    }
    fn retire_global_playback_batch(
        &mut self,
        _: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        panic!("unexpected global batch");
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        _: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        panic!("unexpected timing refresh");
    }
}

fn drain_and_render(
    fixture: &mut DryMappedPath,
    output: &mut [f32],
    retirement: &mut impl AudioBufferRetirement,
) {
    drain_control_messages(
        &mut fixture.path.source.consumer,
        &mut fixture.path.scheduler,
        fixture.path.timeline.output_frame(),
        &mut TriggerQuantization::Immediate,
        &mut fixture.path.timeline,
        &mut fixture.path.source.mixer,
        &mut fixture.path.events,
        retirement,
    );
    drain_parameter_messages(
        &mut fixture.parameters,
        &mut fixture.path.source.mixer,
        &mut fixture.path.timeline,
    );
    fixture.render_into(output, retirement);
}

#[test]
fn changed_map_retains_genuine_birth_and_displaced_owner_until_off_callback_retirement() {
    for genuine_mapped_birth in [false, true] {
        let mut fixture = DryMappedPath::new(2, 23);
        let original = if genuine_mapped_birth {
            fixture.path.source.engine.stop_sample(0).unwrap();
            fixture.path.drain();
            fixture.publish(true, 2);
            let birth = fixture.start(0);
            fixture.path.render(1);
            Some(birth)
        } else {
            fixture.adopt(true, 2);
            None
        };
        let before = fixture.path.voice(0).source_playback.position();
        let old = fixture
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap()
            .clone();
        let (job, delivery) = fixture.map(0, 0.5, 1.0 / 128.0);
        fixture.jobs.push(job);
        fixture.deliveries[0] = delivery;
        fixture.publish(true, 3);
        let snapshot = request(&fixture.path);
        assert!(!old.same_source(snapshot.prepared_map(0).unwrap()));
        let mut retired = HeldRetirement::new(64);
        drain_and_render(&mut fixture, &mut [], &mut retired);
        assert_eq!(
            snapshot.observation().unwrap().state,
            "applied",
            "{:?}",
            snapshot.observation()
        );
        assert_position(fixture.path.voice(0).source_playback.position(), before);
        let custody = fixture.path.voice(0).mapped_start.as_ref().unwrap();
        match original {
            Some(ref birth) => assert!(custody.original_launch().unwrap().same_owner(birth)),
            None => assert!(custody.original_launch().is_none()),
        }
        assert!(Arc::ptr_eq(
            custody.effective_source().unwrap(),
            snapshot.prepared_map(0).unwrap()
        ));
        assert_eq!(retired.maps.len(), 1);
        assert!(Arc::ptr_eq(
            retired.maps[0].effective_source().unwrap(),
            &old
        ));
        assert!(retired.maps[0].original_launch().is_none());
        let weak = Arc::downgrade(&old);
        drop(old);
        assert!(weak.upgrade().is_some());
        drop(retired); // This is the non-callback test/control thread.
        if genuine_mapped_birth {
            assert!(
                weak.upgrade().is_some(),
                "the original genuine launch still pins its own map"
            );
        } else {
            assert!(weak.upgrade().is_none());
        }
    }
}

#[test]
fn incompatible_last_lane_refuses_all_32_original_copies_then_real_domain_repair_recovers() {
    let mut fixture = DryMappedPath::new(32, 23);
    let before = scalar_state(&fixture.path);
    assert_eq!(before.len(), 32);
    fixture.publish_with_rows(true, 2, Some(31));
    let refused = request(&fixture.path);
    service(&mut fixture.path);
    unchanged(&fixture.path, &before);
    assert_eq!(
        refused.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    let current = fixture.adopt(true, 3);
    fixture.assert_domain(&current);
    for (id, generation, playback, _) in before {
        let voice = fixture.path.voice(id);
        assert_eq!(voice.generation, generation);
        assert_position(voice.source_playback.position(), playback.position());
        assert!(voice.source_playback.is_compact_mapped());
        assert_eq!(voice.sync_request.global_revision, Some(3));
    }
    assert!(std::mem::size_of::<[SourcePlayback; 32]>() <= 16 * 1024);
}

#[test]
fn cancelled_original_map_before_final_claim_refuses_entire_dry_cohort() {
    let mut fixture = DryMappedPath::new(2, 23);
    fixture.publish(true, 2);
    let snapshot = request(&fixture.path);
    fixture.path.drain();
    drain_parameter_messages(
        &mut fixture.parameters,
        &mut fixture.path.source.mixer,
        &mut fixture.path.timeline,
    );
    let before = scalar_state(&fixture.path);
    let job = fixture.jobs.remove(0);
    crate::audio_engine::mixer::RtMixer::with_runtime_policy_before_claim_for_test(
        move || {
            job.cancel();
        },
        || {
            fixture.path.render(0);
        },
    );
    unchanged(&fixture.path, &before);
    assert!(!snapshot.map_current(0));
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    assert!(fixture.path.voice(0).mapped_start.is_none());
}

#[test]
fn cancelled_pending_ready_map_recaptures_unavailable_once_then_1500_quiet_polls_and_real_delivery_recovers()
 {
    let mut fixture = DryMappedPath::new(2, 23);
    let mut reference = DryMappedPath::new(2, 23);
    fixture.publish(true, 2);
    let ready = request(&fixture.path);
    assert_eq!(ready.map_preparation(0).class, RuntimeMapClass::Ready);
    fixture.jobs[0].cancel();
    service(&mut fixture.path);
    assert_eq!(
        ready.observation().unwrap().reason,
        Some(RuntimePolicyError::SourceChanged)
    );
    let unavailable = automatic_mapped(&fixture.path);
    assert!(!Arc::ptr_eq(&ready, &unavailable));
    assert_eq!(unavailable.revision(), ready.revision() + 1);
    assert_eq!(
        unavailable.map_preparation(0).class,
        RuntimeMapClass::Unavailable
    );
    service(&mut fixture.path);
    let refused = unavailable.observation().unwrap();
    assert_eq!(refused.reason, Some(RuntimePolicyError::Unsupported));
    for _ in 0..1500 {
        assert!(Arc::ptr_eq(&unavailable, &automatic_mapped(&fixture.path)));
        assert_eq!(unavailable.observation(), Some(refused));
        assert!(fixture.path.source.consumer.is_empty());
        assert_eq!(fixture.path.render(3), reference.path.render(3));
    }
    let (job, delivery) = fixture.map(0, 0.75, 1.0 / 128.0);
    fixture.jobs.push(job);
    fixture.deliveries[0] = delivery;
    fixture.publish(true, 3);
    let recovered = automatic_mapped(&fixture.path);
    assert_eq!(recovered.map_preparation(0).class, RuntimeMapClass::Ready);
    service(&mut fixture.path);
    assert_eq!(
        recovered.observation().unwrap().state,
        "applied",
        "{:?}",
        recovered.observation()
    );
    assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
}

#[test]
fn cancellation_after_mode_only_adoption_recaptures_once_and_keeps_effective_pin_and_pcm() {
    let mut fixture = DryMappedPath::new(2, 23);
    let mut reference = DryMappedPath::new(2, 23);
    let snapshot = fixture.adopt(true, 2);
    reference.adopt(true, 2);
    let source = fixture
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let playback = fixture.path.voice(0).source_playback;
    fixture.jobs[0].cancel();
    assert!(!snapshot.map_current(0));
    assert!(!playback.is_mapped_phase());
    let unavailable = automatic_mapped(&fixture.path);
    assert!(!Arc::ptr_eq(&snapshot, &unavailable));
    assert_eq!(unavailable.revision(), snapshot.revision() + 1);
    assert_eq!(
        unavailable.map_preparation(0).class,
        RuntimeMapClass::Unavailable
    );
    service(&mut fixture.path);
    let observed = unavailable.observation();
    assert_eq!(
        observed.unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    for _ in 0..64 {
        assert!(Arc::ptr_eq(&unavailable, &automatic_mapped(&fixture.path)));
        assert_eq!(unavailable.observation(), observed);
        assert!(fixture.path.source.consumer.is_empty());
        assert!(
            fixture
                .path
                .voice(0)
                .source_playback
                .matches_exact(&playback)
        );
    }
    assert!(Arc::ptr_eq(
        fixture
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap(),
        &source
    ));
    assert_eq!(fixture.path.render(1537), reference.path.render(1537));
}

#[test]
fn mapped_away_back_publications_never_acknowledge_the_old_original_attempt() {
    let mut fixture = DryMappedPath::new(2, 23);
    fixture.publish(true, 2);
    let old = request(&fixture.path);
    fixture.publish(false, 3);
    let away = request(&fixture.path);
    fixture.publish(true, 4);
    let current = request(&fixture.path);
    service(&mut fixture.path);
    assert_eq!(
        old.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    assert_eq!(
        away.observation().unwrap().reason,
        Some(RuntimePolicyError::Stale)
    );
    assert_eq!(current.observation().unwrap().state, "applied");
    assert_eq!(fixture.path.voice(0).sync_request.global_revision, Some(4));
}

#[test]
fn mapped_replacement_reserves_every_displaced_owner_before_claim_and_keeps_exact_pending_work() {
    let mut fixture = DryMappedPath::new(2, 23);
    fixture.adopt(true, 2);
    let mut reference = DryMappedPath::new(2, 23);
    reference.adopt(true, 2);
    let (job, delivery) = fixture.map(0, 0.5, 1.0 / 128.0);
    fixture.jobs.push(job);
    fixture.deliveries[0] = delivery;
    fixture.publish(true, 3);
    let snapshot = request(&fixture.path);
    let before = scalar_state(&fixture.path);
    // The control work clone consumes one slot when it schedules the original.
    // One remaining slot fits due-work retirement but cannot also hold the map.
    let mut retired = HeldRetirement::new(2);
    drain_and_render(&mut fixture, &mut [], &mut retired);
    unchanged(&fixture.path, &before);
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("original due work");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    assert!(retired.maps.is_empty());
    assert_eq!(retired.work.len(), 1);
    assert!(std::ptr::eq(&*retired.work[0], snapshot.as_ref()));
    let attempt = snapshot.observation().unwrap().attempt;
    let mut old_audio = [0.0; 14];
    fixture.render_into(&mut old_audio, &mut retired);
    assert_eq!(old_audio.as_slice(), reference.path.render(7).as_slice());
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    assert_eq!(fixture.path.timeline.output_frame(), 30);
    assert_eq!(retired.slots, 1);
    let before_retry = scalar_state(&fixture.path);
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("same original due work after old audio");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    // Deferred work retries only at a later actual callback, after the real
    // seven-frame old-audio interval and the newly available owner capacity.
    retired.slots = 64;
    drain_and_render(&mut fixture, &mut [], &mut retired);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert_eq!(snapshot.observation().unwrap().attempt, attempt);
    assert_eq!(snapshot.observation().unwrap().execution_frame, Some(30));
    for (id, generation, playback, _) in before_retry {
        assert_eq!(fixture.path.voice(id).generation, generation);
        assert_position(
            fixture.path.voice(id).source_playback.position(),
            playback.position(),
        );
    }
    assert_eq!(retired.maps.len(), 1);
    assert_eq!(retired.work.len(), 2);
    assert!(
        retired
            .work
            .iter()
            .all(|work| std::ptr::eq(&**work, snapshot.as_ref()))
    );
}

#[test]
fn competing_real_global_lease_preserves_pending_original_then_retries_at_actual_e() {
    let mut fixture = DryMappedPath::new(2, 23);
    let mut reference = DryMappedPath::new(2, 23);
    fixture.publish(true, 2);
    let snapshot = request(&fixture.path);
    fixture.path.drain();
    settle_parameters(&mut fixture.path);
    let ownership = fixture.path.source.engine.input_runtime_ownership.clone();
    let before = scalar_state(&fixture.path);
    let lease =
        crate::audio_engine::GlobalTimingLease::acquire(&ownership.global_timing_publishing)
            .unwrap();
    fixture.path.render(0);
    unchanged(&fixture.path, &before);
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("same original pending work");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    let attempt = snapshot.observation().unwrap().attempt;
    drop(lease);
    assert_eq!(fixture.path.render(7), reference.path.render(7));
    assert_eq!(snapshot.observation().unwrap().state, "pending");
    assert_eq!(fixture.path.timeline.output_frame(), 30);
    let before_retry = scalar_state(&fixture.path);
    let ScheduledCommand::AdoptRuntimePolicy(work) =
        &fixture.path.scheduler.deferred_due().unwrap().command
    else {
        panic!("same original pending work after old audio");
    };
    assert!(std::ptr::eq(&**work, snapshot.as_ref()));
    fixture.path.render(0);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    assert_eq!(snapshot.observation().unwrap().attempt, attempt);
    assert_eq!(snapshot.observation().unwrap().execution_frame, Some(30));
    for (id, generation, playback, _) in before_retry {
        assert_eq!(fixture.path.voice(id).generation, generation);
        assert_position(
            fixture.path.voice(id).source_playback.position(),
            playback.position(),
        );
    }
}

#[test]
fn real_global_publisher_before_claim_stales_policy_but_after_lease_is_refused() {
    for after_lease in [false, true] {
        let mut fixture = DryMappedPath::new(2, 23);
        fixture.publish(true, 2);
        let snapshot = request(&fixture.path);
        fixture.path.drain();
        settle_parameters(&mut fixture.path);
        let before = scalar_state(&fixture.path);
        let ownership = fixture.path.source.engine.input_runtime_ownership.clone();
        let (producer, consumer) = rtrb::RingBuffer::new(4);
        let messages = Rc::new(RefCell::new((producer, consumer)));
        let queued = messages.clone();
        let succeeded = Rc::new(Cell::new(None));
        let outcome = succeeded.clone();
        let hook = move || {
            outcome.set(Some(
                crate::audio_engine::push_global_rate_message(
                    &ownership.global_timing_revision,
                    &ownership.global_timing_publishing,
                    &ownership.global_rate_status,
                    &mut queued.borrow_mut().0,
                    Some(1.25),
                    None,
                    "test original publisher",
                )
                .is_ok(),
            ));
        };
        if after_lease {
            crate::audio_engine::mixer::RtMixer::with_runtime_policy_after_global_lease_for_test(
                hook,
                || {
                    fixture.path.render(0);
                },
            );
        } else {
            crate::audio_engine::mixer::RtMixer::with_runtime_policy_before_claim_for_test(
                hook,
                || {
                    fixture.path.render(0);
                },
            );
        }
        assert_eq!(succeeded.get(), Some(!after_lease));
        assert_eq!(messages.borrow_mut().1.pop().is_ok(), !after_lease);
        if after_lease {
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
        } else {
            unchanged(&fixture.path, &before);
            assert_eq!(
                snapshot.observation().unwrap().reason,
                Some(RuntimePolicyError::Stale)
            );
        }
    }
}

#[test]
fn same_map_with_changed_actual_transport_anchor_recertifies_instead_of_reusing_old_projection() {
    let mut fixture = DryMappedPath::new(2, 23);
    fixture.adopt(true, 2);
    fixture.path.render(37);
    let before = fixture.path.voice(0).source_playback;
    let old_epoch = before.mapped_epoch().unwrap();
    let frame = fixture.path.timeline.output_frame();
    // Exercise the original authoritative transport mutator used by qualified
    // anchor commits. No copied voice/epoch is changed to prepare this condition.
    assert!(
        fixture
            .path
            .timeline
            .set_master_period_and_anchor_beat_position_at_frame(MASTER, 0.25, frame)
    );
    assert!(!old_epoch.matches_timeline(&fixture.path.timeline));
    fixture.publish(true, 3);
    let snapshot = request(&fixture.path);
    let wide = wide_oracle(&fixture.path, &snapshot, 0);
    let before_plan = latest_plan(&fixture.path, 0);
    let mut expected = [vec![0.0; 1], vec![0.0; 1]];
    before_plan
        .fill_fractional_buffers_checked_with_readers(
            fixture.path.voice(0).sample.as_ref().unwrap(),
            StemReadSets::default(),
            &before,
            &mut expected,
            1,
        )
        .unwrap();
    service(&mut fixture.path);
    assert_eq!(snapshot.observation().unwrap().state, "applied");
    let current = fixture.path.voice(0).source_playback;
    assert_position(current.position(), before.position());
    assert!(
        current
            .mapped_epoch()
            .unwrap()
            .matches_timeline(&fixture.path.timeline)
    );
    assert!(!current.matches_exact(&before));
    let mut actual = [vec![0.0; 1], vec![0.0; 1]];
    before_plan
        .fill_fractional_buffers_checked_with_readers(
            fixture.path.voice(0).sample.as_ref().unwrap(),
            StemReadSets::default(),
            &current,
            &mut actual,
            1,
        )
        .unwrap();
    assert_eq!(actual, expected);
    fixture.path.render(513);
    assert_mapped_reads(&fixture.path, wide, frame, 0);
}

fn automatic_mapped(path: &Path) -> Arc<RuntimePolicySnapshot> {
    path.source
        .engine
        .request_input_runtime_policy_adoption(true)
        .unwrap();
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .runtime_policy_snapshot()
        .unwrap()
        .unwrap()
}

#[test]
fn actual_after_loop_refusal_recovers_only_from_scalar_cursor_progress_without_policy_edit() {
    let mut fixture = DryMappedPath::new(2, 23);
    fixture
        .path
        .source
        .engine
        .seek_sample(0, 2000.0 / f64::from(RATE))
        .unwrap();
    fixture.path.drain();
    fixture.path.render(1);
    assert_eq!(
        fixture.path.voice(0).source_playback.position().seek_mode,
        ExplicitSeekMode::AfterLoop
    );
    fixture.publish(true, 2);
    let snapshot = automatic_mapped(&fixture.path);
    fixture.assert_domain(&snapshot);
    let before = scalar_state(&fixture.path);
    service(&mut fixture.path);
    unchanged(&fixture.path, &before);
    let failed = snapshot.observation().unwrap();
    assert_eq!(failed.reason, Some(RuntimePolicyError::Unsupported));
    assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.path)));
    assert_eq!(snapshot.observation().unwrap().attempt, failed.attempt + 1);
    service(&mut fixture.path);
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    // The real tail read crosses physical EOF4096. Its original scalar clock
    // enters Normal without a source, map, policy or domain publication.
    fixture.path.render(1600);
    assert_eq!(
        fixture.path.voice(0).source_playback.position().seek_mode,
        ExplicitSeekMode::Normal
    );
    assert!(fixture.path.voice(0).source_playback.position().frame < DYADIC_END);
    assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.path)));
    service(&mut fixture.path);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert_eq!(snapshot.observation().unwrap().execution_frame, Some(1624));
    assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
}

#[test]
fn whole_map_rate_refusal_is_quiet_for_1500_polls_then_actual_master_or_map_edit_recovers() {
    use crate::audio_engine::mapped_source_clock::MappedClockError;
    for replace_map in [false, true] {
        let mut fixture = DryMappedPath::new(2, 23);
        let mut reference = DryMappedPath::new(2, 23);
        let (job, delivery) = fixture.map(0, 0.25, 1.0 / 128.0);
        fixture.jobs.push(job);
        fixture.deliveries[0] = delivery;
        fixture.publish(true, 2);
        let snapshot = automatic_mapped(&fixture.path);
        fixture.assert_domain(&snapshot);
        let original_projection = fixture.path.timeline.beat_projection().unwrap();
        // Source375 / quarter.25 gives a1500-frame/quarter segment. At the
        // actual512-frame master its rate exceeds2 regardless of the cursor.
        assert!(1500.0 / 512.0 > crate::audio_engine::constants::SPEED_MAX);
        let source = snapshot.prepared_map(0).unwrap();
        let failed = SourcePlayback::mapped_anchored_compact_continuation(
            source.geometry(),
            snapshot.revision(),
            fixture.path.timeline,
            latest_plan(&fixture.path, 0).domain(),
            fixture.path.voice(0).source_playback.position(),
            fixture.path.timeline.output_frame(),
        );
        assert_eq!(failed.unwrap_err(), MappedClockError::UnsupportedRate);
        service(&mut fixture.path);
        let refused = snapshot.observation().unwrap();
        assert_eq!(refused.reason, Some(RuntimePolicyError::Unsupported));
        for _ in 0..1500 {
            assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.path)));
            assert_eq!(snapshot.observation(), Some(refused));
            assert!(fixture.path.source.consumer.is_empty());
            assert!(original_projection.matches_timeline(&fixture.path.timeline));
            assert_eq!(fixture.path.render(3), reference.path.render(3));
        }
        let recovered = if replace_map {
            let (job, delivery) = fixture.map(0, 0.75, 1.0 / 128.0);
            fixture.jobs.push(job);
            fixture.deliveries[0] = delivery;
            fixture.publish(true, 3);
            let current = automatic_mapped(&fixture.path);
            assert!(!Arc::ptr_eq(&snapshot, &current));
            assert!(original_projection.matches_timeline(&fixture.path.timeline));
            current
        } else {
            let (producer, consumer) = rtrb::RingBuffer::new(16);
            fixture.parameters = consumer;
            fixture.path.source.engine.test_parameter_producer =
                Some(Arc::new(std::sync::Mutex::new(producer)));
            fixture
                .path
                .source
                .engine
                .set_master_period_with_binding(1024.0 / f64::from(RATE), None)
                .unwrap();
            fixture.path.drain();
            drain_parameter_messages(
                &mut fixture.parameters,
                &mut fixture.path.source.mixer,
                &mut fixture.path.timeline,
            );
            fixture.path.render(0);
            let actual = fixture
                .path
                .source
                .engine
                .input_runtime_ownership
                .global_rate_status
                .read()
                .unwrap();
            assert_eq!(
                actual.effect.state,
                crate::audio_engine::global_rate_status::RateState::Applied
            );
            assert_eq!(
                actual.effect.effective.period,
                Some(1024.0 / f64::from(RATE))
            );
            assert!(!original_projection.matches_timeline(&fixture.path.timeline));
            let current = automatic_mapped(&fixture.path);
            assert!(Arc::ptr_eq(&snapshot, &current));
            let projection_1024 = fixture.path.timeline.beat_projection().unwrap();
            assert_eq!(projection_1024.frames_per_beat(), 1024.0);
            for id in 0..2 {
                let voice = fixture.path.voice(id);
                let source = current.prepared_map(id).unwrap();
                let plan = latest_plan(&fixture.path, id);
                let result = SourcePlayback::mapped_anchored_compact_continuation(
                    source.geometry(),
                    current.revision(),
                    fixture.path.timeline,
                    plan.domain(),
                    voice.source_playback.position(),
                    fixture.path.timeline.output_frame(),
                );
                if id == 0 {
                    assert!(
                        result.is_ok(),
                        "lane={id} actual={projection_1024:?} preparation={:?} plan={plan:?} raw={:?} result={result:?}",
                        current.map_preparation(id),
                        voice.source_playback.position()
                    );
                } else {
                    // The unchanged neighbor's 500-frame/quarter segment is
                    // below 0.5 at FPB1024. Its refusal protects the whole cohort.
                    assert_eq!(result.unwrap_err(), MappedClockError::UnsupportedRate);
                }
            }
            let before_1024 = scalar_state(&fixture.path);
            service(&mut fixture.path);
            unchanged(&fixture.path, &before_1024);
            assert_eq!(
                current.observation().unwrap().reason,
                Some(RuntimePolicyError::Unsupported)
            );
            fixture
                .path
                .source
                .engine
                .set_master_period_with_binding(1000.0 / f64::from(RATE), None)
                .unwrap();
            fixture.path.drain();
            drain_parameter_messages(
                &mut fixture.parameters,
                &mut fixture.path.source.mixer,
                &mut fixture.path.timeline,
            );
            fixture.path.render(0);
            let projection_1000 = fixture.path.timeline.beat_projection().unwrap();
            assert_eq!(projection_1000.frames_per_beat(), 1000.0);
            assert!(!projection_1024.matches_timeline(&fixture.path.timeline));
            let actual = fixture
                .path
                .source
                .engine
                .input_runtime_ownership
                .global_rate_status
                .read()
                .unwrap();
            assert_eq!(
                actual.effect.state,
                crate::audio_engine::global_rate_status::RateState::Applied
            );
            assert_eq!(
                actual.effect.effective.period,
                Some(1000.0 / f64::from(RATE))
            );
            for id in 0..2 {
                let voice = fixture.path.voice(id);
                let source = current.prepared_map(id).unwrap();
                let plan = latest_plan(&fixture.path, id);
                let result = SourcePlayback::mapped_anchored_compact_continuation(
                    source.geometry(),
                    current.revision(),
                    fixture.path.timeline,
                    plan.domain(),
                    voice.source_playback.position(),
                    fixture.path.timeline.output_frame(),
                );
                assert!(
                    result.is_ok(),
                    "lane={id} actual={projection_1000:?} preparation={:?} plan={plan:?} raw={:?} result={result:?}",
                    current.map_preparation(id),
                    voice.source_playback.position()
                );
            }
            let recovered = automatic_mapped(&fixture.path);
            assert!(Arc::ptr_eq(&snapshot, &recovered));
            recovered
        };
        let before = fixture.path.voice(0).source_playback.position();
        service(&mut fixture.path);
        assert_eq!(
            recovered.observation().unwrap().state,
            "applied",
            "{:?}",
            recovered.observation()
        );
        assert_position(fixture.path.voice(0).source_playback.position(), before);
        assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
        fixture.assert_domain(&recovered);
    }
}

#[test]
fn original_decimal_none_map_now_adopts_at_actual_e_without_changing_first_pcm() {
    let mut fixture = DryMappedPath::new(2, 23);
    let mut unchanged_reference = DryMappedPath::new(2, 23);
    // This exact former unsupported input is now deliberately supported by the
    // separate physical-Q class; its previous guard is not weakened or relabelled.
    let (job, delivery) = fixture.map(0, 0.125, 0.001);
    fixture.jobs.push(job);
    fixture.deliveries[0] = delivery;
    fixture.publish(true, 2);
    let snapshot = request(&fixture.path);
    assert_eq!(snapshot.map_preparation(0).class, RuntimeMapClass::Ready);
    assert!(snapshot.map_current(0));
    let execution = fixture.path.timeline.output_frame();
    let before = fixture.path.voice(0).source_playback;
    let raw = before.position();
    let plan = SourceReadPlan {
        frame_pos: raw.frame,
        ..latest_plan(&fixture.path, 0)
    };
    let taps = plan.try_fractional_taps(raw.fraction).unwrap();
    service(&mut fixture.path);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert_eq!(
        snapshot.observation().unwrap().execution_frame,
        Some(execution)
    );
    let playback = fixture.path.voice(0).source_playback;
    assert!(playback.is_general_mapped());
    assert!(playback.reader_period().physical_quarters().is_some());
    assert_position(playback.position(), raw);
    assert_eq!(playback.original_scalar_domain(), before.effective_domain());
    assert_eq!(
        plan.with_effective_domain(playback.effective_domain().unwrap())
            .unwrap()
            .try_fractional_taps(raw.fraction)
            .unwrap(),
        taps
    );
    assert_eq!(fixture.path.render(1), unchanged_reference.path.render(1));
}

#[test]
fn excluded_exact_grid_is_stably_quiet_without_disabling_legacy_map_availability() {
    let mut fixture = DryMappedPath::new(2, 23);
    // A finite original beat below the documented -126 grid bound excludes
    // only exact live preparation; ordinary rounded launch remains independent.
    let (job, delivery) = fixture.map_anchors(
        0,
        &[
            (0.0, -2.0_f64.powi(-200)),
            (0.001, 0.125),
            (1.0 / 32.0, 2.0),
        ],
    );
    fixture.jobs.push(job);
    fixture.deliveries[0] = delivery;
    fixture.publish(true, 2);
    let snapshot = automatic_mapped(&fixture.path);
    assert_eq!(
        snapshot.map_preparation(0).class,
        RuntimeMapClass::Unsupported
    );
    assert!(snapshot.map_current(0));
    let before = scalar_state(&fixture.path);
    service(&mut fixture.path);
    let failed = snapshot.observation().unwrap();
    assert_eq!(failed.reason, Some(RuntimePolicyError::Unsupported));
    for _ in 0..64 {
        assert!(Arc::ptr_eq(&snapshot, &automatic_mapped(&fixture.path)));
        assert_eq!(snapshot.observation(), Some(failed));
        assert!(fixture.path.source.consumer.is_empty());
    }
    unchanged(&fixture.path, &before);
    fixture.path.source.engine.stop_sample(0).unwrap();
    fixture.path.drain();
    let start = fixture.start(0);
    assert_eq!(start.sync_request().mode, SourceSyncMode::Mapped);
    assert!(fixture.path.voice(0).source_playback.is_mapped());
    assert!(!fixture.path.voice(0).source_playback.is_compact_mapped());
}

#[test]
fn same_path_real_reload_publication_before_claim_refuses_original_cohort_before_its_new_ack() {
    let mut fixture = DryMappedPath::new(2, 23);
    fixture.publish(true, 2);
    let snapshot = request(&fixture.path);
    let old_key = snapshot
        .row(0)
        .unwrap()
        .binding
        .placement
        .unwrap()
        .content_key();
    fixture.path.drain();
    settle_parameters(&mut fixture.path);
    let before = scalar_state(&fixture.path);
    assert_eq!(before.len(), 2);
    let samples = (0..2)
        .map(|id| fixture.path.voice(id).sample.as_ref().unwrap().clone())
        .collect::<Vec<_>>();
    let mut original_pcm = Vec::new();
    for (id, sample) in samples.iter().enumerate() {
        let mut pcm = [vec![0.0; 1], vec![0.0; 1]];
        latest_plan(&fixture.path, id)
            .fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &fixture.path.voice(id).source_playback,
                &mut pcm,
                1,
            )
            .unwrap();
        original_pcm.push(pcm);
    }
    // Move the original control owner into the same deterministic claim hook
    // used by the existing resident-control regression. The spare engine grants
    // no source authority and never participates in this callback.
    let spare = crate::audio_engine::AudioEngine::new().unwrap();
    let original = Rc::new(std::mem::replace(&mut fixture.path.source.engine, spare));
    let control = original.clone();
    let producer = fixture.path.source.producer.clone();
    let directory = fixture.path.source.directory.path().to_path_buf();
    let old_snapshot = snapshot.clone();
    let request_id = Rc::new(Cell::new(None));
    let captured_request = request_id.clone();
    crate::audio_engine::mixer::RtMixer::with_runtime_policy_before_claim_for_test(
        move || {
            let request = crate::audio_engine::cold_load::admit_for_format_selected(
                &control,
                0,
                directory.join("external.wav").to_string_lossy().into(),
                (false, false, true, None),
                producer,
                (2, RATE, directory.join("samples")),
            )
            .unwrap();
            wait_for(|| {
                control.input_runtime_ownership.cold_status(0, request) == Some(0)
                    && !old_snapshot.row_current(0)
            });
            captured_request.set(Some(request));
            // The genuine publication has armed its source fences; its own
            // LoadColdSample remains unread until the original claim finishes.
        },
        || {
            fixture.path.render(0);
        },
    );
    fixture.path.source.engine = Rc::try_unwrap(original).ok().unwrap();
    let request_id = request_id.get().unwrap();
    unchanged(&fixture.path, &before);
    assert!(matches!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::SourceChanged | RuntimePolicyError::Stale)
    ));
    assert_eq!(
        fixture
            .path
            .source
            .engine
            .input_runtime_ownership
            .cold_status(0, request_id),
        Some(0)
    );
    for (id, sample) in samples.iter().enumerate() {
        let voice = fixture.path.voice(id);
        assert!(voice.active && voice.sample.as_ref().unwrap().same_window(sample));
        assert!(voice.mapped_start.is_none());
        let mut pcm = [vec![0.0; 1], vec![0.0; 1]];
        latest_plan(&fixture.path, id)
            .fill_fractional_buffers_checked_with_readers(
                sample,
                StemReadSets::default(),
                &voice.source_playback,
                &mut pcm,
                1,
            )
            .unwrap();
        assert_eq!(pcm, original_pcm[id]);
    }
    assert!(!snapshot.map_current(0));
    // Complete the separately admitted real source transaction only after proving
    // that the old original policy wrote no participant before this new ACK.
    fixture.path.drain();
    assert_eq!(
        fixture
            .path
            .source
            .engine
            .input_runtime_ownership
            .cold_status(0, request_id),
        Some(2)
    );
    wait_for(|| {
        fixture.path.source.engine.cold_loading[0].load(std::sync::atomic::Ordering::Acquire) == 0
    });
    let current = fixture
        .path
        .source
        .engine
        .current_input_runtime_pad_binding(0)
        .unwrap()
        .unwrap();
    assert_ne!(current.binding.placement.unwrap().content_key(), old_key);
    assert!(matches!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::SourceChanged | RuntimePolicyError::Stale)
    ));
    assert!(!snapshot.map_current(0));
}

#[test]
fn converted_dry_voice_keeps_pause_hold_resume_global_rate_retrigger_and_real_read_history() {
    let mut fixture = DryMappedPath::new(2, 23);
    fixture.adopt(true, 2);
    fixture.path.render(17);
    let source = fixture
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    let position = fixture.path.voice(0).source_playback.position();
    let pause = fixture.path.source.engine.pause_sample(0).unwrap();
    fixture.path.drain();
    assert!(
        pause
            .current_observation()
            .unwrap()
            .unwrap()
            .ordinary_paused()
    );
    fixture.path.render(31);
    assert_position(fixture.path.voice(0).source_playback.position(), position);
    fixture.path.source.engine.resume_sample(0).unwrap();
    fixture.path.drain();
    assert!(!fixture.path.voice(0).paused);
    assert_position(fixture.path.voice(0).source_playback.position(), position);
    assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
    fixture.path.render(19);
    let position = fixture.path.voice(0).source_playback.position();
    let ticket = Python::attach(|py| {
        fixture
            .path
            .source
            .engine
            .pause_waveform_hold(0, 854_u64.into_pyobject(py).unwrap().as_any(), None)
            .unwrap()
    });
    fixture.path.drain();
    assert!(ticket.is_current() && fixture.path.voice(0).paused);
    fixture.path.render(37);
    assert_position(fixture.path.voice(0).source_playback.position(), position);
    assert!(
        fixture
            .path
            .source
            .engine
            .release_waveform_hold(&ticket)
            .unwrap()
    );
    fixture.path.drain();
    assert_eq!(ticket.publication_status(), "resumed");
    assert_position(fixture.path.voice(0).source_playback.position(), position);
    assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
    let (producer, consumer) = rtrb::RingBuffer::new(16);
    fixture.parameters = consumer;
    fixture.path.source.engine.test_parameter_producer =
        Some(Arc::new(std::sync::Mutex::new(producer)));
    fixture
        .path
        .source
        .engine
        .set_speed_and_master_period(1.25, MASTER * 1.25)
        .unwrap();
    fixture.path.drain();
    drain_parameter_messages(
        &mut fixture.parameters,
        &mut fixture.path.source.mixer,
        &mut fixture.path.timeline,
    );
    fixture.path.render(0);
    assert_position(fixture.path.voice(0).source_playback.position(), position);
    assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .mapped_epoch()
            .unwrap()
            .matches_timeline(&fixture.path.timeline)
    );
    assert!(Arc::ptr_eq(
        fixture
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap(),
        &source
    ));
    fixture.path.render(513);
    let mut history = SourceFeedSnapshot::new();
    assert!(
        fixture
            .path
            .voice(0)
            .source_feed_history
            .capture_into(&mut history)
    );
    assert_eq!(history.retained_range(), Some(0..(23 + 17 + 19 + 513)));
    let mut mapped_frames = 0;
    for span in history.spans() {
        if span.playback.is_mapped() {
            assert!(span.playback.is_compact_mapped());
            assert!(span.slice(0, span.frames).is_some());
            mapped_frames += span.frames;
        }
    }
    assert_eq!(mapped_frames, 17 + 19 + 513);
    let next_origin = history.active_frames();
    let generation = fixture.path.voice(0).generation;
    let birth = fixture.start(0);
    fixture.path.render(1);
    assert!(fixture.path.voice(0).generation > generation);
    assert!(
        fixture
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .original_launch()
            .unwrap()
            .same_owner(&birth)
    );
    let mut restarted_history = SourceFeedSnapshot::new();
    assert!(
        fixture
            .path
            .voice(0)
            .source_feed_history
            .capture_into(&mut restarted_history)
    );
    assert!(!history.is_current());
    assert_eq!(restarted_history.active_origin(), next_origin);
    assert_eq!(restarted_history.active_frames(), next_origin + 1);
    assert_eq!(
        restarted_history.retained_range(),
        Some(next_origin..next_origin + 1)
    );
    assert_eq!(
        restarted_history
            .spans()
            .map(|span| span.frames)
            .sum::<usize>(),
        1
    );
}

#[test]
fn requested_bpm_parent_off_first_adopts_real_legacy_then_completes_original_native_off_receipt() {
    use crate::audio_engine::global_rate_status::BpmReason;
    let mut fixture = DryMappedPath::new(2, 23);
    fixture.adopt(true, 2);
    fixture.path.render(17);
    let before = fixture.path.voice(0).source_playback.position();
    let source = fixture
        .path
        .voice(0)
        .mapped_start
        .as_ref()
        .unwrap()
        .effective_source()
        .unwrap()
        .clone();
    // Publish the requested parent state before admitting OFF, so the genuine
    // receipt captures the current hierarchy stamp rather than a superseded one.
    fixture.publish_with_parent(true, 3, None, false);
    fixture
        .path
        .source
        .engine
        .set_bpm_lock_with_binding(false, None)
        .unwrap();
    fixture.path.drain();
    drain_parameter_messages(
        &mut fixture.parameters,
        &mut fixture.path.source.mixer,
        &mut fixture.path.timeline,
    );
    fixture.path.render(0);
    let requested = fixture
        .path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap();
    assert!(!requested.1.requested && requested.1.effective && !requested.1.applied);
    assert_eq!(requested.1.reason, BpmReason::MappedActive);
    assert!(fixture.path.voice(0).source_playback.is_compact_mapped());
    assert_eq!(fixture.path.timeline.master_period_seconds(), Some(MASTER));
    // Persisted source SYNC and REALTIME wishes survive the requested parent OFF.
    // Their original resolver now supplies Legacy while the actual BPM flag is
    // still ON; conversion must not wait for the receipt it makes possible.
    let snapshot = request(&fixture.path);
    assert_eq!(
        snapshot.row(0).unwrap().request.mode,
        SourceSyncMode::Legacy
    );
    service(&mut fixture.path);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert!(!fixture.path.voice(0).source_playback.is_mapped());
    assert_eq!(
        fixture.path.voice(0).sync_request.mode,
        SourceSyncMode::Legacy
    );
    assert_position(fixture.path.voice(0).source_playback.position(), before);
    fixture.path.render(0);
    let completed = fixture
        .path
        .source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read_bpm()
        .unwrap();
    assert_eq!(completed.1.revision, requested.1.revision);
    assert!(completed.1.applied && !completed.1.effective && !completed.1.requested);
    assert_eq!(completed.1.reason, BpmReason::None);
    assert_eq!(
        fixture
            .path
            .source
            .engine
            .input_runtime_ownership
            .global_rate_status
            .read()
            .unwrap()
            .effect
            .effective
            .period,
        None
    );
    assert!(Arc::ptr_eq(
        fixture
            .path
            .voice(0)
            .mapped_start
            .as_ref()
            .unwrap()
            .effective_source()
            .unwrap(),
        &source
    ));
    let pcm = fixture.path.render(513);
    assert!(pcm.iter().all(|value| value.is_finite()));
    assert!(pcm.iter().any(|value| *value != 0.0));
}

#[test]
fn genuine_tiny_virtual_seam_keeps_original_some_2_25_domain_and_actual_renderer() {
    let mut fixture = DryMappedPath::new_with_shape(2, 23, true);
    fixture.publish(true, 2);
    let snapshot = request(&fixture.path);
    fixture.assert_domain(&snapshot);
    let before = fixture.path.voice(0).source_playback.position();
    let wide = wide_oracle(&fixture.path, &snapshot, 0);
    service(&mut fixture.path);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    assert_position(fixture.path.voice(0).source_playback.position(), before);
    let mut output = [0.0; BLOCK * 2];
    let (_, queries) = observe_compact_queries_for_test(|| {
        fixture.render_into(&mut output, &mut ImmediateAudioBufferRetirement);
    });
    assert_eq!(queries.1, 0);
    assert!(output.iter().all(|value| value.is_finite()));
    assert!(output.iter().any(|value| *value != 0.0));
    assert_mapped_reads(&fixture.path, wide, 23, 0);
}

#[test]
fn productive_actual_e_and_full_dry_renderer_fit_explicit_two_mib_without_rust_requests() {
    let child = std::thread::Builder::new()
        .name("productive-mapped-two-mib".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let mut fixture = DryMappedPath::new(6, 23);
            fixture.publish(true, 2);
            let snapshot = request(&fixture.path);
            fixture.assert_domain(&snapshot);
            let mut reference = (0..6)
                .map(|id| wide_oracle(&fixture.path, &snapshot, id))
                .collect::<Vec<_>>();
            let mut output = [0.0; BLOCK * 2];
            let mut retired = HeldRetirement::new(64);
            let (_, allocations) = observe_current_thread(|| {
                let (_, queries) = observe_compact_queries_for_test(|| {
                    drain_and_render(&mut fixture, &mut output, &mut retired);
                });
                assert_eq!(queries.1, 0);
                assert!(queries.0 >= 6);
            });
            assert_eq!(snapshot.observation().unwrap().state, "applied");
            for (id, expected) in reference.iter_mut().enumerate() {
                let source = snapshot.prepared_map(id).unwrap();
                expected
                    .project_mapped_to(source.geometry(), 23 + BLOCK as u64)
                    .unwrap();
                assert_position(
                    fixture.path.voice(id).source_playback.position(),
                    expected.position(),
                );
            }
            assert!(output.iter().any(|value| *value != 0.0));
            assert_eq!(retired.work.len(), 2);
            assert!(
                retired
                    .work
                    .iter()
                    .all(|work| std::ptr::eq(&**work, snapshot.as_ref()))
            );
            allocations
        })
        .unwrap();
    assert_eq!(child.join().unwrap(), ThreadAllocations::default());
}

#[test]
#[ignore = "actual productive consuming Release profile; run explicitly with --release --ignored --nocapture"]
fn descriptive_productive_actual_e_full_dry_renderer_cohorts_report_median_max_and_limits() {
    use crate::audio_engine::source_reader::{
        reset_tap_observation_for_test, tap_observation_for_test,
    };
    const REPEATS: usize = 3;
    let nominal = BLOCK as f64 / f64::from(RATE);
    for tiny in [false, true] {
        for lanes in [2, 4, 6, 10] {
            let mut fixture = DryMappedPath::new_with_shape(lanes, 23, tiny);
            let mut seconds = [0.0_f64; REPEATS];
            let mut output = [0.0; BLOCK * 2];
            let mut total_taps = 0;
            let mut compact_preparations = 0;
            for repeat in 0..=REPEATS {
                if repeat != 0 {
                    fixture.adopt(false, (repeat * 2 + 1) as u64);
                }
                fixture.publish(true, (repeat * 2 + 2) as u64);
                let snapshot = request(&fixture.path);
                fixture.assert_domain(&snapshot);
                let first = fixture.path.timeline.output_frame();
                let mut expected = (0..lanes)
                    .map(|id| wide_oracle(&fixture.path, &snapshot, id))
                    .collect::<Vec<_>>();
                let mut retired = HeldRetirement::new(64);
                reset_tap_observation_for_test();
                let start = Instant::now();
                let ((_, queries), allocations) = observe_current_thread(|| {
                    observe_compact_queries_for_test(|| {
                        drain_and_render(&mut fixture, &mut output, &mut retired)
                    })
                });
                let elapsed = start.elapsed().as_secs_f64();
                let taps = tap_observation_for_test();
                assert_eq!(allocations, ThreadAllocations::default());
                assert_eq!(queries.1, 0);
                assert!(queries.0 >= lanes);
                assert!(taps.left_reads >= lanes * BLOCK * 2);
                assert_eq!(taps.missing_reads, 0);
                assert_eq!(
                    snapshot.observation().unwrap().state,
                    "applied",
                    "{:?}",
                    snapshot.observation()
                );
                assert_eq!(snapshot.observation().unwrap().execution_frame, Some(first));
                assert!(output.iter().all(|value| value.is_finite()));
                assert!(output.iter().any(|value| *value != 0.0));
                for (id, wide) in expected.iter_mut().enumerate() {
                    wide.project_mapped_to(
                        snapshot.prepared_map(id).unwrap().geometry(),
                        first + BLOCK as u64,
                    )
                    .unwrap();
                    let actual = fixture.path.voice(id).source_playback;
                    assert!(actual.is_compact_mapped());
                    assert_position(actual.position(), wide.position());
                }
                if repeat != 0 {
                    seconds[repeat - 1] = elapsed;
                    total_taps += taps.left_reads + taps.right_reads;
                    compact_preparations += queries.0;
                }
            }
            seconds.sort_by(f64::total_cmp);
            println!(
                "productive_dry_policy_profile: shape={} cohort={} active={} fixed_capacity=32 frames_per_lane={} warmups=1 repeats={} median_ms={:.4} max_ms={:.4} nominal_512_at_48k_ms={:.7} median_interval_ratio={:.3} max_interval_ratio={:.3} within_nominal={} actual_tap_reads={} compact_preparations={} wide_entries=0; includes original queue drain, due-frame preflight/claim, actual-E construction, full dry renderer/DSP, history, copied advance and span refresh; fixture/maps/wide oracle/output/sink preallocated outside clock; Rust allocation/zeroed/reallocation attempts=0, deallocation/Native unobserved; bounded generated-PCM workload, no device/music/worstcase/RT acceptance",
                if tiny {
                    "physical2_virtual2.25"
                } else {
                    "dyadic1500_variable"
                },
                lanes,
                lanes,
                BLOCK,
                REPEATS,
                seconds[1] * 1000.0,
                seconds[2] * 1000.0,
                nominal * 1000.0,
                seconds[1] / nominal,
                seconds[2] / nominal,
                seconds[2] <= nominal,
                total_taps,
                compact_preparations,
            );
        }
    }
}
