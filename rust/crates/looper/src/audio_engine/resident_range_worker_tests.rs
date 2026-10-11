//! Real, device-free 216-pad Source/Pair/Window callback adoption.
//! Two small verified materials exercise shared and distinct content with the
//! unchanged native worker/queue limits. Timing is explicitly Legacy/off here;
//! no automatic-timing, simultaneous-216-voice, RSS or hearing claim is made.

use super::*;
use crate::audio_engine::constants::NUM_SAMPLES;
use crate::audio_engine::input_runtime_binding::{
    InputPadBinding, capture, enqueue_stop_with_producer,
};
use crate::audio_engine::resident_relocation::{
    ResidentWindowTicket, WindowRequest, launch_with_producer, prepare_window_with_producer,
    reconcile,
};
use crate::audio_engine::source_reader::{
    reset_tap_observation_for_test, tap_observation_for_test,
};
use crate::messages::StemMixMode;
use std::mem::size_of;

fn range_voice(mixer: &RtMixer) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn range_voice_mut(mixer: &mut RtMixer) -> &mut crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn paired_range_render(
    h: &mut Harness,
    reference: &mut RtMixer,
    frame: &mut u64,
    frames: usize,
) -> Vec<f32> {
    let mut actual = vec![0.0; frames * 2];
    reset_tap_observation_for_test();
    h.callback.mixer.render_rt_at_output_frame(
        &mut actual,
        &mut [0.0; NUM_SAMPLES],
        *frame,
        &mut h.callback.retirement,
    );
    let taps = tap_observation_for_test();
    assert_eq!(taps.left_reads, frames * 2);
    assert!(taps.right_reads > 0);
    assert_eq!(taps.missing_reads, 0);
    assert!(taps.min_frame.is_some_and(|value| value >= 1));
    assert!(taps.max_frame.is_some_and(|value| value < 6));
    let mut expected = vec![0.0; frames * 2];
    reference.render_at_output_frame(*frame, &mut expected, &mut [0.0; NUM_SAMPLES]);
    assert_eq!(actual, expected, "finite/native transition at {}", *frame);
    assert!(
        range_voice(&h.callback.mixer)
            .source_playback
            .matches_exact(&range_voice(reference).source_playback)
    );
    assert_eq!(
        range_voice(&h.callback.mixer).stretch.pending_fifo_frames(),
        range_voice(reference).stretch.pending_fifo_frames()
    );
    *frame += frames as u64;
    actual
}

fn adopt_range_native(
    h: &mut Harness,
    reference: &mut RtMixer,
    frame: &mut u64,
    request_id: u64,
) -> usize {
    wait_until(|| {
        range_voice_mut(&mut h.callback.mixer)
            .stretch
            .source_preparation_ready()
            && range_voice_mut(reference)
                .stretch
                .source_preparation_ready()
    });
    let target = range_voice_mut(&mut h.callback.mixer)
        .stretch
        .prepared_target_output_frame()
        .unwrap();
    assert_eq!(
        range_voice_mut(reference)
            .stretch
            .prepared_target_output_frame(),
        Some(target)
    );
    let prepared = range_voice_mut(&mut h.callback.mixer)
        .stretch
        .prepared_native_address();
    let prepared_taps = range_voice_mut(&mut h.callback.mixer)
        .stretch
        .prepared_tap_observation()
        .unwrap();
    assert_eq!(prepared_taps.left_reads, 4096 * 2);
    assert!(prepared_taps.right_reads > 0);
    assert_eq!(prepared_taps.missing_reads, 0);
    assert!(prepared_taps.min_frame.is_some_and(|value| value >= 1));
    assert!(prepared_taps.max_frame.is_some_and(|value| value < 6));
    assert_ne!(prepared, 0);
    assert_ne!(
        prepared,
        range_voice(&h.callback.mixer)
            .stretch
            .native_state_address()
    );
    assert_eq!(
        range_voice_mut(&mut h.callback.mixer)
            .stretch
            .prepared_fifo_frames(),
        range_voice_mut(reference).stretch.prepared_fifo_frames()
    );
    let mut partition = [31_usize, 777, 1024, 1, 257, 512].into_iter().cycle();
    while *frame < target {
        let frames = partition.next().unwrap().min((target - *frame) as usize);
        paired_range_render(h, reference, frame, frames);
    }
    let output = paired_range_render(h, reference, frame, 137);
    assert!(output.iter().any(|value| value.abs() > 0.001));
    assert_eq!(
        range_voice(&h.callback.mixer)
            .stretch
            .native_state_address(),
        prepared
    );
    assert_eq!(
        range_voice(&h.callback.mixer).stretch.adopted_request_id(),
        Some(request_id)
    );
    assert_eq!(
        range_voice(&h.callback.mixer)
            .stretch
            .native_tap_observation(),
        Some(prepared_taps)
    );
    prepared
}

