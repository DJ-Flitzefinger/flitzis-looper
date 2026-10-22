use super::*;

fn policy(sync: Option<bool>, realtime_bpm: Option<bool>, revision: u64) -> GlobalTimingPolicy {
    GlobalTimingPolicy::parse(Some(
        &serde_json::json!({
            "schema_version":1,"sync":sync,"realtime_bpm":realtime_bpm,"revision":revision,
        })
        .to_string(),
    ))
    .unwrap()
}

#[test]
fn strict_policy_requires_every_nullable_field_and_refuses_duplicates_and_wrong_types() {
    for encoded in [
        r#"{}"#,
        r#"{"schema_version":1,"realtime_bpm":false,"revision":0}"#,
        r#"{"schema_version":1,"sync":null,"revision":0}"#,
        r#"{"schema_version":true,"sync":null,"realtime_bpm":null,"revision":0}"#,
        r#"{"schema_version":1,"sync":0,"realtime_bpm":null,"revision":0}"#,
        r#"{"schema_version":1,"sync":true,"sync":false,"realtime_bpm":null,"revision":0}"#,
        r#"{"schema_version":1,"sync":null,"realtime_bpm":null,"revision":true}"#,
        r#"{"schema_version":1,"sync":null,"realtime_bpm":null,"revision":18446744073709551616}"#,
        r#"{"schema_version":2,"sync":null,"realtime_bpm":null,"revision":0}"#,
        r#"{"schema_version":1,"sync":null,"realtime_bpm":null,"revision":0,"extra":false}"#,
    ] {
        assert!(
            GlobalTimingPolicy::parse(Some(encoded)).is_err(),
            "{encoded}"
        );
    }
    assert!(GlobalTimingPolicy::parse(Some(&" ".repeat(1025))).is_err());
    assert_eq!(
        GlobalTimingPolicy::parse(None).unwrap(),
        policy(None, None, 0)
    );
}

#[test]
fn resolved_preferences_and_global_choices_keep_legacy_fixed_mapped_and_off_distinct() {
    for realtime in [Some(false), Some(true)] {
        assert_eq!(
            policy(Some(false), realtime, 1).request(true, 7).mode,
            SourceSyncMode::Off
        );
        assert_eq!(
            policy(Some(true), realtime, 1).request(false, 7).mode,
            SourceSyncMode::Off
        );
        assert_eq!(
            policy(None, realtime, 1).request(false, 7).mode,
            SourceSyncMode::Legacy
        );
    }
    assert_eq!(
        policy(Some(true), Some(false), 2).request(true, 0).mode,
        SourceSyncMode::Fixed
    );
    assert_eq!(
        policy(Some(true), Some(true), 3).request(true, 0).mode,
        SourceSyncMode::Mapped
    );
    assert_eq!(
        policy(None, None, 0).request(false, 0),
        SourceSyncRequest::default()
    );
    assert_eq!(
        policy(None, None, 0).request(true, 0).mode,
        SourceSyncMode::Mapped
    );
    assert_ne!(
        policy(Some(false), Some(false), 3).request(false, 0),
        policy(Some(false), Some(false), 5).request(false, 0)
    );
}

#[test]
fn explicit_fixed_requires_real_supported_ratio_and_off_ignores_legacy_bpm_matching() {
    let fixed = policy(Some(true), Some(false), 1).request(true, 0);
    assert_eq!(
        fixed.scalar_ratio(Some(0.5), 1.16, Some(0.4), false),
        Some(1.25)
    );
    for (source, master) in [
        (None, Some(0.4)),
        (Some(0.5), None),
        (Some(0.0), Some(0.4)),
        (Some(f64::NAN), Some(0.4)),
        (Some(4.0), Some(0.01)),
    ] {
        assert!(fixed.scalar_ratio(source, 1.0, master, true).is_none());
    }
    let off = policy(Some(false), Some(true), 2).request(true, 7);
    assert_eq!(
        off.scalar_ratio(Some(0.5), 1.16, Some(0.4), true),
        Some(1.16)
    );
    assert_eq!(
        SourceSyncRequest::default().scalar_ratio(Some(4.0), 1.0, Some(0.01), true),
        Some(SPEED_MAX)
    );
}

#[cfg(windows)]
mod callback;
#[cfg(windows)]
mod key_lock;
