// Opt-in reproduction of the fresh Human scalar-to-SYNC transition.
// Saved observations are data; only restored Native custody admits a map.

struct ToggleLaunch {
    _group: crate::audio_engine::CapturedGlobalStart,
    _ticket: crate::audio_engine::GlobalPlaybackBatchTicket,
    starts: Vec<CapturedPadStart>,
    report: Value,
}

fn toggle_resident_windows(
    path: &mut Path,
    starts: &[f64; 2],
    ends: &[f64; 2],
) -> Vec<crate::audio_engine::resident_relocation::ResidentWindowTicket> {
    let mut windows = Vec::new();
    for id in 0..2 {
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
                other => panic!(
                    "scalar/mapped resident OwnACK {other}: {:?}",
                    window.error()
                ),
            }
        });
        windows.push(window);
    }
    windows
}

fn toggle_publish(
    path: &Path,
    starts: &[f64; 2],
    ends: &[f64; 2],
    origins: &[f64; 2],
    quarters: &[f64; 2],
    deliveries: &[Py<crate::audio_engine::loader_events::LoaderEventDelivery>],
    enabled: bool,
    revision: u64,
) {
    let bindings: Vec<_> = (0..NUM_SAMPLES)
        .map(|id| {
            (id < 2).then(|| {
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
    for id in 0..2 {
        start_rows[id] = starts[id];
        end_rows[id] = Some(ends[id]);
        if enabled {
            rows[id] = SourceMapRow {
                // Effective global resolution of the saved false/revision-zero
                // preference; the saved PAD intent is never edited.
                sync_requested: true,
                intent_revision: 0,
                delivery: Some(Python::attach(|py| deliveries[id].borrow(py).clone())),
                exact_quarters: Some(quarters[id]),
                ordinary_pending: false,
                phase_origin_seconds: Some(origins[id]),
                operational_four_quarter_bars: true,
            };
        }
    }
    path.engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_runtime_state_with_policy(
            true,
            (0..NUM_SAMPLES).map(|id| id < 2).collect(),
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
                &json!({"schema_version":1,"sync":enabled,"realtime_bpm":true,"revision":revision})
                    .to_string(),
            ))
            .unwrap(),
        )
        .unwrap();
}

fn toggle_voice_pairs(path: &Path) -> Vec<(usize, u64, usize, u64)> {
    path.callback
        .mixer
        .voices
        .iter()
        .filter(|voice| voice.active)
        .map(|voice| {
            let point = voice.source_playback.position();
            (
                voice.sample_id,
                voice.generation,
                point.frame,
                point.fraction.to_bits(),
            )
        })
        .collect()
}

fn toggle_residency(path: &Path) -> Vec<Value> {
    Python::attach(|py| {
        (0..2)
            .map(|id| match path.engine.loaded_residency(py, id) {
                Ok(value) => {
                    let encoded: String = PyModule::import(py, "json")
                        .unwrap()
                        .call_method1("dumps", (value,))
                        .unwrap()
                        .extract()
                        .unwrap();
                    serde_json::from_str(&encoded).unwrap()
                }
                Err(error) => json!({"pad_id":id,"unavailable":error.to_string()}),
            })
            .collect()
    })
}

fn toggle_start(path: &mut Path, starts: &[f64; 2], ends: &[f64; 2]) -> ToggleLaunch {
    let group = Python::attach(|py| {
        let entries = (0..2)
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
    let captured: Vec<_> = group
        .resolve()
        .unwrap()
        .unwrap()
        .iter()
        .map(|member| member.start.clone())
        .collect();
    let mapped_births = captured
        .iter()
        .filter(|start| start.mapped_geometry().is_some())
        .count();
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
        .map(|start| start.execution_frame().unwrap())
        .max()
        .unwrap();
    while path.callback.transport.output_frame() < due {
        let frames = (due - path.callback.transport.output_frame()).min(512) as usize;
        path.render(frames);
    }
    path.render(0);
    let pairs = toggle_voice_pairs(path);
    let starts_preserved = pairs.len() == 2
        && pairs.iter().all(|(id, _, frame, bits)| {
            *id < 2 && *frame == (starts[*id] * 48_000.).round() as usize && *bits == 0
        });
    let report = json!({
        "state":group.state(),"reason":group.reason(),
        "ticket_status":ticket.publication_status(),"mapped_births":mapped_births,
        "output_frame":path.callback.transport.output_frame(),
        "master_beat":path.callback.transport.beat_position(),
        "members":captured.iter().map(|start|json!({
            "T":start.original_frame(),"E":start.execution_frame(),
            "effective":start.effective_frame(),"state":start.state(),"reason":start.reason()
        })).collect::<Vec<_>>(),
        "initial_pairs":pairs,"physical_starts_preserved":starts_preserved
    });
    ToggleLaunch {
        _group: group,
        _ticket: ticket,
        starts: captured,
        report,
    }
}

fn toggle_measure(
    path: &mut Path,
    records: &[Value],
    origins: &[f64; 2],
    quarters: &[f64; 2],
    beats_to_render: f64,
    service_policy: bool,
    reference_natural_phase: Option<f64>,
) -> Value {
    let first_frame = path.callback.transport.output_frame();
    let first_master = path.callback.transport.beat_position().unwrap();
    let frames_per_beat = path
        .callback
        .transport
        .beat_projection()
        .unwrap()
        .frames_per_beat();
    let horizon = (beats_to_render * frames_per_beat).ceil() as u64;
    let g_beats: [f64; 2] =
        std::array::from_fn(|id| original_beat(&records[id], origins[id]).unwrap());
    let source_generations: [u64; 2] = std::array::from_fn(|id| path.generation(id));
    let slot = path.engine.loop_acceptance.diagnostics.register().unwrap();
    let mut checkpoints = Vec::new();
    let mut controls = Vec::new();
    let mut next_checkpoint = first_master;
    let mut first_natural = None;
    let mut last_natural = None;
    let mut maximum_excursion = 0.0_f64;
    let mut continuous_claims = true;
    let mut complete_reads = true;
    let mut finite_output = true;
    let mut energy = 0.0;
    let mut poll_count = 0;
    while path.callback.transport.output_frame() - first_frame < horizon {
        let elapsed = path.callback.transport.output_frame() - first_frame;
        if service_policy && (elapsed / 512) % 2 == 0 {
            let before = toggle_voice_pairs(path);
            let clock = path.callback.transport;
            path.engine
                .request_input_runtime_policy_adoption(true)
                .unwrap();
            path.drain();
            path.render(0);
            continuous_claims &= before == toggle_voice_pairs(path)
                && path.callback.transport.matches_clock_epoch(&clock)
                && path.callback.transport.output_frame() == clock.output_frame();
            poll_count += 1;
            if poll_count <= 4 || elapsed / 512 % 64 == 0 {
                controls.push(json!({
                    "output_frame":path.callback.transport.output_frame(),
                    "master_beat":path.callback.transport.beat_position(),
                    "source_pairs":toggle_voice_pairs(path),
                    "status":(0..2).map(|id|public_phase_status(path,id)).collect::<Vec<_>>()
                }));
            }
        }
        let frames = (horizon - elapsed).min(512) as usize;
        let request = path.engine.request_sync_diagnostic_snapshot().unwrap();
        path.callback.mixer.begin_sync_diagnostic_capture(request);
        let output = path.render(frames);
        finite_output &= output.iter().all(|value| value.is_finite());
        energy += output
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>();
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
        let master = f64::from_bits(words[7]);
        let mut beats = [None; 2];
        let mut rows = Vec::new();
        let mut seen = [false; 2];
        complete_reads &= words[11] == 2
            && words[3] == path.callback.transport.output_frame()
            && master.is_finite();
        for row in words[HEADER_WORDS..]
            .chunks_exact(ROW_WORDS)
            .take(words[11] as usize)
        {
            let id = row[0] as usize;
            if id >= 2 || seen[id] {
                complete_reads = false;
                continue;
            }
            seen[id] = true;
            let actual_read = row[16] & (1 << 7) != 0
                && row[33] == 1
                // The diagnostic retains the final complete mixed subchunk.
                // Mapped playback may split a callback at a source-span end.
                && row[29] >= words[3] - frames as u64
                && row[29] < row[30]
                && row[30] == words[3]
                && row[4] == source_generations[id]
                && row[16] & ((1 << 1) | (1 << 12)) == 0;
            complete_reads &= actual_read;
            let source_seconds = (row[31] as f64 + f64::from_bits(row[32])) / 48_000.;
            beats[id] = actual_read
                .then(|| original_beat(&records[id], source_seconds))
                .flatten();
            complete_reads &= beats[id].is_some();
            rows.push(json!({
                "pad_id":id,"source_generation":row[4],"content_lifetime":row[5],
                "window_revision":row[6],"policy_mode_code":row[17],
                "next_source_frame":row[31],"next_source_fraction":f64::from_bits(row[32]),
                "actual_read_output_start":row[29],"actual_read_output_end":row[30],
                "physical_A":row[34],"physical_B":row[35],
                "logical_A":row[10],"logical_B":row[11],
                "actual_dry_read":actual_read,"phase_program_installed":row[16]&(1<<4)!=0,
                "callback_read_witness":row[16]&(1<<3)!=0,
                "literal_map_beat":beats[id],"g_map_beat":g_beats[id],
                "literal_quarter_phase":beats[id].map(|b|centered(b-master,1.)),
                "g_relative_quarter_phase":beats[id].map(|b|centered(b-g_beats[id]-master,1.)),
                "g_relative_loop_phase":beats[id].map(|b|centered(b-g_beats[id]-master,quarters[id]))
            }));
        }
        complete_reads &= seen.iter().all(|value| *value);
        let natural = beats[0].zip(beats[1]).map(|(a, b)| centered(a - b, 1.));
        let rebased = beats[0]
            .zip(beats[1])
            .map(|(a, b)| centered((a - g_beats[0]) - (b - g_beats[1]), 1.));
        if let Some(natural) = natural {
            let initial = *first_natural.get_or_insert(natural);
            maximum_excursion = maximum_excursion
                .max(centered(natural - reference_natural_phase.unwrap_or(initial), 1.).abs());
            last_natural = Some(natural);
        }
        if master >= next_checkpoint || elapsed + frames as u64 == horizon {
            checkpoints.push(json!({
                "output_frame":words[3],"master_beat":master,
                "literal_pair_phase":natural,"g_relative_pair_phase":rebased,"voices":rows,
                "actual":snapshot_json(path,request),
                "public_status":(0..2).map(|id|public_phase_status(path,id)).collect::<Vec<_>>()
            }));
            next_checkpoint = master + 1.;
        }
        path.callback.mixer.end_sync_diagnostic_capture();
    }
    let status: Vec<_> = (0..2).map(|id| public_phase_status(path, id)).collect();
    let healthy_mapped = status.iter().all(|value| {
        value["policy_mode"] == "mapped"
            && value["policy_revision"] == 11
            && value["intent_revision"] == 0
            && value["target_current"] == true
            && value["phase_program_current"] == true
            && value["phase_read_confirmed"] == true
    });
    json!({
        "start_frame":first_frame,"end_frame":path.callback.transport.output_frame(),
        "start_master":first_master,"end_master":path.callback.transport.beat_position(),
        "requested_master_quarters":beats_to_render,"frames_per_beat":frames_per_beat,
        "policy_serviced":service_policy,"control_polls":poll_count,
        "continuous_control_claims":continuous_claims,"current_dry_endpoint_reads":complete_reads,
        "finite_output":finite_output,"output_energy":energy,
        "first_literal_pair_phase":first_natural,"last_literal_pair_phase":last_natural,
        "reference_literal_pair_phase":reference_natural_phase,
        "max_literal_pair_excursion_quarters":maximum_excursion,
        "settled_target_literal_pair_phase":centered(g_beats[0]-g_beats[1],1.),
        "healthy_mapped_adoption":healthy_mapped,"final_public_status":status,
        "final_residency":toggle_residency(path),"control_observations":controls,
        "checkpoints":checkpoints
    })
}

fn toggle_reference_case(root: &std::path::Path, inputs: &Value, reference: usize) -> Value {
    let sources = inputs["sources"].as_array().unwrap();
    let (_directory, _cwd) = owned_directory_at(&root.join("tmp"));
    let mut path = Path::new();
    let mut events = Vec::new();
    let mut fingerprints = Vec::new();
    let mut artifacts = Vec::new();
    let mut deliveries = Vec::new();
    let mut records = Vec::new();
    for (id, source) in sources.iter().enumerate() {
        // The Legacy publication and explicit SYNC target are distinct inputs.
        let mut legacy_source = source.clone();
        legacy_source["grid_origin"] = source["legacy_origin"].clone();
        let (event, decoder, playback) = maps::verified_import(&mut path, id, id, &legacy_source);
        events.push(event);
        fingerprints.push(json!({"pad_id":id,"source_sha256":source["source_sha256"],
            "decoder":decoder,"playback":playback}));
        let receipt = &source["selected_receipt"];
        let snapshot = PathBuf::from(source["map_snapshot"].as_str().unwrap());
        assert!(snapshot.is_absolute());
        assert_eq!(hash_file(&snapshot), receipt["sha256"]);
        assert_eq!(
            fs::metadata(&snapshot).unwrap().len(),
            receipt["bytes"].as_u64().unwrap()
        );
        let relative = std::path::Path::new(receipt["reference"].as_str().unwrap());
        assert!(!relative.is_absolute());
        assert!(
            relative
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        );
        let destination = std::env::current_dir().unwrap().join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(&snapshot, &destination).unwrap();
        assert_eq!(hash_file(&destination), receipt["sha256"]);
        let (artifact, delivery) =
            maps::automatic_map(&mut path, id, "", Some(receipt.to_string()));
        assert!(artifact.map().is_pristine_automatic());
        assert_eq!(
            artifact.map().map_revision(),
            receipt["map_revision"].as_str().unwrap()
        );
        let record: Value = serde_json::from_str(artifact.map().canonical_record()).unwrap();
        assert_eq!(
            record["record"]["source"]["decoder_descriptor"]["pcm"],
            decoder
        );
        artifacts.push(artifact);
        deliveries.push(delivery);
        records.push(record);
    }
    path.engine.stop_all().unwrap();
    path.drain();
    let starts: [f64; 2] = std::array::from_fn(|id| sources[id]["saved_start"].as_f64().unwrap());
    let scalar_ends: [f64; 2] =
        std::array::from_fn(|id| sources[id]["scalar_end"].as_f64().unwrap());
    let origins: [f64; 2] = std::array::from_fn(|id| sources[id]["grid_origin"].as_f64().unwrap());
    let quarters: [f64; 2] = std::array::from_fn(|id| sources[id]["quarters"].as_f64().unwrap());
    let mapped_ends: [f64; 2] = std::array::from_fn(|id| {
        let interval = artifacts[id]
            .map()
            .canonical_loop_interval(starts[id], quarters[id], false)
            .unwrap();
        (interval.source_end * 48_000.).round() / 48_000.
    });
    let scalar_windows = toggle_resident_windows(&mut path, &starts, &scalar_ends);
    let master_reference = path
        .engine
        .current_input_runtime_pad_binding(reference)
        .unwrap()
        .unwrap();
    path.engine
        .set_bpm_lock_with_binding(true, Some(&master_reference))
        .unwrap();
    path.drain();
    let mut bootstrap = Value::Null;
    wait_until_for(Duration::from_secs(30), || {
        path.drain();
        path.render(1);
        bootstrap = Python::attach(|py| {
            let status = path.engine.bpm_lock_status(py).unwrap().unwrap();
            let encoded: String = PyModule::import(py, "json")
                .unwrap()
                .call_method1("dumps", (status,))
                .unwrap()
                .extract()
                .unwrap();
            serde_json::from_str(&encoded).unwrap()
        });
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
        path.engine.input_clock,
        path.engine.input_runtime_ownership.clone(),
    ));
    path.engine
        .input_runtime
        .as_ref()
        .unwrap()
        .set_render_format(48_000, 2)
        .unwrap();
    toggle_publish(
        &path,
        &starts,
        &scalar_ends,
        &origins,
        &quarters,
        &deliveries,
        false,
        10,
    );
    let off_launch = toggle_start(&mut path, &starts, &scalar_ends);
    let original_launch_frames: Vec<_> = off_launch
        .starts
        .iter()
        .map(|start| {
            (
                start.original_frame(),
                start.execution_frame(),
                start.effective_frame(),
            )
        })
        .collect();
    let baseline = toggle_measure(&mut path, &records, &origins, &quarters, 8., false, None);
    let reference_phase = baseline["last_literal_pair_phase"].as_f64();
    let before_on_pairs = toggle_voice_pairs(&path);
    let before_on_clock = path.callback.transport;
    let before_on = json!({"pairs":toggle_voice_pairs(&path),
        "residency":toggle_residency(&path),"master":path.callback.transport.beat_position()});
    // Exactly the live request: prospective rows, no mapped resident preparation.
    toggle_publish(
        &path,
        &starts,
        &mapped_ends,
        &origins,
        &quarters,
        &deliveries,
        true,
        11,
    );
    let after_publication = json!({"pairs":toggle_voice_pairs(&path),
        "residency":toggle_residency(&path),"master":path.callback.transport.beat_position()});
    let on_publication_continuous = before_on_pairs == toggle_voice_pairs(&path)
        && path
            .callback
            .transport
            .matches_clock_epoch(&before_on_clock)
        && path.callback.transport.output_frame() == before_on_clock.output_frame();
    let live = toggle_measure(
        &mut path,
        &records,
        &origins,
        &quarters,
        32.,
        true,
        reference_phase,
    );
    let original_launch_retained =
        off_launch
            .starts
            .iter()
            .zip(&original_launch_frames)
            .all(|(start, frames)| {
                start.state() == "applied"
                    && (
                        start.original_frame(),
                        start.execution_frame(),
                        start.effective_frame(),
                    ) == *frames
            });
    // Preserve the causal live result even if separate restart preparation fails.
    fs::write(
        root.join(format!("fresh-live-on-reference-{reference}.json")),
        serde_json::to_vec_pretty(&json!({"master_reference_pad":reference,
            "restart_not_yet_run":true,"off_launch":off_launch.report,
            "baseline":baseline,"before_live_on":before_on,
            "after_on_publication_before_service":after_publication,
            "publication_continuous":on_publication_continuous,"live_on":live}))
        .unwrap(),
    )
    .unwrap();
    path.engine.stop_all().unwrap();
    path.drain();
    // Separate restart experiment; it cannot repair or certify the live request.
    let mapped_windows = toggle_resident_windows(&mut path, &starts, &mapped_ends);
    toggle_publish(
        &path,
        &starts,
        &mapped_ends,
        &origins,
        &quarters,
        &deliveries,
        true,
        11,
    );
    let restart_launch = toggle_start(&mut path, &starts, &mapped_ends);
    let restart = toggle_measure(
        &mut path,
        &records,
        &origins,
        &quarters,
        32.,
        true,
        reference_phase,
    );
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
    let report = json!({
        "scope":"Fresh saved cuts: live OFF-to-ON and separate restart; no device or listening acceptance",
        "master_reference_pad":reference,"actual_human_master_choice":false,
        "saved_pad_preferences":[{"sync":false,"revision":0},{"sync":false,"revision":0}],
        "effective_global_policy_revisions":{"off":10,"on":11},
        "public_bpm_bootstrap":bootstrap,"fingerprints":fingerprints,
        "starts":starts,"legacy_origins":sources.iter().map(|s|s["legacy_origin"].clone()).collect::<Vec<_>>(),
        "runtime_grid_origins":origins,"quarters":quarters,
        "scalar_ends":scalar_ends,"prospective_mapped_ends":mapped_ends,
        "scalar_resident_ticket_count":scalar_windows.len(),"off_launch":off_launch.report,
        "baseline":baseline,"before_live_on":before_on,"after_on_publication_before_service":after_publication,
        "on_publication_continuous":on_publication_continuous,
        "mapped_resident_preparation_before_live_on":false,"live_on":live,
        "original_off_launch_still_applied":original_launch_retained,
        "restart_is_separate_control":true,"mapped_restart_ticket_count":mapped_windows.len(),
        "restart_launch":restart_launch.report,"restart":restart
    });
    fs::write(
        root.join(format!("fresh-toggle-reference-{reference}.json")),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    report
}

#[test]
#[ignore = "authorized fresh Human originals and saved automatic maps; genuine live SYNC diagnostic"]
fn authorized_music_fresh_live_sync_toggle() {
    const TEST: &str = "audio_engine::cold_load::tests::automatic_analysis_tests::actual_music_loop_tests::mapped_playback::authorized_music_fresh_live_sync_toggle";
    if std::env::var_os(CHILD).is_none() {
        child(TEST, 48_000, true);
        return;
    }
    Python::initialize();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let block = workspace.join("test-artifacts/sync-toggle-human-fresh-20261027");
    let input = std::env::var_os("FLITZI_SYNC_TOGGLE_INPUTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| block.join("input/driver-inputs.json"));
    let root = std::env::var_os("FLITZI_SYNC_TOGGLE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| block.join("output"));
    assert!(input.is_absolute() && root.is_absolute());
    fs::create_dir_all(root.join("tmp")).unwrap();
    let input_bytes = fs::read(&input).unwrap();
    let inputs: Value = serde_json::from_slice(&input_bytes).unwrap();
    assert_eq!(inputs["sources"].as_array().unwrap().len(), 2);
    assert_eq!(inputs["master_reference_cases"], json!([0, 1]));
    assert_eq!(inputs["nn_rerun"], false);
    assert_eq!(inputs["rate_hz"], 48_000);
    assert_eq!(inputs["off_policy_revision"], 10);
    assert_eq!(inputs["on_policy_revision"], 11);
    let reports: Vec<_> = (0..2)
        .map(|reference| toggle_reference_case(&root, &inputs, reference))
        .collect();
    let report = json!({
        "schema":"fresh-live-sync-toggle-v1","complete":true,
        "inputs_sha256":format!("{:x}",Sha256::digest(&input_bytes)),
        "runtime":process::runtime_identity().unwrap(),"cases":reports,
        "health_gate_scope":"Native live adoption and no new >.05-quarter excursion; not Human hearing acceptance"
    });
    let output = root.join("actual-fresh-live-toggle.json");
    fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("ACTUAL_FRESH_LIVE_SYNC_TOGGLE {}", output.display());
    // All diagnostic branches and full reports precede these expected health gates.
    for case in report["cases"].as_array().unwrap() {
        assert_eq!(case["off_launch"]["mapped_births"], 0);
        assert_eq!(case["off_launch"]["physical_starts_preserved"], true);
        assert_eq!(case["baseline"]["current_dry_endpoint_reads"], true);
        assert_eq!(case["baseline"]["finite_output"], true);
        assert!(case["baseline"]["output_energy"].as_f64().unwrap() > 0.);
        assert_eq!(case["on_publication_continuous"], true);
        assert_eq!(case["live_on"]["continuous_control_claims"], true);
        assert_eq!(case["original_off_launch_still_applied"], true);
        assert_eq!(case["live_on"]["current_dry_endpoint_reads"], true);
        assert_eq!(case["live_on"]["finite_output"], true);
        assert!(case["live_on"]["output_energy"].as_f64().unwrap() > 0.);
        assert_eq!(case["restart_launch"]["physical_starts_preserved"], true);
        assert_eq!(case["restart"]["current_dry_endpoint_reads"], true);
        assert_eq!(case["restart"]["finite_output"], true);
        assert!(case["restart"]["output_energy"].as_f64().unwrap() > 0.);
        assert_eq!(
            case["live_on"]["healthy_mapped_adoption"], true,
            "genuine live request failed; inspect retained report before considering restart"
        );
        assert!(
            case["live_on"]["max_literal_pair_excursion_quarters"]
                .as_f64()
                .unwrap()
                <= 0.05,
            "live SYNC created a large new detected beat-phase excursion; retain exact cuts/grid"
        );
    }
}
