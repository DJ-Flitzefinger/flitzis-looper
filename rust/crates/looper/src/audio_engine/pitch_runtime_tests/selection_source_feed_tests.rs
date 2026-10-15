//! Held selection past and an active F ramp replay through the public SET path.
//! Expected PCM uses scalar source/ramp arithmetic and the independent direct C ABI.

use super::*;
use crate::audio_engine::prepared_source::PreparedSourcePermit;
use crate::messages::{PreparedStemSet, STEM_BUFFER_COUNT, STEM_COMPONENT_MASK, StemMixMode};

const HASH: u64 = 42;
const PARTITIONS: [usize; 11] = [1, 73, 511, 127, 512, 379, 512, 512, 512, 512, 446];

/// Initial publication is the existing source-bound fixture seam. Every selection,
/// history capture, request, preparation and adoption afterwards is the real path.
pub(in crate::audio_engine) fn selection_setup(
    lock: bool,
    pitch: PitchIntent,
) -> (Fixture, SampleBuffer, PreparedStemSet, u64) {
    let mut fixture = Fixture::new(lock);
    let complete = synthetic_source();
    let source = complete
        .clone()
        .window(START, END, 2, ResidentContext::KeyLockFiniteLoop)
        .unwrap();
    publish_tone(&fixture.control, 0, source.clone(), true);
    fixture.mixer.load_sample(0, source.clone());
    fixture.mixer.set_pad_loop_region(
        0,
        START as f64 / f64::from(RATE),
        Some(END as f64 / f64::from(RATE)),
    );
    fixture.mixer.set_pad_bpm(0, Some(120.0));
    let binding = input_runtime_binding::capture(&fixture.control.engine, 0)
        .unwrap()
        .unwrap();
    let mut loaded = vec![false; NUM_SAMPLES];
    let mut starts = vec![0.0; NUM_SAMPLES];
    let mut ends = vec![None; NUM_SAMPLES];
    let mut bindings = vec![None; NUM_SAMPLES];
    let mut rows = vec![PitchPadProjection::default(); NUM_SAMPLES];
    loaded[0] = true;
    starts[0] = START as f64 / f64::from(RATE);
    ends[0] = Some(END as f64 / f64::from(RATE));
    bindings[0] = Some(binding);
    rows[0].key_lock = lock;
    rows[0].intent = PitchIntent::new(pitch.base_shift(), 0).unwrap();
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
    let binding = input_runtime_binding::capture(&fixture.control.engine, 0)
        .unwrap()
        .unwrap();
    // The control resolver deliberately returns runtime_revision=0; the runtime
    // row above already published its own actual revision. Bind that current
    // scalar without changing source/timing authority or acknowledging anything.
    let ownership = &fixture.control.engine.input_runtime_ownership;
    let binding = input_runtime_binding::InputPadBinding {
        runtime_revision: ownership.runtime_revision(0),
        ..binding.binding
    };
    assert!(ownership.authority_current(0, binding));
    assert!(ownership.binding_window_current(0, binding));
    assert!(ownership.current(0, binding));
    assert_eq!(
        fixture
            .control
            .engine
            .current_timing_acknowledgements
            .current_for_binding(ownership, 0, binding),
        binding
            .accepted
            .map_or(0, |accepted| accepted.publication_epoch)
    );
    let publication = PreparedSourcePermit::for_native_binding_for_test(
        0,
        binding,
        fixture.control.engine.input_runtime_ownership.clone(),
        fixture
            .control
            .engine
            .current_timing_acknowledgements
            .clone(),
        fixture
            .control
            .engine
            .prepared_source_epoch(0)
            .unwrap()
            .clone(),
    )
    .unwrap();
    let stems = PreparedStemSet {
        complete_set_identity: Arc::new([0x5a; 32]),
        accepted_timing: None,
        reference_samples: complete.samples.clone(),
        reference_residency: complete.residency.clone(),
        publication,
        source_version_hash: HASH,
        sample_rate_hz: RATE,
        channels: 1,
        frame_count: EXTENT,
        available_mask: (1 << STEM_BUFFER_COUNT) - 1,
        stems: std::array::from_fn(|component| SampleBuffer {
            channels: 1,
            residency: None,
            samples: (0..EXTENT)
                .map(|frame| {
                    let time = frame as f64 / f64::from(RATE);
                    (0.11
                        * (std::f64::consts::TAU
                            * ((211.0 + 97.0 * component as f64) * time
                                + (3.0 + component as f64) * time * time))
                            .sin()
                        + 0.03
                            * (std::f64::consts::TAU * (617.0 + 73.0 * component as f64) * time)
                                .cos()) as f32
                })
                .collect::<Vec<_>>()
                .into(),
        }),
    }
    .window_for(&source)
    .unwrap();
    assert!(fixture.mixer.publish_prepared_stems(0, stems.clone()));
    assert_eq!(stems.publication.status(), "accepted");
    let start = fixture.capture(
        0,
        &format!("pitch.extra:{}:set_retrigger", pitch.extra_shift()),
    );
    fixture.publish(&start);
    fixture.wait_worker(&[&start]);
    fixture.render_to(start.execution_target());
    fixture.render(1);
    assert!(start.effective());
    let output_origin = fixture.transport.output_frame();
    assert!(fixture.mixer.seek_sample_at_output_frame(
        0,
        START as f64 / f64::from(RATE),
        output_origin,
    ));
    (fixture, source, stems, output_origin)
}

