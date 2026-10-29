use super::*;

fn range() -> BpmRange {
    BpmRange::configured(true, 50.0, 170.0).unwrap().unwrap()
}
fn primary(bpm: f64) -> Value {
    json!({"schema_version":5,"backend":"beat_this","policy":"neural-inferred-ordinals-v5",
        "status":"estimated","bpm":bpm,"frame_count":28_800_000,"sample_rate_hz":48_000,
        "raw_beats":[0.06,0.56],"ordinal_fit":{"untouched":77.94247297546801}})
}
fn observed(primary_bpm: f64, bpm: f64) -> Observation {
    Observation {
        source: json!({"source_sha256":"retained source"}),
        qm: json!({"analyzer":{"frame_count":26_460_000},"estimate":{"bpm":bpm}}),
        bpm,
        status: "estimated",
        period_drift_seconds: half_double_drift(primary_bpm, bpm, 600.0).unwrap(),
    }
}

#[test]
fn bounds_are_inclusive_and_disabled_configuration_is_still_validated() {
    assert!(range().contains(50.0));
    assert!(range().contains(170.0));
    assert!(!range().contains(50.0_f64.next_down()));
    assert!(!range().contains(170.0_f64.next_up()));
    assert_eq!(BpmRange::configured(false, 50.0, 170.0).unwrap(), None);
    for (lo, hi) in [
        (f64::NAN, 170.0),
        (50.0, f64::INFINITY),
        (29.0, 170.0),
        (50.0, 301.0),
        (80.0, 80.0),
        (170.0, 50.0),
    ] {
        for enabled in [true, false] {
            assert!(BpmRange::configured(enabled, lo, hi).is_err());
        }
    }
}

#[test]
fn in_range_primary_retains_exact_bytes_without_qm() {
    let bpm = 77.94247297546801;
    let mut tree = primary(bpm);
    let exact = tree.to_string();
    let result = apply(
        range(),
        bpm,
        &mut tree,
        exact.clone(),
        usize::MAX,
        0,
        0,
        0,
        &|| false,
        |_| panic!("in-range primary must not call QM"),
    )
    .unwrap();
    assert_eq!(result.bpm, Some(bpm));
    assert_eq!(result.method_json.as_deref(), Some(exact.as_str()));
    assert_eq!(tree["schema_version"], 5);
}

#[test]
fn period_first_primary_is_precise_in_range_and_can_use_the_existing_range_choice() {
    for bpm in [77.94247297546801,174.0] {
        let mut tree=primary(bpm);
        tree["schema_version"]=json!(9);
        tree["policy"]=json!("neural-inferred-periods-v9");
        let raw=tree["raw_beats"].clone();
        let exact=tree.to_string();
        let calls=std::cell::Cell::new(0);
        let result=apply(range(),bpm,&mut tree,exact.clone(),0,0,0,0,&||false,|_| {
            calls.set(calls.get()+1); Ok(observed(bpm,87.0))
        }).unwrap();
        assert_eq!(tree["raw_beats"],raw);
        if range().contains(bpm) {
            assert_eq!(calls.get(),0);
            assert_eq!(result.method_json.as_deref(),Some(exact.as_str()));
            assert_eq!(result.bpm,Some(bpm));
        } else {
            assert_eq!(calls.get(),1);
            assert_eq!(result.bpm,Some(87.0));
            assert_eq!(tree["bpm_range"]["primary"]["schema_version"],9);
        }
    }
}

#[test]
fn symmetric_half_double_drift_has_a_strict_five_millisecond_boundary() {
    assert_eq!(half_double_drift(60.0, 128.0, 0.08).unwrap(), 0.005);
    assert_eq!(half_double_drift(128.0, 60.0, 0.08).unwrap(), 0.005);
    assert!(half_double_drift(60.0, 128.0, 0.08_f64.next_up()).is_err());
    assert!(half_double_drift(128.0, 60.0, 0.08_f64.next_up()).is_err());
    for args in [
        (120.0, 120.0, 600.0),
        (120.0, 87.0, 600.0),
        (f64::NAN, 87.0, 600.0),
        (174.0, 87.0, 0.0),
    ] {
        assert!(half_double_drift(args.0, args.1, args.2).is_err());
    }
}

