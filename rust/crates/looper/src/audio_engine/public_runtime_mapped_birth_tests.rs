// Public original-owned beginnings. PCM/read/feedback proof is not musical acceptance.
use super::*;
use crate::audio_engine::input_mapping::PitchPadProjection;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::source_playback::SourcePlayback;
use crate::audio_engine::source_sync_policy::SourceSyncMode;

mod retained_mapped_rate_tests {
    include!("retained_mapped_rate_tests.rs");
}

fn complete_launch_path() -> LaunchPath {
    let mut source = ColdCustody::new_selected_with_pcm(None, [8192; 128]);
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            source.engine.input_clock,
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let sample = source.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert_eq!(sample.resident_start(), 0);
    assert_eq!(sample.resident_end(), sample.frame_count());
    assert!(crate::audio_engine::mixer::RtMixer::initial_key_lock_geometry_available(&sample));
    assert!(binding(&source, 0).current());
    LaunchPath::from_source(source)
}

fn set_start_policy_after_idle_callback(path: &mut LaunchPath, attack: bool) {
    // Real load/publication revokes idle. Complete the actual callback proof before
    // requesting this stopped-only preference; do not set its ownership flags.
    path.drain();
    assert!(path.render(1).iter().all(|sample| *sample == 0.0));
    assert!(path.source.engine.can_change_key_lock_start_behavior());
    assert!(
        path.source
            .engine
            .set_key_lock_start_behavior(if attack {
                "attack_at_grid"
            } else {
                "first_sound_at_grid"
            })
            .unwrap()
    );
}

fn covered_original_delivery(
    path: &LaunchPath,
    variable: bool,
) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    // Exact physical coverage ends at93.75, beyond selected marker64. The old
    // f64(64/48000) endpoint is slightly below64 under the exact source algebra.
    let points = if variable {
        vec![(0.0, 0.0), (1.0 / 1024.0, 0.6), (1.0 / 512.0, 1.6)]
    } else {
        vec![(0.0, 0.0), (1.0 / 512.0, 1.46484375)]
    };
    delivery_for_points(path, &points)
}

fn delivery_for_points(
    path: &LaunchPath,
    points: &[(f64, f64)],
) -> (OfflineAnalysisJob, LoaderEventDelivery) {
    let mut expected = record(&path.source, 0);
    expected["record"]["accepted"]["anchors"] = json!(
        points
            .iter()
            .enumerate()
            .map(|(index, (seconds, beat))| json!({
                "id": format!("covered-{index}"), "source_seconds_bits": bits(*seconds),
                "beat_bits": bits(*beat), "raw_beat_index": null,
                "review": "reviewed", "sample_edit": null
            }))
            .collect::<Vec<_>>()
    );
    let interval = expected["record"]["accepted"]["intervals"][0].clone();
    expected["record"]["accepted"]["intervals"] = json!(vec![interval; points.len() - 1]);
    seal(&mut expected);
    let (job, event) = stored_event(&path.source, &expected);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    (job, delivery)
}

pub(super) fn ready(path: &mut LaunchPath, sequence: u64) {
    let frame = path.timeline.output_frame();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let runtime = path.source.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.has_ready_action(sequence) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "public mapped birth preparation"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(path.timeline.output_frame(), frame);
}

pub(super) fn render_to(path: &mut LaunchPath, frame: u64) {
    assert!(frame >= path.timeline.output_frame());
    while path.timeline.output_frame() < frame {
        let frames = (frame - path.timeline.output_frame()).min(512) as usize;
        path.render(frames);
    }
}

fn pitch_rows(path: &LaunchPath, key_lock: bool) {
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    rows[0].key_lock = key_lock;
    rows[0].sync = true;
    rows[0].source_root = Some(0);
    rows[0].source_mode = Some(crate::audio_engine::pitch_actions::PitchKeyMode::Major);
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_pitch_runtime_state(rows)
        .unwrap();
}

fn key_lock_feedback(path: &LaunchPath) -> serde_json::Value {
    Python::attach(|py| {
        let status = path
            .source
            .engine
            .pad_key_lock_status(py, 0)
            .unwrap()
            .unwrap();
        let encoded: String = py
            .import("json")
            .unwrap()
            .call_method1("dumps", (status,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&encoded).unwrap()
    })
}

fn assert_physical_scalar_provenance(playback: &SourcePlayback) {
    assert_eq!(
        playback.original_scalar_domain().unwrap().musical_period,
        crate::audio_engine::source_reader::SourceReaderPeriod::Physical
    );
}

fn assert_live_compact_preparation(
    path: &LaunchPath,
    delivery: &LoaderEventDelivery,
    quarters: Option<f64>,
    start: usize,
    end: usize,
) {
    let current = binding(&path.source, 0);
    let qualified = delivery.qualify_source_map(&current, quarters).unwrap();
    let (preparation, source) =
        crate::audio_engine::pad_start::MappedStartSource::prepare_runtime_map(
            &qualified,
            current.binding,
            start as f64 / f64::from(RATE),
            Some(end as f64 / f64::from(RATE)),
        );
    assert_eq!(
        preparation.class,
        crate::audio_engine::source_map_admission::RuntimeMapClass::Ready
    );
    let source = source.unwrap();
    assert!(source.geometry().has_compact_continuation());
    assert!(!source.geometry().has_general_continuation());
}

fn stopped_key_lock_birth(
    shift: i32,
    attack: bool,
) -> (LaunchPath, OfflineAnalysisJob, CapturedPadStart, u64) {
    let mut path = complete_launch_path();
    let (job, delivery) = covered_original_delivery(&path, true);
    publish_original(&path, &delivery, 1);
    path.source
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(shift, 0).unwrap())
        .unwrap();
    set_start_policy_after_idle_callback(&mut path, attack);
    let request = path.source.engine.set_pad_key_lock(0, true).unwrap();
    assert_ne!(request, 0);
    path.drain();
    let start = path.capture();
    path.publish_capture(&start);
    ready(&mut path, start.sequence());
    (path, job, start, request)
}

fn pitch_clock(path: &LaunchPath) {
    use crate::audio_engine::timing::{OutputClockSnapshot, SharedOutputClock};
    let clock = Arc::new(SharedOutputClock::new());
    let now = path.source.engine.input_clock.capture_ns();
    clock.publish(OutputClockSnapshot {
        valid: true,
        observed_at_ns: now,
        audible_at_ns: now,
        output_frame: path.timeline.output_frame(),
        sample_rate_hz: RATE,
        master_period_seconds: path.timeline.master_period_seconds(),
        downbeat_frame: 0,
        master_beat: path
            .timeline
            .beat_position_at_frame(path.timeline.output_frame())
            .unwrap(),
        freshness_ns: 2_000_000_000,
    });
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .attach_pitch_clock(clock)
        .unwrap();
}

fn decimal_general_path() -> (LaunchPath, OfflineAnalysisJob) {
    let mut source = ColdCustody::new_selected_with_signal(None, &vec![8192; 70_560]);
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let mut path = LaunchPath::from_source(source);
    path.source.mixer.set_master_period(0.5);
    assert!(path.timeline.set_master_period(0.5));
    let mut expected = record(&path.source, 0);
    expected["record"]["accepted"]["anchors"] = json!(
        [0.0, 0.37, 0.91, 1.47]
            .into_iter()
            .enumerate()
            .map(|(index, seconds)| json!({"id":format!("decimal-{index}"),
            "source_seconds_bits":bits(seconds),"beat_bits":bits(index as f64),
            "raw_beat_index":null,"review":"reviewed","sample_edit":null}))
            .collect::<Vec<_>>()
    );
    let interval = expected["record"]["accepted"]["intervals"][0].clone();
    expected["record"]["accepted"]["intervals"] = json!(vec![interval; 3]);
    seal(&mut expected);
    let (job, event) = stored_event(&path.source, &expected);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(binding(&path.source, 0));
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0] = SourceMapRow {
        sync_requested: true,
        intent_revision: 1,
        delivery: Some(delivery),
        exact_quarters: None,
        ordinary_pending: false,
    };
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![1.0 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(43_680.0 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(maps),
        )
        .unwrap();
    (path, job)
}

