use super::*;

fn empty() -> Value {
    json!({"schema_version":4,"backend":"beat_this","policy":"neural-unavailable",
        "status":"insufficient","bpm":null,"selected":null,"regions":[],
        "raw_beats":[],"raw_downbeats":[],"model_sha256":"a".repeat(64),
        "frontend_id":"beat-this-1.1.0-soxr-hq-logmel-v1","source_sha256":"b".repeat(64),
        "mono_sha256":"c".repeat(64),"sample_rate_hz":8000,"frame_count":80000})
}

fn clicks(rate: u32, bpm: f64, seconds: usize) -> SampleBuffer {
    let mut samples = vec![0.0_f32; rate as usize * seconds * 2];
    let count = (seconds as f64 * bpm / 60.0) as usize;
    for beat in 0..count {
        let start = ((beat as f64 * 60.0 / bpm + 0.1) * f64::from(rate)) as usize;
        for frame in start..(start + rate as usize / 200).min(samples.len() / 2) {
            samples[frame * 2] = 0.5;
            samples[frame * 2 + 1] = -0.5;
        }
    }
    SampleBuffer {
        channels: 2,
        samples: samples.into(),
        residency: None,
    }
}

#[test]
fn approximate_reuses_rejected_period_without_relabelling_precision() {
    let mut base = empty();
    base["schema_version"] = json!(9);
    base["policy"] = json!("neural-inferred-periods-v9");
    base["ordinal_fit"] = json!({"winner":{"period_seconds":0.6666348364905366},
        "candidate_limit_reached":false,"evidence_tier":"two_ten_second",
        "assignments":[0,1,2,3,4,5,6,7],"outcome":"insufficient"});
    let before = base.clone();
    let sample = clicks(8000, 120.0, 10);
    let chosen = attempt(
        &sample,
        8000,
        None,
        &base.to_string(),
        "QM same-period agreement failed",
        0,
        &|| false,
    )
    .unwrap();
    assert_eq!(chosen.bpm, 60.0 / 0.6666348364905366);
    assert_eq!(base, before);
    let method: Value = serde_json::from_str(&chosen.method_json).unwrap();
    assert_eq!(method["status"], "approximate_estimate");
    assert_eq!(method["ordinal_fit"]["outcome"], "insufficient");
    assert_eq!(method["approximate_tempo"]["original"]["bpm"], Value::Null);
    assert_eq!(method["approximate_tempo"]["kind"], "neural_period");
    assert!(method.get("accepted_timing").is_none());
    assert!(method.get("execution").is_none());
    let range = BpmRange::configured(true, 100.0, 140.0).unwrap();
    assert!(matches!(
        attempt(
            &sample,
            8000,
            range,
            &base.to_string(),
            "failed",
            0,
            &|| false
        ),
        Err(Failure::NoChoice)
    ));
}

#[test]
fn bounded_energy_supports_fractional_tempo_rates_and_opposite_stereo() {
    for rate in [44100, 48000, 96000] {
        for bpm in [90.004297, 120.0] {
            let sample = clicks(rate, bpm, 20);
            let (period, windows) = energy(&sample, rate, &|| false).unwrap().unwrap();
            assert!(
                (60.0 / period - bpm).abs() < 1.0,
                "rate{rate} bpm{bpm} actual{}",
                60.0 / period
            );
            assert!(windows.as_array().unwrap().len() <= 3);
            assert!(
                windows
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|w| w["novelty"].as_array().unwrap().len() <= 2000)
            );
            let mut base = empty();
            base["sample_rate_hz"] = json!(rate);
            base["frame_count"] = json!(sample.frame_count());
            let chosen = attempt(
                &sample,
                rate,
                None,
                &base.to_string(),
                "inference unavailable",
                0,
                &|| false,
            )
            .unwrap();
            let method: Value = serde_json::from_str(&chosen.method_json).unwrap();
            assert_eq!(method["approximate_tempo"]["original"], Value::Null);
            assert_eq!(method["raw_beats"], json!([]));
            if let Some(root) = std::env::var_os("FLITZIS_APPROXIMATE_EVIDENCE_DIR") {
                let root = std::path::PathBuf::from(root);
                assert!(root.is_absolute());
                std::fs::create_dir_all(&root).unwrap();
                std::fs::write(
                    root.join(format!("energy-{rate}-{bpm}.json")),
                    chosen.method_json,
                )
                .unwrap();
            }
        }
    }
}

