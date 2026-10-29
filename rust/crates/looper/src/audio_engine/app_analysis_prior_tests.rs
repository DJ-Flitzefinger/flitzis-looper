use super::*;

fn profile() -> Profile {
    Profile {
        schema_version: 1,
        installation_root: if cfg!(windows) {
            "C:/owned/installation-00000000000000000000000000000000"
        } else {
            "/owned/installation-00000000000000000000000000000000"
        }
        .into(),
        profile_sha256: "a".repeat(64),
        numeric_profile_sha256: format!("{:x}", Sha256::digest(NUMERIC_PROFILE.as_bytes())),
    }
}
fn tree(bpm: f64) -> Value {
    json!({"schema_version":4,"backend":"beat_this","policy":"neural-stable-regions-v4","status":"estimated",
        "bpm":bpm,"frame_count":28_800_000,"sample_rate_hz":48_000,"raw_beats":[0.06,0.56],"raw_downbeats":[0.06],
        "selected":{"retained_metric":77.94247297546801}})
}
fn input(profile: &Profile) -> Value {
    json!({"schema_version":1,"identity":{"pad_id":0,"request_id":1,"source_id":"b".repeat(64),"source_generation":1},
        "installation_root":profile.installation_root,"profile_sha256":profile.profile_sha256,
        "pcm":{"path":"owned mono","source_sha256":"b".repeat(64),"mono_sha256":"c".repeat(64),"sample_rate_hz":48_000,
            "frame_count":28_800_000,"origin_seconds":0.0,"dtype":"float32-le","channels":1,"downmix_revision":"arithmetic-channel-mean-f64-v1"},
        "request_directory":"owned directory","native_live_bytes":0,"aggregate_limit_bytes":LIMIT})
}
fn packet(bytes: &[u8], encoding: &str, shape: &[usize]) -> Value {
    json!({"encoding":encoding,"shape":shape,"sha256":format!("{:x}",Sha256::digest(bytes)),"data":STANDARD.encode(bytes)})
}
fn hex(value: f64) -> String {
    value
        .to_le_bytes()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}
fn receipt(profile: &Profile, request: &Value, shape: Geometry, class: usize) -> Value {
    let mut bytes = Vec::with_capacity(shape.probability_bytes);
    let mut global = Vec::with_capacity(2048);
    for _ in 0..shape.patches {
        for i in 30..=285 {
            bytes.extend_from_slice(&(if i == class { 1.0_f32 } else { 0.0 }).to_le_bytes());
        }
    }
    for i in 30..=285 {
        global.extend_from_slice(&(if i == class { 1.0_f64 } else { 0.0 }).to_le_bytes());
    }
    let mut global = packet(&global, "float64-le/base64", &[256]);
    global["first_argmax_class"] = json!(class);
    let mut source = request["pcm"].clone();
    source.as_object_mut().unwrap().remove("path");
    source.as_object_mut().unwrap().remove("dtype");
    source.as_object_mut().unwrap().remove("channels");
    let prior = json!({"profile":{"profile_id":PROFILE_ID,"profile_sha256":profile.profile_sha256,
        "numeric_profile_sha256":profile.numeric_profile_sha256,"python_version":"3.12.13",
        "python_executable_sha256":"f598fb950a86a895d8f9b4755fc9b38c48adc7a15732a342e55c17a3c3499602",
        "python_dll_sha256":"70713cbbbcacd1ca70f802e87683d85aea185da1aae9bd31d6c02be51cfee819",
        "node_version":"v25.5.0","node_executable_sha256":"d7f55cbc1b1ca9cc2815d7b8c58f94e4822a68cdf5ca55b4f2c9625581b92839",
        "tfjs_version":"4.22.0","graph_sha256":GRAPH_SHA256,"shard_sha256":SHARD_SHA256,
        "frontend_reference_sha256":FRONTEND_REFERENCE,"frontend_runner_sha256":FRONTEND_RUNNER,
        "inference_reference_sha256":INFERENCE_REFERENCE,"inference_runner_sha256":INFERENCE_RUNNER},
        "input":source,"frontend":{"frontend_id":"tempocnn-author-482c64ad-librosa011-global-std-f16-v1",
        "sample_rate_hz":11025,"frame_count":shape.analyzer_frames,"origin_seconds":0.0,"mono_sha256":"d".repeat(64),
        "feature_shape":[shape.patches,40,256,1],"feature_sha256":"e".repeat(64),
        "normalization":{"scope":"entire-overlapping-window-tensor","precision":"float64","roundtrip":"float16-to-float32",
            "mean_f64_le_hex":hex(-0.0),"std_f64_le_hex":hex(1.0)}},
        "probabilities":packet(&bytes,"float32-le/base64",&[shape.patches,256]),"global":global,
        "execution":{"origin":"computed","device":"cpu"},
        "resources":{"native_live_bytes":request["native_live_bytes"],"aggregate_limit_bytes":LIMIT,
            "frontend_admitted_child_peak_bytes":MIB,"frontend_observed_child_peak_bytes":MIB,
            "classifier_observed_child_peak_bytes":MIB,"measurement":"windows-private-and-working-set-peaks","aggregate_claim":true}});
    let mut echo = request["pcm"].clone();
    echo.as_object_mut().unwrap().remove("path");
    json!({"schema_version":1,"status":"completed","reason":"","resources_released":true,
        "identity":request["identity"],"input":echo,"profile":profile.token(),"prior":prior})
}
fn observation(primary: f64, bpm: f64) -> Observation {
    Observation {
        source: json!({}),
        qm: json!({"analyzer":{"frame_count":26_460_000},"estimate":{"kernel_version":4}}),
        bpm,
        status: "estimated",
        period_drift_seconds: bpm_range::half_double_drift(primary, bpm, 600.0).unwrap(),
    }
}

