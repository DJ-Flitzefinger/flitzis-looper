use super::*;
use serde_json::json;

fn encoded(values: &[f64]) -> String {
    STANDARD.encode(
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

fn packed() -> Value {
    json!({
        "encoding": "float64-le/base64",
        "beat_seconds": encoded(&[0.0, 0.001, 0.02]),
        "downbeat_seconds": encoded(&[0.001]),
        "beat_logits": encoded(&[-0.0, f64::from_bits(1), f64::MAX, 1.0000000000000002]),
        "downbeat_logits": encoded(&[0.0, f64::MIN_POSITIVE, -f64::MAX, -0.5]),
    })
}

#[test]
fn packed_binary64_values_validate_without_mutating_the_wire_payload() {
    let predictions = packed();
    let original = predictions.clone();
    validate_predictions(&predictions, 2, 0.021).unwrap();
    assert_eq!(predictions, original);
    let bytes = STANDARD
        .decode(predictions["beat_logits"].as_str().unwrap())
        .unwrap();
    let got: Vec<_> = bytes
        .chunks_exact(8)
        .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap()).to_bits())
        .collect();
    assert_eq!(
        got,
        [
            (-0.0_f64).to_bits(),
            1,
            f64::MAX.to_bits(),
            1.0000000000000002_f64.to_bits()
        ]
    );
}

#[test]
fn packed_base64_requires_canonical_padding_and_complete_values() {
    for invalid in [
        "AAAAAAAAAAA",      // Missing padding.
        "AAAAAAAAAAAA=",    // Extra padding.
        "AAAAAAAAAAB=",     // Nonzero unused bits.
        "AAAAAAAAAAA=AAAA", // Bytes after padding.
        "AAAAAAAAAAA=\n",   // Whitespace.
        "____________",     // URL-safe alphabet.
        "!AAAAAAAAAA=",
        "AA==", // Complete base64, partial binary64.
    ] {
        let mut predictions = packed();
        predictions["beat_logits"] = json!(invalid);
        assert!(
            validate_predictions(&predictions, 2, 1.0).is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn packed_predictions_reject_wrong_encoding_shape_and_unexpected_fields() {
    let original = packed();
    for encoding in [json!("float32-le/base64"), json!(null), json!(1)] {
        let mut predictions = original.clone();
        predictions["encoding"] = encoding;
        assert!(validate_predictions(&predictions, 2, 1.0).is_err());
    }
    let mut extra = original.clone();
    extra["counts"] = json!([3, 1, 4, 4]);
    assert!(validate_predictions(&extra, 2, 1.0).is_err());
    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("downbeat_seconds");
    assert!(validate_predictions(&missing, 2, 1.0).is_err());
    let mut wrong_type = original;
    wrong_type["beat_seconds"] = json!([0.0, 0.001, 0.02]);
    assert!(validate_predictions(&wrong_type, 2, 1.0).is_err());
}

#[test]
fn packed_predictions_reject_all_nonfinite_values_and_invalid_source_positions() {
    for name in ARRAY_NAMES {
        for nonfinite in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut predictions = packed();
            predictions[name] = json!(encoded(&[nonfinite]));
            assert!(
                validate_predictions(&predictions, 2, 1.0).is_err(),
                "{name}"
            );
        }
    }
    for name in ["beat_seconds", "downbeat_seconds"] {
        for invalid in [&[-0.001][..], &[1.0], &[0.001, 0.0], &[0.001, 0.001]] {
            let mut predictions = packed();
            predictions[name] = json!(encoded(invalid));
            assert!(
                validate_predictions(&predictions, 2, 1.0).is_err(),
                "{name} {invalid:?}"
            );
        }
    }
}

#[test]
fn packed_predictions_require_equal_logit_counts_but_allow_empty_arrays() {
    let mut predictions = packed();
    predictions["downbeat_logits"] = json!(encoded(&[0.0]));
    assert_eq!(
        validate_predictions(&predictions, 2, 1.0).unwrap_err(),
        "beat logit count mismatch"
    );
    for name in ARRAY_NAMES {
        predictions[name] = json!("");
    }
    validate_predictions(&predictions, 2, 1.0).unwrap();
}

#[test]
fn packed_prediction_count_and_encoded_extent_are_bounded() {
    let largest = json!(encoded(&vec![0.0; MAX_PREDICTION_COUNT]));
    assert_eq!(
        validate_packed_array(&largest, false, 1.0).unwrap(),
        MAX_PREDICTION_COUNT
    );
    let excessive = json!(encoded(&vec![0.0; MAX_PREDICTION_COUNT + 1]));
    assert_eq!(
        validate_packed_array(&excessive, false, 1.0).unwrap_err(),
        "beat prediction limit exceeded"
    );
    let excessive_extent = json!("!".repeat(MAX_ARRAY_ENCODED_BYTES + 1));
    assert_eq!(
        validate_packed_array(&excessive_extent, false, 1.0).unwrap_err(),
        "beat prediction limit exceeded"
    );
}

#[test]
fn legacy_arrays_retain_finite_position_and_equal_count_validation() {
    let predictions = json!({"beat_seconds": [0.0, 0.5], "downbeat_seconds": [0.0],
        "beat_logits": [-0.1, 0.7], "downbeat_logits": [0.0, 0.4]});
    validate_predictions(&predictions, 1, 1.0).unwrap();
    assert!(validate_predictions(&predictions, 2, 1.0).is_err());
    assert!(validate_predictions(&packed(), 1, 1.0).is_err());
    for (name, values) in [
        ("beat_seconds", json!([0.5, 0.5])),
        ("downbeat_seconds", json!([1.0])),
        ("beat_logits", json!([null, 0.0])),
        ("downbeat_logits", json!([])),
    ] {
        let mut invalid = predictions.clone();
        invalid[name] = values;
        assert!(validate_predictions(&invalid, 1, 1.0).is_err());
    }
}
