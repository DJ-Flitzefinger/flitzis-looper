use super::*;

#[test]
fn joint_header_preserves_the_actual_original_fallback_version() {
    for (version,policy) in [(5,"neural-inferred-ordinals-v5"),(9,"neural-inferred-periods-v9")] {
        let base=json!({"schema_version":version,"policy":policy,"status":"insufficient","bpm":null,
            "raw_beats":[0.06,0.56]});
        let header=neural_result_header(&base);
        assert_eq!(header,json!({"schema_version":version,"policy":policy,"status":"insufficient","bpm":null}));
        assert!(header.get("raw_beats").is_none());
    }
}

#[test]
fn late_joint_refusal_keeps_the_original_float_string_available() {
    let mut tree = json!({"schema_version":5,"backend":"beat_this","bpm":null,
        "policy":"neural-inferred-ordinals-v5","status":"insufficient",
        "retained_metric":77.94247297546801,"raw_beats":[0.06,0.56]});
    let base = tree.to_string();
    let exact = base.clone();
    let result = complete(
        &mut tree,
        json!({"evidence":"new"}),
        120.0,
        "estimated",
        &base,
        LIMIT - 1,
        0,
        0,
        &|| false,
    );
    assert!(matches!(result, Err(Failure::Unavailable(_))));
    assert_eq!(base.as_bytes(), exact.as_bytes());
    assert_eq!(tree["schema_version"], 6); // Deliberately discard this partial tree.
    assert!(base.contains("77.94247297546801"));
}

#[test]
fn cancellation_precedes_optional_budget_failure() {
    let mut tree = json!({"bpm":null});
    let base = tree.to_string();
    assert!(matches!(
        complete(
            &mut tree,
            json!({}),
            120.0,
            "estimated",
            &base,
            usize::MAX,
            0,
            0,
            &|| true
        ),
        Err(Failure::Cancelled)
    ));
    assert_eq!(tree["bpm"], Value::Null);
}

#[test]
fn supersession_during_finalization_never_selects_a_new_scalar() {
    let mut tree = json!({"bpm":null});
    let base = tree.to_string();
    let checks = std::cell::Cell::new(0);
    let stale = || {
        checks.set(checks.get() + 1);
        checks.get() > 1
    };
    assert!(matches!(
        complete(
            &mut tree,
            json!({}),
            120.0,
            "estimated",
            &base,
            0,
            0,
            0,
            &stale
        ),
        Err(Failure::Cancelled)
    ));
    assert_eq!(base, "{\"bpm\":null}");
}

#[test]
#[ignore = "explicit owned source plus genuine cached NN method; no NN, Engine or OwnACK"]
fn cached_prediction_phase_corroboration_component() -> Result<(), Box<dyn std::error::Error>> {
    cached_component(false,false)
}

#[test]
#[ignore = "explicit owned source plus genuine cached NN method; no NN, Engine or OwnACK"]
fn cached_prediction_bpm_range_component() -> Result<(), Box<dyn std::error::Error>> {
    cached_component(true,false)
}

// The historical nominal component is retired. Its sealed schema8 receipts are
// replayed by the saved-method validator; fresh kernel9 cannot emit old8.

