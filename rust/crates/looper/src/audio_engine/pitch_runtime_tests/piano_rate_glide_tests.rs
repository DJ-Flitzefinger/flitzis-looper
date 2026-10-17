//! K20: a real piano attack cannot replace an accepted unfinished source-rate clock.
//!
//! Real ImGui/UiContext/GlobalParams, accepted Source/timing ACKs, queues, worker
//! and the sole scheduler are retained. The parent's independent ScalarClock and
//! separately owned Raw-Native PCM reference cover the original voices through E
//! and each successful attack afterwards. Unprovable wet endpoint history still
//! refuses the whole original action; this does not establish wet-glide support,
//! device deadlines, acoustic, finite-window, continuous-O, or parent acceptance.

use super::*;
use crate::audio_engine::constants::{SPEED_MAX, SPEED_MIN};
use crate::audio_engine::source_playback::SourcePlayback;

const GLIDE_CHILD: &str = "FLITZI_PIANO_RATE_GLIDE_CHILD";
const GLIDE_TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::piano_rate_controller_tests::piano_rate_glide_tests::piano_retrigger_preserves_unfinished_public_rate_glide";
const INITIAL_SPEED: f64 = 0.5;
const GLIDE_SPEED: f64 = 2.0;

fn public_speed(route: &RateRoute, speed: f64) {
    Python::attach(|py| {
        let probe = route.route.probe.bind(py);
        probe
            .getattr("ctx")
            .unwrap()
            .getattr("audio")
            .unwrap()
            .getattr("global_")
            .unwrap()
            .call_method1("set_speed", (speed,))
            .unwrap();
        assert_eq!(
            probe
                .getattr("project")
                .unwrap()
                .getattr("speed")
                .unwrap()
                .extract::<f64>()
                .unwrap(),
            speed
        );
        assert_eq!(
            probe
                .getattr("session")
                .unwrap()
                .getattr("master_period_seconds")
                .unwrap()
                .extract::<f64>()
                .unwrap(),
            PERIODS[0] / speed
        );
    });
}

fn initial_birth(route: &mut RateRoute, id: usize, speed: f64) -> Original {
    // ON starts below C so its initial own Native history stays in the existing
    // 4096-frame range. Both original voices are productive and nonneutral.
    let note = if route.lock { -5 } else { 2 };
    let mut original = route.admit(id, note, false);
    route.assert_no_attacks();
    route.wait_action(&mut original);
    route.render_to(original.e);
    route.callback(0);
    assert!(
        original.action.effective(),
        "{:?}",
        original.action.reason()
    );
    let period = PERIODS[0] / speed;
    // In particular B's nominal .375 at initial speed .5 has actual rate .5.
    let rate = (PERIODS[id] / period).clamp(SPEED_MIN, SPEED_MAX);
    let voice = route.voice(id);
    assert_eq!(voice.source_playback.tempo_ratio(), rate);
    assert_eq!(voice.source_playback.rate_target(), rate);
    assert_eq!(
        voice.stretch.native_state_address(),
        original.prepared_native.unwrap()
    );
    let native = voice.stretch.native_state_address();
    let generation = voice.generation;
    if route.master.is_none() {
        assert_eq!(id, 0);
        route.master = Some(MasterClock {
            frame: original.e,
            beat: (0.1 - route.origin) / PERIODS[0],
            period,
        });
    }
    // The parent settled-rate helper has period .25 assumptions. Initial births
    // use their actual accepted public period, without changing that helper.
    route.references[id] = Some(Reference::birth(
        &route.sources[id],
        id,
        note,
        rate,
        route.lock,
        native,
        generation,
    ));
    assert_eq!(
        route.take_attacks(),
        vec![(
            id,
            original.content.cell().index(),
            original.content.lifetime()
        )]
    );
    original.unchanged();
    route.callback(1);
    route.assert_no_attacks();
    route.settle(&original, false);
    original
}

fn start_route(lock: bool, speed: f64) -> (RateRoute, [Original; 2]) {
    let mut route = RateRoute::new(lock);
    public_speed(&route, speed);
    route.callback(0);
    route.assert_no_attacks();
    let initial = route.rate();
    assert_eq!(initial.state, "applied");
    assert_eq!(initial.effective_speed, speed);
    assert_eq!(initial.effective_period, Some(PERIODS[0] / speed));
    route.observe_rate();
    let originals = [
        initial_birth(&mut route, 0, speed),
        initial_birth(&mut route, 1, speed),
    ];
    route.wait_claims();
    route.callback(733);
    route.assert_no_attacks();
    (route, originals)
}

