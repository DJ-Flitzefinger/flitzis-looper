// Internal qualified Native chronology; public activation stays guarded.
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

pub(super) struct RawStereo {
    state: *mut RubberBandLiveStateOpaque,
    input: VecDeque<[f32; 2]>,
    output: VecDeque<[f32; 2]>,
    pub(super) history: usize,
    delay: usize,
    pitch: f64,
    pending_pitch: Option<f64>,
}

impl RawStereo {
    pub(super) fn new(pitch: f64) -> Self {
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
            pitch,
            pending_pitch: None,
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

    pub(super) fn sample(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.sample_at_pitch(frame, self.pitch)
    }

    pub(super) fn sample_at_pitch(&mut self, frame: [f32; 2], pitch: f64) -> [f32; 2] {
        if self.input.is_empty() {
            self.pending_pitch = Some(pitch);
        }
        self.input.push_back(frame);
        if self.input.len() == BLOCK {
            let head_pitch = self.pending_pitch.take().unwrap();
            if self.pitch.to_bits() != head_pitch.to_bits() {
                unsafe { raw_pitch(self.state, head_pitch) };
                self.pitch = head_pitch;
            }
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

pub(super) fn wait_prepared(path: &mut LaunchPath, start: &CapturedPadStart) {
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
    let oracle = tiny_oracle(true, false);
    (oracle.beginning.rounded(), oracle.quarters.rounded())
}

fn independent_coordinate(frame: u64) -> f64 {
    tiny_oracle(true, false).at_frame(frame as usize).0
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
    owned_birth_with_mode(shift, false, false)
}

fn owned_birth_with_mode(shift: i32, key_lock: bool, unity_head: bool) -> OwnedBirth {
    let path = configured(shift);
    let (job, delivery) = if unity_head {
        // Independent source data: first interval r=1, second approximately 2/3 at F=64.
        // The selected loop repeats both intervals many times inside a Native block.
        let mut expected = positive_launch_record(&path);
        let accepted = &mut expected["record"]["accepted"];
        let first = accepted["anchors"][0].clone();
        let mut middle = accepted["anchors"][1].clone();
        middle["id"] = json!("unity-then-two-thirds-rate");
        middle["source_seconds_bits"] = json!(bits(48.0 / f64::from(RATE)));
        middle["beat_bits"] = json!(bits(0.75));
        let mut last = accepted["anchors"][1].clone();
        last["beat_bits"] = json!(bits(1.125));
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
        (job, delivery)
    } else {
        variable_original_delivery(&path)
    };
    let fixture = finish_owned_birth(path, job, delivery, shift, key_lock);
    tiny_oracle(true, unity_head)
        .assert_physical_quarters(fixture.captured.mapped_geometry().unwrap());
    fixture
}

fn finish_owned_birth(
    mut path: LaunchPath,
    job: OfflineAnalysisJob,
    delivery: LoaderEventDelivery,
    shift: i32,
    key_lock: bool,
) -> OwnedBirth {
    use crate::audio_engine::prepared_native_history::{MappedNativeInput, SourceReadCheckpoint};
    publish_original(&path, &delivery, 1);
    path.render(1);
    let captured = path.capture();
    drain_grid(&mut path);
    assert_eq!(captured.target_frame(), Some(64));
    assert_original_current(&captured);
    // The component owns genuine original Work, using the same freezing APIs as
    // the sole scheduler. It never publishes an attack or opens public admission.
    let mut work = captured.take_work(None).unwrap().unwrap();
    work.freeze_mapped_clock(Some(path.timeline), 64, path.timeline.output_frame())
        .unwrap();
    work.freeze_frames(64, 64).unwrap();
    let target = work.execution_frame();
    let mut snapshot = path
        .source
        .mixer
        .normal_start_snapshot_for_test(0, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)))
        .unwrap();
    let birth = original_birth_checkpoint(&work, &snapshot);
    snapshot.playback = birth.playback;
    snapshot.plan = birth.plan;
    work.freeze_launch_origin(SourceReadCheckpoint {
        playback: snapshot.playback,
        plan: snapshot.plan,
    })
    .unwrap();
    let permit = work.capture_history_permit(&snapshot.sample, RATE).unwrap();
    let pitch = PitchIntent::new(shift.clamp(-23, 23), shift - shift.clamp(-23, 23)).unwrap();
    let mut request = snapshot
        .into_request(
            permit,
            pitch,
            key_lock,
            target,
            work.sequence(),
            work.epoch(),
        )
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

fn original_birth_checkpoint(
    work: &crate::audio_engine::ordinary_start::OrdinaryStartWork,
    snapshot: &crate::audio_engine::prepared_native_history::PitchSourceSnapshot,
) -> crate::audio_engine::prepared_native_history::SourceReadCheckpoint {
    use crate::audio_engine::prepared_native_history::SourceReadCheckpoint;
    let original = SourceReadCheckpoint {
        playback: snapshot.playback,
        plan: snapshot.plan,
    };
    let birth = crate::audio_engine::mapped_birth::playback_at_birth(
        work.origin().mapped_geometry().unwrap(),
        work.sequence(),
        work.mapped_timeline.unwrap(),
        original,
        work.execution_frame(),
        work.launch_reference(),
    )
    .unwrap();
    assert_eq!(birth.playback.position(), original.playback.position());
    assert_eq!(
        birth.playback.position().fraction.to_bits(),
        0.0_f64.to_bits()
    );
    assert_eq!(
        birth.playback.position().frame,
        original.plan.loop_region.start
    );
    assert_eq!(
        birth.playback.original_scalar_domain(),
        Some(original.plan.domain())
    );
    assert_eq!(
        birth.playback.mapped_output_frame(),
        Some(work.execution_frame())
    );
    birth
}

fn independent_input(source: &crate::messages::SampleBuffer, frame: usize) -> [f32; 2] {
    independent_input_with_shape(source, frame, false)
}

#[test]
fn finite_mapped_birth_coverage_replan_keeps_original_proof_and_refuses_stale_source_window_or_ack()
{
    use crate::audio_engine::prepared_native_history::SourceReadCheckpoint;

    for invalid in 0..3 {
        let fixture = owned_birth(7);
        let scalar = fixture
            .path
            .source
            .mixer
            .normal_start_snapshot_for_test(0, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)))
            .unwrap();
        let mut timeline = fixture.path.timeline;
        assert!(timeline.anchor_beat_position_at_frame(0.25, timeline.output_frame()));
        let next = crate::audio_engine::mapped_birth::playback_at_birth(
            fixture.captured.mapped_geometry().unwrap(),
            fixture.work.sequence(),
            timeline,
            SourceReadCheckpoint {
                playback: scalar.playback,
                plan: scalar.plan,
            },
            128,
            fixture.work.launch_reference(),
        )
        .unwrap();
        let original = SourceReadCheckpoint {
            playback: fixture.request.playback,
            plan: fixture.request.plan,
        };
        let original_target = fixture.request.target_output_frame;
        let permit = fixture.request.permit.scalar_identity();
        assert!(fixture.request.coverage.is_some());
        let replacement = fixture
            .request
            .coverage_for_replanned_mapped_birth(next)
            .unwrap()
            .unwrap();
        assert!(replacement.matches_with_readers(
            &fixture.request.sample,
            crate::audio_engine::source_reader::StemReadSets::new(
                fixture.request.stems.as_ref(),
                None
            ),
            RATE,
            fixture.request.binding.accepted,
            next.plan,
            &next.playback,
            true,
        ));
        assert!(
            !fixture
                .request
                .coverage
                .as_ref()
                .unwrap()
                .matches_with_readers(
                    &fixture.request.sample,
                    crate::audio_engine::source_reader::StemReadSets::new(
                        fixture.request.stems.as_ref(),
                        None
                    ),
                    RATE,
                    fixture.request.binding.accepted,
                    next.plan,
                    &next.playback,
                    true,
                )
        );
        let ownership = &fixture.path.source.engine.input_runtime_ownership;
        // Retain this changed window until after the original proof is rejected.
        let mut changed_window = fixture.request.sample.clone();
        match invalid {
            0 => ownership.revoke_source(0),
            1 => {
                changed_window.samples = changed_window.samples.to_vec().into();
                ownership.publish_window(0, &changed_window);
            }
            _ => assert!(
                fixture
                    .path
                    .source
                    .engine
                    .current_timing_acknowledgements
                    .acknowledge_for_key(ownership, fixture.captured.content_key(), 1)
            ),
        }
        assert!(
            fixture
                .request
                .coverage_for_replanned_mapped_birth(next)
                .is_err(),
            "case {invalid}"
        );
        assert!(original.matches_exact(SourceReadCheckpoint {
            playback: fixture.request.playback,
            plan: fixture.request.plan,
        }));
        assert_eq!(fixture.request.target_output_frame, original_target);
        assert_eq!(fixture.request.permit.scalar_identity(), permit);
        assert!(fixture.request.coverage.is_some());
        assert!(fixture.request.actual_feed.is_none());
        assert!(
            fixture
                .path
                .source
                .mixer
                .voices
                .iter()
                .all(|voice| !voice.active)
        );
    }
}

fn independent_coordinate_with_shape(frame: usize, unity_head: bool) -> f64 {
    tiny_oracle(true, unity_head).at_frame(frame).0
}

fn independent_pitch(frame: usize, shift: i32, key_lock: bool, unity_head: bool) -> f64 {
    let h = 2.0_f64.powf(f64::from(shift) / 12.0);
    if !key_lock {
        return h;
    }
    h / tiny_oracle(true, unity_head).at_frame(frame).1
}

fn independent_input_with_shape(
    source: &crate::messages::SampleBuffer,
    frame: usize,
    unity_head: bool,
) -> [f32; 2] {
    let coordinate = independent_coordinate_with_shape(frame, unity_head);
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
    for shift in [7, -23, 24] {
        for lead in [0, 137, 8329] {
            assert_owned_mapped_stream(shift, false, false, lead, 0);
        }
    }
}

#[test]
fn owned_mapped_block_head_pitch_is_partition_independent_across_knots_and_worker_handoff() {
    for silent_tail in [0, 17] {
        for lead in [0, 137] {
            assert_owned_mapped_stream(0, true, true, lead, silent_tail);
        }
    }
    assert_owned_mapped_stream(7, true, false, 137, 0);
}

#[test]
fn owned_mapped_pitch_preconditions_preserve_partial_fifo_and_original_owner() {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut, SourceReadCheckpoint,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;

    let fixture = owned_birth_with_mode(0, true, true);
    let playback = fixture.request.playback;
    let plan = fixture.request.plan;
    let intent = fixture.request.pitch_intent;
    let cut = RetainedNativeStartCut {
        history_output_frames: 4096,
        discarded_raw_output_frames: 4096 - (BLOCK - 1) + 137,
    };
    let mut candidate =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    candidate
        .prepare_owned_mapped_birth_future(fixture.request, cut)
        .unwrap();
    let (actual, actual_plan) = candidate
        .retained_start_feed(&playback, plan)
        .unwrap()
        .unwrap();
    let checkpoint = SourceReadCheckpoint {
        playback: actual,
        plan: actual_plan,
    };
    let native = candidate.rubberband.as_ref().unwrap().state_address();
    let coordinates = candidate.retained_mapped_output_coordinates().unwrap();
    let initial_input = candidate.input.clone();
    let initial_output = candidate.output.clone();
    let initial_input_fifo = candidate.input_fifo.clone();
    let initial_output_fifo = candidate.output_fifo.clone();
    let pending = candidate.mapped_pending_pitch_for_test();
    assert!(pending.is_some());
    let input_len = candidate.input_fifo[0].len();
    let output_len = candidate.output_fifo[0].len();
    let pitch = candidate.pitch_scale;
    let unchanged = |state: &NativeAdapterState| {
        assert_eq!(state.rubberband.as_ref().unwrap().state_address(), native);
        assert_eq!(
            state.retained_mapped_output_coordinates().unwrap(),
            coordinates
        );
        assert_eq!(state.input, initial_input);
        assert_eq!(state.output, initial_output);
        assert_eq!(state.input_fifo, initial_input_fifo);
        assert_eq!(state.output_fifo, initial_output_fifo);
        assert!(state.input_fifo.iter().all(|fifo| fifo.len() == input_len));
        assert!(
            state
                .output_fifo
                .iter()
                .all(|fifo| fifo.len() == output_len)
        );
        assert_eq!(state.mapped_pending_pitch_for_test(), pending);
        assert_eq!(state.pitch_scale.to_bits(), pitch.to_bits());
    };
    let mut wrong = checkpoint;
    wrong.plan.frame_pos += 1;
    assert!(
        candidate
            .live_mapped_pitch_chunk(wrong, 1, intent, true)
            .is_err()
    );
    assert!(
        candidate
            .live_mapped_pitch_chunk(checkpoint, 1, intent, false)
            .is_err()
    );
    assert!(
        candidate
            .live_mapped_pitch_chunk(checkpoint, 1, PitchIntent::new(1, 0).unwrap(), true)
            .is_err()
    );
    let mut bounded = actual;
    let bound = bounded.chunk(1024).0;
    assert!(
        candidate
            .live_mapped_pitch_chunk(checkpoint, bound + 1, intent, true)
            .is_err()
    );
    unchanged(&candidate);

    for (feed, mut output) in [
        (vec![vec![0.0]], vec![vec![0.0]; 2]),
        (vec![Vec::new(); 2], vec![vec![0.0]; 2]),
        (vec![vec![0.0]; 2], vec![Vec::new(); 2]),
    ] {
        let chunk = candidate
            .live_mapped_pitch_chunk(checkpoint, 1, intent, true)
            .unwrap()
            .unwrap();
        assert!(candidate.render_mapped(&feed, &mut output, chunk).is_err());
        unchanged(&candidate);
    }
    let empty = candidate
        .live_mapped_pitch_chunk(checkpoint, 0, intent, true)
        .unwrap()
        .unwrap();
    candidate
        .render_mapped(
            &[Vec::new(), Vec::new()],
            &mut [Vec::new(), Vec::new()],
            empty,
        )
        .unwrap();
    unchanged(&candidate);
    assert!(
        candidate
            .render(&[vec![0.0], vec![0.0]], &mut [vec![0.0], vec![0.0]], 1, 1.0)
            .is_err()
    );
    unchanged(&candidate);

    // Identical numeric geometry from another real original action has no custody here.
    let foreign = owned_birth_with_mode(0, true, true);
    let foreign_playback = foreign.request.playback;
    let foreign_plan = foreign.request.plan;
    let mut other = NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    other
        .prepare_owned_mapped_birth_future(foreign.request, cut)
        .unwrap();
    assert_eq!(
        other.retained_mapped_output_coordinates().unwrap(),
        coordinates
    );
    let (playback, plan) = other
        .retained_start_feed(&foreign_playback, foreign_plan)
        .unwrap()
        .unwrap();
    let chunk = other
        .live_mapped_pitch_chunk(SourceReadCheckpoint { playback, plan }, 1, intent, true)
        .unwrap()
        .unwrap();
    assert!(
        candidate
            .render_mapped(&[vec![0.0], vec![0.0]], &mut [vec![0.0], vec![0.0]], chunk)
            .is_err()
    );
    unchanged(&candidate);
    assert_original_current(&fixture.captured);
    assert_original_current(&foreign.captured);
}

#[test]
fn installed_mapped_voice_revocation_refuses_minted_chunk_without_fifo_mutation() {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut, SourceReadCheckpoint,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    // Isolated lifetime/adapter component. The public tests separately prove
    // installation by the genuine Ordinary, Global and Piano birth claims.
    let fixture = owned_birth_with_mode(0, true, true);
    let playback = fixture.request.playback;
    let plan = fixture.request.plan;
    let intent = fixture.request.pitch_intent;
    let cut = RetainedNativeStartCut {
        history_output_frames: 4096,
        discarded_raw_output_frames: 4096 - (BLOCK - 1),
    };
    let mut candidate =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    candidate
        .prepare_owned_mapped_birth_future(fixture.request, cut)
        .unwrap();
    let lifetime = Arc::new(AtomicU64::new(0));
    let staged = candidate
        .stage_mapped_voice(&lifetime, 0, false, 1)
        .unwrap();
    lifetime.store(1, Ordering::Release);
    candidate.install_mapped_voice_claimed(staged);
    assert!(candidate.has_effective_mapped_voice_for_test());
    assert!(
        candidate
            .stage_mapped_voice(&lifetime, 1, true, 1)
            .is_none()
    );
    let (actual, actual_plan) = candidate
        .retained_start_feed(&playback, plan)
        .unwrap()
        .unwrap();
    let checkpoint = SourceReadCheckpoint {
        playback: actual,
        plan: actual_plan,
    };
    let chunk = candidate
        .live_mapped_pitch_chunk(checkpoint, 1, intent, true)
        .unwrap()
        .unwrap();
    let warm_keys = candidate.mapped_projection_memo_keys_for_test().unwrap();
    assert!(
        warm_keys
            .iter()
            .all(|role| role.iter().any(Option::is_some))
    );
    let input_fifo = candidate.input_fifo.clone();
    let output_fifo = candidate.output_fifo.clone();
    let input = candidate.input.clone();
    let output = candidate.output.clone();
    let coordinates = candidate.retained_mapped_output_coordinates().unwrap();
    let pending = candidate.mapped_pending_pitch_for_test();
    let native = candidate.rubberband.as_ref().unwrap().state_address();
    let mut destination = vec![vec![123.0]; 2];
    lifetime.store(0, Ordering::Release);
    fixture.captured.callback_guard().unwrap();
    assert!(
        candidate
            .render_mapped(&vec![vec![0.25]; 2], &mut destination, chunk)
            .is_err()
    );
    assert_eq!(destination, vec![vec![123.0]; 2]);
    assert_eq!(candidate.input_fifo, input_fifo);
    assert_eq!(candidate.output_fifo, output_fifo);
    assert_eq!(candidate.input, input);
    assert_eq!(candidate.output, output);
    assert_eq!(
        candidate.retained_mapped_output_coordinates().unwrap(),
        coordinates
    );
    assert_eq!(candidate.mapped_pending_pitch_for_test(), pending);
    assert_eq!(
        candidate.rubberband.as_ref().unwrap().state_address(),
        native
    );
    assert!(
        candidate
            .live_mapped_pitch_chunk(checkpoint, 1, intent, true)
            .is_err()
    );
    lifetime.store(2, Ordering::Release);
    assert!(!candidate.has_effective_mapped_voice_for_test());
    assert!(
        candidate
            .live_mapped_pitch_chunk(checkpoint, 1, intent, true)
            .is_err()
    );
    candidate.reset_adapter();
    assert!(!candidate.has_effective_mapped_voice_for_test());
    // An installed request is never a reusable worker/silent-prefix candidate.
    let request = candidate.source.take().unwrap();
    assert!(
        candidate
            .prepare_owned_mapped_birth_future(request, cut)
            .is_err()
    );
}

#[derive(Debug, PartialEq)]
struct MappedAdapterSnapshot {
    native: usize,
    backend_pitch: u64,
    flags: (bool, bool, bool),
    pitch: u64,
    head: Option<u64>,
    witness: Option<(u64, usize, u64, u64, u64)>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    input_fifo: Vec<crate::audio_engine::stretch_processor::FixedFifo>,
    output_fifo: Vec<crate::audio_engine::stretch_processor::FixedFifo>,
    coordinates:
        Option<crate::audio_engine::prepared_native_history::RetainedMappedOutputCoordinates>,
    primed: usize,
}

impl MappedAdapterSnapshot {
    fn capture(state: &crate::audio_engine::prepared_native_history::NativeAdapterState) -> Self {
        Self {
            native: state.rubberband.as_ref().unwrap().state_address(),
            backend_pitch: state.rubberband.as_ref().unwrap().pitch_scale().to_bits(),
            flags: (state.active, state.used, state.dirty),
            pitch: state.pitch_scale.to_bits(),
            head: state.mapped_pending_pitch_for_test().map(f64::to_bits),
            witness: state.mapped_pitch_witness_for_test(),
            input: state.input.clone(),
            output: state.output.clone(),
            input_fifo: state.input_fifo.clone(),
            output_fifo: state.output_fifo.clone(),
            coordinates: state.retained_mapped_output_coordinates().unwrap(),
            primed: state.primed_output_frames,
        }
    }
}

fn actual_test_feed(
    state: &crate::audio_engine::prepared_native_history::NativeAdapterState,
    checkpoint: crate::audio_engine::prepared_native_history::SourceReadCheckpoint,
    frames: usize,
) -> Vec<Vec<f32>> {
    let source = state.source.as_ref().unwrap();
    let mut feed = vec![vec![0.0; frames]; 2];
    checkpoint
        .plan
        .fill_fractional_buffers(
            &source.sample,
            source.stems.as_ref(),
            &checkpoint.playback,
            &mut feed,
            frames,
        )
        .unwrap();
    feed
}

#[test]
fn owned_mapped_pitch_descriptors_expire_after_input_and_same_owner_repreparation() {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut, SourceReadCheckpoint,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    let fixture = owned_long_birth(1536);
    let logical = fixture.request.playback;
    let logical_plan = fixture.request.plan;
    let source = fixture.request.sample.clone();
    let intent = fixture.request.pitch_intent;
    let cut = RetainedNativeStartCut {
        history_output_frames: 4096,
        discarded_raw_output_frames: 4096 - (BLOCK - 1),
    };
    let mut candidate =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    candidate
        .prepare_owned_mapped_birth_future(fixture.request, cut)
        .unwrap();
    let (playback, plan) = candidate
        .retained_start_feed(&logical, logical_plan)
        .unwrap()
        .unwrap();
    let original = SourceReadCheckpoint { playback, plan };
    let stale = candidate
        .live_mapped_pitch_chunk(original, 1, intent, true)
        .unwrap()
        .unwrap();
    let current = candidate
        .live_mapped_pitch_chunk(original, 1, intent, true)
        .unwrap()
        .unwrap();
    let feed = actual_test_feed(&candidate, original, 1);
    let mut output = vec![vec![0.0]; 2];
    candidate
        .render_mapped(&feed, &mut output, current)
        .unwrap();
    candidate.advance_retained_start_feed(1);
    let snapshot = MappedAdapterSnapshot::capture(&candidate);
    assert!(candidate.render_mapped(&feed, &mut output, stale).is_err());
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);
    assert!(
        candidate
            .live_mapped_pitch_chunk(original, 1, intent, true)
            .is_err()
    );
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);

    let mut next_logical = logical;
    assert!(next_logical.advance(1));
    next_logical
        .refresh_mapped_span(fixture.captured.mapped_geometry().unwrap())
        .unwrap();
    let mut next_plan = logical_plan;
    next_plan.frame_pos = next_logical.position().frame;
    next_plan.seek_mode = next_logical.position().seek_mode;
    let (playback, plan) = candidate
        .retained_start_feed(&next_logical, next_plan)
        .unwrap()
        .unwrap();
    let actual = SourceReadCheckpoint { playback, plan };
    let old_incarnation = candidate
        .live_mapped_pitch_chunk(actual, 1, intent, true)
        .unwrap()
        .unwrap();
    let empty = candidate
        .live_mapped_pitch_chunk(actual, 0, intent, true)
        .unwrap()
        .unwrap();
    candidate
        .render_mapped(
            &[Vec::new(), Vec::new()],
            &mut [Vec::new(), Vec::new()],
            empty,
        )
        .unwrap();
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);

    // Reuse the SAME immutable original request and Native handle. Prepare one real
    // source frame so the new owner reaches identical serial/source coordinates.
    assert!(
        candidate
            .mapped_projection_memo_keys_for_test()
            .unwrap()
            .iter()
            .all(|role| role.iter().any(Option::is_some))
    );
    let old_witness = candidate.mapped_pitch_witness_for_test().unwrap();
    let mut request = candidate.source.take().unwrap();
    request.actual_feed = None;
    candidate.reset_adapter();
    let next_cut = RetainedNativeStartCut {
        discarded_raw_output_frames: cut.discarded_raw_output_frames + 1,
        ..cut
    };
    candidate
        .prepare_owned_mapped_birth_future(request, next_cut)
        .unwrap();
    let new_witness = candidate.mapped_pitch_witness_for_test().unwrap();
    assert_eq!(
        (old_witness.0, old_witness.1, old_witness.3, old_witness.4),
        (new_witness.0, new_witness.1, new_witness.3, new_witness.4)
    );
    assert_eq!(new_witness.2, old_witness.2 + 1);
    let snapshot = MappedAdapterSnapshot::capture(&candidate);
    assert!(
        candidate
            .render_mapped(&feed, &mut output, old_incarnation)
            .is_err()
    );
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);

    // A different adapter receiving the same original request cannot consume this
    // adapter's descriptor, independently of any allocator address reuse.
    let (playback, plan) = candidate
        .retained_start_feed(&logical, logical_plan)
        .unwrap()
        .unwrap();
    let actual = SourceReadCheckpoint { playback, plan };
    let foreign_adapter_chunk = candidate
        .live_mapped_pitch_chunk(actual, 1, intent, true)
        .unwrap()
        .unwrap();
    let mut request = candidate.source.take().unwrap();
    request.actual_feed = None;
    let mut other = NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    other
        .prepare_owned_mapped_birth_future(request, next_cut)
        .unwrap();
    let other_witness = other.mapped_pitch_witness_for_test().unwrap();
    assert_ne!(other_witness.0, new_witness.0);
    assert_eq!(
        (other_witness.3, other_witness.4),
        (new_witness.3, new_witness.4)
    );
    let snapshot = MappedAdapterSnapshot::capture(&other);
    assert!(
        other
            .render_mapped(&feed, &mut output, foreign_adapter_chunk)
            .is_err()
    );
    assert_eq!(MappedAdapterSnapshot::capture(&other), snapshot);

    // Continue real reads after all refusals and compare the complete next output
    // to the independent C ABI, proving no hidden Native history was changed.
    let mut oracle = RawStereo::new(independent_long_coordinate_and_pitch(0, 1536).1);
    let expected: Vec<[f32; 2]> = (0..1024)
        .map(|frame| {
            let (coordinate, pitch) = independent_long_coordinate_and_pitch(frame, 1536);
            let left = coordinate.floor() as usize;
            let fraction = coordinate.fract() as f32;
            oracle.sample_at_pitch(
                std::array::from_fn(|channel| {
                    let a = source.samples[left * 2 + channel];
                    let b = source.samples[(left + 1) * 2 + channel];
                    a + (b - a) * fraction
                }),
                pitch,
            )
        })
        .collect();
    let (playback, plan) = other
        .retained_start_feed(&logical, logical_plan)
        .unwrap()
        .unwrap();
    let actual = SourceReadCheckpoint { playback, plan };
    let chunk = other
        .live_mapped_pitch_chunk(actual, 1023, intent, true)
        .unwrap()
        .unwrap();
    let feed = actual_test_feed(&other, actual, 1023);
    let mut output = vec![vec![0.0; 1023]; 2];
    other.render_mapped(&feed, &mut output, chunk).unwrap();
    other.advance_retained_start_feed(1023);
    for index in 0..1023 {
        for channel in 0..2 {
            assert!((output[channel][index] - expected[index + 1][channel]).abs() <= 3e-6);
        }
    }
    assert_original_current(&fixture.captured);
}

