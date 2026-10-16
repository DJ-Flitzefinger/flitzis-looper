// Included in key_lock_tuple_tests.rs to reuse its real public-start fixtures.
// This unchanged-pitch continuation proof does not certify acoustic or device gates.

fn same_pitch_started_fixture(extra: i32, mode: bool) -> Fixture {
    use crate::audio_engine::ordinary_endpoint_reserve_tests::{
        adopt_current_sources, refresh_actual_runtime_bindings,
    };

    let mut fixture = complete_fixture(mode);
    // A direct fixture-bank installation is not a callback SourceACK.
    // Adopt the actual retained source through the production LoadSample claim.
    adopt_current_sources(&mut fixture, mode);
    refresh_actual_runtime_bindings(&fixture);
    start(&mut fixture, extra, mode, 1.0);
    fixture
}

fn same_pitch_public_rate(fixture: &mut Fixture, speed: f64) {
    // Reuse the real parameter-channel seam from the public unity-birth tests.
    // The control producer used by Fixture::drain does not carry SetSpeed.
    let (parameters, mut consumer) =
        rtrb::RingBuffer::<crate::messages::ControlParameterMessage>::new(8);
    fixture.control.engine.test_parameter_producer =
        Some(std::sync::Arc::new(std::sync::Mutex::new(parameters)));
    fixture.control.engine.set_speed(speed).unwrap();
    crate::audio_engine::audio_stream::drain_parameter_messages(
        &mut consumer,
        &mut fixture.mixer,
        &mut fixture.transport,
    );
    fixture.drain();
    fixture.render(0);
    assert!(consumer.is_empty());
}

