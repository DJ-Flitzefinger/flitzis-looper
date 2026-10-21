// Internal qualified fixed-h Native chronology; public activation stays guarded.
// No output-phase, device or musical-onset claim.
use super::*;
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::rubberband_backend::RubberBandLiveStateOpaque;
use std::collections::VecDeque;

const BLOCK: usize = 512;
const CONTINUATION_SUFFIX: usize = 4096;

unsafe extern "C" {
    #[link_name = "rubberband_live_new"]
    fn raw_new(rate: u32, channels: u32, options: i32) -> *mut RubberBandLiveStateOpaque;
    #[link_name = "rubberband_live_delete"]
    fn raw_delete(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_reset"]
    fn raw_reset(state: *mut RubberBandLiveStateOpaque);
    #[link_name = "rubberband_live_set_pitch_scale"]
    fn raw_pitch(state: *mut RubberBandLiveStateOpaque, pitch: f64);
    #[link_name = "rubberband_live_get_start_delay"]
    fn raw_delay(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_get_block_size"]
    fn raw_block(state: *mut RubberBandLiveStateOpaque) -> u32;
    #[link_name = "rubberband_live_shift"]
    fn raw_shift(
        state: *mut RubberBandLiveStateOpaque,
        input: *const *const f32,
        output: *const *mut f32,
    );
}

struct RawStereo {
    state: *mut RubberBandLiveStateOpaque,
    input: VecDeque<[f32; 2]>,
    output: VecDeque<[f32; 2]>,
    history: usize,
    delay: usize,
}

impl RawStereo {
    fn new(pitch: f64) -> Self {
        // Independent pinned C ABI/reset protocol. No DUT Native/source/history helper.
        let state = unsafe { raw_new(RATE, 2, 0x1000_0000) };
        assert!(!state.is_null());
        assert_eq!(unsafe { raw_block(state) } as usize, BLOCK);
        unsafe {
            raw_pitch(state, if pitch > 2.0 { 1.0 } else { pitch });
            raw_reset(state);
        }
        let mut raw = Self {
            state,
            input: VecDeque::new(),
            output: std::iter::repeat_n([0.0; 2], BLOCK - 1).collect(),
            history: 4096,
            delay: 0,
        };
        if pitch > 2.0 {
            raw.shift(&[[0.0; 2]; BLOCK]);
            unsafe { raw_pitch(state, pitch) };
        }
        raw.delay = unsafe { raw_delay(state) } as usize;
        if !(0.5..=2.0).contains(&pitch) {
            // Pinned 48k Short source window + actual reset-profile delay, independently
            // declared here rather than copied from the prepared candidate's receipt.
            raw.history =
                ((2048.0 * pitch.max(1.0 / pitch)).ceil() as usize + raw.delay + 2 * BLOCK)
                    .max(4096)
                    .div_ceil(BLOCK)
                    * BLOCK;
        }
        for _ in 0..raw.history {
            assert_eq!(raw.sample([0.0; 2]), [0.0; 2]);
        }
        assert_eq!(raw.input.len(), 0);
        assert_eq!(raw.output.len(), BLOCK - 1);
        raw
    }

    fn shift(&self, frames: &[[f32; 2]; BLOCK]) -> [[f32; 2]; BLOCK] {
        let input: [[f32; BLOCK]; 2] =
            std::array::from_fn(|channel| std::array::from_fn(|frame| frames[frame][channel]));
        let mut output = [[0.0; BLOCK]; 2];
        let input_ptrs = input.each_ref().map(|channel| channel.as_ptr());
        let output_ptrs = output.each_mut().map(|channel| channel.as_mut_ptr());
        unsafe { raw_shift(self.state, input_ptrs.as_ptr(), output_ptrs.as_ptr()) };
        std::array::from_fn(|frame| [output[0][frame], output[1][frame]])
    }

    fn sample(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.input.push_back(frame);
        if self.input.len() == BLOCK {
            let input = std::array::from_fn(|_| self.input.pop_front().unwrap());
            self.output.extend(self.shift(&input));
        }
        self.output.pop_front().unwrap()
    }
}

impl Drop for RawStereo {
    fn drop(&mut self) {
        unsafe { raw_delete(self.state) };
    }
}

fn configured(shift: i32) -> LaunchPath {
    // This sequence is written BEFORE the real copy-first loader captures source.
    // Its first loop sample is distinct, with nonconstant subsequent attack content.
    let pcm = std::array::from_fn(|index| {
        if index == 32 {
            14000
        } else {
            (((index * 3251 + 173) % 20001) as i32 - 10000) as i16
        }
    });
    let mut source = ColdCustody::new_selected_with_pcm(
        Some(crate::audio_engine::cold_residency::ResidentLoadHint {
            start_s: 32.0 / f64::from(RATE),
            end_s: 64.0 / f64::from(RATE),
            key_lock: false,
        }),
        pcm,
    );
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let mut path = LaunchPath::from_source(source);
    path.source
        .mixer
        .set_pad_pitch_intent(
            0,
            PitchIntent::new(shift.clamp(-23, 23), shift - shift.clamp(-23, 23)).unwrap(),
        )
        .unwrap();
    path
}

fn drain_grid(path: &mut LaunchPath) {
    let frame = path.timeline.output_frame();
    drain_control_messages(
        &mut path.source.consumer,
        &mut path.scheduler,
        frame,
        &mut crate::messages::TriggerQuantization::Grid { step_64ths: 16 },
        &mut path.timeline,
        &mut path.source.mixer,
        &mut path.feedback,
        &mut ImmediateAudioBufferRetirement,
    );
}

fn wait_prepared(path: &mut LaunchPath, start: &CapturedPadStart) {
    let frame = path.timeline.output_frame();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let runtime = path.source.mixer.pitch_runtime.as_mut().unwrap();
        runtime.poll();
        if runtime.has_ready_action(start.sequence()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "mapped fixed-h preparation: {} {:?}",
            start.state(),
            start.reason()
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        path.timeline.output_frame(),
        frame,
        "host wait cannot advance virtual F"
    );
}

fn independent_extent() -> (f64, f64) {
    // Original declared source-second/beat anchor bits, without decimal reduction:
    // B(32/RATE) and B(64/RATE)-B(32/RATE) are the captured full-loop intent.
    let beginning = ((32.0 / f64::from(RATE)) / (48.0 / f64::from(RATE))) * 0.6;
    (beginning, 1.0 - beginning)
}

fn independent_coordinate(frame: u64) -> f64 {
    let (beginning, quarters) = independent_extent();
    let beat = beginning + (frame as f64 / 64.0).rem_euclid(quarters);
    let seconds = if beat < 0.6 {
        (beat / 0.6) * (48.0 / f64::from(RATE))
    } else {
        (48.0 / f64::from(RATE))
            + ((beat - 0.6) / (1.0 - 0.6)) * ((64.0 / f64::from(RATE)) - (48.0 / f64::from(RATE)))
    };
    seconds * f64::from(RATE)
}

fn assert_original_current(captured: &CapturedPadStart) {
    // Source/window/ACK permit and the qualified map-job owner are separate
    // authorities. This original is genuinely captured, never artificially admitted.
    captured.preparation_guard().unwrap();
    captured.callback_guard().unwrap();
}

#[test]
fn internal_fixed_h_birth_keeps_native_stream_canonical_receipts_and_complete_prefix() {
    use crate::audio_engine::mapped_source_clock::MappedClockEpoch;
    use crate::audio_engine::native_history_permit::NativeHistoryContext;
    use crate::audio_engine::prepared_native_history::NativeAdapterState;
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_feed_history::{SourceFeedHistory, SourceReadDomain};
    use crate::audio_engine::source_playback::SourcePlayback;
    use crate::audio_engine::stretch_processor::{ProductiveSourceFeed, StretchProcessor};

    for shift in [7, -23, 24] {
        let mut reference = None;
        for partitions in [&[512][..], &[127, 1, 383, 17, 256][..]] {
            let mut path = configured(shift);
            let (_job, delivery) = variable_original_delivery(&path);
            publish_original(&path, &delivery, 1);
            path.render(1);
            // This genuine capture retains the original source/artifact/geometry pins
            // until after the component's last read and off-callback destruction.
            // It is never published as a voice or reported as an effective SYNC start.
            let captured = path.capture();
            drain_grid(&mut path);
            assert_eq!(captured.target_frame(), Some(64));
            assert_original_current(&captured);
            let geometry = captured.mapped_geometry().unwrap();
            let (beginning, quarters) = independent_extent();
            assert_eq!(geometry.start_beat().to_bits(), beginning.to_bits());
            assert_eq!(geometry.exact_quarters().to_bits(), quarters.to_bits());
            let epoch = MappedClockEpoch::new(
                captured.sequence(),
                path.timeline,
                -path.timeline.beat_position_at_frame(64).unwrap(),
            )
            .unwrap();
            let mut snapshot = path
                .source
                .mixer
                .normal_start_snapshot_for_test(
                    0,
                    32.0 / f64::from(RATE),
                    Some(64.0 / f64::from(RATE)),
                )
                .unwrap();
            snapshot.playback = SourcePlayback::mapped(geometry, epoch, 64).unwrap();
            let position = snapshot.playback.position();
            snapshot.plan.frame_pos = position.frame;
            snapshot.plan.seek_mode = position.seek_mode;
            snapshot.plan.loop_period = snapshot.playback.loop_period();
            let source = snapshot.sample.clone();
            let stems = snapshot.stems.clone();
            let accepted = snapshot.accepted;
            let mut playback = snapshot.playback;
            let mut plan = snapshot.plan;
            let intent =
                PitchIntent::new(shift.clamp(-23, 23), shift - shift.clamp(-23, 23)).unwrap();
            let mut processor = StretchProcessor::new(2);
            let preparation_epoch = processor.preparation_epoch();
            let preparation_owner = processor.source_epoch_owner().clone();
            let binding = binding(&path.source, 0);
            let permit = NativeHistoryContext {
                id: 0,
                ownership: &path.source.engine.input_runtime_ownership,
                acknowledgements: &path.source.engine.current_timing_acknowledgements,
                preparation_epoch: &preparation_owner,
            }
            .capture_admitted(&source, RATE, binding.binding)
            .unwrap();
            let request = snapshot
                .into_request(
                    permit,
                    intent,
                    false,
                    64,
                    captured.sequence(),
                    preparation_epoch,
                )
                .unwrap();
            let mut candidate = Box::new(NativeAdapterState::new(
                Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
                2,
            ));
            assert_original_current(&captured);
            candidate.prepare_source_at_physical_birth(request).unwrap();
            assert_original_current(&captured);
            let feed = ProductiveSourceFeed {
                pitch_intent: intent,
                key_lock: false,
                sample: &source,
                stems: stems.as_ref(),
                outgoing_stems: None,
                sample_rate_hz: RATE,
                accepted,
                plan,
                playback: &playback,
                permit: None,
                output_frame: Some(64),
            };
            assert!(processor.adopt_prepared_pitch(
                &mut candidate,
                &feed,
                preparation_epoch,
                captured.sequence(),
            ));
            let native = processor.native_state_address();
            assert_ne!(native, 0);
            let pitch = 2.0_f64.powf(f64::from(shift) / 12.0);
            let mut oracle = RawStereo::new(pitch);
            // The observed raw delay can exceed a short 4096-frame window at
            // extreme h. Retain the WHOLE prefix plus a fixed suffix, never crop
            // or fit Native latency to an apparent musical attack.
            let emitted_frames = oracle.history.max(oracle.delay) + CONTINUATION_SUFFIX;
            let mut expected = Vec::with_capacity(emitted_frames * 2);
            for frame in 0..emitted_frames {
                let coordinate = independent_coordinate(frame as u64);
                let left = 32 + (coordinate.floor() as usize - 32) % 32;
                let right = 32 + (left + 1 - 32) % 32;
                let fraction = coordinate.fract() as f32;
                let input = std::array::from_fn(|channel| {
                    let a = source.samples[(left - source.resident_start()) * 2 + channel];
                    let b = source.samples[(right - source.resident_start()) * 2 + channel];
                    a + (b - a) * fraction
                });
                expected.extend(oracle.sample(input));
            }
            let mut actual = Vec::with_capacity(emitted_frames * 2);
            let mut history = SourceFeedHistory::new();
            let mut emitted = 0;
            let mut caller = 0;
            let mut receipt_count = 0;
            while emitted < emitted_frames {
                let caller_frames =
                    partitions[caller % partitions.len()].min(emitted_frames - emitted);
                let end = emitted + caller_frames;
                while emitted < end {
                    assert_original_current(&captured);
                    playback.refresh_mapped_span(geometry).unwrap();
                    let frames = playback.chunk(end - emitted).0;
                    assert!(frames > 0);
                    let position = playback.position();
                    plan.frame_pos = position.frame;
                    plan.seek_mode = position.seek_mode;
                    let receipt = processor
                        .process_source(
                            ProductiveSourceFeed {
                                pitch_intent: intent,
                                key_lock: false,
                                sample: &source,
                                stems: stems.as_ref(),
                                outgoing_stems: None,
                                sample_rate_hz: RATE,
                                accepted,
                                plan,
                                playback: &playback,
                                permit: None,
                                output_frame: Some(64 + emitted as u64),
                            },
                            frames,
                            false,
                        )
                        .unwrap();
                    assert_original_current(&captured);
                    assert_eq!(receipt.domain, SourceReadDomain::NativeInput);
                    assert_eq!(receipt.output_frames, frames);
                    assert_eq!(
                        receipt.playback.mapped_output_frame(),
                        Some(64 + emitted as u64)
                    );
                    // Inspect each actual receipt INCLUDING its exclusive endpoint
                    // before the bounded journal can evict any earlier span.
                    for offset in 0..=frames {
                        let position = receipt.playback.position_at(offset).unwrap();
                        let expected = independent_coordinate((emitted + offset) as u64);
                        let actual = position.frame as f64 + position.fraction;
                        assert!(
                            (actual - expected).abs() < 1e-9,
                            "absolute={} emitted={emitted} offset={offset} actual={actual:.17} expected={expected:.17} b0={beginning:.17} L={quarters:.17} epoch={epoch:?}",
                            64 + emitted + offset
                        );
                    }
                    history.record_read(receipt);
                    for frame in 0..frames {
                        actual.extend([
                            processor.output_buffers()[0][frame],
                            processor.output_buffers()[1][frame],
                        ]);
                    }
                    assert!(processor.wet_render_ready());
                    assert_eq!(processor.native_state_address(), native);
                    assert_eq!(processor.native_pitch_scale_observation(), Some(pitch));
                    assert_eq!(processor.adopted_request_id(), Some(captured.sequence()));
                    assert!(playback.advance(frames));
                    emitted += frames;
                    receipt_count += 1;
                    let productive = processor.productive_history().unwrap();
                    assert_eq!(
                        productive.fed_output_frames,
                        (oracle.history + emitted) as u64
                    );
                    assert_eq!(productive.next_position, playback.position());
                }
                caller += 1;
            }
            assert!(receipt_count > 3);
            assert_eq!(
                playback.mapped_output_frame(),
                Some(64 + emitted_frames as u64)
            );
            assert_eq!(processor.native_primed_output_frames(), oracle.history);
            assert!(!processor.has_retained_source_input());
            assert_eq!(
                processor.pending_fifo_frames(),
                (oracle.input.len(), oracle.output.len())
            );
            let actual_max = actual.iter().map(|sample| sample.abs()).fold(0.0, f32::max);
            let oracle_max = expected
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0, f32::max);
            assert!(
                actual_max > 0.01,
                "shift={shift} h={pitch:.17} partitions={partitions:?} emitted={emitted_frames} actualMax={actual_max} oracleMax={oracle_max} rawDelay={} H={}",
                oracle.delay,
                oracle.history
            );
            assert_eq!(actual.len(), expected.len());
            let max_error = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(
                max_error <= 3e-6,
                "h={pitch} partitions={partitions:?} uncropped error={max_error}"
            );
            if let Some(reference) = &reference {
                assert_eq!(&actual, reference);
            } else {
                reference = Some(actual);
            }
            assert!(captured.effective_frame().is_none());
            assert!(!path.source.mixer.voices.iter().any(|voice| voice.active));
            assert_eq!(sync_feedback(&path).unwrap()["effective"], false);
            assert!(captured.cancel());
            // Source/map owners remain held while the component is destroyed here.
            drop(processor);
            drop(candidate);
        }
    }
}

#[test]
fn public_fixed_h_future_birth_stays_refused_until_output_alignment_is_integrated() {
    for (shift, attack_policy) in [(7, false), (-23, false), (24, false), (7, true)] {
        let mut path = configured(shift);
        let (_job, delivery) = variable_original_delivery(&path);
        publish_original(&path, &delivery, 1);
        path.drain();
        path.render(1);
        if attack_policy {
            assert!(
                path.source
                    .engine
                    .set_key_lock_start_behavior("attack_at_grid")
                    .unwrap()
            );
        }
        let previous = start_scalar_neighbor(&mut path);
        let start = path.capture();
        drain_grid(&mut path);
        assert_eq!(start.target_frame(), Some(64));
        assert!(path.source.engine.publish_pad_start(&start, None).unwrap());
        drain_grid(&mut path);
        assert_eq!(start.state(), "rejected");
        assert_eq!(start.reason(), Some("mapped_timing_unavailable"));
        assert!(start.effective_frame().is_none());
        path.render(80);
        assert!(
            !path
                .source
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_eq!(scalar_generation(&path), previous);
        assert_eq!(sync_feedback(&path).unwrap()["effective"], false);
    }
}

fn scalar_generation(path: &LaunchPath) -> u64 {
    path.source
        .mixer
        .voices
        .iter()
        .find(|voice| voice.active && voice.sample_id == 1)
        .unwrap()
        .generation
}

fn start_scalar_neighbor(path: &mut LaunchPath) -> u64 {
    path.source
        .mixer
        .set_pad_loop_region(1, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)));
    assert!(path.source.mixer.play_sample(1, 0.4));
    scalar_generation(path)
}