/// Binary64 scalar rate epochs, with the finite physical loop entered at START.
fn normal_chronology(frames: usize) -> Vec<ScalarFrame> {
    let mut origin_frame = START;
    let mut origin_fraction = 0.0;
    let mut epoch = 0;
    let mut rate: f64 = 1.0;
    let mut target: f64 = 1.0;
    let mut next_step = usize::MAX;
    let mut result = Vec::with_capacity(frames + 1);
    for active in 0..=frames {
        if let Some((_, value)) = EVENTS.iter().find(|(at, _)| *at == active) {
            target = *value;
            next_step = active;
        }
        if active == next_step && rate != target {
            let distance = origin_fraction + (active - epoch) as f64 * rate;
            let whole = distance.floor() as usize;
            origin_frame = START + (origin_frame - START + whole) % (END - START);
            origin_fraction = distance - whole as f64;
            epoch = active;
            rate = if (target - rate).abs() <= 0.05 {
                target
            } else {
                rate + (target - rate).clamp(-0.05, 0.05)
            };
            next_step = active + BLOCK;
        }
        let distance = origin_fraction + (active - epoch) as f64 * rate;
        let whole = distance.floor() as usize;
        result.push(ScalarFrame {
            frame: START + (origin_frame - START + whole) % (END - START),
            fraction: distance - whole as f64,
            passage: Passage::Normal,
            rate,
        });
    }
    result
}

#[derive(Clone, Copy, Debug)]
enum Selection {
    Full,
    Stems(u8),
}

#[derive(Clone, Copy, Debug)]
enum History {
    FullToStems,
    StemsToFull,
    Mask,
    Empty,
}

impl History {
    fn events(self) -> [(usize, Selection); 5] {
        let final_selection = match self {
            Self::FullToStems | Self::Mask => Selection::Stems(0b1010),
            Self::StemsToFull => Selection::Full,
            Self::Empty => Selection::Stems(0),
        };
        [
            (F - 5_200, Selection::Stems(STEM_COMPONENT_MASK)),
            (F - 4_000, Selection::Stems(0b0101)),
            (F - 2_800, Selection::Stems(0)),
            (
                F - 1_600,
                if matches!(self, Self::FullToStems | Self::Empty) {
                    Selection::Full
                } else {
                    Selection::Stems(0b0101)
                },
            ),
            (F - 43, final_selection),
        ]
    }
}

fn select(fixture: &mut Fixture, selection: Selection) {
    match selection {
        Selection::Full => assert!(fixture.mixer.set_stem_mix_mode(0, StemMixMode::FullMix, 0)),
        Selection::Stems(mask) => {
            assert!(fixture.mixer.set_stem_enabled_mask(0, mask, HASH));
            assert!(
                fixture
                    .mixer
                    .set_stem_mix_mode(0, StemMixMode::AllStems, HASH)
            );
        }
    }
}