fn prove_same_pitch_mode_continuation(extra: i32, old: bool) {
    let mut fixture = same_pitch_started_fixture(extra, old);
    let mut reference = same_pitch_started_fixture(extra, old);
    // At r=1 the requested destination mode has the identical native pitch.
    // Its independently born Native preserves that complete history without
    // a mode request and is the oracle for the later real speed transition.
    let mut target_reference = same_pitch_started_fixture(extra, !old);
    assert_eq!(fixture.starts(0), 1);
    assert_eq!(reference.starts(0), 1);
    assert_eq!(target_reference.starts(0), 1);
    let past = fixture.pcm(733);
    assert_eq!(
        past,
        reference.pcm(733),
        "independent Native owners must render the same original past"
    );
    assert_eq!(past, target_reference.pcm(733));
    let f = fixture.transport.output_frame();
    assert_eq!(reference.transport.output_frame(), f);
    assert_eq!(target_reference.transport.output_frame(), f);
    let native = fixture.voice(0).stretch.native_state_address();
    let origin = fixture.voice(0).stretch.native_origin_tuple().unwrap();
    assert_eq!(
        (origin.0, origin.1),
        (PitchIntent::new(0, extra).unwrap(), old)
    );
    let reference_native = reference.voice(0).stretch.native_state_address();
    let target_native = target_reference.voice(0).stretch.native_state_address();
    let target_generation = target_reference.voice(0).generation;
    assert_ne!(native, 0);
    assert_ne!(
        native, reference_native,
        "reference must own an independent Native"
    );
    assert_ne!(
        native, target_native,
        "target reference must own an independent Native"
    );
    let generation = fixture.voice(0).generation;
    let starts = fixture.starts(0);
    let phase = fixture.voice(0).source_playback.position();
    let fifos = fixture.voice(0).stretch.pending_fifo_frames();
    assert_eq!(phase, reference.voice(0).source_playback.position());
    assert_eq!(fifos, reference.voice(0).stretch.pending_fifo_frames());
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 1.0);

    wait(|| {
        assert_eq!(fixture.transport.output_frame(), f);
        fixture
            .mixer
            .voices
            .iter_mut()
            .filter(|voice| voice.active && voice.sample_id == 0)
            .all(|voice| voice.stretch.source_history_work_available())
    });
    let request = fixture.control.engine.set_pad_key_lock(0, !old).unwrap();
    fixture.drain();
    let waiting = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(waiting.request_id, request);
    assert_eq!(waiting.state, "waiting");
    assert_eq!(waiting.effective, old);
    assert!(!waiting.ready);
    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
    assert_eq!(fixture.voice(0).source_playback.position(), phase);
    assert_eq!(fixture.voice(0).stretch.pending_fifo_frames(), fifos);

    let first = fixture.render(1);
    assert_eq!(first, reference.render(1));
    assert_eq!(first, target_reference.render(1));
    let e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(e, f + 4096, "the original mode boundary must stay fixed");
    // Let the old implementation finish its real worker before reaching E so
    // its unnecessary replacement is observable. A retained productive owner
    // needs no new source work and must not be forced to manufacture a candidate.
    wait(|| {
        assert_eq!(fixture.transport.output_frame(), f + 1);
        fixture
            .mixer
            .voices
            .iter_mut()
            .filter(|voice| voice.active && voice.sample_id == 0)
            .all(|voice| {
                !voice.stretch.source_lane_busy() || voice.stretch.prospective_work_ready()
            })
    });
    let before = fixture.render_to(e);
    assert_eq!(before, reference.render_to(e));
    assert_eq!(before, target_reference.render_to(e));
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(
        fixture.voice(0).source_playback.position(),
        reference.voice(0).source_playback.position()
    );
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
    let waiting = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(waiting.request_id, request);
    assert_eq!(waiting.effective, old);
    assert!(!waiting.ready, "the original request must not ACK before E");
    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], old);
    let phase_at_e = fixture.voice(0).source_playback.position();
    let mut actual = fixture.render(1);
    let mut expected = reference.render(1);
    let mut target_expected = target_reference.render(1);
    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], !old);
    assert_eq!(
        fixture.voice(0).stretch.native_state_address(),
        native,
        "r=1 gives both modes the same nonzero native pitch; retain its real history"
    );
    let applied = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(applied.request_id, request);
    assert_eq!(applied.effective, !old);
    assert!(applied.ready);
    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    let phase_after_e = fixture.voice(0).source_playback.position();
    assert_eq!(phase_after_e.frame, phase_at_e.frame + 1);
    assert_eq!(
        phase_after_e.fraction.to_bits(),
        phase_at_e.fraction.to_bits()
    );
    assert_eq!(phase_after_e, reference.voice(0).source_playback.position());
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
    assert_eq!(
        actual, expected,
        "the first sample at E must retain original history"
    );
    assert_eq!(actual, target_expected);

    let mut remaining = 23_999;
    let mut partition = 0;
    let partitions = [73, 511, 7, 512, 127, 3, 1009];
    while remaining != 0 {
        let frames = remaining.min(partitions[partition % partitions.len()]);
        actual.extend(fixture.render(frames));
        expected.extend(reference.render(frames));
        target_expected.extend(target_reference.render(frames));
        remaining -= frames;
        partition += 1;
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            reference.voice(0).stretch.native_state_address(),
            reference_native
        );
        assert_eq!(
            target_reference.voice(0).stretch.native_state_address(),
            target_native
        );
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(
            fixture.voice(0).source_playback.position(),
            reference.voice(0).source_playback.position()
        );
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            reference.voice(0).stretch.pending_fifo_frames()
        );
    }
    assert_eq!(actual.len(), 24_000);
    assert!(actual.iter().all(|sample| sample.is_finite()));
    assert_eq!(
        actual, expected,
        "all continued PCM from E must equal the independently untoggled history"
    );
    assert_eq!(actual, target_expected);
    assert_tone(&actual, 0, extra, 1.0, !old);
    assert_eq!(
        fixture.mixer.pad_pitch_intent[0],
        PitchIntent::new(0, extra).unwrap()
    );
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 1.0);
    assert_eq!(fixture.transport.output_frame(), e + 24_000);
    assert_eq!(reference.transport.output_frame(), e + 24_000);
    assert_eq!(reference.mixer.pad_key_lock_enabled[0], old);

    let rate_frame = fixture.transport.output_frame();
    assert_eq!(target_reference.transport.output_frame(), rate_frame);
    same_pitch_public_rate(&mut fixture, 1.25);
    same_pitch_public_rate(&mut target_reference, 1.25);
    assert_eq!(fixture.transport.output_frame(), rate_frame);
    assert_eq!(target_reference.transport.output_frame(), rate_frame);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);

    // Include the very first irregular smoothing sample, not only a cropped
    // settled tone. Retagging the retained mode must preserve Native history
    // while making every later source-rate update use the new mode's law.
    let mut changed = Vec::new();
    let mut target_changed = Vec::new();
    let mut remaining = 48_000;
    let mut partition = 0;
    while remaining != 0 {
        let frames = remaining.min(partitions[partition % partitions.len()]);
        let pcm = fixture.render(frames);
        let target_pcm = target_reference.render(frames);
        assert_eq!(
            pcm, target_pcm,
            "every smoothing/steady sample must match target-mode birth"
        );
        changed.extend(pcm);
        target_changed.extend(target_pcm);
        remaining -= frames;
        partition += 1;
        assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
        assert_eq!(
            target_reference.voice(0).stretch.native_state_address(),
            target_native
        );
        assert_eq!(fixture.voice(0).generation, generation);
        assert_eq!(target_reference.voice(0).generation, target_generation);
        assert_eq!(fixture.starts(0), starts);
        assert_eq!(target_reference.starts(0), 1);
        assert_eq!(
            fixture.voice(0).source_playback.position(),
            target_reference.voice(0).source_playback.position()
        );
        assert_eq!(
            fixture.voice(0).stretch.pending_fifo_frames(),
            target_reference.voice(0).stretch.pending_fifo_frames()
        );
    }
    assert_eq!(changed.len(), 48_000);
    assert_eq!(changed, target_changed);
    assert_tone(&changed[24_000..], 0, extra, 1.25, !old);
    assert_eq!(fixture.voice(0).source_playback.tempo_ratio(), 1.25);
    assert_eq!(
        target_reference.voice(0).source_playback.tempo_ratio(),
        1.25
    );
    assert_eq!(fixture.mixer.pad_key_lock_enabled[0], !old);
    assert_eq!(target_reference.mixer.pad_key_lock_enabled[0], !old);
    assert_eq!(fixture.transport.output_frame(), rate_frame + 48_000);
    assert!(changed.iter().all(|sample| sample.is_finite()));
    assert_eq!(fixture.voice(0).stretch.native_origin_tuple(), Some(origin));
}

