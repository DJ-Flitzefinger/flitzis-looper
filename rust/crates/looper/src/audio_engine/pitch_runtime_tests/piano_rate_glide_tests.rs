//! K20: a real piano attack cannot replace an accepted unfinished source-rate clock.
//!
//! Real ImGui/UiContext/GlobalParams, accepted Source/timing ACKs, queues, worker
//! and the sole scheduler are retained. The parent's independent ScalarClock and
//! separately owned Raw-Native PCM reference cover the original voices through E
//! and each successful attack afterwards. The reset owner has independently
//! declared genuine rate epochs and E-anchored silent Native pre-origin input,
//! then emits its complete physical beginning at E with a fresh filter. Original
//! T and the accepted rate clock remain independent. Unavailable young history
//! still refuses the whole action. This does
//! not establish device deadlines, acoustic, finite-window, continuous-O, or
//! parent acceptance.

use super::*;
use crate::audio_engine::constants::{SPEED_MAX, SPEED_MIN};
use crate::audio_engine::source_playback::SourcePlayback;

#[path = "finite_window_rate_glide_tests.rs"]
mod finite_window_rate_glide_tests;
#[path = "lower_total_rate_glide_tests.rs"]
mod lower_total_rate_glide_tests;
#[path = "piano_glide_original_target_tests.rs"]
mod piano_glide_original_target_tests;
#[path = "selected_stem_rate_glide_tests.rs"]
mod selected_stem_rate_glide_tests;
#[path = "upper_total_rate_glide_tests.rs"]
mod upper_total_rate_glide_tests;

const GLIDE_CHILD: &str = "FLITZI_PIANO_RATE_GLIDE_CHILD";
const GLIDE_TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::piano_rate_controller_tests::piano_rate_glide_tests::piano_retrigger_preserves_unfinished_public_rate_glide";
const INITIAL_SPEED: f64 = 0.5;
const GLIDE_SPEED: f64 = 2.0;

fn public_speed(route: &RateRoute, speed: f64) {
    Python::attach(|py| {
        let probe = route.route.probe.bind(py);
        let previous_master = probe
            .getattr("session")
            .unwrap()
            .getattr("master_period_seconds")
            .unwrap()
            .extract::<f64>()
            .unwrap();
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
            previous_master
        );
    });
}

fn initial_birth(route: &mut RateRoute, id: usize, speed: f64) -> Original {
    // ON starts below C so its initial own Native history stays in the existing
    // 4096-frame range. Both original voices are productive and nonneutral.
    let note = if route.lock { -5 } else { 2 };
    initial_birth_note(route, id, speed, note)
}

fn initial_birth_note(route: &mut RateRoute, id: usize, speed: f64, note: i32) -> Original {
    let original = route.admit(id, note, false);
    initial_birth_admitted(route, id, speed, original)
}

fn initial_birth_admitted(
    route: &mut RateRoute,
    id: usize,
    speed: f64,
    original: Original,
) -> Original {
    initial_birth_admitted_with_reference(route, id, speed, original, Reference::birth_captured)
}

type BirthReference = fn(&SampleBuffer, usize, PitchIntent, f64, bool, usize, u64) -> Reference;

fn initial_birth_admitted_with_reference(
    route: &mut RateRoute,
    id: usize,
    speed: f64,
    original: Original,
    birth_reference: BirthReference,
) -> Original {
    let master_anchor_frame = original.e;
    initial_birth_admitted_with_reference_at_anchor(
        route,
        id,
        speed,
        original,
        birth_reference,
        master_anchor_frame,
    )
}

