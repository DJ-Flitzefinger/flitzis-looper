//! Real segmented render/FIFO/retirement composition, without CPAL or hearing claims.

use super::*;

#[path = "callback_tests/signal_tests.rs"]
mod signal_tests;
#[path = "callback_tests/target_tests.rs"]
mod target_tests;
use crate::audio_engine::audio_stream::{drain_control_messages, render_scheduled_audio};
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirementWorker, CallbackCloseOwner, RtAudioBufferRetirement,
    create_audio_buffer_retirement_with_callback_paused,
};
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::mix_capture::{
    CaptureConfig, CaptureControl, CaptureStatus, SPOOL_HEADER_BYTES, prepare,
};
use crate::audio_engine::scheduler::ScheduledCommand;
use crate::audio_engine::transport::TransportTimeline;
use crate::messages::{AudioMessage, SampleBuffer, TriggerQuantization};
use rtrb::{Consumer, Producer, RingBuffer};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);

struct CallbackState {
    mixer: Box<RtMixer>,
    scheduler: FixedCapacityScheduler<8>,
    transport: TransportTimeline,
    mode: TriggerQuantization,
    consumer: Consumer<ControlMessage>,
    messages: Vec<AudioMessage>,
    retirement: RtAudioBufferRetirement,
}

struct Rig {
    owner: Option<CallbackCloseOwner<CallbackState>>,
    worker: Option<AudioBufferRetirementWorker>,
    worker_gate: Arc<AtomicBool>,
    producer: Producer<ControlMessage>,
}

impl Rig {
    fn new(paused: bool) -> Self {
        let (producer, consumer) = RingBuffer::new(16);
        let worker_gate = Arc::new(AtomicBool::new(!paused));
        let gate = worker_gate.clone();
        let (owner, worker) = create_audio_buffer_retirement_with_callback_paused(
            |retirement| {
                Box::new(CallbackState {
                    mixer: Box::new(RtMixer::new(2, 48_000.0)),
                    scheduler: FixedCapacityScheduler::new(),
                    transport: TransportTimeline::new(48_000),
                    mode: TriggerQuantization::Immediate,
                    consumer,
                    messages: Vec::with_capacity(64),
                    retirement,
                })
            },
            move || {
                while !gate.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            },
        )
        .unwrap();
        Self {
            owner: Some(owner),
            worker: Some(worker),
            worker_gate,
            producer,
        }
    }

    fn state(&mut self) -> &mut CallbackState {
        self.owner.as_mut().unwrap().state_mut()
    }

    fn send(&mut self, message: ControlMessage) {
        self.producer.push(message).unwrap();
    }

    fn start(&mut self, take: &Take, frame: u64) {
        self.send(ControlMessage::StartMixCapture {
            publication: take.publication.clone(),
            target_frame: MixCaptureTarget::At(frame),
        });
    }

    fn stop(&mut self, take: &Take, frame: u64) {
        self.send(ControlMessage::StopMixCapture {
            session_id: take.id(),
            target_frame: MixCaptureTarget::At(frame),
        });
    }

    fn drain(&mut self) -> usize {
        let state = self.state();
        drain_control_messages(
            &mut state.consumer,
            &mut state.scheduler,
            state.transport.output_frame(),
            &mut state.mode,
            &mut state.transport,
            &mut state.mixer,
            &mut state.messages,
            &mut state.retirement,
        )
    }

    fn render(&mut self, output: &mut [f32]) {
        let state = self.state();
        let mut peaks = [0.0; NUM_SAMPLES];
        render_scheduled_audio(
            &mut state.mixer,
            &mut state.scheduler,
            output,
            &mut peaks,
            state.transport.output_frame(),
            2,
            &mut state.transport,
            &mut state.messages,
            &mut state.retirement,
        );
        state.transport.advance_by_rendered_frames(output.len() / 2);
    }

    fn fill_retirement(&mut self) {
        let marker = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let state = self.state();
        while state.retirement.available_retirement_slots() != 0 {
            state.retirement.retire_cold_adoption(marker.clone());
        }
    }