fn begin_glide(route: &mut RateRoute) -> RateReceipt {
    let frame = route.route.transport.output_frame();
    public_speed(route, GLIDE_SPEED);
    route.callback(0);
    route.assert_no_attacks();
    let accepted = route.rate();
    assert_eq!(accepted.state, "applied");
    assert_eq!(accepted.request, accepted.effect);
    assert_eq!(accepted.execution, Some(frame));
    assert_eq!(accepted.effective_speed, GLIDE_SPEED);
    assert_eq!(accepted.effective_period, Some(0.25));
    assert!(accepted.error.is_none());
    route.master.as_mut().unwrap().change(frame, 0.25);
    for (id, reference) in route.references.iter_mut().enumerate() {
        reference
            .as_mut()
            .unwrap()
            .clock
            .target((PERIODS[id] / 0.25).clamp(SPEED_MIN, SPEED_MAX));
    }
    route.observe_rate();
    accepted
}

fn assert_rate_receipt(route: &RateRoute, accepted: &RateReceipt) {
    let actual = route.rate();
    assert_eq!(actual.state, "applied");
    assert_eq!(actual.request, accepted.request);
    assert_eq!(actual.effect, accepted.effect);
    assert_eq!(actual.execution, accepted.execution);
    assert_eq!(actual.effective_speed, accepted.effective_speed);
    assert_eq!(actual.effective_period, accepted.effective_period);
    assert!(actual.error.is_none());
    assert_eq!(route.route.transport.master_period_seconds(), Some(0.25));
}

struct VoiceWitness {
    playback: SourcePlayback,
    native: usize,
    fifo: (usize, usize),
    generation: u64,
}

impl VoiceWitness {
    fn capture(route: &RateRoute, id: usize) -> Self {
        let voice = route.voice(id);
        Self {
            playback: voice.source_playback,
            native: voice.stretch.native_state_address(),
            fifo: voice.stretch.pending_fifo_frames(),
            generation: voice.generation,
        }
    }

    fn unchanged(&self, route: &RateRoute, id: usize) {
        let voice = route.voice(id);
        assert!(voice.source_playback.matches_exact(&self.playback));
        assert_eq!(voice.stretch.native_state_address(), self.native);
        assert_eq!(voice.stretch.pending_fifo_frames(), self.fifo);
        assert_eq!(voice.generation, self.generation);
    }
}

fn actual_rate_steps(mut clock: SourcePlayback) -> [(usize, f64); 2] {
    // Probe only a Copy of the actual clock; never advance a live voice.
    std::array::from_fn(|_| {
        let span = clock.chunk(1024);
        clock.advance(span.0);
        span
    })
}

fn scalar_rate_steps(mut clock: ScalarClock) -> [(usize, f64); 2] {
    std::array::from_fn(|_| {
        clock.before_sample();
        let frames = if clock.until_step == 0 {
            1024
        } else {
            clock.until_step.min(1024)
        };
        let rate = clock.rate;
        for _ in 0..frames {
            clock.advance();
        }
        (frames, rate)
    })
}

fn assert_rate_clock(route: &RateRoute, original: &Original, expected: &ScalarClock) {
    let actual = route.voice(0).source_playback;
    assert!(
        (actual.tempo_ratio() - expected.rate).abs() < 1e-14,
        "K20 original E={} changed actual rate: expected {} target {} until {}, got {:?}",
        original.e,
        expected.rate,
        expected.target,
        expected.until_step,
        actual
    );
    assert_eq!(actual.rate_target(), expected.target);
    assert_eq!(
        actual_rate_steps(actual),
        scalar_rate_steps(expected.clone()),
        "K20 original active-frame step chronology at E={}",
        original.e
    );
}