const QUIET_START: usize = 375;
const QUIET_LOOP_FRAMES: usize = 24_000;
const QUIET_PREFIX: usize = 2048;

fn declared_quiet_pcm(relative: usize) -> i16 {
    let frame = relative % QUIET_LOOP_FRAMES;
    if frame < QUIET_PREFIX {
        0
    } else if frame == QUIET_PREFIX {
        14_000
    } else {
        (((frame * 3251 + 173) % 20_001) as i32 - 10_000) as i16
    }
}

fn quiet_original_path(attack: bool) -> (LaunchPath, OfflineAnalysisJob) {
    quiet_original_path_with_residency(attack, false)
}

fn quiet_original_path_with_residency(
    attack: bool,
    finite: bool,
) -> (LaunchPath, OfflineAnalysisJob) {
    let pcm: Vec<_> = (0usize..48_000)
        .map(|frame| {
            frame
                .checked_sub(QUIET_START)
                .map_or(-12_000, declared_quiet_pcm)
        })
        .collect();
    let hint = finite.then_some(crate::audio_engine::cold_residency::ResidentLoadHint {
        start_s: QUIET_START as f64 / f64::from(RATE),
        end_s: (QUIET_START + QUIET_LOOP_FRAMES) as f64 / f64::from(RATE),
        key_lock: false,
    });
    let mut source = ColdCustody::new_selected_with_signal(hint, &pcm);
    let sample = source.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    assert_eq!(
        crate::audio_engine::native_source_coverage::complete_view_available(&sample),
        !finite
    );
    if finite {
        assert_eq!(
            (sample.resident_start(), sample.resident_end()),
            (QUIET_START, QUIET_START + QUIET_LOOP_FRAMES)
        );
    }
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            source.engine.input_clock,
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let mut path = LaunchPath::from_source(source);
    // Original dyadic seconds and beats give exactly64 source frames/beat.
    // Together with M(n)=n/64, A+n is an independent integer input oracle.
    let mut expected = record(&path.source, 0);
    expected["record"]["accepted"]["anchors"] = json!(
        [(0.0, 0.0), (0.5, 375.0), (1.0, 750.0)]
            .into_iter()
            .enumerate()
            .map(|(index, (seconds, beat))| json!({
                "id": format!("quiet-{index}"), "source_seconds_bits": bits(seconds),
                "beat_bits": bits(beat), "raw_beat_index": null,
                "review": "reviewed", "sample_edit": null
            }))
            .collect::<Vec<_>>()
    );
    let interval = json!({"coverage": "trusted", "meter": {
        "status": "quarter_note_4_4_verified", "downbeat_label": 0,
        "evidence": "synthetic dyadic declared-quarter PCM fixture; no musical acceptance"
    }});
    expected["record"]["accepted"]["intervals"] = json!(vec![interval; 2]);
    seal(&mut expected);
    let (job, event) = stored_event(&path.source, &expected);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    assert_live_compact_preparation(
        &path,
        &delivery,
        Some(375.0),
        QUIET_START,
        QUIET_START + QUIET_LOOP_FRAMES,
    );
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(binding(&path.source, 0));
    let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
    maps[0] = SourceMapRow {
        sync_requested: true,
        intent_revision: 1,
        delivery: Some(delivery),
        exact_quarters: Some(375.0),
        ordinary_pending: false,
    };
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![QUIET_START as f64 / f64::from(RATE); NUM_SAMPLES],
            vec![Some((QUIET_START + QUIET_LOOP_FRAMES) as f64 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(maps),
        )
        .unwrap();
    set_start_policy_after_idle_callback(&mut path, attack);
    (path, job)
}

enum PublicQuietBirth {
    Ordinary(CapturedPadStart),
    Piano(crate::audio_engine::pitch_actions::CapturedPitchAction),
}

impl PublicQuietBirth {
    fn sequence(&self) -> u64 {
        match self {
            Self::Ordinary(start) => start.sequence(),
            Self::Piano(action) => action.sequence(),
        }
    }
    fn original(&self) -> u64 {
        match self {
            Self::Ordinary(start) => start.target_frame().unwrap(),
            Self::Piano(action) => action.original_due_frame(),
        }
    }
    fn execution(&self) -> u64 {
        match self {
            Self::Ordinary(start) => start.execution_frame().unwrap(),
            Self::Piano(action) => action.execution_target(),
        }
    }
    fn state(&self) -> &'static str {
        match self {
            Self::Ordinary(start) => start.state(),
            Self::Piano(action) => action.state(),
        }
    }
    fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Ordinary(start) => start.reason(),
            Self::Piano(action) => action.reason(),
        }
    }
    fn effective(&self) -> bool {
        match self {
            Self::Ordinary(start) => start.effective_frame().is_some(),
            Self::Piano(action) => action.effective(),
        }
    }
    fn key(&self) -> crate::audio_engine::content_placement::ContentKey {
        match self {
            Self::Ordinary(start) => start.content_key(),
            Self::Piano(action) => action.content_key(),
        }
    }
}