    fn close(&mut self) {
        self.worker_gate.store(true, Ordering::Release);
        // Actual CallbackCloseOwner transfers the complete mixer, held capture,
        // deferred/FIFO publications and retirement backlog to the existing worker.
        drop(self.owner.take());
        drop(self.worker.take());
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.close();
    }
}

struct Take {
    control: CaptureControl,
    publication: Arc<CapturePublication>,
    path: PathBuf,
    directory: Option<tempfile::TempDir>,
}

impl Take {
    fn new(channels: usize) -> Self {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(4)
            .unwrap();
        let parent = workspace.join("test-artifacts/record-capture-20261020");
        fs::create_dir_all(&parent).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("callback-")
            .tempdir_in(parent)
            .unwrap();
        fs::write(directory.path().join("OWNER.md"),
            b"Owned capture callback regression: synthetic PCM, actual native scheduler/mixer/control FIFO/retirement and exclusive float spool. No device/Human sources. Remove only after original capture owner release and actual writer join; unsettled fixtures retained.\n").unwrap();
        let path = directory.path().join("take.capture");
        let (control, work) = prepare(
            &path,
            CaptureConfig {
                sample_rate_hz: 48_000,
                channels,
                block_frames: 4096,
                block_count: 4,
                max_pool_pcm_bytes: 128 * 1024,
            },
        )
        .unwrap();
        let deadline = Instant::now() + WAIT;
        while !control.status().writer_ready {
            assert!(Instant::now() < deadline, "writer readiness timeout");
            std::thread::sleep(Duration::from_millis(1));
        }
        Self {
            control,
            publication: Arc::new(CapturePublication::new(work)),
            path,
            directory: Some(directory),
        }
    }

    fn id(&self) -> u64 {
        self.publication.session_id()
    }
    fn status(&self) -> CaptureStatus {
        self.control.status()
    }

    fn joined(&mut self) -> CaptureStatus {
        let deadline = Instant::now() + WAIT;
        while !self.control.join_if_finished().unwrap() {
            assert!(
                Instant::now() < deadline,
                "writer join timeout: {:?}",
                self.status()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let status = self.status();
        assert!(status.worker_joined && status.worker_closed && status.rt_released);
        status
    }

    fn bits(&self) -> Vec<u32> {
        fs::read(&self.path).unwrap()[SPOOL_HEADER_BYTES as usize..]
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect()
    }
}

impl Drop for Take {
    fn drop(&mut self) {
        // An unqueued original publication may still own the work. This fallback
        // runs only on the test/control thread, never inside the observed callback.
        drop(self.publication.claim());
        let deadline = Instant::now() + WAIT;
        let mut settled = false;
        while Instant::now() < deadline {
            match self.control.join_if_finished() {
                Ok(true) => {
                    settled = self.status().rt_released;
                    break;
                }
                Ok(false) => std::thread::sleep(Duration::from_millis(1)),
                Err(_) => break,
            }
        }
        if !settled && let Some(directory) = self.directory.take() {
            eprintln!(
                "unsettled capture callback fixture retained: {}",
                directory.keep().display()
            );
        }
    }
}

fn load_constant(rig: &mut Rig) {
    let mixer = &mut rig.state().mixer;
    mixer.load_sample(
        0,
        SampleBuffer {
            channels: 2,
            residency: None,
            samples: Arc::from(vec![0.75_f32; 8192].into_boxed_slice()),
        },
    );
    mixer.set_pad_gain(0, -6.0206);
    mixer.set_volume(0.5);
}

#[test]
fn scheduled_capture_copies_exact_post_gain_master_mix_with_exclusive_in_buffer_extent() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    load_constant(&mut rig);
    rig.start(&take, 3);
    rig.stop(&take, 11);
    assert_eq!(rig.drain(), 2);
    rig.state()
        .scheduler
        .schedule(
            3,
            ScheduledCommand::PlaySample {
                id: 0,
                volume: 0.8,
                received_at_ns: None,
                expected_source_generation: None,
            },
        )
        .unwrap();
    let mut mix = Vec::new();
    for frames in [2, 5, 1, 6] {
        let mut output = vec![0.0; frames * 2];
        rig.render(&mut output);
        mix.extend(output);
    }
    assert!(mix[..6].iter().all(|value| *value == 0.0));
    assert!(
        mix[6..]
            .iter()
            .all(|value| (*value - 0.15_f32).abs() < 1.0e-5)
    );
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(3), Some(11)));
    assert_eq!((status.submitted_frames, status.written_frames), (8, 8));
    assert_eq!(
        take.bits(),
        mix[6..22]
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    );
}