fn wait_refusal(route: &mut RateRoute, original: &Original) {
    let frame = route.route.transport.output_frame();
    let deadline = Instant::now() + WAIT;
    while original.action.state() != "rejected" {
        assert!(!original.action.effective());
        assert!(Instant::now() < deadline, "host refusal exceeded 15 s");
        route.callback(0);
        route.assert_no_attacks();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(route.route.transport.output_frame(), frame);
    assert_eq!(original.action.reason(), Some("preparation_failed"));
    assert!(!original.action.effective());
    assert!(
        !route
            .route
            .mixer
            .pitch_runtime
            .as_ref()
            .unwrap()
            .has_ready_action(original.sequence)
    );
    original.unchanged();
}

fn settle_refusal(route: &RateRoute, original: &Original) {
    Python::attach(|py| {
        let probe = route.route.probe.bind(py);
        let pitch = probe
            .getattr("transport")
            .unwrap()
            .getattr("pitch")
            .unwrap();
        pitch.call_method0("poll").unwrap();
        assert!(
            !pitch
                .getattr("_owners")
                .unwrap()
                .call_method1("__contains__", (original.sequence,))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert_eq!(
            pitch
                .call_method1("status", (0,))
                .unwrap()
                .extract::<(String, Option<String>)>()
                .unwrap(),
            (
                "error".to_owned(),
                Some("Pitch preparation failed; try again.".to_owned())
            )
        );
        assert!(
            original
                .py
                .getattr(py, "effective_output_frame")
                .unwrap()
                .extract::<Option<u64>>(py)
                .unwrap()
                .is_none()
        );
        assert!(
            probe
                .getattr("session")
                .unwrap()
                .getattr("input_mapping_error")
                .unwrap()
                .is_none()
        );
    });
}

fn emit_case(route: &RateRoute, case: &str, original: &Original, rate: &RateReceipt) {
    let clock = &route.references[0].as_ref().unwrap().clock;
    eprintln!(
        "piano_rate_glide {}",
        serde_json::json!({
            "case":case,"key_lock":route.lock,"rate_request":rate.request,
            "rate_E":rate.execution,"original_sequence":original.sequence,
            "original_T":original.t,"original_E":original.e,
            "action_state":original.action.state(),"action_reason":original.action.reason(),
            "action_effective":original.action.effective(),
            "actual_rate":route.voice(0).source_playback.tempo_ratio(),
            "target":route.voice(0).source_playback.rate_target(),
            "scalar_until_step":clock.until_step,"actual_rate_steps":actual_rate_steps(route.voice(0).source_playback),
            "native":route.voice(0).stretch.native_state_address(),
            "raw_pcm_max_error":route.maximum,"compared_frames":route.compared,"starts":route.starts,
            "scope":"real public rate and piano; independent source-clock/Raw-Native PCM; whole wet refusal or phase-only successful clock projection; no pending-glide wet-history/device/acoustic/parent acceptance"
        })
    );
}

fn wet_refusal(lock: bool, initial_speed: f64, note: i32, case: &str) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals) = start_route(lock, initial_speed);
    let accepted = begin_glide(&mut route);
    let rate_f = accepted.execution.unwrap();
    route.callback(73);
    route.assert_no_attacks();
    if note == 0 {
        assert!(lock);
        assert_eq!(route.voice(0).source_playback.tempo_ratio(), 1.0);
        assert_eq!(route.voice(0).source_playback.rate_target(), 2.0);
        assert_eq!(
            PitchIntent::default()
                .native_pitch_scale(1.0, true)
                .unwrap(),
            1.0
        );
    }
    let old = VoiceWitness::capture(&route, 0);
    let peer = VoiceWitness::capture(&route, 1);
    let refused = route.admit(0, note, false);
    assert_eq!(refused.t, rate_f + 73);
    assert_eq!(refused.e, refused.t + 8192);
    route.assert_no_attacks();
    wait_refusal(&mut route, &refused);
    old.unchanged(&route, 0);
    peer.unchanged(&route, 1);
    assert_eq!(route.starts, [1, 1]);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(originals[0].note, 0).unwrap())
    );
    route.render_to(refused.e);
    let at_e = VoiceWitness::capture(&route, 0);
    let peer_at_e = VoiceWitness::capture(&route, 1);
    let expected = route.references[0].as_ref().unwrap().clock.clone();
    assert!(expected.rate < expected.target);
    assert!(expected.until_step > 0 && expected.until_step < 512);
    route.callback(0);
    at_e.unchanged(&route, 0);
    peer_at_e.unchanged(&route, 1);
    assert_rate_clock(&route, &refused, &expected);
    route.check_voices();
    route.assert_no_attacks();
    assert_eq!(route.voice(0).generation, old.generation);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_rate_receipt(&route, &accepted);
    emit_case(&route, case, &refused, &accepted);
    settle_refusal(&route, &refused);
    route.wait_claims();

    // No late retry/retarget rescues the old receipt. A new same-note real click
    // after settling earns its own T/E, Native owner and reset-phase PCM history.
    route.render_to(rate_f + 16_073);
    assert_eq!(route.voice(0).source_playback.tempo_ratio(), 2.0);
    assert_eq!(route.voice(1).source_playback.tempo_ratio(), 1.5);
    let fresh = route.start(0, note, false);
    assert!(fresh.sequence > refused.sequence);
    assert_eq!(fresh.e, fresh.t + 8192);
    assert_ne!(fresh.prepared_native.unwrap(), old.native);
    assert_eq!(route.voice(0).generation, old.generation + 1);
    for frames in CHUNKS.into_iter().chain([HISTORY + 37]) {
        route.callback(frames);
        route.assert_no_attacks();
    }
    route.wait_claims();
    assert_eq!(route.starts, [2, 1]);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_rate_receipt(&route, &accepted);
    assert_eq!(refused.action.state(), "rejected");
    assert_eq!(refused.action.reason(), Some("preparation_failed"));
    assert!(!refused.action.effective());
    for original in originals.iter().chain([&refused, &fresh]) {
        original.unchanged();
    }
    emit_case(&route, &format!("{case}-fresh-settled"), &fresh, &accepted);
    assert!(route.scheduler.is_empty());
    route.route.close();
}

