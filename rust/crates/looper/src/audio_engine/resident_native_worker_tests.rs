//! Actual finite WindowWork range I/O, its own callback ACK, then native catch-up/continuation.
use super::*;
use crate::audio_engine::input_runtime_binding::{InputRuntimeOwnership, capture};
use crate::audio_engine::resident_relocation::launch_with_producer;
use crate::audio_engine::source_reader::{
    reset_tap_observation_for_test, tap_observation_for_test,
};

fn voice(mixer: &RtMixer) -> &crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn voice_mut(mixer: &mut RtMixer) -> &mut crate::audio_engine::voice_slot::VoiceSlot {
    mixer
        .voices
        .iter_mut()
        .find(|voice| voice.is_playing_sample(0))
        .unwrap()
}

fn render(loaded: &mut Loaded, frame: u64, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * 2];
    loaded.callback.mixer.render_rt_at_output_frame(
        &mut output,
        &mut [0.0; NUM_SAMPLES],
        frame,
        &mut loaded.callback.retirement,
    );
    output
}

fn full_oracle(loaded: &Loaded, start: usize, end: usize) -> RtMixer {
    // Construct complete PCM independently from the generated original WAV integers.
    // No productive range reader or finite sample supplies the control's samples.
    let sample = SampleBuffer {
        channels: 2,
        residency: None,
        samples: Arc::from(
            loaded
                .mono
                .iter()
                .flat_map(|value| [*value; 2])
                .collect::<Vec<_>>(),
        ),
    }
    .with_complete_source(48_000);
    let mut mixer = RtMixer::new(2, 48_000.0);
    let ownership = Arc::new(InputRuntimeOwnership::tracked());
    ownership.publish_source(0, &sample, 48_000, 1);
    mixer.set_input_runtime_ownership(ownership);
    mixer.load_sample(0, sample);
    mixer.set_pad_loop_region(0, start as f64 / 48_000.0, Some(end as f64 / 48_000.0));
    mixer.set_speed(0.73);
    mixer.set_pad_key_lock(0, true);
    assert!(mixer.play_sample_at_output_frame(0, 1.0, 0));
    mixer
}

