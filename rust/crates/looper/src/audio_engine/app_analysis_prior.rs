//! Optional nominal-unit arbitration on the existing immutable analysis request.
use super::qm_corroboration::{Failure, Observation};
use super::*;
use crate::audio_engine::neural_analysis::budget;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::path::Path;

const MIB: usize = 1024 * 1024;
const RESPONSE_LIMIT: usize = 2 * MIB;
const TOKEN_LIMIT: usize = 32 * 1024;
const CLASSES: usize = 256;
const PROFILE_ID: &str = "tempocnn-0.0.8-deeptemp-k16-3-tfjs4.22-node25.5-cpu-f32-v1";
const NUMERIC_PROFILE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../workers/metrical_prior/numeric-profile.json"
));
const FRONTEND_REFERENCE: &str = "e4f8ba95983a74624692a044c18301493443a08af1a04833b589c7a1959022ba";
const FRONTEND_RUNNER: &str = "301407566bf84094cee436996fa72d3d2b7687b8a2459128a640fe5f0cfd6100";
const INFERENCE_REFERENCE: &str =
    "9bbf0037fe210a04a75c812badc38c003ae900cbc1c05aca4ce52b6bbeaa0f21";
const GRAPH_SHA256: &str = "f93693c5a913b69aa378438665937600c09286f57714359155f2763496aba184";
const SHARD_SHA256: &str = "f7688e8868706813cd74610442f9923fca51162c1ebde6cf7fecfd62ebd24a15";
const INFERENCE_RUNNER: &str = "bd51cf2c20d411ff747f2918d5ef2c3cb0470c32d881f45946b50de1ef4154df";

#[derive(Clone, Copy, Debug)]
struct Geometry {
    analyzer_frames: usize,
    patches: usize,
    probability_bytes: usize,
    bridge_bytes: usize,
}

fn unavailable(detail: impl std::fmt::Display) -> Failure {
    Failure::Unavailable(detail.to_string().chars().take(240).collect())
}
fn check(cancelled: &impl Fn() -> bool) -> Result<(), Failure> {
    if cancelled() {
        Err(Failure::Cancelled)
    } else {
        Ok(())
    }
}
fn admit(parts: &[usize]) -> Result<usize, Failure> {
    let bytes = budget::sum(parts).map_err(unavailable)?;
    if bytes > LIMIT {
        Err(unavailable(
            "nominal check exceeds 512 MiB preparation budget",
        ))
    } else {
        Ok(bytes)
    }
}
fn digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn geometry(frames: u64, rate: u32) -> Result<Geometry, Failure> {
    if frames == 0 || !(8_000..=384_000).contains(&rate) {
        return Err(unavailable("nominal input geometry unavailable"));
    }
    let analyzer_frames = usize::try_from((u128::from(frames) * 11_025).div_ceil(u128::from(rate)))
        .map_err(|_| unavailable("nominal analyzer extent overflow"))?;
    let mel_frames = analyzer_frames
        .checked_div(512)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| unavailable("nominal mel extent overflow"))?;
    let patches = 1 + (mel_frames.max(256) - 256) / 128;
    if !(1..=1024).contains(&patches) {
        return Err(unavailable("nominal patch bound unavailable"));
    }
    let probability_bytes = patches
        .checked_mul(1024)
        .ok_or_else(|| unavailable("nominal packet overflow"))?;
    let encoded = probability_bytes
        .div_ceil(3)
        .checked_mul(4)
        .ok_or_else(|| unavailable("nominal packet overflow"))?;
    let bridge_bytes = budget::sum(&[
        MIB,
        probability_bytes
            .checked_mul(2)
            .ok_or_else(|| unavailable("nominal bridge overflow"))?,
        encoded
            .checked_mul(4)
            .ok_or_else(|| unavailable("nominal bridge overflow"))?,
        4 * 2732,
    ])
    .map_err(unavailable)?;
    Ok(Geometry {
        analyzer_frames,
        patches,
        probability_bytes,
        bridge_bytes,
    })
}

fn candidate_class(bpm: f64) -> Option<usize> {
    if !bpm.is_finite() || !(30.0..=285.0).contains(&bpm) || bpm.fract() == 0.5 {
        return None;
    }
    let class = (bpm + 0.5).floor() as usize;
    (30..=285).contains(&class).then_some(class)
}