fn prepare_quiet_birth(path: &mut LaunchPath, piano: bool, future: bool) -> PublicQuietBirth {
    path.source
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(7, 0).unwrap())
        .unwrap();
    if future {
        assert!(path.render(1).iter().all(|sample| *sample == 0.0));
    }
    let start = if piano {
        assert!(
            !future,
            "this Piano replan fixture deliberately uses an immediate original target"
        );
        pitch_rows(path, false);
        pitch_clock(path);
        let action = path
            .source
            .engine
            .capture_pad_pitch_action(0, "pitch.extra:7:set_retrigger", None)
            .unwrap();
        assert!(action.mapped_geometry().unwrap().has_general_continuation());
        assert!(path.source.engine.publish_pitch_action(&action).unwrap());
        path.drain();
        PublicQuietBirth::Piano(action)
    } else {
        let captured = path
            .source
            .engine
            .capture_pad_start(
                0,
                QUIET_START as f64 / f64::from(RATE),
                Some((QUIET_START + QUIET_LOOP_FRAMES) as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        assert!(
            captured
                .mapped_geometry()
                .unwrap()
                .has_general_continuation()
        );
        if future {
            mapped_fixed_pitch_birth_tests::drain_grid(path);
        }
        path.publish_capture(&captured);
        PublicQuietBirth::Ordinary(captured)
    };
    ready(path, start.sequence());
    start
}

fn silence_until(path: &mut LaunchPath, frame: u64) {
    assert!(path.timeline.output_frame() <= frame);
    while path.timeline.output_frame() < frame {
        let frames = (frame - path.timeline.output_frame()).min(512) as usize;
        assert!(path.render(frames).iter().all(|sample| *sample == 0.0));
    }
}

fn quiet_raw_output(frames: usize, skip: usize) -> Vec<f32> {
    let mut raw = mapped_fixed_pitch_birth_tests::RawStereo::new(2.0_f64.powf(7.0 / 12.0));
    (0..frames)
        .flat_map(|frame| {
            let sample = f32::from(declared_quiet_pcm(frame + skip)) / 32768.0;
            raw.sample([sample; 2])
        })
        .collect()
}

fn assert_quiet_first_read(path: &LaunchPath, execution: u64) {
    let mut history = SourceFeedSnapshot::new();
    assert!(path.voice().source_feed_history.capture_into(&mut history));
    let first = history.spans().next().unwrap();
    assert_eq!(first.active_start, 0);
    assert_eq!(first.playback.position().frame, QUIET_START);
    assert_eq!(
        first.playback.position().fraction.to_bits(),
        0.0_f64.to_bits()
    );
    assert_eq!(first.playback.mapped_output_frame(), Some(execution));
    assert_eq!(first.plan.loop_region.start, QUIET_START);
    assert_eq!(first.plan.loop_region.end, QUIET_START + QUIET_LOOP_FRAMES);
}

fn render_quiet_output(
    path: &mut LaunchPath,
    frames: usize,
    partitions: &[usize],
    execution: u64,
) -> Vec<f32> {
    let mut actual = Vec::with_capacity(frames * 2);
    let mut offset = 0;
    let mut part = 0;
    while offset < frames {
        let count = partitions[part % partitions.len()].min(frames - offset);
        actual.extend(path.render(count));
        if part == 0 {
            assert_quiet_first_read(path, execution);
        }
        offset += count;
        part += 1;
    }
    actual
}

#[test]
fn quiet_physical_prefix_and_distinct_attack_survive_both_policies_at_t_and_late_e() {
    const FRAMES: usize = 16_384;
    let expected = quiet_raw_output(FRAMES, 0);
    let trimmed = quiet_raw_output(FRAMES, QUIET_PREFIX);
    let first = |pcm: &[f32]| {
        pcm.chunks_exact(2)
            .position(|frame| frame.iter().any(|sample| *sample != 0.0))
            .unwrap()
    };
    let expected_first = first(&expected);
    assert!(expected_first > first(&trimmed));
    assert!(
        expected[..QUIET_PREFIX * 2]
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert_ne!(
        expected, trimmed,
        "a scout/trim cannot satisfy the complete-stream oracle"
    );
    for attack in [false, true] {
        for future in [false, true] {
            for partitions in [&[512][..], &[1, 127, 383, 17][..]] {
                let (mut path, _job) = quiet_original_path(attack);
                let start = prepare_quiet_birth(&mut path, false, future);
                let original = start.original();
                let execution = start.execution();
                if future {
                    assert_eq!(execution, original);
                } else {
                    assert!(execution > original);
                }
                silence_until(&mut path, execution);
                path.render(0);
                assert_eq!(start.state(), "applied", "{:?}", start.reason());
                assert_eq!(path.voice().source_playback.position().frame, QUIET_START);
                assert_physical_scalar_provenance(&path.voice().source_playback);
                assert_eq!(
                    path.voice().source_playback.mapped_output_frame(),
                    Some(execution)
                );
                assert_eq!(
                    path.voice().source_playback.tempo_ratio().to_bits(),
                    1.0_f64.to_bits()
                );
                let actual = render_quiet_output(&mut path, FRAMES, partitions, execution);
                assert_eq!(actual, expected);
                assert_eq!(first(&actual), expected_first);
                assert_eq!(
                    path.voice().source_playback.position().frame,
                    QUIET_START + FRAMES
                );
                assert_eq!(
                    path.voice().source_playback.position().fraction.to_bits(),
                    0.0_f64.to_bits()
                );
                let status = sync_feedback(&path).unwrap();
                assert_eq!(status["birth"]["original_target_frame"], original);
                assert_eq!(status["birth"]["execution_frame"], execution);
                assert_eq!(status["birth"]["emitted"], true);
                assert_eq!(
                    status["birth"]["requested"],
                    if attack {
                        "attack_at_grid"
                    } else {
                        "first_sound_at_grid"
                    }
                );
                assert_eq!(
                    status["birth"]["alignment"],
                    if attack {
                        "pending"
                    } else {
                        "earliest_bound_met"
                    }
                );
                if attack {
                    assert_eq!(status["birth"]["reason"], "no_qualified_anchor");
                }
                assert_eq!(status["phase_program_current"], false);
            }
        }
    }
}

#[test]
fn prepared_ordinary_and_piano_native_births_reprepare_twice_without_losing_original_t_or_pcm() {
    assert_twice_reprepared_native_births(false);
}

#[test]
fn finite_prepared_ordinary_and_piano_native_births_requalify_coverage_twice_and_keep_complete_pcm()
{
    assert_twice_reprepared_native_births(true);
}

fn assert_twice_reprepared_native_births(finite: bool) {
    let expected = quiet_raw_output(16_384, 0);
    for piano in [false, true] {
        let (mut path, _job) = quiet_original_path_with_residency(true, finite);
        let start = prepare_quiet_birth(&mut path, piano, false);
        let original = start.original();
        let binding = binding(&path.source, 0);
        let control = path
            .source
            .mixer
            .source_control_revision(start.key())
            .unwrap();
        let witness = |path: &LaunchPath| {
            path.source
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ready_mapped_birth_for_test(start.sequence())
                .unwrap()
        };
        let first = witness(&path);
        assert!(first.complete_beginning);
        assert!(first.reference.matches_exact(
            crate::audio_engine::loop_phase_reference::LoopPhaseReference::capture(
                Some(path.timeline),
                original
            )
        ));
        assert_eq!(
            first.checkpoint.playback.tempo_ratio().to_bits(),
            1.0_f64.to_bits()
        );
        let original_reference = first.reference;
        let original_permit = first.permit;
        let mut previous_stamp = first.stamp;
        for master in [0.25, 0.75] {
            let old_execution = start.execution();
            assert!(
                path.timeline
                    .anchor_beat_position_at_frame(master, path.timeline.output_frame())
            );
            assert!(binding.current());
            assert_eq!(
                path.source
                    .mixer
                    .source_control_revision(start.key())
                    .unwrap(),
                control
            );
            silence_until(&mut path, old_execution);
            path.render(0);
            assert!(!start.effective());
            assert_ne!(start.state(), "rejected", "{:?}", start.reason());
            assert_eq!(start.original(), original);
            assert!(start.execution() > old_execution);
            assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
            ready(&mut path, start.sequence());
            let next = witness(&path);
            assert!(next.complete_beginning);
            assert!(next.reference.matches_exact(original_reference));
            assert_eq!(next.permit, original_permit);
            assert_ne!(
                (next.stamp.0, next.stamp.2),
                (previous_stamp.0, previous_stamp.2)
            );
            assert_eq!(
                next.stamp.3, previous_stamp.3,
                "only the original H silence was fed"
            );
            assert_eq!(next.stamp.4, start.execution());
            assert_eq!(next.checkpoint.playback.position().frame, QUIET_START);
            assert_eq!(
                next.checkpoint.playback.position().fraction.to_bits(),
                0.0_f64.to_bits()
            );
            assert_eq!(
                next.checkpoint.playback.mapped_output_frame(),
                Some(start.execution())
            );
            previous_stamp = next.stamp;
        }
        let execution = start.execution();
        let final_prepared = witness(&path);
        silence_until(&mut path, execution);
        path.render(0);
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert!(
            path.voice()
                .source_playback
                .matches_exact(&final_prepared.checkpoint.playback)
        );
        assert_eq!(
            path.voice().stretch.native_state_address(),
            final_prepared.stamp.1
        );
        assert_eq!(
            render_quiet_output(&mut path, 16_384, &[1, 127, 383, 17], execution),
            expected
        );
        let status = sync_feedback(&path).unwrap();
        assert_eq!(status["birth"]["original_target_frame"], original);
        assert_eq!(status["birth"]["execution_frame"], execution);
        assert_eq!(status["birth"]["reference_qualified_at_birth"], false);
        assert_eq!(status["birth"]["emitted"], true);
        assert_eq!(status["birth"]["alignment"], "pending");
        assert_eq!(status["birth"]["reason"], "no_qualified_anchor");
        assert_eq!(status["phase_program_current"], false);
    }
}

#[test]
fn real_master_control_change_refuses_prepared_ordinary_and_piano_births_without_recapture() {
    for piano in [false, true] {
        let (mut path, _job) = quiet_original_path(false);
        let start = prepare_quiet_birth(&mut path, piano, false);
        let original = start.original();
        let execution = start.execution();
        assert!(
            path.source
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ready_mapped_birth_for_test(start.sequence())
                .unwrap()
                .complete_beginning
        );
        let control = path
            .source
            .mixer
            .source_control_revision(start.key())
            .unwrap();
        path.source.mixer.set_master_period(PERIOD * 2.0);
        assert!(path.timeline.set_master_period(PERIOD * 2.0));
        assert_ne!(
            path.source
                .mixer
                .source_control_revision(start.key())
                .unwrap(),
            control
        );
        silence_until(&mut path, execution);
        path.render(0);
        assert_eq!(start.state(), "rejected", "{:?}", start.reason());
        assert!(!start.effective());
        assert_eq!(start.original(), original);
        assert_eq!(start.execution(), execution);
        assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
        assert!(
            path.source
                .mixer
                .pitch_runtime
                .as_ref()
                .unwrap()
                .ready_mapped_birth_for_test(start.sequence())
                .is_none()
        );
        assert!(path.render(513).iter().all(|sample| *sample == 0.0));
    }
}

#[test]
fn public_mapped_native_complete_prefix_matches_raw_cabi_and_callback_partition() {
    // Constant physical input makes the PCM oracle independent of map interpolation.
    // The entire raw prefix remains in the comparison, including its valid silence.
    let mut outputs = Vec::new();
    for partitions in [&[512][..], &[1, 127, 383, 17][..]] {
        let (mut path, job) = decimal_general_path();
        path.source
            .mixer
            .set_pad_pitch_intent(0, PitchIntent::new(7, 0).unwrap())
            .unwrap();
        let start = path
            .source
            .engine
            .capture_pad_start(
                0,
                1.0 / f64::from(RATE),
                Some(43_680.0 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        let geometry = start.mapped_geometry().unwrap();
        assert!(geometry.has_general_continuation());
        assert!(!geometry.has_compact_continuation());
        path.publish_capture(&start);
        ready(&mut path, start.sequence());
        let execution = start.execution_frame().unwrap();
        assert_eq!(start.target_frame(), Some(0));
        assert!(execution > 0);
        render_to(&mut path, execution);
        let h = 2.0_f64.powf(7.0 / 12.0);
        let mut raw = mapped_fixed_pitch_birth_tests::RawStereo::new(h);
        // Include the decimal knot and the selected physical loop's first wrap.
        let count = raw.history.max(48_000) + 1024;
        let expected: Vec<_> = (0..count).flat_map(|_| raw.sample([0.25; 2])).collect();
        let mut actual = Vec::with_capacity(expected.len());
        let mut offset = 0;
        let mut partition = 0;
        while offset < count {
            let frames = partitions[partition % partitions.len()].min(count - offset);
            actual.extend(path.render(frames));
            if partition == 0 {
                job.cancel(); // Pending work is revoked; this adopted owner remains immutable.
            }
            offset += frames;
            partition += 1;
        }
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(actual, expected);
        assert_ne!(path.voice().stretch.native_state_address(), 0);
        assert!(path.voice().stretch.has_effective_mapped_voice_for_test());
        assert!(path.voice().source_playback.is_general_mapped());
        let mut history = SourceFeedSnapshot::new();
        assert!(path.voice().source_feed_history.capture_into(&mut history));
        let first = history.spans().next().unwrap();
        assert_eq!(first.playback.position().frame, 1);
        assert_eq!(
            first.playback.position().fraction.to_bits(),
            0.0_f64.to_bits()
        );
        assert_eq!(first.playback.mapped_output_frame(), Some(execution));
        let status = sync_feedback(&path).unwrap();
        assert_eq!(status["birth"]["emitted"], true);
        assert_eq!(status["birth"]["alignment"], "earliest_bound_met");
        assert_eq!(status["birth"]["execution_frame"], execution);
        assert_eq!(status["phase_program_current"], false);
        outputs.push(actual);
    }
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn public_mapped_key_lock_initial_unity_owns_native_before_later_map_rate_change() {
    let mut path = complete_launch_path();
    let mut expected = record(&path.source, 0);
    let accepted = &mut expected["record"]["accepted"];
    let first = accepted["anchors"][0].clone();
    let mut middle = accepted["anchors"][1].clone();
    middle["id"] = json!("unity-before-knot");
    middle["source_seconds_bits"] = json!(bits(1.0 / 1024.0));
    middle["beat_bits"] = json!(bits(0.732421875));
    let mut last = accepted["anchors"][1].clone();
    last["source_seconds_bits"] = json!(bits(1.0 / 512.0));
    last["beat_bits"] = json!(bits(2.197265625));
    let interval = accepted["intervals"][0].clone();
    accepted["anchors"] = json!([first, middle, last]);
    accepted["intervals"] = json!([interval.clone(), interval]);
    seal(&mut expected);
    let (job, event) = stored_event(&path.source, &expected);
    let delivery = Python::attach(|py| {
        let event = event.bind(py).cast::<PyDict>().unwrap();
        let owner = event.get_item("_native_event_owner").unwrap().unwrap();
        let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
        assert!(
            job.resolve_source_beatmap_artifact_completion(&delivery)
                .unwrap()
                .is_some()
        );
        delivery.clone()
    });
    publish_original(&path, &delivery, 1);
    assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
    path.drain();
    let start = path.capture();
    path.publish_capture(&start);
    ready(&mut path, start.sequence());
    render_to(&mut path, start.execution_frame().unwrap());
    path.drain();
    path.render(0); // Execute the due claim without offering any first output interval.
    assert_eq!(key_lock_feedback(&path)["ready"], false);
    assert_eq!(key_lock_feedback(&path)["effective"], false);
    path.render(1);
    assert_eq!(start.state(), "applied", "{:?}", start.reason());
    assert_eq!(path.voice().source_playback.rate_target(), 1.0);
    let native = path.voice().stretch.native_state_address();
    assert_ne!(native, 0);
    path.render(17);
    assert_eq!(path.voice().source_playback.rate_target(), 0.5);
    assert_eq!(path.voice().stretch.native_state_address(), native);
    assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], true);
    assert_eq!(key_lock_feedback(&path)["ready"], true);
}

#[test]
fn accepted_musical_domain_keeps_compact_ordinary_and_piano_births() {
    const PERIOD: f64 = 1.0 / 64.0;
    const END: usize = 1500;
    for piano in [false, true] {
        let mut source = ColdCustody::new_selected_with_signal(None, &[8192; 4096]);
        source.engine.input_runtime = Some(InputRuntime::new_with_ownership(
            source.producer.clone(),
            source.engine.input_clock,
            source.engine.input_runtime_ownership.clone(),
        ));
        let mut path = LaunchPath::from_source(source);
        path.source.mixer.set_master_period(PERIOD);
        assert!(path.timeline.set_master_period(PERIOD));
        path.source
            .engine
            .set_pad_timing_intent(0, "automatic")
            .unwrap();
        path.drain();
        let timing = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
            &path.source.engine,
            &path.source.producer,
            0,
            PERIOD,
        );
        path.drain();
        assert_eq!(timing.publication_status().unwrap(), "accepted");
        let current = binding(&path.source, 0);
        let accepted = current.binding.accepted.unwrap();
        assert_eq!(accepted.period_seconds.to_bits(), PERIOD.to_bits());
        assert_eq!(
            path.source
                .engine
                .current_timing_acknowledgements
                .current_for_slot(&path.source.engine.input_runtime_ownership, 0,),
            accepted.publication_epoch
        );
        let (_job, delivery) = delivery_for_points(&path, &[(0.0, 0.0), (1.0 / 16.0, 4.0)]);
        assert_live_compact_preparation(&path, &delivery, None, 0, END);
        let mut bindings = vec![None; NUM_SAMPLES];
        bindings[0] = Some(current);
        let mut maps = vec![SourceMapRow::default(); NUM_SAMPLES];
        maps[0] = SourceMapRow {
            sync_requested: true,
            intent_revision: 1,
            delivery: Some(delivery),
            exact_quarters: None,
            ordinary_pending: false,
        };
        path.source
            .engine
            .input_runtime
            .as_ref()
            .unwrap()
            .set_runtime_state_with_source_maps(
                true,
                (0..NUM_SAMPLES).map(|id| id == 0).collect(),
                vec![0.0; NUM_SAMPLES],
                vec![Some(1.0 / 32.0); NUM_SAMPLES],
                bindings.iter().map(Option::as_ref).collect(),
                Some(vec![
                    PitchPadProjection {
                        sync: true,
                        ..Default::default()
                    };
                    NUM_SAMPLES
                ]),
                Some(maps),
            )
            .unwrap();
        assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
        path.drain();
        let start = if piano {
            pitch_rows(&path, true);
            pitch_clock(&path);
            let action = path
                .source
                .engine
                .capture_pad_pitch_action(0, "pitch.extra:0:set_retrigger", None)
                .unwrap();
            assert!(action.mapped_geometry().unwrap().has_compact_continuation());
            assert!(!action.mapped_geometry().unwrap().has_general_continuation());
            assert!(path.source.engine.publish_pitch_action(&action).unwrap());
            path.drain();
            PublicQuietBirth::Piano(action)
        } else {
            let start = path
                .source
                .engine
                .capture_pad_start(0, 0.0, Some(1.0 / 32.0), 1.0, false, None, None)
                .unwrap();
            assert!(start.mapped_geometry().unwrap().has_compact_continuation());
            assert!(!start.mapped_geometry().unwrap().has_general_continuation());
            path.publish_capture(&start);
            PublicQuietBirth::Ordinary(start)
        };
        ready(&mut path, start.sequence());
        let execution = start.execution();
        render_to(&mut path, execution);
        path.render(0);
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        let playback = path.voice().source_playback;
        assert!(playback.mapped_epoch().unwrap().is_compact_anchored());
        assert_eq!(playback.position().frame, 0);
        assert_eq!(playback.position().fraction.to_bits(), 0.0_f64.to_bits());
        assert_eq!(playback.mapped_output_frame(), Some(execution));
        assert_eq!(
            playback.original_scalar_domain(),
            playback.effective_domain()
        );
        assert_eq!(
            playback
                .original_scalar_domain()
                .unwrap()
                .musical_period
                .scalar_period(),
            Some(END as f64)
        );
        let mut raw = mapped_fixed_pitch_birth_tests::RawStereo::new(1.0);
        let expected: Vec<_> = (0..2048).flat_map(|_| raw.sample([0.25; 2])).collect();
        let actual: Vec<_> = [1, 127, 383, 513, 1024]
            .into_iter()
            .flat_map(|frames| path.render(frames))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], true);
        let accepted = path.voice().source_timing.accepted;
        let native = path.voice().stretch.native_state_address();
        assert!(accepted.is_some());
        publish_distinct_cold_source(&mut path, false);
        assert_eq!(path.voice().source_timing.accepted, accepted);
        assert!(path.voice().stretch.has_effective_mapped_voice_for_test());
        let expected: Vec<_> = (0..2048).flat_map(|_| raw.sample([0.25; 2])).collect();
        assert_eq!(path.render(2048), expected);
        assert_eq!(path.voice().stretch.native_state_address(), native);
        assert_eq!(path.voice().source_timing.accepted, accepted);
    }
}