fn initial_birth_admitted_with_reference_at_anchor(
    route: &mut RateRoute,
    id: usize,
    speed: f64,
    mut original: Original,
    birth_reference: impl FnOnce(&SampleBuffer, usize, PitchIntent, f64, bool, usize, u64) -> Reference,
    master_anchor_frame: u64,
) -> Original {
    assert_eq!(original.id, id);
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
    let first_bootstrap = route.master.is_none();
    if first_bootstrap {
        assert_eq!(id, 0);
        assert_eq!(route.route.transport.bootstrap_reference(), Some(id));
        assert_eq!(master_anchor_frame, original.e);
        route.master = Some(MasterClock {
            frame: master_anchor_frame,
            beat: (0.1 - route.origin) / PERIODS[0],
            period,
        });
    }
    // The parent settled-rate helper has period .25 assumptions. Initial births
    // use their actual accepted public period, without changing that helper.
    route.references[id] = Some(birth_reference(
        &route.reference_sources[id],
        id,
        PitchIntent::new(original.note, original.extra).unwrap(),
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
    if first_bootstrap {
        assert_eq!(route.route.transport.bootstrap_reference(), None);
    }
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
    ensure_positive_pre_t_history(&mut route, &originals[0], speed, &[0, 2, 7, 12]);
    (route, originals)
}

fn ensure_positive_pre_t_history(
    route: &mut RateRoute,
    birth: &Original,
    initial_speed: f64,
    totals: &[i32],
) {
    let required = totals
        .iter()
        .map(|total| {
            RawRubberBand::history_bound(scalar_pitch(*total, route.lock, initial_speed))
                .div_ceil(512)
                * 512
        })
        .max()
        .expect("a positive fixture declares its cold profiles");
    let age = route.route.transport.output_frame() - birth.e;
    if age < required as u64 {
        route.callback((required as u64 - age) as usize);
        route.assert_no_attacks();
    }
    assert!(route.route.transport.output_frame() - birth.e >= required as u64);
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
        assert!(clock.advance(span.0));
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

fn scalar_pitch(note: i32, lock: bool, rate: f64) -> f64 {
    let h = 2.0_f64.powf(f64::from(note) / 12.0);
    if lock { h / rate } else { h }
}

/// Derive every original epoch from its public birth and the one accepted rate
/// event. Neither the live clock, source journal, captured plan nor worker result
/// supplies a reference phase/rate. The final entry is the frozen old E clock.
fn original_timeline(
    birth: &Original,
    initial_speed: f64,
    rate_f: u64,
    e: u64,
) -> Vec<ScalarClock> {
    assert!(birth.t <= birth.e && birth.e <= rate_f && rate_f < e);
    let mut clock = ScalarClock::new(0, initial_speed.clamp(SPEED_MIN, SPEED_MAX));
    // Physical source phase starts at the actual initial birth E. Its requested
    // T remains the independent master anchor, not an unheard source prefix.
    let mut timeline = Vec::with_capacity((e - birth.e) as usize + 1);
    for frame in birth.e..e {
        if frame == rate_f {
            clock.target(GLIDE_SPEED);
        }
        clock.before_sample();
        timeline.push(clock.clone());
        clock.advance();
    }
    timeline.push(clock);
    timeline
}

/// H expands over actual included p values until no earlier epoch increases it.
/// An admission probe may end at T; actual fresh Native prehistory ends at E.
/// Each reset lattice is independent of the old callback birth E. A caller may
/// not borrow history beyond its declared endpoint.
fn reset_bound(timeline: &[ScalarClock], note: i32, lock: bool) -> Option<(usize, usize, usize)> {
    let end = timeline.len() - 1;
    let pitch = scalar_pitch(note, lock, timeline[end].rate);
    let mut queried = std::collections::BTreeMap::new();
    let mut bound = RawRubberBand::history_bound(pitch);
    queried.insert(pitch.to_bits(), bound);
    loop {
        let origin = end.checked_sub(bound.div_ceil(512) * 512)?;
        let mut required = bound;
        for clock in &timeline[origin..end] {
            let pitch = scalar_pitch(note, lock, clock.rate);
            let frames = *queried
                .entry(pitch.to_bits())
                .or_insert_with(|| RawRubberBand::history_bound(pitch));
            required = required.max(frames);
        }
        if required == bound {
            return Some((origin, bound, queried.len()));
        }
        bound = required;
    }
}

struct ResetOracle {
    reference: Reference,
    old_endpoint: ScalarClock,
    active_origin: usize,
    active_end: usize,
    bound: usize,
    endpoint_bound: usize,
    queried_pitches: usize,
    cold_pitch: f64,
    endpoint_pitch: f64,
}

impl ResetOracle {
    fn new(
        _source: &SampleBuffer,
        timeline: &[ScalarClock],
        contract: PitchIntent,
        lock: bool,
        native: usize,
        generation: u64,
    ) -> Self {
        let note = contract.base_shift() + contract.extra_shift();
        let end = timeline.len() - 1;
        let (origin, bound, queried_pitches) =
            reset_bound(timeline, note, lock).expect("independent real lifecycle must cover H");
        assert_eq!(end - origin, bound.div_ceil(512) * 512);
        let old_endpoint = timeline[end].clone();
        let endpoint_pitch = scalar_pitch(note, lock, old_endpoint.rate);
        let endpoint_bound = RawRubberBand::history_bound(endpoint_pitch);
        // A mixed-rate first block's final sample selects its cold Native profile.
        let cold_pitch = scalar_pitch(note, lock, timeline[origin + 511].rate);
        let mut raw = RawRubberBand::new(cold_pitch);
        // Preserve every independently derived historical pitch/rate epoch, but
        // supply zero PCM before this fresh source's physical birth at E.
        for epoch in &timeline[origin..end] {
            let pitch = scalar_pitch(note, lock, epoch.rate);
            raw.sample(0.0, pitch);
        }
        assert_eq!(raw.fifo(), (0, 511));
        let mut reset_endpoint = old_endpoint.clone();
        reset_endpoint.origin = LOOP_START as f64;
        reset_endpoint.elapsed = 0;
        assert_eq!(reset_endpoint.rate, old_endpoint.rate);
        assert_eq!(reset_endpoint.target, old_endpoint.target);
        assert_eq!(reset_endpoint.until_step, old_endpoint.until_step);
        Self {
            reference: Reference {
                clock: reset_endpoint,
                raw: Some(raw),
                // A fresh physical birth starts its filter independently of prefeed.
                filter: PerPadDspChain::new(0, RATE as f32, 1),
                note,
                contract,
                lock,
                native,
                generation,
            },
            old_endpoint,
            active_origin: origin,
            active_end: end,
            bound,
            endpoint_bound,
            queried_pitches,
            cold_pitch,
            endpoint_pitch,
        }
    }
}

fn assert_ready_cache(route: &RateRoute, cached: Option<usize>) {
    if let Some(cached) = cached {
        let runtime = route.route.mixer.pitch_runtime.as_ref().unwrap();
        assert!(runtime.selected_endpoint_ready());
        assert_eq!(runtime.selected_endpoint_native_address(), cached);
        assert_ne!(route.voice(0).stretch.native_state_address(), cached);
    }
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
            "first_sample":route.last_callback_first,
            "fifo":route.voice(0).stretch.pending_fifo_frames(),
            "pitch_action_work_inline_bytes":std::mem::size_of::<crate::audio_engine::pitch_actions::PitchActionWork>(),
            "endpoint_reset_history_inline_bytes":std::mem::size_of::<crate::audio_engine::prepared_source_feed::EndpointResetHistory>(),
            "endpoint_reset_history_option_inline_bytes":std::mem::size_of::<Option<crate::audio_engine::prepared_source_feed::EndpointResetHistory>>(),
            "scope":"real public rate and piano; independent birth-derived scalar epochs/Raw-Native reset PCM; whole unavailable-history refusal or own wet reset owner; no device/acoustic/finite-window/continuous-O/parent acceptance"
        })
    );
}