fn dry_attack() {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals) = start_route(false, INITIAL_SPEED);
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    let old = VoiceWitness::capture(&route, 0);
    let peer = VoiceWitness::capture(&route, 1);
    let mut expected = route.references[0].as_ref().unwrap().clock.clone();
    assert!(expected.rate < expected.target);
    assert_eq!(expected.until_step, 439);
    let attack = route.admit(0, 0, false);
    assert_eq!(attack.t, accepted.execution.unwrap() + 73);
    assert_eq!(attack.e, attack.t, "OFF h1 is a real scalar/dry attack");
    assert!(attack.action.effective(), "{:?}", attack.action.reason());
    assert_rate_clock(&route, &attack, &expected);
    assert_eq!(route.voice(0).source_playback.position().frame, LOOP_START);
    assert_eq!(route.voice(0).source_playback.position().fraction, 0.0);
    assert_eq!(route.voice(0).generation, old.generation + 1);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    peer.unchanged(&route, 1);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::default())
    );
    // Independent dry reference resets only source phase and the ordinary pad
    // filter; its current rate, target and remaining active-frame interval stay.
    expected.origin = LOOP_START as f64;
    expected.elapsed = 0;
    let mut reference = Reference::birth(
        &route.sources[0],
        0,
        0,
        expected.rate,
        false,
        old.native,
        old.generation + 1,
    );
    assert!(reference.raw.is_none());
    reference.clock = expected;
    route.references[0] = Some(reference);
    assert_eq!(
        route.take_attacks(),
        vec![(0, attack.content.cell().index(), attack.content.lifetime())]
    );
    route.settle(&attack, false);
    Python::attach(|py| {
        assert_eq!(
            attack
                .py
                .getattr(py, "effective_output_frame")
                .unwrap()
                .extract::<u64>(py)
                .unwrap(),
            attack.e
        );
    });
    route.check_voices();
    emit_case(&route, "dry-off-unfinished", &attack, &accepted);
    route.render_to(accepted.execution.unwrap() + 16_073);
    route.wait_claims();
    assert_eq!(route.starts, [2, 1]);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_rate_receipt(&route, &accepted);
    for original in originals.iter().chain([&attack]) {
        original.unchanged();
    }
    emit_case(&route, "dry-off-settled", &attack, &accepted);
    assert!(route.scheduler.is_empty());
    route.route.close();
}

