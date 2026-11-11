//! Standalone CPU operator control for the HTDemucs exporter, using synthetic Q/K/V.
//! The caller supplies and admits the exact generated operator fixture before startup.
#[allow(dead_code)]
#[path = "beat_this_audio/runtime.rs"]
mod runtime;

use flitzis_looper_analysis::trial_lifetime::Context;
use ort::{
    ep,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [graph, expected_digest, output, profile] = args.as_slice() else {
        return Err("expected GRAPH SHA256 OUTPUT PROFILE_PREFIX".into());
    };
    if !runtime::package_requested()
        || std::env::var(runtime::LOAD_POLICY_ENV).as_deref() != Ok("package-system32")
    {
        return Err("attention control requires the Windows controlled ORT package".into());
    }
    let expected_digest = expected_digest.to_str().ok_or("invalid graph digest")?;
    let bytes = read_bounded(graph, 1024 * 1024)?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected_digest {
        return Err("attention control graph digest mismatch".into());
    }
    let manifest = std::env::var_os(runtime::MANIFEST_ENV).ok_or("missing runtime manifest")?;
    let digest = std::env::var(runtime::DIGEST_ENV).map_err(|e| e.to_string())?;
    let context = Context::new(None);
    let (admission, environment) =
        runtime::configure(Some((Path::new(&manifest), &digest)), &context)?;
    let admission = admission.ok_or("missing runtime admission")?;
    runtime::commit(environment)?;

    let providers = [ep::CPU::default()
        .with_arena_allocator(false)
        .build()
        .error_on_failure()];
    let builder = Session::builder().map_err(|e| e.to_string())?;
    let mut session = builder
        .with_no_environment_execution_providers()
        .map_err(|e| e.to_string())?
        .with_execution_providers(providers)
        .map_err(|e| e.to_string())?
        .with_intra_threads(1)
        .map_err(|e| e.to_string())?
        .with_inter_threads(1)
        .map_err(|e| e.to_string())?
        .with_parallel_execution(false)
        .map_err(|e| e.to_string())?
        .with_intra_op_spinning(false)
        .map_err(|e| e.to_string())?
        .with_inter_op_spinning(false)
        .map_err(|e| e.to_string())?
        .with_memory_pattern(false)
        .map_err(|e| e.to_string())?
        .with_optimization_level(GraphOptimizationLevel::Disable)
        .map_err(|e| e.to_string())?
        .with_config_entry("session.disable_prepacking", "1")
        .map_err(|e| e.to_string())?
        .with_profiling(profile)
        .map_err(|e| e.to_string())?
        .commit_from_memory(&bytes)
        .map_err(|e| e.to_string())?;
    drop(bytes);
    let dir = graph.parent().ok_or("missing fixture directory")?;
    let inputs = [
        ("q_time", 1344),
        ("k_time", 1344),
        ("v_time", 1344),
        ("q_spectral", 2688),
        ("k_spectral", 2688),
        ("v_spectral", 2688),
    ]
    .into_iter()
    .map(|(name, tokens)| {
        let count = 8 * tokens * 64;
        let raw = read_bounded(&dir.join(format!("{name}.f32")), count * 4)?;
        if raw.len() != count * 4 {
            return Err("wrong attention fixture input extent".into());
        }
        let values = raw
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("exact chunk")))
            .collect::<Vec<_>>();
        if values.iter().any(|value| !value.is_finite()) {
            return Err("nonfinite attention fixture input".into());
        }
        let tensor = Tensor::from_array(([1, 8, tokens, 64], values)).map_err(|e| e.to_string())?;
        Ok((name, tensor))
    })
    .collect::<Result<Vec<_>, String>>()?;
    let results = session.run(inputs).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(file);
    for (index, queries) in [2688, 1344, 2688, 1344, 2688, 1344, 2688, 1344, 2688, 1344]
        .into_iter()
        .enumerate()
    {
        let name = format!("context_{index}");
        let (shape, values) = results
            .get(&name)
            .ok_or("missing attention result")?
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        if shape.as_ref() != [1, 8, queries, 64] || values.iter().any(|value| !value.is_finite()) {
            return Err("invalid attention fixture result".into());
        }
        for value in values {
            writer
                .write_all(&value.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
    }
    writer.flush().map_err(|e| e.to_string())?;
    drop((writer, results));
    let profile_path = session.end_profiling().map_err(|e| e.to_string())?;
    drop(session);
    context.cleanup()?;
    println!("ATTENTION_PROFILE {profile_path}");
    println!("ATTENTION_RUNTIME {}", admission.receipt()?);
    Ok(())
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    File::open(path)
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut raw))
        .map_err(|e| e.to_string())?;
    if raw.len() > limit {
        return Err("attention fixture file exceeds its extent".into());
    }
    Ok(raw)
}