fn apply_wet_attack(
    route: &mut RateRoute,
    originals: &[Original; 2],
    accepted: &RateReceipt,
    initial_speed: f64,
    note: i32,
    input: (&str, bool),
    cached: Option<usize>,
) {
    let (case, midi) = input;
    apply_wet_attack_from(
        route,
        originals,
        accepted,
        initial_speed,
        WetAttack {
            case,
            pitch: PitchIntent::new(note, 0).unwrap(),
            cached,
        },
        |route| route.admit(0, note, midi),
    );
}

struct WetAttack<'a> {
    case: &'a str,
    pitch: PitchIntent,
    cached: Option<usize>,
}

fn apply_wet_attack_from(
    route: &mut RateRoute,
    originals: &[Original; 2],
    accepted: &RateReceipt,
    initial_speed: f64,
    input: WetAttack<'_>,
    admit: impl FnOnce(&mut RateRoute) -> Original,
) {
    let WetAttack {
        case,
        pitch,
        cached,
    } = input;
    let note = pitch.base_shift() + pitch.extra_shift();
    let old = VoiceWitness::capture(route, 0);
    let peer = VoiceWitness::capture(route, 1);
    let starts = route.starts;
    let frame = route.route.transport.output_frame();
    let timeline = original_timeline(
        &originals[0],
        initial_speed,
        accepted.execution.unwrap(),
        frame + 8192,
    );
    let t_index = usize::try_from(frame - originals[0].e).unwrap();
    let target_timeline = &timeline[..=t_index];
    let endpoint_pitch_t = scalar_pitch(note, route.lock, target_timeline.last().unwrap().rate);
    let (_, required_t_history, _) = reset_bound(target_timeline, note, route.lock)
        .expect("positive fixture must earn genuine pre-T callback history");
    assert!(frame - originals[0].e >= (required_t_history.div_ceil(512) * 512) as u64);
    let mut attack = admit(route);
    assert_eq!(
        (attack.note, attack.extra),
        (pitch.base_shift(), pitch.extra_shift())
    );
    assert_eq!(attack.t, frame);
    assert_eq!(attack.e, attack.t + 8192);
    route.assert_no_attacks();
    assert!(!attack.action.effective());
    route.wait_action(&mut attack);
    old.unchanged(route, 0);
    peer.unchanged(route, 1);
    assert_eq!(route.starts, starts);
    assert_ready_cache(route, cached);
    let native = attack.prepared_native.unwrap();
    assert_ne!(native, old.native);
    assert_ne!(native, peer.native);
    assert_ne!(Some(native), cached);
    let expected_e_clock = timeline.last().unwrap().clone();
    let endpoint_pitch_e = scalar_pitch(note, route.lock, expected_e_clock.rate);
    let endpoint_bound_e = RawRubberBand::history_bound(endpoint_pitch_e);
    let oracle = ResetOracle::new(
        &route.reference_sources[0],
        &timeline,
        pitch,
        route.lock,
        native,
        old.generation + 1,
    );
    let trajectory_profiles = timeline[oracle.active_origin..timeline.len() - 1]
        .iter()
        .map(|clock| scalar_pitch(note, route.lock, clock.rate).to_bits())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    // Genuine rate epochs drive this separate Native through silent pre-origin
    // input. Physical source and fresh EQ begin at E; no T-to-E output is consumed.
    assert_eq!(oracle.endpoint_pitch, endpoint_pitch_e);
    assert_eq!(oracle.endpoint_bound, endpoint_bound_e);
    if case == "wet-on-plus12-cold-above2" {
        assert!(oracle.cold_pitch > 2.0);
        assert!(endpoint_pitch_e <= 2.0);
        assert!(oracle.bound > endpoint_bound_e);
        assert!(trajectory_profiles > 2);
        assert_ne!(
            oracle.active_origin % 512,
            0,
            "new E prehistory lattice differs from the old callback birth lattice"
        );
    }
    if case == "wet-on-momentary-unity" {
        assert_eq!(old.playback.tempo_ratio(), 1.0);
        assert_eq!(endpoint_pitch_t, 1.0);
        assert_ne!(endpoint_pitch_e, 1.0);
    }
    // The unchanged old source and independently running peer remain audible
    // through the entire original F-to-E window before the zero-frame claim.
    route.render_to(attack.e);
    assert!(!attack.action.effective());
    assert_eq!(route.starts, starts);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    assert_eq!(route.voice(0).generation, old.generation);
    assert_ready_cache(route, cached);
    let expected = &expected_e_clock;
    assert_rate_clock(route, &attack, expected);
    let live_phase = route.references[0].as_ref().unwrap().clock.phase();
    assert!((live_phase - expected.phase()).abs() < 2e-7);
    let peer_at_e = VoiceWitness::capture(route, 1);
    route.callback(0);
    assert!(attack.action.effective(), "{:?}", attack.action.reason());
    assert_eq!(attack.action.state(), "applied");
    assert_rate_clock(route, &attack, expected);
    let voice = route.voice(0);
    let expected_phase = oracle.reference.clock.phase();
    let position = voice.source_playback.position();
    assert!((position.frame as f64 + position.fraction - expected_phase).abs() < 2e-7);
    assert_eq!(voice.generation, old.generation + 1);
    assert_eq!(voice.stretch.native_state_address(), native);
    assert_eq!(voice.stretch.adopted_request_id(), Some(attack.sequence));
    assert_eq!(
        voice.stretch.native_origin_tuple(),
        Some((pitch, route.lock, attack.sequence))
    );
    assert_eq!(
        voice.stretch.native_primed_output_frames(),
        oracle.active_end - oracle.active_origin
    );
    assert_eq!(
        voice.stretch.pending_fifo_frames(),
        oracle.reference.raw.as_ref().unwrap().fifo()
    );
    let fifo_at_e = voice.stretch.pending_fifo_frames();
    peer_at_e.unchanged(route, 1);
    assert_ready_cache(route, cached);
    assert_eq!(route.route.mixer.pad_pitch_intent(0), Some(pitch));
    assert_eq!(
        route.take_attacks(),
        vec![(0, attack.content.cell().index(), attack.content.lifetime())]
    );
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
    let history_evidence = serde_json::json!({
        "case":case,"base":pitch.base_shift(),"extra":pitch.extra_shift(),"total":note,
        "original_birth_T":originals[0].t,"original_birth_E":originals[0].e,"original_T":attack.t,"original_E":attack.e,
        "A_active":oracle.active_origin,"T_active":t_index,"E_active":oracle.active_end,"H_T":required_t_history,"H_E":oracle.bound,
        "endpoint_H":endpoint_bound_e,"cold_p":oracle.cold_pitch,"T_p":endpoint_pitch_t,"endpoint_p":endpoint_pitch_e,
        "first_prefeed_p":scalar_pitch(note, route.lock, timeline[oracle.active_origin].rate),
        "old_birth_lattice_residue":oracle.active_origin%512,"E_lattice_residue":oracle.active_end%512,
        "queried_E_pitches":oracle.queried_pitches,"trajectory_pitches":trajectory_profiles,"prefeed_samples":oracle.active_end-oracle.active_origin,
        "old_T_phase":target_timeline.last().unwrap().phase(),"old_E_phase":oracle.old_endpoint.phase(),"new_E_phase":expected_phase,"E_rate":expected.rate,"E_target":expected.target,
        "E_until_step":expected.until_step,"cache_native":cached,"new_native":native,
        "fifo_at_E":fifo_at_e,
        "scope":"independent accepted rate epochs and E prehistory fixed point, complete physical beginning and fresh EQ at E, pinned direct C-ABI owner; no live plan/journal/PCM reference input"
    });
    route.references[0] = Some(oracle.reference);
    route.settle(&attack, false);
    route.check_voices();
    // Include E's very first sample before irregular chunks, step boundaries,
    // and enough later output to settle both actual public rate trajectories.
    route.callback(1);
    route.assert_no_attacks();
    assert_eq!(route.last_callback_first.unwrap().0, attack.e);
    let first = route.last_callback_first.unwrap();
    eprintln!("piano_rate_glide_reset_history {}", history_evidence);
    eprintln!(
        "piano_rate_glide_first_sample {}",
        serde_json::json!({
            "case":case,"frame":first.0,"actual":first.1,"independent":first.2,
            "error":(first.1-first.2).abs(),"fifo":route.voice(0).stretch.pending_fifo_frames()
        })
    );
    emit_case(route, case, &attack, accepted);
    for frames in CHUNKS.into_iter().chain([HISTORY + 37]) {
        route.callback(frames);
        route.assert_no_attacks();
    }
    let after_settling =
        (accepted.execution.unwrap() + 16_073).max(route.route.transport.output_frame() + 1024);
    route.render_to(after_settling);
    assert_eq!(route.voice(0).source_playback.tempo_ratio(), 2.0);
    assert_eq!(route.voice(1).source_playback.tempo_ratio(), 1.5);
    assert_rate_clock(route, &attack, &route.references[0].as_ref().unwrap().clock);
    route.wait_claims();
    assert_eq!(route.starts, [starts[0] + 1, starts[1]]);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_rate_receipt(route, accepted);
    for original in originals.iter().chain([&attack]) {
        original.unchanged();
    }
    emit_case(route, &format!("{case}-after-settling"), &attack, accepted);
    assert!(route.scheduler.is_empty());
}

