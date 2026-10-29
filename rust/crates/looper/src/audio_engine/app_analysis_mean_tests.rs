use super::*;

fn base() -> Value {
    json!({"schema_version":9,"backend":"beat_this","policy":"neural-inferred-periods-v9",
        "status":"uncorroborated","bpm":null,"raw_beats":[0.1,0.6],"retained_metric":77.94247297546801})
}
fn precise(bpm: f64) -> CompletedObservation {
    CompletedObservation {
        source: json!({}),
        qm: json!({"estimate":{"bpm":bpm}}),
        bpm: Some(bpm),
        status: "estimated",
        mean: None,
        period_drift_seconds: 0.0,
    }
}

#[test]
fn independent_precise_qm_precedes_mean_and_final_range_never_selects_mean_instead() {
    let mut tree = base();
    let original = tree.to_string();
    let calls = std::cell::Cell::new(0);
    let chosen = attempt(
        &[],
        300.0,
        None,
        &mut tree,
        &original,
        0,
        0,
        0,
        0,
        &|| false,
        |_| {
            calls.set(calls.get() + 1);
            Ok(precise(86.123456789))
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(chosen.bpm, 86.123456789);
    assert_eq!(tree["tempo_fallback"]["kind"], "precise_qm");
    assert!(tree["tempo_fallback"].get("mean").is_none());
    assert!(tree["tempo_fallback"].get("range_constraint").is_none());
    assert_eq!(tree["raw_beats"], json!([0.1, 0.6]));
    let mut rejected = base();
    let exact = rejected.to_string();
    let range = BpmRange::configured(true, 100.0, 150.0).unwrap();
    assert!(matches!(
        attempt(
            &[],
            300.0,
            range,
            &mut rejected,
            &exact,
            0,
            0,
            0,
            0,
            &|| false,
            |_| Ok(precise(86.123456789))
        ),
        Err(Failure::Unavailable(_))
    ));
    assert_eq!(rejected, base());
    assert_eq!(exact, original);
}

#[test]
fn precise_observation_near_limit_refuses_before_envelope_mutation() {
    let mut tree = base();
    let original_tree = tree.clone();
    let original = tree.to_string();
    let exact = original.as_bytes().to_vec();
    let base_core = LIMIT - SMALL - 16 * 1024;
    assert!(admit(&[base_core, SMALL]).is_ok());
    let calls = std::cell::Cell::new(0);
    let result = attempt(
        &[],
        300.0,
        None,
        &mut tree,
        &original,
        base_core,
        base_core,
        0,
        0,
        &|| false,
        |_| {
            calls.set(calls.get() + 1);
            let mut observed = precise(120.0);
            // Actual heap is small; the held-source floor models the near-limit
            // parent without allocating hundreds of MiB in this regression.
            observed.source = json!({"retained": "s".repeat(32 * 1024)});
            observed.qm["retained"] = json!("q".repeat(32 * 1024));
            Ok(observed)
        },
    );
    assert!(matches!(result, Err(Failure::Unavailable(_))));
    assert_eq!(calls.get(), 1);
    assert_eq!(tree, original_tree);
    assert_eq!(tree["bpm"], Value::Null);
    assert!(tree.get("tempo_fallback").is_none());
    assert_eq!(original.as_bytes(), exact.as_slice());
}

#[test]
fn mean_transaction_uses_unrounded_neural_scalar_and_preserves_original_strict_header() {
    let beats: Vec<_> = (0..600)
        .map(|i| {
            let x = i as f64;
            0.1 + 0.5 * x + 0.0000005 * x * x
        })
        .collect();
    let qm_mean = app_tempo::mean::interior(&beats, 301.0, usize::MAX, &|| false)
        .unwrap()
        .unwrap();
    let expected = qm_mean.bpm;
    let mut tree = base();
    let original = tree.to_string();
    let choice = attempt(
        &beats,
        301.0,
        None,
        &mut tree,
        &original,
        0,
        0,
        0,
        0,
        &|| false,
        |_| {
            Ok(CompletedObservation {
                source: json!({}),
                qm: json!({"estimate":{"bpm":null}}),
                bpm: None,
                status: "uncorroborated",
                mean: Some(qm_mean),
                period_drift_seconds: 0.0,
            })
        },
    )
    .unwrap();
    assert_eq!(choice.bpm.to_bits(), expected.to_bits());
    assert_eq!(tree["status"], "mean_estimate");
    assert_eq!(tree["tempo_fallback"]["neural_result"]["schema_version"], 9);
    assert_eq!(tree["tempo_fallback"]["neural_result"]["bpm"], Value::Null);
    assert_eq!(
        tree["tempo_fallback"]["mean"]["maximum_period_ratio"]
            .as_f64()
            .unwrap()
            <= 1.01,
        true
    );
    assert!(original.contains("77.94247297546801"));
}

#[test]
fn late_failure_and_cancel_never_commit_a_new_bpm_method_tuple() {
    let mut tree = base();
    let original = tree.to_string();
    let exact = original.clone();
    assert!(matches!(
        complete(
            &mut tree,
            json!({}),
            120.0,
            "mean_estimate",
            &original,
            LIMIT - 1,
            0,
            0,
            &|| false
        ),
        Err(Failure::Unavailable(_))
    ));
    assert_eq!(original, exact);
    let stale = std::cell::Cell::new(false);
    let mut tree = base();
    assert!(matches!(
        attempt(
            &[],
            300.0,
            None,
            &mut tree,
            &original,
            0,
            0,
            0,
            0,
            &|| stale.get(),
            |_| {
                stale.set(true);
                Ok(precise(120.0))
            }
        ),
        Err(Failure::Cancelled)
    ));
    assert_eq!(tree, base());
}

/// Input: {cases:[{id,neural_method,neural_float_bits?,source,qm,qm_float_bits?,
/// duration_bits,minimum_bpm?,maximum_bpm?}]}. Float maps are JSON-pointer->u64
/// relative to neural_method/qm. Source/QM receipts remain supplied historical
/// observations, never a new source verification, DSP call, inference or OwnACK.
#[test]
#[ignore = "explicit owned genuine cached NN/QM evidence; no source, model, DSP or OwnACK"]
fn replay_cached_tempo_fallback() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    fn owned(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        assert!(path.is_absolute());
        let owner = path.parent().ok_or("parent")?.join("OWNER.md");
        assert!(std::fs::metadata(&owner)?.len() <= 65_536);
        assert!(!std::fs::read_to_string(owner)?.trim().is_empty());
        Ok(())
    }
    fn bits(tree: &mut Value, map: Option<&Value>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(map) = map {
            let map = map.as_object().ok_or("float map")?;
            assert!(map.len() <= 250_000);
            for (pointer, bits) in map {
                assert!(pointer.starts_with('/') && pointer.len() <= 1024);
                let slot = tree.pointer_mut(pointer).ok_or("float pointer")?;
                assert!(slot.is_f64());
                *slot = json!(f64::from_bits(bits.as_u64().ok_or("float bits")?));
                assert!(slot.as_f64().is_some_and(f64::is_finite));
            }
        }
        Ok(())
    }
    let input = PathBuf::from(std::env::var_os("FLITZI_TEMPO_FALLBACK_INPUT").ok_or("input")?);
    let output = PathBuf::from(std::env::var_os("FLITZI_TEMPO_FALLBACK_OUTPUT").ok_or("output")?);
    owned(&input)?;
    owned(&output)?;
    assert!(std::fs::metadata(&input)?.len() <= 64 * 1024 * 1024);
    let mut data: Value = serde_json::from_slice(&std::fs::read(input)?)?;
    let cases = data["cases"].as_array_mut().ok_or("cases")?;
    assert!(cases.len() <= 64);
    let mut report = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    for case in cases {
        let mut neural = case["neural_method"].take();
        bits(&mut neural, case.get("neural_float_bits"))?;
        let mut qm = case["qm"].take();
        bits(&mut qm, case.get("qm_float_bits"))?;
        assert_eq!(neural["schema_version"], 9);
        assert_eq!(neural["ordinal_fit"]["outcome"], "conflicting_sections");
        assert_eq!(neural["ordinal_fit"]["candidate_limit_reached"], false);
        let duration = f64::from_bits(case["duration_bits"].as_u64().ok_or("duration bits")?);
        assert!(duration.is_finite() && duration > 0.0);
        let neural_beats: Vec<_> = neural["raw_beats"]
            .as_array()
            .ok_or("NN beats")?
            .iter()
            .map(|n| n.as_f64().ok_or("NN time"))
            .collect::<Result<_, _>>()?;
        let hop = qm["odf_hop_samples"].as_u64().ok_or("QM hop")?;
        let rate = qm["analyzer"]["sample_rate_hz"].as_u64().ok_or("QM rate")?;
        assert_eq!(rate, 44_100);
        let qm_beats: Vec<_> = qm["raw_beat_frames"]
            .as_array()
            .ok_or("QM frames")?
            .iter()
            .map(|n| {
                n.as_f64()
                    .ok_or("QM frame")
                    .map(|n| n * (hop as f64 / rate as f64))
            })
            .collect::<Result<_, _>>()?;
        let downbeats = qm["raw_downbeat_indices"]
            .as_array()
            .ok_or("QM downbeats")?
            .len();
        let analyzer_duration =
            qm["analyzer"]["frame_count"].as_u64().ok_or("QM extent")? as f64 / rate as f64;
        let estimate =
            app_tempo::estimate_with_budget(&qm_beats, analyzer_duration, downbeats, LIMIT);
        assert!(!app_tempo::optional_resource_refusal(&estimate));
        let mean = if app_tempo::completed_conflict(&estimate) {
            app_tempo::mean::interior(&qm_beats, duration, LIMIT, &|| false)
                .map_err(|e| format!("{e:?}"))?
        } else {
            None
        };
        let bpm = estimate.bpm;
        let status = estimate.status;
        let version = app_tempo::schema_version(&estimate);
        let mut own = serde_json::to_value(estimate)?;
        own.as_object_mut().unwrap().remove("policy");
        own["kernel_version"] = json!(version);
        qm["estimate"] = own;
        let observed = CompletedObservation {
            source: case["source"].take(),
            qm,
            bpm,
            status,
            mean,
            period_drift_seconds: 0.0,
        };
        let base = neural.to_string();
        let range = if case.get("minimum_bpm").is_some() {
            BpmRange::configured(
                true,
                case["minimum_bpm"].as_f64().ok_or("minimum")?,
                case["maximum_bpm"].as_f64().ok_or("maximum")?,
            )?
        } else {
            None
        };
        let selected = attempt(
            &neural_beats,
            duration,
            range,
            &mut neural,
            &base,
            0,
            0,
            0,
            0,
            &|| false,
            |_| Ok(observed),
        );
        let row = match selected {
            Ok(choice) => json!({"id":case["id"],"method":neural,"bpm":choice.bpm}),
            Err(Failure::Cancelled) => return Err("unexpected cancellation".into()),
            Err(error) => {
                // Refusals before commit leave this exact restored tree intact;
                // an exceptional late refusal reports original bytes directly.
                json!({"id":case["id"],"base_method_json":base,"bpm":null,"reason":error.to_string()})
            }
        };
        writeln!(
            report,
            "{}",
            json!({"result":row,"scope":"Cached scalar transaction only; no fresh source, DSP, inference or OwnACK"})
        )?;
    }
    report.sync_all()?;
    Ok(())
}