fn release_retirement(retirement: &mut PausedRetirement) {
    retirement.release();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while retirement.owner.state_mut().available_retirement_slots() < retirement.capacity {
        assert!(
            std::time::Instant::now() < deadline,
            "retirement capacity did not return"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn public_dry_mixed_group_retirement_pressure_is_atomic_and_cannot_move_frozen_birth() {
    for miss_target in [false, true] {
        let mut path = configured(0);
        let (_job, delivery) = variable_original_delivery(&path);
        publish_mixed_members(&path, &delivery);
        let previous = start_scalar_neighbor(&mut path);
        path.render(1);
        let (captured, members) = capture_mixed_global(&path);
        drain_grid(&mut path);
        assert_eq!(captured.original_frame(), Some(64));
        let mut retirement = PausedRetirement::new_full();
        let ticket = path
            .source
            .engine
            .publish_captured_global_start(&captured)
            .unwrap()
            .unwrap();
        if miss_target {
            // Admit/prepare with capacity, then remove claim capacity at E itself.
            drain_grid(&mut path);
            wait_prepared(&mut path, &members[0]);
            path.render_with_retirement(63, retirement.owner.state_mut());
            path.render_with_retirement(1, retirement.owner.state_mut());
            assert!(
                members
                    .iter()
                    .all(|member| member.effective_frame().is_none())
            );
            assert_eq!(scalar_generation(&path), previous);
            assert!(
                !path
                    .source
                    .mixer
                    .voices
                    .iter()
                    .any(|voice| voice.active && voice.sample_id == 0)
            );
            release_retirement(&mut retirement);
            path.render_with_retirement(17, retirement.owner.state_mut());
            assert_eq!(captured.state(), "rejected", "{:?}", captured.reason());
            assert_eq!(captured.reason(), Some("mapped_timing_unavailable"));
            assert!(
                members
                    .iter()
                    .all(|member| member.effective_frame().is_none())
            );
            assert_eq!(scalar_generation(&path), previous);
        } else {
            assert_eq!(
                drain_control_messages(
                    &mut path.source.consumer,
                    &mut path.scheduler,
                    1,
                    &mut crate::messages::TriggerQuantization::Immediate,
                    &mut path.timeline,
                    &mut path.source.mixer,
                    &mut path.feedback,
                    retirement.owner.state_mut(),
                ),
                0
            );
            assert!(path.scheduler.is_empty());
            assert!(
                members
                    .iter()
                    .all(|member| !member.accepted() && member.effective_frame().is_none())
            );
            path.render_with_retirement(17, retirement.owner.state_mut());
            assert_eq!(scalar_generation(&path), previous);
            release_retirement(&mut retirement);
            drain_grid(&mut path);
            wait_prepared(&mut path, &members[0]);
            path.render_with_retirement(46, retirement.owner.state_mut());
            path.render_with_retirement(17, retirement.owner.state_mut());
            assert_eq!(ticket.publication_status(), "accepted");
            assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
            for member in &members {
                assert_eq!(
                    (
                        member.target_frame(),
                        member.execution_frame(),
                        member.effective_frame()
                    ),
                    (Some(64), Some(64), Some(64))
                );
            }
            assert!(!path.voice().stretch.wet_render_ready());
            path.render_with_retirement(128, retirement.owner.state_mut());
            verify_actual_reads(&path, &members[0], 145);
            assert_ne!(scalar_generation(&path), previous);
        }
    }
}