fn wet_after_settling() {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals) = start_route(false, INITIAL_SPEED);
    let accepted = begin_glide(&mut route);
    let rate_f = accepted.execution.unwrap();
    route.render_to(rate_f + 9001);
    assert!(route.voice(0).source_playback.tempo_ratio() < 2.0);
    let old = VoiceWitness::capture(&route, 0);
    let mut attack = route.admit(0, 7, false);
    assert_eq!(attack.t, rate_f + 9001);
    assert_eq!(attack.e, attack.t + 8192);
    route.assert_no_attacks();
    route.wait_action(&mut attack);
    assert_ne!(attack.prepared_native.unwrap(), old.native);
    route.render_to(attack.e);
    assert!(!attack.action.effective());
    let expected = route.references[0].as_ref().unwrap().clock.clone();
    assert_eq!(expected.rate, 2.0);
    assert_eq!(expected.target, 2.0);
    assert_eq!(expected.until_step, 0);
    let peer = VoiceWitness::capture(&route, 1);
    route.callback(0);
    assert_rate_clock(&route, &attack, &expected);
    assert!(attack.action.effective(), "{:?}", attack.action.reason());
    assert_eq!(route.voice(0).source_playback.position().frame, LOOP_START);
    assert_eq!(route.voice(0).source_playback.position().fraction, 0.0);
    assert_eq!(route.voice(0).generation, old.generation + 1);
    assert_eq!(
        route.voice(0).stretch.native_state_address(),
        attack.prepared_native.unwrap()
    );
    peer.unchanged(&route, 1);
    route.install_birth_reference(&attack);
    route.settle(&attack, false);
    route.check_voices();
    for frames in CHUNKS.into_iter().chain([HISTORY + 37]) {
        route.callback(frames);
        route.assert_no_attacks();
    }
    route.wait_claims();
    assert_eq!(route.starts, [2, 1]);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_rate_receipt(&route, &accepted);
    for original in originals.iter().chain([&attack]) {
        original.unchanged();
    }
    emit_case(
        &route,
        "wet-off-original-E-after-settling",
        &attack,
        &accepted,
    );
    assert!(route.scheduler.is_empty());
    route.route.close();
}