#[test]
fn public_unit_rate_mode_changes_retain_the_productive_native_and_every_continued_sample() {
    for extra in [-7, 7] {
        for old in [false, true] {
            prove_same_pitch_mode_continuation(extra, old);
        }
    }
}

#[test]
fn public_pause_resume_fences_original_continuous_mode_and_fresh_request_remains_usable() {
    let mut fixture = same_pitch_started_fixture(7, false);
    let mut reference = same_pitch_started_fixture(7, false);
    assert_eq!(fixture.pcm(733), reference.pcm(733));
    let generation = fixture.voice(0).generation;
    let native = fixture.voice(0).stretch.native_state_address();
    let starts = fixture.starts(0);
    let f = fixture.transport.output_frame();
    let original = fixture.control.engine.set_pad_key_lock(0, true).unwrap();
    fixture.drain();
    assert_eq!(fixture.render(1), reference.render(1));
    let e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(e, f + 4096);

    let frozen = fixture.voice(0).source_playback;
    let fifos = fixture.voice(0).stretch.pending_fifo_frames();
    // Both real controls happen before E at the same output frame. Their final
    // active/paused values alias the capture, but their continuity must not.
    for current in [&mut fixture, &mut reference] {
        let frame = current.transport.output_frame();
        current.control.engine.pause_sample(0).unwrap();
        current.drain();
        assert_eq!(current.transport.output_frame(), frame);
        assert!(current.voice(0).paused);
        assert!(current.voice(0).source_playback.matches_exact(&frozen));
        current.control.engine.resume_sample(0).unwrap();
        current.drain();
        assert_eq!(current.transport.output_frame(), frame);
        assert!(!current.voice(0).paused);
        assert!(current.voice(0).source_playback.matches_exact(&frozen));
        assert_eq!(current.voice(0).stretch.pending_fifo_frames(), fifos);
    }
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(fixture.render_to(e + 1), reference.render_to(e + 1));
    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
    let rejected = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(rejected.request_id, original);
    assert_eq!(rejected.state, "error");
    assert!(!rejected.ready);
    assert!(!rejected.effective);
    assert!(!fixture.mixer.pad_key_lock_enabled[0]);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(
        fixture.voice(0).source_playback.position(),
        reference.voice(0).source_playback.position()
    );
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );

    let fresh_f = fixture.transport.output_frame();
    let fresh = fixture.control.engine.set_pad_key_lock(0, true).unwrap();
    assert!(fresh > original);
    fixture.drain();
    assert_eq!(fixture.render(1), reference.render(1));
    let fresh_e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(fresh_e, fresh_f + 4096);
    assert_eq!(fixture.render_to(fresh_e), reference.render_to(fresh_e));
    let waiting = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(waiting.request_id, fresh);
    assert!(!waiting.effective);
    assert!(!waiting.ready);
    assert_eq!(fixture.render(1), reference.render(1));
    let applied = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(applied.request_id, fresh);
    assert!(applied.effective && applied.ready);
    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
    assert_eq!(fixture.pcm(8193), reference.pcm(8193));
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(
        fixture.voice(0).source_playback.position(),
        reference.voice(0).source_playback.position()
    );
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
}

