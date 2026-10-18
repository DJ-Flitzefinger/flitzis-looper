//! K13/K16-22 upper total: real black-key +6 and separately admitted offset +18
//! retain the accepted .5 -> 2 glide. Reuse the lower fixture's productive route,
//! independent original PCM/rate epochs and direct Native oracle. Synthetic
//! timing labels, complete mono input, host waits and observer time remain seams.

use super::lower_total_rate_glide_tests::{TotalGlideCase, total_glide};
use super::*;

const UPPER_CHILD: &str = "FLITZI_UPPER_TOTAL_PIANO_GLIDE_CHILD";
const UPPER_TEST: &str = "audio_engine::pitch_runtime_tests::piano_mouse_tests::piano_rate_controller_tests::piano_rate_glide_tests::upper_total_rate_glide_tests::upper_total_piano_offset_retrigger_keeps_original_public_glide";

fn upper_total(lock: bool, midi: bool) {
    let history = total_glide(
        lock,
        midi,
        TotalGlideCase {
            name: "upper",
            note: 6,
            extra: 18,
            total: 24,
            // A starts at total +6; B keeps its existing independent birth.
            birth_notes: [-12, -12],
            birth_reference: Reference::birth_captured_dynamic,
        },
    );
    assert_eq!(history.h, 4.0);
    // These are from the actual included independent first completed block and
    // original E timeline, never inferred from the worker's selected profile.
    assert_eq!(history.cold_pitch, if lock { 8.0 } else { 4.0 });
    assert!(history.endpoint_pitch > 2.0);
    if lock {
        assert!(history.bound > history.endpoint_bound);
        assert!(history.queried_pitches > 2);
        assert_eq!(history.h / GLIDE_SPEED, 2.0);
    } else {
        assert_eq!(history.endpoint_pitch, 4.0);
        assert_eq!(history.bound, history.endpoint_bound);
        assert_eq!(history.queried_pitches, 1);
    }
}

#[test]
fn upper_total_piano_offset_retrigger_keeps_original_public_glide() {
    if let Ok(child) = std::env::var(UPPER_CHILD) {
        match child.as_str() {
            "gui-off" => upper_total(false, false),
            "gui-on" => upper_total(true, false),
            "midi-off" => upper_total(false, true),
            "midi-on" => upper_total(true, true),
            _ => panic!("unknown declared upper-total piano/glide child"),
        }
        return;
    }
    for child in ["gui-off", "gui-on", "midi-off", "midi-on"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", UPPER_TEST, "--nocapture", "--test-threads=1"])
            .env(UPPER_CHILD, child)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "isolated upper-total piano/glide {child} failed:\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains("1 passed; 0 failed"));
        assert!(stderr.contains("upper_total_piano_glide_history"));
        assert!(stderr.contains("piano_rate_glide_first_sample"));
        print!("{stdout}");
        eprint!("{stderr}");
    }
}