#[test]
fn physical_compact_geometry_birth_uses_general_then_off_preserves_original_pair_and_domain() {
    let mut path = complete_launch_path();
    let (_job, delivery) = covered_original_delivery(&path, false);
    assert_live_compact_preparation(&path, &delivery, None, 32, 64);
    publish_original(&path, &delivery, 1);
    let start = path.start();
    assert!(start.mapped_geometry().unwrap().has_general_continuation());
    assert!(path.voice().source_playback.is_general_mapped());
    assert_physical_scalar_provenance(&path.voice().source_playback);
    assert_eq!(path.voice().source_playback.position().frame, 32);
    assert_eq!(
        path.voice().source_playback.position().fraction.to_bits(),
        0.0_f64.to_bits()
    );
    path.render(17);
    let before = path.voice().source_playback;
    let generation = path.voice().generation;
    let source = path.voice().sample.as_ref().unwrap().source_address();
    let runtime = path.source.engine.input_runtime.as_ref().unwrap();
    let mut bindings = vec![None; NUM_SAMPLES];
    bindings[0] = Some(binding(&path.source, 0));
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    rows[0] = SourceMapRow {
        sync_requested: true,
        intent_revision: 2,
        delivery: Some(delivery),
        exact_quarters: None,
        ordinary_pending: false,
    };
    runtime
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id == 0).collect(),
            vec![32.0 / f64::from(RATE); NUM_SAMPLES],
            vec![Some(64.0 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(rows),
            crate::audio_engine::source_sync_policy::GlobalTimingPolicy::parse(Some(
                r#"{"schema_version":1,"sync":false,"realtime_bpm":null,"revision":2}"#,
            ))
            .unwrap(),
        )
        .unwrap();
    runtime.request_runtime_policy_adoption().unwrap();
    let request = runtime.runtime_policy_snapshot().unwrap().unwrap();
    let requested = request.row(0).unwrap().request;
    assert_eq!(requested.policy_sync, Some(false));
    assert_eq!(requested.global_revision, Some(2));
    assert_eq!(requested.intent_revision, 2);
    assert_eq!(requested.mode, SourceSyncMode::Legacy);
    path.drain();
    // Policy publication advances the common fence. Perform both real drains
    // at the unchanged actual frame before the scheduler may claim conversion.
    let (_parameters, mut parameter_consumer) = rtrb::RingBuffer::new(1);
    drain_parameter_messages(
        &mut parameter_consumer,
        &mut path.source.mixer,
        &mut path.timeline,
    );
    assert_eq!(path.timeline.output_frame(), 17);
    path.render(0);
    let observation = request.observation().unwrap();
    assert_eq!(observation.state, "applied", "{observation:?}");
    let after = path.voice().source_playback;
    assert!(!after.is_mapped());
    assert_eq!(after.effective_domain(), before.original_scalar_domain());
    assert_eq!(after.position().frame, before.position().frame);
    assert_eq!(
        after.position().fraction.to_bits(),
        before.position().fraction.to_bits()
    );
    assert_eq!(path.voice().generation, generation);
    assert_eq!(
        path.voice().sample.as_ref().unwrap().source_address(),
        source
    );
    assert_eq!(path.render(1), vec![0.25; 2]);
}