fn ready_cache_same_note() {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals) = start_route(false, INITIAL_SPEED);
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    let frame = route.route.transport.output_frame();
    let old = VoiceWitness::capture(&route, 0);
    let peer = VoiceWitness::capture(&route, 1);
    assert_eq!(old.playback.tempo_ratio(), 0.55);
    assert_eq!(old.playback.rate_target(), 2.0);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(2, 0).unwrap())
    );
    // Keep the real Python projection and public Native selection on A. No
    // action, artificial cache arming, output advance or mixer setter is used.
    Python::attach(|py| {
        let probe = route.route.probe.bind(py);
        let revision = {
            let engine = route.route.engine.borrow(py);
            let revision = engine.select_input_pad(0).unwrap();
            assert_eq!(
                engine.input_selected_projection().unwrap(),
                (revision, Some(0))
            );
            revision
        };
        let receipt = pyo3::types::PyDict::new(py);
        receipt
            .set_item("native_selection_already_applied", true)
            .unwrap();
        receipt
            .set_item("expected_native_selection_revision", revision)
            .unwrap();
        assert!(
            probe
                .getattr("input_mapping")
                .unwrap()
                .call_method("_execute_select_pad", ("ui.select_pad:0",), Some(&receipt))
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        assert_eq!(
            probe
                .getattr("project")
                .unwrap()
                .getattr("selected_pad")
                .unwrap()
                .extract::<usize>()
                .unwrap(),
            0
        );
        assert!(
            probe
                .getattr("input_mapping")
                .unwrap()
                .call_method0("refresh_pitch_runtime_state")
                .unwrap()
                .extract::<bool>()
                .unwrap()
        );
        probe.call_method0("frame").unwrap();
        assert_eq!(
            route
                .route
                .engine
                .borrow(py)
                .input_selected_projection()
                .unwrap(),
            (revision, Some(0))
        );
    });
    let deadline = Instant::now() + WAIT;
    let cached = loop {
        route.callback(0);
        route.assert_no_attacks();
        old.unchanged(&route, 0);
        peer.unchanged(&route, 1);
        assert_eq!(route.route.transport.output_frame(), frame);
        let runtime = route.route.mixer.pitch_runtime.as_ref().unwrap();
        if runtime.selected_endpoint_ready() {
            break runtime.selected_endpoint_native_address();
        }
        assert!(
            Instant::now() < deadline,
            "selected endpoint host wait exceeded 15 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_ne!(cached, 0);
    assert_ne!(cached, old.native);
    assert_ne!(cached, peer.native);
    let selected = route
        .route
        .mixer
        .capture_selected_normal_endpoint()
        .unwrap();
    assert_eq!(selected.id, 0);
    assert_eq!(selected.render.pitch, PitchIntent::new(2, 0).unwrap());
    assert!(!selected.render.key_lock);
    assert_eq!(selected.snapshot.playback.tempo_ratio(), 2.0);
    assert_eq!(selected.snapshot.playback.rate_target(), 2.0);
    let binding = Python::attach(|py| {
        route
            .route
            .engine
            .borrow(py)
            .current_input_runtime_pad_binding(0)
            .unwrap()
            .unwrap()
            .binding
    });
    assert_eq!(
        selected.snapshot.sample.source_address(),
        binding.source_address
    );
    assert_eq!(
        selected.snapshot.sample.source_sample_count(),
        binding.sample_count
    );
    assert_eq!(selected.snapshot.sample.channels, binding.channels);
    assert_eq!(selected.snapshot.accepted, binding.accepted);
    drop(selected);
    eprintln!(
        "piano_rate_glide_cache_READY {}",
        serde_json::json!({
            "frame":frame,"note":2,"key_lock":false,"cached_native":cached,
            "cached_rate":2.0,"live_rate":old.playback.tempo_ratio(),
            "live_target":old.playback.rate_target(),"live_native":old.native,
            "peer_native":peer.native,"starts":route.starts,
            "scope":"actual selected endpoint ready at bank target while both actual source clocks retain the unfinished public glide; zero output-time host wait"
        })
    );

    // Same note and mode isolate the rate-clock mismatch: a nominal target-r2
    // endpoint must not turn this real click into a cache hit at current r.55.
    let refused = route.admit(0, 2, false);
    assert_eq!(refused.t, frame);
    assert_eq!(refused.e, frame + 8192, "mismatching ready cache must miss");
    route.assert_no_attacks();
    wait_refusal(&mut route, &refused);
    old.unchanged(&route, 0);
    peer.unchanged(&route, 1);
    assert_eq!(route.route.transport.output_frame(), frame);
    let runtime = route.route.mixer.pitch_runtime.as_ref().unwrap();
    assert!(runtime.selected_endpoint_ready());
    assert_eq!(runtime.selected_endpoint_native_address(), cached);
    assert_eq!(route.starts, [1, 1]);
    route.render_to(refused.e);
    let at_e = VoiceWitness::capture(&route, 0);
    let peer_at_e = VoiceWitness::capture(&route, 1);
    let expected = route.references[0].as_ref().unwrap().clock.clone();
    assert!(expected.rate < expected.target);
    assert_eq!(expected.until_step, 439);
    route.callback(0);
    at_e.unchanged(&route, 0);
    peer_at_e.unchanged(&route, 1);
    assert_rate_clock(&route, &refused, &expected);
    route.check_voices();
    route.assert_no_attacks();
    assert_eq!(route.starts, [1, 1]);
    assert_eq!(route.voice(0).generation, old.generation);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(2, 0).unwrap())
    );
    let runtime = route.route.mixer.pitch_runtime.as_ref().unwrap();
    assert!(runtime.selected_endpoint_ready());
    assert_eq!(runtime.selected_endpoint_native_address(), cached);
    assert_rate_receipt(&route, &accepted);
    settle_refusal(&route, &refused);
    route.wait_claims();
    for original in originals.iter().chain([&refused]) {
        original.unchanged();
    }
    emit_case(&route, "wet-off-ready-cache-same-note", &refused, &accepted);
    assert!(route.scheduler.is_empty());
    route.route.close();
}

#[test]
fn piano_retrigger_preserves_unfinished_public_rate_glide() {
    if let Ok(child) = std::env::var(GLIDE_CHILD) {
        match child.as_str() {
            "wet-off" => wet_refusal(false, INITIAL_SPEED, 7, "wet-off-unfinished"),
            "wet-on" => wet_refusal(true, INITIAL_SPEED, 7, "wet-on-unfinished"),
            "wet-on-unity" => wet_refusal(true, 0.95, 0, "wet-on-momentary-unity"),
            "dry-off" => dry_attack(),
            "wet-off-settles" => wet_after_settling(),
            "wet-off-cache-repeat" => ready_cache_same_note(),
            _ => panic!("unknown declared piano rate-glide child"),
        }
        return;
    }
    for child in [
        "wet-off",
        "wet-on",
        "wet-on-unity",
        "dry-off",
        "wet-off-settles",
        "wet-off-cache-repeat",
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", GLIDE_TEST, "--nocapture", "--test-threads=1"])
            .env(GLIDE_CHILD, child)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "isolated piano rate-glide {child} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains("piano_rate_glide"));
        print!("{stdout}");
        eprint!("{stderr}");
    }
}
