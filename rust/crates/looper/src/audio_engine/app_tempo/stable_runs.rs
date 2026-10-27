//! Recover stable excerpts without interpreting extra/missing detector events as beats.
use super::{RegionEstimate, TempoEstimate, region};

const MAX_RUNS: usize = 64;

pub(super) fn recover(beats: &[f64], duration: f64, result: &mut TempoEstimate) {
    let Some(bounds) = candidate_bounds(beats, duration) else {
        return;
    };
    recover_candidates(beats, duration, bounds, result);
}

pub(super) fn is_candidate(beats: &[f64], duration: f64, window: [f64; 2]) -> bool {
    candidate_bounds(beats, duration).is_some_and(|bounds| bounds.contains(&window))
}

fn candidate_bounds(beats: &[f64], duration: f64) -> Option<Vec<[f64; 2]>> {
    let mut bounds = Vec::with_capacity(MAX_RUNS);
    let mut start = 0;
    // Adjacent IOI scale changes discover candidates only. Each candidate still
    // needs the unchanged strict original-ordinal fit and independent support.
    for end in 2..=beats.len() {
        let boundary = end == beats.len()
            || !(2.0 / 3.0..=1.5)
                .contains(&((beats[end] - beats[end - 1]) / (beats[end - 1] - beats[end - 2])));
        if boundary {
            if end - start >= 24 && beats[end - 1] - beats[start] >= 30.0 {
                if bounds.len() == MAX_RUNS {
                    // Never hide a conflicting 65th candidate by truncation.
                    return None;
                }
                bounds.push([beats[start], beats[end - 1].next_up().min(duration)]);
            }
            start = end;
        }
    }
    Some(bounds)
}