#[test]
fn dispatched_midi_mapped_birth_uses_the_real_stopped_key_lock_request_for_both_policies() {
    for attack in [false, true] {
        let mut path = complete_launch_path();
        let (_job, delivery) = covered_original_delivery(&path, true);
        publish_original(&path, &delivery, 1);
        set_start_policy_after_idle_callback(&mut path, attack);
        let request = path.source.engine.set_pad_key_lock(0, true).unwrap();
        assert_ne!(request, 0);
        path.drain();
        let runtime = path.source.engine.input_runtime.as_ref().unwrap();
        runtime.replace_mappings(vec![("midi:note:1:60".into(), "pad.trigger:0".into())]);
        runtime.set_enabled(true);
        assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let start = loop {
            if let Some(event) = runtime.poll_event() {
                assert!(event.direct && event.dispatched);
                assert_eq!(event.action_key.as_deref(), Some("pad.trigger:0"));
                let start = event.captured_trigger.unwrap().captured_start();
                assert_eq!(start.received_timestamp(), event.received_at_ns);
                break start;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "mapped Native MIDI birth"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        path.drain();
        ready(&mut path, start.sequence());
        let execution = start.execution_frame().unwrap();
        render_to(&mut path, execution);
        path.drain();
        path.render(0);
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_ne!(path.voice().stretch.native_state_address(), 0);
        assert_eq!(key_lock_feedback(&path)["request_id"], request);
        assert_eq!(key_lock_feedback(&path)["ready"], false);
        assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], false);
        path.render(1);
        let status = sync_feedback(&path).unwrap();
        assert_eq!(status["birth"]["emitted"], true);
        assert_eq!(
            status["birth"]["original_target_frame"],
            start.target_frame().unwrap()
        );
        assert_eq!(status["birth"]["execution_frame"], execution);
        assert_eq!(
            status["birth"]["alignment"],
            if attack {
                "pending"
            } else {
                "earliest_bound_met"
            }
        );
        assert_eq!(status["phase_program_current"], false);
        assert_eq!(key_lock_feedback(&path)["ready"], true);
    }
}

