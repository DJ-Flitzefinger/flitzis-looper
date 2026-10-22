//! Strict complete-project compatibility; no runtime policy or ownership is imported.

use super::*;

fn neutral() -> Value {
    json!({"schema_version":1,"sync":null,"realtime_bpm":null,"revision":0})
}

#[test]
fn global_timing_policy_project_omission_is_comparison_only_and_not_content() {
    let legacy = tests::complete_project();
    let raw = legacy.to_string();
    let digest = Sha256::digest(raw.as_bytes());
    let old_record = json!({"project_image_json":raw});
    let mut current = legacy.clone();
    current["global_timing_policy"] = neutral();
    let current_record = json!({"project_image_json":current.to_string()});
    assert_eq!(
        project(old_record["project_image_json"].as_str().unwrap()).unwrap(),
        legacy
    );
    assert_eq!(
        project_intent(Path::new("samples"), &old_record).unwrap(),
        project_intent(Path::new("samples"), &current_record).unwrap()
    );
    assert_eq!(
        Sha256::digest(
            old_record["project_image_json"]
                .as_str()
                .unwrap()
                .as_bytes()
        ),
        digest
    );
    current["global_timing_policy"] =
        json!({"schema_version":1,"sync":true,"realtime_bpm":false,"revision":u64::MAX});
    assert!(project(&current.to_string()).is_ok());
    assert_eq!(
        project_slot(&current, 0).unwrap(),
        project_slot(&legacy, 0).unwrap()
    );
    let changed_record = json!({"project_image_json":current.to_string()});
    assert_ne!(
        project_intent(Path::new("samples"), &old_record).unwrap(),
        project_intent(Path::new("samples"), &changed_record).unwrap()
    );
}

#[test]
fn global_timing_policy_complete_project_rejects_wrong_types_missing_and_extra_fields() {
    let invalid = [
        Value::Null,
        json!(true),
        json!({}),
        json!({"schema_version":1,"sync":null,"revision":0}),
        json!({"schema_version":true,"sync":null,"realtime_bpm":null,"revision":0}),
        json!({"schema_version":1.0,"sync":null,"realtime_bpm":null,"revision":0}),
        json!({"schema_version":2,"sync":null,"realtime_bpm":null,"revision":0}),
        json!({"schema_version":1,"sync":1,"realtime_bpm":false,"revision":0}),
        json!({"schema_version":1,"sync":false,"realtime_bpm":"false","revision":0}),
        json!({"schema_version":1,"sync":false,"realtime_bpm":null,"revision":true}),
        json!({"schema_version":1,"sync":false,"realtime_bpm":null,"revision":-1}),
        json!({"schema_version":1,"sync":false,"realtime_bpm":null,"revision":1.0}),
        json!({"schema_version":1,"sync":false,"realtime_bpm":null,"revision":0,"effective":true}),
    ];
    for policy in invalid {
        let mut candidate = tests::complete_project();
        candidate["global_timing_policy"] = policy;
        assert!(
            project(&candidate.to_string()).is_err(),
            "{}",
            candidate["global_timing_policy"]
        );
    }
}
