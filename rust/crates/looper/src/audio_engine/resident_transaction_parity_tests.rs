//! C2b actual command drain/render and off-thread immutable reader retirement.
//! These compact generated PCM fixtures prove native behavior; complete-evidence
//! acceptance and long musical-period bounds are checked by resident_long_cycle_tests.
use super::*;
use crate::audio_engine::audio_stream::drain_control_messages;
use crate::audio_engine::buffer_retirement::{
    AudioBufferRetirement, AudioBufferRetirementWorker, RtAudioBufferRetirement,
    create_audio_buffer_retirement,
};
use crate::messages::{ResidentControlIntent, ResidentTransaction};
use std::sync::atomic::{AtomicBool, AtomicU8};

struct RetirementGate {
    actual: RtAudioBufferRetirement,
    capacity: usize,
}

impl AudioBufferRetirement for RetirementGate {
    fn retire_resident_capture(
        &mut self,
        value: Arc<crate::audio_engine::resident_seek::ResidentSeekCapture>,
    ) {
        self.actual.retire_resident_capture(value);
    }
    fn retire_resident_cancellation(&mut self, value: Arc<AtomicBool>) {
        self.actual.retire_resident_cancellation(value);
    }
    fn retire_resident_transaction(&mut self, value: Box<ResidentTransaction>) {
        self.actual.retire_resident_transaction(value);
    }
    fn retire_cold_adoption(&mut self, value: Arc<AtomicU8>) {
        self.actual.retire_cold_adoption(value);
    }
    fn retire_sample(&mut self, value: SampleBuffer) {
        self.actual.retire_sample(value);
    }
    fn retire_prepared_stems(&mut self, value: PreparedStemSet) {
        self.actual.retire_prepared_stems(value);
    }
    fn retire_constant_timing(&mut self, value: PreparedConstantTiming) {
        self.actual.retire_constant_timing(value);
    }
    fn retire_global_playback_batch(
        &mut self,
        value: Arc<crate::audio_engine::global_playback_batch::GlobalPlaybackBatch>,
    ) {
        self.actual.retire_global_playback_batch(value);
    }
    fn retire_accepted_timing_refresh(
        &mut self,
        value: Arc<crate::audio_engine::accepted_timing_refresh::AcceptedTimingRefresh>,
    ) {
        self.actual.retire_accepted_timing_refresh(value);
    }
    fn available_retirement_slots(&mut self) -> usize {
        self.capacity.min(self.actual.available_retirement_slots())
    }
}

struct NativeQueue {
    producer: rtrb::Producer<ControlMessage>,
    consumer: rtrb::Consumer<ControlMessage>,
    retirement: RetirementGate,
    _worker: AudioBufferRetirementWorker,
}

impl NativeQueue {
    fn new() -> Self {
        let (producer, consumer) = rtrb::RingBuffer::new(4);
        let (actual, worker) = create_audio_buffer_retirement();
        Self {
            producer,
            consumer,
            retirement: RetirementGate {
                actual,
                capacity: usize::MAX,
            },
            _worker: worker,
        }
    }