#[test]
fn independent_fractional_qm_is_selected_once_without_rounding_or_raw_changes() {
    let bpm = 87.000123;
    let original_bpm = bpm * 2.0;
    let mut tree = primary(original_bpm);
    let raw = tree["raw_beats"].clone();
    let fit = tree["ordinal_fit"].clone();
    let exact = tree.to_string();
    let calls = std::cell::Cell::new(0);
    let result = apply(
        range(),
        original_bpm,
        &mut tree,
        exact,
        0,
        0,
        0,
        0,
        &|| false,
        |_| {
            calls.set(calls.get() + 1);
            Ok(observed(original_bpm, bpm))
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(result.bpm, Some(bpm));
    assert_eq!(tree["raw_beats"], raw);
    assert_eq!(tree["ordinal_fit"], fit);
    assert_eq!(tree["bpm_range"]["primary"]["bpm"], original_bpm);
    assert_eq!(tree["bpm_range"]["outcome"], "qm_half_double");
    assert!(result.beat_error.is_none());
}

#[test]
fn own_qm_refusal_panic_and_v6_primary_never_publish_the_outside_value() {
    for panic in [false, true] {
        let mut tree = primary(174.0);
        let base = tree.to_string();
        let result = apply(
            range(),
            174.0,
            &mut tree,
            base,
            0,
            0,
            0,
            0,
            &|| false,
            |_| {
                if panic {
                    panic!("optional QM panic");
                }
                Err(unavailable("QM has no independently admitted tempo"))
            },
        )
        .unwrap();
        assert!(result.bpm.is_none());
        let rejected: Value = serde_json::from_str(result.method_json.as_ref().unwrap()).unwrap();
        assert!(rejected["bpm"].is_null());
        assert_eq!(rejected["bpm_range"]["primary"]["bpm"], 174.0);
        assert!(rejected["bpm_range"].get("selection").is_none());
        assert!(
            result
                .beat_error
                .unwrap()
                .contains("No automatic BPM within")
        );
    }
    let mut tree = primary(174.0);
    tree["schema_version"] = json!(6);
    tree["backend"] = json!("beat_this_qm_dsp");
    tree["qm_corroboration"] = json!({"original":"retained"});
    let base = tree.to_string();
    let result = apply(
        range(),
        174.0,
        &mut tree,
        base,
        0,
        0,
        0,
        0,
        &|| false,
        |_| panic!("v6 must not call another QM selection"),
    )
    .unwrap();
    assert!(result.bpm.is_none());
    assert_eq!(tree["bpm_range"]["primary"]["schema_version"], 6);
    assert_eq!(tree["qm_corroboration"]["original"], "retained");
}

#[test]
fn unsupported_relation_or_outside_qm_cannot_replace_the_primary() {
    for bpm in [100.0, 200.0] {
        let mut tree = primary(174.0);
        let base = tree.to_string();
        let result = apply(
            range(),
            174.0,
            &mut tree,
            base,
            0,
            0,
            0,
            0,
            &|| false,
            |_| {
                Ok(Observation {
                    source: json!({}),
                    qm: json!({"analyzer":{"frame_count":26_460_000}}),
                    bpm,
                    status: "estimated",
                    period_drift_seconds: 0.0,
                })
            },
        )
        .unwrap();
        assert!(result.bpm.is_none());
        let rejected: Value = serde_json::from_str(result.method_json.as_ref().unwrap()).unwrap();
        assert!(rejected["bpm_range"].get("selection").is_none());
    }
}

#[test]
fn minimal_and_late_budget_failures_cannot_restore_an_outside_bpm() {
    let mut tree = primary(174.0);
    let base = tree.to_string();
    let result = apply(
        range(),
        174.0,
        &mut tree,
        base,
        LIMIT,
        0,
        0,
        0,
        &|| false,
        |_| panic!("unadmitted rejected metadata must not call QM"),
    )
    .unwrap();
    assert!(result.bpm.is_none() && result.method_json.is_none());
    let mut tree = primary(174.0);
    let base = tree.to_string();
    let live = LIMIT - 2 * SMALL;
    let result = apply(
        range(),
        174.0,
        &mut tree,
        base,
        live,
        live,
        0,
        0,
        &|| false,
        |_| {
            let mut qm = observed(174.0, 87.0);
            qm.qm["large_diagnostic"] = json!("x".repeat(2 * SMALL));
            Ok(qm)
        },
    )
    .unwrap();
    assert!(result.bpm.is_none());
    let rejected: Value = serde_json::from_str(result.method_json.as_ref().unwrap()).unwrap();
    assert_eq!(rejected["bpm_range"]["outcome"], "no_supported_candidate");
    assert!(rejected["bpm_range"].get("selection").is_none());
}

#[test]
fn cancellation_wins_over_refusal_and_late_selection() {
    let mut tree = primary(174.0);
    let base = tree.to_string();
    assert!(matches!(
        apply(
            range(),
            174.0,
            &mut tree,
            base,
            usize::MAX,
            0,
            0,
            0,
            &|| true,
            |_| panic!("canceled work must not call QM")
        ),
        Err(Failure::Cancelled)
    ));
    let stale = std::cell::Cell::new(false);
    let mut tree = primary(174.0);
    let base = tree.to_string();
    assert!(matches!(
        apply(
            range(),
            174.0,
            &mut tree,
            base,
            0,
            0,
            0,
            0,
            &|| stale.get(),
            |_| {
                stale.set(true);
                Ok(observed(174.0, 87.0))
            }
        ),
        Err(Failure::Cancelled)
    ));
}

#[test]
fn scientific_filter_preserves_completed_key_and_existing_grid() {
    let original = LoadedAnalysis {
        bpm: 174.0,
        key: Some("F minor".into()),
        key_status: AnalysisKeyStatus::Ready,
        key_error: None,
        beat_grid: flitzis_looper_analysis::BeatGrid {
            beats: vec![0.1],
            downbeats: vec![],
            bars: vec![],
        },
        bpm_method_json: None,
        beat_error: None,
    };
    let disabled = scientific(None, original.clone());
    assert_eq!(disabled.bpm, 174.0);
    assert!(disabled.beat_error.is_none());
    let result = scientific(Some(range()), original);
    assert_eq!(result.bpm, 0.0);
    assert_eq!(result.key.as_deref(), Some("F minor"));
    assert_eq!(result.key_status, AnalysisKeyStatus::Ready);
    assert_eq!(result.beat_grid.beats, vec![0.1]);
    assert!(result.bpm_method_json.is_none());
    assert!(result.beat_error.unwrap().contains("within 50..170"));
}
