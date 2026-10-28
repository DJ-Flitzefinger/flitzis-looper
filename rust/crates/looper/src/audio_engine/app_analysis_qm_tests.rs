use super::*;

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
        std::env::var_os("FLITZI_QM_CORROBORATION_INPUT").ok_or("explicit component input")?,
    );
    let root = owned(&input_path)?;
    assert!(fs::metadata(&input_path)?.len() <= 16 * 1024 * 1024);
    let mut input: Value = serde_json::from_slice(&fs::read(input_path)?)?;
    let source = PathBuf::from(input["source_path"].as_str().ok_or("source path")?);
    assert!(source.is_absolute());
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
        std::env::var_os("FLITZI_QM_CORROBORATION_OUTPUT").ok_or("explicit component output")?,
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
    assert_eq!(tree["schema_version"], 5);
    assert_eq!(tree["backend"], "beat_this");
    assert_eq!(tree["bpm"], Value::Null);
    let original_ordinal = format!("\"ordinal_fit\":{}", tree["ordinal_fit"]);
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
    drop(original);
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
    let nn_mono = tree["mono_sha256"].as_str().unwrap().to_owned();
    let base = tree.to_string();
    let source_live = budget::sum(&[pcm_bytes, budget::value_tree(&context.descriptor)?, SMALL])?;
    let raw_workspace = 16 * 1024 * 1024; // Conservative reservation, not fake retained logits.
    let grid_bytes = tree["raw_beats"].as_array().unwrap().len() * 4
        + tree["raw_downbeats"].as_array().unwrap().len() * 8;
    let base_core = budget::sum(&[
        source_live,
        raw_workspace,
        budget::value_tree(&tree)?,
        base.len() * 2,
        grid_bytes,
    ])?;
    let choice = attempt(
        &sample,
        rate,
        &context,
        &nn_mono,
        &decoder_identity,
        period,
        &mut tree,
        &base,
        base_core,
        source_live,
        raw_workspace,
        grid_bytes,
        &|| false,
    )?;
    assert_eq!(choice.bpm, 120.0);
    assert!(choice.method_json.contains(&original_ordinal));
    assert!(choice.method_json.contains(&original_beats));
    assert!(choice.method_json.contains(&original_execution));
    assert_eq!(tree["schema_version"], 6);
    assert_eq!(
        tree["qm_corroboration"]["source"]["loaded_interleaved_sha256"],
        playback_sha
    );
    assert_eq!(
        tree["qm_corroboration"]["source"]["verified_nn_mono_sha256"],
        nn_mono
    );
    assert!(
        tree["ordinal_fit"]["fractional_check"]["window_phase_difference_seconds"]
            .as_f64()
            .unwrap()
            > 0.005
    );
    let mut current = cold_store::sealed_reader(&source)?;
    assert_eq!(hash(&mut current)?, expected);
    drop(current);
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
