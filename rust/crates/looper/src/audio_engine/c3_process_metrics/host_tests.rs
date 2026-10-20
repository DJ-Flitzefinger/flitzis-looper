//! Instrumentation only. No model/process admission or memory-pressure policy.
use super::*;

#[test]
fn actual_system_commit_snapshot_has_checked_bytes_and_caller_scope() {
    let value = host_snapshot().unwrap();
    let total = value["system_commit_total_bytes"].as_u64().unwrap();
    let limit = value["system_commit_limit_bytes"].as_u64().unwrap();
    let available = value["system_available_commit_bytes"].as_u64().unwrap();
    assert_eq!(total.checked_add(available), Some(limit));
    assert!(limit > 0);
    assert!(value["commit_limit"].as_u64().unwrap() > 0);
    assert!(value["available_commit"].is_u64());
    assert!(value["observed_at_unix_ns"].as_u64().unwrap() > 0);
    assert!(value["observation_span_ns"].is_u64());
    assert!(
        value["caller_commit_scope"]
            .as_str()
            .unwrap()
            .contains("caller")
    );
}

#[test]
fn commit_page_conversion_rejects_unknown_invalid_and_overflowing_geometry() {
    assert_eq!(commit_bytes(3, 9, 4096).unwrap(), (12288, 36864, 24576));
    assert!(commit_bytes(0, 0, 0).is_err());
    assert!(commit_bytes(9, 3, 4096).is_err());
    assert!(commit_bytes(0, usize::MAX, 2).is_err());
}

#[test]
fn failed_host_query_propagates_without_manufacturing_zero_headroom() {
    let result = host_snapshot_with(|_, _| Err("injected GetPerformanceInfo failure".into()));
    assert_eq!(result.unwrap_err(), "injected GetPerformanceInfo failure");
    assert!(
        query_succeeded("injected Win32 query", 0)
            .unwrap_err()
            .contains("injected Win32 query")
    );
    assert!(query_succeeded("injected successful query", 1).is_ok());
}

#[test]
fn unknown_job_membership_and_limit_queries_never_become_unlimited_context() {
    let unknown = current_job_context_with(
        || Err("injected membership failure".into()),
        || panic!("limits must not be queried after unknown membership"),
    );
    assert_eq!(unknown["status"], "unknown");
    assert!(unknown["in_job"].is_null());
    assert_eq!(unknown["error"], "injected membership failure");
    assert!(unknown["limit_flags"].is_null());

    let limits = current_job_context_with(
        || Ok(true),
        || Err("injected current-Job query failure".into()),
    );
    assert_eq!(limits["status"], "limits_unknown");
    assert_eq!(limits["in_job"], true);
    assert!(limits["job_memory_limit_bytes"].is_null());

    let absent = current_job_context_with(
        || Ok(false),
        || panic!("no Job limits may be queried for a nonmember"),
    );
    assert_eq!(absent["status"], "not_in_job");
    assert_eq!(absent["in_job"], false);
}

#[test]
fn queried_job_limits_remain_raw_metadata_with_no_ancestor_claim() {
    let value = current_job_context_with(
        || Ok(true),
        || {
            let mut limits = JobExtendedLimits::default();
            limits.basic.flags = 0x2008;
            limits.basic.active_process_limit = 64;
            limits.process_memory_limit = 123;
            limits.job_memory_limit = 456;
            Ok(limits)
        },
    );
    assert_eq!(value["limit_flags"], 0x2008);
    assert_eq!(value["active_process_limit"], 64);
    assert_eq!(value["process_memory_limit_bytes"], 123);
    assert_eq!(value["job_memory_limit_bytes"], 456);
    assert!(value["scope"].as_str().unwrap().contains("not enumerated"));
}

#[test]
fn actual_host_and_current_job_queries_preserve_process_handle_plateau() {
    // Run this isolated counter test serially, with no other same-PID owners.
    // The actual query paths use pseudo/NULL handles and create none to close.
    host_snapshot().unwrap();
    let _ = current_job_context();
    let before = snapshot();
    for _ in 0..64 {
        host_snapshot().unwrap();
        let context = current_job_context();
        assert!(matches!(
            context["status"].as_str().unwrap(),
            "queried" | "not_in_job" | "unknown" | "limits_unknown"
        ));
    }
    let after = snapshot();
    assert_eq!(after["handle_count"], before["handle_count"]);
    // Existing snapshot/delta remains process-only and entirely numeric.
    assert!(before.get("system_commit_total_bytes").is_none());
    assert!(delta(&before, &after).is_object());
}
