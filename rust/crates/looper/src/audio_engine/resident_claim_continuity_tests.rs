//! A late unrelated placement writer must defer a physical control before any DSP cut.
use super::*;
use crate::audio_engine::audio_stream::drain_control_messages;
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirementWorker, RtAudioBufferRetirement, create_audio_buffer_retirement,
};
use crate::audio_engine::productive_source_history::ProductiveSourceHistory;
use crate::audio_engine::resident_relocation::control_publication_tests::PendingWriter;
use crate::audio_engine::source_feed_history::SourceFeedSnapshot;
use crate::messages::{ResidentControlIntent, ResidentTransaction};
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static BEFORE_CUT: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

/// Called after prospective control checks, before the placement claim and any physical cut.
pub(in crate::audio_engine) fn before_cut() {
    let hook = BEFORE_CUT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn with_before_cut<T>(hook: impl FnOnce() + 'static, action: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_CUT.with(|slot| *slot.borrow_mut() = None);
        }
    }
    BEFORE_CUT.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    action()
}

struct ClaimQueue {
    producer: rtrb::Producer<ControlMessage>,
    consumer: rtrb::Consumer<ControlMessage>,
    retirement: RtAudioBufferRetirement,
    _worker: AudioBufferRetirementWorker,
}

impl ClaimQueue {
    fn new() -> Self {
        let (producer, consumer) = rtrb::RingBuffer::new(4);
        let (retirement, worker) = create_audio_buffer_retirement();
        Self {
            producer,
            consumer,
            retirement,
            _worker: worker,
        }
    }

    fn enqueue(
        &mut self,
        mixer: &RtMixer,
        sample: SampleBuffer,
        intent: ResidentControlIntent,
    ) -> PreparedSourcePermit {
        let publication = PreparedSourcePermit::new(
            mixer.prepared_source_epochs[0].clone(),
            mixer.prepared_source_epochs[0].load(Ordering::Acquire),
        );
        publication.mark_pending().unwrap();
        self.producer
            .push(ControlMessage::RelocateResident(Box::new(
                ResidentTransaction {
                    id: 0,
                    sample,
                    stems: None,
                    binding: binding(mixer),
                    publication: publication.clone(),
                    expected_window_revision: mixer.sample_bank[0]
                        .as_ref()
                        .unwrap()
                        .window_revision(),
                    intent,
                    seek_pin: None,
                },
            )))
            .unwrap();
        publication
    }

    fn drain(&mut self, mixer: &mut RtMixer, output_frame: u64) -> usize {
        drain_control_messages(
            &mut self.consumer,
            &mut FixedCapacityScheduler::<8>::new(),
            output_frame,
            &mut TriggerQuantization::Immediate,
            &mut TransportTimeline::new(RATE),
            mixer,
            &mut Vec::new(),
            &mut self.retirement,
        )
    }

    fn render(&mut self, mixer: &mut RtMixer, output_frame: u64, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        mixer.render_rt_at_output_frame(
            &mut output,
            &mut [0.0; NUM_SAMPLES],
            output_frame,
            &mut self.retirement,
        );
        output
    }
}

#[derive(Clone, Copy, Debug)]
enum PhysicalControl {
    Loop,
    Seek,
}

impl PhysicalControl {
    fn intent(self) -> ResidentControlIntent {
        match self {
            Self::Loop => ResidentControlIntent {
                loop_region: Some((4000, Some(5500))),
                ..Default::default()
            },
            Self::Seek => ResidentControlIntent {
                seek_position_s: Some(4300.0 / f64::from(RATE)),
                ..Default::default()
            },
        }
    }

    fn apply_reference(self, reference: &mut RtMixer, frame: u64) {
        match self {
            Self::Loop => reference.set_pad_loop_region(
                0,
                4000.0 / f64::from(RATE),
                Some(5500.0 / f64::from(RATE)),
            ),
            Self::Seek => {
                assert!(reference.seek_sample_at_output_frame(0, 4300.0 / f64::from(RATE), frame,));
            }
        }
    }
}