#[test]
fn owned_mapped_pitch_counter_limits_and_current_fifo_capacity_refuse_before_mutation() {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut, SourceReadCheckpoint,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::stretch_processor::FixedFifo;
    let fixture = owned_long_birth(1536);
    let logical = fixture.request.playback;
    let plan = fixture.request.plan;
    let intent = fixture.request.pitch_intent;
    let cut = RetainedNativeStartCut {
        history_output_frames: 4096,
        discarded_raw_output_frames: 4096 - (BLOCK - 1),
    };
    let mut candidate =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    candidate
        .prepare_owned_mapped_birth_future(fixture.request, cut)
        .unwrap();
    let (playback, plan) = candidate
        .retained_start_feed(&logical, plan)
        .unwrap()
        .unwrap();
    let actual = SourceReadCheckpoint { playback, plan };
    // Keep the real queued samples in a deliberately insufficient output capacity.
    // The rejection snapshot includes all new queue indices and storage. This is the
    // current valid original source, not a foreign-owner failure.
    let original_output_fifo = candidate.output_fifo.clone();
    for fifo in &mut candidate.output_fifo {
        let mut contents = vec![0.0; fifo.len()];
        let mut copy = fifo.clone();
        assert_eq!(copy.pop_into(&mut contents), contents.len());
        let mut insufficient = FixedFifo::new(contents.len());
        assert_eq!(insufficient.push_slice(&contents), contents.len());
        *fifo = insufficient;
    }
    let chunk = candidate
        .live_mapped_pitch_chunk(actual, BLOCK, intent, true)
        .unwrap()
        .unwrap();
    let snapshot = MappedAdapterSnapshot::capture(&candidate);
    let feed = actual_test_feed(&candidate, actual, BLOCK);
    let mut output = vec![vec![0.0; BLOCK]; 2];
    assert!(candidate.render_mapped(&feed, &mut output, chunk).is_err());
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);
    assert_original_current(&fixture.captured);
    // No input was accepted, so restore the identical pre-probe queues before
    // independently exercising numeric limits on otherwise usable fixed storage.
    candidate.output_fifo = original_output_fifo;

    let witness = candidate.mapped_pitch_witness_for_test().unwrap();
    let stale = candidate
        .live_mapped_pitch_chunk(actual, 1, intent, true)
        .unwrap()
        .unwrap();
    candidate.set_mapped_counter_limits_for_test(witness.2, u64::MAX, witness.4);
    let snapshot = MappedAdapterSnapshot::capture(&candidate);
    assert!(!candidate.mapped_next_counters_fit_for_test(1, false));
    assert!(!candidate.mapped_next_counters_fit_for_test(1, true));
    assert!(
        candidate
            .live_mapped_pitch_chunk(actual, 1, intent, true)
            .is_err()
    );
    assert!(candidate.render_mapped(&feed, &mut output, stale).is_err());
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);
    let empty = candidate
        .live_mapped_pitch_chunk(actual, 0, intent, true)
        .unwrap()
        .unwrap();
    candidate
        .render_mapped(
            &[Vec::new(), Vec::new()],
            &mut [Vec::new(), Vec::new()],
            empty,
        )
        .unwrap();
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);
    candidate.set_mapped_counter_limits_for_test(witness.2, 0, u64::MAX);
    let snapshot = MappedAdapterSnapshot::capture(&candidate);
    assert!(!candidate.mapped_next_counters_fit_for_test(1, false));
    assert!(candidate.mapped_next_counters_fit_for_test(1, true));
    assert!(
        candidate
            .live_mapped_pitch_chunk(actual, 1, intent, true)
            .is_err()
    );
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);

    candidate.set_mapped_counter_limits_for_test(u64::MAX, 0, witness.4);
    let mut request = candidate.source.take().unwrap();
    request.actual_feed = None;
    let snapshot = MappedAdapterSnapshot::capture(&candidate);
    assert!(
        candidate
            .prepare_owned_mapped_birth_future(request, cut)
            .is_err()
    );
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), snapshot);
    assert_original_current(&fixture.captured);

    let unavailable = owned_long_birth(1536);
    let mut exhausted =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    exhausted.exhaust_mapped_adapter_id_for_test();
    let snapshot = MappedAdapterSnapshot::capture(&exhausted);
    assert!(
        exhausted
            .prepare_owned_mapped_birth_future(unavailable.request, cut)
            .is_err()
    );
    assert_eq!(MappedAdapterSnapshot::capture(&exhausted), snapshot);
    assert_original_current(&unavailable.captured);
}