#[test]
fn exact_classes_reject_midpoints_and_uncovered_values() {
    for bpm in [29.999, 285.001, 60.5, 100.5, f64::NAN, f64::INFINITY] {
        assert!(candidate_class(bpm).is_none());
    }
    assert_eq!(candidate_class(30.0), Some(30));
    assert_eq!(candidate_class(285.0), Some(285));
    assert_eq!(candidate_class(77.94247297546801), Some(78));
}

#[test]
fn retired_schema8_tool_rejects_new_qm_kernel_before_any_classifier_call() {
    let profile=profile();
    let mut tree=tree(61.0);
    let base=tree.to_string();
    let result=transact(&mut tree,&base,0,0,0,0,None,geometry(28_800_000,48_000).unwrap(),
        &profile,input(&profile),false,&||false,|_,primary| {
            let mut observation=observation(primary,122.0);
            observation.qm["estimate"]["kernel_version"]=json!(9);
            Ok(observation)
        },|_|panic!("retired8 must not call a classifier for kernel9"));
    assert!(matches!(result,Err(Failure::NoChoice)));
    assert_eq!(tree.to_string(),base);
}
#[test]
fn full_geometry_reservation_matches_the_retained_two_resource_cases() {
    let day = geometry(21_154_169, 48_000).unwrap();
    assert_eq!((day.patches, day.bridge_bytes), (73, 1_607_696));
    let wait = geometry(20_483_344, 48_000).unwrap();
    assert_eq!((wait.patches, wait.bridge_bytes), (70, 1_585_168));
    assert!(geometry(u64::MAX, 8_000).is_err());
    assert!(geometry(0, 48_000).is_err());
}
#[test]
fn precise_qm_selection_retains_original_math_and_raw_once() {
    let profile = profile();
    let shape = geometry(28_800_000, 48_000).unwrap();
    let mut tree = tree(61.0000615);
    let original = tree.clone();
    let base = tree.to_string();
    let calls = std::cell::Cell::new(0);
    let chosen = transact(
        &mut tree,
        &base,
        0,
        0,
        0,
        0,
        None,
        shape,
        &profile,
        input(&profile),
        false,
        &|| false,
        |_, primary| Ok(observation(primary, 122.000123)),
        |request| {
            calls.set(calls.get() + 1);
            Ok(receipt(&profile, request, shape, 122))
        },
    )
    .unwrap();
    assert_eq!(chosen.bpm, 122.000123);
    assert_eq!(calls.get(), 1);
    assert_eq!(tree["schema_version"], 8);
    assert_eq!(
        tree["nominal_selection"]["neural_result"]["bpm"],
        61.0000615
    );
    assert_eq!(tree["selected"], original["selected"]);
    assert_eq!(tree["raw_beats"], original["raw_beats"]);
    assert_eq!(base, original.to_string());
}
#[test]
fn range_and_unavailable_primaries_do_not_start_a_prior_observation() {
    let profile = profile();
    let shape = geometry(28_800_000, 48_000).unwrap();
    for schema in [4, 6, 7, 8] {
        let mut tree = tree(174.0);
        tree["schema_version"] = json!(schema);
        let base = tree.to_string();
        let range = BpmRange::configured(true, 50.0, 170.0).unwrap();
        assert!(matches!(
            transact(
                &mut tree,
                &base,
                0,
                0,
                0,
                0,
                range,
                shape,
                &profile,
                input(&profile),
                false,
                &|| false,
                |_, _| panic!("range owns this request"),
                |_| panic!("no prior")
            ),
            Err(Failure::NoChoice)
        ));
    }
    let mut tree = tree(61.5);
    let base = tree.to_string();
    assert!(matches!(
        transact(
            &mut tree,
            &base,
            0,
            0,
            0,
            0,
            None,
            shape,
            &profile,
            input(&profile),
            false,
            &|| false,
            |_, _| panic!("midpoint is not a unique class"),
            |_| panic!("no prior")
        ),
        Err(Failure::NoChoice)
    ));
}
#[test]
fn no_choice_and_runtime_failure_keep_the_exact_base_available() {
    let profile = profile();
    let shape = geometry(28_800_000, 48_000).unwrap();
    for failed in [false, true] {
        let mut tree = tree(61.0);
        let base = tree.to_string();
        let exact = base.clone();
        let result = transact(
            &mut tree,
            &base,
            0,
            0,
            0,
            0,
            None,
            shape,
            &profile,
            input(&profile),
            false,
            &|| false,
            |_, primary| Ok(observation(primary, 122.0)),
            |request| {
                if failed {
                    Err(unavailable("injected process refusal"))
                } else {
                    Ok(receipt(&profile, request, shape, 61))
                }
            },
        );
        assert!(if failed {
            matches!(result, Err(Failure::Unavailable(_)))
        } else {
            matches!(result, Err(Failure::NoChoice))
        });
        assert_eq!(base, exact);
        assert_eq!(tree["schema_version"], 4);
    }
    let mean = [1.0 / 256.0; 256];
    assert!(!prefers_qm(&mean, 61, 122));
}
#[test]
fn packed_source_profile_global_and_hash_tampering_is_rejected() {
    let profile = profile();
    let shape = geometry(28_800_000, 48_000).unwrap();
    let request = input(&profile);
    for variant in 0..7 {
        let mut response_value = receipt(&profile, &request, shape, 122);
        match variant {
            0 => response_value["input"]["mono_sha256"] = json!("0".repeat(64)),
            1 => response_value["profile"]["profile_sha256"] = json!("0".repeat(64)),
            2 => response_value["prior"]["profile"]["graph_sha256"] = json!("0".repeat(64)),
            3 => response_value["prior"]["probabilities"]["sha256"] = json!("0".repeat(64)),
            4 => response_value["prior"]["global"]["first_argmax_class"] = json!(61),
            5 => {
                response_value["prior"]["frontend"]["normalization"]["std_f64_le_hex"] =
                    json!(hex(f64::NAN))
            }
            _ => {
                let mut bytes = STANDARD
                    .decode(response_value["prior"]["global"]["data"].as_str().unwrap())
                    .unwrap();
                bytes[(122 - 30) * 8..(123 - 30) * 8]
                    .copy_from_slice(&1.0_f64.next_down().to_le_bytes());
                response_value["prior"]["global"]["data"] = json!(STANDARD.encode(&bytes));
                response_value["prior"]["global"]["sha256"] =
                    json!(format!("{:x}", Sha256::digest(&bytes)));
            }
        }
        assert!(
            response(response_value, &profile, &request, shape, 0, false, &|| {
                false
            })
            .is_err()
        );
    }
}
#[test]
fn cancellation_precedes_resource_refusal_and_late_payload_processing() {
    let profile = profile();
    let shape = geometry(28_800_000, 48_000).unwrap();
    let mut tree = tree(61.0);
    let base = tree.to_string();
    assert!(matches!(
        transact(
            &mut tree,
            &base,
            usize::MAX,
            0,
            0,
            0,
            None,
            shape,
            &profile,
            input(&profile),
            false,
            &|| true,
            |_, _| panic!("canceled"),
            |_| panic!("canceled")
        ),
        Err(Failure::Cancelled)
    ));
    let stale = std::cell::Cell::new(false);
    assert!(matches!(
        transact(
            &mut tree,
            &base,
            0,
            0,
            0,
            0,
            None,
            shape,
            &profile,
            input(&profile),
            false,
            &|| stale.get(),
            |_, primary| Ok(observation(primary, 122.0)),
            |request| {
                stale.set(true);
                Ok(receipt(&profile, request, shape, 122))
            }
        ),
        Err(Failure::Cancelled)
    ));
    assert_eq!(tree["schema_version"], 4);
}