fn assert_history_unchanged(
    actual: Option<ProductiveSourceHistory>,
    expected: Option<ProductiveSourceHistory>,
) {
    match (actual, expected) {
        (Some(actual), Some(expected)) => {
            assert_eq!(actual.binding, expected.binding);
            assert_eq!(actual.next_position, expected.next_position);
            assert_eq!(actual.domain, expected.domain);
            assert_eq!(actual.fed_output_frames, expected.fed_output_frames);
        }
        (None, None) => {}
        _ => panic!("a deferred physical control changed productive source history"),
    }
}

fn source_reads(mixer: &RtMixer) -> SourceFeedSnapshot {
    let mut snapshot = SourceFeedSnapshot::new();
    assert!(voice(mixer).source_feed_history.capture_into(&mut snapshot));
    snapshot
}

fn assert_reads_unchanged(actual: &SourceFeedSnapshot, expected: &SourceFeedSnapshot) {
    assert_eq!(actual.active_origin(), expected.active_origin());
    assert_eq!(actual.active_frames(), expected.active_frames());
    assert_eq!(actual.captured_epoch(), expected.captured_epoch());
    assert_eq!(actual.spans().len(), expected.spans().len());
    for (actual, expected) in actual.spans().zip(expected.spans()) {
        assert_eq!(actual.active_start, expected.active_start);
        assert_eq!(actual.frames, expected.frames);
        assert!(actual.playback.matches_exact(&expected.playback));
        assert!(actual.plan.matches_exact(expected.plan));
        assert_eq!(actual.context, expected.context);
    }
}

fn eq_tail(mixer: &RtMixer) -> [f32; 64] {
    let mut chain = mixer.dsp_chain_snapshot_for_test(0).unwrap();
    let mut tail = [0.0; 64];
    assert!(chain.process_interleaved_block(&mut tail, 1));
    tail
}

fn render_pair(
    actual: &mut RtMixer,
    reference: &mut RtMixer,
    queue: &mut ClaimQueue,
    elapsed: &mut u64,
    frames: usize,
) -> Vec<f32> {
    let output = queue.render(actual, *elapsed, frames);
    assert_eq!(output, render(reference, *elapsed, frames));
    assert!(
        voice(actual)
            .source_playback
            .matches_exact(&voice(reference).source_playback)
    );
    *elapsed += frames as u64;
    output
}

fn wait_prepared_pair(actual: &mut RtMixer, reference: &mut RtMixer) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while (!voice_mut(actual).stretch.source_preparation_ready()
        || !voice_mut(reference).stretch.source_preparation_ready())
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(voice_mut(actual).stretch.source_preparation_ready());
    assert!(voice_mut(reference).stretch.source_preparation_ready());
}

fn warm_pair(
    actual: &mut RtMixer,
    reference: &mut RtMixer,
    queue: &mut ClaimQueue,
    elapsed: &mut u64,
    wet: bool,
) {
    if !wet {
        render_pair(actual, reference, queue, elapsed, 229);
        return;
    }
    render_pair(actual, reference, queue, elapsed, 13);
    wait_prepared_pair(actual, reference);
    render_pair(actual, reference, queue, elapsed, 4090);
    assert!(voice(actual).stretch.wet_render_ready());
    assert!(voice(actual).stretch.productive_history().is_some());
    let deadline = Instant::now() + Duration::from_secs(5);
    while (!voice(actual).stretch.source_cut_available()
        || !voice(reference).stretch.source_cut_available())
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(voice(actual).stretch.source_cut_available());
    assert!(voice(reference).stretch.source_cut_available());
}

