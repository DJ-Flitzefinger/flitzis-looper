use super::*;
use serde_json::{Value, json};

#[test]
fn json_shape_pinned_value_vec_string_and_sorted_map_layout_matches_counter_assumptions() {
    assert_eq!(size_of::<Value>(), 32);
    assert_eq!(size_of::<String>(), 24);
    assert_eq!(size_of::<usize>(), 8);
    assert_eq!(
        size_of::<serde_json::Map<String, Value>>(),
        size_of::<std::collections::BTreeMap<String, Value>>()
    );
    // Read actual tiny allocations; never construct the reported 14-MiB case.
    let dense: Value = serde_json::from_str("[[0],[0],[0],[0],[0]]").unwrap();
    let outer = dense.as_array().unwrap();
    assert_eq!(outer.capacity(), 8);
    for child in outer {
        assert_eq!(child.as_array().unwrap().capacity(), 4);
    }
    let mut map = serde_json::Map::new();
    map.insert("z".into(), Value::Null);
    map.insert("a".into(), Value::Null);
    assert_eq!(
        map.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a", "z"]
    );
    let borrowed = value_tree(&dense).unwrap();
    assert!(borrowed >= 32 + 8 * 32 + 5 * 4 * 32);
}

#[test]
fn streaming_json_shape_refuses_scaled_dense_arrays_and_tiny_objects_that_old_32x_admits() {
    let count = 1024;
    let dense = format!("[{}]", vec!["[0]"; count].join(","));
    let tiny_objects = format!("[{}]", vec![r#"{"a":0}"#; count].join(","));
    let scaled_bytes = 14 * 1024 * 1024;
    assert!(peak(scaled_bytes, 0, 0, index_bound(scaled_bytes).unwrap(), 0).is_ok());
    for encoded in [&dense, &tiny_objects] {
        let tree = scan(encoded, 0, 0).unwrap();
        let copies = scaled_bytes / encoded.len();
        let scaled_tree = tree.checked_mul(copies).unwrap();
        assert!(tree_admission(scaled_bytes, scaled_tree, 0, 0).is_err());
        let actual: Value = serde_json::from_str(encoded).unwrap();
        assert!(tree >= 32 + actual.as_array().unwrap().capacity() * 32);
        assert!(value_tree(&actual).unwrap() >= tree);
    }
}

#[test]
fn streaming_shape_prefix_admission_runs_before_typed_values_and_charges_all_context() {
    let encoded = format!(
        r#"{{"lineage":[{{"payload":{{"dense":[{}]}}}}]}}"#,
        vec!["[0]"; 1024].join(",")
    );
    let tree = scan(&encoded, 0, 0).unwrap();
    let peak = tree_admission(encoded.len(), tree, 0, 0).unwrap();
    let held = MAX_BUILD_BYTES - peak;
    assert!(scan(&encoded, held, 0).is_ok());
    assert!(scan(&encoded, held + 1, 0).unwrap_err().contains("512-MiB"));
    assert!(scan(&encoded, held, 1).unwrap_err().contains("512-MiB"));
    // This is deliberately not an Envelope. A typed parse would complain about
    // fields; resource refusal must instead happen in its preceding scanner.
    let error = crate::audio_engine::source_beatmap::SourceBeatmap::parse_with_context(
        &encoded,
        held + 1,
        0,
    )
    .unwrap_err();
    assert!(error.contains("512-MiB"));
    assert!(tree_admission(1, usize::MAX, 0, 0).is_err());
    assert!(scan(&encoded, usize::MAX, 0).is_err());
    assert!(scan("[] trailing", 0, 0).is_err());
    assert!(scan(&format!("{}0{}", "[".repeat(129), "]".repeat(129)), 0, 0).is_err());
}

#[test]
fn borrowed_growth_admission_counts_dense_old_inverse_before_any_copy_and_preserves_raw_floor() {
    let payload = json!({"inverse":{"positions":(0..1024).map(|_|json!([0])).collect::<Vec<_>>()}});
    let serialized = serialized_len(&payload).unwrap();
    let base_tree = value_tree(&payload).unwrap();
    let inverse_tree = value_tree(&payload["inverse"]).unwrap();
    let base_peak = tree_admission(serialized, base_tree, 0, 0).unwrap();
    let held = MAX_BUILD_BYTES - base_peak;
    assert!(tree_admission(serialized, base_tree, held, 0).is_ok());
    assert!(
        tree_admission(
            serialized,
            sum(&[base_tree, inverse_tree]).unwrap(),
            held,
            0
        )
        .is_err()
    );
    let dense = json!({"payload":payload});
    assert!(value_admission(&dense, MAX_BUILD_BYTES, 0).is_err());
    assert_eq!(
        peak(4096, 8192, 2, 0, 0).unwrap(),
        4096 * 32 + 8192 + 2 * 8192 + FIXED_BYTES
    );
}