#[test]
fn late_metadata_refusal_and_supersession_never_consume_the_exact_base() {
    let mut tree = tree(61.0);
    let base = tree.to_string();
    let exact = base.clone();
    assert!(matches!(
        complete(
            &mut tree,
            json!({"new":"evidence"}),
            122.0,
            "estimated",
            &base,
            LIMIT - 1,
            0,
            0,
            &|| false
        ),
        Err(Failure::Unavailable(_))
    ));
    assert_eq!(base, exact);
    assert!(matches!(
        complete(
            &mut tree,
            json!({}),
            122.0,
            "estimated",
            &base,
            usize::MAX,
            0,
            0,
            &|| true
        ),
        Err(Failure::Cancelled)
    ));
    assert_eq!(base, exact);
}

#[test]
fn closed_wire_and_execution_origin_refuse_extra_or_null_fields() {
    let profile = profile();
    let shape = geometry(28_800_000, 48_000).unwrap();
    let request = input(&profile);
    let mut good = receipt(&profile, &request, shape, 122);
    assert!(serde_json::from_value::<ResponseWire>(good.clone()).is_ok());
    good["prior"]["execution"]["cached_observation"] = Value::Null;
    assert!(serde_json::from_value::<ResponseWire>(good).is_err());
    let mut good = receipt(&profile, &request, shape, 122);
    good["prior"]["probabilities"]["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ResponseWire>(good).is_err());
}

#[test]
fn unconfigured_profile_skips_before_optional_geometry_or_python_work() {
    let directory = tempfile::tempdir().unwrap();
    let context = NeuralContext {
        installation: directory.path().join("beat-this").to_str().unwrap().into(),
        staging_root: directory.path().to_owned(),
        descriptor: json!({"original":{"sha256":"b".repeat(64)}}),
        identity: PcmIdentity {
            pad_id: 0,
            request_id: 1,
            source_id: "b".repeat(64),
            source_generation: 1,
        },
        cancelled: Arc::new(|| false),
        additional_held_bytes: 0,
    };
    let sample = SampleBuffer {
        residency: None,
        channels: 1,
        samples: Arc::from([0.0_f32].as_slice()),
    };
    let mut tree = tree(61.0);
    let base = tree.to_string();
    let cancellation: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| false);
    assert!(matches!(
        attempt(
            &sample,
            48_000,
            &context,
            "",
            "",
            &directory.path().join("mono.f32le"),
            &mut tree,
            &base,
            usize::MAX,
            0,
            0,
            0,
            None,
            &cancellation
        ),
        Err(Failure::NoChoice)
    ));
    assert_eq!(tree["schema_version"], 4);
}