fn adopted_suffix(
    actual: &mut RtMixer,
    reference: &mut RtMixer,
    queue: &mut ClaimQueue,
    elapsed: &mut u64,
    wet: bool,
) {
    if !wet {
        for frames in [1, 31, 127, 257, 512] {
            render_pair(actual, reference, queue, elapsed, frames);
        }
        return;
    }
    render_pair(actual, reference, queue, elapsed, 1);
    wait_prepared_pair(actual, reference);
    let target = voice_mut(actual)
        .stretch
        .prepared_target_output_frame()
        .unwrap();
    assert_eq!(
        Some(target),
        voice_mut(reference).stretch.prepared_target_output_frame()
    );
    let mut audible = false;
    while *elapsed < target + 4096 {
        let frames = 127.min((target + 4096 - *elapsed) as usize);
        let output = render_pair(actual, reference, queue, elapsed, frames);
        if *elapsed > target {
            audible |= output.iter().any(|sample| sample.abs() > 0.001);
        }
    }
    assert!(
        audible,
        "the retried physical control produces real Native output"
    );
}

#[test]
fn late_placement_writer_defers_loop_cut_without_changing_live_native_or_eq() {
    deferred_physical_control(PhysicalControl::Loop);
}

#[test]
fn late_placement_writer_defers_seek_cut_without_changing_live_native_or_eq() {
    deferred_physical_control(PhysicalControl::Seek);
}

fn deferred_physical_control(control: PhysicalControl) {
    for wet in [false, true] {
        let full = complete();
        let mut actual = fixture(&full, None, wet);
        let mut reference = fixture(&full, None, wet);
        let mut queue = ClaimQueue::new();
        let mut elapsed = 0;
        warm_pair(&mut actual, &mut reference, &mut queue, &mut elapsed, wet);
        let before_native = voice(&actual).stretch.native_state_address();
        let before_adoption = voice(&actual).stretch.adopted_request_id();
        let before_fifo = voice(&actual).stretch.pending_fifo_frames();
        let before_native_history = voice(&actual).stretch.productive_history();
        let before_reads = source_reads(&actual);
        let before_clock = voice(&actual).source_playback;
        let before_generation = voice(&actual).generation;
        let before_volume = voice(&actual).volume;
        let before_gain = actual.pad_gain_smoothers[0];
        let before_eq_history = actual.dsp_source_history_for_test(0);
        let before_eq_tail = eq_tail(&actual);
        assert!(before_eq_tail.iter().any(|value| value.abs() > 1e-8));
        let next = full
            .window(
                0,
                full.frame_count(),
                2,
                if wet {
                    ResidentContext::KeyLockFullTrack
                } else {
                    ResidentContext::FullTrack
                },
            )
            .unwrap();
        let publication = queue.enqueue(&actual, next.clone(), control.intent());
        let ownership = actual.input_runtime_ownership.clone();
        let held = Rc::new(RefCell::new(None));
        let keep = held.clone();
        let processed = with_before_cut(
            move || *keep.borrow_mut() = Some(PendingWriter::hold(ownership)),
            || queue.drain(&mut actual, elapsed),
        );
        assert!(
            held.borrow().is_some(),
            "the writer arrived inside the callback command"
        );
        assert_eq!(processed, 0);
        assert_eq!(publication.status(), "pending");
        assert!(matches!(
            actual.deferred_control(),
            Some(ControlMessage::RelocateResident(_))
        ));
        assert_eq!(actual.sample_bank[0].as_ref().unwrap().window_revision(), 1);
        assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
        assert_eq!(voice(&actual).stretch.native_state_address(), before_native);
        assert_eq!(voice(&actual).stretch.adopted_request_id(), before_adoption);
        assert_eq!(voice(&actual).stretch.pending_fifo_frames(), before_fifo);
        assert_history_unchanged(
            voice(&actual).stretch.productive_history(),
            before_native_history,
        );
        assert_reads_unchanged(&source_reads(&actual), &before_reads);
        assert!(voice(&actual).source_playback.matches_exact(&before_clock));
        assert_eq!(voice(&actual).generation, before_generation);
        assert_eq!(voice(&actual).volume, before_volume);
        assert_eq!(actual.pad_gain_smoothers[0], before_gain);
        assert_history_unchanged(actual.dsp_source_history_for_test(0), before_eq_history);
        assert_eq!(eq_tail(&actual), before_eq_tail);

        // A separate untouched mixer supplies the expected PCM while the writer still owns
        // the marker; a deferred command must not introduce silence or restart the EQ tail.
        for frames in [1, 31, 127] {
            render_pair(
                &mut actual,
                &mut reference,
                &mut queue,
                &mut elapsed,
                frames,
            );
            assert_eq!(voice(&actual).stretch.native_state_address(), before_native);
        }
        held.borrow_mut().take().unwrap().complete();
        assert_eq!(queue.drain(&mut actual, elapsed), 1);
        assert_eq!(publication.status(), "accepted");
        assert!(actual.deferred_control().is_none());
        assert_eq!(
            actual.sample_bank[0].as_ref().unwrap().resident_binding(),
            next.resident_binding()
        );
        control.apply_reference(&mut reference, elapsed);
        adopted_suffix(&mut actual, &mut reference, &mut queue, &mut elapsed, wet);
    }
}