fn owned_long_birth(knot: usize) -> OwnedBirth {
    use crate::audio_engine::prepared_native_history::{MappedNativeInput, SourceReadCheckpoint};
    let pcm: Vec<i16> = (0..8193)
        .map(|index| (((index * 3251 + 173) % 20001) as i32 - 10000) as i16)
        .collect();
    let mut source = ColdCustody::new_selected_with_signal(None, &pcm);
    source.engine.input_runtime = Some(
        crate::audio_engine::input_mapping::InputRuntime::new_with_ownership(
            source.producer.clone(),
            crate::audio_engine::timing::InputClock::new(),
            source.engine.input_runtime_ownership.clone(),
        ),
    );
    let mut path = LaunchPath::from_source(source);
    let mut expected = record(&path.source, 0);
    let accepted = &mut expected["record"]["accepted"];
    let first = accepted["anchors"][0].clone();
    let mut middle = accepted["anchors"][1].clone();
    middle["id"] = json!("long-block-boundary");
    middle["source_seconds_bits"] = json!(bits(knot as f64 / f64::from(RATE)));
    middle["beat_bits"] = json!(bits(knot as f64 / 64.0));
    let mut last = accepted["anchors"][1].clone();
    last["source_seconds_bits"] = json!(bits(covered_terminal_seconds(8192)));
    last["beat_bits"] = json!(bits(knot as f64 / 64.0 + (8192 - knot) as f64 / 48.0));
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
    path.render(1);
    let captured = path
        .source
        .engine
        .capture_pad_start(
            0,
            0.0,
            Some(8192.0 / f64::from(RATE)),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    drain_grid(&mut path);
    assert_eq!(captured.target_frame(), Some(64));
    assert_original_current(&captured);
    let mut work = captured.take_work(None).unwrap().unwrap();
    work.freeze_mapped_clock(Some(path.timeline), 64, path.timeline.output_frame())
        .unwrap();
    work.freeze_frames(64, 64).unwrap();
    let target = work.execution_frame();
    let mut snapshot = path
        .source
        .mixer
        .normal_start_snapshot_for_test(0, 0.0, Some(8192.0 / f64::from(RATE)))
        .unwrap();
    let birth = original_birth_checkpoint(&work, &snapshot);
    snapshot.playback = birth.playback;
    snapshot.plan = birth.plan;
    assert_eq!(snapshot.sample.frame_count(), 8193);
    assert_eq!(snapshot.plan.loop_region.end, 8192);
    long_oracle(knot).assert_physical_quarters(captured.mapped_geometry().unwrap());
    work.freeze_launch_origin(SourceReadCheckpoint {
        playback: snapshot.playback,
        plan: snapshot.plan,
    })
    .unwrap();
    let permit = work.capture_history_permit(&snapshot.sample, RATE).unwrap();
    let mut request = snapshot
        .into_request(
            permit,
            PitchIntent::default(),
            true,
            target,
            work.sequence(),
            work.epoch(),
        )
        .unwrap();
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

fn independent_long_coordinate_and_pitch(frame: usize, knot: usize) -> (f64, f64) {
    let (source, rate) = long_oracle(knot).at_frame(frame);
    (source, 1.0 / rate)
}

fn long_oracle(knot: usize) -> &'static OracleMap {
    static MAPS: std::sync::OnceLock<[OracleMap; 4]> = std::sync::OnceLock::new();
    let knots = [511, 512, 513, 1536];
    let maps = MAPS.get_or_init(|| {
        knots.map(|knot| {
            OracleMap::new(
                &[
                    (0.0, 0.0),
                    (knot as f64 / f64::from(RATE), knot as f64 / 64.0),
                    (
                        covered_terminal_seconds(8192),
                        knot as f64 / 64.0 + (8192 - knot) as f64 / 48.0,
                    ),
                ],
                0,
                8192,
            )
        })
    });
    &maps[knots
        .iter()
        .position(|candidate| *candidate == knot)
        .unwrap()]
}

#[test]
fn owned_mapped_pitch_long_span_feeds_two_blocks_and_knots_bracket_block_heads() {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_feed_history::SourceReadDomain;
    use crate::audio_engine::stretch_processor::{ProductiveSourceFeed, StretchProcessor};
    for knot in [511, 512, 513, 1536] {
        let mut partition_reference = None;
        for partitions in [&[1024][..], &[127, 1, 383, 17, 256][..]] {
            let fixture = owned_long_birth(knot);
            let source = fixture.request.sample.clone();
            let stems = fixture.request.stems.clone();
            let accepted = fixture.request.binding.accepted;
            let mut playback = fixture.request.playback;
            let mut plan = fixture.request.plan;
            let initial_pitch = independent_long_coordinate_and_pitch(0, knot).1;
            let mut oracle = RawStereo::new(initial_pitch);
            let history = oracle.history;
            let observation = history + CONTINUATION_SUFFIX;
            let expected: Vec<f32> = (0..observation)
                .flat_map(|frame| {
                    let (coordinate, pitch) = independent_long_coordinate_and_pitch(frame, knot);
                    let left = coordinate.floor() as usize % 8192;
                    let right = (left + 1) % 8192;
                    let fraction = coordinate.fract() as f32;
                    let input = std::array::from_fn(|channel| {
                        let a = source.samples[left * 2 + channel];
                        let b = source.samples[right * 2 + channel];
                        a + (b - a) * fraction
                    });
                    oracle.sample_at_pitch(input, pitch)
                })
                .collect();
            let mut candidate = Box::new(NativeAdapterState::new(
                Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
                2,
            ));
            candidate
                .prepare_owned_mapped_birth_future(
                    fixture.request,
                    RetainedNativeStartCut {
                        history_output_frames: history,
                        discarded_raw_output_frames: history - (BLOCK - 1),
                    },
                )
                .unwrap();
            let native = candidate.rubberband.as_ref().unwrap().state_address();
            let mut processor = StretchProcessor::new(2);
            processor.reset();
            assert!(processor.adopt_prepared_pitch(
                &mut candidate,
                &ProductiveSourceFeed {
                    pitch_intent: PitchIntent::default(),
                    key_lock: true,
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
            let mut actual = Vec::with_capacity(observation * 2);
            let mut emitted = 0;
            let mut caller = 0;
            let mut whole_1024 = 0;
            while emitted < observation {
                let end = (emitted + partitions[caller % partitions.len()]).min(observation);
                while emitted < end {
                    playback
                        .refresh_mapped_span(fixture.captured.mapped_geometry().unwrap())
                        .unwrap();
                    let position = playback.position();
                    plan.frame_pos = position.frame;
                    plan.seek_mode = position.seek_mode;
                    let max = playback.chunk(end - emitted).0;
                    let frames = processor.retained_source_chunk_limit(&playback, plan, max);
                    assert!(frames > 0);
                    whole_1024 += usize::from(frames == 1024);
                    let receipt = processor
                        .process_source(
                            ProductiveSourceFeed {
                                pitch_intent: PitchIntent::default(),
                                key_lock: true,
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
                            true,
                        )
                        .unwrap();
                    assert_eq!(receipt.domain, SourceReadDomain::NativeInput);
                    assert_eq!(receipt.output_frames, frames);
                    assert_eq!(
                        receipt.playback.mapped_output_frame(),
                        Some(64 + emitted as u64)
                    );
                    assert_eq!(
                        receipt.playback.tempo_ratio().to_bits(),
                        playback.tempo_ratio().to_bits()
                    );
                    for offset in 0..frames {
                        let position = receipt.playback.position_at(offset).unwrap();
                        let coordinate =
                            independent_long_coordinate_and_pitch(emitted + offset, knot).0;
                        assert!(
                            (position.frame as f64 + position.fraction - coordinate).abs() < 1e-8
                        );
                        actual.extend([
                            processor.output_buffers()[0][offset],
                            processor.output_buffers()[1][offset],
                        ]);
                    }
                    assert!(playback.advance(frames));
                    emitted += frames;
                    let head = (emitted / BLOCK).checked_sub(1).map(|block| block * BLOCK);
                    let expected_pitch = head.map_or(initial_pitch, |frame| {
                        independent_long_coordinate_and_pitch(frame, knot).1
                    });
                    assert_eq!(
                        processor.native_pitch_scale_observation(),
                        Some(expected_pitch)
                    );
                    assert_eq!(processor.native_state_address(), native);
                }
                caller += 1;
            }
            if knot == 1536 && partitions.len() == 1 {
                assert!(
                    whole_1024 > 0,
                    "a single qualified span must actually feed two Native blocks"
                );
            }
            let error = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(
                error <= 3e-6,
                "knot={knot}, partitions={partitions:?}, error={error}"
            );
            if let Some(reference) = &partition_reference {
                assert_eq!(&actual, reference);
            } else {
                partition_reference = Some(actual);
            }
            assert_eq!(
                processor.pending_fifo_frames(),
                (oracle.input.len(), oracle.output.len())
            );
            assert!(fixture.captured.effective_frame().is_none());
            assert_eq!(sync_feedback(&fixture.path).unwrap()["effective"], false);
        }
    }
}

fn assert_owned_mapped_stream(
    shift: i32,
    key_lock: bool,
    unity_head: bool,
    lead: usize,
    silent_tail: usize,
) {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_feed_history::{SourceFeedHistory, SourceReadDomain};
    use crate::audio_engine::stretch_processor::{ProductiveSourceFeed, StretchProcessor};

    // These leads are declared before observing output, independently of Native delay.
    let mut partition_reference = None;
    for partitions in [&[512][..], &[127, 1, 383, 17, 256][..]] {
        let fixture = owned_birth_with_mode(shift, key_lock, unity_head);
        let source = fixture.request.sample.clone();
        let stems = fixture.request.stems.clone();
        let accepted = fixture.request.binding.accepted;
        let intent = fixture.request.pitch_intent;
        let mut playback = fixture.request.playback;
        let mut plan = fixture.request.plan;
        let pitch = independent_pitch(0, shift, key_lock, unity_head);
        assert_eq!(
            intent
                .native_pitch_scale(fixture.request.playback.tempo_ratio(), key_lock)
                .unwrap()
                .to_bits(),
            pitch.to_bits()
        );
        if unity_head {
            assert_eq!(pitch, 1.0);
            assert!((1.49..1.51).contains(&independent_pitch(20, shift, key_lock, true)));
            assert_eq!(independent_pitch(48, shift, key_lock, true), 1.0);
        }
        let mut oracle = RawStereo::new(pitch);
        let history_frames = oracle.history + silent_tail;
        for _ in 0..silent_tail {
            assert_eq!(oracle.sample_at_pitch([0.0; 2], pitch), [0.0; 2]);
        }
        let raw_cut = history_frames - (BLOCK - 1) + lead;
        let observation = oracle.history.max(oracle.delay) + CONTINUATION_SUFFIX;
        // Preserve ALL output before the declared cut as separate prefix evidence.
        // In particular a nonzero early prefix must not disappear from the oracle.
        let raw_stream: Vec<f32> = (0..lead + observation)
            .flat_map(|frame| {
                oracle.sample_at_pitch(
                    independent_input_with_shape(&source, frame, unity_head),
                    independent_pitch(frame, shift, key_lock, unity_head),
                )
            })
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
                key_lock,
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
        if key_lock {
            processor.process_resampled_with_pitch(1, 1.0, key_lock, intent);
            assert!(!processor.wet_render_ready());
            assert_eq!(processor.pending_fifo_frames(), initial_fifos);
            assert_eq!(
                processor.retained_mapped_output_coordinates().unwrap(),
                Some(initial_coordinates)
            );
            assert_eq!(processor.native_state_address(), native);
        }
        let bound = processor.retained_source_chunk_limit(&playback, plan, 1024);
        for (bad_output, bad_frames) in [(Some(65), 1), (Some(64), bound + 1)] {
            assert!(
                processor
                    .process_source(
                        ProductiveSourceFeed {
                            pitch_intent: intent,
                            key_lock,
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
                        key_lock,
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
                let frames = processor.retained_source_chunk_limit(&playback, plan, logical_bound);
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
                            key_lock,
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
                    let expected =
                        independent_coordinate_with_shape(lead + emitted + offset, unity_head);
                    assert!((position.frame as f64 + position.fraction - expected).abs() < 1e-9);
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
                let last_head = ((history_frames + lead + emitted) / BLOCK - 1) * BLOCK;
                let expected_pitch = if last_head < history_frames {
                    pitch
                } else {
                    independent_pitch(last_head - history_frames, shift, key_lock, unity_head)
                };
                assert_eq!(
                    processor.native_pitch_scale_observation(),
                    Some(expected_pitch)
                );
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

#[test]
fn owned_mapped_numeric_projection_memo_preserves_cold_hits_errors_and_exclusive_endpoints() {
    use crate::audio_engine::prepared_native_history::{SourceReadCheckpoint, mapped_rate_probe};
    let fixture = owned_long_birth(1536);
    let input = fixture.request.mapped_input.as_ref().unwrap();
    let original = SourceReadCheckpoint {
        playback: fixture.request.playback,
        plan: fixture.request.plan,
    };
    let start = original.playback.mapped_output_frame().unwrap();
    input.projection_memo_clear_for_test();
    assert_eq!(input.projection_memo_keys_for_test(), [[None; 2]; 2]);
    for frame in [
        start,
        start + 1,
        start + 1535,
        start + 1536,
        start + 8192,
        start,
    ] {
        let reference = input.projection_pair_for_test(frame, false);
        mapped_rate_probe::start();
        let cold = input.projection_pair_for_test(frame, true);
        let first = mapped_rate_probe::finish();
        assert_eq!(first.projection_hits + first.projection_misses, 2);
        for (actual, expected) in cold.into_iter().zip(reference) {
            assert!(actual.unwrap().matches_exact(expected.unwrap()));
        }
        mapped_rate_probe::start();
        let hit = input.projection_pair_for_test(frame, true);
        let counts = mapped_rate_probe::finish();
        assert_eq!((counts.projection_hits, counts.projection_misses), (2, 0));
        for (actual, expected) in hit
            .into_iter()
            .zip(input.projection_pair_for_test(frame, false))
        {
            assert!(actual.unwrap().matches_exact(expected.unwrap()));
        }
    }
    let keys = input.projection_memo_keys_for_test();
    for frame in [start - 1, u64::MAX, u64::MAX] {
        let actual = input.projection_pair_for_test(frame, true);
        let reference = input.projection_pair_for_test(frame, false);
        for (actual, expected) in actual.into_iter().zip(reference) {
            assert_eq!(
                actual.err().unwrap().to_string(),
                expected.err().unwrap().to_string()
            );
        }
        assert_eq!(
            input.projection_memo_keys_for_test(),
            keys,
            "errors are not memoized"
        );
    }
    let mut exhausted = original;
    let frames = exhausted.playback.chunk(usize::MAX).0;
    assert!(frames > 0);
    assert!(exhausted.playback.advance(frames));
    exhausted.plan.frame_pos = exhausted.playback.position().frame;
    exhausted.plan.seek_mode = exhausted.playback.position().seek_mode;
    assert!(!exhausted.playback.can_read_frames(1));
    let expected = input.projection_pair_for_test(start + frames as u64, false)[1]
        .as_ref()
        .unwrap()
        .to_owned();
    input.project(&mut exhausted, 0).unwrap();
    assert!(exhausted.matches_exact(expected));
    let mut forged = exhausted;
    forged.plan.frame_pos += 1;
    let before = forged;
    assert!(input.project(&mut forged, 0).is_err());
    assert!(forged.matches_exact(before));
    assert_original_current(&fixture.captured);
}

#[test]
fn owned_mapped_numeric_projection_memo_does_not_cross_new_request_or_geometry_owners() {
    use crate::audio_engine::prepared_native_history::mapped_rate_probe;
    let old = owned_long_birth(511);
    let input = old.request.mapped_input.as_ref().unwrap();
    let frame = old.request.playback.mapped_output_frame().unwrap() + 2048;
    let old_values = input
        .projection_pair_for_test(frame, true)
        .map(Result::unwrap);
    assert_eq!(input.projection_memo_keys_for_test()[0][1], Some(frame));
    drop(old);

    let fresh = owned_long_birth(1536);
    let input = fresh.request.mapped_input.as_ref().unwrap();
    // Genuine capture can warm its own Current origin. No previous nonorigin
    // endpoint may survive a fresh request, even if the allocator reuses storage.
    assert!(
        input
            .projection_memo_keys_for_test()
            .iter()
            .flatten()
            .all(|key| *key != Some(frame))
    );
    let expected = input
        .projection_pair_for_test(frame, false)
        .map(Result::unwrap);
    assert!(!expected[0].matches_exact(old_values[0]));
    mapped_rate_probe::start();
    let actual = input
        .projection_pair_for_test(frame, true)
        .map(Result::unwrap);
    let first = mapped_rate_probe::finish();
    assert_eq!((first.projection_hits, first.projection_misses), (0, 2));
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(actual.matches_exact(expected));
    }
    mapped_rate_probe::start();
    let hit = input
        .projection_pair_for_test(frame, true)
        .map(Result::unwrap);
    let counts = mapped_rate_probe::finish();
    assert_eq!((counts.projection_hits, counts.projection_misses), (2, 0));
    for (actual, expected) in hit.into_iter().zip(expected) {
        assert!(actual.matches_exact(expected));
    }
    assert_original_current(&fresh.captured);
}

#[test]
fn owned_mapped_future_projection_requires_original_checkpoint_and_fails_atomically() {
    use crate::audio_engine::mapped_source_clock::MappedLoopGeometry;
    use crate::audio_engine::prepared_native_history::{
        MappedNativeInput, NativeAdapterState, RetainedNativeStartCut, SourceReadCheckpoint,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_beatmap::SourceBeatmap;
    use crate::audio_engine::source_reader::{StemRenderSelection, StemTransition};

    // Dedicated exact dyadic axes permit a changed interior excursion which
    // rejoins the original line before physical Z64. Both A/Z inverses stay exact.
    let path = configured(7);
    let points = [(0.0, 0.0), (1.0 / 1024.0, 0.5), (1.0 / 512.0, 1.5)];
    let mut expected = positive_launch_record(&path);
    let template = expected["record"]["accepted"]["anchors"][0].clone();
    let anchor = |id: &str, seconds: f64, beat: f64| {
        let mut value = template.clone();
        value["id"] = json!(id);
        value["source_seconds_bits"] = json!(bits(seconds));
        value["beat_bits"] = json!(bits(beat));
        value
    };
    expected["record"]["accepted"]["anchors"] = json!([
        anchor("start", points[0].0, points[0].1),
        anchor("middle", points[1].0, points[1].1),
        anchor("end", points[2].0, points[2].1),
    ]);
    let interval = expected["record"]["accepted"]["intervals"][0].clone();
    expected["record"]["accepted"]["intervals"] = json!([interval.clone(), interval.clone()]);
    seal(&mut expected);
    let (job, delivery) = positive_launch_delivery(&path, &expected);
    let mut fixture = finish_owned_birth(path, job, delivery, 7, false);
    let oracle = OracleMap::new(&points, 32, 64);
    oracle.assert_physical_quarters(fixture.captured.mapped_geometry().unwrap());
    let input = fixture.request.mapped_input.take().unwrap();
    let original = SourceReadCheckpoint {
        playback: fixture.request.playback,
        plan: fixture.request.plan,
    };
    // Cache the genuine canonical result first; a hit cannot authorize an impostor.
    assert!(
        input
            .projection_pair_for_test(64, true)
            .iter()
            .all(Result::is_ok)
    );
    let mut copied = original;
    assert!(!copied.project_output_frames(1));
    assert!(copied.matches_exact(original));
    let scalar = fixture
        .path
        .source
        .mixer
        .normal_start_snapshot_for_test(0, 32.0 / f64::from(RATE), Some(64.0 / f64::from(RATE)))
        .unwrap();
    let scalar = SourceReadCheckpoint {
        playback: scalar.playback,
        plan: scalar.plan,
    };
    let birth = |geometry: &MappedLoopGeometry, marker| {
        crate::audio_engine::mapped_birth::playback_at_birth(
            geometry,
            marker,
            fixture.work.mapped_timeline.unwrap(),
            scalar,
            64,
            fixture.work.launch_reference(),
        )
        .unwrap()
    };
    for corrupt in [0, 1, 2, 3] {
        let mut checkpoint = original;
        match corrupt {
            0 => {
                checkpoint = birth(
                    fixture.captured.mapped_geometry().unwrap(),
                    fixture.work.sequence() + 1,
                );
                assert_eq!(checkpoint.playback.position(), original.playback.position());
                assert_eq!(checkpoint.plan.domain(), original.plan.domain());
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
    accepted["anchors"]
        .as_array_mut()
        .unwrap()
        .insert(2, anchor("foreign-excursion", 9.0 / 8192.0, 19.0 / 32.0));
    accepted["anchors"]
        .as_array_mut()
        .unwrap()
        .insert(3, anchor("original-line-return", 5.0 / 4096.0, 0.75));
    accepted["intervals"] = json!(vec![interval; 4]);
    seal(&mut foreign);
    let foreign = MappedLoopGeometry::new_general_physical(
        SourceBeatmap::parse(&foreign.to_string()).unwrap(),
        geometry.domain().sample_frames,
        RATE,
        geometry.domain().region,
        false,
    )
    .unwrap();
    let foreign_oracle = OracleMap::new(
        &[
            points[0],
            points[1],
            (9.0 / 8192.0, 19.0 / 32.0),
            (5.0 / 4096.0, 0.75),
            points[2],
        ],
        32,
        64,
    );
    assert_eq!(oracle.inverse(32), foreign_oracle.inverse(32));
    assert_eq!(oracle.inverse(64), foreign_oracle.inverse(64));
    assert_eq!(geometry.domain(), foreign.domain());
    foreign_oracle.assert_physical_quarters(&foreign);
    let mut supplied = birth(&foreign, fixture.work.sequence());
    assert_eq!(supplied.plan.domain(), original.plan.domain());
    assert_eq!(supplied.playback.position(), original.playback.position());
    assert!(!supplied.matches_exact(original));
    let impostor = supplied;
    assert!(input.project(&mut supplied, 20).is_err());
    assert!(supplied.matches_exact(impostor));
    supplied = original;
    input.project(&mut supplied, 20).unwrap();
    let position = supplied.playback.position();
    assert!((position.frame as f64 + position.fraction - oracle.at_frame(20).0).abs() < 1e-9);
    let mut wrong = impostor.playback;
    wrong.project_mapped_compact_to(&foreign, 84).unwrap();
    assert!(!supplied.playback.matches_exact(&wrong));
    fixture.request.playback = wrong;
    fixture.request.plan.frame_pos = wrong.position().frame;
    assert!(MappedNativeInput::capture(&fixture.work, &fixture.request).is_err());
    // The same valid current owner also refuses the equal-domain foreign first
    // checkpoint before changing any working buffer, queued FIFO or Native state.
    fixture.request.playback = original.playback;
    fixture.request.plan = original.plan;
    fixture.request.mapped_input = Some(input);
    let intent = fixture.request.pitch_intent;
    let raw = RawStereo::new(2.0_f64.powf(7.0 / 12.0));
    let mut candidate =
        NativeAdapterState::new(Some(RubberBandLiveShifter::new(RATE, 2).unwrap()), 2);
    candidate
        .prepare_owned_mapped_birth_future(
            fixture.request,
            RetainedNativeStartCut {
                history_output_frames: raw.history,
                discarded_raw_output_frames: raw.history - (BLOCK - 1),
            },
        )
        .unwrap();
    let before = MappedAdapterSnapshot::capture(&candidate);
    assert!(
        candidate
            .live_mapped_pitch_chunk(impostor, 1, intent, false)
            .is_err()
    );
    assert_eq!(MappedAdapterSnapshot::capture(&candidate), before);
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
        // Numeric hits must never preserve an old cancellation, ACK or source permit.
        let input = fixture.request.mapped_input.as_ref().unwrap();
        let frame = fixture.request.playback.mapped_output_frame().unwrap();
        assert!(
            input
                .projection_pair_for_test(frame, true)
                .iter()
                .all(Result::is_ok)
        );
        assert!(
            input
                .projection_pair_for_test(frame + 1, true)
                .iter()
                .all(Result::is_ok)
        );
        assert!(
            input
                .projection_memo_keys_for_test()
                .iter()
                .flatten()
                .all(Option::is_some)
        );
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
            assert!(
                candidate
                    .mapped_projection_memo_keys_for_test()
                    .unwrap()
                    .iter()
                    .all(|role| role.iter().any(Option::is_some))
            );
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
    {
        let request = candidate.source.as_ref().unwrap();
        let frame = request.playback.mapped_output_frame().unwrap();
        let input = request.mapped_input.as_ref().unwrap();
        assert!(
            input
                .projection_pair_for_test(frame, true)
                .iter()
                .all(Result::is_ok)
        );
        assert!(
            input
                .projection_pair_for_test(frame + 1, true)
                .iter()
                .all(Result::is_ok)
        );
    }
    let warm_keys = candidate.mapped_projection_memo_keys_for_test().unwrap();
    assert!(
        warm_keys
            .iter()
            .all(|role| role.iter().any(Option::is_some))
    );
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
    assert_eq!(
        candidate.mapped_projection_memo_keys_for_test(),
        Some(warm_keys)
    );
    assert!(weak_map.upgrade().is_some());
    assert!(weak_pcm.upgrade().is_some());
    drop(candidate.source.take());
    assert!(weak_map.upgrade().is_none());
    assert!(weak_pcm.upgrade().is_none());
}

#[test]
fn internal_fixed_h_birth_keeps_native_stream_canonical_receipts_and_complete_prefix() {
    use crate::audio_engine::prepared_native_history::{
        NativeAdapterState, RetainedNativeStartCut,
    };
    use crate::audio_engine::rubberband_backend::RubberBandLiveShifter;
    use crate::audio_engine::source_feed_history::{SourceFeedHistory, SourceReadDomain};
    use crate::audio_engine::stretch_processor::{ProductiveSourceFeed, StretchProcessor};

    for shift in [7, -23, 24] {
        let mut reference = None;
        for partitions in [&[512][..], &[127, 1, 383, 17, 256][..]] {
            let OwnedBirth {
                path,
                job: _job,
                delivery: _delivery,
                captured,
                work: _work,
                request,
            } = owned_birth(shift);
            // This genuine capture retains the original source/artifact/geometry pins
            // until after the component's last read and off-callback destruction.
            // It is never published as a voice or reported as an effective SYNC start.
            assert_eq!(captured.target_frame(), Some(64));
            assert_original_current(&captured);
            let geometry = captured.mapped_geometry().unwrap();
            let (beginning, quarters) = independent_extent();
            tiny_oracle(true, false).assert_physical_quarters(geometry);
            let epoch = request.playback.mapped_epoch().unwrap();
            let source = request.sample.clone();
            let stems = request.stems.clone();
            let accepted = request.binding.accepted;
            let mut playback = request.playback;
            let mut plan = request.plan;
            let intent =
                PitchIntent::new(shift.clamp(-23, 23), shift - shift.clamp(-23, 23)).unwrap();
            let mut processor = StretchProcessor::new(2);
            let preparation_epoch = request.epoch;
            let pitch = 2.0_f64.powf(f64::from(shift) / 12.0);
            let mut oracle = RawStereo::new(pitch);
            let mut candidate = Box::new(NativeAdapterState::new(
                Some(RubberBandLiveShifter::new(RATE, 2).unwrap()),
                2,
            ));
            assert_original_current(&captured);
            candidate
                .prepare_owned_mapped_birth_future(
                    request,
                    RetainedNativeStartCut {
                        history_output_frames: oracle.history,
                        discarded_raw_output_frames: oracle.history - (BLOCK - 1),
                    },
                )
                .unwrap();
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
            assert!(processor.has_retained_source_input());
            let endpoint = playback.position();
            plan.frame_pos = endpoint.frame;
            plan.seek_mode = endpoint.seek_mode;
            let actual_checkpoint = processor
                .actual_source_checkpoint(&playback, plan)
                .unwrap()
                .unwrap();
            let plain = crate::audio_engine::prepared_native_history::SourceReadCheckpoint {
                playback,
                plan,
            };
            if !actual_checkpoint.matches_exact(plain) {
                assert!(!playback.can_read_frames(1));
            }
            // Native retains the canonical next span. A caller's final advance may
            // leave an exhausted span; refresh only a copy through the original map.
            let mut canonical = playback;
            canonical.refresh_mapped_span(geometry).unwrap();
            assert_eq!(canonical.position(), endpoint);
            assert_eq!(
                canonical.position().fraction.to_bits(),
                endpoint.fraction.to_bits()
            );
            assert_eq!(
                canonical.mapped_output_frame(),
                playback.mapped_output_frame()
            );
            assert!(
                canonical
                    .mapped_epoch()
                    .unwrap()
                    .matches_exact(playback.mapped_epoch().unwrap())
            );
            assert_eq!(canonical.effective_domain(), playback.effective_domain());
            assert_eq!(
                canonical.original_scalar_domain(),
                playback.original_scalar_domain()
            );
            let mut reference_check = canonical;
            reference_check.carry_launch_reference_from(&playback);
            assert!(reference_check.matches_exact(&canonical));
            let mut canonical_plan = plan;
            canonical_plan.frame_pos = canonical.position().frame;
            canonical_plan.seek_mode = canonical.position().seek_mode;
            assert!(actual_checkpoint.matches_exact(
                crate::audio_engine::prepared_native_history::SourceReadCheckpoint {
                    playback: canonical,
                    plan: canonical_plan,
                }
            ));
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
fn public_fixed_h_future_birth_preserves_complete_beginning_and_requested_alignment() {
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
        wait_prepared(&mut path, &start);
        assert!(start.effective_frame().is_none());
        let execution = start.execution_frame().unwrap();
        assert!(execution >= start.target_frame().unwrap());
        path.render((execution - path.timeline.output_frame()) as usize);
        path.render(1);
        assert_eq!(start.state(), "applied", "{:?}", start.reason());
        assert_eq!(start.effective_frame(), Some(execution));
        assert!(
            path.source
                .mixer
                .voices
                .iter()
                .any(|voice| voice.active && voice.sample_id == 0)
        );
        assert_eq!(scalar_generation(&path), previous);
        let status = sync_feedback(&path).unwrap();
        assert_eq!(status["effective"], true);
        assert_eq!(status["birth"]["emitted"], true);
        assert_eq!(
            status["birth"]["requested"],
            if attack_policy {
                "attack_at_grid"
            } else {
                "first_sound_at_grid"
            }
        );
        assert_eq!(
            status["birth"]["alignment"],
            if attack_policy {
                "pending"
            } else {
                "earliest_bound_met"
            }
        );
        assert_eq!(
            status["birth"]["reason"],
            if attack_policy {
                json!("no_qualified_anchor")
            } else {
                json!(null)
            }
        );
        assert_eq!(status["phase_program_current"], false);
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
fn public_dry_mixed_group_retirement_pressure_is_atomic_and_reprepares_original_beginning() {
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
            assert_ne!(captured.state(), "rejected", "{:?}", captured.reason());
            assert!(
                members
                    .iter()
                    .all(|member| member.effective_frame().is_none())
            );
            assert_eq!(scalar_generation(&path), previous);
            wait_prepared(&mut path, &members[0]);
            let execution = members[0].execution_frame().unwrap();
            assert!(execution > 64);
            while path.timeline.output_frame() < execution {
                let frames = (execution - path.timeline.output_frame()).min(512) as usize;
                path.render_with_retirement(frames, retirement.owner.state_mut());
            }
            path.render_with_retirement(1, retirement.owner.state_mut());
            assert_eq!(captured.state(), "applied", "{:?}", captured.reason());
            for member in &members {
                assert_eq!(member.target_frame(), Some(64));
                assert_eq!(member.execution_frame(), Some(execution));
                assert_eq!(member.effective_frame(), Some(execution));
            }
            let mut history = SourceFeedSnapshot::new();
            assert!(path.voice().source_feed_history.capture_into(&mut history));
            let first = history.spans().next().unwrap();
            assert_eq!(first.playback.position().frame, 32);
            assert_eq!(
                first.playback.position().fraction.to_bits(),
                0.0_f64.to_bits()
            );
            assert_eq!(sync_feedback(&path).unwrap()["birth"]["emitted"], true);
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