fn wet_attack(lock: bool, initial_speed: f64, note: i32, case: &str) {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals) = start_route(lock, initial_speed);
    let accepted = begin_glide(&mut route);
    route.callback(73);
    route.assert_no_attacks();
    apply_wet_attack(
        &mut route,
        &originals,
        &accepted,
        initial_speed,
        note,
        (case, false),
        None,
    );
    route.route.close();
}

fn unavailable_young_history() {
    let _mouse = MOUSE_CONTEXT.lock().unwrap();
    let (mut route, originals) = start_route(true, INITIAL_SPEED);
    // A real public attack starts a genuinely young lifecycle. Its
    // callback journal has only this lifecycle's actual samples, with no injected
    // gap or invented past. The peer continues on its original owner throughout.
    let young = initial_birth_note(&mut route, 0, INITIAL_SPEED, -4);
    let accepted = begin_glide(&mut route);
    let rate_f = accepted.execution.unwrap();
    assert_eq!(rate_f, young.e + 1);
    route.callback(73);
    route.assert_no_attacks();
    let old = VoiceWitness::capture(&route, 0);
    let peer = VoiceWitness::capture(&route, 1);
    let refused = route.admit(0, 12, false);
    assert_eq!(refused.t, rate_f + 73);
    assert_eq!(refused.e, refused.t + 8192);
    let timeline = original_timeline(&young, INITIAL_SPEED, rate_f, refused.e);
    let t_index = usize::try_from(refused.t - young.e).unwrap();
    assert!(reset_bound(&timeline[..=t_index], 12, true).is_none());
    let starts = route.starts;
    wait_refusal(&mut route, &refused);
    old.unchanged(&route, 0);
    peer.unchanged(&route, 1);
    assert_eq!(route.starts, starts);
    assert_eq!(
        route.route.mixer.pad_pitch_intent(0),
        Some(PitchIntent::new(young.note, 0).unwrap())
    );
    route.render_to(refused.e);
    let at_e = VoiceWitness::capture(&route, 0);
    let peer_at_e = VoiceWitness::capture(&route, 1);
    let expected = timeline.last().unwrap();
    route.callback(0);
    at_e.unchanged(&route, 0);
    peer_at_e.unchanged(&route, 1);
    assert_rate_clock(&route, &refused, expected);
    route.check_voices();
    route.assert_no_attacks();
    settle_refusal(&route, &refused);
    route.render_to(rate_f + 16_073);
    route.wait_claims();
    assert_eq!(route.starts, starts);
    assert_eq!(route.voice(0).generation, old.generation);
    assert_eq!(route.voice(0).stretch.native_state_address(), old.native);
    assert_eq!(route.voice(1).generation, peer.generation);
    assert_eq!(route.voice(1).stretch.native_state_address(), peer.native);
    assert_rate_receipt(&route, &accepted);
    assert_eq!(refused.action.state(), "rejected");
    assert_eq!(refused.action.reason(), Some("preparation_failed"));
    assert!(!refused.action.effective());
    for original in originals.iter().chain([&young, &refused]) {
        original.unchanged();
    }
    eprintln!(
        "piano_rate_glide_unavailable_history {}",
        serde_json::json!({
            "young_birth":young.e,"rate_F":rate_f,"original_T":refused.t,"original_E":refused.e,
            "available_lifecycle_frames":timeline.len()-1,"independent_H_fixed_point_available":false,
            "starts":route.starts,"unchanged_native":old.native,"unchanged_generation":old.generation
        })
    );
    emit_case(
        &route,
        "wet-on-plus12-genuine-young-history-refusal",
        &refused,
        &accepted,
    );
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
    apply_wet_attack(
        &mut route,
        &originals,
        &accepted,
        INITIAL_SPEED,
        7,
        ("wet-off-original-E-after-settling", false),
        None,
    );
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

    // Same note and mode isolate the clock mismatch. The actual READY nominal
    // r2 cache remains unconsumed while this click earns its own reset owner.
    apply_wet_attack(
        &mut route,
        &originals,
        &accepted,
        INITIAL_SPEED,
        2,
        ("wet-off-ready-cache-same-note", false),
        Some(cached),
    );
    route.route.close();
}

#[test]
fn piano_retrigger_preserves_unfinished_public_rate_glide() {
    if let Ok(child) = std::env::var(GLIDE_CHILD) {
        match child.as_str() {
            "wet-off" => wet_attack(false, INITIAL_SPEED, 7, "wet-off-unfinished"),
            "wet-on" => wet_attack(true, INITIAL_SPEED, 7, "wet-on-unfinished"),
            "wet-on-unity" => wet_attack(true, 0.95, 0, "wet-on-momentary-unity"),
            "dry-off" => dry_attack(),
            "wet-off-settles" => wet_after_settling(),
            "wet-off-cache-repeat" => ready_cache_same_note(),
            "wet-on-plus12-cold-above2" => {
                wet_attack(true, INITIAL_SPEED, 12, "wet-on-plus12-cold-above2")
            }
            "wet-on-young-history-refusal" => unavailable_young_history(),
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
        "wet-on-plus12-cold-above2",
        "wet-on-young-history-refusal",
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