#[test]
fn playback_stop_and_master_mute_leave_capture_running_through_silence() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    load_constant(&mut rig);
    assert!(rig.state().mixer.play_sample(0, 0.8));
    rig.start(&take, 0);
    rig.stop(&take, 10);
    rig.drain();
    rig.state()
        .scheduler
        .schedule(6, ScheduledCommand::StopAll)
        .unwrap();
    let mut mix = [0.0; 20];
    rig.render(&mut mix[..4]);
    rig.state().mixer.set_volume(0.0); // Same effective master mute path as momentary mute.
    rig.render(&mut mix[4..8]);
    rig.state().mixer.set_volume(0.5);
    rig.render(&mut mix[8..16]);
    rig.render(&mut mix[16..]);
    rig.render(&mut []); // Stop at the previous exclusive callback end executes here.
    assert!(mix[..4].iter().all(|value| *value > 0.0));
    assert!(mix[4..8].iter().all(|value| *value == 0.0));
    assert!(mix[8..12].iter().all(|value| *value > 0.0));
    assert!(mix[12..].iter().all(|value| *value == 0.0));
    rig.close();
    assert!(take.joined().successful_spool());
    assert_eq!(
        take.bits(),
        mix.iter().map(|value| value.to_bits()).collect::<Vec<_>>()
    );
}

#[test]
fn actual_full_retirement_retains_sealed_owner_and_callback_close_releases_whole_image() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    rig.start(&take, 0);
    rig.stop(&take, 3);
    rig.drain();
    rig.fill_retirement();
    let (_, allocations) = crate::test_memory::observe_current_thread(|| {
        rig.render(&mut [0.0; 10]);
        rig.render(&mut []);
    });
    assert_eq!(
        allocations,
        crate::test_memory::ThreadAllocations::default()
    );
    let status = take.status();
    assert!(status.rt_sealed);
    assert!(!status.rt_released);
    assert_eq!(status.end_frame, Some(3));
    assert_eq!(
        rig.state().mixer.mix_capture.as_ref().unwrap().session_id(),
        take.id()
    );
    assert!(rig.state().scheduler.is_empty());
    assert_eq!(rig.state().retirement.available_retirement_slots(), 0);
    rig.close();
    assert!(take.joined().successful_spool());
}

#[test]
fn original_reserved_stop_lane_works_when_scheduler_reports_full_and_wrong_session_is_inert() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    rig.start(&take, 0);
    rig.drain();
    rig.render(&mut []);
    for frame in 20..27 {
        rig.state()
            .scheduler
            .schedule(frame, ScheduledCommand::StopAll)
            .unwrap();
    }
    assert!(rig.state().scheduler.is_full());
    rig.send(ControlMessage::StopMixCapture {
        session_id: take.id() + 1,
        target_frame: MixCaptureTarget::At(1),
    });
    rig.stop(&take, 4);
    rig.stop(&take, 4); // Keeps original event/order; it consumes no second slot.
    rig.stop(&take, 5); // Changed duplicate endpoint also preserves the accepted original.
    assert_eq!(rig.drain(), 4);
    assert_eq!(rig.state().scheduler.peek_next_target_frame(), Some(4));
    rig.render(&mut [0.0; 12]);
    assert_eq!(take.status().end_frame, Some(4));
    assert_eq!(rig.state().scheduler.peek_next_target_frame(), Some(20));
    rig.close();
    assert!(take.joined().successful_spool());
}

#[test]
fn stop_before_original_future_start_cancels_only_that_session_without_positive_extent() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    rig.state()
        .scheduler
        .schedule(7, ScheduledCommand::StopAll)
        .unwrap();
    rig.start(&take, 10);
    rig.stop(&take, 5);
    rig.drain();
    assert_eq!(rig.state().scheduler.peek_next_target_frame(), Some(7));
    assert!(rig.state().mixer.mix_capture.is_none());
    rig.close();
    let status = take.joined();
    assert_eq!(status.failure, Some(CaptureFailure::Cancelled));
    assert!(!status.started);
    assert_eq!((status.first_frame, status.end_frame), (None, None));
    assert_eq!(status.written_frames, 0);
}

