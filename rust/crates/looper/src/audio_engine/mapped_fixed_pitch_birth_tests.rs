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

pub(super) fn configured(shift: i32) -> LaunchPath {
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

pub(super) fn drain_grid(path: &mut LaunchPath) {
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

pub(super) fn independent_extent() -> (f64, f64) {
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

struct OwnedBirth {
    path: LaunchPath,
    job: OfflineAnalysisJob,
    delivery: LoaderEventDelivery,
    captured: CapturedPadStart,
    work: Box<crate::audio_engine::ordinary_start::OrdinaryStartWork>,
    request: crate::audio_engine::prepared_native_history::NativeHistoryRequest,
}

fn owned_birth(shift: i32) -> OwnedBirth {
    use crate::audio_engine::prepared_native_history::{MappedNativeInput, SourceReadCheckpoint};
    use crate::audio_engine::source_playback::SourcePlayback;

    let mut path = configured(shift);
    let (job, delivery) = variable_original_delivery(&path);
    publish_original(&path, &delivery, 1);
    path.render(1);
    let captured = path.capture();
    drain_grid(&mut path);
    assert_eq!(captured.target_frame(), Some(64));
    assert_original_current(&captured);
    // The component owns genuine original Work, using the same freezing APIs as
    // the sole scheduler. It never publishes an attack or opens public admission.
    let mut work = captured.take_work(None).unwrap().unwrap();
    work.freeze_mapped_clock(Some(path.timeline), 64, path.timeline.output_frame()).unwrap();
    work.freeze_frames(64, 64).unwrap();
    let (clock, target) = work.mapped_clock().unwrap();
    let mut snapshot = path
        .source
        .mixer
        .normal_start_snapshot_for_test(0, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)))
        .unwrap();
    snapshot.playback =
        SourcePlayback::mapped(captured.mapped_geometry().unwrap(), clock, target).unwrap();
    let position = snapshot.playback.position();
    snapshot.plan.frame_pos = position.frame;
    snapshot.plan.seek_mode = position.seek_mode;
    snapshot.plan.loop_period = snapshot.playback.reader_period();
    work.freeze_launch_origin(SourceReadCheckpoint {
        playback: snapshot.playback,
        plan: snapshot.plan,
    })
    .unwrap();
    let permit = work.capture_history_permit(&snapshot.sample, RATE).unwrap();
    let pitch = PitchIntent::new(shift.clamp(-23, 23), shift - shift.clamp(-23, 23)).unwrap();
    let mut request = snapshot
        .into_request(permit, pitch, false, target, work.sequence(), work.epoch())
        .unwrap();
    assert!(!crate::audio_engine::native_source_coverage::complete_view_available(&request.sample));
    assert!(request.coverage.is_some());
    request.mapped_input = Some(Box::new(
        MappedNativeInput::capture(&work, &request).unwrap(),
    ));
    OwnedBirth {
        path,
        job,
        delivery,
        captured,
        work,
        request,
    }
}

fn independent_input(source: &crate::messages::SampleBuffer, frame: usize) -> [f32; 2] {
    let coordinate = independent_coordinate(frame as u64);
    let left = 32 + (coordinate.floor() as usize - 32) % 32;
    let right = 32 + (left + 1 - 32) % 32;
    let fraction = coordinate.fract() as f32;
    std::array::from_fn(|channel| {
        let a = source.samples[(left - source.resident_start()) * 2 + channel];
        let b = source.samples[(right - source.resident_start()) * 2 + channel];
        a + (b - a) * fraction
    })
}

#[test]
fn owned_mapped_future_declared_suffix_keeps_complete_raw_prefix_actual_receipts_and_three_labels()
{
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_feed_history::{SourceFeedHistory, SourceReadDomain};
    use crate::audio_engine::stretch_processor::{ProductiveSourceFeed, StretchProcessor};

    for shift in [7, -23, 24] {
        // These leads are declared before observing output, independently of Native delay.
        for lead in [0, 137, 8329] {
            let mut partition_reference = None;
            for partitions in [&[512][..], &[127, 1, 383, 17, 256][..]] {
                let fixture = owned_birth(shift);
                let source = fixture.request.sample.clone();
                let stems = fixture.request.stems.clone();
                let accepted = fixture.request.binding.accepted;
                let intent = fixture.request.pitch_intent;
                let mut playback = fixture.request.playback;
                let mut plan = fixture.request.plan;
                let pitch = 2.0_f64.powf(f64::from(shift) / 12.0);
                let mut oracle = RawStereo::new(pitch);
                let history_frames = oracle.history;
                let raw_cut = history_frames - (BLOCK - 1) + lead;
                let observation = oracle.history.max(oracle.delay) + CONTINUATION_SUFFIX;
                // Preserve ALL output before the declared cut as separate prefix evidence.
                // In particular a nonzero early prefix must not disappear from the oracle.
                let raw_stream: Vec<f32> = (0..lead + observation)
                    .flat_map(|frame| oracle.sample(independent_input(&source, frame)))
                    .collect();
                if lead == 8329 {
                    assert!(
                        raw_stream[..lead * 2]
                            .iter()
                            .any(|sample| sample.abs() > 1e-9)
                    );
                }
                let mut candidate = Box::new(NativeAdapterState::new(
                    Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
                    2,
                ));
                let native = candidate.rubberband.as_ref().unwrap().state_address();
                candidate
                    .prepare_owned_mapped_birth_future(
                        fixture.request,
                        RetainedNativeStartCut {
                            history_output_frames: history_frames,
                            discarded_raw_output_frames: raw_cut,
                        },
                    )
                    .unwrap();
                assert_original_current(&fixture.captured);
                let mut processor = StretchProcessor::new(2);
                processor.reset();
                let epoch = processor.preparation_epoch();
                assert_eq!(epoch, fixture.work.epoch());
                assert!(processor.adopt_prepared_pitch(
                    &mut candidate,
                    &ProductiveSourceFeed {
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
                    },
                    epoch,
                    fixture.captured.sequence()
                ));
                assert_eq!(processor.native_state_address(), native);
                // A later requested revision fences new adoption, while this already
                // adopted component retains its immutable original map and Native stream.
                let superseded_request = shift == 7 && lead == 137 && partitions.len() == 1;
                if superseded_request {
                    publish_original(&fixture.path, &fixture.delivery, 2);
                    assert!(fixture.captured.callback_guard().is_err());
                    assert!(fixture.captured.preparation_guard().is_err());
                }
                let initial_coordinates = processor
                    .retained_mapped_output_coordinates()
                    .unwrap()
                    .unwrap();
                let initial_fifos = processor.pending_fifo_frames();
                let bound = processor.retained_source_chunk_limit(&playback, plan, 1024);
                for (bad_output, bad_frames) in [(Some(65), 1), (Some(64), bound + 1)] {
                    assert!(
                        processor
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
                                    output_frame: bad_output,
                                },
                                bad_frames,
                                false
                            )
                            .is_none()
                    );
                    assert_eq!(
                        processor.retained_mapped_output_coordinates().unwrap(),
                        Some(initial_coordinates)
                    );
                    assert_eq!(processor.pending_fifo_frames(), initial_fifos);
                    assert_eq!(processor.native_state_address(), native);
                }
                let missing_right = source
                    .window(
                        32,
                        63,
                        77,
                        crate::messages::ResidentContext::KeyLockFiniteLoop,
                    )
                    .unwrap();
                assert!(
                    processor
                        .process_source(
                            ProductiveSourceFeed {
                                pitch_intent: intent,
                                key_lock: false,
                                sample: &missing_right,
                                stems: stems.as_ref(),
                                outgoing_stems: None,
                                sample_rate_hz: RATE,
                                accepted,
                                plan,
                                playback: &playback,
                                permit: None,
                                output_frame: Some(64),
                            },
                            1,
                            false
                        )
                        .is_none()
                );
                assert_eq!(
                    processor.retained_mapped_output_coordinates().unwrap(),
                    Some(initial_coordinates)
                );
                assert_eq!(processor.pending_fifo_frames(), initial_fifos);
                assert_eq!(processor.native_state_address(), native);
                let mut history = SourceFeedHistory::new();
                let mut actual = Vec::with_capacity(observation * 2);
                let mut emitted = 0;
                let mut caller = 0;
                let mut different_boundaries = 0;
                while emitted < observation {
                    let end = (emitted + partitions[caller % partitions.len()]).min(observation);
                    while emitted < end {
                        if !superseded_request {
                            assert_original_current(&fixture.captured);
                        }
                        playback
                            .refresh_mapped_span(fixture.captured.mapped_geometry().unwrap())
                            .unwrap();
                        let position = playback.position();
                        plan.frame_pos = position.frame;
                        plan.seek_mode = position.seek_mode;
                        let logical_bound = playback.chunk(end - emitted).0;
                        let frames =
                            processor.retained_source_chunk_limit(&playback, plan, logical_bound);
                        different_boundaries += usize::from(frames < logical_bound);
                        assert!(frames > 0);
                        let before = processor
                            .retained_mapped_output_coordinates()
                            .unwrap()
                            .unwrap();
                        assert_eq!(before.logical_output_frame, 64 + emitted as u64);
                        assert_eq!(before.raw_output_frame, (raw_cut + emitted) as u64);
                        assert_eq!(before.actual_input_frame, (64 + lead + emitted) as u64);
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
                                    output_frame: Some(before.logical_output_frame),
                                },
                                frames,
                                false,
                            )
                            .unwrap();
                        assert_eq!(receipt.domain, SourceReadDomain::NativeInput);
                        assert_eq!(
                            receipt.playback.mapped_output_frame(),
                            Some(before.actual_input_frame)
                        );
                        // Inspect every actual read INCLUDING its exclusive endpoint before eviction.
                        for offset in 0..=frames {
                            let position = receipt.playback.position_at(offset).unwrap();
                            let expected = independent_coordinate((lead + emitted + offset) as u64);
                            assert!(
                                (position.frame as f64 + position.fraction - expected).abs() < 1e-9
                            );
                        }
                        history.record_read(receipt);
                        for frame in 0..frames {
                            actual.extend([
                                processor.output_buffers()[0][frame],
                                processor.output_buffers()[1][frame],
                            ]);
                        }
                        assert!(playback.advance(frames));
                        emitted += frames;
                        let after = processor
                            .retained_mapped_output_coordinates()
                            .unwrap()
                            .unwrap();
                        assert_eq!(after.logical_output_frame, (64 + emitted) as u64);
                        assert_eq!(after.raw_output_frame, (raw_cut + emitted) as u64);
                        assert_eq!(after.actual_input_frame, (64 + lead + emitted) as u64);
                        assert_eq!(processor.native_state_address(), native);
                        assert_eq!(processor.native_pitch_scale_observation(), Some(pitch));
                        assert!(processor.wet_render_ready());
                    }
                    caller += 1;
                }
                if lead != 0 {
                    assert!(different_boundaries > 0);
                }
                assert!(actual.iter().any(|sample| sample.abs() > 0.01));
                assert_eq!(actual.len(), observation * 2);
                let error = actual
                    .iter()
                    .zip(&raw_stream[lead * 2..])
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0, f32::max);
                assert!(
                    error <= 3e-6,
                    "shift={shift} lead={lead} partitions={partitions:?} error={error}"
                );
                if let Some(reference) = &partition_reference {
                    assert_eq!(&actual, reference);
                } else {
                    partition_reference = Some(actual);
                }
                assert_eq!(
                    processor.native_primed_output_frames(),
                    history_frames + lead
                );
                assert_eq!(
                    processor.productive_history().unwrap().fed_output_frames,
                    (history_frames + lead + emitted) as u64
                );
                assert!(processor.has_retained_source_input());
                assert_eq!(
                    processor.pending_fifo_frames(),
                    (oracle.input.len(), oracle.output.len())
                );
                assert!(fixture.captured.effective_frame().is_none());
                assert!(
                    !fixture
                        .path
                        .source
                        .mixer
                        .voices
                        .iter()
                        .any(|voice| voice.active)
                );
                assert_eq!(sync_feedback(&fixture.path).unwrap()["effective"], false);
            }
        }
    }
}

