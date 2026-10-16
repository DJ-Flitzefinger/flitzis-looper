// Included in key_lock_tuple_tests.rs. Two genuine complete-source pads share
// only immutable PCM; requests, Native owners and mode checkpoints stay separate.

fn two_complete_pitch_pads() -> Fixture {
    use crate::audio_engine::ordinary_endpoint_reserve_tests::{
        adopt_current_sources, refresh_actual_runtime_bindings,
    };

    let mut fixture = complete_fixture(false);
    let sample = fixture.control.engine.sample_cache.lock().unwrap()[0]
        .clone()
        .unwrap();
    publish_tone(&fixture.control, 1, sample.clone(), true);
    fixture.mixer.load_sample(1, sample);
    // Both complete sources precede their actual LoadSample claims and SourceACKs.
    adopt_current_sources(&mut fixture, false);
    refresh_actual_runtime_bindings(&fixture);
    for (id, extra) in [(0, 7), (1, -7)] {
        fixture
            .mixer
            .set_pad_pitch_intent(id, PitchIntent::new(0, extra).unwrap())
            .unwrap();
        let origin = fixture
            .control
            .engine
            .capture_pad_start(
                id,
                LOOP_START as f64 / f64::from(RATE),
                Some(LOOP_END as f64 / f64::from(RATE)),
                1.0,
                false,
                None,
                None,
            )
            .unwrap();
        assert!(
            fixture
                .control
                .engine
                .publish_pad_start(&origin, None)
                .unwrap()
        );
        fixture.drain();
        wait(|| {
            let runtime = fixture.mixer.pitch_runtime.as_mut().unwrap();
            runtime.poll();
            runtime.has_ready_action(origin.sequence())
        });
        fixture.render_to(origin.execution_frame().unwrap());
        fixture.render(1);
        assert!(origin.effective_frame().is_some(), "{:?}", origin.reason());
        fixture.pcm(8192);
        assert_eq!(fixture.starts(id), 1);
    }
    fixture
}

fn assert_two_pad_history(
    fixture: &Fixture,
    reference: &Fixture,
    natives: [usize; 2],
    generations: [u64; 2],
) {
    assert_eq!(
        fixture.transport.output_frame(),
        reference.transport.output_frame()
    );
    for id in [0, 1] {
        assert_eq!(
            fixture.voice(id).stretch.native_state_address(),
            natives[id]
        );
        assert_eq!(fixture.voice(id).generation, generations[id]);
        assert_eq!(fixture.starts(id), 1);
        assert_eq!(reference.starts(id), 1);
        assert_eq!(
            fixture.voice(id).source_playback.position(),
            reference.voice(id).source_playback.position()
        );
        assert_eq!(
            fixture.voice(id).stretch.pending_fifo_frames(),
            reference.voice(id).stretch.pending_fifo_frames()
        );
    }
}