#[test]
fn control_failure_is_serviced_by_zero_channel_empty_render_without_fabricated_quiescence() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    rig.start(&take, 100);
    rig.drain();
    rig.fill_retirement();
    assert!(
        take.control
            .request_failure(take.id(), CaptureFailure::Stream)
    );
    assert!(!take.status().rt_sealed);
    let state = rig.state();
    render_scheduled_audio(
        &mut state.mixer,
        &mut state.scheduler,
        &mut [],
        &mut [0.0; NUM_SAMPLES],
        0,
        0,
        &mut state.transport,
        &mut state.messages,
        &mut state.retirement,
    );
    let status = take.status();
    assert!(status.rt_sealed);
    assert!(!status.rt_released);
    assert!(!status.started);
    assert_eq!(status.first_frame, None);
    assert!(rig.state().scheduler.is_empty());
    rig.close();
    assert_eq!(take.joined().failure, Some(CaptureFailure::Stream));
}

#[test]
fn busy_duplicate_and_stale_session_commands_cannot_replace_or_stop_original_owner() {
    let mut first = Take::new(2);
    let mut rejected = Take::new(2);
    let mut later = Take::new(2);
    let mut rig = Rig::new(false);
    rig.start(&first, 0);
    rig.start(&rejected, 0);
    rig.start(&first, 0); // Same Arc has no second work to claim.
    rig.stop(&first, 2);
    rig.drain();
    rig.render(&mut [0.0; 6]);
    assert_eq!(first.status().end_frame, Some(2));
    assert_eq!(rejected.status().failure, Some(CaptureFailure::Rejected));
    rig.start(&later, 3);
    rig.stop(&first, 3); // Old original ID cannot stop the new slot.
    rig.stop(&later, 5);
    rig.drain();
    rig.render(&mut [0.0; 6]);
    rig.close();
    assert!(first.joined().successful_spool());
    assert!(!rejected.joined().started);
    assert!(later.joined().successful_spool());
    assert_eq!(
        (later.status().first_frame, later.status().end_frame),
        (Some(3), Some(5))
    );
}

#[test]
fn start_preflight_keeps_original_fifo_work_until_two_retirement_slots_are_available() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    rig.fill_retirement();
    rig.start(&take, 0);
    assert_eq!(rig.drain(), 0);
    assert!(!rig.state().consumer.is_empty());
    assert!(rig.state().mixer.mix_capture.is_none());
    assert!(!take.status().rt_released);
    rig.worker_gate.store(true, Ordering::Release);
    let deadline = Instant::now() + WAIT;
    while rig.state().retirement.available_retirement_slots() < 2 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    rig.stop(&take, 2);
    assert_eq!(rig.drain(), 2);
    rig.render(&mut [0.0; 6]);
    rig.close();
    assert!(take.joined().successful_spool());
}

#[test]
fn scheduler_pressure_and_geometry_rejection_retire_new_owner_without_starting() {
    let mut pressure = Take::new(2);
    let mut geometry = Take::new(1);
    let mut effective_geometry = Take::new(2);
    let mut rig = Rig::new(false);
    for frame in 10..17 {
        rig.state()
            .scheduler
            .schedule(frame, ScheduledCommand::StopAll)
            .unwrap();
    }
    rig.start(&pressure, 0); // One physical slot cannot admit start plus its own stop.
    rig.drain();
    assert!(rig.state().mixer.mix_capture.is_none());
    assert_eq!(pressure.status().failure, Some(CaptureFailure::Rejected));
    rig.start(&geometry, 0);
    rig.drain();
    assert_eq!(geometry.status().failure, Some(CaptureFailure::Geometry));
    rig.render(&mut [0.0; 36]);
    rig.start(&effective_geometry, 20);
    rig.drain();
    rig.state().mixer.sample_rate_hz = 48_000.5; // Fixture-only malformed geometry before E.
    rig.render(&mut [0.0; 8]);
    rig.close();
    assert!(!pressure.joined().started);
    assert!(!geometry.joined().started);
    let status = effective_geometry.joined();
    assert_eq!(status.failure, Some(CaptureFailure::Geometry));
    assert!(!status.started);
}