#[test]
fn actual_finite_selection_commands_keep_native_ramps_then_retire_from_job_and_history() {
    use crate::audio_engine::buffer_retirement::ImmediateAudioBufferRetirement;
    use crate::audio_engine::input_runtime_binding::InputRuntimeOwnership;
    use crate::audio_engine::prepared_source::PreparedSourcePermit;
    use crate::audio_engine::resident_relocation::{
        history_count_for_test, set_stem_pair_full_mix_with_producer,
    };

    let mut h = Harness::new();
    let source_ticket = h.ticket(0);
    let pair = h.prepare(0, &source_ticket, true, None);
    let selected = selection(&pair);
    let _saved = h.save(&pair);
    let pair_reader = pair.reader_lifetime_for_test();
    h.engine
        .publish_stem_pair_with_producer(&pair, &source_ticket, &h.producer)
        .unwrap();
    let published = queued_stems(&mut h.consumer);
    let hash = published.source_version_hash;
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(source_ticket.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    assert_eq!(
        h.callback.mixer.stem_demand_for_measurement(0),
        (StemMixMode::FullMix, 0b1111, 0)
    );
    h.callback.mixer.set_speed(0.73);
    let ticket = prepare_window_with_producer(
        &h.engine,
        0,
        WindowRequest {
            loop_region: Some((1.0 / f64::from(RATE), Some(6.0 / f64::from(RATE)))),
            key_lock: Some(true),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        h.consumer.peek().is_ok() || matches!(ticket.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        ticket.publication_status(),
        "pending",
        "{:?}",
        ticket.error().unwrap()
    );
    let ranges = ticket.read_observation_for_test().unwrap();
    let bytes = 5 * 2 * size_of::<f32>();
    assert_eq!(ranges.source.read_bytes, bytes as u64);
    assert_eq!(ranges.source.allocated_bytes, bytes);
    assert_eq!(
        ranges.stems.read_bytes,
        [bytes as u64, bytes as u64, bytes as u64, bytes as u64, 0]
    );
    assert_eq!(ranges.stems.allocated_bytes, bytes * 4);
    assert_eq!(ranges.stems.fresh_opens, 0);
    assert_eq!(ranges.stems.integrity_bytes, 0);
    assert_eq!(
        ranges.stems.byte_ranges,
        [
            Some((8, 8 + bytes)),
            Some((8, 8 + bytes)),
            Some((8, 8 + bytes)),
            Some((8, 8 + bytes)),
            None
        ]
    );
    assert!(ranges.admitted_peak_bytes < crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES);
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    assert_eq!(
        h.callback.mixer.stem_demand_for_measurement(0),
        (StemMixMode::FullMix, 0b1111, 0)
    );
    let mut job = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
    let mut components = job
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    assert!(job.stems.iter().all(|stem| stem.samples.len() == 10));
    assert!(h.callback.mixer.key_lock_for_measurement(0));
    h.callback.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(launch_with_producer(&h.engine, &ticket, false, 1, &h.producer).unwrap());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    drop(published);
    drop(pair);
    h.callback.retirement.retired.clear();

    // Independent original WAV integers supply complete FullMix and component PCM.
    // The reference has its own identity; it cannot keep productive History alive.
    let integers = [0_i16, 8192, -16384, 32767, 0, -4096, 16384];
    let full = SampleBuffer {
        channels: 2,
        residency: None,
        samples: Arc::from(
            integers
                .into_iter()
                .flat_map(|value| [f32::from(value) / 32768.0; 2])
                .collect::<Vec<_>>(),
        ),
    }
    .with_complete_source(RATE);
    let control_stems = PreparedStemSet {
        complete_set_identity: Arc::new([97; 32]),
        reference_samples: full.samples.clone(),
        publication: PreparedSourcePermit::unrestricted(),
        accepted_timing: None,
        source_version_hash: hash,
        sample_rate_hz: RATE,
        channels: 2,
        frame_count: full.frame_count(),
        available_mask: 0b1111,
        stems: std::array::from_fn(|_| SampleBuffer {
            channels: 2,
            residency: full.residency.clone(),
            // The committed pair's explicit PCM16 conversion divides by32767;
            // the original decoder above divides by32768. Build each separately.
            samples: integers
                .into_iter()
                .flat_map(|value| [f32::from(value) / 32767.0; 2])
                .collect::<Vec<_>>()
                .into(),
        }),
    };
    let mut reference = RtMixer::new(2, RATE as f32);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, &full, RATE, 1);
    reference.set_input_runtime_ownership(ownership);
    reference.load_sample(0, full);
    assert!(reference.publish_prepared_stems(0, control_stems));
    reference.set_pad_loop_region(0, 1.0 / f64::from(RATE), Some(6.0 / f64::from(RATE)));
    reference.set_speed(0.73);
    reference.set_pad_eq(0, -12.0, -3.0, 4.0);
    assert!(reference.play_sample_at_output_frame(0, 1.0, 0));
    // The requested stopped ON first emits one warm block from the dry baseline.
    reference.set_pad_key_lock(0, true);
    let mut frame = 0;
    paired_range_render(&mut h, &mut reference, &mut frame, 13);
    let full_native = adopt_range_native(&mut h, &mut reference, &mut frame, 1);

    let fifo = range_voice(&h.callback.mixer).stretch.pending_fifo_frames();
    let fed = range_voice(&h.callback.mixer)
        .stretch
        .productive_history()
        .unwrap()
        .fed_output_frames;
    h.producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetStemMixMode {
            id: 0,
            mode: StemMixMode::AllStems,
            source_version_hash: hash,
        })
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert!(reference.set_stem_mix_mode(0, StemMixMode::AllStems, hash));
    assert!(
        h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert_eq!(
        range_voice(&h.callback.mixer)
            .stretch
            .native_state_address(),
        full_native
    );
    assert_eq!(
        range_voice(&h.callback.mixer).stretch.pending_fifo_frames(),
        fifo
    );
    paired_range_render(&mut h, &mut reference, &mut frame, 31);
    assert!(
        h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    let old_components = components.clone();
    let old_source = Arc::downgrade(
        &range_voice(&h.callback.mixer)
            .sample
            .as_ref()
            .unwrap()
            .samples,
    );
    let old = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
    let lease = h.engine.project_assets.cold_lease_for_reader(old).unwrap();
    let generation = selected_path(&h.root, &selected, "wav_generation");
    let registered = h
        .engine
        .project_assets
        .held_reader_backings(&lease, Some(&generation))
        .unwrap();
    assert!(
        registered
            .iter()
            .any(|samples| old_source.ptr_eq(&Arc::downgrade(samples)))
    );
    assert!(old_components.iter().all(|component| {
        registered
            .iter()
            .any(|samples| component.ptr_eq(&Arc::downgrade(samples)))
    }));
    let held_bytes: usize = registered
        .iter()
        .map(|samples| samples.len() * size_of::<f32>())
        .sum();
    drop(registered);
    let refresh = prepare_window_with_producer(
        &h.engine,
        0,
        WindowRequest {
            storage_range: Some((0.0, 6.0 / f64::from(RATE))),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        h.consumer.peek().is_ok() || matches!(refresh.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        refresh.publication_status(),
        "pending",
        "{:?}",
        refresh.error().unwrap()
    );
    let observation = refresh.read_observation_for_test().unwrap();
    assert_eq!(observation.source.read_bytes, 48);
    assert_eq!(observation.stems.read_bytes, [48, 48, 48, 48, 0]);
    assert_eq!(observation.stems.allocated_bytes, 48 * 4);
    assert_eq!(
        observation.admitted_peak_bytes,
        held_bytes + 48 * 5 + 64 * 1024
    );
    let playback_before = range_voice(&h.callback.mixer).source_playback;
    let fifo_before = range_voice(&h.callback.mixer).stretch.pending_fifo_frames();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(refresh.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    assert!(refresh.is_current());
    assert!(
        range_voice(&h.callback.mixer)
            .source_playback
            .matches_exact(&playback_before)
    );
    assert_eq!(
        range_voice(&h.callback.mixer)
            .stretch
            .native_state_address(),
        full_native
    );
    assert_eq!(
        range_voice(&h.callback.mixer).stretch.pending_fifo_frames(),
        fifo_before
    );
    assert!(
        h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    job = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
    components = job
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    assert!(
        components
            .iter()
            .all(|component| !old_components.iter().any(|old| old.ptr_eq(component)))
    );
    h.callback.retirement.retired.clear();
    assert!(old_source.strong_count() > 0);
    assert!(
        old_components
            .iter()
            .all(|component| component.strong_count() > 0),
        "Native A must retain all four outgoing History crops across the actual B ACK"
    );
    for frames in [1, 127, 37] {
        paired_range_render(&mut h, &mut reference, &mut frame, frames);
        assert_eq!(
            range_voice(&h.callback.mixer)
                .stretch
                .native_state_address(),
            full_native
        );
        assert!(
            !range_voice_mut(&mut h.callback.mixer)
                .stretch
                .source_preparation_ready()
        );
    }
    assert!(
        !h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(
        range_voice(&h.callback.mixer)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames
            > fed
    );
    paired_range_render(&mut h, &mut reference, &mut frame, 1);
    let stem_native = adopt_range_native(&mut h, &mut reference, &mut frame, 2);
    wait_until(|| {
        old_components
            .iter()
            .all(|component| component.strong_count() == 0)
    });
    assert_eq!(old_source.strong_count(), 0);

    set_stem_pair_full_mix_with_producer(&h.engine, 0, &h.producer).unwrap();
    reconcile(&h.engine).unwrap();
    assert_eq!(history_count_for_test(&h.engine, 0), 1);
    h.callback.retirement.slots = 0;
    assert_eq!(h.callback.drain(&mut h.consumer), 0);
    assert_eq!(
        h.callback.mixer.stem_demand_for_measurement(0).0,
        StemMixMode::AllStems
    );
    assert!(
        components
            .iter()
            .all(|component| component.strong_count() > 0)
    );
    let fifo = range_voice(&h.callback.mixer).stretch.pending_fifo_frames();
    h.callback.retirement.slots = 3;
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    reference.set_stem_pair_full_mix_rt(0, &mut ImmediateAudioBufferRetirement);
    assert_eq!(
        range_voice(&h.callback.mixer).stretch.pending_fifo_frames(),
        fifo
    );
    assert!(
        h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    h.callback.retirement.slots = 0;
    for frames in [31, 1, 127] {
        paired_range_render(&mut h, &mut reference, &mut frame, frames);
        assert!(
            h.callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
        assert!(h.callback.mixer.stems_for_measurement()[0].is_some());
        assert_eq!(
            range_voice(&h.callback.mixer)
                .stretch
                .native_state_address(),
            stem_native
        );
    }
    // The last FROM samples are consumed before a real bank owner reaches the sink.
    h.callback.retirement.slots = 1;
    paired_range_render(&mut h, &mut reference, &mut frame, 37);
    assert!(
        !h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    assert!(h.callback.mixer.stems_for_measurement()[0].is_none());
    assert_eq!(
        range_voice(&h.callback.mixer)
            .stretch
            .native_state_address(),
        stem_native
    );
    assert_eq!(
        h.callback
            .retirement
            .retired
            .iter()
            .filter(|owner| matches!(owner, RetiredAudioBuffer::PreparedStems(_)))
            .count(),
        2,
        "control and bank owners must reach the actual retirement sink"
    );
    h.callback.retirement.retired.clear();
    assert!(
        components
            .iter()
            .all(|component| component.strong_count() > 0),
        "Job/History remain real owners after FROM and sink retirement"
    );
    drop(job);
    assert!(
        components
            .iter()
            .all(|component| component.strong_count() > 0),
        "native History must outlive the held Job"
    );
    assert!(pair_reader.strong_count() > 0);
    h.callback.retirement.slots = usize::MAX;
    paired_range_render(&mut h, &mut reference, &mut frame, 1);
    let released_native = adopt_range_native(&mut h, &mut reference, &mut frame, 3);
    assert_ne!(released_native, stem_native);
    wait_until(|| {
        components
            .iter()
            .all(|component| component.strong_count() == 0)
    });
    assert_eq!(history_count_for_test(&h.engine, 0), 1);
    reconcile(&h.engine).unwrap();
    assert_eq!(history_count_for_test(&h.engine, 0), 0);
    assert_eq!(
        pair_reader.strong_count(),
        0,
        "final historical verified pair reader must end"
    );
    h.engine.project_assets.collect_for_test();
    assert_complete_selection(&h.root, &selected);
    let continued = paired_range_render(&mut h, &mut reference, &mut frame, 2048);
    assert!(continued.iter().any(|value| value.abs() > 0.001));
    assert_eq!(
        range_voice(&h.callback.mixer)
            .stretch
            .native_state_address(),
        released_native
    );
}

#[test]
fn actual_finite_pair_keylock_window_ack_reads_four_components_then_preserves_native_continuation()
{
    let mut h = Harness::new();
    let source_ticket = h.ticket(0);
    let pair = h.prepare(0, &source_ticket, true, None);
    let _saved = h.save(&pair);
    h.engine
        .publish_stem_pair_with_producer(&pair, &source_ticket, &h.producer)
        .unwrap();
    let full_stems = queued_stems(&mut h.consumer);
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(source_ticket.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    {
        let mut producer = h.producer.lock().unwrap();
        producer
            .push(ControlMessage::SetStemMixMode {
                id: 0,
                mode: StemMixMode::AllStems,
                source_version_hash: full_stems.source_version_hash,
            })
            .unwrap();
        producer
            .push(ControlMessage::SetStemEnabledMask {
                id: 0,
                enabled_stem_mask: 0b1111,
                source_version_hash: full_stems.source_version_hash,
            })
            .unwrap();
    }
    assert_eq!(h.callback.drain(&mut h.consumer), 2);
    h.callback.mixer.set_speed(0.73);
    let ticket = prepare_window_with_producer(
        &h.engine,
        0,
        WindowRequest {
            loop_region: Some((1.0 / f64::from(RATE), Some(6.0 / f64::from(RATE)))),
            key_lock: Some(true),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        h.consumer.peek().is_ok() || matches!(ticket.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        ticket.publication_status(),
        "pending",
        "{:?}",
        ticket.error().unwrap()
    );
    let observation = ticket.read_observation_for_test().unwrap();
    let bytes = 5 * 2 * size_of::<f32>();
    assert_eq!(observation.source.read_bytes, bytes as u64);
    assert_eq!(observation.source.allocated_bytes, bytes);
    assert_eq!(
        (observation.source.start_frame, observation.source.end_frame),
        (1, 6)
    );
    assert_eq!(
        observation.stems.read_bytes,
        [bytes as u64, bytes as u64, bytes as u64, bytes as u64, 0]
    );
    assert_eq!(observation.stems.allocated_bytes, bytes * 4);
    assert_eq!(observation.stems.fresh_opens, 0);
    assert_eq!(observation.stems.integrity_bytes, 0);
    let offset = 2 * size_of::<f32>();
    assert_eq!(
        observation.stems.byte_ranges,
        [
            Some((offset, offset + bytes)),
            Some((offset, offset + bytes)),
            Some((offset, offset + bytes)),
            Some((offset, offset + bytes)),
            None
        ]
    );
    assert!(observation.admitted_peak_bytes < crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES);
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    reconcile(&h.engine).unwrap();
    let finite = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
    let finite_stems = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
    assert_eq!(
        finite.residency.as_ref().unwrap().context,
        ResidentContext::KeyLockFiniteLoop
    );
    assert_eq!(finite.samples.len(), 10);
    assert!(
        finite_stems
            .stems
            .iter()
            .all(|stem| stem.samples.len() == 10 && stem.same_window(&finite))
    );
    assert!(h.callback.mixer.key_lock_for_measurement(0));
    assert!(launch_with_producer(&h.engine, &ticket, false, 1, &h.producer).unwrap());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);

    // The independently retained complete FullMix/four-PCM set supplies the control.
    // The finite native worker receives only the ACKed crops above.
    let full = h.material.sample.clone();
    let mut reference = RtMixer::new(2, RATE as f32);
    let ownership =
        Arc::new(crate::audio_engine::input_runtime_binding::InputRuntimeOwnership::tracked());
    ownership.publish_source(0, &full, RATE, 1);
    reference.set_input_runtime_ownership(ownership);
    reference.load_sample(0, full);
    let mut control_stems = full_stems;
    control_stems.publication =
        crate::audio_engine::prepared_source::PreparedSourcePermit::unrestricted();
    assert!(reference.publish_prepared_stems(0, control_stems.clone()));
    reference.set_stem_mix_mode(0, StemMixMode::AllStems, control_stems.source_version_hash);
    reference.set_stem_enabled_mask(0, 0b1111, control_stems.source_version_hash);
    reference.set_pad_loop_region(0, 1.0 / f64::from(RATE), Some(6.0 / f64::from(RATE)));
    reference.set_speed(0.73);
    assert!(reference.play_sample_at_output_frame(0, 1.0, 0));
    reference.set_pad_key_lock(0, true);
    let render = |mixer: &mut RtMixer, frame: u64, frames: usize| {
        let mut output = vec![0.0; frames * 2];
        mixer.render_at_output_frame(frame, &mut output, &mut [0.0; NUM_SAMPLES]);
        output
    };
    assert_eq!(
        render(&mut h.callback.mixer, 0, 13),
        render(&mut reference, 0, 13)
    );
    wait_until(|| {
        h.callback
            .mixer
            .voices
            .iter_mut()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .stretch
            .source_preparation_ready()
            && reference
                .voices
                .iter_mut()
                .find(|voice| voice.is_playing_sample(0))
                .unwrap()
                .stretch
                .source_preparation_ready()
    });
    let prepared = h
        .callback
        .mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .stretch
        .prepared_native_address();
    let prepared_reads = h
        .callback
        .mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
        .stretch
        .prepared_tap_observation()
        .unwrap();
    assert_eq!(prepared_reads.left_reads, 4096 * 2);
    assert!(prepared_reads.right_reads > 0);
    assert_eq!(prepared_reads.missing_reads, 0);
    assert!(prepared_reads.min_frame.is_some_and(|frame| frame >= 1));
    assert!(prepared_reads.max_frame.is_some_and(|frame| frame < 6));
    assert_ne!(prepared, 0);
    let mut frame = 13;
    let mut audible = false;
    for frames in [
        31, 777, 1024, 1, 2247, 37, 1, 127, 384, 96, 257, 512, 31, 2048, 8192,
    ] {
        reset_tap_observation_for_test();
        let output = render(&mut h.callback.mixer, frame, frames);
        let reads = tap_observation_for_test();
        assert_eq!(reads.left_reads, frames * 2);
        assert_eq!(reads.missing_reads, 0);
        assert!(reads.min_frame.is_some_and(|frame| frame >= 1));
        assert!(reads.max_frame.is_some_and(|frame| frame < 6));
        assert_eq!(output, render(&mut reference, frame, frames));
        frame += frames as u64;
        let actual = h
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        let control = reference
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert!(
            actual
                .source_playback
                .matches_exact(&control.source_playback)
        );
        if frame > 4096 {
            audible |= output.iter().any(|value| value.abs() > 0.001);
            assert_eq!(actual.stretch.native_state_address(), prepared);
            assert_eq!(actual.stretch.adopted_request_id(), Some(1));
            assert_eq!(
                actual.stretch.native_tap_observation(),
                Some(prepared_reads)
            );
            assert_eq!(
                actual.stretch.pending_fifo_frames(),
                control.stretch.pending_fifo_frames()
            );
        }
        assert_eq!(
            actual.sample.as_ref().unwrap().resident_binding(),
            finite.resident_binding()
        );
        assert!(
            h.callback.mixer.stems_for_measurement()[0]
                .as_ref()
                .unwrap()
                .stems
                .iter()
                .all(|stem| stem.samples.len() == 10)
        );
    }
    assert!(
        audible,
        "actual finite four-component native continuation was silent"
    );
    assert!(ticket.is_current());
    let a_source = Arc::downgrade(&finite.samples);
    let a_stems = finite_stems
        .stems
        .each_ref()
        .map(|stem| Arc::downgrade(&stem.samples));
    drop(finite);
    drop(finite_stems);
    let generation_path = selected_path(&h.root, &selection(&pair), "wav_generation");
    for (start, end) in [(0, 6), (1, 7)] {
        let old = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
        let lease = h.engine.project_assets.cold_lease_for_reader(old).unwrap();
        let registered_before_refresh = h
            .engine
            .project_assets
            .held_reader_backings(&lease, Some(&generation_path))
            .unwrap();
        assert!(
            registered_before_refresh
                .iter()
                .any(|samples| a_source.ptr_eq(&Arc::downgrade(samples)))
        );
        assert!(
            a_stems.iter().all(|stem| registered_before_refresh
                .iter()
                .any(|samples| stem.ptr_eq(&Arc::downgrade(samples)))),
            "actual reader registry omitted one or more Native A components"
        );
        let registered_bytes: usize = registered_before_refresh
            .iter()
            .map(|samples| samples.len() * size_of::<f32>())
            .sum();
        let refresh = prepare_window_with_producer(
            &h.engine,
            0,
            WindowRequest {
                storage_range: Some((start as f64 / f64::from(RATE), end as f64 / f64::from(RATE))),
                ..WindowRequest::default()
            },
            h.producer.clone(),
        )
        .unwrap();
        wait_until(|| {
            h.consumer.peek().is_ok()
                || matches!(refresh.publication_status(), "failed" | "cancelled")
        });
        assert_eq!(
            refresh.publication_status(),
            "pending",
            "{:?}",
            refresh.error().unwrap()
        );
        let observation = refresh.read_observation_for_test().unwrap();
        assert_eq!(
            observation.source.read_bytes,
            ((end - start) * 2 * size_of::<f32>()) as u64
        );
        assert_eq!(observation.stems.read_bytes, [48, 48, 48, 48, 0]);
        assert_eq!(observation.stems.allocated_bytes, 48 * 4);
        assert_eq!(
            observation.admitted_peak_bytes,
            registered_bytes + 48 * 5 + 64 * 1024,
            "actual worker peak must charge every unique existing registered reader plus new FullMix/four component crops"
        );
        if start == 1 {
            assert!(
                observation.admitted_peak_bytes >= (40 + 48 + 48) * 5 + 64 * 1024,
                "peak {} missed native A/current B/new C fullmix+four PCM overlap",
                observation.admitted_peak_bytes
            );
        }
        let before = h
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap()
            .stretch
            .pending_fifo_frames();
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(refresh.publication_status(), "accepted");
        reconcile(&h.engine).unwrap();
        assert!(refresh.is_current());
        assert!(a_source.upgrade().is_some());
        assert!(
            a_stems.iter().all(|stem| stem.upgrade().is_some()),
            "storage ACK ended native A component owner"
        );
        let actual = h
            .callback
            .mixer
            .voices
            .iter()
            .find(|voice| voice.is_playing_sample(0))
            .unwrap();
        assert_eq!(actual.stretch.native_state_address(), prepared);
        assert_eq!(actual.stretch.pending_fifo_frames(), before);
        assert_eq!(
            actual.stretch.native_tap_observation(),
            Some(prepared_reads)
        );
        assert_eq!(
            render(&mut h.callback.mixer, frame, 137),
            render(&mut reference, frame, 137)
        );
        frame += 137;
    }
    // Productive mode-only transactions while a selected committed four-component
    // mask ramp is still supplying both sides. The bank physical range remains1..7;
    // the actual loop/taps stay1..6 and no implicit loop edit may cut its phase.
    let hash = control_stems.source_version_hash;
    h.producer
        .lock()
        .unwrap()
        .push(ControlMessage::SetStemEnabledMask {
            id: 0,
            enabled_stem_mask: 0b1010,
            source_version_hash: hash,
        })
        .unwrap();
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert!(reference.set_stem_enabled_mask(0, 0b1010, hash));
    paired_range_render(&mut h, &mut reference, &mut frame, 31);
    for (enabled, frames) in [(false, 31), (true, 1)] {
        let position = range_voice(&h.callback.mixer).source_playback;
        let voice_generation = range_voice(&h.callback.mixer).generation;
        let before_source = h.callback.mixer.bank_for_measurement()[0].clone().unwrap();
        let before_stems = h.callback.mixer.stems_for_measurement()[0].clone().unwrap();
        let toggle = prepare_window_with_producer(
            &h.engine,
            0,
            WindowRequest {
                key_lock: Some(enabled),
                ..Default::default()
            },
            h.producer.clone(),
        )
        .unwrap();
        wait_until(|| {
            h.consumer.peek().is_ok()
                || matches!(toggle.publication_status(), "failed" | "cancelled")
        });
        let observation = toggle.read_observation_for_test().unwrap();
        assert_eq!(
            (observation.source.start_frame, observation.source.end_frame),
            (1, 7)
        );
        // Both mode directions preserve the already registered 1..7 extent. Reuse must
        // retain each actual PCM owner, without reading or allocating four fresh crops.
        assert_eq!(observation.source.read_bytes, 0);
        assert_eq!(observation.source.allocated_bytes, 0);
        assert_eq!(observation.stems.read_bytes, [0; 5]);
        assert_eq!(observation.stems.allocated_bytes, 0);
        assert_eq!(observation.stems.fresh_opens, 0);
        assert_eq!(observation.stems.integrity_bytes, 0);
        match h.consumer.peek().unwrap() {
            ControlMessage::RelocateResident(transaction) => {
                assert!(Arc::ptr_eq(
                    &transaction.sample.samples,
                    &before_source.samples
                ));
                let queued_stems = transaction.stems.as_ref().unwrap();
                for (queued, previous) in queued_stems.stems.iter().zip(&before_stems.stems) {
                    assert!(Arc::ptr_eq(&queued.samples, &previous.samples));
                    assert_eq!((queued.resident_start(), queued.resident_end()), (1, 7));
                }
            }
            _ => panic!("expected actual mode-only resident transaction"),
        }
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(toggle.publication_status(), "accepted");
        reconcile(&h.engine).unwrap();
        let after_source = h.callback.mixer.bank_for_measurement()[0].as_ref().unwrap();
        assert!(Arc::ptr_eq(&after_source.samples, &before_source.samples));
        assert_eq!(
            (after_source.resident_start(), after_source.resident_end()),
            (1, 7)
        );
        let after_stems = h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap();
        for (adopted, previous) in after_stems.stems.iter().zip(&before_stems.stems) {
            assert!(Arc::ptr_eq(&adopted.samples, &previous.samples));
            assert_eq!((adopted.resident_start(), adopted.resident_end()), (1, 7));
        }
        let status = h.engine.input_runtime_ownership.key_lock_status(0).unwrap();
        assert_eq!(
            Some(status.request_id),
            toggle.key_lock_request_id_for_test()
        );
        assert_eq!(status.window_revision, after_source.window_revision());
        assert!(toggle.is_current());
        assert!(
            range_voice(&h.callback.mixer)
                .source_playback
                .matches_exact(&position)
        );
        assert_eq!(range_voice(&h.callback.mixer).generation, voice_generation);
        assert!(
            h.callback
                .mixer
                .stem_transition_for_measurement(0)
                .is_active()
        );
        assert_eq!(h.callback.mixer.key_lock_for_measurement(0), enabled);
        reference.set_pad_key_lock(0, enabled);
        paired_range_render(&mut h, &mut reference, &mut frame, frames);
    }
    for frames in [31, 127, 1] {
        paired_range_render(&mut h, &mut reference, &mut frame, frames);
    }
    assert!(
        !h.callback
            .mixer
            .stem_transition_for_measurement(0)
            .is_active()
    );
    let resumed_native = adopt_range_native(&mut h, &mut reference, &mut frame, 2);
    assert_ne!(resumed_native, prepared);
    let status = h.engine.input_runtime_ownership.key_lock_status(0).unwrap();
    assert!(status.effective && status.ready);
    assert_eq!(status.state, "wet");
}

struct PadSnapshot {
    sample: SampleBuffer,
    stems: PreparedStemSet,
    sample_bits: Vec<u32>,
    stem_bits: [Vec<u32>; 4],
    binding: InputPadBinding,
    source_generation: u64,
    request: u64,
    loaded_generation: (u64, u32),
    digest: String,
    preparation_epoch: u64,
    timing_epoch: u64,
    stem_demand: (StemMixMode, u8, u64),
}

fn bits(sample: &SampleBuffer) -> Vec<u32> {
    sample.samples.iter().map(|value| value.to_bits()).collect()
}

fn acknowledge_full_mix_window(h: &mut Harness, id: usize, frames: usize) {
    assert!(h.callback.mixer.stems_for_measurement()[id].is_none());
    let before = h.callback.mixer.bank_for_measurement()[id]
        .as_ref()
        .unwrap()
        .clone();
    let ticket = prepare_window_with_producer(
        &h.engine,
        id,
        WindowRequest {
            storage_range: Some((0.0, frames as f64 / f64::from(RATE))),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    assert_eq!(ticket.publication_status(), "pending");
    assert!(ticket.read_observation_for_test().is_none());
    match h.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert_eq!(transaction.id, id);
            assert!(transaction.stems.is_none());
            assert!(Arc::ptr_eq(&before.samples, &transaction.sample.samples));
            assert_eq!(
                (
                    transaction.sample.resident_start(),
                    transaction.sample.resident_end()
                ),
                (0, frames)
            );
            assert_eq!(
                transaction.sample.residency.as_ref().unwrap().context,
                ResidentContext::FullTrack
            );
        }
        _ => panic!("pad {id}: expected actual FULL MIX window transaction"),
    }
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    reconcile(&h.engine).unwrap();
    assert!(h.callback.mixer.stems_for_measurement()[id].is_none());
    let binding = capture(&h.engine, id).unwrap().unwrap();
    assert!(binding.current() && binding.available());
    assert!(binding.binding.accepted.is_none());
    assert_eq!(
        h.engine.current_timing_acknowledgements.current_epoch(id),
        0
    );
    assert!(Arc::ptr_eq(
        &before.samples,
        &h.callback.mixer.bank_for_measurement()[id]
            .as_ref()
            .unwrap()
            .samples
    ));
}

fn snapshot(h: &Harness, id: usize) -> PadSnapshot {
    let sample = h.callback.mixer.bank_for_measurement()[id]
        .as_ref()
        .unwrap()
        .clone();
    let stems = h.callback.mixer.stems_for_measurement()[id]
        .as_ref()
        .unwrap()
        .clone();
    let binding = capture(&h.engine, id).unwrap().unwrap();
    assert!(binding.current() && binding.available(), "pad {id}");
    assert!(binding.binding.accepted.is_none());
    assert!(stems.accepted_timing.is_none());
    assert_eq!(stems.publication.status(), "accepted");
    assert_eq!(stems.available_mask, 0b1111);
    let stem_demand = h.callback.mixer.stem_demand_for_measurement(id);
    assert_eq!(
        stem_demand,
        (StemMixMode::AllStems, 0b1111, stems.source_version_hash)
    );
    assert_eq!(
        h.engine.current_timing_acknowledgements.current_epoch(id),
        0
    );
    assert!(Arc::ptr_eq(&sample.samples, &stems.reference_samples));
    assert!(stems.stems.iter().all(|stem| stem.same_window(&sample)));
    assert!(Arc::ptr_eq(
        &sample.samples,
        &h.engine.sample_cache.lock().unwrap()[id]
            .as_ref()
            .unwrap()
            .samples,
    ));
    PadSnapshot {
        sample_bits: bits(&sample),
        stem_bits: std::array::from_fn(|component| bits(&stems.stems[component])),
        sample,
        stems,
        binding: binding.binding,
        source_generation: binding.source_generation,
        request: h.engine.pad_request_ids.lock().unwrap()[id],
        loaded_generation: h.engine.loaded_source_generations.lock().unwrap()[id],
        digest: h.engine.loaded_source_digests.lock().unwrap()[id]
            .clone()
            .unwrap(),
        preparation_epoch: h.engine.prepared_source_epochs[id].load(Ordering::Acquire),
        timing_epoch: h.engine.current_timing_acknowledgements.current_epoch(id),
        stem_demand,
    }
}

fn assert_preserved(h: &Harness, id: usize, before: &PadSnapshot) {
    let after = snapshot(h, id);
    assert!(
        Arc::ptr_eq(&before.sample.samples, &after.sample.samples),
        "pad {id}"
    );
    assert!(Arc::ptr_eq(
        &before.sample.residency.as_ref().unwrap().source,
        &after.sample.residency.as_ref().unwrap().source,
    ));
    assert_eq!(
        before.sample.resident_binding(),
        after.sample.resident_binding()
    );
    assert_eq!(before.sample_bits, after.sample_bits, "pad {id}");
    assert_eq!(
        before.stems.source_version_hash,
        after.stems.source_version_hash
    );
    assert_eq!(before.stems.sample_rate_hz, after.stems.sample_rate_hz);
    assert_eq!(before.stems.channels, after.stems.channels);
    assert_eq!(before.stems.frame_count, after.stems.frame_count);
    assert_eq!(before.stems.available_mask, after.stems.available_mask);
    assert!(Arc::ptr_eq(
        &before.stems.complete_set_identity,
        &after.stems.complete_set_identity,
    ));
    for component in 0..4 {
        assert!(
            Arc::ptr_eq(
                &before.stems.stems[component].samples,
                &after.stems.stems[component].samples,
            ),
            "pad {id}, component {component}"
        );
        assert_eq!(
            before.stems.stems[component].resident_binding(),
            after.stems.stems[component].resident_binding(),
        );
        assert_eq!(before.stem_bits[component], after.stem_bits[component]);
    }
    assert_eq!(before.binding, after.binding, "pad {id}");
    assert_eq!(before.source_generation, after.source_generation);
    assert_eq!(before.request, after.request);
    assert_eq!(before.loaded_generation, after.loaded_generation);
    assert_eq!(before.digest, after.digest);
    assert_eq!(before.preparation_epoch, after.preparation_epoch);
    assert_eq!(before.timing_epoch, after.timing_epoch);
    assert_eq!(before.stem_demand, after.stem_demand);
}

fn finite_window(h: &mut Harness, id: usize, start: usize, end: usize) -> ResidentWindowTicket {
    assert!(h.consumer.peek().is_err());
    let ticket = prepare_window_with_producer(
        &h.engine,
        id,
        WindowRequest {
            loop_region: Some((
                start as f64 / f64::from(RATE),
                Some(end as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    wait_until(|| {
        h.consumer.peek().is_ok() || matches!(ticket.publication_status(), "failed" | "cancelled")
    });
    assert_eq!(
        ticket.publication_status(),
        "pending",
        "pad {id}: {:?}",
        ticket.error().unwrap()
    );
    let (sample, stems) = match h.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert_eq!(transaction.id, id);
            assert_eq!(transaction.sample.resident_start(), start);
            assert_eq!(transaction.sample.resident_end(), end);
            assert_eq!(
                transaction.sample.residency.as_ref().unwrap().context,
                ResidentContext::FiniteLoop,
            );
            let stems = transaction.stems.as_ref().unwrap();
            assert_eq!(stems.stems.len(), 4);
            assert!(
                stems
                    .stems
                    .iter()
                    .all(|stem| stem.same_window(&transaction.sample))
            );
            (transaction.sample.clone(), stems.clone())
        }
        _ => panic!("pad {id}: expected actual window command"),
    };
    let observation = ticket.read_observation_for_test().unwrap();
    let bytes = (end - start) * 2 * size_of::<f32>();
    assert_eq!(observation.source.read_bytes, bytes as u64, "pad {id}");
    assert_eq!(observation.source.allocated_bytes, bytes);
    assert_eq!(
        (observation.source.start_frame, observation.source.end_frame),
        (start, end)
    );
    assert_eq!(
        observation.stems.read_bytes,
        [bytes as u64, bytes as u64, bytes as u64, bytes as u64, 0]
    );
    assert_eq!(observation.stems.allocated_bytes, bytes * 4);
    assert_eq!(observation.stems.fresh_opens, 0);
    assert_eq!(observation.stems.integrity_bytes, 0);
    let byte_start = start * 2 * size_of::<f32>();
    assert_eq!(
        observation.stems.byte_ranges,
        [
            Some((byte_start, byte_start + bytes)),
            Some((byte_start, byte_start + bytes)),
            Some((byte_start, byte_start + bytes)),
            Some((byte_start, byte_start + bytes)),
            None,
        ]
    );
    assert!(observation.admitted_peak_bytes < crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES);
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(ticket.publication_status(), "accepted");
    assert!(ticket.is_current());
    reconcile(&h.engine).unwrap();
    let actual = snapshot(h, id);
    assert!(Arc::ptr_eq(&sample.samples, &actual.sample.samples));
    for component in 0..4 {
        assert!(Arc::ptr_eq(
            &stems.stems[component].samples,
            &actual.stems.stems[component].samples
        ));
    }
    ticket
}

fn render_usable(h: &mut Harness, id: usize, ticket: &ResidentWindowTicket, before: &PadSnapshot) {
    assert_preserved(h, id, before);
    assert!(ticket.is_current());
    assert!(h.callback.mixer.voices.iter().all(|voice| !voice.active));
    assert!(launch_with_producer(&h.engine, ticket, false, 1, &h.producer).unwrap());
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    let voice = h
        .callback
        .mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(id))
        .unwrap();
    assert!(Arc::ptr_eq(
        &voice.sample.as_ref().unwrap().samples,
        &before.sample.samples
    ));
    assert_eq!(
        h.callback
            .mixer
            .voices
            .iter()
            .filter(|voice| voice.active)
            .count(),
        1
    );
    let mut output = [0.0; 4];
    let mut peaks = [0.0; NUM_SAMPLES];
    h.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut peaks,
        0,
        &mut h.callback.retirement,
    );
    assert!(output.iter().all(|value| value.is_finite()), "pad {id}");
    assert!(output.iter().any(|value| *value != 0.0), "pad {id} silent");
    assert!(peaks[id] > 0.0, "pad {id} missing actual render activity");
    assert!(enqueue_stop_with_producer(
        &h.engine.input_runtime_ownership,
        &mut h.producer.lock().unwrap(),
        Some(id),
    ));
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert!(h.callback.mixer.voices.iter().all(|voice| !voice.active));
    assert_preserved(h, id, before);
}

#[test]
fn all_216_actual_pair_windows_preserve_other_215_on_both_endpoint_edits() {
    assert_eq!(NUM_SAMPLES, 216);
    let mut h = Harness::new();
    // Reopen a genuine checked preparation: Harness released its creator-only
    // registration after its initial source ACKs. Never clone that retired right.
    let primary =
        prepare_material(&h.root, &h.material.lease.original_path, RATE, 2, &|| false).unwrap();
    let second_path = h.root.join("second.wav");
    fs::write(&second_path, stereo_wav(11)).unwrap();
    let secondary = prepare_material(&h.root, &second_path, RATE, 2, &|| false).unwrap();
    let _secondary_original_owner = h
        .engine
        .project_assets
        .acquire(&h.root, &secondary.lease.original_path)
        .unwrap();
    let versions = [canonical_version(&primary), canonical_version(&secondary)];
    assert_ne!(
        primary.metadata()["material_id"],
        secondary.metadata()["material_id"]
    );
    assert_ne!(
        primary.metadata()["original"]["sha256"],
        secondary.metadata()["original"]["sha256"]
    );
    assert_eq!(
        (primary.sample.frame_count(), secondary.sample.frame_count()),
        (7, 11)
    );
    let frames = [primary.sample.frame_count(), secondary.sample.frame_count()];
    let references = [
        h.wav_reference.clone(),
        format!(
            "samples/materials/M{}/stems/.ready-{GENERATION}",
            secondary.metadata()["material_id"].as_str().unwrap(),
        ),
    ];
    write_complete_wavs(
        &h.root.parent().unwrap().join(&references[1]),
        &stereo_wav(frames[1]),
        &versions[1],
    );
    let preparations = [
        control::prepared_for_test(&h.engine, primary, &h.root).unwrap(),
        control::prepared_for_test(&h.engine, secondary, &h.root).unwrap(),
    ];
    let mut source_tickets = Vec::new();
    // Serial admission preserves the production two-worker/32-job limits and
    // the existing eight-command ring; this is all-bank readiness, not load stress.
    for id in 0..NUM_SAMPLES {
        let ticket = control::adopt_for_format(
            &h.engine,
            id,
            &preparations[id % 2],
            h.producer.clone(),
            (2, RATE, h.root.clone()),
        )
        .unwrap();
        wait_until(|| h.consumer.peek().is_ok());
        assert_eq!(ticket.sample_id(), id);
        assert_eq!(ticket.phase().unwrap(), "pending");
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        match terminal(&h.engine, ticket.request_id()) {
            LoaderEvent::Success {
                id: actual_id,
                request_id,
                ..
            } => {
                assert_eq!(actual_id, id);
                assert_eq!(request_id, ticket.request_id());
            }
            event => panic!("pad {id}: source failed: {event:?}"),
        }
        wait_until(|| h.engine.cold_loading[id].load(Ordering::Acquire) == 0);
        assert_eq!(ticket.phase().unwrap(), "acknowledged");
        assert!(ticket.is_current().unwrap());
        source_tickets.push(ticket);
    }
    for preparation in &preparations {
        preparation.release_preparation().unwrap();
    }
    for id in 0..NUM_SAMPLES {
        acknowledge_full_mix_window(&mut h, id, frames[id % 2]);
        assert!(source_tickets[id].is_current().unwrap());
    }

    let mut saved_owners = Vec::new();
    for id in 0..NUM_SAMPLES {
        let group = id % 2;
        let ticket = h
            .engine
            .capture_prepared_source(id, versions[group].clone())
            .unwrap();
        assert_eq!(ticket.publication_status(), "captured");
        let pair = h
            .engine
            .prepare_stem_pair_at_root(
                &h.root,
                id,
                &versions[group],
                &references[group],
                &ticket,
                true,
                None,
            )
            .unwrap();
        assert!(pair.has_components());
        assert_complete_selection(&h.root, &selection(&pair));
        if id < 2 {
            saved_owners.push(h.save(&pair));
        } else {
            pair.select();
        }
        h.engine
            .publish_stem_pair_with_producer(&pair, &ticket, &h.producer)
            .unwrap();
        assert_eq!(ticket.publication_status(), "pending");
        let queued = queued_stems(&mut h.consumer);
        assert_eq!(h.callback.drain(&mut h.consumer), 1);
        assert_eq!(ticket.publication_status(), "accepted");
        reconcile(&h.engine).unwrap();
        // Actual ALL demand, including all four component bits, is delivered
        // through the ordinary fixed commands; availability alone is insufficient.
        {
            let mut producer = h.producer.lock().unwrap();
            producer
                .push(ControlMessage::SetStemMixMode {
                    id,
                    mode: StemMixMode::AllStems,
                    source_version_hash: queued.source_version_hash,
                })
                .unwrap();
            producer
                .push(ControlMessage::SetStemEnabledMask {
                    id,
                    enabled_stem_mask: 0b1111,
                    source_version_hash: queued.source_version_hash,
                })
                .unwrap();
        }
        assert_eq!(h.callback.drain(&mut h.consumer), 2);
        let actual = h.callback.mixer.stems_for_measurement()[id]
            .as_ref()
            .unwrap();
        for component in 0..4 {
            assert!(Arc::ptr_eq(
                &queued.stems[component].samples,
                &actual.stems[component].samples
            ));
            if id >= 2 {
                let shared = h.callback.mixer.stems_for_measurement()[group]
                    .as_ref()
                    .unwrap();
                assert!(Arc::ptr_eq(
                    &shared.stems[component].samples,
                    &actual.stems[component].samples
                ));
            }
        }
    }
    assert_eq!(saved_owners.len(), 2);
    assert!(!Arc::ptr_eq(
        &h.callback.mixer.stems_for_measurement()[0]
            .as_ref()
            .unwrap()
            .complete_set_identity,
        &h.callback.mixer.stems_for_measurement()[1]
            .as_ref()
            .unwrap()
            .complete_set_identity,
    ));
    let mut windows = Vec::new();
    for id in 0..NUM_SAMPLES {
        windows.push(finite_window(&mut h, id, 1, frames[id % 2] - 1));
        assert!(source_tickets[id].is_current().unwrap());
    }
    assert_eq!(windows.len(), 216);
    let held_bytes: usize = (0..NUM_SAMPLES)
        .map(|id| {
            let sample = h.callback.mixer.bank_for_measurement()[id]
                .as_ref()
                .unwrap();
            let stems = h.callback.mixer.stems_for_measurement()[id]
                .as_ref()
                .unwrap();
            (sample.samples.len()
                + stems
                    .stems
                    .iter()
                    .map(|stem| stem.samples.len())
                    .sum::<usize>())
                * size_of::<f32>()
        })
        .sum();
    // Even counting shared buffers repeatedly, the actual tiny stereo test fits
    // the supported 128-MiB minimum. This is allocated PCM, never RSS evidence.
    assert!(held_bytes < 128 * 1024 * 1024);

    for edited in [0, 215] {
        let before: Vec<_> = (0..NUM_SAMPLES).map(|id| snapshot(&h, id)).collect();
        let changed = finite_window(&mut h, edited, 2, frames[edited % 2] - 1);
        let after = snapshot(&h, edited);
        assert!(!Arc::ptr_eq(
            &before[edited].sample.samples,
            &after.sample.samples
        ));
        assert_eq!(
            after.sample.window_revision(),
            before[edited].sample.window_revision() + 1
        );
        assert!(Arc::ptr_eq(
            &before[edited].stems.complete_set_identity,
            &after.stems.complete_set_identity
        ));
        assert_eq!(before[edited].source_generation, after.source_generation);
        assert_eq!(before[edited].digest, after.digest);
        windows[edited] = changed;
        for id in 0..NUM_SAMPLES {
            assert!(source_tickets[id].is_current().unwrap());
            if id != edited {
                assert_preserved(&h, id, &before[id]);
            }
        }
        for id in 0..NUM_SAMPLES {
            if id != edited {
                render_usable(&mut h, id, &windows[id], &before[id]);
            }
        }
    }

    // An exact eligible view takes the actual worker-free Arc-sharing branch,
    // yet still needs this new ticket's ordinary pending -> callback ACK.
    let before = snapshot(&h, 2);
    let reused = prepare_window_with_producer(
        &h.engine,
        2,
        WindowRequest {
            loop_region: Some((
                1.0 / f64::from(RATE),
                Some((frames[0] - 1) as f64 / f64::from(RATE)),
            )),
            ..WindowRequest::default()
        },
        h.producer.clone(),
    )
    .unwrap();
    assert_eq!(reused.publication_status(), "pending");
    assert!(reused.read_observation_for_test().is_none());
    match h.consumer.peek().unwrap() {
        ControlMessage::RelocateResident(transaction) => {
            assert!(Arc::ptr_eq(
                &before.sample.samples,
                &transaction.sample.samples
            ));
            for component in 0..4 {
                assert!(Arc::ptr_eq(
                    &before.stems.stems[component].samples,
                    &transaction.stems.as_ref().unwrap().stems[component].samples
                ));
            }
        }
        _ => panic!("expected own reused-view native command"),
    }
    assert_eq!(h.callback.drain(&mut h.consumer), 1);
    assert_eq!(reused.publication_status(), "accepted");
    assert!(reused.is_current());
    reconcile(&h.engine).unwrap();
    let after = snapshot(&h, 2);
    assert!(Arc::ptr_eq(&before.sample.samples, &after.sample.samples));
    assert_eq!(
        before.sample.window_revision(),
        after.sample.window_revision()
    );
}