#[test]
fn actual_finite_keylock_window_worker_ack_feeds_native_and_continues_full_pcm_oracle() {
    let mut loaded = Loaded::new();
    let before = capture(&loaded.engine, 0).unwrap().unwrap();
    loaded.callback.mixer.set_speed(0.73);
    let ticket = loaded.prepare(WindowRequest {
        loop_region: Some((24.0 / 48_000.0, Some(96.0 / 48_000.0))),
        key_lock: Some(true),
        ..WindowRequest::default()
    });
    loaded.pending(&ticket);
    let observation = ticket.read_observation_for_test().unwrap();
    let bytes = (96 - 24) * 2 * std::mem::size_of::<f32>();
    assert_eq!(observation.source.read_bytes, bytes as u64);
    assert_eq!(observation.source.allocated_bytes, bytes);
    assert_eq!(
        (observation.source.start_frame, observation.source.end_frame),
        (24, 96)
    );
    assert_eq!(observation.stems.read_bytes, [0; 5]);
    assert_eq!(observation.stems.allocated_bytes, 0);
    assert!(observation.admitted_peak_bytes < crate::audio_engine::cold_jobs::PCM_LIMIT_BYTES);
    loaded.adopt(&ticket);
    let finite = loaded.sample();
    assert_eq!((finite.resident_start(), finite.resident_end()), (24, 96));
    assert_eq!(finite.samples.len(), (96 - 24) * 2);
    assert_eq!(
        finite.residency.as_ref().unwrap().context,
        ResidentContext::KeyLockFiniteLoop
    );
    assert_eq!(finite.frame_count(), loaded.mono.len());
    assert!(!before.current());
    let after = capture(&loaded.engine, 0).unwrap().unwrap();
    assert!(after.current() && after.available());
    assert_eq!(after.binding.resident, finite.resident_binding());
    assert!(loaded.callback.mixer.key_lock_for_measurement(0));
    assert!(launch_with_producer(&loaded.engine, &ticket, false, 1, &loaded.producer).unwrap());
    assert_eq!(loaded.callback.drain(&mut loaded.consumer), 1);
    let mut reference = full_oracle(&loaded, 24, 96);
    let mut peaks = [0.0; NUM_SAMPLES];
    let mut expected = vec![0.0; 26];
    reference.render_at_output_frame(0, &mut expected, &mut peaks);
    assert_eq!(render(&mut loaded, 0, 13), expected);
    wait_until(Duration::from_secs(5), || {
        voice_mut(&mut loaded.callback.mixer)
            .stretch
            .source_preparation_ready()
            && voice_mut(&mut reference).stretch.source_preparation_ready()
    });
    let prepared = voice_mut(&mut loaded.callback.mixer)
        .stretch
        .prepared_native_address();
    let prepared_reads = voice_mut(&mut loaded.callback.mixer)
        .stretch
        .prepared_tap_observation()
        .unwrap();
    assert_eq!(prepared_reads.left_reads, 4096 * 2);
    assert!(prepared_reads.right_reads > 0);
    assert_eq!(prepared_reads.missing_reads, 0);
    assert!(prepared_reads.min_frame.is_some_and(|frame| frame >= 24));
    assert!(prepared_reads.max_frame.is_some_and(|frame| frame < 96));
    assert_ne!(prepared, 0);
    assert_ne!(
        prepared,
        voice(&loaded.callback.mixer).stretch.native_state_address()
    );
    assert_eq!(
        voice_mut(&mut loaded.callback.mixer)
            .stretch
            .prepared_fifo_frames(),
        Some((0, 511))
    );
    let mut frame = 13;
    let mut audible = false;
    for frames in [
        31, 777, 1024, 1, 2247, 37, 1, 127, 384, 96, 257, 512, 31, 2048, 8192,
    ] {
        reset_tap_observation_for_test();
        let actual = render(&mut loaded, frame, frames);
        let reads = tap_observation_for_test();
        assert_eq!(reads.left_reads, frames * 2);
        assert_eq!(reads.missing_reads, 0);
        assert!(reads.min_frame.is_some_and(|frame| frame >= 24));
        assert!(reads.max_frame.is_some_and(|frame| frame < 96));
        let mut expected = vec![0.0; frames * 2];
        reference.render_at_output_frame(frame, &mut expected, &mut peaks);
        assert_eq!(actual, expected, "finite worker/native at {frame}");
        frame += frames as u64;
        if frame > 4096 {
            audible |= actual.iter().any(|value| value.abs() > 0.001);
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.native_state_address(),
                prepared
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.adopted_request_id(),
                Some(1)
            );
            assert_eq!(
                voice(&loaded.callback.mixer)
                    .stretch
                    .native_tap_observation(),
                Some(prepared_reads)
            );
            assert_eq!(
                voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
                voice(&reference).stretch.pending_fifo_frames()
            );
        }
        assert!(
            voice(&loaded.callback.mixer)
                .source_playback
                .matches_exact(&voice(&reference).source_playback)
        );
        assert_eq!(
            voice(&loaded.callback.mixer)
                .sample
                .as_ref()
                .unwrap()
                .resident_binding(),
            finite.resident_binding()
        );
    }
    assert!(
        audible,
        "actual finite worker native continuation was silent"
    );
    assert!(ticket.is_current());
    let a_binding = finite.resident_binding();
    let a_backing = Arc::downgrade(&finite.samples);
    drop(finite);
    let history_before_refresh = voice(&loaded.callback.mixer)
        .stretch
        .productive_history()
        .unwrap();
    let fifo_before_refresh = voice(&loaded.callback.mixer).stretch.pending_fifo_frames();
    let b = loaded.prepare(WindowRequest {
        storage_range: Some((20.0 / 48_000.0, 100.0 / 48_000.0)),
        ..WindowRequest::default()
    });
    loaded.adopt(&b);
    assert!(!ticket.is_current());
    assert!(
        a_backing.upgrade().is_some(),
        "native A owner ended at storage B ACK"
    );
    assert_ne!(loaded.sample().resident_binding(), a_binding);
    assert_eq!(
        voice(&loaded.callback.mixer).stretch.native_state_address(),
        prepared
    );
    assert_eq!(
        voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
        fifo_before_refresh
    );
    assert_eq!(
        voice(&loaded.callback.mixer)
            .stretch
            .productive_history()
            .unwrap()
            .fed_output_frames,
        history_before_refresh.fed_output_frames
    );
    let lease = loaded
        .engine
        .project_assets
        .cold_lease_for_reader(&loaded.sample())
        .unwrap();
    let registered_before_c = loaded
        .engine
        .project_assets
        .held_reader_backings(&lease, None)
        .unwrap();
    assert!(
        registered_before_c
            .iter()
            .any(|samples| a_backing.ptr_eq(&Arc::downgrade(samples))),
        "actual registry snapshot omitted Native A backing"
    );
    let registered_bytes: usize = registered_before_c
        .iter()
        .map(|samples| samples.len() * std::mem::size_of::<f32>())
        .sum();
    let c = loaded.prepare(WindowRequest {
        storage_range: Some((16.0 / 48_000.0, 112.0 / 48_000.0)),
        ..WindowRequest::default()
    });
    loaded.pending(&c);
    let peak = c.read_observation_for_test().unwrap();
    let a_bytes = (96 - 24) * 2 * std::mem::size_of::<f32>();
    let b_bytes = (100 - 20) * 2 * std::mem::size_of::<f32>();
    let c_bytes = (112 - 16) * 2 * std::mem::size_of::<f32>();
    assert_eq!(peak.source.read_bytes, c_bytes as u64);
    assert_eq!(peak.source.allocated_bytes, c_bytes);
    assert_eq!(
        peak.admitted_peak_bytes,
        registered_bytes + c_bytes + 64 * 1024,
        "actual peak did not charge the unique registry snapshot and new C/read scratch"
    );
    assert!(
        peak.admitted_peak_bytes >= a_bytes + b_bytes + c_bytes + 64 * 1024,
        "peak {} did not include native A, current B, new C and read scratch",
        peak.admitted_peak_bytes
    );
    loaded.adopt(&c);
    assert!(c.is_current());
    assert!(a_backing.upgrade().is_some());
    assert_eq!(
        voice(&loaded.callback.mixer).stretch.native_state_address(),
        prepared
    );
    assert_eq!(
        voice(&loaded.callback.mixer).stretch.pending_fifo_frames(),
        fifo_before_refresh
    );
    for frames in [1, 17, 127, 384, 777] {
        let actual = render(&mut loaded, frame, frames);
        let mut expected = vec![0.0; frames * 2];
        reference.render_at_output_frame(frame, &mut expected, &mut peaks);
        assert_eq!(
            actual, expected,
            "native A continuation after storage C ACK"
        );
        frame += frames as u64;
        assert_eq!(
            voice(&loaded.callback.mixer).stretch.native_state_address(),
            prepared
        );
        assert_eq!(
            voice(&loaded.callback.mixer)
                .stretch
                .native_tap_observation(),
            Some(prepared_reads)
        );
    }
}
