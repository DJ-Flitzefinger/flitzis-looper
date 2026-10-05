//! Bounded diagnostic prediction validation, outside the realtime audio path.
//!
//! Schema 2 preserves every IEEE-754 binary64 value in uncompressed inline base64.
//! Validation borrows the envelope and never replaces it with expanded JSON arrays.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

const MAX_PREDICTION_COUNT: usize = 250_000;
const FLOAT_BYTES: usize = size_of::<f64>();
const MAX_ARRAY_BYTES: usize = MAX_PREDICTION_COUNT * FLOAT_BYTES;
const MAX_ARRAY_ENCODED_BYTES: usize = MAX_ARRAY_BYTES.div_ceil(3) * 4;
const ARRAY_NAMES: [&str; 4] = [
    "beat_seconds",
    "downbeat_seconds",
    "beat_logits",
    "downbeat_logits",
];

pub(super) fn validate_predictions(
    predictions: &Value,
    schema_version: u64,
    duration: f64,
) -> Result<(), String> {
    if schema_version == 2 {
        let fields = predictions
            .as_object()
            .ok_or("missing packed beat predictions")?;
        if fields.len() != ARRAY_NAMES.len() + 1 || predictions["encoding"] != "float64-le/base64" {
            return Err("invalid packed beat prediction encoding/fields".into());
        }
    }
    let mut logit_len = None;
    for name in ARRAY_NAMES {
        let source_times = name.ends_with("seconds");
        let count = if schema_version == 2 {
            validate_packed_array(&predictions[name], source_times, duration)?
        } else {
            validate_legacy_array(&predictions[name], source_times, duration)?
        };
        if !source_times {
            if logit_len.is_some_and(|length| length != count) {
                return Err("beat logit count mismatch".into());
            }
            logit_len = Some(count);
        }
    }
    Ok(())
}

fn validate_packed_array(
    array: &Value,
    source_times: bool,
    duration: f64,
) -> Result<usize, String> {
    let encoded = array
        .as_str()
        .ok_or("missing packed beat prediction array")?;
    // Check the encoded extent before the decoder allocates. The enclosing
    // native envelope has its separate, unchanged 1-MiB limit.
    if encoded.len() > MAX_ARRAY_ENCODED_BYTES {
        return Err("beat prediction limit exceeded".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "invalid packed beat prediction base64")?;
    if bytes.len() > MAX_ARRAY_BYTES {
        return Err("beat prediction limit exceeded".into());
    }
    if !bytes.len().is_multiple_of(FLOAT_BYTES) {
        return Err("partial packed beat prediction value".into());
    }
    let mut previous = -1.0_f64;
    for chunk in bytes.chunks_exact(FLOAT_BYTES) {
        let value = f64::from_le_bytes(chunk.try_into().expect("complete binary64 chunk"));
        validate_value(value, source_times, duration, &mut previous)?;
    }
    Ok(bytes.len() / FLOAT_BYTES)
}

fn validate_legacy_array(
    array: &Value,
    source_times: bool,
    duration: f64,
) -> Result<usize, String> {
    let values = array.as_array().ok_or("missing beat prediction array")?;
    if values.len() > MAX_PREDICTION_COUNT {
        return Err("beat prediction limit exceeded".into());
    }
    let mut previous = -1.0_f64;
    for value in values {
        let value = value.as_f64().ok_or("nonfinite beat prediction")?;
        validate_value(value, source_times, duration, &mut previous)?;
    }
    Ok(values.len())
}

fn validate_value(
    value: f64,
    source_times: bool,
    duration: f64,
    previous: &mut f64,
) -> Result<(), String> {
    if !value.is_finite() {
        return Err("nonfinite beat prediction".into());
    }
    if source_times && (value < 0.0 || value >= duration || value <= *previous) {
        return Err("invalid beat source positions".into());
    }
    *previous = value;
    Ok(())
}

#[cfg(test)]
#[path = "analysis_prediction_tests.rs"]
mod tests;