#[test]
fn owned_mapped_future_projection_requires_original_checkpoint_and_fails_atomically() {
    use crate::audio_engine::mapped_source_clock::{MappedClockEpoch, MappedLoopGeometry};
    use crate::audio_engine::prepared_native_history::{MappedNativeInput, SourceReadCheckpoint};
    use crate::audio_engine::source_beatmap::SourceBeatmap;
    use crate::audio_engine::source_playback::SourcePlayback;
    use crate::audio_engine::source_reader::{StemRenderSelection, StemTransition};

    let mut fixture = owned_birth(7);
    let input = fixture.request.mapped_input.take().unwrap();
    let original = SourceReadCheckpoint {
        playback: fixture.request.playback,
        plan: fixture.request.plan,
    };
    let mut copied = original;
    assert!(!copied.project_output_frames(1));
    assert!(copied.matches_exact(original));
    for corrupt in [0, 1, 2, 3] {
        let mut checkpoint = original;
        match corrupt {
            0 => {
                let epoch = MappedClockEpoch::new(
                    fixture.work.sequence() + 1,
                    fixture.path.timeline,
                    -fixture.path.timeline.beat_position_at_frame(64).unwrap(),
                )
                .unwrap();
                checkpoint.playback =
                    SourcePlayback::mapped(fixture.captured.mapped_geometry().unwrap(), epoch, 64)
                        .unwrap();
            }
            1 => {
                checkpoint.plan.transition =
                    StemTransition::start(StemRenderSelection::full_mix(), 128)
            }
            _ => {}
        }
        let before = checkpoint;
        let frames = match corrupt {
            2 => u64::MAX,
            // Addition reaches the legal u64 exclusive endpoint, but that address
            // cannot refresh a next span (it needs frame+1). No partial mutation.
            3 => u64::MAX - 64,
            _ => 1,
        };
        assert!(input.project(&mut checkpoint, frames).is_err());
        assert!(checkpoint.matches_exact(before));
    }
    // Equal-domain foreign geometry shares the starting affine anchors, but its
    // canonical exclusive endpoint already exposes the changed following interval.
    let geometry = fixture.captured.mapped_geometry().unwrap();
    let mut foreign: serde_json::Value =
        serde_json::from_str(geometry.map().canonical_record()).unwrap();
    let accepted = &mut foreign["record"]["accepted"];
    let mut later = accepted["anchors"][1].clone();
    later["id"] = json!("foreign-later-knot");
    later["source_seconds_bits"] = json!(bits(56.0 / f64::from(RATE)));
    later["beat_bits"] = json!(bits(0.78));
    accepted["anchors"].as_array_mut().unwrap().insert(2, later);
    let interval = accepted["intervals"][1].clone();
    accepted["intervals"].as_array_mut().unwrap().push(interval);
    seal(&mut foreign);
    let foreign = MappedLoopGeometry::new(
        SourceBeatmap::parse(&foreign.to_string()).unwrap(),
        geometry.domain().sample_frames,
        RATE,
        geometry.domain().region,
        geometry.exact_quarters(),
        false,
    )
    .unwrap();
    let (clock, _) = fixture.work.mapped_clock().unwrap();
    let mut supplied = original;
    supplied.playback = SourcePlayback::mapped(&foreign, clock, 64).unwrap();
    assert!(!supplied.matches_exact(original));
    let impostor = supplied;
    assert!(input.project(&mut supplied, 20).is_err());
    assert!(supplied.matches_exact(impostor));
    supplied = original;
    input.project(&mut supplied, 20).unwrap();
    let position = supplied.playback.position();
    assert!((position.frame as f64 + position.fraction - independent_coordinate(20)).abs() < 1e-9);
    let wrong = SourcePlayback::mapped(&foreign, clock, 84).unwrap();
    assert!(!supplied.playback.matches_exact(&wrong));
    fixture.request.playback = wrong;
    fixture.request.plan.frame_pos = wrong.position().frame;
    assert!(MappedNativeInput::capture(&fixture.work, &fixture.request).is_err());
}