    fn enqueue(
        &mut self,
        mixer: &RtMixer,
        sample: SampleBuffer,
        stems: Option<PreparedStemSet>,
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
                    stems,
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

fn plain(sample: &SampleBuffer) -> RtMixer {
    let mut mixer = RtMixer::new(1, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, sample, RATE, 1);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, sample.clone());
    mixer.set_pad_loop_region(
        0,
        START as f64 / f64::from(RATE),
        Some(END as f64 / f64::from(RATE)),
    );
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

fn wait_released(backing: &std::sync::Weak<[f32]>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while backing.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        backing.upgrade().is_none(),
        "retired immutable PCM still has an owner"
    );
}

#[test]
fn native_ack_nonresident_intro_tail_source_end_seek_matches_complete_pcm_and_keeps_markers() {
    for target in [100, 8800, 10000] {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let old_backing = Arc::downgrade(&finite.samples);
        let mut actual = plain(&finite);
        let mut reference = plain(&full);
        let mut queue = NativeQueue::new();
        let pre = queue.render(&mut actual, 0, 77);
        assert_eq!(pre, render(&mut reference, 0, 77));
        let intent = ResidentControlIntent {
            seek_position_s: Some(target as f64 / f64::from(RATE)),
            ..Default::default()
        };
        let publication = queue.enqueue(
            &actual,
            full.window(0, full.frame_count(), 2, ResidentContext::FullTrack)
                .unwrap(),
            None,
            intent,
        );
        queue.retirement.capacity = 0;
        assert_eq!(queue.drain(&mut actual, 77), 0);
        assert_eq!(publication.status(), "pending");
        assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
        assert_eq!(
            queue.render(&mut actual, 77, 53),
            render(&mut reference, 77, 53)
        );
        queue.retirement.capacity = usize::MAX;
        assert_eq!(queue.drain(&mut actual, 130), 1);
        assert_eq!(publication.status(), "accepted");
        assert!(reference.seek_sample_at_output_frame(0, target as f64 / f64::from(RATE), 130));
        assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
        assert!(
            voice(&actual)
                .source_playback
                .matches_exact(&voice(&reference).source_playback)
        );
        let requested = target.min(full.frame_count());
        let output = queue.render(&mut actual, 130, 2001);
        assert_eq!(output, render(&mut reference, 130, 2001));
        // At unity rate the independent original sine-knot oracle traverses intro
        // or actual source tail once, then the existing physical loop.
        for (frame, value) in output.iter().enumerate() {
            let before_end = if requested < START {
                END
            } else {
                full.frame_count()
            };
            let index = if frame < before_end - requested {
                requested + frame
            } else {
                START + (frame - (before_end - requested)) % (END - START)
            };
            let expected = (index as f32 * 0.073).sin() * 0.4;
            assert_eq!(
                value.to_bits(),
                expected.to_bits(),
                "target={target} frame={frame}"
            );
        }
        drop(finite);
        wait_released(&old_backing);
    }
}

#[test]
fn native_ack_paused_seek_remains_paused_and_stopped_seek_remains_noop() {
    for paused in [true, false] {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let mut actual = plain(&finite);
        let mut reference = plain(&full);
        let mut queue = NativeQueue::new();
        assert_eq!(
            queue.render(&mut actual, 0, 43),
            render(&mut reference, 0, 43)
        );
        if paused {
            actual.pause_sample(0);
            reference.pause_sample(0);
        } else {
            actual.stop_sample_rt(0, &mut queue.retirement);
            reference.stop_sample(0);
        }
        let intent = ResidentControlIntent {
            seek_position_s: Some(8800.0 / f64::from(RATE)),
            ..Default::default()
        };
        let publication = queue.enqueue(
            &actual,
            full.window(0, full.frame_count(), 2, ResidentContext::FullTrack)
                .unwrap(),
            None,
            intent,
        );
        assert_eq!(queue.drain(&mut actual, 43), 1);
        assert_eq!(publication.status(), "accepted");
        assert_eq!(
            reference.seek_sample_at_output_frame(0, 8800.0 / f64::from(RATE), 43),
            paused
        );
        assert_eq!(queue.render(&mut actual, 43, 91), vec![0.0; 91]);
        assert_eq!(render(&mut reference, 43, 91), vec![0.0; 91]);
        assert_eq!(actual.loop_region_frames(0), (START, Some(END)));
        if paused {
            assert!(voice(&actual).paused);
            assert_eq!(voice(&actual).source_playback.position().frame, 8800);
            actual.resume_sample(0);
            reference.resume_sample(0);
        } else {
            assert!(!actual.voices.iter().any(|voice| voice.active));
            assert_eq!(
                actual.pad_playhead_seconds(0),
                reference.pad_playhead_seconds(0)
            );
            assert!(actual.play_sample_at_output_frame(0, 1.0, 134));
            assert!(reference.play_sample_at_output_frame(0, 1.0, 134));
        }
        assert_eq!(
            queue.render(&mut actual, 134, 999),
            render(&mut reference, 134, 999)
        );
    }
}

#[test]
fn native_ack_finite_edit_and_all_match_complete_output_with_fractional_rate_filter_and_same_stems()
{
    for with_stems in [false, true] {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let complete_set = with_stems.then(|| stems(&full));
        let mut actual = fixture(
            &finite,
            complete_set
                .clone()
                .map(|set| set.window_for(&finite).unwrap()),
            false,
        );
        let mut reference = fixture(&full, complete_set.clone(), false);
        let mut queue = NativeQueue::new();
        assert_eq!(
            queue.render(&mut actual, 0, 197),
            render(&mut reference, 0, 197)
        );
        for mixer in [&mut actual, &mut reference] {
            mixer.set_speed(1.25);
            if with_stems {
                mixer.set_stem_enabled_mask(0, 0b0101, 91);
            }
        }
        assert_eq!(
            queue.render(&mut actual, 197, 31),
            render(&mut reference, 197, 31)
        );
        let accepted_before = actual.pad_accepted_timing[0];
        let mut elapsed = 228;
        for (start, end, context) in [
            (4000, Some(5500), ResidentContext::FiniteLoop),
            (0, None, ResidentContext::FullTrack),
        ] {
            let next = full
                .window(
                    start,
                    end.unwrap_or(full.frame_count()),
                    actual.sample_bank[0].as_ref().unwrap().window_revision() + 1,
                    context,
                )
                .unwrap();
            let next_stems = complete_set.clone().map(|mut set| {
                set.accepted_timing = accepted_before;
                set.window_for(&next).unwrap()
            });
            let intent = ResidentControlIntent {
                loop_region: Some((start, end)),
                ..Default::default()
            };
            let publication = queue.enqueue(&actual, next, next_stems, intent);
            queue.retirement.capacity = 0;
            assert_eq!(queue.drain(&mut actual, elapsed), 0);
            assert_eq!(publication.status(), "pending");
            assert_eq!(
                queue.render(&mut actual, elapsed, 17),
                render(&mut reference, elapsed, 17)
            );
            elapsed += 17;
            queue.retirement.capacity = usize::MAX;
            assert_eq!(queue.drain(&mut actual, elapsed), 1);
            assert_eq!(publication.status(), "accepted");
            reference.set_pad_loop_region(
                0,
                start as f64 / f64::from(RATE),
                end.map(|end| end as f64 / f64::from(RATE)),
            );
            assert_eq!(actual.loop_region_frames(0), (start, end));
            assert_eq!(actual.pad_accepted_timing[0], accepted_before);
            for frames in [1, 127, 384, 96, 257, 512, 31, 901] {
                assert_eq!(
                    queue.render(&mut actual, elapsed, frames),
                    render(&mut reference, elapsed, frames)
                );
                elapsed += frames as u64;
            }
            assert!(
                voice(&actual)
                    .source_playback
                    .matches_exact(&voice(&reference).source_playback)
            );
        }
    }
}

#[test]
fn native_rejected_or_cancelled_transaction_preserves_effective_audio_and_retires_payload() {
    for case in 0..4 {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let mut actual = fixture(&finite, None, false);
        let mut reference = fixture(&finite, None, false);
        let mut queue = NativeQueue::new();
        assert_eq!(
            queue.render(&mut actual, 0, 37),
            render(&mut reference, 0, 37)
        );
        let captured = binding(&actual);
        let next = full
            .window(4000, 5500, 2, ResidentContext::FiniteLoop)
            .unwrap();
        let backing = Arc::downgrade(&next.samples);
        let intent = ResidentControlIntent {
            loop_region: Some((4000, Some(5500))),
            key_lock: (case == 3).then_some(true),
            ..Default::default()
        };
        let publication = queue.enqueue(&actual, next, None, intent);
        match case {
            0 => assert!(publication.cancel_unclaimed()),
            1 => {
                actual.prepared_source_epochs[0].fetch_add(1, Ordering::AcqRel);
            }
            2 => {
                actual.input_runtime_ownership.revoke_source(0);
            }
            3 => {} // Requested wet context cannot adopt a finite-only payload.
            _ => unreachable!(),
        }
        assert_eq!(queue.drain(&mut actual, 37), 1);
        assert_eq!(publication.status(), "rejected");
        assert_eq!(
            actual.sample_bank[0].as_ref().unwrap().resident_binding(),
            captured.resident
        );
        assert!(!actual.pad_key_lock_enabled[0]);
        assert_eq!(
            queue.render(&mut actual, 37, 997),
            render(&mut reference, 37, 997)
        );
        assert_eq!(
            actual.pad_accepted_timing[0],
            reference.pad_accepted_timing[0]
        );
        wait_released(&backing);
    }
}

#[test]
fn native_fulltrack_keylock_enable_then_relocation_preserves_native_fifo_filter_and_same_stems() {
    for with_stems in [false, true] {
        let full = complete();
        let finite = full
            .window(START, END, 1, ResidentContext::FiniteLoop)
            .unwrap();
        let complete_set = with_stems.then(|| stems(&full));
        let mut actual = fixture(
            &finite,
            complete_set
                .clone()
                .map(|set| set.window_for(&finite).unwrap()),
            false,
        );
        let mut reference = fixture(&full, complete_set.clone(), false);
        let mut queue = NativeQueue::new();
        assert_eq!(
            queue.render(&mut actual, 0, 137),
            render(&mut reference, 0, 137)
        );
        let next = full
            .window(0, full.frame_count(), 2, ResidentContext::KeyLockFullTrack)
            .unwrap();
        let next_stems = complete_set.clone().map(|mut set| {
            set.accepted_timing = Some(accepted());
            set.window_for(&next).unwrap()
        });
        let publication = queue.enqueue(
            &actual,
            next,
            next_stems,
            ResidentControlIntent {
                key_lock: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(queue.drain(&mut actual, 137), 1);
        assert_eq!(publication.status(), "accepted");
        reference.set_pad_key_lock(0, true);
        assert_eq!(
            queue.render(&mut actual, 137, 13),
            render(&mut reference, 137, 13)
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while (!voice_mut(&mut actual).stretch.source_preparation_ready()
            || !voice_mut(&mut reference).stretch.source_preparation_ready())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(voice_mut(&mut actual).stretch.source_preparation_ready());
        assert!(voice_mut(&mut reference).stretch.source_preparation_ready());
        assert_eq!(
            queue.render(&mut actual, 150, 4090),
            render(&mut reference, 150, 4090)
        );
        let address = voice(&actual).stretch.native_state_address();
        assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
        let history = voice(&actual).stretch.productive_history().unwrap();
        let fifo = voice(&actual).stretch.pending_fifo_frames();
        let playback = voice(&actual).source_playback;
        let next = full
            .window(0, full.frame_count(), 3, ResidentContext::KeyLockFullTrack)
            .unwrap();
        let next_stems = complete_set.map(|mut set| {
            set.accepted_timing = Some(accepted());
            set.window_for(&next).unwrap()
        });
        let publication = queue.enqueue(&actual, next, next_stems, Default::default());
        assert_eq!(queue.drain(&mut actual, 4240), 1);
        assert_eq!(publication.status(), "accepted");
        assert_eq!(voice(&actual).stretch.native_state_address(), address);
        assert_eq!(voice(&actual).stretch.pending_fifo_frames(), fifo);
        assert_eq!(
            voice(&actual)
                .stretch
                .productive_history()
                .unwrap()
                .fed_output_frames,
            history.fed_output_frames
        );
        assert!(voice(&actual).source_playback.matches_exact(&playback));
        let mut elapsed = 4240;
        for frames in [1, 17, 127, 384, 512, 777, 2048, 4096] {
            assert_eq!(
                queue.render(&mut actual, elapsed, frames),
                render(&mut reference, elapsed, frames)
            );
            assert_eq!(voice(&actual).stretch.native_state_address(), address);
            assert_eq!(voice(&actual).stretch.adopted_request_id(), Some(1));
            elapsed += frames as u64;
        }
    }
}

#[test]
fn native_previous_complete_voice_seek_keeps_new_bank_window_and_clamps_to_original_extent() {
    let full = complete();
    let mut actual = plain(&full);
    let mut reference = plain(&full);
    let mut queue = NativeQueue::new();
    assert_eq!(
        queue.render(&mut actual, 0, 43),
        render(&mut reference, 0, 43)
    );
    let replacement = SampleBuffer {
        channels: 1,
        samples: Arc::from(vec![0.25; 1000]),
        residency: None,
    }
    .with_complete_source(RATE);
    let next_bank = replacement
        .window(100, 300, 1, ResidentContext::FiniteLoop)
        .unwrap();
    for mixer in [&mut actual, &mut reference] {
        mixer
            .input_runtime_ownership
            .publish_source(0, &next_bank, RATE, 2);
        apply(
            mixer,
            ControlMessage::LoadSample {
                id: 0,
                sample: next_bank.clone(),
            },
        );
        mixer.set_pad_loop_region(0, 100.0 / f64::from(RATE), Some(300.0 / f64::from(RATE)));
    }
    let bank_before = actual.sample_bank[0].as_ref().unwrap().resident_binding();
    let old_voice_before = voice(&actual).sample.as_ref().unwrap().samples.clone();
    assert_eq!(
        voice(&actual).source_loop_region,
        Some(FrameRange {
            start: START,
            end: END
        })
    );
    let publication = queue.enqueue(
        &actual,
        replacement
            .window(0, 1000, 2, ResidentContext::FullTrack)
            .unwrap(),
        None,
        ResidentControlIntent {
            seek_position_s: Some(10000.0 / f64::from(RATE)),
            ..Default::default()
        },
    );
    assert_eq!(queue.drain(&mut actual, 43), 1);
    assert_eq!(publication.status(), "accepted");
    assert!(publication.preserved_window());
    assert_eq!(
        actual.sample_bank[0].as_ref().unwrap().resident_binding(),
        bank_before
    );
    assert!(Arc::ptr_eq(
        &voice(&actual).sample.as_ref().unwrap().samples,
        &old_voice_before
    ));
    assert_eq!(voice(&actual).source_playback.position().frame, 9000);
    assert_eq!(
        publication.resident_seek_seconds(),
        Some(9000.0 / f64::from(RATE))
    );
    assert!(reference.seek_sample_at_output_frame(0, 10000.0 / f64::from(RATE), 43));
    let output = queue.render(&mut actual, 43, 101);
    assert_eq!(output, render(&mut reference, 43, 101));
    for (frame, value) in output.iter().enumerate() {
        let source = START + frame % (END - START);
        assert_eq!(
            value.to_bits(),
            ((source as f32 * 0.073).sin() * 0.4).to_bits()
        );
    }
}

#[test]
fn native_bank_replacement_keeps_old_stems_transition_and_retires_voice_owners_on_stop() {
    let full = complete();
    let finite = full
        .window(START, END, 1, ResidentContext::FiniteLoop)
        .unwrap();
    let finite_set = stems(&full).window_for(&finite).unwrap();
    let old_stem_backing = Arc::downgrade(&finite_set.stems[1].samples);
    let old_pcm_backing = Arc::downgrade(&finite.samples);
    let mut actual = fixture(&finite, Some(finite_set), false);
    let mut reference = fixture(&full, Some(stems(&full)), false);
    drop(finite);
    let mut queue = NativeQueue::new();
    assert_eq!(
        queue.render(&mut actual, 0, 137),
        render(&mut reference, 0, 137)
    );
    for mixer in [&mut actual, &mut reference] {
        mixer.set_speed(1.25);
        mixer.set_stem_enabled_mask(0, 0b0101, 91);
    }
    assert_eq!(
        queue.render(&mut actual, 137, 17),
        render(&mut reference, 137, 17)
    );
    assert!(actual.stem_transitions[0].is_active());

    let replacement = SampleBuffer {
        channels: 1,
        samples: Arc::from(vec![0.25; 1000]),
        residency: None,
    }
    .with_complete_source(RATE);
    let next_bank = replacement
        .window(100, 300, 1, ResidentContext::FiniteLoop)
        .unwrap();
    actual
        .input_runtime_ownership
        .publish_source(0, &next_bank, RATE, 2);
    queue
        .producer
        .push(ControlMessage::LoadSample {
            id: 0,
            sample: next_bank,
        })
        .unwrap();
    queue.retirement.capacity = 0;
    assert_eq!(queue.drain(&mut actual, 154), 0);
    assert_eq!(
        queue.render(&mut actual, 154, 31),
        render(&mut reference, 154, 31)
    );
    queue.retirement.capacity = usize::MAX;
    assert_eq!(queue.drain(&mut actual, 185), 1);
    let frozen = voice(&actual).frozen_stems.as_ref().unwrap();
    assert_eq!(
        frozen.selection,
        StemRenderSelection::from_state(StemMixMode::AllStems, 91, 0b0101)
    );
    assert!(
        frozen
            .transition
            .matches_exact(reference.stem_transitions[0])
    );
    assert!(old_stem_backing.upgrade().is_some());
    assert!(old_pcm_backing.upgrade().is_some());
    // New bank controls are deliberately different; only its future launch uses them.
    actual.set_stem_mix_mode(0, StemMixMode::FullMix, 0);
    actual.set_stem_enabled_mask(0, 0, 0);
    let mut elapsed = 185;
    for frames in [1, 13, 257, 512, 1023, 17] {
        assert_eq!(
            queue.render(&mut actual, elapsed, frames),
            render(&mut reference, elapsed, frames)
        );
        assert!(
            voice(&actual)
                .frozen_stems
                .as_ref()
                .unwrap()
                .transition
                .matches_exact(reference.stem_transitions[0])
        );
        elapsed += frames as u64;
    }
    assert!(
        !voice(&actual)
            .frozen_stems
            .as_ref()
            .unwrap()
            .transition
            .is_active()
    );
    queue
        .producer
        .push(ControlMessage::StopSample { id: 0 })
        .unwrap();
    queue.retirement.capacity = 0;
    assert_eq!(queue.drain(&mut actual, elapsed), 0);
    assert_eq!(
        queue.render(&mut actual, elapsed, 17),
        render(&mut reference, elapsed, 17)
    );
    elapsed += 17;
    queue.retirement.capacity = usize::MAX;
    assert_eq!(queue.drain(&mut actual, elapsed), 1);
    assert!(
        queue
            .render(&mut actual, elapsed, 31)
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(
        actual
            .voices
            .iter()
            .all(|voice| voice.frozen_stems.is_none())
    );
    wait_released(&old_stem_backing);
    wait_released(&old_pcm_backing);
}

#[test]
fn native_old_complete_stem_seek_reads_original_knots_with_new_bank_selection() {
    let full = complete();
    let mut actual = plain(&full);
    let mut queue = NativeQueue::new();
    queue
        .producer
        .push(ControlMessage::StopSample { id: 0 })
        .unwrap();
    queue
        .producer
        .push(ControlMessage::PublishPreparedStems {
            id: 0,
            stems: stems(&full),
        })
        .unwrap();
    queue
        .producer
        .push(ControlMessage::PlaySample {
            id: 0,
            volume: 1.0,
            received_at_ns: None,
        })
        .unwrap();
    assert_eq!(queue.drain(&mut actual, 0), 3);
    assert!(actual.prepared_stems[0].is_some());
    actual.set_stem_mix_mode(0, StemMixMode::AllStems, 91);
    actual.set_stem_enabled_mask(0, 0b1010, 91);
    queue.render(&mut actual, 0, 128); // Complete the documented 128 source-frame ramp.
    assert!(!actual.stem_transitions[0].is_active());
    let replacement = SampleBuffer {
        channels: 1,
        samples: Arc::from(vec![0.25; 1000]),
        residency: None,
    }
    .with_complete_source(RATE);
    let next_bank = replacement
        .window(100, 300, 1, ResidentContext::FiniteLoop)
        .unwrap();
    actual
        .input_runtime_ownership
        .publish_source(0, &next_bank, RATE, 2);
    queue
        .producer
        .push(ControlMessage::LoadSample {
            id: 0,
            sample: next_bank,
        })
        .unwrap();
    assert_eq!(queue.drain(&mut actual, 128), 1);
    let bank = actual.sample_bank[0].as_ref().unwrap().resident_binding();
    actual.set_stem_mix_mode(0, StemMixMode::FullMix, 0);
    for target in [100, 8800, 10000] {
        let publication = queue.enqueue(
            &actual,
            replacement
                .window(0, 1000, 2, ResidentContext::FullTrack)
                .unwrap(),
            None,
            ResidentControlIntent {
                seek_position_s: Some(target as f64 / f64::from(RATE)),
                ..Default::default()
            },
        );
        assert_eq!(queue.drain(&mut actual, 128), 1);
        assert_eq!(publication.status(), "accepted");
        assert!(publication.preserved_window());
        assert_eq!(
            actual.sample_bank[0].as_ref().unwrap().resident_binding(),
            bank
        );
        let clamped = target.min(9000);
        let output = queue.render(&mut actual, 128, 211);
        for (offset, value) in output.iter().enumerate() {
            let absolute = clamped + offset;
            let source = if clamped < START {
                if absolute < END {
                    absolute
                } else {
                    START + (absolute - END) % (END - START)
                }
            } else if absolute < 9000 {
                absolute
            } else {
                START + (absolute - 9000) % (END - START)
            };
            let knot = (source as f32 * 0.073).sin() * 0.4;
            let expected = knot * 0.2 + knot * 0.4;
            assert_eq!(
                value.to_bits(),
                expected.to_bits(),
                "old stem knot at {target}+{offset}"
            );
        }
    }
}