fn run_history(fixture: &mut Fixture, origin: u64, history: History) {
    let events = history.events();
    let mut controls: Vec<_> = EVENTS
        .iter()
        .map(|(at, _)| *at)
        .chain(events.iter().map(|(at, _)| *at))
        .collect();
    controls.sort_unstable();
    for active in controls {
        fixture.render_to(origin + active as u64);
        if let Some((_, rate)) = EVENTS.iter().find(|(at, _)| *at == active) {
            if active == EVENTS[0].0 {
                fixture.mixer.set_speed(*rate);
            } else {
                if active == EVENTS[1].0 {
                    fixture.mixer.set_bpm_lock(true);
                }
                fixture.mixer.set_master_period(0.5 / rate);
            }
        }
        if let Some((_, selection)) = events.iter().find(|(at, _)| *at == active) {
            select(fixture, *selection);
        }
    }
    fixture.render_to(origin + F as u64);
}

fn selected_tap(
    source: &SampleBuffer,
    stems: &PreparedStemSet,
    at: usize,
    select: Selection,
) -> f32 {
    let resident = at - source.resident_start();
    match select {
        Selection::Full => source.samples[resident],
        Selection::Stems(mask) => stems
            .stems
            .iter()
            .enumerate()
            .filter(|(component, _)| mask & (1 << component) != 0)
            .map(|(_, stem)| stem.samples[resident])
            .sum(),
    }
}

/// Each ramp consumes 128 actual source frames, including fractional/rate epochs.
/// Interpolate the two already mixed taps, independently of production readers.
fn selected_pcm(
    timeline: &[ScalarFrame],
    active: usize,
    source: &SampleBuffer,
    stems: &PreparedStemSet,
    events: &[(usize, Selection)],
) -> f32 {
    let mut from = Selection::Full;
    let mut current = Selection::Full;
    let mut ramp_start = None;
    for &(at, next) in events.iter().take_while(|(at, _)| *at <= active) {
        from = current;
        current = next;
        ramp_start = Some(at);
    }
    let progress = ramp_start.map_or(128.0, |start| {
        let mut elapsed = 0.0;
        for frame in &timeline[start..active] {
            elapsed += frame.rate;
            if elapsed >= 128.0 {
                return 128.0;
            }
        }
        elapsed
    });
    let to_gain = progress as f32 / 128.0;
    let from_gain = 1.0 - to_gain;
    let tap = |at| {
        if progress < 128.0 {
            selected_tap(source, stems, at, from) * from_gain
                + selected_tap(source, stems, at, current) * to_gain
        } else {
            selected_tap(source, stems, at, current)
        }
    };
    let frame = timeline[active];
    let right = if frame.frame + 1 == END {
        START
    } else {
        frame.frame + 1
    };
    let left = tap(frame.frame);
    left + (tap(right) - left) * frame.fraction as f32
}

fn native_pitch(frame: ScalarFrame, pitch: PitchIntent, lock: bool) -> f64 {
    2.0_f64.powf(f64::from(pitch.total_semitones()) / 12.0) / if lock { frame.rate } else { 1.0 }
}

fn reset_origin(timeline: &[ScalarFrame], end: usize, pitch: PitchIntent, lock: bool) -> usize {
    let mut cache = BTreeMap::new();
    let mut bound = RawRubberBand::history_bound(native_pitch(timeline[end], pitch, lock));
    loop {
        let origin = (end - bound) / BLOCK * BLOCK;
        let mut required = bound;
        for &frame in &timeline[origin..end] {
            let native = native_pitch(frame, pitch, lock);
            required = required.max(
                *cache
                    .entry(native.to_bits())
                    .or_insert_with(|| RawRubberBand::history_bound(native)),
            );
        }
        if required == bound {
            return origin;
        }
        bound = required;
    }
}