#[test]
fn rejected_later_key_lock_requests_keep_the_old_native_fifo_and_complete_pcm() {
    for attack in [false, true] {
        let (mut actual, _job, start, original_request) = stopped_key_lock_birth(7, attack);
        let (mut reference, _reference_job, reference_start, _) = stopped_key_lock_birth(7, attack);
        let execution = start.execution_frame().unwrap();
        assert_eq!(reference_start.execution_frame(), Some(execution));
        render_to(&mut actual, execution);
        render_to(&mut reference, execution);
        actual.drain();
        reference.drain();
        actual.render(0);
        reference.render(0);
        let native = actual.voice().stretch.native_state_address();
        assert_ne!(native, 0);
        assert_eq!(key_lock_feedback(&actual)["request_id"], original_request);
        assert_eq!(key_lock_feedback(&actual)["ready"], false);
        assert_eq!(sync_feedback(&actual).unwrap()["birth"]["emitted"], false);
        // A newer equal-valued request must not borrow C0 from the earlier birth.
        for (step, enabled) in [true, false, true].into_iter().enumerate() {
            let playback = actual.voice().source_playback;
            let request = actual.source.engine.set_pad_key_lock(0, enabled).unwrap();
            assert!(request > original_request);
            actual.drain();
            assert_eq!(actual.voice().stretch.native_state_address(), native);
            assert!(actual.voice().source_playback.matches_exact(&playback));
            let status = key_lock_feedback(&actual);
            assert_eq!(status["request_id"], request);
            assert_eq!(status["state"], "error");
            assert_eq!(status["ready"], false);
            for frames in [1, 127, 383, 17, 512] {
                assert_eq!(actual.render(frames), reference.render(frames));
                assert_eq!(actual.voice().stretch.native_state_address(), native);
            }
            assert_eq!(sync_feedback(&actual).unwrap()["birth"]["emitted"], true);
            assert_eq!(key_lock_feedback(&actual)["ready"], false);
            assert_eq!(
                key_lock_feedback(&actual)["effective"],
                true,
                "actual old ON survives request {step}"
            );
        }
    }
}

#[test]
fn unqualified_overlap_cannot_borrow_or_destroy_a_retained_native_birth() {
    {
        let (mut actual, _job, start, _) = stopped_key_lock_birth(7, false);
        let (mut reference, _reference_job, reference_start, _) = stopped_key_lock_birth(7, false);
        let execution = start.execution_frame().unwrap();
        assert_eq!(reference_start.execution_frame(), Some(execution));
        render_to(&mut actual, execution);
        render_to(&mut reference, execution);
        assert_eq!(actual.render(17), reference.render(17));
        let native = actual.voice().stretch.native_state_address();
        let original_source = actual.voice().sample.as_ref().unwrap().source_address();
        assert_eq!(sync_feedback(&actual).unwrap()["birth"]["emitted"], true);
        for path in [&mut actual, &mut reference] {
            // An unqualified legacy voice has no mapped birth witness.
            // It must not borrow the retained voice's C0 or positive status.
            let sample = path.voice().sample.as_ref().unwrap().clone();
            let index = path
                .source
                .mixer
                .voices
                .iter()
                .position(|voice| !voice.active)
                .unwrap();
            path.source.mixer.voices[index].start_rt(
                crate::audio_engine::voice_slot::VoiceStartConfig {
                    sample_id: 0,
                    sample,
                    initial_frame_pos: 32,
                    volume: 0.0,
                    initial_tempo_ratio: 1.0,
                    start_output_frame: None,
                    source_timing: Default::default(),
                    source_admission: None,
                },
                &mut ImmediateAudioBufferRetirement,
            );
        }
        let playback = actual.voice().source_playback;
        let request = actual.source.engine.set_pad_key_lock(0, false).unwrap();
        actual.drain();
        assert_eq!(actual.voice().stretch.native_state_address(), native);
        assert_eq!(
            actual.voice().sample.as_ref().unwrap().source_address(),
            original_source
        );
        assert!(actual.voice().source_playback.matches_exact(&playback));
        let status = key_lock_feedback(&actual);
        assert_eq!(status["request_id"], request);
        assert_eq!(status["state"], "error");
        assert_eq!(status["ready"], false);
        for frames in [1, 127, 383, 17, 512] {
            assert_eq!(actual.render(frames), reference.render(frames));
            assert_eq!(actual.voice().stretch.native_state_address(), native);
        }
        let birth = sync_feedback(&actual).unwrap()["birth"].clone();
        assert_eq!(birth["requested"], "mixed");
        assert_eq!(birth["emitted"], false);
        assert!(birth["original_target_frame"].is_null());
        assert!(birth["execution_frame"].is_null());
    }
}

fn publish_distinct_cold_source(path: &mut LaunchPath, replace_assignment: bool) {
    // Only this test's generated RIFF is copied. The pinned old source stays intact.
    let mut bytes = std::fs::read(path.source.directory.path().join("external.wav")).unwrap();
    for sample in bytes[44..].chunks_exact_mut(2) {
        sample.copy_from_slice(&(-8192_i16).to_le_bytes());
    }
    let new_path = path.source.directory.path().join("distinct.wav");
    std::fs::write(&new_path, bytes).unwrap();
    assert!(path.source.consumer.is_empty());
    let request = crate::audio_engine::cold_load::admit_for_format_selected(
        &path.source.engine,
        0,
        new_path.to_string_lossy().into(),
        (false, false, replace_assignment, None),
        path.source.producer.clone(),
        (2, RATE, path.source.directory.path().join("samples")),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while path
        .source
        .engine
        .input_runtime_ownership
        .cold_status(0, request)
        != Some(0)
        || path.source.consumer.slots() != 1
    {
        assert!(
            std::time::Instant::now() < deadline,
            "distinct source preparation"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let frame = path.timeline.output_frame();
    path.drain();
    assert_eq!(path.timeline.output_frame(), frame);
    assert_eq!(
        path.source
            .engine
            .input_runtime_ownership
            .cold_status(0, request),
        Some(2)
    );
    while path.source.engine.cold_loading[0].load(std::sync::atomic::Ordering::Acquire) != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "distinct source own ACK"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(binding(&path.source, 0).current());
}

#[test]
fn explicit_assignment_replacement_stops_mapped_birth_and_clears_current_feedback() {
    let (mut path, _job, start, _) = stopped_key_lock_birth(7, false);
    render_to(&mut path, start.execution_frame().unwrap());
    path.render(17);
    assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], true);
    let before = binding(&path.source, 0);
    let old_key = before.binding.placement.unwrap().content_key();
    publish_distinct_cold_source(&mut path, true);
    let after = binding(&path.source, 0);
    assert_ne!(after.source_generation, before.source_generation);
    assert_ne!(after.binding.source_address, before.binding.source_address);
    assert_ne!(after.binding.placement.unwrap().content_key(), old_key);
    assert!(
        !path
            .source
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 0)
    );
    assert!(path.render(17).iter().all(|sample| *sample == 0.0));
    assert!(sync_feedback(&path).unwrap()["birth"].is_null());
    // FreshNeutral clears pitch, preserving the existing KL preference. An idle
    // warmed reserve is available for the new source; it is not an emitted birth.
    let status = key_lock_feedback(&path);
    assert_eq!(status["source_generation"], after.source_generation);
    assert_eq!(status["source_identity"], after.binding.source_address);
    assert_eq!(status["request_id"], 0);
    assert_eq!(status["state"], "armed");
    assert_eq!(status["effective"], true);
    assert_eq!(status["ready"], true);
    assert!(status["error"].is_null());
}

