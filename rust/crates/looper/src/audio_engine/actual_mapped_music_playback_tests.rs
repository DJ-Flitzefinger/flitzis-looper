// Real source reads, independent map-coordinate oracle and full convergence history.
// Neither a detector grid nor a final phase ACK establishes musical acceptance.
use crate::audio_engine::input_mapping::{InputRuntime, PitchPadProjection};
use crate::audio_engine::source_map_admission::SourceMapRow;
use crate::audio_engine::source_sync_policy::GlobalTimingPolicy;

fn decoded_bits(value: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
}

// Independent f64 interpolation of the retained immutable JSON, never a Native
// target/cursor query. A gap is unavailable, not a fabricated zero residual.
fn original_beat(record: &Value, seconds: f64) -> Option<f64> {
    let accepted = &record["record"]["accepted"];
    let anchors = accepted["anchors"].as_array()?;
    let next = anchors.partition_point(|a| decoded_bits(&a["source_seconds_bits"]) <= seconds);
    let index = next.saturating_sub(1).min(anchors.len().checked_sub(2)?);
    let a = &anchors[index];
    let b = &anchors[index + 1];
    if !matches!(
        accepted["intervals"][index]["coverage"].as_str()?,
        "trusted" | "estimated"
    ) {
        return None;
    }
    let x0 = decoded_bits(&a["source_seconds_bits"]);
    let x1 = decoded_bits(&b["source_seconds_bits"]);
    // The actual test's saved origins and complete loops are inside coverage.
    if seconds < x0 || seconds > x1 {
        return None;
    }
    let y0 = decoded_bits(&a["beat_bits"]);
    let y1 = decoded_bits(&b["beat_bits"]);
    Some(y0 + (seconds - x0) * (y1 - y0) / (x1 - x0))
}

fn centered(value: f64, period: f64) -> f64 {
    (value + period / 2.0).rem_euclid(period) - period / 2.0
}

fn public_phase_status(path: &Path, id: usize) -> Value {
    Python::attach(|py| {
        let Some(status) = path.engine.pad_source_sync_status(py, id).unwrap() else {
            return Value::Null;
        };
        let wire: String = PyModule::import(py, "json")
            .unwrap()
            .call_method1("dumps", (status,))
            .unwrap()
            .extract()
            .unwrap();
        serde_json::from_str(&wire).unwrap()
    })
}