#[test]
fn public_two_pad_continuous_requests_keep_independent_tags_through_one_supersession() {
    let mut fixture = two_complete_pitch_pads();
    let mut reference = two_complete_pitch_pads();
    assert_eq!(fixture.pcm(733), reference.pcm(733));
    let natives = [0, 1].map(|id| fixture.voice(id).stretch.native_state_address());
    let generations = [0, 1].map(|id| fixture.voice(id).generation);
    let indices = [0, 1].map(|id| {
        fixture
            .mixer
            .voices
            .iter()
            .position(|voice| voice.active && voice.sample_id == id)
            .unwrap()
    });
    assert_ne!(natives[0], 0);
    assert_ne!(natives[1], 0);
    assert_ne!(natives[0], natives[1]);
    assert!(
        fixture
            .mixer
            .continuous_key_lock_voices
            .iter()
            .all(Option::is_none)
    );
    let f = fixture.transport.output_frame();

    let first = fixture.control.engine.set_pad_key_lock(0, true).unwrap();
    fixture.drain();
    assert_eq!(fixture.render(1), reference.render(1));
    let e0 = fixture.mixer.pending_key_lock_tuples[0]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(e0, f + 4096);
    let second = fixture.control.engine.set_pad_key_lock(1, true).unwrap();
    fixture.drain();
    assert_eq!(fixture.render(1), reference.render(1));
    let e1 = fixture.mixer.pending_key_lock_tuples[1]
        .as_ref()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(e1, f + 1 + 4096);
    for (id, request) in [(0, first), (1, second)] {
        let pending = fixture.mixer.pending_key_lock_tuples[id].as_ref().unwrap();
        assert_eq!(pending.request.request_id, request);
        assert!(pending.voices[indices[id]].unwrap().continuous);
        let checkpoint = fixture.mixer.continuous_key_lock_voices[indices[id]].unwrap();
        assert_eq!((checkpoint.pad_id, checkpoint.request_id), (id, request));
        assert_eq!(checkpoint.native, natives[id]);
    }
    assert_eq!(
        fixture
            .mixer
            .continuous_key_lock_voices
            .iter()
            .flatten()
            .count(),
        2
    );
    assert_two_pad_history(&fixture, &reference, natives, generations);

    let latest = fixture.control.engine.set_pad_key_lock(0, false).unwrap();
    assert_ne!(latest, first);
    fixture.drain();
    assert_eq!(fixture.render(1), reference.render(1));
    assert!(fixture.mixer.pending_key_lock_tuples[0].is_none());
    assert!(fixture.mixer.continuous_key_lock_voices[indices[0]].is_none());
    let remaining = fixture.mixer.continuous_key_lock_voices[indices[1]].unwrap();
    assert_eq!((remaining.pad_id, remaining.request_id), (1, second));
    assert_eq!(remaining.native, natives[1]);
    assert_eq!(
        fixture
            .mixer
            .continuous_key_lock_voices
            .iter()
            .flatten()
            .count(),
        1,
        "finishing pad 0 must not clear pad 1's original checkpoint"
    );
    assert_two_pad_history(&fixture, &reference, natives, generations);

    assert_eq!(fixture.render_to(e0), reference.render_to(e0));
    let waiting = fixture.mixer.pad_key_lock_status[1].unwrap();
    assert_eq!(waiting.request_id, second);
    assert!(!waiting.effective && !waiting.ready);
    assert_eq!(fixture.render(1), reference.render(1));
    assert_eq!(fixture.transport.output_frame(), e1);
    let waiting = fixture.mixer.pad_key_lock_status[1].unwrap();
    assert_eq!(waiting.request_id, second);
    assert!(
        !waiting.effective && !waiting.ready,
        "pad 1 must wait for its own E1"
    );
    assert!(fixture.mixer.continuous_key_lock_voices[indices[1]].is_some());
    let latest_status = fixture.mixer.pad_key_lock_status[0].unwrap();
    assert_eq!(latest_status.request_id, latest);
    assert!(!latest_status.effective);
    assert!(!fixture.mixer.pad_key_lock_enabled[0]);

    let mut actual = fixture.render(1);
    let mut expected = reference.render(1);
    assert_eq!(actual, expected);
    let applied = fixture.mixer.pad_key_lock_status[1].unwrap();
    assert_eq!(applied.request_id, second);
    assert!(applied.effective && applied.ready);
    assert!(fixture.mixer.pad_key_lock_enabled[1]);
    assert!(fixture.mixer.pending_key_lock_tuples[1].is_none());
    assert!(
        fixture
            .mixer
            .continuous_key_lock_voices
            .iter()
            .all(Option::is_none)
    );
    assert_two_pad_history(&fixture, &reference, natives, generations);

    let mut remaining = 8192;
    let mut partition = 0;
    let partitions = [73, 511, 7, 512, 127, 3, 1009];
    while remaining != 0 {
        let frames = remaining.min(partitions[partition % partitions.len()]);
        let pcm = fixture.render(frames);
        let baseline = reference.render(frames);
        assert_eq!(
            pcm, baseline,
            "both independently owned voices must retain the exact sum"
        );
        actual.extend(pcm);
        expected.extend(baseline);
        remaining -= frames;
        partition += 1;
        assert_two_pad_history(&fixture, &reference, natives, generations);
    }
    assert_eq!(actual.len(), 8193);
    assert_eq!(actual, expected);
    assert!(actual.iter().all(|sample| sample.is_finite()));
    assert_eq!(fixture.transport.output_frame(), e1 + 8193);
    assert_eq!(
        fixture.mixer.pad_key_lock_status[0].unwrap().request_id,
        latest
    );
    assert!(!fixture.mixer.pad_key_lock_enabled[0]);
    assert!(fixture.mixer.pad_key_lock_enabled[1]);
    assert!(!reference.mixer.pad_key_lock_enabled[0]);
    assert!(!reference.mixer.pad_key_lock_enabled[1]);
    assert!(
        fixture.mixer.pending_key_lock_tuples[..2]
            .iter()
            .all(Option::is_none)
    );
    assert!(
        fixture
            .mixer
            .continuous_key_lock_voices
            .iter()
            .all(Option::is_none)
    );
}
