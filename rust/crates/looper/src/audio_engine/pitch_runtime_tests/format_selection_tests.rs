//! Real stereo/rate variants of the held-selection SET chronology.
//! The scalar PCM and direct C ABI reference never read a DUT cursor or history bound.

use super::*;
use crate::audio_engine::rubberband_backend::RubberBandLiveStateOpaque;

const FORMAT_CHANNELS: usize = 2;
const FORMAT_BLOCK: usize = 512;
const ADOPTED_FRAMES: usize = 4_097;

unsafe extern "C" {
    #[link_name = "rubberband_live_new"]
    fn stereo_raw_new(rate: u32, channels: u32, options: i32) -> *mut RubberBandLiveStateOpaque;
    #[link_name = "rubberband_live_delete"]
    fn stereo_raw_delete(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_reset"]
    fn stereo_raw_reset(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_set_pitch_scale"]
    fn stereo_raw_pitch(state: *mut RubberBandLiveStateOpaque, pitch: f64);
    #[link_name = "rubberband_live_get_start_delay"]
    fn stereo_raw_delay(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_get_block_size"]
    fn stereo_raw_block(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_get_channel_count"]
    fn stereo_raw_channels(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_shift"]
    fn stereo_raw_shift(
        state: *mut RubberBandLiveStateOpaque,
        input: *const *const f32,
        output: *const *mut f32,
    );
}

/// One channels-together native owner, with an independent adapter FIFO per channel.
struct StereoRawNative {
    state: *mut RubberBandLiveStateOpaque,
    pitch: f64,
    input: [VecDeque<f32>; FORMAT_CHANNELS],
    output: [VecDeque<f32>; FORMAT_CHANNELS],
}

impl StereoRawNative {
    fn new(rate: u32, pitch: f64) -> Self {
        // Pinned Short window | ChannelsTogether, without the production wrapper.
        let state = unsafe { stereo_raw_new(rate, FORMAT_CHANNELS as u32, 0x1000_0000) };
        assert!(!state.is_null());
        assert_eq!(unsafe { stereo_raw_block(state) } as usize, FORMAT_BLOCK);
        assert_eq!(
            unsafe { stereo_raw_channels(state) } as usize,
            FORMAT_CHANNELS
        );
        let mut raw = Self {
            state,
            pitch,
            input: std::array::from_fn(|_| VecDeque::new()),
            output: std::array::from_fn(|_| std::iter::repeat_n(0.0, FORMAT_BLOCK - 1).collect()),
        };
        unsafe {
            stereo_raw_pitch(state, if pitch > 2.0 { 1.0 } else { pitch });
            stereo_raw_reset(state);
        }
        if pitch > 2.0 {
            // Silent native cold initialization consumes neither real feed nor FIFO time.
            let _ = raw.shift(&[[0.0; FORMAT_BLOCK]; FORMAT_CHANNELS]);
            unsafe { stereo_raw_pitch(state, pitch) };
        }
        raw
    }

    fn shift(
        &mut self,
        input: &[[f32; FORMAT_BLOCK]; FORMAT_CHANNELS],
    ) -> [[f32; FORMAT_BLOCK]; FORMAT_CHANNELS] {
        let mut output = [[0.0; FORMAT_BLOCK]; FORMAT_CHANNELS];
        let input_ptrs =
            std::array::from_fn::<_, FORMAT_CHANNELS, _>(|channel| input[channel].as_ptr());
        let output_ptrs = [output[0].as_mut_ptr(), output[1].as_mut_ptr()];
        unsafe { stereo_raw_shift(self.state, input_ptrs.as_ptr(), output_ptrs.as_ptr()) };
        output
    }

    fn history_bound(rate: u32, pitch: f64) -> usize {
        let raw = Self::new(rate, pitch);
        if (0.5..=2.0).contains(&pitch) {
            return 4_096;
        }
        // Independently pinned Short-window sizes: ceil(rate / 32), rounded up to
        // a power of two. In particular, 96 kHz must not reuse 48 kHz's 2048.
        let window = match rate {
            44_100 => 2_048,
            96_000 => 4_096,
            _ => panic!("undeclared stereo reference format: {rate}"),
        };
        let delay = unsafe { stereo_raw_delay(raw.state) } as usize;
        ((window as f64 * pitch.max(1.0 / pitch)).ceil() as usize + delay + 2 * FORMAT_BLOCK)
            .max(4_096)
            .div_ceil(FORMAT_BLOCK)
            * FORMAT_BLOCK
    }

    fn frame(&mut self, samples: [f32; FORMAT_CHANNELS], pitch: f64) -> [f32; FORMAT_CHANNELS] {
        if pitch != self.pitch {
            unsafe { stereo_raw_pitch(self.state, pitch) };
            self.pitch = pitch;
        }
        for (channel, sample) in samples.into_iter().enumerate() {
            self.input[channel].push_back(sample);
        }
        if self.input[0].len() == FORMAT_BLOCK {
            let input = std::array::from_fn(|channel| {
                std::array::from_fn(|_| self.input[channel].pop_front().unwrap())
            });
            let output = self.shift(&input);
            for (channel, frames) in output.into_iter().enumerate() {
                self.output[channel].extend(frames);
            }
        }
        std::array::from_fn(|channel| self.output[channel].pop_front().unwrap())
    }

    fn fifos(&self) -> [(usize, usize); FORMAT_CHANNELS] {
        std::array::from_fn(|channel| (self.input[channel].len(), self.output[channel].len()))
    }
}

impl Drop for StereoRawNative {
    fn drop(&mut self) {
        unsafe { stereo_raw_delete(self.state) };
    }
}

#[derive(Clone, Copy)]
struct FormatFrame {
    frame: usize,
    fraction: f64,
    rate: f64,
}

fn format_chronology(
    start: usize,
    end: usize,
    events: &[(usize, f64)],
    frames: usize,
) -> Vec<FormatFrame> {
    let mut origin_frame = start;
    let mut origin_fraction = 0.0;
    let mut epoch = 0;
    let mut rate: f64 = 1.0;
    let mut target: f64 = 1.0;
    let mut next_step = usize::MAX;
    let mut result = Vec::with_capacity(frames + 1);
    for active in 0..=frames {
        if let Some((_, value)) = events.iter().find(|(at, _)| *at == active) {
            target = *value;
            next_step = active;
        }
        if active == next_step && rate != target {
            let distance = origin_fraction + (active - epoch) as f64 * rate;
            let whole = distance.floor() as usize;
            origin_frame = start + (origin_frame - start + whole) % (end - start);
            origin_fraction = distance - whole as f64;
            epoch = active;
            rate = if (target - rate).abs() <= 0.05 {
                target
            } else {
                rate + (target - rate).clamp(-0.05, 0.05)
            };
            next_step = active + FORMAT_BLOCK;
        }
        let distance = origin_fraction + (active - epoch) as f64 * rate;
        let whole = distance.floor() as usize;
        result.push(FormatFrame {
            frame: start + (origin_frame - start + whole) % (end - start),
            fraction: distance - whole as f64,
            rate,
        });
    }
    result
}

fn assert_format_phase(fixture: &Fixture, expected: FormatFrame) {
    let actual = fixture.voice(0).source_playback.position();
    assert_eq!(actual.frame, expected.frame);
    assert_eq!(actual.fraction.to_bits(), expected.fraction.to_bits());
    assert_eq!(actual.seek_mode, ExplicitSeekMode::Normal);
    assert_eq!(
        fixture.voice(0).source_playback.tempo_ratio().to_bits(),
        expected.rate.to_bits()
    );
}

fn format_selected_tap(
    source: &SampleBuffer,
    stems: &PreparedStemSet,
    at: usize,
    channel: usize,
    selection: Selection,
) -> f32 {
    match selection {
        Selection::Full => {
            source.samples[(at - source.resident_start()) * FORMAT_CHANNELS + channel]
        }
        Selection::Stems(mask) => stems
            .stems
            .iter()
            .enumerate()
            .filter(|(component, _)| mask & (1 << component) != 0)
            .map(|(_, stem)| stem.samples[(at - stem.resident_start()) * FORMAT_CHANNELS + channel])
            .sum(),
    }
}

fn format_selected_pcm(
    timeline: &[FormatFrame],
    active: usize,
    source: &SampleBuffer,
    stems: &PreparedStemSet,
    events: &[(usize, Selection)],
    start: usize,
    end: usize,
) -> [f32; FORMAT_CHANNELS] {
    let mut from = Selection::Full;
    let mut current = Selection::Full;
    let mut ramp_start = None;
    for &(at, next) in events.iter().take_while(|(at, _)| *at <= active) {
        from = current;
        current = next;
        ramp_start = Some(at);
    }
    let progress = ramp_start.map_or(128.0, |first| {
        let mut elapsed = 0.0;
        for frame in &timeline[first..active] {
            elapsed += frame.rate;
            if elapsed >= 128.0 {
                return 128.0;
            }
        }
        elapsed
    });
    let to_gain = progress as f32 / 128.0;
    let from_gain = 1.0 - to_gain;
    let frame = timeline[active];
    let right = if frame.frame + 1 == end {
        start
    } else {
        frame.frame + 1
    };
    std::array::from_fn(|channel| {
        let tap = |at| {
            if progress < 128.0 {
                format_selected_tap(source, stems, at, channel, from) * from_gain
                    + format_selected_tap(source, stems, at, channel, current) * to_gain
            } else {
                format_selected_tap(source, stems, at, channel, current)
            }
        };
        let left = tap(frame.frame);
        left + (tap(right) - left) * frame.fraction as f32
    })
}

fn format_native_pitch(frame: FormatFrame, pitch: PitchIntent, lock: bool) -> f64 {
    2.0_f64.powf(f64::from(pitch.total_semitones()) / 12.0) / if lock { frame.rate } else { 1.0 }
}

fn format_reset_origin(
    timeline: &[FormatFrame],
    end: usize,
    rate: u32,
    pitch: PitchIntent,
    lock: bool,
) -> (usize, usize) {
    let mut cache = BTreeMap::new();
    let mut bound =
        StereoRawNative::history_bound(rate, format_native_pitch(timeline[end], pitch, lock));
    loop {
        assert!(
            bound <= end,
            "declared actual past must contain the complete native bound"
        );
        // A belongs to this source's own O-relative block lattice, even if E is off-grid.
        let origin = (end - bound) / FORMAT_BLOCK * FORMAT_BLOCK;
        let mut required = bound;
        for &frame in &timeline[origin..end] {
            let native = format_native_pitch(frame, pitch, lock);
            required = required.max(
                *cache
                    .entry(native.to_bits())
                    .or_insert_with(|| StereoRawNative::history_bound(rate, native)),
            );
        }
        if required == bound {
            return (origin, bound);
        }
        bound = required;
    }
}

fn run_format_history(
    fixture: &mut Fixture,
    output_origin: u64,
    history: History,
    capture: usize,
    rate_events: &[(usize, f64)],
    timeline: &[FormatFrame],
) {
    let selections = history.events_at(capture);
    // 30 complete step intervals plus seven frames establish actual settled .5/2,
    // including the full 1.5-wide transition; a requested target alone is no proof.
    let checkpoints: Vec<_> = rate_events[..3]
        .iter()
        .map(|&(at, target)| (at + 30 * FORMAT_BLOCK + 7, target))
        .collect();
    for (index, (checkpoint, _)) in checkpoints.iter().enumerate() {
        assert!(*checkpoint < rate_events[index + 1].0);
    }
    let mut controls: Vec<_> = rate_events
        .iter()
        .map(|(at, _)| *at)
        .chain(selections.iter().map(|(at, _)| *at))
        .chain(checkpoints.iter().map(|(at, _)| *at))
        .collect();
    controls.sort_unstable();
    controls.dedup();
    let mut actual_extremes = [false; 2];
    for active in controls {
        assert!(active < capture);
        fixture.render_to(output_origin + active as u64);
        if let Some((_, target)) = rate_events.iter().find(|(at, _)| *at == active) {
            if active == rate_events[0].0 {
                fixture.mixer.set_speed(*target);
            } else {
                if active == rate_events[1].0 {
                    fixture.mixer.set_bpm_lock(true);
                }
                fixture.mixer.set_master_period(0.5 / target);
            }
        }
        if let Some((_, selection)) = selections.iter().find(|(at, _)| *at == active) {
            select(fixture, *selection);
        }
        if let Some((_, target)) = checkpoints.iter().find(|(at, _)| *at == active) {
            assert_format_phase(fixture, timeline[active]);
            assert_eq!(timeline[active].rate.to_bits(), target.to_bits());
            let actual = fixture.voice(0).source_playback.tempo_ratio();
            assert_eq!(actual.to_bits(), target.to_bits());
            actual_extremes[usize::from(*target == 2.0)] = true;
        }
    }
    assert!(actual_extremes.into_iter().all(|seen| seen));
    fixture.render_to(output_origin + capture as u64);
}

#[test]
fn held_selection_stereo_formats_replay_real_past_at_original_settled_e() {
    for rate in [44_100_u32, 96_000] {
        for lock in [true, false] {
            for positive in [true, false] {
                let (history, pitch) = match (lock, positive) {
                    (true, true) => (History::FullToStems, PitchIntent::new(6, 18).unwrap()),
                    (true, false) => (History::StemsToFull, PitchIntent::new(-5, -18).unwrap()),
                    (false, true) => (History::Mask, PitchIntent::new(6, 18).unwrap()),
                    (false, false) => (History::Empty, PitchIntent::new(-5, -18).unwrap()),
                };
                let beat_frames = rate as usize / 2;
                let end = 3 * beat_frames;
                let capture = end - 4_096;
                let start_frame = 4 * rate as usize / 3;
                let end_frame = start_frame + 2 * rate as usize;
                let extent = 5 * rate as usize;
                let rate_events = [
                    (1_237, 0.5),
                    (17_999, 2.0),
                    (end - 20_000, 0.5),
                    (capture - 73, 2.0),
                ];
                assert!(rate_events.windows(2).all(|pair| pair[0].0 < pair[1].0));
                assert!(rate_events.iter().all(|(at, _)| at % FORMAT_BLOCK != 0));
                let timeline =
                    format_chronology(start_frame, end_frame, &rate_events, end + ADOPTED_FRAMES);
                assert_ne!(timeline[capture].rate, 2.0);
                assert_ne!(timeline[end].rate, 2.0);
                assert_ne!(timeline[capture].rate, timeline[end].rate);
                let (mut fixture, source, stems, output_origin) =
                    selection_setup_format(lock, PitchIntent::default(), rate, FORMAT_CHANNELS);
                assert_eq!(fixture.sample_rate_hz, rate);
                assert_eq!(fixture.channels, FORMAT_CHANNELS);
                assert_eq!(source.channels, FORMAT_CHANNELS);
                assert_eq!(source.frame_count(), extent);
                assert_eq!(stems.sample_rate_hz, rate);
                assert_eq!(stems.channels, FORMAT_CHANNELS);
                assert_eq!(stems.frame_count, extent);
                assert!(
                    source
                        .samples
                        .chunks_exact(FORMAT_CHANNELS)
                        .any(|frame| frame[0] != frame[1])
                );
                for stem in &stems.stems {
                    assert_eq!(stem.channels, FORMAT_CHANNELS);
                    assert!(
                        stem.samples
                            .chunks_exact(FORMAT_CHANNELS)
                            .any(|frame| frame[0] != frame[1])
                    );
                }
                run_format_history(
                    &mut fixture,
                    output_origin,
                    history,
                    capture,
                    &rate_events,
                    &timeline,
                );
                assert_format_phase(&fixture, timeline[capture]);
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
                clock.master_beat = capture as f64 / beat_frames as f64;
                fixture.clock.publish(clock);
                let action = fixture
                    .control
                    .engine
                    .capture_pad_pitch_action(
                        0,
                        &format!("pitch.extra:{}:set", pitch.extra_shift()),
                        None,
                    )
                    .unwrap();
                let original_t = action.original_due_frame();
                let sequence = action.sequence();
                let timestamp = action.received_timestamp();
                let content = action.content_key();
                assert_eq!(original_t, output_origin + end as u64);
                fixture.publish(&action);
                assert_eq!(action.execution_target(), original_t);
                assert_eq!(original_t - fixture.transport.output_frame(), 4_096);
                fixture.wait_worker(&[&action]);
                assert_eq!(
                    fixture.transport.output_frame(),
                    output_origin + capture as u64
                );

                let (origin, bound) = format_reset_origin(&timeline, end, rate, pitch, lock);
                let selections = history.events_at(capture);
                assert!(bound > end - capture);
                assert!(
                    origin < selections[0].0,
                    "real selection past must precede F inside A..E"
                );
                let first_pitch =
                    format_native_pitch(timeline[origin + FORMAT_BLOCK - 1], pitch, lock);
                let mut raw = StereoRawNative::new(rate, first_pitch);
                let mut wrong_fullmix = StereoRawNative::new(rate, first_pitch);
                for active in origin..end {
                    let native = format_native_pitch(timeline[active], pitch, lock);
                    raw.frame(
                        format_selected_pcm(
                            &timeline,
                            active,
                            &source,
                            &stems,
                            &selections,
                            start_frame,
                            end_frame,
                        ),
                        native,
                    );
                    wrong_fullmix.frame(
                        format_selected_pcm(
                            &timeline,
                            active,
                            &source,
                            &stems,
                            &[],
                            start_frame,
                            end_frame,
                        ),
                        native,
                    );
                }
                let expected_fifo = (
                    (end - origin) % FORMAT_BLOCK,
                    FORMAT_BLOCK - 1 - (end - origin) % FORMAT_BLOCK,
                );
                assert_eq!(raw.fifos(), [expected_fifo; FORMAT_CHANNELS]);
                assert_eq!(raw.fifos(), wrong_fullmix.fifos());
                fixture.render_to(original_t);
                assert_format_phase(&fixture, timeline[end]);
                let endpoint = fixture.mixer.capture_pitch_voice_snapshot(index).unwrap();
                assert!(!endpoint.plan.transition.is_active());
                if matches!(history, History::StemsToFull) {
                    assert_eq!(endpoint.plan.selection.required_stem_hash(), None);
                    assert!(
                        endpoint.stems.is_some(),
                        "FullMix E retains the actual historical pin"
                    );
                    let readers = endpoint.readers().unwrap();
                    assert!(readers.current.is_none() && readers.outgoing.is_none());
                }
                let mut maximum = [0.0_f32; FORMAT_CHANNELS];
                let mut wrong_maximum = [0.0_f32; FORMAT_CHANNELS];
                let mut native_address = None;
                let mut rendered = 0;
                for frames in PARTITIONS {
                    let actual = fixture.render(frames);
                    assert_eq!(actual.len(), frames * FORMAT_CHANNELS);
                    assert!(
                        action.effective(),
                        "{} {:?}",
                        action.state(),
                        action.reason()
                    );
                    let address = fixture.voice(0).stretch.native_state_address();
                    assert_ne!(address, 0);
                    assert_eq!(*native_address.get_or_insert(address), address);
                    for (offset, samples) in actual.chunks_exact(FORMAT_CHANNELS).enumerate() {
                        let active = end + rendered + offset;
                        let input = format_selected_pcm(
                            &timeline,
                            active,
                            &source,
                            &stems,
                            &selections,
                            start_frame,
                            end_frame,
                        );
                        let native = format_native_pitch(timeline[active], pitch, lock);
                        let expected = raw.frame(input, native);
                        let wrong = wrong_fullmix.frame(input, native);
                        for channel in 0..FORMAT_CHANNELS {
                            let error = (samples[channel] - expected[channel]).abs();
                            maximum[channel] = maximum[channel].max(error);
                            wrong_maximum[channel] = wrong_maximum[channel]
                                .max((samples[channel] - wrong[channel]).abs());
                            assert!(
                                error < 2.0e-6,
                                "rate={rate} {history:?} Lock={lock} total={} channel={channel} adopted_offset={} actual={} independent_A_reset={} error={error}",
                                pitch.total_semitones(),
                                rendered + offset,
                                samples[channel],
                                expected[channel]
                            );
                        }
                    }
                    rendered += frames;
                    assert_format_phase(&fixture, timeline[end + rendered]);
                    let fifo = fixture.voice(0).stretch.pending_fifo_frames();
                    assert_eq!(raw.fifos(), [fifo; FORMAT_CHANNELS]);
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
                    assert!(!fixture.voice(0).stretch.wet_mode_failed());
                }
                assert_eq!(rendered, ADOPTED_FRAMES);
                for (channel, wrong) in wrong_maximum.into_iter().enumerate() {
                    assert!(
                        wrong > 1.0e-5,
                        "actual stereo stem past must be indispensable: rate={rate} {history:?} Lock={lock} total={} channel={channel} wrong_FullMix_past={wrong}",
                        pitch.total_semitones()
                    );
                }
                assert!(fixture.scheduler.is_empty());
                println!(
                    "held_selection_stereo_SET rate={rate} channels={FORMAT_CHANNELS} {history:?} Lock={lock} total={} O={output_origin} A={origin} H={bound} F={capture} E={end} frames={rendered} max_abs={maximum:?} wrong_FullMix_only_past={wrong_maximum:?}",
                    pitch.total_semitones()
                );
            }
        }
    }
}
