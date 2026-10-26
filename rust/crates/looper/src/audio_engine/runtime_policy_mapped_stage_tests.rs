//! Reachable old-Mapped/new-Fixed cohort before its ordinary policy conversion.

use super::*;
use crate::audio_engine::source_reader_period::SourceReaderPeriod;
use std::sync::Mutex;

#[test]
fn static_unavailable_neighbor_phase_cannot_block_an_original_mapped_to_fixed_mode_effect() {
    let mut fixture = DryMappedPath::new(2, 17);
    fixture.path.source.engine.stop_sample(1).unwrap();
    service(&mut fixture.path);
    assert!(
        !fixture
            .path
            .source
            .mixer
            .voices
            .iter()
            .any(|voice| voice.active && voice.sample_id == 1)
    );
    fixture.adopt(true, 2);
    assert!(fixture.path.voice(0).source_playback.is_mapped());

    // The new PAD is born under the latest static request before the existing
    // mapped voice has consumed that ordinary mode change. No impossible mixed
    // desired global RT policy or manufactured source/map ownership is used.
    let (producer, parameters) = rtrb::RingBuffer::new(16);
    fixture.path.source.engine.test_parameter_producer = Some(Arc::new(Mutex::new(producer)));
    fixture.parameters = parameters;
    fixture
        .path
        .source
        .engine
        .set_pad_timing_intent(1, "manual")
        .unwrap();
    fixture
        .path
        .source
        .engine
        .set_pad_bpm(1, Some(60.0 / DYADIC_PERIOD))
        .unwrap();
    fixture.path.drain();
    drain_parameter_messages(
        &mut fixture.parameters,
        &mut fixture.path.source.mixer,
        &mut fixture.path.timeline,
    );
    let bindings = (0..NUM_SAMPLES)
        .map(|id| {
            (id < 2).then(|| {
                fixture
                    .path
                    .source
                    .engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert!(bindings[1].as_ref().unwrap().binding.accepted.is_none());
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    for (id, row) in rows.iter_mut().enumerate().take(2) {
        *row = SourceMapRow {
            sync_requested: true,
            intent_revision: fixture.intent_revision,
            delivery: (id == 0).then(|| fixture.deliveries[0].clone()),
            ordinary_pending: false,
            exact_quarters: None,
        };
    }
    let mut ends = vec![None; NUM_SAMPLES];
    ends[0] = Some(fixture.end_seconds());
    // Two-frame mismatch is outside the new <=1-frame physical class.
    ends[1] = Some(1498.0 / f64::from(RATE));
    fixture
        .path
        .source
        .engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
            vec![0.0; NUM_SAMPLES],
            ends,
            bindings.iter().map(Option::as_ref).collect(),
            Some(vec![
                PitchPadProjection {
                    sync: true,
                    key_lock: false,
                    ..Default::default()
                };
                NUM_SAMPLES
            ]),
            Some(rows),
            GlobalTimingPolicy::parse(Some(&wire(true, false, 3).to_string())).unwrap(),
        )
        .unwrap();
    let birth = fixture
        .path
        .source
        .engine
        .capture_pad_start(
            1,
            0.0,
            Some(1498.0 / f64::from(RATE)),
            1.0,
            false,
            None,
            None,
        )
        .unwrap();
    fixture.path.drain();
    assert!(
        fixture
            .path
            .source
            .engine
            .publish_pad_start(&birth, None)
            .unwrap()
    );
    fixture.path.drain();
    assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
    assert_eq!(
        fixture.path.voice(1).sync_request.mode,
        SourceSyncMode::Fixed
    );
    assert!(fixture.path.voice(0).source_playback.is_mapped());
    assert_eq!(
        fixture.path.voice(0).sync_request.mode,
        SourceSyncMode::Mapped
    );
    assert_eq!(
        fixture.path.voice(1).source_playback.reader_period(),
        SourceReaderPeriod::Physical
    );
    let raw = [
        fixture.path.voice(0).source_playback.position(),
        fixture.path.voice(1).source_playback.position(),
    ];
    let master = fixture.path.timeline;
    let snapshot = request(&fixture.path);
    service(&mut fixture.path);
    assert_eq!(
        snapshot.observation().unwrap().state,
        "applied",
        "{:?}",
        snapshot.observation()
    );
    for (id, position) in raw.iter().enumerate() {
        assert_eq!(
            fixture.path.voice(id).sync_request.mode,
            SourceSyncMode::Fixed
        );
        assert!(!fixture.path.voice(id).source_playback.is_mapped());
        assert!(!fixture.path.voice(id).source_playback.is_static_phase());
        let actual = fixture.path.voice(id).source_playback.position();
        assert_eq!(
            (actual.frame, actual.fraction.to_bits()),
            (position.frame, position.fraction.to_bits())
        );
    }
    assert!(fixture.path.timeline.matches_clock_epoch(&master));
    fixture.path.render(8192);
    let before = scalar_state(&fixture.path);
    assert!(Arc::ptr_eq(&request(&fixture.path), &snapshot));
    service(&mut fixture.path);
    assert_eq!(snapshot.observation().unwrap().state, "refused");
    assert_eq!(
        snapshot.observation().unwrap().reason,
        Some(RuntimePolicyError::Unsupported)
    );
    unchanged(&fixture.path, &before);
    assert!((0..2).all(|id| !fixture.path.voice(id).source_playback.is_static_phase()));
}