#[test]
fn prepared_mapped_birth_cannot_acquire_effective_lifetime_after_source_replacement() {
    let (mut path, _job, start, _) = stopped_key_lock_birth(7, false);
    let execution = start.execution_frame().unwrap();
    assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
    publish_distinct_cold_source(&mut path, true);
    render_to(&mut path, execution);
    assert!(path.render(513).iter().all(|sample| *sample == 0.0));
    assert_eq!(start.state(), "rejected");
    assert!(start.effective_frame().is_none());
    assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
    assert!(sync_feedback(&path).unwrap()["birth"].is_null());
}

#[test]
fn nonreplacing_source_publication_and_rejected_key_lock_keep_old_native_complete_pcm() {
    let (mut actual, _job, start, _) = stopped_key_lock_birth(7, false);
    let (mut reference, _reference_job, reference_start, _) = stopped_key_lock_birth(7, false);
    let execution = start.execution_frame().unwrap();
    assert_eq!(reference_start.execution_frame(), Some(execution));
    render_to(&mut actual, execution);
    render_to(&mut reference, execution);
    assert_eq!(actual.render(17), reference.render(17));
    assert!(actual.voice().stretch.has_effective_mapped_voice_for_test());
    let native = actual.voice().stretch.native_state_address();
    let original_source = actual.voice().sample.as_ref().unwrap().source_address();
    let playback = actual.voice().source_playback;
    let before = binding(&actual.source, 0);
    publish_distinct_cold_source(&mut actual, false);
    let after = binding(&actual.source, 0);
    assert_ne!(after.source_generation, before.source_generation);
    assert_ne!(after.binding.source_address, before.binding.source_address);
    assert_eq!(
        actual.voice().sample.as_ref().unwrap().source_address(),
        original_source
    );
    assert_eq!(actual.voice().stretch.native_state_address(), native);
    assert!(actual.voice().source_playback.matches_exact(&playback));
    assert!(sync_feedback(&actual).is_none_or(|status| status["birth"].is_null()));
    assert!(actual.voice().stretch.has_effective_mapped_voice_for_test());
    // Publication alone must preserve the old stream, before any new KL request.
    assert_eq!(actual.render(513), reference.render(513));
    let playback = actual.voice().source_playback;
    let request = actual.source.engine.set_pad_key_lock(0, false).unwrap();
    assert_ne!(request, 0);
    actual.drain();
    assert_eq!(actual.voice().stretch.native_state_address(), native);
    assert!(actual.voice().source_playback.matches_exact(&playback));
    let status = key_lock_feedback(&actual);
    assert_eq!(status["request_id"], request);
    assert_eq!(status["state"], "error");
    assert_eq!(status["ready"], false);
    for frames in [1, 127, 383, 17, 512, 1024] {
        assert_eq!(actual.render(frames), reference.render(frames));
        assert_eq!(actual.voice().stretch.native_state_address(), native);
        assert_eq!(
            actual.voice().sample.as_ref().unwrap().source_address(),
            original_source
        );
    }
    assert!(sync_feedback(&actual).unwrap()["birth"].is_null());
    assert_eq!(key_lock_feedback(&actual)["ready"], false);
}

#[test]
fn superseding_key_lock_request_before_effect_refuses_the_original_prepared_birth() {
    for enabled in [false, true] {
        let (mut path, _job, start, original) = stopped_key_lock_birth(7, false);
        let replacement = path.source.engine.set_pad_key_lock(0, enabled).unwrap();
        assert!(replacement > original);
        path.drain();
        let execution = start.execution_frame().unwrap();
        render_to(&mut path, execution);
        path.render(1);
        assert_eq!(start.state(), "rejected");
        assert!(start.effective_frame().is_none());
        assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
        assert!(sync_feedback(&path).unwrap()["birth"].is_null());
    }
}

#[test]
fn paused_first_mapped_native_interval_cannot_confirm_key_lock_or_birth() {
    let (mut path, _job, start, _) = stopped_key_lock_birth(0, false);
    render_to(&mut path, start.execution_frame().unwrap());
    path.drain();
    path.render(0);
    let index = path
        .source
        .mixer
        .voices
        .iter()
        .position(|voice| voice.active && voice.sample_id == 0)
        .unwrap();
    path.source.mixer.voices[index].pause();
    assert!(path.render(17).iter().all(|sample| *sample == 0.0));
    assert_eq!(key_lock_feedback(&path)["ready"], false);
    assert_eq!(key_lock_feedback(&path)["effective"], false);
    assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], false);
    path.source.mixer.voices[index].resume();
    path.render(17);
    assert_eq!(key_lock_feedback(&path)["ready"], false);
    assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], false);
}

#[test]
fn public_mapped_piano_retrigger_keeps_original_request_and_pending_attack_alignment() {
    let mut path = complete_launch_path();
    let (_job, delivery) = covered_original_delivery(&path, true);
    publish_original(&path, &delivery, 1);
    set_start_policy_after_idle_callback(&mut path, true);
    assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
    path.drain();
    pitch_rows(&path, true);
    pitch_clock(&path);
    let action = path
        .source
        .engine
        .capture_pad_pitch_action(0, "pitch.extra:7:set_retrigger", None)
        .unwrap();
    assert_eq!(action.requested_sync_request().mode, SourceSyncMode::Mapped);
    assert_eq!(action.sync_request().mode, SourceSyncMode::Mapped);
    assert!(path.source.engine.publish_pitch_action(&action).unwrap());
    path.drain();
    ready(&mut path, action.sequence());
    let execution = action.execution_target();
    render_to(&mut path, execution);
    path.render(1);
    assert_eq!(action.state(), "applied", "{:?}", action.reason());
    assert!(path.voice().source_playback.is_mapped());
    assert_ne!(path.voice().stretch.native_state_address(), 0);
    assert!(path.voice().stretch.has_effective_mapped_voice_for_test());
    let status = sync_feedback(&path).unwrap();
    assert_eq!(status["birth"]["requested"], "attack_at_grid");
    assert_eq!(status["birth"]["alignment"], "pending");
    assert_eq!(status["birth"]["reason"], "no_qualified_anchor");
    assert_eq!(status["birth"]["emitted"], true);
    assert_eq!(
        status["birth"]["original_target_frame"],
        action.original_due_frame()
    );
    assert_eq!(status["birth"]["execution_frame"], execution);
    assert_eq!(status["phase_program_current"], false);
}