#[test]
fn late_cancelled_loop_control_rejects_without_clearing_live_native_or_eq() {
    for wet in [false, true] {
        let full = complete();
        let mut actual = fixture(&full, None, wet);
        let mut reference = fixture(&full, None, wet);
        let mut queue = ClaimQueue::new();
        let mut elapsed = 0;
        warm_pair(&mut actual, &mut reference, &mut queue, &mut elapsed, wet);
        let before_native = voice(&actual).stretch.native_state_address();
        let before_adoption = voice(&actual).stretch.adopted_request_id();
        let before_fifo = voice(&actual).stretch.pending_fifo_frames();
        let before_native_history = voice(&actual).stretch.productive_history();
        let before_reads = source_reads(&actual);
        let before_clock = voice(&actual).source_playback;
        let before_eq_history = actual.dsp_source_history_for_test(0);
        let before_eq_tail = eq_tail(&actual);
        assert!(before_eq_tail.iter().any(|value| value.abs() > 1e-8));
        let next = full
            .window(
                0,
                full.frame_count(),
                2,
                if wet {
                    ResidentContext::KeyLockFullTrack
                } else {
                    ResidentContext::FullTrack
                },
            )
            .unwrap();
        let publication = queue.enqueue(&actual, next, PhysicalControl::Loop.intent());
        let cancel = publication.clone();
        assert_eq!(
            with_before_cut(
                move || assert!(cancel.cancel_unclaimed()),
                || queue.drain(&mut actual, elapsed),
            ),
            1
        );
        assert_eq!(publication.status(), "rejected");
        assert!(actual.deferred_control().is_none());
        assert_eq!(actual.sample_bank[0].as_ref().unwrap().window_revision(), 1);
        assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
        assert_eq!(voice(&actual).stretch.native_state_address(), before_native);
        assert_eq!(voice(&actual).stretch.adopted_request_id(), before_adoption);
        assert_eq!(voice(&actual).stretch.pending_fifo_frames(), before_fifo);
        assert_history_unchanged(
            voice(&actual).stretch.productive_history(),
            before_native_history,
        );
        assert_reads_unchanged(&source_reads(&actual), &before_reads);
        assert!(voice(&actual).source_playback.matches_exact(&before_clock));
        assert_history_unchanged(actual.dsp_source_history_for_test(0), before_eq_history);
        assert_eq!(eq_tail(&actual), before_eq_tail);
        for frames in [1, 31, 127, 257] {
            render_pair(
                &mut actual,
                &mut reference,
                &mut queue,
                &mut elapsed,
                frames,
            );
        }
    }
}