#[test]
fn held_selection_set_replays_real_past_and_active_f_ramp_at_original_settled_e() {
    for (history, lock, pitch) in [
        (History::FullToStems, true, PitchIntent::new(6, 18).unwrap()),
        (
            History::StemsToFull,
            true,
            PitchIntent::new(-5, -18).unwrap(),
        ),
        (History::Mask, false, PitchIntent::new(6, 18).unwrap()),
        (History::Empty, false, PitchIntent::new(-5, -18).unwrap()),
    ] {
        let (mut fixture, source, stems, output_origin) =
            selection_setup(lock, PitchIntent::default());
        run_history(&mut fixture, output_origin, history);
        let timeline = normal_chronology(E + 4_097);
        assert_phase(&fixture, timeline[F]);
        let index = fixture
            .mixer
            .voices
            .iter()
            .position(|voice| voice.active && voice.sample_id == 0)
            .unwrap();
        let captured = fixture.mixer.capture_pitch_voice_snapshot(index).unwrap();
        assert!(captured.plan.transition.is_active());
        assert!(Arc::ptr_eq(
            &captured.stems.as_ref().unwrap().complete_set_identity,
            &stems.complete_set_identity
        ));
        let generation = fixture.voice(0).generation;
        let source_address = fixture.voice(0).sample.as_ref().unwrap().source_address();
        let starts = fixture.starts(0);
        fixture.rows(PitchPadProjection {
            intent: PitchIntent::new(pitch.base_shift(), 0).unwrap(),
            key_lock: lock,
            quantize_step_64ths: Some(16),
            ..PitchPadProjection::default()
        });
        fixture.refresh_clock();
        let mut clock = fixture.clock.read().unwrap();
        clock.downbeat_frame = output_origin;
        clock.master_beat = F as f64 / 24_000.0;
        fixture.clock.publish(clock);
        let action = fixture
            .control
            .engine
            .capture_pad_pitch_action(0, &format!("pitch.extra:{}:set", pitch.extra_shift()), None)
            .unwrap();
        let original_t = action.original_due_frame();
        let sequence = action.sequence();
        let timestamp = action.received_timestamp();
        let content = action.content_key();
        assert_eq!(original_t, output_origin + E as u64);
        fixture.publish(&action);
        assert_eq!(action.execution_target(), original_t);
        fixture.wait_worker(&[&action]);
        assert_eq!(fixture.transport.output_frame(), output_origin + F as u64);

        let origin = reset_origin(&timeline, E, pitch, lock);
        assert!(origin < history.events()[0].0);
        let first_pitch = native_pitch(timeline[origin + BLOCK - 1], pitch, lock);
        let mut raw = RawRubberBand::new(first_pitch);
        let mut wrong_fullmix = RawRubberBand::new(first_pitch);
        let events = history.events();
        for active in origin..E {
            let native = native_pitch(timeline[active], pitch, lock);
            raw.sample(
                selected_pcm(&timeline, active, &source, &stems, &events),
                native,
            );
            wrong_fullmix.sample(
                selected_pcm(&timeline, active, &source, &stems, &[]),
                native,
            );
        }
        assert_eq!(raw.fifo(), wrong_fullmix.fifo());
        fixture.render_to(original_t);
        assert_phase(&fixture, timeline[E]);
        let endpoint = fixture.mixer.capture_pitch_voice_snapshot(index).unwrap();
        assert!(!endpoint.plan.transition.is_active());
        if matches!(history, History::StemsToFull) {
            assert_eq!(endpoint.plan.selection.required_stem_hash(), None);
            assert!(
                endpoint.stems.is_some(),
                "FullMix E retains the historical pin"
            );
            let readers = endpoint.readers().unwrap();
            assert!(readers.current.is_none() && readers.outgoing.is_none());
        }
        let mut maximum = 0.0_f32;
        let mut wrong_maximum = 0.0_f32;
        let mut native_address = None;
        let mut rendered = 0;
        for frames in PARTITIONS {
            let actual = fixture.render(frames);
            assert!(
                action.effective(),
                "{} {:?}",
                action.state(),
                action.reason()
            );
            let address = fixture.voice(0).stretch.native_state_address();
            assert_eq!(*native_address.get_or_insert(address), address);
            for (offset, &sample) in actual.iter().enumerate() {
                let active = E + rendered + offset;
                let input = selected_pcm(&timeline, active, &source, &stems, &events);
                let native = native_pitch(timeline[active], pitch, lock);
                let expected = raw.sample(input, native);
                let wrong = wrong_fullmix.sample(input, native);
                let error = (sample - expected).abs();
                maximum = maximum.max(error);
                wrong_maximum = wrong_maximum.max((sample - wrong).abs());
                assert!(
                    error < 2.0e-6,
                    "{history:?} Lock={lock} total={} adopted_offset={} actual={sample} independent_A_reset={expected} error={error}",
                    pitch.total_semitones(),
                    rendered + offset,
                );
            }
            rendered += frames;
            assert_phase(&fixture, timeline[E + rendered]);
            assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), raw.fifo());
            assert_eq!(fixture.voice(0).generation, generation);
            assert_eq!(
                fixture.voice(0).sample.as_ref().unwrap().source_address(),
                source_address
            );
            assert_eq!(
                fixture.voice(0).stretch.adopted_request_id(),
                Some(sequence)
            );
            assert_eq!(fixture.starts(0), starts);
            assert_eq!(fixture.mixer.pad_pitch_intent(0), Some(pitch));
            assert_eq!(action.original_due_frame(), original_t);
            assert_eq!(action.execution_target(), original_t);
            assert_eq!(action.sequence(), sequence);
            assert_eq!(action.received_timestamp(), timestamp);
            assert_eq!(action.content_key(), content);
        }
        assert_eq!(rendered, 4_097);
        assert!(
            wrong_maximum > 1.0e-5,
            "the actual stem past must be indispensable: {history:?}"
        );
        assert!(fixture.scheduler.is_empty());
        println!(
            "held_selection_SET {history:?} Lock={lock} total={} O={output_origin} A={origin} F={F} E={E} samples={rendered} max_abs={maximum} wrong_FullMix_only_past={wrong_maximum}",
            pitch.total_semitones()
        );
    }
}

