//! Recover stable excerpts without interpreting extra/missing detector events as beats.
use super::{RegionEstimate, TempoEstimate, region};

const MAX_RUNS: usize = 64;

pub(super) fn recover(beats: &[f64], duration: f64, result: &mut TempoEstimate) {
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
                    return;
                }
                bounds.push([beats[start], beats[end - 1].next_up().min(duration)]);
            }
            start = end;
        }
    }
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
        assert_eq!(result.integer_probe.unwrap().status, "inconclusive");
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
        assert!(root.is_absolute());
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
            if pad < 3 {
                assert_eq!(result.bpm, original["bpm"].as_f64());
                close_metadata(
                    &serde_json::to_value(&result.regions).unwrap(),
                    &original["regions"],
                );
                close_metadata(
                    &serde_json::to_value(&result.integer_probe).unwrap(),
                    &original["integer_probe"],
                );
            } else {
                assert_eq!(original["status"], "insufficient");
                assert_eq!(result.status, "estimated");
                assert!((89.97..89.99).contains(&result.bpm.unwrap()));
                assert_eq!(
                    result.integer_probe.as_ref().unwrap().status,
                    "inconclusive"
                );
                assert!(result.regions.len() >= 2);
            }
            let mut method = original.clone();
            let update = serde_json::to_value(&result).unwrap();
            method
                .as_object_mut()
                .unwrap()
                .extend(update.as_object().unwrap().clone());
            method["schema_version"] = json!(3);
            assert_eq!(method["raw_beats"], original["raw_beats"]);
            assert_eq!(method["raw_downbeats"], original["raw_downbeats"]);
            fs::write(
                root.join(format!("pad{pad}-v3-method.json")),
                serde_json::to_vec_pretty(&method).unwrap(),
            )
            .unwrap();
            reports.push(
                json!({"pad":pad,"source_sha256":original["source_sha256"],"estimate":result}),
            );
        }
        fs::write(
            root.join("tempo-replay.json"),
            serde_json::to_vec_pretty(&reports).unwrap(),
        )
        .unwrap();
    }
}
