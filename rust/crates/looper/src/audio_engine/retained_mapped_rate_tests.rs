// Continuous same-owner Native proof; no reset replay or musical/phase acceptance.
use super::*;
use crate::audio_engine::global_rate_status::{RateError, RateState};
use crate::messages::ControlParameterMessage;
use crate::test_memory::{ThreadAllocations, observe_current_thread};

struct Parameters {
    consumer: rtrb::Consumer<ControlParameterMessage>,
}
impl Parameters {
    fn new(path: &mut LaunchPath) -> Self {
        let (producer, consumer) = rtrb::RingBuffer::new(16);
        path.source.engine.test_parameter_producer =
            Some(Arc::new(std::sync::Mutex::new(producer)));
        Self { consumer }
    }
    fn queue(&mut self, path: &mut LaunchPath, speed: f64, period: f64) {
        path.source
            .engine
            .set_speed_and_master_period(speed, period)
            .unwrap();
        path.drain();
    }
    fn drain(&mut self, path: &mut LaunchPath) {
        drain_parameter_messages(
            &mut self.consumer,
            &mut path.source.mixer,
            &mut path.timeline,
        );
        assert!(self.consumer.is_empty());
    }
    fn change(&mut self, path: &mut LaunchPath, speed: f64, period: f64) {
        self.queue(path, speed, period);
        self.drain(path);
        path.render(0);
    }
}
fn effect(path: &LaunchPath) -> crate::audio_engine::global_rate_status::RateEffect {
    path.source
        .engine
        .input_runtime_ownership
        .global_rate_status
        .read()
        .unwrap()
        .effect
}
fn diagnostic(path: &LaunchPath) -> (RateState, RateError) {
    let effect = effect(path);
    (effect.state, effect.error)
}

fn start(path: &mut LaunchPath, shift: i32, key_lock: bool, piano: bool) -> PublicQuietBirth {
    path.source
        .mixer
        .set_pad_pitch_intent(0, PitchIntent::new(shift, 0).unwrap())
        .unwrap();
    let start = if piano {
        pitch_rows(path, key_lock);
        pitch_clock(path);
        let action = path
            .source
            .engine
            .capture_pad_pitch_action(0, &format!("pitch.extra:{shift}:set_retrigger"), None)
            .unwrap();
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
        path.publish_capture(&captured);
        PublicQuietBirth::Ordinary(captured)
    };
    ready(path, start.sequence());
    render_to(path, start.execution());
    path.render(0);
    start
}

fn independent_pad_cohort_source(
    count: usize,
    key_lock: bool,
) -> (LaunchPath, Vec<OfflineAnalysisJob>) {
    let pcm: Vec<_> = (0usize..48_000)
        .map(|frame| {
            frame
                .checked_sub(QUIET_START)
                .map_or(-12_000, declared_quiet_pcm)
        })
        .collect();
    let mut source = ColdCustody::new_selected_with_signal(None, &pcm);
    // PAD0 and PAD1 already have independent real assignments. Load every further
    // PAD through the same worker and its own ACK before any voice is born.
    for id in 2..count {
        source.load_and_ack_selected_with_frames(id, None, pcm.len());
    }
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            source.engine.input_clock,
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let mut path = LaunchPath::from_source(source);
    let mut jobs = Vec::with_capacity(count);
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    for id in 0..count {
        let mut expected = record(&path.source, id);
        expected["record"]["accepted"]["anchors"] = json!(
            [(0.0, 0.0), (0.5, 375.0), (1.0, 750.0)]
                .into_iter()
                .enumerate()
                .map(|(index, (seconds, beat))| json!({
                    "id": format!("cohort-{index}"), "source_seconds_bits": bits(seconds),
                    "beat_bits": bits(beat), "raw_beat_index": null,
                    "review": "reviewed", "sample_edit": null,
                }))
                .collect::<Vec<_>>()
        );
        let interval = json!({"coverage": "trusted", "meter": {
            "status": "quarter_note_4_4_verified", "downbeat_label": 0,
            "evidence": "synthetic dyadic declared-quarter PCM fixture; no musical acceptance"
        }});
        expected["record"]["accepted"]["intervals"] = json!(vec![interval; 2]);
        seal(&mut expected);
        assert!(maps(&path.source).is_empty());
        let job = path
            .source
            .engine
            .begin_source_beatmap_store_async(id, expected.to_string())
            .unwrap();
        assert_eq!(wait_terminal(&job)["stage"], "ready");
        let mut events = maps(&path.source);
        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap();
        let delivery = Python::attach(|py| {
            let event = event.bind(py).cast::<PyDict>().unwrap();
            assert_eq!(
                event
                    .get_item("id")
                    .unwrap()
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                id
            );
            let owner = event.get_item("_native_event_owner").unwrap().unwrap();
            let delivery = owner.extract::<PyRef<'_, LoaderEventDelivery>>().unwrap();
            assert!(
                job.resolve_source_beatmap_artifact_completion(&delivery)
                    .unwrap()
                    .is_some()
            );
            delivery.clone()
        });
        let current = binding(&path.source, id);
        assert!(current.current());
        bindings[id] = Some(current);
        rows[id] = SourceMapRow {
            phase_origin_seconds: None,
            operational_four_quarter_bars: false,
            sync_requested: true,
            intent_revision: 1,
            delivery: Some(delivery),
            exact_quarters: Some(375.0),
            ordinary_pending: false,
        };
        jobs.push(job);
    }
    path.source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_source_maps(
            true,
            (0..NUM_SAMPLES).map(|id| id < count).collect(),
            vec![QUIET_START as f64 / f64::from(RATE); NUM_SAMPLES],
            vec![Some((QUIET_START + QUIET_LOOP_FRAMES) as f64 / f64::from(RATE)); NUM_SAMPLES],
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    key_lock,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(rows),
        )
        .unwrap();
    set_start_policy_after_idle_callback(&mut path, false);
    (path, jobs)
}