fn identical_old_to(fixture: &mut Fixture, baseline: &mut Fixture, target: u64) {
    assert_eq!(
        fixture.transport.output_frame(),
        baseline.transport.output_frame()
    );
    while fixture.transport.output_frame() < target {
        let frame = fixture.transport.output_frame();
        let mut stop = target.min(frame + 379);
        for due in [
            fixture.scheduler.peek_next_target_frame(),
            baseline.scheduler.peek_next_target_frame(),
        ]
        .into_iter()
        .flatten()
        {
            if due > frame {
                stop = stop.min(due);
            }
        }
        let frames = (stop - frame) as usize;
        assert_eq!(fixture.render(frames), baseline.render(frames));
        assert!(
            fixture
                .voice(0)
                .source_playback
                .matches_exact(&baseline.voice(0).source_playback)
        );
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            baseline.voice(0).stretch.pending_fifo_frames()
        );
    }
}

#[test]
fn held_selection_set_actual_aba_refuses_old_original_then_same_worker_retries() {
    // Dry zero pitch isolates the old voice from independently timed automatic Native work.
    let (mut fixture, source, stems, output_origin) =
        selection_setup(false, PitchIntent::default());
    let (mut baseline, _, _, baseline_origin) = selection_setup(false, PitchIntent::default());
    assert_eq!(baseline_origin, output_origin);
    run_history(&mut fixture, output_origin, History::FullToStems);
    run_history(&mut baseline, baseline_origin, History::FullToStems);
    let runtime = fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize;
    let generation = fixture.voice(0).generation;
    let source_address = fixture.voice(0).sample.as_ref().unwrap().source_address();
    let native_address = fixture.voice(0).stretch.native_state_address();
    let starts = fixture.starts(0);
    fixture.rows(PitchPadProjection {
        intent: PitchIntent::new(6, 0).unwrap(),
        key_lock: false,
        ..PitchPadProjection::default()
    });
    let old = fixture.capture(0, "pitch.extra:18:set");
    let old_t = old.original_due_frame();
    let old_sequence = old.sequence();
    let old_timestamp = old.received_timestamp();
    let old_content = old.content_key();
    fixture.publish(&old);
    let old_e = old.execution_target();
    assert_eq!(old_t, output_origin + F as u64);
    assert_eq!(old_e, old_t + 8_192);
    fixture.wait_worker(&[&old]);
    let aba = F + 7_800;
    identical_old_to(&mut fixture, &mut baseline, output_origin + aba as u64);
    // Both commands occur between reads. A restored scalar endpoint does not
    // restore the captured source fence or erase the genuine next ramp.
    for mixer in [&mut fixture, &mut baseline] {
        select(mixer, Selection::Stems(0b0101));
        select(mixer, Selection::Stems(0b1010));
    }
    identical_old_to(&mut fixture, &mut baseline, old_e + 513);
    assert_eq!(old.state(), "rejected");
    assert_eq!(old.reason(), Some("preparation_failed"));
    assert!(!old.effective());
    assert_eq!(old.original_due_frame(), old_t);
    assert_eq!(old.execution_target(), old_e);
    assert_eq!(old.sequence(), old_sequence);
    assert_eq!(old.received_timestamp(), old_timestamp);
    assert_eq!(old.content_key(), old_content);
    assert_eq!(
        fixture.voice(0).stretch.native_state_address(),
        native_address
    );
    assert_eq!(
        fixture.mixer.pad_pitch_intent(0),
        Some(PitchIntent::default())
    );
    assert!(fixture.scheduler.is_empty());

    fixture.rows(PitchPadProjection {
        intent: PitchIntent::new(6, 0).unwrap(),
        key_lock: false,
        ..PitchPadProjection::default()
    });
    let fresh = fixture.capture(0, "pitch.extra:18:set");
    let fresh_t = fresh.original_due_frame();
    let fresh_sequence = fresh.sequence();
    let fresh_timestamp = fresh.received_timestamp();
    let fresh_content = fresh.content_key();
    assert!(fresh_sequence > old_sequence);
    fixture.publish(&fresh);
    let fresh_e = fresh.execution_target();
    assert_eq!(fresh_e, fresh_t + 8_192);
    fixture.wait_worker(&[&fresh]);
    assert_eq!(
        fixture.mixer.pitch_runtime.as_ref().unwrap() as *const _ as usize,
        runtime
    );
    let end = (fresh_e - output_origin) as usize;
    let timeline = normal_chronology(end + 4_097);
    let pitch = PitchIntent::new(6, 18).unwrap();
    let origin = reset_origin(&timeline, end, pitch, false);
    assert!(origin < aba && aba < end);
    let mut events = History::FullToStems.events().to_vec();
    events.extend([
        (aba, Selection::Stems(0b0101)),
        (aba, Selection::Stems(0b1010)),
    ]);
    let mut raw = RawRubberBand::new(4.0);
    for active in origin..end {
        raw.sample(
            selected_pcm(&timeline, active, &source, &stems, &events),
            4.0,
        );
    }
    identical_old_to(&mut fixture, &mut baseline, fresh_e);
    let mut rendered = 0;
    let mut adopted_address = None;
    for frames in PARTITIONS {
        let actual = fixture.render(frames);
        assert!(fresh.effective(), "{} {:?}", fresh.state(), fresh.reason());
        let address = fixture.voice(0).stretch.native_state_address();
        assert_eq!(*adopted_address.get_or_insert(address), address);
        for (offset, &sample) in actual.iter().enumerate() {
            let active = end + rendered + offset;
            let expected = raw.sample(
                selected_pcm(&timeline, active, &source, &stems, &events),
                4.0,
            );
            assert!(
                (sample - expected).abs() < 2.0e-6,
                "fresh same-worker SET adopted_offset={} actual={sample} independent_A_reset={expected}",
                rendered + offset
            );
        }
        rendered += frames;
        assert_phase(&fixture, timeline[end + rendered]);
        assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), raw.fifo());
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(
            fixture.voice(0).sample.as_ref().unwrap().source_address(),
            source_address
        );
        assert_eq!(
            fixture.voice(0).stretch.adopted_request_id(),
            Some(fresh_sequence)
        );
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(fixture.mixer.pad_pitch_intent(0), Some(pitch));
        assert_eq!(fresh.original_due_frame(), fresh_t);
        assert_eq!(fresh.execution_target(), fresh_e);
        assert_eq!(fresh.sequence(), fresh_sequence);
        assert_eq!(fresh.received_timestamp(), fresh_timestamp);
        assert_eq!(fresh.content_key(), fresh_content);
    }
    assert_eq!(rendered, 4_097);
    assert_eq!(old.state(), "rejected");
    assert!(!old.effective());
    assert!(fixture.scheduler.is_empty());
}