fn fields(value: &Value, expected: &[&str]) -> Result<(), Failure> {
    let map = value
        .as_object()
        .ok_or_else(|| unavailable("nominal object missing"))?;
    if map.len() != expected.len() || expected.iter().any(|key| !map.contains_key(*key)) {
        return Err(unavailable("nominal object fields differ"));
    }
    Ok(())
}
fn packed(
    value: &Value,
    encoding: &str,
    shape: &[usize],
    bytes: usize,
) -> Result<Vec<u8>, Failure> {
    fields(value, &["encoding", "shape", "sha256", "data"])?;
    if value["encoding"] != encoding || value["shape"] != json!(shape) {
        return Err(unavailable("nominal packet shape or encoding differs"));
    }
    let expected = value["sha256"]
        .as_str()
        .filter(|text| digest(text))
        .ok_or_else(|| unavailable("nominal packet digest missing"))?;
    let encoded = value["data"]
        .as_str()
        .ok_or_else(|| unavailable("nominal packet data missing"))?;
    if encoded.len() != bytes.div_ceil(3) * 4 {
        return Err(unavailable("nominal packet extent differs"));
    }
    let mut decoded = Vec::new();
    let capacity = bytes
        .checked_add(2)
        .ok_or_else(|| unavailable("nominal decode overflow"))?;
    decoded.try_reserve_exact(capacity).map_err(unavailable)?;
    decoded.resize(capacity, 0);
    let n = STANDARD
        .decode_slice(encoded, &mut decoded)
        .map_err(unavailable)?;
    if n != bytes {
        return Err(unavailable("nominal decoded extent differs"));
    }
    decoded.truncate(n);
    if STANDARD.encode(&decoded) != encoded || format!("{:x}", Sha256::digest(&decoded)) != expected
    {
        return Err(unavailable("nominal packet identity differs"));
    }
    Ok(decoded)
}
fn f64_hex(value: &Value) -> Result<f64, Failure> {
    let text = value
        .as_str()
        .filter(|text| {
            text.len() == 16
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or_else(|| unavailable("nominal binary64 statistic differs"))?;
    let mut bytes = [0_u8; 8];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(unavailable)?;
    }
    let result = f64::from_le_bytes(bytes);
    if !result.is_finite() {
        return Err(unavailable("nominal statistic is nonfinite"));
    }
    Ok(result)
}

/// Decode one authoritative packed CDF, then drop all binary scratch before any
/// method-tree/string coexistence. No decimal probability arrays are created.
fn distribution(
    prior: &Value,
    shape: Geometry,
    cancelled: &impl Fn() -> bool,
) -> Result<[f64; 256], Failure> {
    check(cancelled)?;
    let bytes = packed(
        &prior["probabilities"],
        "float32-le/base64",
        &[shape.patches, 256],
        shape.probability_bytes,
    )?;
    let mut mean = [0.0_f64; 256];
    for (row, chunk) in bytes.chunks_exact(1024).enumerate() {
        if row.is_multiple_of(16) {
            check(cancelled)?;
        }
        let mut sum = 0.0_f64;
        for (index, entry) in chunk.chunks_exact(4).enumerate() {
            let value = f32::from_le_bytes(entry.try_into().unwrap());
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(unavailable("nominal probability is invalid"));
            }
            let value = f64::from(value);
            sum += value;
            mean[index] += value;
        }
        if (sum - 1.0).abs() > 0.00001 {
            return Err(unavailable("nominal softmax sum differs"));
        }
    }
    drop(bytes);
    for value in &mut mean {
        *value /= shape.patches as f64;
    }
    let global = &prior["global"];
    fields(
        global,
        &["encoding", "shape", "sha256", "data", "first_argmax_class"],
    )?;
    let packet = json!({"encoding":global["encoding"],"shape":global["shape"],"sha256":global["sha256"],"data":global["data"]});
    let bytes = packed(&packet, "float64-le/base64", &[256], 2048)?;
    let mut first = 0;
    for (index, chunk) in bytes.chunks_exact(8).enumerate() {
        if f64::from_le_bytes(chunk.try_into().unwrap()).to_bits() != mean[index].to_bits() {
            return Err(unavailable("nominal global mean differs"));
        }
        if mean[index] > mean[first] {
            first = index;
        }
    }
    if global["first_argmax_class"].as_u64() != Some((first + 30) as u64) {
        return Err(unavailable("nominal first maximum differs"));
    }
    check(cancelled)?;
    Ok(mean)
}