#[test]
fn owned_mapped_future_refuses_cancel_stale_ack_bad_coverage_and_keeps_original_pins_on_reset() {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::stretch_processor::{ProductiveSourceFeed, StretchProcessor};

    for invalid in 0..11 {
        let mut fixture = owned_birth(7);
        let source = fixture.request.sample.clone();
        let stems = fixture.request.stems.clone();
        let accepted = fixture.request.binding.accepted;
        let playback = fixture.request.playback;
        let plan = fixture.request.plan;
        let intent = fixture.request.pitch_intent;
        let oracle = RawStereo::new(2.0_f64.powf(7.0 / 12.0));
        let mut cut = RetainedNativeStartCut {
            history_output_frames: oracle.history,
            discarded_raw_output_frames: oracle.history - (BLOCK - 1) + 137,
        };
        let mut candidate = Box::new(NativeAdapterState::new(
            Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
            2,
        ));
        if !(5..7).contains(&invalid) {
            match invalid {
                0 => {
                    assert!(fixture.captured.cancel());
                }
                1 => fixture.job.cancel(),
                2 => {
                    // This mapped fixture has no accepted scalar timing: clearing zero
                    // is not a stale ACK. Publish a genuinely different, key-bound ACK.
                    assert!(accepted.is_none());
                    assert!(
                        fixture
                            .path
                            .source
                            .engine
                            .current_timing_acknowledgements
                            .acknowledge_for_key(
                                &fixture.path.source.engine.input_runtime_ownership,
                                fixture.captured.content_key(),
                                1,
                            )
                    );
                    assert!(!fixture.request.permit.current(&source, RATE));
                }
                3 => fixture.request.coverage = None,
                4 => fixture.request.key_lock = true,
                7 => cut.discarded_raw_output_frames = oracle.history - BLOCK,
                8 => cut.discarded_raw_output_frames = usize::MAX,
                9 => {
                    cut.discarded_raw_output_frames =
                        crate::audio_engine::source_feed_history::SOURCE_FEED_HISTORY_MAX_FRAMES
                }
                _ => {
                    let mut replacement_window = source.clone();
                    replacement_window.samples = source.samples.to_vec().into();
                    fixture
                        .path
                        .source
                        .engine
                        .input_runtime_ownership
                        .publish_window(0, &replacement_window);
                    assert!(!fixture.request.permit.current(&source, RATE));
                }
            }
            assert!(
                candidate
                    .prepare_owned_mapped_birth_future(fixture.request, cut)
                    .is_err(),
                "unexpected preparation success for invalid case {invalid}"
            );
            assert!(candidate.source.is_none());
        } else {
            candidate
                .prepare_owned_mapped_birth_future(fixture.request, cut)
                .unwrap();
            if invalid == 5 {
                assert!(fixture.captured.cancel());
            } else {
                fixture.job.cancel();
            }
            let mut processor = StretchProcessor::new(2);
            processor.reset();
            assert_eq!(processor.preparation_epoch(), fixture.work.epoch());
            assert!(!processor.adopt_prepared_pitch(
                &mut candidate,
                &ProductiveSourceFeed {
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
                },
                processor.preparation_epoch(),
                fixture.captured.sequence()
            ));
            assert!(candidate.source.is_some());
            assert!(!processor.has_retained_source_input());
        }
    }
    let fixture = owned_birth(7);
    let weak_map = Arc::downgrade(&fixture.captured.mapped_source_owner().unwrap());
    let weak_pcm = Arc::downgrade(&fixture.request.sample.samples);
    let oracle = RawStereo::new(2.0_f64.powf(7.0 / 12.0));
    let mut candidate =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    candidate
        .prepare_owned_mapped_birth_future(
            fixture.request,
            RetainedNativeStartCut {
                history_output_frames: oracle.history,
                discarded_raw_output_frames: oracle.history - (BLOCK - 1) + 137,
            },
        )
        .unwrap();
    drop(fixture.work);
    drop(fixture.captured);
    drop(fixture.path);
    drop(fixture.delivery);
    drop(fixture.job);
    assert!(weak_map.upgrade().is_some());
    assert!(weak_pcm.upgrade().is_some());
    // This is the exact fixed-storage reset used by callback paths. Source/map
    // ownership survives it, and only worker-style request retirement releases it.
    candidate.reset_adapter();
    assert!(weak_map.upgrade().is_some());
    assert!(weak_pcm.upgrade().is_some());
    drop(candidate.source.take());
    assert!(weak_map.upgrade().is_none());
    assert!(weak_pcm.upgrade().is_none());
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
            snapshot.plan.loop_period = snapshot.playback.reader_period();
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
