//! Separate dormant FullMix intent from an actually rendered component mask.
use super::*;

#[test]
fn full_mix_saved_mask_zero_accepts_real_cold_default_without_rewriting_audio() {
    isolated(
        "bank_public_tests::recovered_bank_tests::parked_mask_tests::full_mix_saved_mask_zero_accepts_real_cold_default_without_rewriting_audio",
        || {
            let mut fixture = recovery_fixture(false, true);
            put(&mut fixture, 36);
            let before = fixture.callback.mixer.stem_demand_for_measurement(36);
            assert_eq!(before.0, StemMixMode::FullMix);
            assert_eq!(
                before.1, 15,
                "ordinary Cold restore owns its genuine default"
            );
            // The original saved image already has zero. Do not modify its
            // musical intent to imitate the unrelated fresh callback default.
            let saved = fixture.envelope();
            let data: Value = serde_json::from_str(&saved).unwrap();
            assert_eq!(data["snapshot"]["stem_intent"]["enabled_mask"], json!(0));
            let mut expected = vec![None; 36];
            expected[0] = Some(saved);
            let original = view(&fixture, 1);
            let before_slots = snapshots(&fixture);
            let acceptance = accept(&fixture, &original, expected);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(status(&acceptance)["status"], json!("accepted"));
            fixture.with_engine(|engine| {
                assert!(engine.validate_recovered_bank(&acceptance).unwrap())
            });
            assert_eq!(
                fixture.callback.mixer.stem_demand_for_measurement(36),
                before
            );
            unchanged(&fixture, &before_slots);
            release(&fixture, &acceptance);
        },
    );
}

#[test]
fn all_stems_saved_mask_mismatch_still_requires_real_restore() {
    isolated(
        "bank_public_tests::recovered_bank_tests::parked_mask_tests::all_stems_saved_mask_mismatch_still_requires_real_restore",
        || {
            let mut fixture = recovery_fixture(true, true);
            let saved = restored_image(&fixture);
            let registration = put_selected_image(&mut fixture, 36, &saved);
            let before = fixture.callback.mixer.stem_demand_for_measurement(36);
            assert_eq!(before.0, StemMixMode::AllStems);
            let mut desired: Value = serde_json::from_str(&saved).unwrap();
            desired["snapshot"]["stem_intent"]["enabled_mask"] = json!(before.1 ^ 1);
            let mut expected = vec![None; 36];
            expected[0] = Some(desired.to_string());
            let original = view(&fixture, 1);
            let acceptance = accept(&fixture, &original, expected);
            assert_eq!(fixture.callback.drain(), 1);
            assert_eq!(status(&acceptance)["status"], json!("waiting_restore"));
            fixture.with_engine(|engine| {
                assert!(!engine.validate_recovered_bank(&acceptance).unwrap())
            });
            assert!(registration.is_current());
            assert_eq!(
                fixture.callback.mixer.stem_demand_for_measurement(36),
                before
            );
            release(&fixture, &acceptance);
        },
    );
}
