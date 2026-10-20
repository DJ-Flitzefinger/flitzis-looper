//! Immutable requested T with an independent full birth at the accepted E rate clock.
use super::*;
use crate::audio_engine::dsp::{DspNodeSlot, DspParameterId, DspParameterSlot};

fn nonneutral(filter: &mut PerPadDspChain) {
    for (slot, db) in [
        (DspParameterSlot::Slot0, -12.0_f32),
        (DspParameterSlot::Slot1, -3.0_f32),
        (DspParameterSlot::Slot2, 4.0_f32),
    ] {
        let normalized = if db <= 0.0 {
            0.5 * 10.0_f32.powf(db / 20.0)
        } else {
            0.5 + 0.5 * db / 6.0
        };
        assert!(filter.set_parameter(
            DspParameterId::per_pad(0, DspNodeSlot::Slot0, slot).unwrap(),
            normalized
        ));
    }
}

fn start_route_at_original_t(lock: bool) -> (RateRoute, [Original; 2]) {
    let mut route = RateRoute::new(lock);
    public_speed(&route, INITIAL_SPEED);
    route.callback(0);
    route.assert_no_attacks();
    let initial = route.rate();
    assert_eq!(initial.state, "applied");
    assert_eq!(initial.effective_speed, INITIAL_SPEED);
    assert_eq!(initial.effective_period, Some(PERIODS[0] / INITIAL_SPEED));
    route.observe_rate();
    let originals = std::array::from_fn(|id| {
        let note = if lock { -5 } else { 2 };
        let original = route.admit(id, note, false);
        assert_eq!(original.e.checked_sub(original.t), Some(8192));
        let master_anchor_frame = original.e;
        initial_birth_admitted_with_reference_at_anchor(
            &mut route,
            id,
            INITIAL_SPEED,
            original,
            Reference::birth_captured,
            master_anchor_frame,
        )
    });
    route.wait_claims();
    route.callback(733);
    route.assert_no_attacks();
    (route, originals)
}

fn original_timeline_from_birth(
    birth: &Original,
    initial_speed: f64,
    rate_frame: u64,
    target: u64,
) -> Vec<ScalarClock> {
    assert!(birth.t <= birth.e && birth.e <= rate_frame && rate_frame < target);
    let mut clock = ScalarClock::new(0, initial_speed.clamp(SPEED_MIN, SPEED_MAX));
    // Physical source phase starts at the first emitted birth E. The accepted
    // rate event keeps its absolute frame and its complete smoothing interval.
    let mut timeline = Vec::with_capacity((target - birth.e) as usize + 1);
    for frame in birth.e..target {
        if frame == rate_frame {
            clock.target(GLIDE_SPEED);
        }
        clock.before_sample();
        timeline.push(clock.clone());
        clock.advance();
    }
    timeline.push(clock);
    timeline
}