#[test]
fn stationary_tones_and_noise_do_not_supply_rhythm() {
    for rate in [44100, 48000, 96000] {
        for hz in [110.3, 440.0, 997.0] {
            let samples: Vec<f32> = (0..rate * 10)
                .map(|frame| {
                    (0.5 * (f64::from(frame) * hz * std::f64::consts::TAU / f64::from(rate)).sin())
                        as f32
                })
                .collect();
            let sample = SampleBuffer {
                channels: 1,
                samples: samples.into(),
                residency: None,
            };
            assert!(
                energy(&sample, rate, &|| false).unwrap().is_none(),
                "stationary {rate}/{hz}"
            );
        }
    }
    let mut seed = 12345_u32;
    let samples: Vec<f32> = (0..480000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (f64::from(seed) / f64::from(u32::MAX) - 0.5) as f32
        })
        .collect();
    let sample = SampleBuffer {
        channels: 1,
        samples: samples.into(),
        residency: None,
    };
    assert!(energy(&sample, 48000, &|| false).unwrap().is_none());
}

#[test]
fn source_windows_cover_quiet_intro_and_nearly_tied_different_tempos() {
    let mut sample = clicks(8000, 120.0, 60);
    let mut mixed = sample.samples.to_vec();
    let alternate = clicks(8000, 100.0, 20);
    mixed[8000 * 20 * 2..8000 * 40 * 2].copy_from_slice(&alternate.samples);
    for value in &mut mixed[8000 * 40 * 2..] {
        *value *= 1.0000001192092896;
    }
    sample.samples = mixed.into();
    let mut base = empty();
    base["frame_count"] = json!(sample.frame_count());
    let choice = attempt(
        &sample,
        8000,
        None,
        &base.to_string(),
        "inference unavailable",
        0,
        &|| false,
    )
    .unwrap();
    assert!([100.0, 120.0].iter().any(|v| (choice.bpm - v).abs() < 0.01));
    if let Some(root) = std::env::var_os("FLITZIS_APPROXIMATE_EVIDENCE_DIR") {
        let root = std::path::PathBuf::from(root);
        assert!(root.is_absolute());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("mixed-output.json"), choice.method_json).unwrap();
    }
    let mut quiet = clicks(8000, 120.0, 60);
    let mut samples = quiet.samples.to_vec();
    samples[..8000 * 30 * 2].fill(0.0);
    quiet.samples = samples.into();
    assert!((60.0 / energy(&quiet, 8000, &|| false).unwrap().unwrap().0 - 120.0).abs() < 0.1);
}

#[test]
#[ignore = "requires explicitly generated source-bound replay packets in the owned cache"]
fn replay_saved_and_interval_packets_through_native_wrapper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FLITZIS_APPROXIMATE_EVIDENCE_DIR").unwrap());
    assert!(root.is_absolute());
    let sample = clicks(8000, 120.0, 10);
    for name in ["period", "interval"] {
        let original = std::fs::read_to_string(root.join(format!("{name}-input.json"))).unwrap();
        let choice = attempt(
            &sample,
            8000,
            None,
            &original,
            "Original precise refusal retained",
            0,
            &|| false,
        )
        .unwrap();
        std::fs::write(root.join(format!("{name}-output.json")), choice.method_json).unwrap();
    }
}

#[test]
fn unusable_corrupt_cancelled_and_unadmitted_sources_never_invent_tempo() {
    for value in [0.0_f32, 0.5, f32::NAN, f32::INFINITY] {
        let sample = SampleBuffer {
            channels: 1,
            samples: vec![value; 80000].into(),
            residency: None,
        };
        assert!(energy(&sample, 8000, &|| false).unwrap().is_none());
    }
    let short = clicks(8000, 120.0, 2);
    assert!(energy(&short, 8000, &|| false).unwrap().is_none());
    let sample = clicks(8000, 120.0, 10);
    assert!(matches!(
        attempt(
            &sample,
            8000,
            None,
            &empty().to_string(),
            "failed",
            0,
            &|| true
        ),
        Err(Failure::Cancelled)
    ));
    assert!(matches!(
        attempt(
            &sample,
            8000,
            None,
            &empty().to_string(),
            "failed",
            LIMIT,
            &|| false
        ),
        Err(Failure::Unavailable(_))
    ));
    let calls = std::cell::Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() > 850
    };
    assert!(matches!(
        energy(&sample, 8000, &cancel),
        Err(Failure::Cancelled)
    ));
}