#[test]
fn same_frame_start_stop_is_truthfully_empty_and_stale_start_never_captures_late_audio() {
    let mut empty = Take::new(2);
    let mut stale = Take::new(2);
    let mut rig = Rig::new(false);
    rig.start(&empty, 2);
    rig.stop(&empty, 2);
    rig.drain();
    rig.render(&mut [0.0; 8]);
    rig.start(&stale, 1);
    rig.drain();
    rig.close();
    let status = empty.joined();
    assert!(status.empty());
    assert!(!status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(2), Some(2)));
    assert_eq!(stale.joined().failure, Some(CaptureFailure::Rejected));
    assert!(!stale.status().started);
}

#[test]
fn many_single_frame_productive_callbacks_share_original_fixed_partial_block() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    rig.start(&take, 0);
    rig.stop(&take, 2051);
    rig.drain();
    let (_, allocations) = crate::test_memory::observe_current_thread(|| {
        for _ in 0..2051 {
            rig.render(&mut [0.0; 2]);
        }
        rig.render(&mut []);
    });
    assert_eq!(
        allocations,
        crate::test_memory::ThreadAllocations::default()
    );
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!(status.allocated_blocks, 4);
    assert_eq!(
        (status.submitted_frames, status.written_frames),
        (2051, 2051)
    );
    assert_eq!((status.submitted_blocks, status.written_blocks), (1, 1));
    assert!(take.bits().iter().all(|bits| *bits == 0));
}

#[test]
fn actual_callback_close_abandons_active_original_and_flushes_truthful_prefix_off_rt() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(true);
    rig.start(&take, 0);
    rig.drain();
    rig.render(&mut [0.0; 6]);
    rig.fill_retirement();
    assert!(!take.status().rt_sealed);
    assert!(!take.status().rt_released);
    assert_eq!(take.status().captured_end, Some(3));
    rig.close(); // The real existing close lane owns the live partial block.
    let status = take.joined();
    assert_eq!(status.failure, Some(CaptureFailure::Abandoned));
    assert_eq!((status.first_frame, status.end_frame), (Some(0), Some(3)));
    assert_eq!((status.submitted_frames, status.written_frames), (3, 3));
    assert!(!status.successful_spool());
    assert_eq!(take.bits(), vec![0; 6]);
}

#[test]
fn first_stop_scheduler_error_is_sticky_terminal_instead_of_an_infinite_active_capture() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    rig.start(&take, 0);
    rig.drain();
    rig.render(&mut [0.0; 6]);
    // Fault-inject loss of the real scheduler's reservation after start. This
    // exercises the actual first-stop Err boundary without corrupting the
    // process-global input-order allocator used by concurrent native tests.
    assert_eq!(rig.state().scheduler.cancel_capture_session(take.id()), 0);
    rig.stop(&take, 5);
    assert_eq!(rig.drain(), 1);
    assert!(rig.state().mixer.mix_capture.is_none());
    rig.close();
    let status = take.joined();
    assert_eq!(status.failure, Some(CaptureFailure::Rejected));
    assert_eq!((status.first_frame, status.end_frame), (Some(0), Some(3)));
    assert_eq!(status.written_frames, 3);
    assert!(!status.successful_spool());
}

#[test]
fn changed_duplicate_stop_before_future_start_preserves_already_accepted_original_endpoint() {
    let mut take = Take::new(2);
    let mut rig = Rig::new(false);
    rig.start(&take, 10);
    rig.stop(&take, 14);
    rig.stop(&take, 5);
    assert_eq!(rig.drain(), 3);
    assert_eq!(
        rig.state().scheduler.capture_stop_target_frame(take.id()),
        Some(14)
    );
    rig.render(&mut [0.0; 32]);
    rig.close();
    let status = take.joined();
    assert!(status.successful_spool());
    assert_eq!((status.first_frame, status.end_frame), (Some(10), Some(14)));
}