fn original_target_glide(lock: bool) {
    let _mouse = MOUSE_CONTEXT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut route, originals) = start_route_at_original_t(lock);
    route.route.mixer.set_pad_eq(0, -12.0, -3.0, 4.0);
    nonneutral(&mut route.references[0].as_mut().unwrap().filter);
    route.callback(733);
    route.assert_no_attacks();
    let initial_history =
        RawRubberBand::history_bound(scalar_pitch(7, lock, INITIAL_SPEED)).div_ceil(512) * 512;
    let old_callback_age = route.route.transport.output_frame() - originals[0].e;
    if old_callback_age < initial_history as u64 {
        // Only genuine old-source callbacks establish the pre-T journal. The
        // future T-to-E interval cannot provide this cold Native's warm history.
        route.callback((initial_history as u64 - old_callback_age) as usize);
        route.assert_no_attacks();
    }
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    let target = route.route.transport.output_frame();
    let timeline = original_timeline_from_birth(
        &originals[0],
        INITIAL_SPEED,
        accepted.execution.unwrap(),
        target,
    );
    let callback_age = target - originals[0].e;
    let (_, target_history, _) = reset_bound(&timeline, 7, lock).unwrap_or_else(|| {
        panic!("positive original-T fixture lacks history: key_lock={lock} initial_H={initial_history} callback_age={callback_age} T={target}")
    });
    assert!(callback_age >= (target_history.div_ceil(512) * 512) as u64);
    eprintln!(
        "original_t_glide_history key_lock={lock} initial_H={initial_history} exact_T_H={target_history} actual_callback_age={callback_age} T={target}"
    );
    let old = VoiceWitness::capture(&route, 0);
    let peer = VoiceWitness::capture(&route, 1);
    let mut attack = route.admit(0, 7, false);
    assert_eq!(attack.t, target);
    assert_eq!(attack.e, attack.t + 8192);
    route.wait_action(&mut attack);
    old.unchanged(&route, 0);
    peer.unchanged(&route, 1);
    let native = attack.prepared_native.unwrap();
    assert_ne!(native, old.native);
    assert_ne!(native, peer.native);

    // Derive E's accepted rate/target/step clock from the original public birth
    // and accepted rate event. No DUT source phase or worker checkpoint enters
    // the reference. Native prehistory retains its existing real rate journal.
    let execution_timeline = original_timeline_from_birth(
        &originals[0],
        INITIAL_SPEED,
        accepted.execution.unwrap(),
        target + 8192,
    );
    assert_eq!(attack.e, target + 8192);
    let reset = ResetOracle::new(
        &route.reference_sources[0],
        &execution_timeline,
        PitchIntent::new(7, 0).unwrap(),
        lock,
        native,
        old.generation + 1,
    );
    let primed = reset.active_end - reset.active_origin;
    let mut oracle = reset.reference;
    nonneutral(&mut oracle.filter);
    oracle.filter.reset();
    let expected_phase = oracle.clock.phase();
    assert_eq!(expected_phase, LOOP_START as f64);
    route.render_to(attack.e);
    route.assert_no_attacks();
    let peer_at_e = VoiceWitness::capture(&route, 1);
    assert_rate_clock(&route, &attack, execution_timeline.last().unwrap());
    route.callback(0);
    assert!(attack.action.effective(), "{:?}", attack.action.reason());
    assert_eq!(route.voice(0).stretch.native_state_address(), native);
    assert_eq!(
        route.voice(0).stretch.adopted_request_id(),
        Some(attack.sequence)
    );
    peer_at_e.unchanged(&route, 1);
    assert_rate_clock(&route, &attack, execution_timeline.last().unwrap());
    assert_eq!(route.voice(0).stretch.native_primed_output_frames(), primed);
    assert_eq!(
        route.voice(0).stretch.pending_fifo_frames(),
        oracle.raw.as_ref().unwrap().fifo()
    );
    let phase = route.voice(0).source_playback.position();
    assert!(
        (phase.frame as f64 + phase.fraction - expected_phase).abs() < 2e-7,
        "full birth at E lost source beginning: expected {expected_phase}, actual {phase:?}"
    );
    route.references[0] = Some(oracle);
    assert_eq!(
        route.take_attacks(),
        vec![(0, attack.content.cell().index(), attack.content.lifetime())]
    );
    // RateRoute compares every emitted sample against the independent Native/EQ,
    // including E's first sample; no reference PCM beginning is cropped.
    for frames in [1, 127, 509, 31, 1024, 17, 4096, 733] {
        route.callback(frames);
        route.assert_no_attacks();
    }
    assert_rate_receipt(&route, &accepted);
    for original in originals.iter().chain([&attack]) {
        original.unchanged();
    }
    route.route.close();
}

#[test]
fn delayed_glide_lock_off_keeps_original_t_and_matches_independent_e_birth_native_and_eq() {
    original_target_glide(false);
}

#[test]
fn delayed_glide_lock_on_keeps_original_t_and_matches_independent_e_birth_native_and_eq() {
    original_target_glide(true);
}