fn recover_candidates(
    beats: &[f64],
    duration: f64,
    bounds: Vec<[f64; 2]>,
    result: &mut TempoEstimate,
) {
    let mut runs: Vec<RegionEstimate> = bounds
        .into_iter()
        .filter_map(|w| region(beats, w[0], w[1], 24, 30.0))
        .collect();
    runs.sort_by(|a, b| {
        let span = |r: &RegionEstimate| r.window_seconds[1] - r.window_seconds[0];
        span(b)
            .total_cmp(&span(a))
            .then(a.window_seconds[0].total_cmp(&b.window_seconds[0]))
    });
    let Some(selected) = runs.first().cloned().filter(|_| runs.len() >= 2) else {
        return;
    };
    // Runs are disjoint by construction. Conservatively refuse competing stable
    // tempi, including half/double tempo, rather than choosing a convenient family.
    let consistent = runs
        .iter()
        .chain(&result.regions)
        .all(|r| (r.bpm / selected.bpm - 1.0).abs() <= 0.00025);
    let conflict = runs
        .iter()
        .position(|r| (r.bpm / selected.bpm - 1.0).abs() > 0.00025);
    if consistent {
        result.status = "estimated";
        result.bpm = Some(selected.bpm);
        result.selected = Some(selected);
    } else {
        result.status = "uncorroborated";
    }
    // Keep any original common-middle fit for the unchanged integer counterprobe.
    // At most four original windows exist; retain at least the two run witnesses.
    let mut published = vec![runs[0].clone(), runs[conflict.unwrap_or(1)].clone()];
    if let Some(common) = result
        .regions
        .iter()
        .find(|r| r.window_seconds == [0.2 * duration, 0.8 * duration])
    {
        published.push(common.clone());
    }
    for r in &result.regions {
        if published.len() < 5
            && !published
                .iter()
                .any(|p| p.window_seconds == r.window_seconds)
        {
            published.push(r.clone());
        }
    }
    result.regions = published;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_engine::app_tempo::estimate;

    fn mixed_subdivisions() -> Vec<f64> {
        let mut beats = Vec::new();
        for i in 0..300 {
            let t = 0.02 + i as f64 * (2.0 / 3.0);
            beats.push((t / 0.02).round() * 0.02);
            if (50..90).contains(&i) || (220..260).contains(&i) {
                beats.push(((t + 1.0 / 3.0) / 0.02).round() * 0.02);
            }
        }
        beats
    }

    #[test]
    fn extra_subdivisions_do_not_destroy_two_strict_original_ordinal_runs() {
        let beats = mixed_subdivisions();
        let original = beats.clone();
        let result = estimate(&beats, 201.0);
        assert_eq!(result.status, "estimated");
        assert!((result.bpm.unwrap() - 90.0).abs() < 0.01);
        let selected = result.selected.unwrap();
        assert!(selected.retained >= 24);
        assert!(selected.window_seconds[1] - selected.window_seconds[0] >= 30.0);
        let probe = result.integer_probe.unwrap();
        assert_eq!(probe.evidence_tier, "two_ten_second");
        assert_eq!(probe.fractional_check.status, "pass");
        assert_ne!(probe.status, "inconclusive");
        assert_eq!(beats, original);
        assert!(result.regions.iter().any(|r| r.window_seconds[0] > 60.0));
        assert!(result.regions.iter().any(|r| r.window_seconds[0] < 1.0));
    }

    #[test]
    fn competing_disjoint_stable_tempi_and_too_many_runs_fail_closed() {
        for count in [4, 65] {
            let mut beats = Vec::new();
            for run in 0..count {
                let period = if run % 2 == 0 { 2.0 / 3.0 } else { 1.0 / 3.0 };
                let start = run as f64 * 40.0 + 0.02;
                beats.extend(
                    (0..)
                        .map(|i| start + i as f64 * period)
                        .take_while(|t| *t < start + 35.0),
                );
            }
            let mut result = estimate(&[], count as f64 * 40.0);
            recover(&beats, count as f64 * 40.0, &mut result);
            assert!(result.bpm.is_none());
            assert!(result.selected.is_none());
            if count == 4 {
                assert_eq!(result.status, "uncorroborated");
            } else {
                assert!(result.regions.is_empty());
            }
        }
    }

    #[test]
    fn one_run_cannot_corroborate_itself_or_repair_irregular_events() {
        for beats in [
            (0..60).map(|i| i as f64 * 0.65).collect::<Vec<_>>(),
            (0..250)
                .map(|i| i as f64 * 0.5 + 0.0001 * (i as f64).powi(2))
                .collect(),
        ] {
            let mut result = estimate(&[], 200.0);
            recover(&beats, 200.0, &mut result);
            assert!(result.bpm.is_none());
        }
    }

    #[test]
    fn conflicting_shorter_run_remains_visible_in_unavailable_metadata() {
        let mut beats = Vec::new();
        for (start, span, period) in [
            (0.02, 50.0, 2.0 / 3.0),
            (60.02, 45.0, 2.0 / 3.0),
            (120.02, 35.0, 1.0 / 3.0),
        ] {
            beats.extend(
                (0..)
                    .map(|i| start + i as f64 * period)
                    .take_while(|t| *t < start + span),
            );
        }
        let mut result = estimate(&[], 160.0);
        recover(&beats, 160.0, &mut result);
        assert!(result.bpm.is_none());
        assert!(result.regions.iter().any(|r| (r.bpm - 90.0).abs() < 1e-8));
        assert!(result.regions.iter().any(|r| (r.bpm - 180.0).abs() < 1e-8));
    }

    fn close_metadata(actual: &serde_json::Value, saved: &serde_json::Value) {
        use serde_json::Value;
        match (actual, saved) {
            (Value::Number(a), Value::Number(b)) if a.is_f64() && b.is_f64() => {
                // Saved JSON roundtrips can move residual diagnostics by one ULP.
                assert!((a.as_f64().unwrap() - b.as_f64().unwrap()).abs() <= 1e-12);
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (x, y) in a.iter().zip(b) {
                    close_metadata(x, y);
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.len(), b.len());
                for (key, value) in a {
                    close_metadata(value, b.get(key).expect("matching metadata key"));
                }
            }
            _ => assert_eq!(actual, saved),
        }
    }

    #[test]
    fn successful_integer_fractional_and_double_tempo_paths_remain_unchanged() {
        for bpm in [94.0, 91.892_207_568_556_94, 180.0] {
            let beats: Vec<_> = (0..)
                .map(|i| 0.07 + i as f64 * 60.0 / bpm)
                .take_while(|t| *t < 201.0)
                .collect();
            let result = estimate(&beats, 201.0);
            assert_eq!(result.status, "estimated");
            assert!((result.bpm.unwrap() - bpm).abs() < 1e-10);
            assert_eq!(result.selected.unwrap().window_seconds, [40.2, 160.8]);
        }
    }

    #[test]
    #[ignore = "authorized source-bound Human raw observations supplied outside Git"]
    fn authorized_three_pad_tempo_replay_preserves_success_and_recovers_48() {
        use serde_json::{Value, json};
        use std::{fs, path::PathBuf};
        let root =
            PathBuf::from(std::env::var_os("FLITZI_TEMPO_REPLAY_ROOT").expect("owned replay root"));
        let baseline = PathBuf::from(
            std::env::var_os("FLITZI_TEMPO_BASELINE_ROOT").expect("retained v3 baseline root"),
        );
        assert!(root.is_absolute());
        assert!(baseline.is_absolute());
        let mut reports = Vec::new();
        for pad in 1..=3 {
            let original: Value = serde_json::from_slice(
                &fs::read(root.join(format!("input/pad{pad}-method.json"))).unwrap(),
            )
            .unwrap();
            let beats: Vec<_> = original["raw_beats"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let duration = original["frame_count"].as_f64().unwrap()
                / original["sample_rate_hz"].as_f64().unwrap();
            let result = estimate(&beats, duration);
            let previous: Value = serde_json::from_slice(
                &fs::read(baseline.join(format!("pad{pad}-v3-method.json"))).unwrap(),
            )
            .unwrap();
            assert_eq!(previous["source_sha256"], original["source_sha256"]);
            assert_eq!(previous["raw_beats"], original["raw_beats"]);
            assert_eq!(previous["raw_downbeats"], original["raw_downbeats"]);
            assert!(
                result
                    .bpm
                    .unwrap()
                    .to_bits()
                    .abs_diff(previous["bpm"].as_f64().unwrap().to_bits())
                    <= 4
            );
            let probe = result.integer_probe.as_ref().unwrap();
            assert_eq!(
                result.bpm,
                Some(if probe.status == "supported_integer_estimate" {
                    probe.candidate_bpm
                } else {
                    result.selected.as_ref().unwrap().bpm
                })
            );
            close_metadata(
                &serde_json::to_value(&result.regions).unwrap(),
                &previous["regions"],
            );
            close_metadata(
                &serde_json::to_value(&result.selected).unwrap(),
                &previous["selected"],
            );
            if pad < 3 {
                assert_eq!(result.bpm, original["bpm"].as_f64());
                close_metadata(
                    &serde_json::to_value(&result.regions).unwrap(),
                    &original["regions"],
                );
                close_metadata(
                    &serde_json::to_value(&result.selected).unwrap(),
                    &original["selected"],
                );
            } else {
                assert_eq!(original["status"], "insufficient");
                assert_eq!(result.status, "estimated");
                assert!((89.97..89.99).contains(&result.bpm.unwrap()));
                let probe = result.integer_probe.as_ref().unwrap();
                assert_eq!(probe.status, "fractional_estimate");
                assert_eq!(probe.evidence_tier, "two_ten_second");
                assert_eq!(probe.fractional_check.status, "pass");
                assert!(probe.windows.iter().all(|w| {
                    let retained: Vec<_> = beats
                        .iter()
                        .enumerate()
                        .filter(|(i, t)| {
                            **t >= w.window_seconds[0]
                                && **t < w.window_seconds[1]
                                && w.excluded_raw_indices.binary_search(i).is_err()
                        })
                        .collect();
                    retained.last().unwrap().1 - retained.first().unwrap().1 >= 10.0
                }));
                assert!(result.regions.len() >= 2);
            }
            let mut method = original.clone();
            let update = serde_json::to_value(&result).unwrap();
            method
                .as_object_mut()
                .unwrap()
                .extend(update.as_object().unwrap().clone());
            method["schema_version"] = json!(5);
            assert_eq!(method["raw_beats"], original["raw_beats"]);
            assert_eq!(method["raw_downbeats"], original["raw_downbeats"]);
            fs::write(
                root.join(format!("pad{pad}-v5-method.json")),
                serde_json::to_vec_pretty(&method).unwrap(),
            )
            .unwrap();
            reports.push(
                json!({"pad":pad,"source_sha256":original["source_sha256"],"estimate":result}),
            );
        }
        fs::write(
            root.join("tempo-replay-v5.json"),
            serde_json::to_vec_pretty(&reports).unwrap(),
        )
        .unwrap();
    }

    #[test]
    #[ignore = "authorized retained GPU-FLAC observations; numeric replay without inference or App"]
    fn authorized_gpu_flac_corroborated_hull_replay_preserves_the_exact_selected_base() {
        use serde_json::{Value, json};
        use sha2::{Digest, Sha256};
        use std::{fs, path::PathBuf};
        let input = PathBuf::from(
            std::env::var_os("FLITZI_CORROBORATED_HULL_SOURCE_REPORT")
                .expect("retained authorized report"),
        );
        let root = PathBuf::from(
            std::env::var_os("FLITZI_CORROBORATED_HULL_REPLAY_ROOT")
                .expect("owned replay output root"),
        );
        assert!(input.is_absolute() && root.is_absolute());
        let input = input.canonicalize().unwrap();
        let output = root
            .canonicalize()
            .unwrap()
            .join("source-bound-flac-v5.json");
        assert_ne!(input, output);
        let bytes = fs::read(&input).unwrap();
        let report: Value = serde_json::from_slice(&bytes).unwrap();
        let original: Value = serde_json::from_str(report["bpm_method"].as_str().unwrap()).unwrap();
        assert_eq!(original["schema_version"], 4);
        assert_eq!(original["policy"], "neural-stable-regions-v4");
        assert_eq!(
            original["source_sha256"],
            report["authorized_source_sha256"]
        );
        assert_eq!(
            original["source_sha256"],
            "339911dd7a54af5efc80fdcc05e1432648de9640348803e9b4c0657f59c1a5ea"
        );
        assert_eq!(report["own_source_ack"], true);
        assert_eq!(
            original["frame_count"],
            report["actual_playback_pcm"]["full_frames"]
        );
        assert_eq!(
            original["sample_rate_hz"],
            report["actual_playback_pcm"]["rate_hz"]
        );
        assert_eq!(
            original["mono_sha256"],
            report["actual_playback_pcm"]["mono_sha256"]
        );
        assert_eq!(original["frame_count"], 14_251_520);
        assert_eq!(original["sample_rate_hz"], 48_000);
        assert_eq!(
            original["integer_probe"]["fractional_check"]["status"],
            "inconclusive"
        );
        let beats: Vec<_> = original["raw_beats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let duration = original["frame_count"].as_f64().unwrap()
            / original["sample_rate_hz"].as_f64().unwrap();
        let result = estimate(&beats, duration);
        assert_eq!(result.policy, "neural-stable-regions-v5");
        assert_eq!(result.status, "estimated");
        let selected = result.selected.as_ref().unwrap();
        assert_eq!(result.bpm, Some(selected.bpm));
        // Retained JSON parsing can move a binary64 diagnostic by a few ULPs;
        // this tolerance never permits replacing the exact new selected base.
        assert!(
            selected
                .bpm
                .to_bits()
                .abs_diff(original["bpm"].as_f64().unwrap().to_bits())
                <= 4
        );
        close_metadata(
            &serde_json::to_value(&result.selected).unwrap(),
            &original["selected"],
        );
        close_metadata(
            &serde_json::to_value(&result.regions).unwrap(),
            &original["regions"],
        );
        let probe = result.integer_probe.as_ref().unwrap();
        let common = probe.common_region.as_ref().unwrap();
        assert_eq!(common.window_seconds, [0.0, 2.0 * duration / 3.0]);
        assert_eq!(common.retained, 303);
        assert!(common.excluded_raw_indices.is_empty());
        assert_eq!(probe.base_bpm, selected.bpm);
        assert_eq!(probe.candidate_bpm, 92.0);
        assert_eq!(probe.evidence_tier, "two_ten_second");
        assert_eq!(probe.windows.len(), 2);
        assert_eq!(probe.status, "fractional_estimate");
        assert_eq!(probe.reason, "long_span_mismatch");
        assert_eq!(probe.fractional_check.status, "pass");
        assert!((0.024..0.025).contains(&probe.fractional_check.max_residual_seconds.unwrap()));
        assert!(
            (0.0029..0.0031).contains(
                &probe
                    .fractional_check
                    .window_phase_difference_seconds
                    .unwrap()
            )
        );
        assert!(probe.window_phase_difference_seconds.unwrap() > 0.2);
        assert!(probe.common_max_residual_seconds.unwrap() > 0.13);
        let mut method = original.clone();
        method.as_object_mut().unwrap().extend(
            serde_json::to_value(&result)
                .unwrap()
                .as_object()
                .unwrap()
                .clone(),
        );
        method["schema_version"] = json!(5);
        assert_eq!(method["raw_beats"], original["raw_beats"]);
        assert_eq!(method["raw_downbeats"], original["raw_downbeats"]);
        assert_eq!(method["execution"], original["execution"]);
        fs::write(output, serde_json::to_vec_pretty(&json!({
            "schema_version":1,
            "source_report_sha256":format!("{:x}",Sha256::digest(&bytes)),
            "scope":"Retained original observations only; no new NN, App, Native installation, map/phase ACK or hearing claim; original execution receipt retained",
            "method":method,
        })).unwrap()).unwrap();
    }
}
