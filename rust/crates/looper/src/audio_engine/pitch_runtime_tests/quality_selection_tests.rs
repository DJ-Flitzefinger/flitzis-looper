//! Predeclared public SET diagnostic: exact provenance, settled pitch, uncropped attacks.
//! No fitted delay/gain/crop and no acoustic acceptance inferred from A-reset equality.

use super::*;
use crate::audio_engine::audio_quality_metrics::{
    MAX_CAPTURE_TAIL_ENERGY, MAX_DISCARDED_ENERGY, TIMING_BUDGET_MS, energy_times, stereo_energy,
    stereo_peak,
};

const CAPTURE_START: usize = E - 24_000;
const CAPTURE_END: usize = E + 48_000;
const TONE_START: usize = E + 24_000;
const MARKER_BASE: usize = 138_163;

#[derive(Clone, Copy, Debug)]
enum Attack {
    Impulse,
    Tone,
    Percussion,
}

fn analytic_source(attack: Option<(Attack, usize)>) -> SampleBuffer {
    let samples = (0..EXTENT)
        .flat_map(|frame| {
            let value = attack.map(|(signal, offset)| {
                let first = MARKER_BASE + offset;
                if frame < first || frame >= first + 384 {
                    return 0.0;
                }
                let n = frame - first;
                let progress = n as f64 / 384.0;
                let time = n as f64 / f64::from(RATE);
                match signal {
                    Attack::Impulse => {
                        if n == 0 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    Attack::Tone => {
                        ((std::f64::consts::PI * progress).sin().powi(2)
                            * (std::f64::consts::TAU * 900.0 * time + 0.37).sin())
                            as f32
                    }
                    Attack::Percussion => {
                        // Same deterministic component(0,0) as the original isolated probe.
                        let mut noise = (n as u32).wrapping_mul(747_796_405).wrapping_add(131);
                        noise = ((noise >> ((noise >> 28) + 4)) ^ noise).wrapping_mul(277_803_737);
                        noise = (noise >> 22) ^ noise;
                        let noise = (noise as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32 * 0.075;
                        let envelope = (-7.0 * progress).exp();
                        (envelope * (std::f64::consts::TAU * 180.0 * time + 0.7).sin()) as f32 * 0.7
                            + noise * envelope as f32 * 2.0
                    }
                }
            });
            std::array::from_fn::<_, 2, _>(|channel| match value {
                Some(value) => value * if channel == 0 { 1.0 } else { 0.5 },
                None => {
                    (0.25
                        * (std::f64::consts::TAU * [160.0, 211.0][channel] * frame as f64
                            / f64::from(RATE))
                        .sin()) as f32
                }
            })
        })
        .collect::<Vec<_>>();
    SampleBuffer {
        samples: samples.into(),
        channels: 2,
        residency: None,
    }
    .with_complete_source(RATE)
}

fn planar(interleaved: &[f32]) -> [Vec<f32>; 2] {
    assert_eq!(interleaved.len() % 2, 0);
    std::array::from_fn(|channel| interleaved.chunks_exact(2).map(|f| f[channel]).collect())
}

struct Capture {
    actual: [Vec<f32>; 2],
    a: [Vec<f32>; 2],
    o: [Vec<f32>; 2],
    dry: [Vec<f32>; 2],
    a_origin: usize,
}

fn capture(lock: bool, pitch: PitchIntent, attack: Option<(Attack, usize)>) -> Capture {
    let timeline = format_chronology(START, END, &EVENTS, CAPTURE_END);
    assert_eq!(timeline[F].rate.to_bits(), 0.9200000000000003_f64.to_bits());
    assert_eq!(timeline[E].rate.to_bits(), 1.29_f64.to_bits());
    assert_eq!(timeline[E].frame, MARKER_BASE);
    let (mut fixture, source, stems, output_origin) =
        selection_setup_source(lock, PitchIntent::default(), RATE, analytic_source(attack));
    let generation = fixture.voice(0).generation;
    let address = fixture.voice(0).sample.as_ref().unwrap().source_address();
    let starts = fixture.starts(0);
    let mut actual = Vec::with_capacity(CAPTURE_END * 2);
    for (index, (at, rate)) in EVENTS.into_iter().enumerate() {
        actual.extend(fixture.render_to(output_origin + at as u64));
        if index == 0 {
            fixture.mixer.set_speed(rate);
        } else {
            if index == 1 {
                fixture.mixer.set_bpm_lock(true);
            }
            fixture.mixer.set_master_period(0.5 / rate);
        }
    }
    actual.extend(fixture.render_to(output_origin + F as u64));
    assert_format_phase(&fixture, timeline[F]);
    fixture.rows(PitchPadProjection {
        intent: PitchIntent::new(pitch.base_shift(), 0).unwrap(),
        key_lock: lock,
        quantize_step_64ths: Some(16),
        ..PitchPadProjection::default()
    });
    fixture.refresh_clock();
    let mut clock = fixture.clock.read().unwrap();
    clock.downbeat_frame = output_origin;
    clock.master_beat = F as f64 / 24_000.0;
    fixture.clock.publish(clock);
    let action = fixture
        .control
        .engine
        .capture_pad_pitch_action(0, &format!("pitch.extra:{}:set", pitch.extra_shift()), None)
        .unwrap();
    let receipt = (
        action.sequence(),
        action.received_timestamp(),
        action.content_key(),
    );
    assert_eq!(action.original_due_frame(), output_origin + E as u64);
    fixture.publish(&action);
    fixture.wait_worker(&[&action]);
    assert_eq!(fixture.transport.output_frame(), output_origin + F as u64);
    actual.extend(fixture.render_to(output_origin + E as u64));
    assert_format_phase(&fixture, timeline[E]);
    let mut native_address = None;
    while fixture.transport.output_frame() < output_origin + CAPTURE_END as u64 {
        let count = (output_origin + CAPTURE_END as u64 - fixture.transport.output_frame()).min(512)
            as usize;
        actual.extend(fixture.render(count));
        assert!(
            action.effective(),
            "{} {:?}",
            action.state(),
            action.reason()
        );
        let native = fixture.voice(0).stretch.native_state_address();
        assert_ne!(native, 0);
        assert_eq!(*native_address.get_or_insert(native), native);
    }
    assert_eq!(actual.len(), CAPTURE_END * 2);
    assert!(actual.iter().all(|sample| sample.is_finite()));
    assert_format_phase(&fixture, timeline[CAPTURE_END]);
    assert_eq!(fixture.voice(0).generation, generation);
    assert_eq!(
        fixture.voice(0).sample.as_ref().unwrap().source_address(),
        address
    );
    assert_eq!(fixture.starts(0), starts);
    assert_eq!(
        (
            action.sequence(),
            action.received_timestamp(),
            action.content_key()
        ),
        receipt
    );
    assert_eq!(action.execution_target(), output_origin + E as u64);
    assert_eq!(
        fixture.voice(0).stretch.adopted_request_id(),
        Some(action.sequence())
    );
    assert!(!fixture.voice(0).stretch.wet_mode_failed());
    assert!(fixture.scheduler.is_empty());

    let (a_origin, bound) = format_reset_origin(&timeline, E, RATE, pitch, lock);
    let mut raw_a = StereoRawNative::new(
        RATE,
        format_native_pitch(timeline[a_origin + 511], pitch, lock),
    );
    // Continuous-O uses its OWN first completed block, not A's cold pitch.
    let mut raw_o = StereoRawNative::new(RATE, format_native_pitch(timeline[511], pitch, lock));
    let mut a: [Vec<f32>; 2] = std::array::from_fn(|_| Vec::new());
    let mut o: [Vec<f32>; 2] = std::array::from_fn(|_| Vec::new());
    let mut dry: [Vec<f32>; 2] = std::array::from_fn(|_| Vec::new());
    let mut a_error = [0.0_f32; 2];
    let mut o_error = [0.0_f32; 2];
    for active in 0..CAPTURE_END {
        let input = format_selected_pcm(&timeline, active, &source, &stems, &[], START, END);
        let native = format_native_pitch(timeline[active], pitch, lock);
        let continuous = raw_o.frame(input, native);
        let reset = (active >= a_origin).then(|| raw_a.frame(input, native));
        assert!(input.into_iter().all(f32::is_finite));
        assert!(continuous.into_iter().all(f32::is_finite));
        if let Some(reset) = reset {
            assert!(reset.into_iter().all(f32::is_finite));
        }
        for channel in 0..2 {
            dry[channel].push(input[channel]);
            o[channel].push(continuous[channel]);
            if let Some(reset) = reset {
                a[channel].push(reset[channel]);
                if active >= E {
                    a_error[channel] =
                        a_error[channel].max((actual[active * 2 + channel] - reset[channel]).abs());
                    o_error[channel] = o_error[channel]
                        .max((actual[active * 2 + channel] - continuous[channel]).abs());
                }
            }
        }
    }
    assert!(
        a_error.into_iter().all(|error| error < 2.0e-6),
        "exact A provenance failed: {a_error:?}"
    );
    assert_eq!(
        raw_a.fifos(),
        [fixture.voice(0).stretch.pending_fifo_frames(); 2]
    );
    assert_eq!(raw_a.fifos(), raw_o.fifos());
    if attack.is_some() {
        assert!(
            actual[..CAPTURE_START * 2].iter().all(|x| *x == 0.0),
            "earlier actual response cannot be discarded"
        );
        assert!(
            dry.iter()
                .all(|channel| channel[..CAPTURE_START].iter().all(|x| *x == 0.0))
        );
    }
    println!(
        "variable_quality total={} lock={lock} attack={attack:?} O={output_origin} F={F} E={E} A={a_origin} H={bound} post_E_frames={} A_max={a_error:?} O_max={o_error:?}",
        pitch.total_semitones(),
        CAPTURE_END - E
    );
    Capture {
        actual: planar(&actual[CAPTURE_START * 2..]),
        a,
        o,
        dry,
        a_origin,
    }
}

fn marker_metrics(label: &str, channels: &[Vec<f32>; 2], origin: usize, dry_q: [i64; 3]) -> u8 {
    assert!(channels.iter().flatten().all(|x| x.is_finite()));
    let energy = stereo_energy(channels);
    let total: f64 = energy.iter().sum();
    assert!(
        total.is_finite() && total > 0.0,
        "{label}: missing isolated response"
    );
    let absolute_q = energy_times(&energy, RATE).map(|q| q + origin as i64);
    let offsets = std::array::from_fn::<_, 3, _>(|i| absolute_q[i] - dry_q[i]);
    let (peak_frame, peak) = stereo_peak(channels);
    let peak_at = origin + peak_frame;
    let cut = E.saturating_sub(origin).min(energy.len());
    let prefix = energy[..cut].iter().sum::<f64>() / total;
    let tail = energy[energy.len().saturating_sub(RATE as usize / 50)..]
        .iter()
        .sum::<f64>()
        / total;
    let timing = (f64::from(RATE) * TIMING_BUDGET_MS / 1_000.0).ceil() as i64;
    let failures = u8::from(offsets[0].abs() > timing)
        | (u8::from(offsets[1].abs() > timing) << 1)
        | (u8::from(peak_at < E) << 2)
        | (u8::from(prefix > MAX_DISCARDED_ENERGY) << 3)
        | (u8::from(tail > MAX_CAPTURE_TAIL_ENERGY) << 4);
    println!(
        "isolated_quality {label} origin={origin} frames={} original_peak_at={peak_at} peak={peak:.9} absolute_q={absolute_q:?} dry_q={dry_q:?} offsets={offsets:?} energy_before_E_fraction={prefix:.12} final20ms_fraction={tail:.12} fixed_E_cut_failure_mask={failures}",
        energy.len()
    );
    failures
}

#[test]
fn public_variable_rate_set_has_declared_stereo_plateau_pitch() {
    for lock in [true, false] {
        for pitch in [
            PitchIntent::new(6, 18).unwrap(),
            PitchIntent::new(-5, -18).unwrap(),
        ] {
            let captured = capture(lock, pitch, None);
            for (channel, source_hz) in [160.0, 211.0].into_iter().enumerate() {
                let pcm = &captured.actual[channel]
                    [TONE_START - CAPTURE_START..CAPTURE_END - CAPTURE_START];
                let cents = assert_tone_for_source(
                    pcm,
                    source_hz,
                    pitch.base_shift(),
                    pitch.extra_shift(),
                    1.29,
                    lock,
                );
                let onset_energy = captured.actual[channel]
                    [E - CAPTURE_START..TONE_START - CAPTURE_START]
                    .iter()
                    .map(|x| f64::from(*x).powi(2))
                    .sum::<f64>();
                println!(
                    "settled_quality total={} lock={lock} channel={channel} source_hz={source_hz} window=[{TONE_START},{CAPTURE_END}) cents={cents:.6} first_half_second_energy={onset_energy:.9}",
                    pitch.total_semitones()
                );
            }
        }
    }
}

#[test]
fn public_variable_rate_set_reports_uncropped_isolated_attack_criteria() {
    let mut cases = 0;
    let mut failures = [0_usize; 3];
    for lock in [true, false] {
        for pitch in [
            PitchIntent::new(6, 18).unwrap(),
            PitchIntent::new(-5, -18).unwrap(),
        ] {
            for signal in [Attack::Impulse, Attack::Tone, Attack::Percussion] {
                for offset in [0, 17, 511] {
                    let captured = capture(lock, pitch, Some((signal, offset)));
                    let dry_q = energy_times(&stereo_energy(&captured.dry), RATE);
                    for (index, (label, channels, origin)) in [
                        ("public_uncropped", &captured.actual, CAPTURE_START),
                        ("A_uncropped", &captured.a, captured.a_origin),
                        ("O_uncropped", &captured.o, 0),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        failures[index] +=
                            usize::from(marker_metrics(label, channels, origin, dry_q) != 0);
                    }
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 36);
    println!(
        "isolated_quality measured_cases={cases} any_fixed_criterion_failure_public_A_O={failures:?}; diagnostic only, no changed compensation or Human acceptance"
    );
}