#[test]
fn fresh_piano_without_map_uses_only_original_explicit_pending_permission() {
    let mut path = complete_launch_path();
    publish(
        &path.source,
        SourceMapRow {
            sync_requested: true,
            intent_revision: 1,
            ordinary_pending: true,
            delivery: None,
            exact_quarters: None,
        },
    )
    .unwrap();
    assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
    path.drain();
    pitch_rows(&path, true);
    pitch_clock(&path);
    let action = path
        .source
        .engine
        .capture_pad_pitch_action(0, "pitch.extra:7:set_retrigger", None)
        .unwrap();
    assert_eq!(action.requested_sync_request().mode, SourceSyncMode::Mapped);
    assert_eq!(action.sync_request().mode, SourceSyncMode::Legacy);
    assert!(action.mapped_geometry().is_none());
    assert!(path.source.engine.publish_pitch_action(&action).unwrap());
    path.drain();
    ready(&mut path, action.sequence());
    render_to(&mut path, action.execution_target());
    path.render(1);
    assert_eq!(action.state(), "applied", "{:?}", action.reason());
    assert!(!path.voice().source_playback.is_mapped());
    assert_eq!(path.voice().sync_request.mode, SourceSyncMode::Legacy);
    assert!(sync_feedback(&path).unwrap()["birth"].is_null());
    // Live SET still owns the ordinary timing guards; this permission is not a map.
    publish(
        &path.source,
        SourceMapRow {
            sync_requested: true,
            intent_revision: 2,
            ordinary_pending: false,
            delivery: None,
            exact_quarters: None,
        },
    )
    .unwrap();
    pitch_clock(&path);
    assert!(
        path.source
            .engine
            .capture_pad_pitch_action(0, "pitch.extra:8:set_retrigger", None)
            .is_err()
    );
}

#[test]
fn public_global_native_mapped_and_dry_members_share_actual_e_and_original_t() {
    let mut path = complete_launch_path();
    let (_job, delivery) = covered_original_delivery(&path, true);
    publish_mixed_members(&path, &delivery);
    assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
    path.drain();
    let (group, members) = capture_mixed_global(&path);
    path.drain();
    let ticket = path
        .source
        .engine
        .publish_captured_global_start(&group)
        .unwrap()
        .unwrap();
    path.drain();
    ready(&mut path, members[0].sequence());
    let execution = members[0].execution_frame().unwrap();
    assert!(execution > 0);
    render_to(&mut path, execution);
    path.render(1);
    assert_eq!(group.state(), "applied", "{:?}", group.reason());
    assert_eq!(ticket.publication_status(), "accepted");
    for member in &members {
        assert_eq!(member.target_frame(), Some(0));
        assert_eq!(member.execution_frame(), Some(execution));
        assert_eq!(member.effective_frame(), Some(execution));
    }
    assert_ne!(path.voice().stretch.native_state_address(), 0);
    assert!(path.voice().stretch.has_effective_mapped_voice_for_test());
    let scalar = path
        .source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap();
    assert!(!scalar.source_playback.is_mapped());
    assert_eq!(scalar.source_playback.position().frame, 33);
    assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], true);
}

#[test]
fn last_global_mapped_birth_exhausted_lifetime_refuses_without_scalar_prefix() {
    let mut path = complete_launch_path();
    let (_job, delivery) = covered_original_delivery(&path, true);
    publish_mixed_members(&path, &delivery);
    assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
    path.drain();
    // The dry member is first, so this exercises all-member staging before effects.
    let group = Python::attach(|py| {
        let entries = [1, 0]
            .into_iter()
            .map(|id| {
                let binding = path
                    .source
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap();
                (
                    Py::new(py, binding).unwrap(),
                    32.0 / f64::from(RATE),
                    Some(64.0 / f64::from(RATE)),
                    None,
                )
            })
            .collect();
        path.source
            .engine
            .capture_global_start(py, entries, None)
            .unwrap()
    });
    let members = group.resolve().unwrap().unwrap();
    assert!(members[0].start.mapped_geometry().is_none());
    assert!(members[1].start.mapped_geometry().is_some());
    let native = members[1].start.clone();
    path.drain();
    let _ticket = path
        .source
        .engine
        .publish_captured_global_start(&group)
        .unwrap()
        .unwrap();
    path.drain();
    ready(&mut path, native.sequence());
    let execution = native.execution_frame().unwrap();
    render_to(&mut path, execution);
    assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
    let before = path.source.mixer.voices[0].source_playback;
    let generation = path.source.mixer.voices[0].generation;
    // Numeric exhaustion injection grants no source/admission or active voice.
    path.source.mixer.voices[1].generation = u64::MAX - 1;
    assert!(path.render(1).iter().all(|sample| *sample == 0.0));
    assert_eq!(group.state(), "rejected");
    assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
    assert!(
        path.source.mixer.voices[0]
            .source_playback
            .matches_exact(&before)
    );
    assert_eq!(path.source.mixer.voices[0].generation, generation);
    assert!(
        members
            .iter()
            .all(|member| member.start.effective_frame().is_none())
    );
    assert!(sync_feedback(&path).unwrap()["birth"].is_null());
}

#[test]
fn mapped_piano_midi_capture_cancel_revokes_its_original_pending_guard() {
    let mut path = complete_launch_path();
    let (_job, delivery) = covered_original_delivery(&path, true);
    publish_original(&path, &delivery, 1);
    assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
    path.drain();
    pitch_rows(&path, true);
    pitch_clock(&path);
    let runtime = path.source.engine.input_runtime.as_ref().unwrap();
    runtime.set_selected_pad(0).unwrap();
    runtime.replace_mappings(vec![(
        "midi:note:1:60".into(),
        "pitch.extra:7:set_retrigger".into(),
    )]);
    runtime.set_enabled(true);
    assert!(runtime.inject_midi_message(&[0x90, 60, 127]));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let action = loop {
        if let Some(event) = runtime.poll_event() {
            assert!(event.direct && !event.dispatched);
            assert!(event.pitch_rejection.is_none());
            break event.captured_pitch.unwrap();
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(action.sync_request().mode, SourceSyncMode::Mapped);
    let source = action.mapped_source_owner().unwrap();
    let pending = action.mapped_pending_guard().unwrap();
    assert!(pending.current(&source));
    runtime
        .cancel_pitch_capture(
            &action,
            crate::audio_engine::pitch_actions::PitchActionReject::Capacity,
        )
        .unwrap();
    assert!(!pending.current(&source));
    assert_eq!(action.state(), "rejected");
    assert!(path.source.mixer.voices.iter().all(|voice| !voice.active));
}

#[test]
fn birth_feedback_rejects_request_only_aba_before_next_audio_callback() {
    let mut path = complete_launch_path();
    let (_job, delivery) = covered_original_delivery(&path, false);
    publish_original(&path, &delivery, 1);
    path.start();
    path.render(1);
    assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], true);
    let original = binding(&path.source, 0);
    path.source
        .engine
        .input_runtime_ownership
        .prepare_source_request_advance(0)
        .unwrap()
        .commit();
    let next = binding(&path.source, 0);
    assert!(original.binding.same_projection(&next.binding));
    assert!(
        sync_feedback(&path).is_none(),
        "old birth cannot cross an identical-binding request ABA"
    );
    path.render(1);
    assert!(sync_feedback(&path).unwrap()["birth"].is_null());
}

#[test]
fn silent_dry_first_interval_confirms_mix_but_skipped_first_interval_cannot_recover() {
    for skip_first in [false, true] {
        let mut source = ColdCustody::new_selected_with_pcm(None, [0; 128]);
        source.engine.input_runtime = Some(
            crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
                source.producer.clone(),
                source.engine.input_clock,
                source.engine.input_runtime_ownership.clone(),
            ),
        );
        let mut path = LaunchPath::from_source(source);
        let (_job, delivery) = covered_original_delivery(&path, false);
        publish_original(&path, &delivery, 1);
        let start = path.start();
        assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], false);
        if skip_first {
            path.timeline.advance_by_rendered_frames(1);
        }
        assert!(path.render(8).iter().all(|sample| *sample == 0.0));
        assert_eq!(start.state(), "applied");
        assert_eq!(
            sync_feedback(&path).unwrap()["birth"]["emitted"],
            !skip_first
        );
        path.render(8);
        assert_eq!(
            sync_feedback(&path).unwrap()["birth"]["emitted"],
            !skip_first
        );
        path.source
            .mixer
            .stop_sample_rt(0, &mut ImmediateAudioBufferRetirement);
        assert!(sync_feedback(&path).unwrap()["birth"].is_null());
    }
}