#[test]
fn captured_range_rejects_an_outside_qm_before_classifier_and_records_inside_choice() {
    let profile = profile();
    let shape = geometry(28_800_000, 48_000).unwrap();
    let mut outside = tree(61.0);
    let base = outside.to_string();
    let range = BpmRange::configured(true, 50.0, 100.0).unwrap();
    assert!(matches!(
        transact(
            &mut outside,
            &base,
            0,
            0,
            0,
            0,
            range,
            shape,
            &profile,
            input(&profile),
            false,
            &|| false,
            |_, primary| Ok(observation(primary, 122.0)),
            |_| panic!("out-of-range QM must not call classifier")
        ),
        Err(Failure::NoChoice)
    ));
    let mut inside = tree(61.0);
    let base = inside.to_string();
    let range = BpmRange::configured(true, 50.0, 170.0).unwrap();
    let result = transact(
        &mut inside,
        &base,
        0,
        0,
        0,
        0,
        range,
        shape,
        &profile,
        input(&profile),
        false,
        &|| false,
        |_, primary| Ok(observation(primary, 122.0)),
        |request| Ok(receipt(&profile, request, shape, 122)),
    )
    .unwrap();
    assert_eq!(result.bpm, 122.0);
    assert_eq!(
        inside["nominal_selection"]["range_constraint"],
        json!({"minimum_bpm":50.0,"maximum_bpm":170.0})
    );
}