// Prior input additionally requires profile_token (schema_version,
// installation_root, profile_sha256, numeric_profile_sha256) and prior_response
// (schema_version,status,reason,resources_released,identity,input,profile,prior).
// Its completed prior holds the frozen packed CDF/global/statistics and explicit
// cached_observation proof. Only ephemeral request echoes/current Native charge
// are rebound; scientific values and execution/source seals are never repaired.
fn cached_component(use_range: bool,use_prior:bool) -> Result<(), Box<dyn std::error::Error>> {
    use crate::audio_engine::{analysis_pcm::PcmIdentity, sample_loader};
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    fn hash(file: &mut File) -> Result<String, Box<dyn std::error::Error>> {
        file.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 65_536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        file.seek(SeekFrom::Start(0))?;
        Ok(format!("{:x}", hash.finalize()))
    }
    fn owned(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        assert!(path.is_absolute());
        let parent = path.parent().unwrap().canonicalize()?;
        let owner = parent.join("OWNER.md");
        assert!(fs::metadata(&owner)?.len() > 0 && fs::metadata(&owner)?.len() <= 65_536);
        assert!(!fs::read_to_string(owner)?.trim().is_empty());
        Ok(parent)
    }
    let input_path = PathBuf::from(
        std::env::var_os(if use_prior {"FLITZI_NOMINAL_PRIOR_INPUT"} else if use_range { "FLITZI_BPM_RANGE_INPUT" } else { "FLITZI_QM_CORROBORATION_INPUT" })
            .ok_or("explicit component input")?,
    );
    let root = owned(&input_path)?;
    assert!(fs::metadata(&input_path)?.len() <= 16 * 1024 * 1024);
    let mut input: Value = serde_json::from_slice(&fs::read(input_path)?)?;
    let source = PathBuf::from(input["source_path"].as_str().ok_or("source path")?);
    assert!(source.is_absolute());
    assert!(source.canonicalize()?.starts_with(Path::new("C:/SCRATCHBEATS").canonicalize()?));
    let expected = input["source_sha256"]
        .as_str()
        .ok_or("source SHA")?
        .to_owned();
    let rate = u32::try_from(
        input["loaded_sample_rate_hz"]
            .as_u64()
            .ok_or("loaded rate")?,
    )?;
    assert_eq!(rate, 48_000);
    let output = PathBuf::from(
        std::env::var_os(if use_prior {"FLITZI_NOMINAL_PRIOR_OUTPUT"} else if use_range { "FLITZI_BPM_RANGE_OUTPUT" } else { "FLITZI_QM_CORROBORATION_OUTPUT" })
            .ok_or("explicit component output")?,
    );
    owned(&output)?;
    assert!(!output.exists());
    let mut tree = input["neural_method"].take();
    // The opt-in cache crosses serde_json's decimal reader only in this test.
    // Restore supplied original bits; never recompute or repair fit metrics.
    if let Some(bits) = input.get("neural_float_bits") {
        let bits = bits.as_object().ok_or("cached neural float bit map")?;
        assert!(bits.len() <= 250_000);
        for (pointer, encoded) in bits {
            assert!(pointer.starts_with('/') && pointer.len() <= 1024);
            let slot = tree
                .pointer_mut(pointer)
                .ok_or("cached float pointer missing")?;
            assert!(slot.is_f64(), "cached float pointer must identify a float");
            let value = f64::from_bits(encoded.as_u64().ok_or("cached float bits must be u64")?);
            *slot =
                Value::Number(serde_json::Number::from_f64(value).ok_or("nonfinite cached float")?);
        }
    }
    assert_eq!(tree["source_sha256"], expected);
    if use_prior {assert!(matches!(tree["schema_version"].as_u64(),Some(4|5)));}
    else {assert_eq!(tree["schema_version"], 5);}
    assert_eq!(tree["backend"], "beat_this");
    if use_range||use_prior { assert!(tree["bpm"].as_f64().is_some()); }
    else { assert_eq!(tree["bpm"], Value::Null); }
    let original_ordinal = tree.get("ordinal_fit").map(|fit|format!("\"ordinal_fit\":{fit}"));
    let original_beats = format!("\"raw_beats\":{}", tree["raw_beats"]);
    let original_execution = format!("\"execution\":{}", tree["execution"]);
    let directory = tempfile::tempdir_in(root)?;
    let snapshot_path = directory.path().join("source.snapshot");
    let mut original = cold_store::sealed_reader(&source)?;
    assert_eq!(hash(&mut original)?, expected);
    let source_bytes = original.metadata()?.len();
    let mut writer = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&snapshot_path)?;
    assert_eq!(std::io::copy(&mut original, &mut writer)?, source_bytes);
    writer.sync_all()?;
    drop(writer);
    assert_eq!(hash(&mut original)?, expected);
    let mut snapshot = cold_store::sealed_reader(&snapshot_path)?;
    assert_eq!(hash(&mut snapshot)?, expected);
    let mut decoded = sample_loader::decode_audio_snapshot_streamed(
        snapshot.try_clone()?,
        &source,
        &directory.path().join("decoder.f32le"),
        rate,
        LIMIT - SMALL,
        &|| false,
        |_| {},
    )?;
    let descriptor = json!({"schema_version":1,"original":{"sha256":expected,"bytes":source_bytes},
        "pcm":decoded.artifact.descriptor.clone()});
    let decoder_identity = cold_store::descriptor_digest(&descriptor)?;
    let (mut playback, _) = sample_loader::prepare_playback_streamed(
        &mut decoded,
        &directory.path().join("playback.f32le"),
        2,
        rate,
        LIMIT - SMALL,
        &|| false,
        |_| {},
    )?;
    drop(decoded);
    let frames = usize::try_from(playback.descriptor["full_frames"].as_u64().unwrap())?;
    assert_eq!(tree["frame_count"], frames);
    assert_eq!(tree["sample_rate_hz"], rate);
    let count = frames.checked_mul(2).ok_or("sample extent")?;
    let pcm_bytes = count.checked_mul(4).ok_or("sample byte extent")?;
    admit(&[pcm_bytes.checked_mul(2).ok_or("Arc conversion extent")?])?;
    let mut values = Vec::new();
    values.try_reserve_exact(count)?;
    assert_eq!(values.capacity(), count);
    playback.reader.seek(SeekFrom::Start(0))?;
    let mut buffer = [0_u8; 4096 * 8];
    let mut left = count;
    while left > 0 {
        let n = left.min(buffer.len() / 4);
        playback.reader.read_exact(&mut buffer[..n * 4])?;
        values.extend(
            buffer[..n * 4]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap())),
        );
        left -= n;
    }
    assert_eq!(playback.reader.read(&mut [0_u8; 1])?, 0);
    let playback_sha = playback.descriptor["interleaved_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    drop(playback);
    let sample = SampleBuffer {
        residency: None,
        channels: 2,
        samples: values.into(),
    };
    let context = NeuralContext {
        installation: String::new(),
        staging_root: directory.path().to_owned(),
        descriptor,
        identity: PcmIdentity {
            pad_id: 0,
            request_id: 1,
            source_id: expected.clone(),
            source_generation: 1,
        },
        cancelled: Arc::new(|| false),
        additional_held_bytes: 0,
    };
    let period = if use_range||use_prior { None } else {
    let beats: Vec<_> = tree["raw_beats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let estimate = app_tempo::estimate(&beats, frames as f64 / f64::from(rate));
    let period =
        app_tempo::phase_only_period(&beats, frames as f64 / f64::from(rate), &estimate, &|| {
            false
        })
        .map_err(|_| "cached component cancelled")?
        .unwrap();
    drop(estimate);
    drop(beats);
    Some(period)
    };
    let nn_mono = tree["mono_sha256"].as_str().unwrap().to_owned();
    let base = tree.to_string();
    let source_live = budget::sum(&[pcm_bytes, budget::value_tree(&context.descriptor)?, SMALL,
        if use_prior {budget::value_tree(&input)?}else{0}])?;
    let raw_workspace = 16 * 1024 * 1024; // Conservative reservation, not fake retained logits.
    let grid_bytes = tree["raw_beats"].as_array().unwrap().len() * 4
        + tree["raw_downbeats"].as_array().unwrap().len() * 8;
    let base_core = budget::sum(&[
        source_live,
        raw_workspace,
        budget::value_tree(&tree)?,
        base.len() * 2,
        grid_bytes,
    ])?.max(if use_prior { super::super::admission_plan(frames,2,rate,0,false)?.primary_peak_bytes } else {0});
    let choice = if use_prior {
        let primary_bpm=tree["bpm"].as_f64().unwrap();
        let token=input["profile_token"].take();
        let cached=input["prior_response"].take();
        let mono_path=directory.path().join("mono.f32le");
        let mut mono_file=crate::audio_engine::analysis_jobs::create_staged_pcm(mono_path.to_str().unwrap())?;
        let mono_snapshot=LoadedPcmSnapshot::new(sample.clone(),rate,context.identity.clone(),LIMIT)?;
        let mut writer=MonoWriter{file:&mut mono_file,digest:Sha256::new(),bytes:0};
        mono_snapshot.stream_f32_le(&mut writer,LIMIT,LIMIT,&||false)?;
        writer.flush()?;
        assert_eq!(writer.bytes,frames*4);
        assert_eq!(format!("{:x}",writer.digest.finalize()),nn_mono);
        mono_file.sync_all()?;
        let selected=super::super::nominal_prior::cached_component(&sample,rate,&context,&nn_mono,
            &decoder_identity,&mono_path,&mut tree,&base,base_core,source_live,
            raw_workspace,grid_bytes,token,cached)?;
        verify_staged(&mut mono_file,frames*4,&nn_mono,&||false)?;
        drop(mono_file);
        assert!(primary_bpm==61.0||primary_bpm==144.0);
        assert_eq!(selected.bpm,if primary_bpm==61.0{122.0}else{72.0});
        assert_eq!(tree["schema_version"],8);
        assert_eq!(tree["nominal_selection"]["neural_result"]["bpm"],primary_bpm);
        selected
    } else if use_range {
        let range = super::super::bpm_range::BpmRange::configured(true,
            input["minimum_bpm"].as_f64().ok_or("minimum BPM")?,
            input["maximum_bpm"].as_f64().ok_or("maximum BPM")?)?.unwrap();
        let primary_bpm = tree["bpm"].as_f64().unwrap();
        let selected = super::super::bpm_range::apply(range,primary_bpm,&mut tree,base,
            base_core,source_live,raw_workspace,grid_bytes,&|| false,|work_core| {
                observation(&sample,rate,&context,&nn_mono,&decoder_identity,work_core,&|| false,
                    |bpm,extent| super::super::bpm_range::half_double_drift(primary_bpm,bpm,extent))
            })?;
        assert_eq!(primary_bpm,174.0);
        assert_eq!(selected.bpm,Some(87.0));
        assert!(selected.beat_error.is_none());
        assert_eq!(tree["schema_version"],7);
        assert_eq!(tree["bpm_range"]["primary"]["bpm"],primary_bpm);
        Choice { bpm:selected.bpm.unwrap(),method_json:selected.method_json.unwrap() }
    } else { attempt(
        &sample,
        rate,
        &context,
        &nn_mono,
        &decoder_identity,
        period.unwrap(),
        &mut tree,
        &base,
        base_core,
        source_live,
        raw_workspace,
        grid_bytes,
        &|| false,
    )? };
    if !use_prior {assert_eq!(choice.bpm, if use_range {87.0} else {120.0});}
    if let Some(original_ordinal)=original_ordinal {assert!(choice.method_json.contains(&original_ordinal));}
    assert!(choice.method_json.contains(&original_beats));
    assert!(choice.method_json.contains(&original_execution));
    assert_eq!(tree["schema_version"], if use_prior {8} else if use_range {7} else {6});
    let evidence = if use_prior {&tree["nominal_selection"]} else if use_range { &tree["bpm_range"]["selection"] } else { &tree["qm_corroboration"] };
    assert_eq!(
        evidence["source"]["loaded_interleaved_sha256"],
        playback_sha
    );
    assert_eq!(
        evidence["source"]["verified_nn_mono_sha256"],
        nn_mono
    );
    if !use_range&&!use_prior { assert!(
        tree["ordinal_fit"]["fractional_check"]["window_phase_difference_seconds"]
            .as_f64()
            .unwrap()
            > 0.005
    ); }
    let mut current = cold_store::sealed_reader(&source)?;
    assert_eq!(hash(&mut current)?, expected);
    drop(current);
    assert_eq!(hash(&mut original)?,expected);
    drop(original);
    drop(snapshot);
    drop(context);
    directory.close()?;
    let mut report = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    report.write_all(choice.method_json.as_bytes())?;
    report.sync_all()?;
    println!(
        "QM_CORROBORATION_COMPONENT source_sha256={expected} bpm={} no_nn_or_source_ack_claim=true",
        choice.bpm
    );
    Ok(())
}