fn prefers_qm(mean: &[f64; 256], neural: usize, qm: usize) -> bool {
    if neural == qm {
        return false;
    }
    let maximum = mean.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    mean[qm - 30] == maximum
        && mean[qm - 30] > mean[neural - 30]
        && mean.iter().filter(|&&p| p == maximum).count() == 1
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema_version: u32,
    installation_root: String,
    profile_sha256: String,
    numeric_profile_sha256: String,
}
impl Profile {
    fn token(&self) -> Value {
        json!({"schema_version":self.schema_version,"installation_root":self.installation_root,
            "profile_sha256":self.profile_sha256,"numeric_profile_sha256":self.numeric_profile_sha256})
    }
    fn validate(&self) -> Result<(), Failure> {
        let root = Path::new(&self.installation_root);
        let name = root.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if self.schema_version != 1
            || !root.is_absolute()
            || self.installation_root.len() > 16 * 1024
            || !name.starts_with("installation-")
            || name.len() != 45
            || !name[13..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !digest(&self.profile_sha256)
            || self.numeric_profile_sha256
                != format!("{:x}", Sha256::digest(NUMERIC_PROFILE.as_bytes()))
        {
            return Err(unavailable("nominal frozen profile differs"));
        }
        Ok(())
    }
}

fn validate_prior(
    prior: &Value,
    profile: &Profile,
    request: &Value,
    shape: Geometry,
    native_live: usize,
    allow_cached: bool,
    cancelled: &impl Fn() -> bool,
) -> Result<[f64; 256], Failure> {
    fields(
        prior,
        &[
            "profile",
            "input",
            "frontend",
            "probabilities",
            "global",
            "execution",
            "resources",
        ],
    )?;
    let p = &prior["profile"];
    fields(
        p,
        &[
            "profile_id",
            "profile_sha256",
            "numeric_profile_sha256",
            "python_version",
            "python_executable_sha256",
            "python_dll_sha256",
            "node_version",
            "node_executable_sha256",
            "tfjs_version",
            "graph_sha256",
            "shard_sha256",
            "frontend_reference_sha256",
            "frontend_runner_sha256",
            "inference_reference_sha256",
            "inference_runner_sha256",
        ],
    )?;
    for (key, expected) in [
        ("profile_id", PROFILE_ID),
        ("profile_sha256", profile.profile_sha256.as_str()),
        (
            "numeric_profile_sha256",
            profile.numeric_profile_sha256.as_str(),
        ),
        ("python_version", "3.12.13"),
        (
            "python_executable_sha256",
            "f598fb950a86a895d8f9b4755fc9b38c48adc7a15732a342e55c17a3c3499602",
        ),
        (
            "python_dll_sha256",
            "70713cbbbcacd1ca70f802e87683d85aea185da1aae9bd31d6c02be51cfee819",
        ),
        ("node_version", "v25.5.0"),
        (
            "node_executable_sha256",
            "d7f55cbc1b1ca9cc2815d7b8c58f94e4822a68cdf5ca55b4f2c9625581b92839",
        ),
        ("tfjs_version", "4.22.0"),
        ("graph_sha256", GRAPH_SHA256),
        ("shard_sha256", SHARD_SHA256),
        ("frontend_reference_sha256", FRONTEND_REFERENCE),
        ("frontend_runner_sha256", FRONTEND_RUNNER),
        ("inference_reference_sha256", INFERENCE_REFERENCE),
    ] {
        if p[key] != expected {
            return Err(unavailable("nominal profile pin differs"));
        }
    }
    if p["inference_runner_sha256"] != INFERENCE_RUNNER {
        return Err(unavailable("nominal inference identity differs"));
    }
    let input = &prior["input"];
    fields(
        input,
        &[
            "source_sha256",
            "mono_sha256",
            "sample_rate_hz",
            "frame_count",
            "origin_seconds",
            "downmix_revision",
        ],
    )?;
    for key in [
        "source_sha256",
        "mono_sha256",
        "sample_rate_hz",
        "frame_count",
        "downmix_revision",
    ] {
        if input[key] != request["pcm"][key] {
            return Err(unavailable("nominal complete input differs"));
        }
    }
    if input["origin_seconds"].as_f64() != Some(0.0) {
        return Err(unavailable("nominal input origin differs"));
    }
    let frontend = &prior["frontend"];
    fields(
        frontend,
        &[
            "frontend_id",
            "sample_rate_hz",
            "frame_count",
            "origin_seconds",
            "mono_sha256",
            "feature_shape",
            "feature_sha256",
            "normalization",
        ],
    )?;
    if frontend["frontend_id"] != "tempocnn-author-482c64ad-librosa011-global-std-f16-v1"
        || frontend["sample_rate_hz"].as_u64() != Some(11025)
        || frontend["frame_count"].as_u64() != Some(shape.analyzer_frames as u64)
        || frontend["origin_seconds"].as_f64() != Some(0.0)
        || frontend["feature_shape"] != json!([shape.patches, 40, 256, 1])
        || !frontend["mono_sha256"].as_str().is_some_and(digest)
        || !frontend["feature_sha256"].as_str().is_some_and(digest)
    {
        return Err(unavailable("nominal frontend geometry or identity differs"));
    }
    let normalization = &frontend["normalization"];
    fields(
        normalization,
        &[
            "scope",
            "precision",
            "roundtrip",
            "mean_f64_le_hex",
            "std_f64_le_hex",
        ],
    )?;
    if normalization["scope"] != "entire-overlapping-window-tensor"
        || normalization["precision"] != "float64"
        || normalization["roundtrip"] != "float16-to-float32"
        || f64_hex(&normalization["std_f64_le_hex"])? <= 0.0
    {
        return Err(unavailable("nominal normalization differs"));
    }
    f64_hex(&normalization["mean_f64_le_hex"])?;
    let execution = &prior["execution"];
    let cached = execution["origin"] == "cached_observation";
    if cached {
        if !allow_cached {
            return Err(unavailable(
                "cached nominal execution is not a worker result",
            ));
        }
        fields(execution, &["origin", "device", "cached_observation"])?;
        let proof = &execution["cached_observation"];
        fields(
            proof,
            &[
                "runner_sha256",
                "cdf_sha256",
                "input_manifest_sha256",
                "frontend_seal_sha256",
                "numeric_profile_sha256",
            ],
        )?;
        for (key, expected) in [
            ("runner_sha256", INFERENCE_REFERENCE),
            (
                "cdf_sha256",
                "6bede0d19e36cec9466aa1ab20d07f8634b54828d5c056acd33c1f43b5a3bc52",
            ),
            (
                "input_manifest_sha256",
                "7230218c541822b40aa55db5148d98de1e5e4d2fd046bba09ae29845ec279523",
            ),
            (
                "frontend_seal_sha256",
                "487eebdad8e2c7c1bb8fda975577ee74072e006829a38396c0a4fde0181e921e",
            ),
            (
                "numeric_profile_sha256",
                profile.numeric_profile_sha256.as_str(),
            ),
        ] {
            if proof[key] != expected {
                return Err(unavailable("cached nominal proof differs"));
            }
        }
    } else {
        fields(execution, &["origin", "device"])?;
        if execution["origin"] != "computed" {
            return Err(unavailable("nominal execution origin differs"));
        }
    }
    if execution["device"] != "cpu" {
        return Err(unavailable("nominal execution device differs"));
    }
    let resources = &prior["resources"];
    fields(
        resources,
        &[
            "native_live_bytes",
            "aggregate_limit_bytes",
            "frontend_admitted_child_peak_bytes",
            "frontend_observed_child_peak_bytes",
            "classifier_observed_child_peak_bytes",
            "measurement",
            "aggregate_claim",
        ],
    )?;
    if resources["native_live_bytes"].as_u64() != Some(native_live as u64)
        || resources["aggregate_limit_bytes"].as_u64() != Some(LIMIT as u64)
        || resources["measurement"]
            != if cached {
                "cached-rss-only"
            } else {
                "windows-private-and-working-set-peaks"
            }
        || resources["aggregate_claim"].as_bool() != Some(!cached)
    {
        return Err(unavailable("nominal aggregate resource receipt differs"));
    }
    for key in [
        "frontend_admitted_child_peak_bytes",
        "frontend_observed_child_peak_bytes",
        "classifier_observed_child_peak_bytes",
    ] {
        let bytes = usize::try_from(
            resources[key]
                .as_u64()
                .ok_or_else(|| unavailable("nominal resource extent missing"))?,
        )
        .map_err(unavailable)?;
        if bytes == 0 || bytes > LIMIT {
            return Err(unavailable("nominal resource extent differs"));
        }
        if !cached {
            admit(&[native_live, bytes])?;
        }
    }
    distribution(prior, shape, cancelled)
}

fn response(
    mut value: Value,
    profile: &Profile,
    request: &Value,
    shape: Geometry,
    native_live: usize,
    allow_cached: bool,
    cancelled: &impl Fn() -> bool,
) -> Result<(Value, [f64; 256]), Failure> {
    check(cancelled)?;
    let status = value["status"]
        .as_str()
        .ok_or_else(|| unavailable("nominal worker status missing"))?;
    if status == "completed" {
        fields(
            &value,
            &[
                "schema_version",
                "status",
                "reason",
                "resources_released",
                "identity",
                "input",
                "profile",
                "prior",
            ],
        )?;
    } else {
        fields(
            &value,
            &[
                "schema_version",
                "status",
                "reason",
                "resources_released",
                "identity",
                "input",
                "profile",
            ],
        )?;
    }
    if value["schema_version"].as_u64() != Some(1)
        || value["resources_released"].as_bool() != Some(true)
        || value["identity"] != request["identity"]
        || value["profile"] != profile.token()
        || !value["reason"].as_str().is_some_and(|s| s.len() <= 1024)
    {
        return Err(unavailable("nominal worker envelope differs"));
    }
    let mut expected = request["pcm"].clone();
    expected
        .as_object_mut()
        .ok_or_else(|| unavailable("nominal expected input missing"))?
        .remove("path");
    if value["input"] != expected {
        return Err(unavailable("nominal worker input echo differs"));
    }
    match status {
        "cancelled" => return Err(Failure::Cancelled),
        "failed" => return Err(unavailable("optional nominal worker failed")),
        "completed" => {}
        _ => return Err(unavailable("nominal worker status differs")),
    }
    let prior = value["prior"].take();
    drop(value);
    let mean = validate_prior(
        &prior,
        profile,
        request,
        shape,
        native_live,
        allow_cached,
        cancelled,
    )?;
    Ok((prior, mean))
}

// Closed structs keep malformed nested JSON from allocating an arbitrary Value
// tree before the admitted packed-response bounds can be checked.
macro_rules! wire_record {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Deserialize, Serialize)]
        #[serde(deny_unknown_fields)]
        struct $name { $($field: $ty),* }
    };
}
wire_record!(WireIdentity {
    pad_id: u64,
    request_id: u64,
    source_id: String,
    source_generation: u64
});
wire_record!(WireInput {
    source_sha256: String,
    mono_sha256: String,
    sample_rate_hz: u32,
    frame_count: u64,
    origin_seconds: f64,
    dtype: String,
    channels: u32,
    downmix_revision: String
});
wire_record!(PriorInput {
    source_sha256: String,
    mono_sha256: String,
    sample_rate_hz: u32,
    frame_count: u64,
    origin_seconds: f64,
    downmix_revision: String
});
wire_record!(PriorProfile {
    profile_id: String,
    profile_sha256: String,
    numeric_profile_sha256: String,
    python_version: String,
    python_executable_sha256: String,
    python_dll_sha256: String,
    node_version: String,
    node_executable_sha256: String,
    tfjs_version: String,
    graph_sha256: String,
    shard_sha256: String,
    frontend_reference_sha256: String,
    frontend_runner_sha256: String,
    inference_reference_sha256: String,
    inference_runner_sha256: String
});
wire_record!(Normalization {
    scope: String,
    precision: String,
    roundtrip: String,
    mean_f64_le_hex: String,
    std_f64_le_hex: String
});
wire_record!(Frontend {
    frontend_id: String,
    sample_rate_hz: u32,
    frame_count: u64,
    origin_seconds: f64,
    mono_sha256: String,
    feature_shape: [usize; 4],
    feature_sha256: String,
    normalization: Normalization
});
wire_record!(ProbabilityPacket {
    encoding: String,
    shape: [usize; 2],
    sha256: String,
    data: String
});
wire_record!(GlobalPacket {
    encoding: String,
    shape: [usize; 1],
    sha256: String,
    data: String,
    first_argmax_class: u16
});
wire_record!(CacheProof {
    runner_sha256: String,
    cdf_sha256: String,
    input_manifest_sha256: String,
    frontend_seal_sha256: String,
    numeric_profile_sha256: String
});
fn present<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    origin: String,
    device: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    cached_observation: Option<CacheProof>,
}
wire_record!(Resources {
    native_live_bytes: u64,
    aggregate_limit_bytes: u64,
    frontend_admitted_child_peak_bytes: u64,
    frontend_observed_child_peak_bytes: u64,
    classifier_observed_child_peak_bytes: u64,
    measurement: String,
    aggregate_claim: bool
});
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PriorWire {
    profile: PriorProfile,
    input: PriorInput,
    frontend: Frontend,
    probabilities: ProbabilityPacket,
    #[serde(rename = "global")]
    global_mean: GlobalPacket,
    execution: Execution,
    resources: Resources,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResponseWire {
    schema_version: u32,
    status: String,
    reason: String,
    resources_released: bool,
    identity: WireIdentity,
    input: WireInput,
    profile: Profile,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    prior: Option<PriorWire>,
}

fn probe_profile(root: &Path, cancelled: &impl Fn() -> bool) -> Result<Profile, Failure> {
    check(cancelled)?;
    if !root
        .join("current.json")
        .try_exists()
        .map_err(|_| unavailable("nominal profile readiness unavailable"))?
    {
        return Err(Failure::NoChoice);
    }
    let root_text = root
        .to_str()
        .ok_or_else(|| unavailable("nominal profile path unavailable"))?;
    let profile = Python::attach(|py| {
        let result = py
            .import("flitzis_looper.analysis.metrical_prior")
            .and_then(|module| module.getattr("probe_metrical_prior_profile"))
            .and_then(|entry| entry.call1((root_text,)))
            .map_err(|_| unavailable("nominal profile verification failed"))?;
        let encoded = result
            .cast::<pyo3::types::PyString>()
            .map_err(|_| unavailable("nominal profile token missing"))?
            .to_str()
            .map_err(|_| unavailable("nominal profile token invalid"))?;
        if encoded.len() > TOKEN_LIMIT {
            return Err(unavailable("nominal profile token exceeds its bound"));
        }
        serde_json::from_str::<Profile>(encoded)
            .map_err(|_| unavailable("nominal frozen profile token differs"))
    })?;
    check(cancelled)?;
    profile.validate()?;
    let actual = Path::new(&profile.installation_root)
        .parent()
        .ok_or_else(|| unavailable("nominal profile parent missing"))?;
    if actual
        .canonicalize()
        .map_err(|_| unavailable("nominal profile parent unavailable"))?
        != root
            .canonicalize()
            .map_err(|_| unavailable("nominal profile root unavailable"))?
    {
        return Err(unavailable(
            "nominal frozen installation is outside its configured root",
        ));
    }
    Ok(profile)
}

fn invoke(
    request: &Value,
    shape: Geometry,
    cancellation: &Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Value, Failure> {
    check(&|| cancellation())?;
    let encoded = serde_json::to_string(request).map_err(unavailable)?;
    if encoded.len() > 32 * 1024 {
        return Err(unavailable("nominal request exceeds its bound"));
    }
    let dynamic_limit = shape.probability_bytes.div_ceil(3) * 4 + 64 * 1024 + 2732;
    let value = Python::attach(|py| {
        let probe = Py::new(py, AnalysisCancellation(cancellation.clone()))
            .map_err(|_| unavailable("nominal cancellation probe unavailable"))?;
        let module = py
            .import("flitzis_looper.analysis.metrical_prior")
            .map_err(|_| unavailable("nominal bridge unavailable"))?;
        let result = module
            .getattr("run_metrical_prior")
            .and_then(|entry| entry.call1((encoded, probe)))
            .map_err(|_| unavailable("optional nominal worker failed"))?;
        let encoded = result
            .cast::<pyo3::types::PyString>()
            .map_err(|_| unavailable("nominal worker response missing"))?
            .to_str()
            .map_err(|_| unavailable("nominal worker response invalid"))?;
        if encoded.len() > RESPONSE_LIMIT.min(dynamic_limit) {
            return Err(unavailable("nominal response exceeds its bound"));
        }
        let response: ResponseWire = serde_json::from_str(encoded)
            .map_err(|_| unavailable("nominal response fields differ"))?;
        serde_json::to_value(response).map_err(unavailable)
    })?;
    check(&|| cancellation())?;
    Ok(value)
}

fn eligible(tree: &Value, range: Option<BpmRange>) -> Option<(f64, usize)> {
    if !matches!(tree["schema_version"].as_u64(), Some(4 | 5))
        || tree["backend"] != "beat_this"
        || !matches!(
            tree["status"].as_str(),
            Some("estimated" | "single_region_estimate")
        )
    {
        return None;
    }
    let bpm = tree["bpm"].as_f64()?;
    if range.is_some_and(|r| !r.contains(bpm)) {
        return None;
    }
    Some((bpm, candidate_class(bpm)?))
}

#[allow(clippy::too_many_arguments)]
fn transact(
    tree: &mut Value,
    base: &String,
    base_core: usize,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    range: Option<BpmRange>,
    shape: Geometry,
    profile: &Profile,
    mut request: Value,
    allow_cached: bool,
    cancelled: &impl Fn() -> bool,
    observe: impl FnOnce(usize, f64) -> Result<Observation, Failure>,
    run: impl FnOnce(&Value) -> Result<Value, Failure>,
) -> Result<qm_corroboration::Choice, Failure> {
    check(cancelled)?;
    let (primary_bpm, primary_class) = eligible(tree, range).ok_or(Failure::NoChoice)?;
    let baseline = budget::sum(&[
        source_live,
        raw_workspace,
        budget::value_tree(tree).map_err(unavailable)?,
        base.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])
    .map_err(unavailable)?
    .max(base_core);
    let work_core = admit(&[baseline, shape.bridge_bytes])?;
    profile.validate()?;
    let observed = observe(work_core, primary_bpm)?;
    check(cancelled)?;
    if range.is_some_and(|r| !r.contains(observed.bpm)) {
        return Err(Failure::NoChoice);
    }
    let qm_class = candidate_class(observed.bpm).ok_or(Failure::NoChoice)?;
    if qm_class == primary_class {
        return Err(Failure::NoChoice);
    }
    let nn_frames = tree["frame_count"]
        .as_u64()
        .ok_or_else(|| unavailable("nominal source extent missing"))?;
    let nn_rate = tree["sample_rate_hz"]
        .as_u64()
        .filter(|&n| n > 0)
        .ok_or_else(|| unavailable("nominal source rate missing"))?;
    let qm_frames = observed.qm["analyzer"]["frame_count"]
        .as_u64()
        .ok_or_else(|| unavailable("nominal QM extent missing"))?;
    let span = (nn_frames as f64 / nn_rate as f64).min(qm_frames as f64 / 44_100.0);
    let drift = bpm_range::half_double_drift(primary_bpm, observed.bpm, span)
        .map_err(|_| Failure::NoChoice)?;
    if drift != observed.period_drift_seconds {
        return Err(unavailable("nominal QM agreement receipt differs"));
    }
    let qm_bytes = budget::sum(&[
        budget::value_tree(&observed.source).map_err(unavailable)?,
        budget::value_tree(&observed.qm).map_err(unavailable)?,
        size_of::<Observation>(),
    ])
    .map_err(unavailable)?;
    let native_live = admit(&[baseline, shape.bridge_bytes, qm_bytes])?;
    request["native_live_bytes"] = json!(native_live);
    let encoded_length = budget::serialized_len(&request).map_err(unavailable)?;
    if encoded_length > 32 * 1024 {
        return Err(unavailable("nominal request exceeds its bound"));
    }
    let returned = run(&request)?;
    check(cancelled)?;
    let (prior, mean) = response(
        returned,
        profile,
        &request,
        shape,
        native_live,
        allow_cached,
        cancelled,
    )?;
    if !prefers_qm(&mean, primary_class, qm_class) {
        return Err(Failure::NoChoice);
    }
    let neural_result = json!({"schema_version":tree["schema_version"],"backend":tree["backend"],
        "policy":tree["policy"],"status":tree["status"],"bpm":tree["bpm"]});
    let Observation {
        source,
        qm,
        bpm,
        status,
        period_drift_seconds,
    } = observed;
    let mut selection = serde_json::Map::new();
    selection.insert("neural_result".into(), neural_result);
    selection.insert("source".into(), source);
    selection.insert("qm".into(), qm);
    selection.insert("period_drift_seconds".into(), json!(period_drift_seconds));
    if let Some(range) = range {
        selection.insert(
            "range_constraint".into(),
            json!({
        "minimum_bpm":range.minimum_bpm,"maximum_bpm":range.maximum_bpm}),
        );
    }
    selection.insert("prior".into(), prior);
    complete(
        tree,
        Value::Object(selection),
        bpm,
        status,
        base,
        source_live,
        raw_workspace,
        grid_bytes,
        cancelled,
    )
}

#[allow(clippy::too_many_arguments)]
fn complete(
    tree: &mut Value,
    selection: Value,
    bpm: f64,
    status: &str,
    base: &String,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<qm_corroboration::Choice, Failure> {
    check(cancelled)?;
    let fields = tree
        .as_object_mut()
        .ok_or_else(|| unavailable("nominal method envelope missing"))?;
    fields.insert("schema_version".into(), json!(8));
    fields.insert("backend".into(), json!("beat_this_qm_tempocnn"));
    fields.insert("policy".into(), json!("neural-qm-nominal-prior-v8"));
    fields.insert("status".into(), json!(status));
    fields.insert("bpm".into(), json!(bpm));
    fields.insert("nominal_selection".into(), selection);
    // Response/Python/decoded binary buffers have retired. Only the exact base
    // String, one augmented tree and final output String coexist now.
    let tree_bytes = budget::value_tree(tree).map_err(unavailable)?;
    let length = budget::serialized_len(tree).map_err(unavailable)?;
    let capacity = length
        .checked_mul(2)
        .ok_or_else(|| unavailable("nominal serialization overflow"))?;
    admit(&[
        source_live,
        raw_workspace,
        tree_bytes,
        capacity,
        base.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])?;
    check(cancelled)?;
    let method_json = serde_json::to_string(tree).map_err(unavailable)?;
    admit(&[
        source_live,
        raw_workspace,
        tree_bytes,
        method_json.capacity(),
        base.capacity(),
        grid_bytes,
        size_of::<LoadedAnalysis>(),
    ])?;
    check(cancelled)?;
    Ok(qm_corroboration::Choice { bpm, method_json })
}

fn request(
    profile: &Profile,
    context: &NeuralContext,
    rate: u32,
    frames: usize,
    mono: &Path,
    nn_mono: &str,
    directory: &Path,
) -> Result<Value, Failure> {
    Ok(json!({"schema_version":1,
        "identity":{"pad_id":context.identity.pad_id,"request_id":context.identity.request_id,
            "source_id":context.identity.source_id,"source_generation":context.identity.source_generation},
        "installation_root":profile.installation_root,"profile_sha256":profile.profile_sha256,
        "pcm":{"path":mono.to_str().ok_or_else(||unavailable("nominal mono path unavailable"))?,
            "source_sha256":context.descriptor["original"]["sha256"],"mono_sha256":nn_mono,
            "sample_rate_hz":rate,"frame_count":frames,"origin_seconds":0.0,"dtype":"float32-le",
            "channels":1,"downmix_revision":crate::audio_engine::analysis_pcm::MONO_RULE},
        "request_directory":directory.to_str().ok_or_else(||unavailable("nominal request directory unavailable"))?,
        "native_live_bytes":0,"aggregate_limit_bytes":LIMIT}))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn attempt(
    sample: &SampleBuffer,
    rate: u32,
    context: &NeuralContext,
    nn_mono: &str,
    decoder_identity: &str,
    mono: &Path,
    tree: &mut Value,
    base: &String,
    base_core: usize,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    range: Option<BpmRange>,
    cancellation: &Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<qm_corroboration::Choice, Failure> {
    let cancelled = || cancellation();
    check(&cancelled)?;
    eligible(tree, range).ok_or(Failure::NoChoice)?;
    let root = Path::new(&context.installation)
        .parent()
        .ok_or(Failure::NoChoice)?
        .join("metrical-prior");
    if !root
        .join("current.json")
        .try_exists()
        .map_err(|_| unavailable("nominal profile readiness unavailable"))?
    {
        return Err(Failure::NoChoice);
    }
    let shape = geometry(sample.frame_count() as u64, rate)?;
    admit(&[base_core, shape.bridge_bytes])?;
    let profile = probe_profile(&root, &cancelled)?;
    let parent = mono
        .parent()
        .ok_or_else(|| unavailable("nominal staging parent unavailable"))?;
    let directory = tempfile::Builder::new()
        .prefix(".nominal-")
        .tempdir_in(parent)
        .map_err(unavailable)?;
    let request = request(
        &profile,
        context,
        rate,
        sample.frame_count(),
        mono,
        nn_mono,
        directory.path(),
    )?;
    let result = transact(
        tree,
        base,
        base_core,
        source_live,
        raw_workspace,
        grid_bytes,
        range,
        shape,
        &profile,
        request,
        false,
        &cancelled,
        |work_core, primary| {
            qm_corroboration::observation(
                sample,
                rate,
                context,
                nn_mono,
                decoder_identity,
                work_core,
                &cancelled,
                |bpm, extent| {
                    if range.is_some_and(|r| !r.contains(bpm))
                        || candidate_class(bpm).is_none()
                        || candidate_class(bpm) == candidate_class(primary)
                    {
                        return Err(Failure::NoChoice);
                    }
                    bpm_range::half_double_drift(primary, bpm, extent)
                        .map_err(|_| Failure::NoChoice)
                },
            )
        },
        |request| invoke(request, shape, cancellation),
    );
    check(&cancelled)?;
    match (result, directory.close()) {
        (result, Ok(())) => result,
        (_, Err(_)) => Err(unavailable("nominal request cleanup unavailable")),
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn cached_component(
    sample: &SampleBuffer,
    rate: u32,
    context: &NeuralContext,
    nn_mono: &str,
    decoder_identity: &str,
    mono: &Path,
    tree: &mut Value,
    base: &String,
    base_core: usize,
    source_live: usize,
    raw_workspace: usize,
    grid_bytes: usize,
    token: Value,
    mut cached: Value,
) -> Result<qm_corroboration::Choice, Failure> {
    let profile: Profile = serde_json::from_value(token).map_err(unavailable)?;
    profile.validate()?;
    let shape = geometry(sample.frame_count() as u64, rate)?;
    let request = request(
        &profile,
        context,
        rate,
        sample.frame_count(),
        mono,
        nn_mono,
        mono.parent()
            .ok_or_else(|| unavailable("cached nominal parent missing"))?,
    )?;
    transact(
        tree,
        base,
        base_core,
        source_live,
        raw_workspace,
        grid_bytes,
        None,
        shape,
        &profile,
        request,
        true,
        &|| false,
        |work_core, primary| {
            qm_corroboration::observation(
                sample,
                rate,
                context,
                nn_mono,
                decoder_identity,
                work_core,
                &|| false,
                |bpm, extent| bpm_range::half_double_drift(primary, bpm, extent),
            )
        },
        |request| {
            // Only current wrapper/custody/resource context is bound here. The
            // presealed CDF, frontend and cached-execution proof stay unchanged.
            cached["identity"] = request["identity"].clone();
            let mut echo = request["pcm"].clone();
            echo.as_object_mut().unwrap().remove("path");
            cached["input"] = echo;
            cached["profile"] = profile.token();
            cached["prior"]["resources"]["native_live_bytes"] =
                request["native_live_bytes"].clone();
            Ok(cached)
        },
    )
}

#[cfg(test)]
#[path = "app_analysis_prior_tests.rs"]
mod tests;
