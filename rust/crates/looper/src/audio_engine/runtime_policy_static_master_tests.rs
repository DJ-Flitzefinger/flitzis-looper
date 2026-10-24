//! Original accepted-refresh effects with an independently active static source.

use super::*;
use crate::audio_engine::constant_timing::l3_constant_timing_fixture;
use crate::audio_engine::{accepted_timing_refresh, input_runtime_binding};

fn publish_reference(fixture: &mut StaticPath, requested_period: f64) -> f64 {
    fixture
        .path
        .source
        .engine
        .set_pad_timing_intent(1, "automatic")
        .unwrap();
    let ticket = l3_constant_timing_fixture::publish_synthetic_loaded_timing(
        &fixture.path.source.engine,
        &fixture.path.source.producer,
        1,
        requested_period,
    );
    fixture.drain();
    assert_eq!(ticket.publication_status().unwrap(), "accepted");
    let binding = input_runtime_binding::capture(&fixture.path.source.engine, 1)
        .unwrap()
        .unwrap();
    binding.binding.accepted.unwrap().period_seconds
}

fn current_reference_refresh(
    fixture: &StaticPath,
    period: f64,
) -> accepted_timing_refresh::AcceptedTimingRefreshTicket {
    let binding = input_runtime_binding::capture(&fixture.path.source.engine, 1)
        .unwrap()
        .unwrap();
    accepted_timing_refresh::enqueue(
        &fixture.path.source.engine,
        &fixture.path.source.producer,
        &binding,
        0.0,
        Some(4096.0 / f64::from(RATE)),
        Some(period),
    )
    .unwrap()
}

fn static_with_nonbinary_master() -> (StaticPath, f64) {
    let mut fixture = StaticPath::new(&[1500]);
    fixture
        .path
        .source
        .load_and_ack_selected_with_frames(1, None, STATIC_SOURCE_FRAMES);
    fixture.path.source.mixer.set_pad_key_lock(1, false);
    let period = publish_reference(&mut fixture, 24_004.0 / f64::from(RATE));
    fixture
        .path
        .source
        .engine
        .set_pad_bpm(0, Some(120.0))
        .unwrap();
    fixture.drain();
    fixture.path.source.mixer.set_master_period(period);
    assert!(
        fixture
            .path
            .timeline
            .set_master_period_and_anchor_beat_position_at_frame(
                period,
                1234.567_890_123,
                (1_u64 << 40) + 997,
            )
    );
    fixture.path.timeline.complete_bootstrap();
    // The discontinuous anchor invalidates the earlier launch generation.
    // Start through original custody again after that actual master change.
    fixture.path.source.engine.stop_sample(0).unwrap();
    fixture.drain();
    fixture.publish(false, 1, 7, None);
    let birth = fixture.start(0);
    assert_eq!(birth.state(), "applied", "{:?}", birth.reason());
    fixture.publish(true, 2, 7, None);
    let first = fixture.effect();
    assert_eq!(first.observation().unwrap().state, "applied");
    fixture.path.render(8192);
    let second = fixture.effect();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(
        second.observation().unwrap().state,
        "applied",
        "{:?}",
        second.observation()
    );
    assert!(fixture.path.voice(0).source_playback.is_static_phase());
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .static_matches_projection(fixture.path.timeline.beat_projection().unwrap(),)
    );
    (fixture, period)
}

#[test]
fn static_other_pad_equal_period_accepted_refresh_keeps_the_exact_actual_master_evaluator() {
    let (mut fixture, period) = static_with_nonbinary_master();
    let frame = fixture.path.timeline.output_frame();
    let claim = frame + 1;
    let projection = fixture.path.timeline.beat_projection().unwrap();
    let before = fixture.path.voice(0).source_playback;
    let mut counterfactual = fixture.path.timeline;
    assert!(counterfactual.set_master_period_preserving_beat_position_at_frame(period, claim));
    assert!(!projection.matches_exact(counterfactual.beat_projection().unwrap()));
    assert!(
        (0..65_536_u64).any(|offset| {
            projection.beat_position_at_frame(claim + offset).to_bits()
                != counterfactual
                    .beat_position_at_frame(claim + offset)
                    .unwrap()
                    .to_bits()
        }),
        "fixture must distinguish the original f64 evaluator from a same-tempo reanchor"
    );
    let refresh = current_reference_refresh(&fixture, period);
    fixture.drain();
    // The original accepted-refresh contract deliberately schedules E+1 after
    // both bounded drains. Render the real preceding frame, then claim at E+1.
    fixture.path.render(1);
    fixture.path.render(0);
    assert_eq!(refresh.publication_status(), "accepted");
    assert!(refresh.is_current());
    assert!(projection.matches_exact(fixture.path.timeline.beat_projection().unwrap()));
    let mut at_claim = before;
    assert!(at_claim.advance(1));
    assert_eq!(fixture.path.timeline.output_frame(), claim);
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .matches_exact(&at_claim)
    );
    assert!(
        fixture
            .path
            .voice(0)
            .source_playback
            .static_matches_projection(projection)
    );
    let expected = at_claim.position_at(127).unwrap();
    fixture.path.render(127);
    assert_eq!(fixture.path.voice(0).source_playback.position(), expected);
}

#[test]
fn static_successful_original_accepted_master_change_dissolves_then_requalifies_live_phase() {
    let (mut fixture, old_period) = static_with_nonbinary_master();
    fixture.path.render(17);
    let before = fixture.path.voice(0).source_playback;
    let frame = fixture.path.timeline.output_frame();
    let claim = frame + 1;
    let beat = fixture.path.timeline.beat_position_at_frame(claim).unwrap();
    let period = publish_reference(&mut fixture, 20_000.0 / f64::from(RATE));
    assert_ne!(period.to_bits(), old_period.to_bits());
    assert!(
        fixture.path.voice(0).source_playback.matches_exact(&before),
        "an unrelated source's timing publication cannot mutate this static program"
    );
    let refresh = current_reference_refresh(&fixture, period);
    fixture.drain();
    fixture.path.render(1);
    fixture.path.render(0);
    assert_eq!(refresh.publication_status(), "accepted");
    assert!(refresh.is_current());
    assert_eq!(fixture.path.timeline.output_frame(), claim);
    assert_eq!(
        fixture
            .path
            .timeline
            .beat_position_at_frame(claim)
            .unwrap()
            .to_bits(),
        beat.to_bits()
    );
    assert_eq!(fixture.path.timeline.master_period_seconds(), Some(period));
    assert!(!fixture.path.voice(0).source_playback.is_static_phase());
    assert_eq!(
        fixture.path.voice(0).source_playback.position(),
        before.position_at(1).unwrap()
    );
    fixture.publish(true, 2, 7, None);
    let pending = fixture.effect();
    assert_eq!(pending.observation().unwrap().state, "refused");
    assert_eq!(
        pending.observation().unwrap().reason,
        Some(RuntimePolicyError::Busy)
    );
    fixture.path.render(8192);
    let raw = fixture.path.voice(0).source_playback.position();
    let live = fixture.path.timeline.output_frame();
    let applied = fixture.effect();
    assert!(Arc::ptr_eq(&pending, &applied));
    assert_eq!(
        applied.observation().unwrap().state,
        "applied",
        "{:?}",
        applied.observation()
    );
    let playback = fixture.path.voice(0).source_playback;
    assert!(playback.is_static_phase());
    assert_eq!(playback.static_first_frame(), Some(live));
    assert_eq!(playback.position(), raw);
    assert!(playback.static_matches_projection(fixture.path.timeline.beat_projection().unwrap()));
}