fn cohort(count: usize, shift: i32, key_lock: bool) -> (LaunchPath, Vec<OfflineAnalysisJob>) {
    assert!((1..=32).contains(&count));
    let (mut path, jobs) = if count == 1 {
        let (path, job) = quiet_original_path(false);
        (path, vec![job])
    } else {
        independent_pad_cohort_source(count, key_lock)
    };
    for id in 0..count {
        if key_lock || count > 1 {
            assert!(path.source.engine.set_pad_key_lock(id, key_lock).unwrap() > 0);
            path.drain();
        }
        path.source
            .mixer
            .set_pad_pitch_intent(id, PitchIntent::new(shift, 0).unwrap())
            .unwrap();
    }
    for id in 0..count {
        let captured = path
            .source
            .engine
            .capture_pad_start(
                id,
                QUIET_START as f64 / f64::from(RATE),
                Some((QUIET_START + QUIET_LOOP_FRAMES) as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        path.publish_capture(&captured);
        ready(&mut path, captured.sequence());
        render_to(&mut path, captured.execution_frame().unwrap());
        path.render(0);
        assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
        path.render(17);
    }
    assert_eq!(
        path.source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        count
    );
    let mut ids = Vec::new();
    let mut keys = Vec::new();
    let mut native_addresses = Vec::new();
    for voice in path.source.mixer.voices.iter().filter(|voice| voice.active) {
        let birth = voice.mapped_birth.unwrap();
        let key = voice
            .source_admission
            .as_ref()
            .unwrap()
            .content_key()
            .unwrap();
        let current = binding(&path.source, voice.sample_id);
        assert!(current.current());
        assert_eq!(current.binding.placement.unwrap().content_key(), Some(key));
        assert_eq!(birth.key, key);
        assert_eq!(birth.source_generation, current.source_generation);
        let native = voice.stretch.native_state_address();
        assert_ne!(native, 0);
        assert!(!ids.contains(&voice.sample_id));
        assert!(!keys.contains(&key));
        assert!(!native_addresses.contains(&native));
        ids.push(voice.sample_id);
        keys.push(key);
        native_addresses.push(native);
        assert!(birth.status.emitted);
        assert!(voice.stretch.has_effective_mapped_voice_for_test());
        let coordinates = voice
            .stretch
            .retained_mapped_output_coordinates()
            .unwrap()
            .unwrap();
        assert_eq!(
            coordinates.logical_output_frame,
            coordinates.actual_input_frame
        );
        assert_eq!(
            coordinates.logical_output_frame,
            path.timeline.output_frame()
        );
    }
    ids.sort_unstable();
    assert_eq!(ids, (0..count).collect::<Vec<_>>());
    (path, jobs)
}

#[derive(Clone, Copy)]
struct Master {
    frame: u64,
    beat: f64,
    period: f64,
}
fn master_at(segments: &[Master], frame: u64) -> f64 {
    let segment = segments
        .iter()
        .rev()
        .find(|segment| segment.frame <= frame)
        .unwrap();
    segment.beat + (frame as f64 - segment.frame as f64) / (f64::from(RATE) * segment.period)
}
fn declared_input(segments: &[Master], frame: u64, birth: u64) -> [f32; 2] {
    let relative = OracleFraction::binary(master_at(segments, frame))
        .sub(OracleFraction::binary(master_at(segments, birth)))
        .mul(OracleFraction::integer(64))
        .rem(OracleFraction::integer(QUIET_LOOP_FRAMES as u64));
    let whole = usize::try_from(relative.n.checked_div(relative.d).unwrap()).unwrap();
    let fraction = OracleFraction::new(relative.n.checked_rem(relative.d).unwrap(), relative.d)
        .rounded() as f32;
    let left = f32::from(declared_quiet_pcm(whole)) / 32768.0;
    let right = f32::from(declared_quiet_pcm((whole + 1) % QUIET_LOOP_FRAMES)) / 32768.0;
    [left + (right - left) * fraction; 2]
}

#[test]
fn retained_mapped_rate_two_cuts_keep_same_native_full_pcm_and_partial_block_pitch() {
    const FRAMES: usize = 24_593;
    for (piano, key_lock, shift, attack) in [
        (false, true, 0, false),
        (true, false, 7, true),
        (false, true, 7, true),
    ] {
        let mut partition_outputs = Vec::new();
        for partitions in [&[512][..], &[1, 127, 383, 17][..]] {
            let (mut path, _job) = quiet_original_path(attack);
            if key_lock {
                assert!(path.source.engine.set_pad_key_lock(0, true).unwrap() > 0);
                path.drain();
            }
            let birth = start(&mut path, shift, key_lock, piano);
            let execution = birth.execution();
            let mut parameters = Parameters::new(&mut path);
            let h = 2.0_f64.powf(f64::from(shift) / 12.0);
            let mut raw = mapped_fixed_pitch_birth_tests::RawStereo::new(h);
            let mut master = vec![Master {
                frame: 0,
                beat: 0.0,
                period: PERIOD,
            }];
            let mut actual = Vec::new();
            let mut expected = Vec::new();
            let mut offset = 0;
            let mut part = 0;
            let native = path.voice().stretch.native_state_address();
            // Cuts immediately before/on/after independent512-input heads, after real attack input.
            for (end, next_period) in [
                (4095, Some(PERIOD * 0.8)),
                (4608, Some(PERIOD * 1.25)),
                (5121, Some(PERIOD * 0.9)),
                (FRAMES, None),
            ] {
                while offset < end {
                    let frames = partitions[part % partitions.len()].min(end - offset);
                    let period = master.last().unwrap().period;
                    let ratio = OracleFraction::integer(64)
                        .div(OracleFraction::binary(f64::from(RATE) * period))
                        .rounded();
                    let pitch = if key_lock { h / ratio } else { h };
                    for n in offset..offset + frames {
                        expected.extend(raw.sample_at_pitch(
                            declared_input(&master, execution + n as u64, execution),
                            pitch,
                        ));
                    }
                    actual.extend(path.render(frames));
                    offset += frames;
                    part += 1;
                }
                if let Some(period) = next_period {
                    let old = path.voice().source_playback;
                    let before = path.voice().stretch.mapped_continuation_state_for_test();
                    let at = path.timeline.output_frame();
                    let beat = master_at(&master, at);
                    parameters.change(&mut path, PERIOD / period, period);
                    assert_eq!(
                        effect(&path).state,
                        RateState::Applied,
                        "{:?}",
                        diagnostic(&path)
                    );
                    assert_eq!(path.timeline.output_frame(), at);
                    assert_eq!(path.voice().source_playback.position(), old.position());
                    assert_eq!(
                        path.voice().source_playback.position().fraction.to_bits(),
                        old.position().fraction.to_bits()
                    );
                    assert_eq!(
                        path.voice().source_playback.original_scalar_domain(),
                        old.original_scalar_domain()
                    );
                    let after = path.voice().stretch.mapped_continuation_state_for_test();
                    assert!(before.same_storage(&after));
                    assert_eq!(after.revision, before.revision + 1);
                    assert_eq!(after.coordinates, before.coordinates);
                    master.push(Master {
                        frame: at,
                        beat,
                        period,
                    });
                }
                assert_eq!(path.voice().stretch.native_state_address(), native);
                let coordinates = path
                    .voice()
                    .stretch
                    .retained_mapped_output_coordinates()
                    .unwrap()
                    .unwrap();
                assert_eq!(coordinates.logical_output_frame, execution + offset as u64);
                assert_eq!(
                    coordinates.actual_input_frame,
                    coordinates.logical_output_frame
                );
                assert_eq!(
                    coordinates.raw_output_frame,
                    (raw.history - 511 + offset) as u64
                );
                assert_eq!(
                    sync_feedback(&path).unwrap()["phase_program_current"],
                    false
                );
                assert!(sync_feedback(&path).unwrap()["grid_phase_confirmed"].is_null());
            }
            assert_eq!(
                actual, expected,
                "continuous single Native, piano={piano} KL={key_lock} shift={shift}"
            );
            partition_outputs.push(actual);
        }
        assert_eq!(partition_outputs[0], partition_outputs[1]);
    }
}

#[test]
fn retained_mapped_rate_zero_input_cut_revokes_chunks_but_speed_only_keeps_epoch_and_stamp() {
    let (mut path, _job) = cohort(1, 7, true);
    let mut parameters = Parameters::new(&mut path);
    let index = path
        .source
        .mixer
        .voices
        .iter()
        .position(|voice| voice.active)
        .unwrap();
    let snapshot = path
        .source
        .mixer
        .rate_voice_snapshot_for_test(index)
        .unwrap();
    let chunk = path
        .voice()
        .stretch
        .mapped_chunk_for_test(snapshot.playback, snapshot.plan, 1)
        .unwrap();
    assert!(path.voice().stretch.mapped_chunk_current_for_test(&chunk));
    let before = path.voice().stretch.mapped_continuation_state_for_test();
    let birth_keys = path
        .voice()
        .stretch
        .mapped_projection_memo_keys_for_test()
        .unwrap()[0];
    assert!(birth_keys.iter().all(Option::is_some));
    let playback = path.voice().source_playback;
    path.source.engine.set_speed(0.8).unwrap();
    path.drain();
    parameters.drain(&mut path);
    path.render(0);
    assert_eq!(effect(&path).state, RateState::Applied);
    assert!(path.voice().source_playback.matches_exact(&playback));
    assert_eq!(
        path.voice().stretch.mapped_continuation_state_for_test(),
        before
    );
    assert!(path.voice().stretch.mapped_chunk_current_for_test(&chunk));
    parameters.change(&mut path, 0.8, PERIOD * 1.25);
    assert_eq!(
        effect(&path).state,
        RateState::Applied,
        "{:?}",
        diagnostic(&path)
    );
    assert!(!path.voice().stretch.mapped_chunk_current_for_test(&chunk));
    let after = path.voice().stretch.mapped_continuation_state_for_test();
    assert!(before.same_storage(&after));
    assert_eq!(after.revision, before.revision + 1);
    let snapshot = path
        .source
        .mixer
        .rate_voice_snapshot_for_test(index)
        .unwrap();
    let fresh = path
        .voice()
        .stretch
        .mapped_chunk_for_test(snapshot.playback, snapshot.plan, 1)
        .unwrap();
    assert!(path.voice().stretch.mapped_chunk_current_for_test(&fresh));
    let mut wrong = snapshot.playback;
    assert!(wrong.advance(1));
    assert!(
        path.voice()
            .stretch
            .mapped_chunk_for_test(wrong, snapshot.plan, 1)
            .is_none()
    );
    // A second real cut at the SAME output address must not reuse the first cut's
    // same-address Current result. Birth projection keys remain independently valid.
    let second_before = path.voice().stretch.mapped_continuation_state_for_test();
    let frame = path.timeline.output_frame();
    parameters.change(&mut path, 1.25, PERIOD * 0.8);
    assert_eq!(
        effect(&path).state,
        RateState::Applied,
        "{:?}",
        diagnostic(&path)
    );
    assert_eq!(path.timeline.output_frame(), frame);
    assert!(!path.voice().stretch.mapped_chunk_current_for_test(&fresh));
    let second_after = path.voice().stretch.mapped_continuation_state_for_test();
    assert!(second_before.same_storage(&second_after));
    assert_eq!(second_after.revision, second_before.revision + 1);
    let keys = path
        .voice()
        .stretch
        .mapped_projection_memo_keys_for_test()
        .unwrap();
    assert_eq!(keys[0], birth_keys);
    assert_eq!(keys[1][0], Some(frame));
    let snapshot = path
        .source
        .mixer
        .rate_voice_snapshot_for_test(index)
        .unwrap();
    assert!(
        path.voice()
            .stretch
            .mapped_chunk_for_test(snapshot.playback, snapshot.plan, 1)
            .is_some()
    );
}

#[test]
fn retained_mapped_rate_bank_b_never_supplies_old_a_source_authority() {
    let (mut actual, _job) = cohort(1, 7, true);
    let (mut reference, _other_job) = cohort(1, 7, true);
    let mut actual_parameters = Parameters::new(&mut actual);
    let mut reference_parameters = Parameters::new(&mut reference);
    let source = actual.voice().sample.as_ref().unwrap().source_address();
    let native = actual.voice().stretch.native_state_address();
    publish_distinct_cold_source(&mut actual, false);
    assert_ne!(binding(&actual.source, 0).binding.source_address, source);
    let request = actual.source.engine.set_pad_key_lock(0, false).unwrap();
    assert_ne!(request, 0);
    actual.drain();
    assert_eq!(key_lock_feedback(&actual)["request_id"], request);
    assert_eq!(key_lock_feedback(&actual)["ready"], false);
    for period in [PERIOD * 1.25, PERIOD * 0.8] {
        actual_parameters.change(&mut actual, PERIOD / period, period);
        reference_parameters.change(&mut reference, PERIOD / period, period);
        assert_eq!(
            effect(&actual).state,
            RateState::Applied,
            "{:?}",
            diagnostic(&actual)
        );
        assert_eq!(effect(&reference).state, RateState::Applied);
        assert_eq!(
            actual.voice().sample.as_ref().unwrap().source_address(),
            source
        );
        assert_eq!(actual.voice().stretch.native_state_address(), native);
        assert!(sync_feedback(&actual).is_none_or(|status| status["birth"].is_null()));
        assert_eq!(key_lock_feedback(&actual)["ready"], false);
        for frames in [1, 127, 383, 17, 512] {
            assert_eq!(actual.render(frames), reference.render(frames));
        }
    }
}

#[test]
fn retained_mapped_rate_late_b_serialization_change_refuses_captured_a_without_effects() {
    let (mut path, _job) = cohort(1, 7, true);
    publish_distinct_cold_source(&mut path, false);
    let mut parameters = Parameters::new(&mut path);
    path.source
        .engine
        .set_speed_and_master_period(0.8, PERIOD * 1.25)
        .unwrap();
    // Keep the genuine request pending until both original drains have settled.
    parameters.drain(&mut path);
    assert_eq!(effect(&path).state, RateState::Preparing);
    path.drain();
    let frame = path.timeline.output_frame();
    path.source
        .mixer
        .capture_pending_global_rate_for_test(frame)
        .unwrap();
    let before = path.voice().source_playback;
    let native = path.voice().stretch.mapped_continuation_state_for_test();
    let source = path
        .voice()
        .source_admission
        .as_ref()
        .unwrap()
        .retry_identity();
    let current_b = binding(&path.source, 0);
    let digest_b = path.source.engine.loaded_source_digests.lock().unwrap()[0].clone();
    publish_distinct_c_after_capture(&mut path);
    let current_c = binding(&path.source, 0);
    let placement_b = current_b.binding.placement.unwrap();
    let placement_c = current_c.binding.placement.unwrap();
    assert!(current_c.current());
    assert_ne!(
        path.source.engine.loaded_source_digests.lock().unwrap()[0],
        digest_b
    );
    assert_ne!(current_c.source_generation, current_b.source_generation);
    assert_ne!(
        current_c.binding.source_address,
        current_b.binding.source_address
    );
    assert_ne!(placement_c.content_key(), placement_b.content_key());
    assert!(placement_c.lifetime() > placement_b.lifetime());
    assert!(placement_c.binding_epoch() > placement_b.binding_epoch());
    assert_eq!(path.timeline.output_frame(), frame);
    assert!(path.voice().source_playback.matches_exact(&before));
    assert_eq!(
        path.voice().stretch.mapped_continuation_state_for_test(),
        native
    );
    assert_eq!(
        path.voice()
            .source_admission
            .as_ref()
            .unwrap()
            .retry_identity(),
        source
    );
    // C's legitimate publication owns its new feedback. The refused rate claim must
    // neither restore A/B feedback nor lend C any part of A's Native/readiness proof.
    let birth = sync_feedback(&path);
    assert!(
        birth
            .as_ref()
            .is_none_or(|status| status["birth"].is_null())
    );
    path.render(0);
    assert_eq!(effect(&path).state, RateState::Rejected);
    assert_eq!(effect(&path).error, RateError::SourceChanged);
    assert_eq!(path.timeline.master_period_seconds(), Some(PERIOD));
    assert_eq!(path.timeline.output_frame(), frame);
    assert!(path.voice().source_playback.matches_exact(&before));
    assert_eq!(
        path.voice().stretch.mapped_continuation_state_for_test(),
        native
    );
    assert_eq!(
        path.voice()
            .source_admission
            .as_ref()
            .unwrap()
            .retry_identity(),
        source
    );
    assert_eq!(sync_feedback(&path), birth);
}

fn publish_distinct_c_after_capture(path: &mut LaunchPath) {
    let mut bytes = std::fs::read(path.source.directory.path().join("external.wav")).unwrap();
    for sample in bytes[44..].chunks_exact_mut(2) {
        sample.copy_from_slice(&4096_i16.to_le_bytes());
    }
    let source_c = path.source.directory.path().join("distinct-c.wav");
    std::fs::write(&source_c, bytes).unwrap();
    assert!(path.source.consumer.is_empty());
    let request = crate::audio_engine::cold_load::admit_for_format_selected(
        &path.source.engine,
        0,
        source_c.to_string_lossy().into(),
        (false, false, false, None),
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
            "distinct C preparation"
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
        assert!(std::time::Instant::now() < deadline, "distinct C own ACK");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(binding(&path.source, 0).current());
}

#[test]
fn retained_mapped_rate_global_native_and_dry_claim_together_and_pending_piano_blocks_all() {
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
    render_to(&mut path, members[0].execution_frame().unwrap());
    path.render(17);
    assert_eq!(group.state(), "applied");
    assert_eq!(ticket.publication_status(), "accepted");
    let mut parameters = Parameters::new(&mut path);
    let first = path.voice().source_playback;
    parameters.change(&mut path, 0.8, PERIOD * 1.25);
    assert_eq!(
        effect(&path).state,
        RateState::Applied,
        "{:?}",
        diagnostic(&path)
    );
    assert_eq!(path.voice().source_playback.position(), first.position());
    assert_eq!(
        path.voice()
            .stretch
            .mapped_continuation_state_for_test()
            .revision,
        1
    );
    assert_eq!(
        path.source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        2
    );
    path.render(513);
    pitch_clock(&path);
    let pending = path
        .source
        .engine
        .capture_pad_pitch_action(0, "pitch.extra:7:set_retrigger", None)
        .unwrap();
    assert!(path.source.engine.publish_pitch_action(&pending).unwrap());
    path.drain();
    assert!(!pending.effective());
    let before: Vec<_> = path
        .source
        .mixer
        .voices
        .iter()
        .filter(|voice| voice.active)
        .map(|voice| (voice.generation, voice.source_playback))
        .collect();
    let native = path.voice().stretch.mapped_continuation_state_for_test();
    let old_period = path.timeline.master_period_seconds();
    parameters.change(&mut path, 1.0, PERIOD);
    assert_eq!(effect(&path).state, RateState::Rejected);
    assert_eq!(path.timeline.master_period_seconds(), old_period);
    for (voice, (generation, playback)) in path
        .source
        .mixer
        .voices
        .iter()
        .filter(|voice| voice.active)
        .zip(&before)
    {
        assert_eq!(voice.generation, *generation);
        assert!(voice.source_playback.matches_exact(playback));
    }
    assert_eq!(
        path.voice().stretch.mapped_continuation_state_for_test(),
        native
    );
    assert!(!pending.effective());
}

fn mixed_native_pair(keep: Option<usize>) -> (LaunchPath, OfflineAnalysisJob) {
    let pcm: Vec<i16> = (0..128)
        .map(|frame| ((frame * 1999) % 24001 - 12000) as i16)
        .collect();
    let mut source = ColdCustody::new_selected_with_signal(None, &pcm);
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            source.engine.input_clock,
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let mut path = LaunchPath::from_source(source);
    let (job, delivery) = covered_original_delivery(&path, false);
    publish_mixed_members(&path, &delivery);
    path.source
        .mixer
        .set_pad_pitch_intent(1, PitchIntent::new(7, 0).unwrap())
        .unwrap();
    for id in [0, 1] {
        assert!(path.source.engine.set_pad_key_lock(id, true).unwrap() > 0);
    }
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
    for member in &members {
        ready(&mut path, member.sequence());
    }
    let execution = members[0].execution_frame().unwrap();
    assert_eq!(members[1].execution_frame(), Some(execution));
    render_to(&mut path, execution);
    path.render(0);
    assert_eq!(group.state(), "applied", "{:?}", group.reason());
    assert_eq!(ticket.publication_status(), "accepted");
    for id in [0, 1] {
        let voice = path
            .source
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(id))
            .unwrap();
        assert_eq!(voice.source_playback.is_mapped(), id == 0);
        assert_ne!(voice.stretch.native_state_address(), 0);
        assert!(voice.stretch.owns_pitch_contract(
            PitchIntent::new(if id == 0 { 0 } else { 7 }, 0).unwrap(),
            true
        ));
    }
    if let Some(keep) = keep {
        // Each control owns an independently prepared public group. Stop only its unrelated
        // peer before the first input; never copy a DUT playback, FIFO or prepared candidate.
        path.source
            .mixer
            .voices
            .iter_mut()
            .find(|voice| voice.active && voice.sample_id != keep)
            .unwrap()
            .stop();
    }
    (path, job)
}

fn playing_lane(path: &LaunchPath, id: usize) -> usize {
    path.source
        .mixer
        .voices
        .iter()
        .position(|voice| voice.is_playing_sample(id))
        .unwrap()
}

#[test]
fn retained_mapped_rate_mixed_scalar_native_preserves_both_streams_through_two_cuts() {
    let (mut mixed, _job) = mixed_native_pair(None);
    let (mut mapped, _mapped_job) = mixed_native_pair(Some(0));
    let (mut scalar, _scalar_job) = mixed_native_pair(Some(1));
    assert_eq!(
        mixed.timeline.output_frame(),
        mapped.timeline.output_frame()
    );
    assert_eq!(
        mixed.timeline.output_frame(),
        scalar.timeline.output_frame()
    );
    let lanes = [playing_lane(&mixed, 0), playing_lane(&mixed, 1)];
    let addresses = lanes.map(|index| {
        mixed.source.mixer.voices[index]
            .stretch
            .native_state_address()
    });
    let mut parameters = Parameters::new(&mut mixed);
    let mut mapped_parameters = Parameters::new(&mut mapped);
    let mut scalar_parameters = Parameters::new(&mut scalar);
    for period in [None, Some(PERIOD * 1.25), Some(PERIOD / 1.16)] {
        if let Some(period) = period {
            let before = lanes.map(|index| {
                let voice = &mixed.source.mixer.voices[index];
                (
                    voice.source_playback,
                    voice.stretch.mapped_continuation_state_for_test(),
                )
            });
            let at = mixed.timeline.output_frame();
            parameters.change(&mut mixed, PERIOD / period, period);
            mapped_parameters.change(&mut mapped, PERIOD / period, period);
            scalar_parameters.change(&mut scalar, PERIOD / period, period);
            for path in [&mixed, &mapped, &scalar] {
                assert_eq!(
                    effect(path).state,
                    RateState::Applied,
                    "{:?}",
                    diagnostic(path)
                );
                assert_eq!(path.timeline.output_frame(), at);
            }
            for (index, (playback, state)) in lanes.into_iter().zip(&before) {
                let voice = &mixed.source.mixer.voices[index];
                assert_eq!(voice.source_playback.position(), playback.position());
                assert_eq!(
                    voice.source_playback.position().fraction.to_bits(),
                    playback.position().fraction.to_bits()
                );
                assert!(state.same_storage(&voice.stretch.mapped_continuation_state_for_test()));
            }
        }
        for frames in [17, 1, 127, 383, 512, 7, 509] {
            let output = mixed.render(frames);
            let mapped_output = mapped.render(frames);
            let scalar_output = scalar.render(frames);
            let expected: Vec<_> = mapped_output
                .iter()
                .zip(&scalar_output)
                .map(|(a, b)| a + b)
                .collect();
            assert_eq!(
                output, expected,
                "independently owned scalar and mapped Native streams"
            );
            for (id, reference) in [(0, &mapped), (1, &scalar)] {
                let actual = mixed
                    .source
                    .mixer
                    .rate_voice_snapshot_for_test(lanes[id])
                    .unwrap();
                let expected = reference
                    .source
                    .mixer
                    .rate_voice_snapshot_for_test(playing_lane(reference, id))
                    .unwrap();
                if id == 1 {
                    assert!(actual.playback.matches_exact(&expected.playback));
                    assert!(
                        actual
                            .actual_feed_checkpoint()
                            .matches_exact(expected.actual_feed_checkpoint())
                    );
                } else {
                    // Independent qualified maps retain their own identity markers.
                    // Compare exact coordinate/reader semantics, never substitute an owner.
                    assert_eq!(actual.playback.position(), expected.playback.position());
                    assert_eq!(
                        actual.playback.position().fraction.to_bits(),
                        expected.playback.position().fraction.to_bits()
                    );
                    assert_eq!(
                        actual.playback.tempo_ratio().to_bits(),
                        expected.playback.tempo_ratio().to_bits()
                    );
                    assert_eq!(
                        actual.playback.effective_domain(),
                        expected.playback.effective_domain()
                    );
                    let a = actual.actual_feed_checkpoint();
                    let b = expected.actual_feed_checkpoint();
                    assert_eq!(a.playback.position(), b.playback.position());
                    assert_eq!(
                        a.playback.position().fraction.to_bits(),
                        b.playback.position().fraction.to_bits()
                    );
                    assert!(a.plan.matches_exact(b.plan));
                }
                assert_eq!(
                    mixed.source.mixer.voices[lanes[id]]
                        .stretch
                        .native_state_address(),
                    addresses[id]
                );
            }
        }
    }
    assert_eq!(
        mixed
            .voice()
            .stretch
            .mapped_continuation_state_for_test()
            .revision,
        2
    );
}

#[test]
fn retained_mapped_rate_scalar_native_pause_or_owned_hold_refuses_entire_mixed_claim() {
    for held in [false, true] {
        let (mut path, _job) = mixed_native_pair(None);
        path.render(17);
        let scalar = playing_lane(&path, 1);
        let hold = if held {
            let action = Python::attach(|py| {
                let values = PyDict::new(py);
                values.set_item("press", 77_u64).unwrap();
                path.source
                    .engine
                    .pause_waveform_hold(1, &values.get_item("press").unwrap().unwrap(), None)
                    .unwrap()
            });
            path.drain();
            assert_eq!(
                action.action.state(),
                crate::audio_engine::hold_action::HoldActionState::PausedOwned
            );
            let key = action.action.key();
            let original = path
                .source
                .engine
                .input_runtime_ownership
                .slot_snapshot(1)
                .unwrap();
            assert_eq!(original.content_key(), Some(key));
            let pair = path.source.engine._capture_pad_pair(1, 2).unwrap();
            let ticket = crate::audio_engine::pad_pair::control::prepare(
                &path.source.engine,
                &pair,
                RATE,
                2,
            )
            .unwrap();
            path.source.engine._publish_pad_pair(&ticket).unwrap();
            path.drain();
            assert!(ticket.own_acknowledgement().is_some());
            assert!(path.source.engine._reconcile_pad_pair(&ticket).unwrap());
            let ack = ticket.own_acknowledgement().unwrap();
            path.source.engine._finish_pad_pair(&ticket, &ack).unwrap();
            assert_eq!(action.current_control_slot(), Some(2));
            assert_ne!(key.cell().index(), 2);
            assert_eq!(path.source.mixer.voices[scalar].sample_id, 2);
            assert!(path.source.mixer.rate_hold_active_for_key_for_test(key));
            // A comparison-only foreign incarnation cannot borrow this cell's real Hold.
            let foreign = crate::audio_engine::content_placement::ContentKey::new(
                original.owner(),
                key.cell(),
                key.lifetime().checked_add(1).unwrap(),
            )
            .unwrap();
            assert!(!path.source.mixer.rate_hold_active_for_key_for_test(foreign));
            let other = path
                .source
                .engine
                .input_runtime_ownership
                .slot_snapshot(0)
                .unwrap()
                .content_key()
                .unwrap();
            assert!(!path.source.mixer.rate_hold_active_for_key_for_test(other));
            Some(action)
        } else {
            path.source.mixer.voices[scalar].pause();
            None
        };
        path.render(0);
        assert!(path.source.mixer.voices[scalar].paused);
        let before: Vec<_> = path
            .source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .map(|voice| {
                (
                    voice.source_playback,
                    voice.stretch.mapped_continuation_state_for_test(),
                )
            })
            .collect();
        let sync = sync_feedback(&path);
        let key_lock = key_lock_feedback(&path);
        let frame = path.timeline.output_frame();
        let mut parameters = Parameters::new(&mut path);
        parameters.change(&mut path, 0.8, PERIOD * 1.25);
        assert_eq!(effect(&path).state, RateState::Rejected);
        assert_eq!(effect(&path).error, RateError::MappedTimingUnavailable);
        assert_eq!(path.timeline.master_period_seconds(), Some(PERIOD));
        assert_eq!(path.timeline.output_frame(), frame);
        assert_eq!(sync_feedback(&path), sync);
        assert_eq!(key_lock_feedback(&path), key_lock);
        for (voice, (playback, native)) in path
            .source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .zip(&before)
        {
            assert!(voice.source_playback.matches_exact(playback));
            assert_eq!(voice.stretch.mapped_continuation_state_for_test(), *native);
        }
        if let Some(hold) = hold {
            assert_eq!(
                hold.action.state(),
                crate::audio_engine::hold_action::HoldActionState::PausedOwned
            );
        }
    }
}

#[test]
fn retained_mapped_rate_requires_genuine_first_mix_and_keeps_old_audio_on_refusal() {
    for skip_first in [false, true] {
        let (mut path, _job) = quiet_original_path(false);
        let _birth = start(&mut path, 7, false, false);
        let mut parameters = Parameters::new(&mut path);
        let index = path
            .source
            .mixer
            .voices
            .iter()
            .position(|voice| voice.active)
            .unwrap();
        if skip_first {
            path.source.mixer.voices[index].pause();
            path.render(17);
            path.source.mixer.voices[index].resume();
        }
        let before = path.voice().source_playback;
        let native = path.voice().stretch.mapped_continuation_state_for_test();
        let old_period = path.timeline.master_period_seconds();
        parameters.change(&mut path, 0.8, PERIOD * 1.25);
        assert_eq!(effect(&path).state, RateState::Rejected);
        assert_eq!(path.timeline.master_period_seconds(), old_period);
        assert!(path.voice().source_playback.matches_exact(&before));
        assert_eq!(
            path.voice().stretch.mapped_continuation_state_for_test(),
            native
        );
        assert!(!path.voice().mapped_birth.unwrap().status.emitted);
    }
}

#[test]
fn retained_mapped_rate_all_32_last_counter_refuses_without_prefix_then_repairs() {
    let (mut path, _job) = cohort(32, 7, false);
    let mut parameters = Parameters::new(&mut path);
    let last = path
        .source
        .mixer
        .voices
        .iter()
        .rposition(|voice| voice.active)
        .unwrap();
    let previous_revision = path.source.mixer.voices[last]
        .stretch
        .mapped_continuation_state_for_test()
        .revision;
    path.source.mixer.voices[last]
        .stretch
        .set_mapped_continuation_revision_for_test(u64::MAX);
    let before: Vec<_> = path
        .source
        .mixer
        .voices
        .iter()
        .map(|voice| {
            (
                voice.source_playback,
                voice.generation,
                voice.stretch.mapped_continuation_state_for_test(),
                voice.source_admission.as_ref().unwrap().retry_identity(),
                voice.source_feed_history.continuity_revision(),
            )
        })
        .collect();
    let histories: Vec<_> = path
        .source
        .mixer
        .voices
        .iter()
        .map(|voice| {
            let mut snapshot = SourceFeedSnapshot::new();
            assert!(voice.source_feed_history.capture_into(&mut snapshot));
            snapshot
        })
        .collect();
    let sync = sync_feedback(&path);
    let key_lock = key_lock_feedback(&path);
    for voice in path.source.mixer.voices.iter().filter(|voice| voice.active) {
        let keys = voice
            .stretch
            .mapped_projection_memo_keys_for_test()
            .unwrap();
        assert!(keys[0].iter().any(Option::is_some));
        assert!(keys[1].iter().any(Option::is_some));
    }
    let frame = path.timeline.output_frame();
    let old_period = path.timeline.master_period_seconds();
    parameters.change(&mut path, 0.8, PERIOD * 1.25);
    assert_eq!(effect(&path).state, RateState::Rejected);
    assert_eq!(effect(&path).error, RateError::MappedTimingUnavailable);
    assert_eq!(path.timeline.master_period_seconds(), old_period);
    assert_eq!(path.timeline.output_frame(), frame);
    assert_eq!(sync_feedback(&path), sync);
    assert_eq!(key_lock_feedback(&path), key_lock);
    for ((voice, (playback, generation, native, source, continuity)), history) in
        path.source.mixer.voices.iter().zip(&before).zip(&histories)
    {
        assert!(voice.source_playback.matches_exact(playback));
        assert_eq!(voice.generation, *generation);
        assert_eq!(voice.stretch.mapped_continuation_state_for_test(), *native);
        assert_eq!(
            voice.source_admission.as_ref().unwrap().retry_identity(),
            *source
        );
        assert_eq!(voice.source_feed_history.continuity_revision(), *continuity);
        let mut after = SourceFeedSnapshot::new();
        assert!(voice.source_feed_history.capture_into(&mut after));
        assert_eq!(after.active_frames(), history.active_frames());
        assert_eq!(after.active_origin(), history.active_origin());
        assert_eq!(after.captured_epoch(), history.captured_epoch());
        assert_eq!(after.spans().len(), history.spans().len());
        for (actual, expected) in after.spans().zip(history.spans()) {
            assert!(actual.playback.matches_exact(&expected.playback));
            assert!(actual.plan.matches_exact(expected.plan));
            assert_eq!(
                (actual.active_start, actual.frames, actual.context),
                (expected.active_start, expected.frames, expected.context)
            );
        }
    }
    // Restore only the test-injected counter; source/admission/currentness remain genuine.
    path.source.mixer.voices[last]
        .stretch
        .set_mapped_continuation_revision_for_test(previous_revision);
    parameters.change(&mut path, 0.8, PERIOD * 1.25);
    assert_eq!(
        effect(&path).state,
        RateState::Applied,
        "{:?}",
        diagnostic(&path)
    );
    for voice in path.source.mixer.voices.iter().filter(|voice| voice.active) {
        assert_eq!(
            voice.stretch.mapped_continuation_state_for_test().revision,
            1
        );
    }
    path.render(513);
}

fn consume(path: &mut LaunchPath, parameters: &mut Parameters, output: &mut [f32; 1024]) {
    parameters.drain(path);
    let start = path.timeline.output_frame();
    render_scheduled_audio(
        &mut path.source.mixer,
        &mut path.scheduler,
        output,
        &mut [0.0; NUM_SAMPLES],
        start,
        2,
        &mut path.timeline,
        &mut path.feedback,
        &mut ImmediateAudioBufferRetirement,
    );
    path.timeline.advance_by_rendered_frames(512);
}

#[test]
fn retained_mapped_rate_claim_render_fit_two_mib_without_rust_requests() {
    std::thread::Builder::new().name("retained-mapped-rate-two-mib".into())
        .stack_size(2 * 1024 * 1024).spawn(|| {
            let (mut path, _job) = cohort(6, 7, true);
            let mut parameters = Parameters::new(&mut path);
            parameters.queue(&mut path, 0.8, PERIOD * 1.25);
            let mut output = [0.; 1024];
            let (_, allocations) = observe_current_thread(|| consume(&mut path, &mut parameters, &mut output));
            assert_eq!(allocations, ThreadAllocations::default());
            assert_eq!(effect(&path).state, RateState::Applied, "{:?}", diagnostic(&path));
            for voice in path.source.mixer.voices.iter().filter(|voice| voice.active) {
                assert_eq!(voice.stretch.mapped_continuation_state_for_test().revision, 1);
                assert!(!voice.source_playback.has_phase_program());
            }
            assert_eq!(std::mem::size_of::<SourcePlayback>(), 512);
            assert_eq!(std::mem::size_of::<[Option<SourcePlayback>; 32]>(), 16384);
            assert!(std::mem::size_of::<crate::audio_engine::prepared_native_history::MappedRateContinuation>() <= 64);
            assert!(std::mem::size_of::<[Option<crate::audio_engine::prepared_native_history::MappedRateContinuation>; 32]>() <= 2304);
            eprintln!("retained mapped rate: stage={} bytes, stage32={} bytes; copied playback512/Option32=16384",
                std::mem::size_of::<crate::audio_engine::prepared_native_history::MappedRateContinuation>(),
                std::mem::size_of::<[Option<crate::audio_engine::prepared_native_history::MappedRateContinuation>; 32]>());
            eprintln!("retained mapped rate boxed storage: NativeAdapterState={} MappedNativeInput={}",
                std::mem::size_of::<crate::audio_engine::prepared_native_history::NativeAdapterState>(),
                std::mem::size_of::<crate::audio_engine::prepared_native_history::MappedNativeInput>());
            let (slot, role) = crate::audio_engine::prepared_native_history::MappedNativeInput::projection_memo_layout_for_test();
            assert!(slot <= 752);
            assert!(role <= 1504);
            assert!(std::mem::size_of::<crate::audio_engine::prepared_native_history::MappedNativeInput>() <= 5992);
            eprintln!("numeric projection memo: slot={slot} role={role} total={} bytes; two Birth + two Current, old Box2984; copied playback unchanged", role * 2);
            let tap_bytes = std::mem::size_of::<[crate::audio_engine::source_reader::FractionalSourceTaps; 1024]>();
            assert_eq!(tap_bytes, 32768);
            eprintln!("ephemeral exact reader batch: 1024 initialized taps/{tap_bytes} bytes; no persistent reader authority");
        }).unwrap().join().unwrap();
}

#[test]
#[ignore = "descriptive consuming profile; Root runs serial Release after systems POST"]
fn retained_mapped_rate_consuming_profile_2_4_6_10() {
    for voices in [2, 4, 6, 10] {
        let (mut path, _job) = cohort(voices, 7, true);
        let mut parameters = Parameters::new(&mut path);
        let mut maximum = std::time::Duration::ZERO;
        let mut output = [0.; 1024];
        for pass in 0..4 {
            let period = if pass % 2 == 0 { PERIOD * 1.25 } else { PERIOD };
            parameters.queue(&mut path, PERIOD / period, period);
            let started = std::time::Instant::now();
            let (_, allocations) =
                observe_current_thread(|| consume(&mut path, &mut parameters, &mut output));
            let elapsed = started.elapsed();
            assert_eq!(allocations, ThreadAllocations::default());
            assert_eq!(
                effect(&path).state,
                RateState::Applied,
                "{:?}",
                diagnostic(&path)
            );
            if pass != 0 {
                maximum = maximum.max(elapsed);
            }
        }
        eprintln!(
            "retained_native_rate voices={voices} actual_E_claim_plus512 max_ms={:.4} nominal_ms={:.4}; one warm/three repeats; no Rust requests; descriptive, no device guarantee",
            maximum.as_secs_f64() * 1000.,
            512. / f64::from(RATE) * 1000.
        );
    }
}

struct ProbeOwnerBaseline {
    native: (u64, usize, u64),
    admission: crate::audio_engine::native_history_permit::NativeSourceAdmissionIdentity,
    birth: crate::audio_engine::mapped_birth::BirthStatus,
}

fn probe_owner_baselines(path: &LaunchPath, voices: usize) -> Vec<ProbeOwnerBaseline> {
    assert_eq!(
        path.source
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        voices
    );
    (0..voices)
        .map(|id| {
            let voice = &path.source.mixer.voices[playing_lane(path, id)];
            ProbeOwnerBaseline {
                native: voice
                    .stretch
                    .mapped_continuation_state_for_test()
                    .own_identity()
                    .unwrap(),
                admission: voice.source_admission.as_ref().unwrap().retry_identity(),
                birth: voice.mapped_birth.unwrap().status,
            }
        })
        .collect()
}

fn assert_paired_probe_state(
    combined: &LaunchPath,
    split: &LaunchPath,
    combined_owners: &[ProbeOwnerBaseline],
    split_owners: &[ProbeOwnerBaseline],
) {
    use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
    assert_eq!(
        combined.timeline.output_frame(),
        split.timeline.output_frame()
    );
    assert_eq!(combined_owners.len(), split_owners.len());
    for id in 0..combined_owners.len() {
        let a = &combined.source.mixer.voices[playing_lane(combined, id)];
        let b = &split.source.mixer.voices[playing_lane(split, id)];
        let left = a.stretch.mapped_continuation_state_for_test();
        let right = b.stretch.mapped_continuation_state_for_test();
        assert_ne!(left.own_identity(), right.own_identity());
        for (path, voice, state, owner) in [
            (combined, a, &left, &combined_owners[id]),
            (split, b, &right, &split_owners[id]),
        ] {
            assert!(binding(&path.source, id).current());
            assert!(voice.stretch.has_effective_mapped_voice_for_test());
            assert_eq!(state.own_identity(), Some(owner.native));
            assert_eq!(
                voice.source_admission.as_ref().unwrap().retry_identity(),
                owner.admission
            );
            assert_eq!(voice.mapped_birth.unwrap().status, owner.birth);
            assert!(owner.birth.emitted);
            assert_eq!(
                state.effective_tuple(),
                Some((PitchIntent::new(7, 0).unwrap(), true))
            );
            let coordinates = state.coordinates.unwrap();
            assert_eq!(
                coordinates.logical_output_frame,
                path.timeline.output_frame()
            );
            assert_eq!(
                coordinates.actual_input_frame,
                coordinates.logical_output_frame
            );
        }
        // Immutable T/E, actual tuple and consumed-input/FIFO chronology must already
        // agree before measurement; no clock or foreign owner is copied to make them agree.
        assert_eq!(combined_owners[id].birth, split_owners[id].birth);
        assert!(
            left.same_stream_state(&right),
            "independent PAD{id} stream state differs"
        );
        assert_eq!(a.source_playback.position(), b.source_playback.position());
        assert_eq!(
            a.source_playback.position().fraction.to_bits(),
            b.source_playback.position().fraction.to_bits()
        );
        assert_eq!(
            a.source_playback.tempo_ratio().to_bits(),
            b.source_playback.tempo_ratio().to_bits()
        );
        assert_eq!(
            a.source_playback.effective_domain(),
            b.source_playback.effective_domain()
        );
        assert_eq!(
            a.source_playback.original_scalar_domain(),
            b.source_playback.original_scalar_domain()
        );
        assert_eq!(
            a.source_feed_history.continuity_revision(),
            b.source_feed_history.continuity_revision()
        );
        let mut ah = SourceFeedSnapshot::new();
        let mut bh = SourceFeedSnapshot::new();
        assert!(a.source_feed_history.capture_into(&mut ah));
        assert!(b.source_feed_history.capture_into(&mut bh));
        assert_eq!(
            (ah.active_origin(), ah.active_frames(), ah.captured_epoch()),
            (bh.active_origin(), bh.active_frames(), bh.captured_epoch())
        );
    }
}

fn measure_mapped_component(
    operation: impl FnOnce(),
) -> (
    std::time::Duration,
    crate::audio_engine::prepared_native_history::mapped_rate_probe::Snapshot,
) {
    use crate::audio_engine::prepared_native_history::mapped_rate_probe;
    mapped_rate_probe::start();
    let started = std::time::Instant::now();
    let (_, allocations) = observe_current_thread(operation);
    let elapsed = started.elapsed();
    let components = mapped_rate_probe::finish();
    assert_eq!(allocations, ThreadAllocations::default());
    (elapsed, components)
}

#[test]
#[ignore = "paired component diagnosis; Root runs serial Release after systems POST"]
fn retained_mapped_rate_paired_component_profile_2_4_6_10() {
    use crate::audio_engine::prepared_native_history::mapped_rate_probe;
    use std::time::{Duration, Instant};

    // Fixed TLS observer entry/read/exit control, probes disabled. No nested observer.
    let started = Instant::now();
    std::hint::black_box(());
    let empty_bare = started.elapsed();
    let started = Instant::now();
    let (_, allocations) = observe_current_thread(|| std::hint::black_box(()));
    let empty_observed = started.elapsed();
    assert_eq!(allocations, ThreadAllocations::default());
    eprintln!(
        "allocation observer empty control: bare_ns={} observed_ns={}; descriptive single samples",
        empty_bare.as_nanos(),
        empty_observed.as_nanos()
    );

    for voices in [2, 4, 6, 10] {
        let (mut combined, _combined_jobs) = cohort(voices, 7, true);
        let (mut split, _split_jobs) = cohort(voices, 7, true);
        let combined_owners = probe_owner_baselines(&combined, voices);
        let split_owners = probe_owner_baselines(&split, voices);
        assert_paired_probe_state(&combined, &split, &combined_owners, &split_owners);
        let mut combined_parameters = Parameters::new(&mut combined);
        let mut split_parameters = Parameters::new(&mut split);
        let mut reference = [0.; 1024];
        let mut actual = [0.; 1024];
        let mut samples = [[(Duration::ZERO, mapped_rate_probe::Snapshot::default()); 3]; 4];
        let mut combined_times = [Duration::ZERO; 4];
        for pass in 0..4 {
            let period = if pass % 2 == 0 { PERIOD * 1.25 } else { PERIOD };
            combined_parameters.queue(&mut combined, PERIOD / period, period);
            split_parameters.queue(&mut split, PERIOD / period, period);
            assert_paired_probe_state(&combined, &split, &combined_owners, &split_owners);
            let execution = split.timeline.output_frame();
            // The independent control performs the unchanged combined claim+512 path.
            // Probes are disabled; Native/FIFO owners are never cloned or reset.
            let started = Instant::now();
            let (_, allocations) = observe_current_thread(|| {
                consume(&mut combined, &mut combined_parameters, &mut reference)
            });
            combined_times[pass] = started.elapsed();
            assert_eq!(allocations, ThreadAllocations::default());
            samples[pass][0] = measure_mapped_component(|| {
                split_parameters.drain(&mut split);
                split.render(0);
            });
            assert_eq!(split.timeline.output_frame(), execution);
            assert_eq!(
                effect(&split).state,
                RateState::Applied,
                "{:?}",
                diagnostic(&split)
            );
            samples[pass][1] = measure_mapped_component(|| {
                consume(&mut split, &mut split_parameters, &mut actual)
            });
            assert!(
                samples[pass][0].1.buckets[mapped_rate_probe::Stage::Projection as usize].calls > 0
            );
            for stage in [
                mapped_rate_probe::Stage::ReaderFill,
                mapped_rate_probe::Stage::NativePitch,
                mapped_rate_probe::Stage::NativeShift,
            ] {
                assert_eq!(samples[pass][0].1.buckets[stage as usize].calls, 0);
            }
            for stage in [
                mapped_rate_probe::Stage::Projection,
                mapped_rate_probe::Stage::Canonicalize,
                mapped_rate_probe::Stage::EndpointProof,
                mapped_rate_probe::Stage::ReaderFill,
                mapped_rate_probe::Stage::NativeShift,
            ] {
                assert!(samples[pass][1].1.buckets[stage as usize].calls > 0);
            }
            assert_eq!(actual, reference);
            assert_paired_probe_state(&combined, &split, &combined_owners, &split_owners);

            // Both now begin the continuing interval at the same post-claim state.
            consume(&mut combined, &mut combined_parameters, &mut reference);
            samples[pass][2] = measure_mapped_component(|| {
                consume(&mut split, &mut split_parameters, &mut actual)
            });
            assert_eq!(actual, reference);
            assert_paired_probe_state(&combined, &split, &combined_owners, &split_owners);
        }

        // Matched real-render observer control with all six probes disabled. Each side
        // starts from the exact same semantic stream state and retains its own owner.
        let started = Instant::now();
        consume(&mut combined, &mut combined_parameters, &mut reference);
        let bare_render = started.elapsed();
        let started = Instant::now();
        let (_, allocations) =
            observe_current_thread(|| consume(&mut split, &mut split_parameters, &mut actual));
        let observed_render = started.elapsed();
        assert_eq!(allocations, ThreadAllocations::default());
        assert_eq!(actual, reference);
        assert_paired_probe_state(&combined, &split, &combined_owners, &split_owners);

        eprintln!(
            "retained_mapped_components voices={voices}; one warm/three repeats; six buckets INCLUSIVE and NON-ADDITIVE; no Rust requests; preparation/state comparisons outside measurement"
        );
        for (pass, phases) in samples.iter().enumerate() {
            eprintln!(
                "pass={pass} combined_claim_plus512_ms={:.4}",
                combined_times[pass].as_secs_f64() * 1000.
            );
            for (name, (elapsed, snapshot)) in [
                "claim_E_zero_output",
                "installed512_same_E",
                "continuing512",
            ]
            .into_iter()
            .zip(phases)
            {
                eprintln!(
                    "pass={pass} phase={name} total_ms={:.4}",
                    elapsed.as_secs_f64() * 1000.
                );
                eprintln!(
                    "  projection memo: hits={} misses={}",
                    snapshot.projection_hits, snapshot.projection_misses
                );
                for (bucket_name, bucket) in [
                    "projection",
                    "canonicalize",
                    "endpoint_proof",
                    "reader_fill",
                    "native_pitch",
                    "native_shift",
                ]
                .into_iter()
                .zip(snapshot.buckets)
                {
                    eprintln!(
                        "  {bucket_name}: calls={} inclusive_ms={:.4}",
                        bucket.calls,
                        bucket.nanos as f64 / 1_000_000.
                    );
                }
            }
        }
        eprintln!(
            "voices={voices} matched probes-disabled steady512 allocation-observer control: bare_ms={:.4} observed_ms={:.4}; descriptive, not a subtraction-based cost proof",
            bare_render.as_secs_f64() * 1000.,
            observed_render.as_secs_f64() * 1000.
        );
    }
}