#[test]
#[ignore = "authorized FLAC and independent short-loop sources; actual 64-bar mapped rendering"]
fn authorized_music_three_pad_mapped_64_bars() {
    const TEST: &str = "audio_engine::cold_load::tests::automatic_analysis_tests::actual_music_loop_tests::mapped_playback::authorized_music_three_pad_mapped_64_bars";
    if std::env::var_os(CHILD).is_none() {
        child(TEST, 48_000, true);
        return;
    }
    Python::initialize();
    let root = PathBuf::from(std::env::var_os("FLITZI_MUSIC_LOOP_ROOT").expect("owned root"));
    let input =
        PathBuf::from(std::env::var_os("FLITZI_MUSIC_LOOP_INPUTS").expect("source fingerprints"));
    let installation = std::env::var("LOOPER_BEAT_THIS_INSTALL_DIR").unwrap();
    assert!(root.is_absolute() && input.is_absolute());
    let inputs: Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    let sources = inputs["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    let (_directory, _cwd) = owned_directory_at(&root.join("tmp"));
    let mut path = Path::new();
    let mut events = Vec::new();
    let mut fingerprints = Vec::new();
    let mut artifacts: Vec<crate::audio_engine::SourceBeatmapArtifact> = Vec::new();
    let mut deliveries = Vec::new();
    let mut records = Vec::new();
    for (id, source_id) in [0, 1, 1].into_iter().enumerate() {
        let (event, decoder, playback) =
            maps::verified_import(&mut path, id, source_id, &sources[source_id]);
        events.push(event); // Each PAD retains its own original source lease/ACK.
        fingerprints.push(
            json!({"pad_id":id,"original_source":source_id,"decoder":decoder,"playback":playback}),
        );
        let restore = (id == 2).then(|| artifacts[1].receipt_json().to_owned());
        let (artifact, delivery) = maps::automatic_map(&mut path, id, &installation, restore);
        assert!(artifact.map().is_pristine_automatic());
        let record: Value = serde_json::from_str(artifact.map().canonical_record()).unwrap();
        assert_eq!(
            record["record"]["source"]["decoder_descriptor"]["pcm"],
            decoder
        );
        fs::write(
            root.join(format!("source-map-pad{id}.json")),
            artifact.map().canonical_record(),
        )
        .unwrap();
        records.push(record);
        artifacts.push(artifact);
        deliveries.push(delivery);
    }
    assert_eq!(
        artifacts[1].map().map_revision(),
        artifacts[2].map().map_revision()
    );
    // The reused cold-load fixture starts a historical synthetic voice. Stop
    // it through the real owner before measuring this three-source cohort.
    path.engine.stop_all().unwrap();
    path.drain();
    path.engine.set_master_bpm(94.0).unwrap();
    path.drain();
    path.engine.set_speed(1.0).unwrap();
    path.drain();
    let quarters = [256.0, 32.0, 32.0];
    let starts: [f64; 3] = std::array::from_fn(|id| {
        (sources[usize::from(id != 0)]["saved_start"]
            .as_f64()
            .unwrap()
            * 48_000.)
            .round()
            / 48_000.
    });
    let origins: [f64; 3] = std::array::from_fn(|id| {
        (sources[usize::from(id != 0)]["grid_origin"]
            .as_f64()
            .unwrap()
            * 48_000.)
            .round()
            / 48_000.
    });
    let ends: [f64; 3] = std::array::from_fn(|id| {
        let interval = artifacts[id]
            .map()
            .canonical_loop_interval(starts[id], quarters[id], false)
            .expect("original map coverage");
        (interval.source_end * 48_000.).round() / 48_000.
    });
    let mut windows = Vec::new();
    for id in 0..3 {
        let window = path
            .engine
            .prepare_resident_control_with_source(
                id,
                Some(starts[id]),
                Some(ends[id]),
                None,
                Some(false),
                Some(path.generation(id)),
            )
            .unwrap();
        wait_until_for(Duration::from_secs(120), || {
            path.drain();
            resident_relocation::reconcile(&path.engine).unwrap();
            match window.publication_status() {
                "accepted" => true,
                "preparing" | "pending" | "adopting" => false,
                other => panic!("actual resident ACK {other}: {:?}", window.error()),
            }
        });
        windows.push(window);
    }
    // Match the selected short PAD's genuine current reference after its
    // resident OwnACK; do not retain a superseded pre-window binding.
    let master_reference = path
        .engine
        .current_input_runtime_pad_binding(1)
        .unwrap()
        .unwrap();
    path.engine
        .set_bpm_lock_with_binding(true, Some(&master_reference))
        .unwrap();
    path.drain();
    let mut bootstrap = Value::Null;
    wait_until_for(Duration::from_secs(30), || {
        path.drain();
        assert!(path.render(1).iter().all(|v| *v == 0.0));
        assert!(path.callback.mixer.voices.iter().all(|v| !v.active));
        bootstrap = Python::attach(|py| {
            let status = path.engine.bpm_lock_status(py).unwrap().unwrap();
            let wire: String = PyModule::import(py, "json")
                .unwrap()
                .call_method1("dumps", (status,))
                .unwrap()
                .extract()
                .unwrap();
            serde_json::from_str(&wire).unwrap()
        });
        fs::write(
            root.join("bpm-bootstrap.json"),
            serde_json::to_vec_pretty(&bootstrap).unwrap(),
        )
        .unwrap();
        bootstrap["state"] == "applied" && bootstrap["effective_enabled"] == true
    });
    let producer = path
        .engine
        .test_control_channels
        .as_ref()
        .unwrap()
        .0
        .clone();
    path.engine.input_runtime = Some(InputRuntime::new_with_ownership(
        producer,
        path.engine.input_clock.clone(),
        path.engine.input_runtime_ownership.clone(),
    ));
    path.engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_render_format(48_000, 2)
        .unwrap();
    let bindings: Vec<_> = (0..NUM_SAMPLES)
        .map(|id| {
            (id < 3).then(|| {
                path.engine
                    .current_input_runtime_pad_binding(id)
                    .unwrap()
                    .unwrap()
            })
        })
        .collect();
    let mut rows = vec![SourceMapRow::default(); NUM_SAMPLES];
    let mut start_rows = vec![0.0; NUM_SAMPLES];
    let mut end_rows = vec![None; NUM_SAMPLES];
    for id in 0..3 {
        start_rows[id] = starts[id];
        end_rows[id] = Some(ends[id]);
        rows[id] = SourceMapRow {
            sync_requested: true,
            intent_revision: 1,
            delivery: Some(Python::attach(|py| deliveries[id].borrow(py).clone())),
            exact_quarters: Some(quarters[id]),
            ordinary_pending: false,
            phase_origin_seconds: Some(origins[id]),
            operational_four_quarter_bars: true,
        };
    }
    path.engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id < 3).collect(),
            start_rows,
            end_rows,
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
            GlobalTimingPolicy::parse(Some(
                r#"{"schema_version":1,"sync":true,"realtime_bpm":true,"revision":1}"#,
            ))
            .unwrap(),
        )
        .unwrap();
    let group = Python::attach(|py| {
        let entries = (0..3)
            .map(|id| {
                (
                    Py::new(
                        py,
                        path.engine
                            .current_input_runtime_pad_binding(id)
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap(),
                    starts[id],
                    Some(ends[id]),
                    None,
                )
            })
            .collect();
        path.engine.capture_global_start(py, entries, None).unwrap()
    });
    let members = group.resolve().unwrap().unwrap();
    let captured: Vec<_> = members.iter().map(|m| m.start.clone()).collect();
    assert!(captured.iter().all(|c| c.mapped_geometry().is_some()));
    path.drain();
    let ticket = path
        .engine
        .publish_captured_global_start(&group)
        .unwrap()
        .unwrap();
    path.drain();
    for start in &captured {
        path.wait_start(start);
    }
    let due = captured
        .iter()
        .map(|c| c.execution_frame().unwrap())
        .max()
        .unwrap();
    while path.callback.transport.output_frame() < due {
        let n = (due - path.callback.transport.output_frame()).min(512) as usize;
        path.render(n);
    }
    path.render(0);
    assert_eq!(group.state(), "applied", "{:?}", group.reason());
    assert_eq!(ticket.publication_status(), "accepted");
    let first_pairs: Vec<_> = path
        .callback
        .mixer
        .voices
        .iter()
        .filter(|v| v.active)
        .map(|v| {
            let pair = v.source_playback.position();
            json!({"pad_id":v.sample_id,"frame":pair.frame,"fraction":pair.fraction})
        })
        .collect();
    fs::write(
        root.join("actual-first-pairs.json"),
        serde_json::to_vec_pretty(&first_pairs).unwrap(),
    )
    .unwrap();
    for pair in &first_pairs {
        let id = pair["pad_id"].as_u64().unwrap() as usize;
        assert_eq!(pair["frame"], (starts[id] * 48_000.).round() as u64);
        assert_eq!(pair["fraction"], 0.0);
    }
    let launch: Vec<_> = captured.iter().map(|c| json!({"T":c.target_frame(),"E":c.execution_frame(),"effective":c.effective_frame()})).collect();
    let frame0 = path.callback.transport.output_frame();
    let master0 = path.callback.transport.beat_position();
    let slot = path.engine.loop_acceptance.diagnostics.register().unwrap();
    let horizon = (256.0_f64 * 60.0 / 94.0 * 48_000.).ceil() as u64;
    let minimum_total = horizon + 512;
    let maximum_total = horizon + (129.0_f64 * 60.0 / 94.0 * 48_000.).ceil() as u64 + 512;
    let mut long_seam_end = None;
    let mut long_seam = Value::Null;
    let g_beats: [f64; 3] = std::array::from_fn(|id| {
        original_beat(&records[id], origins[id]).expect("original g coverage")
    });
    let generations: [u64; 3] = std::array::from_fn(|id| path.generation(id));
    let mut initial = None;
    let mut checkpoints = Vec::new();
    let mut next_checkpoint = 0.0;
    let mut settled_at = [None; 3];
    let mut public_confirmed_at = [None; 3];
    let mut max_full_debt = [0.0_f64; 3];
    let mut max_fractional = [0.0_f64; 3];
    let mut max_confirmed_debt = [0.0_f64; 3];
    let mut max_confirmed_beat_debt = [0.0_f64; 3];
    let mut last_pair: [(u64, u64); 3] = [(0, 0); 3];
    let mut last_full = [0.0; 3];
    let mut wraps = [0; 3];
    let mut max_seam_change = [0.0_f64; 3];
    let mut first_relative = None;
    let mut first_detected_phase = None;
    let mut max_pair_excursion = 0.0_f64;
    let mut energy = 0.0;
    let mut control_polls = 0;
    let mut first_phase_claim = Value::Null;
    let mut control_observations = Vec::new();
    loop {
        let elapsed = path.callback.transport.output_frame() - frame0;
        if elapsed >= maximum_total
            || (elapsed >= minimum_total && long_seam_end.is_some_and(|end| elapsed >= end + 512))
        {
            break;
        }
        // The real App services the same Native request on unchanged control
        // ticks. A mode-only birth needs an actual read before its PHASE claim.
        if elapsed >= 512 && (elapsed / 512) % 2 == 1 {
            let before: Vec<_> = path
                .callback
                .mixer
                .voices
                .iter()
                .filter(|v| v.active)
                .map(|v| (v.sample_id, v.generation, v.source_playback.position()))
                .collect();
            path.engine
                .request_input_runtime_policy_adoption(true)
                .unwrap();
            path.drain();
            control_polls += 1;
            for (id, generation, pair) in &before {
                let voice = path
                    .callback
                    .mixer
                    .voices
                    .iter()
                    .find(|v| v.active && v.sample_id == *id)
                    .unwrap();
                let after = voice.source_playback.position();
                assert_eq!(voice.generation, *generation);
                assert_eq!(
                    (after.frame, after.fraction.to_bits()),
                    (pair.frame, pair.fraction.to_bits()),
                    "PHASE claim may not seek the source"
                );
            }
            let installed = path
                .callback
                .mixer
                .voices
                .iter()
                .filter(|v| v.active)
                .all(|v| v.source_playback.is_mapped_phase());
            if installed && first_phase_claim.is_null() {
                first_phase_claim = json!({"output_frame":path.callback.transport.output_frame(),"master_beat":path.callback.transport.beat_position(),"control_poll":control_polls,"scope":"actual unchanged source pairs at the control claim"});
            }
            if control_polls <= 4
                || (installed
                    && control_observations
                        .last()
                        .is_none_or(|v: &Value| v["phase_installed"] != true))
            {
                control_observations.push(json!({"output_frame":path.callback.transport.output_frame(),"phase_installed":installed,"status":public_phase_status(&path,0)}));
            }
        }
        let boundary = if elapsed < horizon {
            horizon
        } else if elapsed < minimum_total {
            minimum_total
        } else {
            maximum_total
        };
        let frames = (boundary - elapsed).min(512) as usize;
        let master_before = path.callback.transport.beat_position();
        let request = path.engine.request_sync_diagnostic_snapshot().unwrap();
        path.callback.mixer.begin_sync_diagnostic_capture(request);
        let output = path.render(frames);
        assert!(output.iter().all(|v| v.is_finite()));
        energy += output.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        slot.observe(
            &path.callback.mixer,
            &path.callback.transport,
            path.engine.input_clock.capture_ns(),
            frames,
        );
        let snapshot = path
            .callback
            .mixer
            .sync_diagnostic_snapshot(
                request,
                slot.birth,
                &path.callback.transport,
                path.engine.input_clock.capture_ns(),
                frames,
            )
            .unwrap();
        let words = &snapshot.0;
        assert_eq!(
            words[11], 3,
            "all three independent real sources must remain active"
        );
        assert_eq!(words[3], path.callback.transport.output_frame());
        let master = f64::from_bits(words[7]);
        assert!(master.is_finite());
        assert!((f64::from_bits(words[8]) - 60.0 / 94.0).abs() < 1e-12);
        let mut seen = [false; 3];
        let mut full = [0.0; 3];
        let mut fractional = [0.0; 3];
        let mut beats = [0.0; 3];
        let mut new_witness = false;
        let mut pairs = [(0, 0); 3];
        let mut rows_json = Vec::new();
        for row in words[HEADER_WORDS..].chunks_exact(ROW_WORDS).take(3) {
            let id = row[0] as usize;
            assert!(id < 3 && !seen[id]);
            seen[id] = true;
            assert_eq!(row[4], generations[id]);
            assert_eq!(
                row[40],
                fingerprints[id]["playback"]["full_frames"]
                    .as_u64()
                    .unwrap()
            );
            assert_eq!(row[41], 48_000);
            assert_eq!(row[17], 3, "requested SYNC must have actual Mapped audio");
            assert_ne!(row[16] & (1 << 7), 0, "actual complete read required");
            assert_ne!(
                row[16] & (1 << 10),
                0,
                "actual mapped source clock required"
            );
            assert_eq!(row[16] & ((1 << 1) | (1 << 12)), 0, "dry KL-off path");
            assert_eq!(row[33], 1, "dry audible source domain");
            assert_eq!(
                row[30], words[3],
                "source and master must share the endpoint"
            );
            pairs[id] = (row[31], row[32]);
            let seconds = (row[31] as f64 + f64::from_bits(row[32])) / 48_000.;
            let beat = original_beat(&records[id], seconds)
                .expect("actual read crossed unavailable original map coverage");
            let debt = beat - g_beats[id] - master;
            beats[id] = beat;
            full[id] = centered(debt, quarters[id]);
            fractional[id] = centered(debt, 1.0);
            max_full_debt[id] = max_full_debt[id].max(full[id].abs());
            max_fractional[id] = max_fractional[id].max(fractional[id].abs());
            let confirmed = row[16] & (1 << 3) != 0;
            if confirmed {
                new_witness |= settled_at[id].is_none();
                settled_at[id].get_or_insert(master);
                max_confirmed_debt[id] = max_confirmed_debt[id].max(full[id].abs());
                max_confirmed_beat_debt[id] = max_confirmed_beat_debt[id].max(fractional[id].abs());
            }
            if initial.is_some() && pairs[id].0 < last_pair[id].0 {
                wraps[id] += 1;
                if id == 0 && long_seam_end.is_none() {
                    long_seam_end = Some(elapsed + frames as u64);
                    long_seam = json!({"output_frame_bracket":[frame0+elapsed,words[3]],"master_beat_bracket":[master_before,master],"observed_endpoint_seconds_since_E":(elapsed+frames as u64)as f64/48_000.,"precision":"wrap observed within this callback chunk, not exact seam sample"});
                }
                max_seam_change[id] =
                    max_seam_change[id].max(centered(full[id] - last_full[id], quarters[id]).abs());
            }
            rows_json.push(json!({"pad":id,"source_frame":row[31],"source_fraction":f64::from_bits(row[32]),"full_loop_residual":full[id],"beat_residual":fractional[id],"whole_quarters":full[id]-fractional[id],"applied_rate":f64::from_bits(row[14]),"target_rate":f64::from_bits(row[15]),"phase_program_installed":row[16]&(1<<4)!=0,"phase_read_current":confirmed}));
        }
        assert_eq!(
            pairs[1], pairs[2],
            "independent identical short PADs interfered"
        );
        let relative = centered(fractional[0] - fractional[1], 1.0);
        let detected_phase = centered(beats[0] - beats[1], 1.0);
        first_detected_phase.get_or_insert(detected_phase);
        let first = *first_relative.get_or_insert(relative);
        max_pair_excursion = max_pair_excursion.max(centered(relative - first, 1.0).abs());
        last_pair = pairs;
        last_full = full;
        if initial.is_none() {
            initial = Some(
                json!({"output_frame":words[3],"master_beat":master,"g_rebased_pair_phase":relative,"detected_pair_phase":detected_phase,"rows":rows_json}),
            );
        }
        if new_witness
            || master - master0.unwrap() >= next_checkpoint
            || elapsed + frames as u64 == horizon
            || elapsed + frames as u64 == minimum_total
        {
            let public: Vec<_> = (0..3)
                .map(|id| {
                    let status = public_phase_status(&path, id);
                    if status["target_current"] == true
                        && status["phase_program_current"] == true
                        && status["phase_read_confirmed"] == true
                    {
                        assert_eq!(status["source_generation"], generations[id]);
                        assert_eq!(status["policy_mode"], "mapped");
                        assert_eq!(status["policy_revision"], 1);
                        assert_eq!(status["intent_revision"], 1);
                        assert_eq!(status["map_revision"], artifacts[id].map().map_revision());
                        public_confirmed_at[id].get_or_insert(master);
                    }
                    status
                })
                .collect();
            checkpoints.push(json!({"output_frame":words[3],"master_beat":master,"g_rebased_pair_phase":relative,"detected_pair_phase":detected_phase,"rows":rows_json,"public_current_status":public}));
            next_checkpoint += 1.0;
        }
    }
    let total = path.callback.transport.output_frame() - frame0;
    let final_public: Vec<_> = (0..3).map(|id| public_phase_status(&path, id)).collect();
    path.engine.stop_all().unwrap();
    path.drain();
    path.engine.shut_down().unwrap();
    for artifact in &artifacts {
        artifact.retire_unselected();
    }
    for source in sources {
        assert_eq!(
            hash_file(std::path::Path::new(source["path"].as_str().unwrap())),
            source["source_sha256"]
        );
    }
    let report = json!({"scope":"Real three-PAD mapped dry source reads over 64 master bars, not device or listening acceptance","complete":true,"master_bpm":94,"master_reference_pad":1,"master_reference_source_generation":master_reference.source_generation,"public_bpm_bootstrap":bootstrap,"nominal_64_bar_frames":horizon,"minimum_frames_with_tail":minimum_total,"rendered_frames":total,"rendered_master_quarters":total as f64/48_000.*94./60.,"actual_first_long_wrap":long_seam,"source_fingerprints":fingerprints,"quarters":quarters,"starts":starts,"ends":ends,"saved_grid_origins":origins,"grid_origin_beats":g_beats,"launch":launch,"actual_first_pairs":first_pairs,"start_output_frame":frame0,"start_master_beat":master0,"initial":initial,"control_polls":control_polls,"control_observations":control_observations,"first_phase_claim":first_phase_claim,"first_callback_read_witness_master_beat":settled_at,"first_public_phase_confirmed_master_beat":public_confirmed_at,"final_public_status":final_public,"max_full_loop_debt":max_full_debt,"max_fractional_beat_debt":max_fractional,"max_callback_witness_full_loop_debt":max_confirmed_debt,"max_callback_witness_fractional_beat_debt":max_confirmed_beat_debt,"max_pair_beat_excursion_from_initial":max_pair_excursion,"loop_wraps":wraps,"max_seam_residual_change":max_seam_change,"energy":energy,"checkpoints":checkpoints});
    fs::write(
        root.join("actual-mapped-playback.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(energy > 0.0);
    assert!(
        first_detected_phase.unwrap().abs() < 0.05,
        "this counterexample must start with aligned detected quarter phases; retain unchanged inputs"
    );
    assert!(
        long_seam_end.is_some(),
        "observe the actual long source seam, not just the nominal master horizon"
    );
    assert!(
        public_confirmed_at.iter().all(Option::is_some),
        "actual public current source/policy/read feedback must confirm independently"
    );
    assert!(
        settled_at.iter().all(Option::is_some),
        "all actual programmes must obtain a current read by the bounded rendered endpoint"
    );
    assert!(
        max_confirmed_beat_debt.iter().all(|e| *e < 0.0001),
        "a settled callback read witness must match beat phase; integer phrase offsets remain valid"
    );
    assert!(
        max_pair_excursion < 0.05,
        "SYNC must not introduce a large new beat-phase excursion; retain full negative history before repair"
    );
}