#[test]
fn public_supersession_cancels_only_original_continuous_mode_and_fresh_request_remains_usable() {
    let mut fixture = same_pitch_started_fixture(7, false);
    let mut reference = same_pitch_started_fixture(7, false);
    assert_eq!(fixture.pcm(733), reference.pcm(733));
    let native = fixture.voice(0).stretch.native_state_address();
    let generation = fixture.voice(0).generation;
    let starts = fixture.starts(0);
    let f = fixture.transport.output_frame();
    let original = fixture.control.engine.set_pad_key_lock(0, true).unwrap();
    fixture.drain();
    assert_eq!(fixture.render(1), reference.render(1));
    let pending = fixture.mixer.pending_key_lock_tuples[0].as_ref().unwrap();
    let original_e = pending.execution.unwrap();
    assert_eq!(original_e, f + 4096);
    assert!(
        pending
            .voices
            .iter()
            .flatten()
            .all(|voice| voice.continuous)
    );
    assert!(!fixture.voice(0).stretch.source_lane_busy());

    let replacement = fixture.control.engine.set_pad_key_lock(0, false).unwrap();
    assert!(replacement > original);
    fixture.drain();
    assert_eq!(
        fixture.render_to(original_e + 8193),
        reference.render_to(original_e + 8193)
    );
    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
    let current = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(current.request_id, replacement);
    assert!(current.ready);
    assert!(!current.effective);
    assert!(!fixture.mixer.pad_key_lock_enabled[0]);
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(
        fixture.voice(0).source_playback.position(),
        reference.voice(0).source_playback.position()
    );
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );

    let fresh_f = fixture.transport.output_frame();
    let fresh = fixture.control.engine.set_pad_key_lock(0, true).unwrap();
    assert!(fresh > replacement);
    fixture.drain();
    assert_eq!(fixture.render(1), reference.render(1));
    let fresh_e = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(fresh_e, fresh_f + 4096);
    assert_eq!(fixture.render_to(fresh_e), reference.render_to(fresh_e));
    let waiting = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(waiting.request_id, fresh);
    assert!(!waiting.ready);
    assert!(!waiting.effective);
    assert_eq!(fixture.render(1), reference.render(1));
    let applied = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(applied.request_id, fresh);
    assert!(applied.ready && applied.effective);
    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
    assert_eq!(fixture.pcm(8193), reference.pcm(8193));
    assert_eq!(fixture.voice(0).stretch.native_state_address(), native);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(
        fixture.voice(0).source_playback.position(),
        reference.voice(0).source_playback.position()
    );
    assert_eq!(
        fixture.voice(0).stretch.pending_fifo_frames(),
        reference.voice(0).stretch.pending_fifo_frames()
    );
}
