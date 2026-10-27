//! Off-RT estimated coordinates from the original pinned neural capture.
//! Event order is a count hypothesis, never reviewed quarters or verified meter.

use super::{MAX_ANCHORS, SourceBeatmap, budget, editor};
use crate::audio_engine::source_beatmap_capture::beat_this::BeatThisRawData;
use serde_json::{Value, json};

const POLICY: &str = "beat-this-local-line-pwa-v1";
const RADIUS: usize = 4;
const STRIDE: usize = 4;

#[derive(Clone, Copy)]
struct Knot {
    seconds: f64,
    beat: f64,
    raw_index: Option<usize>,
    fitted: bool,
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

/// Fixed scratch; this median does not allocate or relabel observations.
fn local_period(beats: &[f64], link: usize) -> Option<f64> {
    let first = link.saturating_sub(3);
    let after = (first + 8).min(beats.len() - 1);
    let mut periods = [0.0_f64; 8];
    let count = after - first;
    for (slot, i) in (first..after).enumerate() {
        periods[slot] = beats[i + 1] - beats[i];
    }
    periods[..count].sort_unstable_by(f64::total_cmp);
    let period = if count.is_multiple_of(2) {
        (periods[count / 2 - 1] + periods[count / 2]) * 0.5
    } else {
        periods[count / 2]
    };
    (period.is_finite() && (0.2..=2.0).contains(&period)).then_some(period)
}

fn count_supported(beats: &[f64], link: usize) -> bool {
    local_period(beats, link).is_some_and(|p| {
        let interval = beats[link + 1] - beats[link];
        interval >= 0.5 * p && interval <= 1.5 * p
    })
}

fn limit(period: f64) -> f64 {
    0.025_f64.min(0.05 * period)
}

fn fit(beats: &[f64], index: usize, duration: f64) -> Option<f64> {
    let first = index.saturating_sub(RADIUS);
    let after = (index + RADIUS + 1).min(beats.len());
    if after - first < 5 || (first..after - 1).any(|i| !count_supported(beats, i)) {
        return None;
    }
    let n = (after - first) as f64;
    let mx = ((first as f64 - index as f64) + (after - 1) as f64 - index as f64) * 0.5;
    let my = beats[first..after].iter().sum::<f64>() / n;
    let (xx, xy) = (first..after).fold((0.0, 0.0), |(xx, xy), i| {
        let x = i as f64 - index as f64 - mx;
        (xx + x * x, xy + x * (beats[i] - my))
    });
    let period = xy / xx;
    let center = my - period * mx;
    if !period.is_finite()
        || !(0.2..=2.0).contains(&period)
        || !center.is_finite()
        || center < 0.0
        || center >= duration
        || (first..after)
            .any(|i| (beats[i] - center - period * (i as f64 - index as f64)).abs() > limit(period))
    {
        return None;
    }
    Some(center)
}

fn segment_supported(beats: &[f64], left: Knot, right: Knot) -> bool {
    let (Some(first), Some(last)) = (left.raw_index, right.raw_index) else {
        return false;
    };
    let period = (right.seconds - left.seconds) / (right.beat - left.beat);
    left.fitted
        && right.fitted
        && (0.2..=2.0).contains(&period)
        && (first..last).all(|i| count_supported(beats, i))
        && (first..=last).all(|i| {
            let predicted = left.seconds
                + (i as f64 - left.beat) / (right.beat - left.beat)
                    * (right.seconds - left.seconds);
            (beats[i] - predicted).abs() <= limit(period)
        })
}

fn admission(
    raw_bytes: usize,
    held: usize,
    events: usize,
    descriptor: &Value,
) -> Result<(), String> {
    if !(9..=MAX_ANCHORS).contains(&events) {
        return Err("automatic_map_observation_count".into());
    }
    let knots = events
        .div_ceil(STRIDE)
        .checked_add(3)
        .ok_or("automatic_map_budget_overflow")?;
    let rows = knots
        .checked_mul(512)
        .ok_or("automatic_map_budget_overflow")?;
    let bound = budget::sum(&[raw_bytes, rows, 128 * 1024])?;
    if bound > super::MAX_RECORD_BYTES {
        return Err("beatmap_byte_limit".into());
    }
    let retained = raw_bytes
        .checked_mul(2)
        .ok_or("automatic_map_budget_overflow")?;
    budget::peak(bound, held, knots, budget::index_bound(bound)?, retained)?;
    let tree = budget::sum(&[
        budget::value_tree(descriptor)?
            .checked_mul(2)
            .ok_or("automatic_map_budget_overflow")?,
        raw_bytes
            .checked_mul(3)
            .ok_or("automatic_map_budget_overflow")?,
        knots
            .checked_mul(8192)
            .ok_or("automatic_map_budget_overflow")?,
        512 * 1024,
    ])?;
    budget::tree_admission(bound, tree, held, retained)?;
    Ok(())
}

/// Pure data construction. Actual source/artifact/adoption permission is separate.
pub(in crate::audio_engine) fn build(
    raw: &BeatThisRawData,
    held: usize,
) -> Result<SourceBeatmap, String> {
    let beats = &raw.predictions().beat_seconds;
    let descriptor = raw.source_descriptor();
    if budget::sum(&[held, raw.encoding_workspace_bytes()?])? > budget::MAX_BUILD_BYTES {
        return Err("automatic_map_capture_encoding_budget".into());
    }
    let raw_bytes = raw.encoded_len()?;
    admission(raw_bytes, held, beats.len(), descriptor)?;
    let duration = descriptor["pcm"]["full_frames"]
        .as_u64()
        .ok_or("decoder_extent_missing")? as f64
        / descriptor["pcm"]["rate_hz"]
            .as_u64()
            .ok_or("decoder_rate_missing")? as f64;
    let mut knots = Vec::new();
    knots
        .try_reserve_exact(beats.len().div_ceil(STRIDE) + 3)
        .map_err(|e| e.to_string())?;
    for index in (0..beats.len())
        .step_by(STRIDE)
        .chain((!(beats.len() - 1).is_multiple_of(STRIDE)).then_some(beats.len() - 1))
    {
        let fitted = fit(beats, index, duration);
        knots.push(Knot {
            seconds: fitted.unwrap_or(beats[index]),
            beat: index as f64,
            raw_index: Some(index),
            fitted: fitted.is_some(),
        });
    }
    if knots.windows(2).any(|p| p[0].seconds >= p[1].seconds) {
        return Err("automatic_map_nonmonotone_fit".into());
    }
    let mut supported = Vec::new();
    supported
        .try_reserve_exact(knots.len() + 1)
        .map_err(|e| e.to_string())?;
    supported.extend(
        knots
            .windows(2)
            .map(|p| segment_supported(beats, p[0], p[1])),
    );
    // Explicit one-beat edge estimates. Generic query extrapolation stays diagnostic.
    let first = knots[0];
    if first.seconds > 0.0 {
        let period = (knots[1].seconds - first.seconds) / (knots[1].beat - first.beat);
        let usable = supported[0] && first.seconds <= period;
        knots.insert(
            0,
            Knot {
                seconds: 0.0,
                beat: first.beat - first.seconds / period,
                raw_index: None,
                fitted: false,
            },
        );
        supported.insert(0, usable);
    }
    let last = *knots.last().ok_or("automatic_map_missing_knots")?;
    if last.seconds < duration {
        let prior = knots[knots.len() - 2];
        let period = (last.seconds - prior.seconds) / (last.beat - prior.beat);
        let usable = *supported.last().ok_or("automatic_map_missing_segments")?
            && duration - last.seconds <= period;
        knots.push(Knot {
            seconds: duration,
            beat: last.beat + (duration - last.seconds) / period,
            raw_index: None,
            fitted: false,
        });
        supported.push(usable);
    }
    if knots.windows(2).any(|p| {
        !p[0].seconds.is_finite()
            || !p[0].beat.is_finite()
            || p[0].seconds >= p[1].seconds
            || p[0].beat >= p[1].beat
    }) || !knots
        .last()
        .is_some_and(|k| k.seconds == duration && k.beat.is_finite())
    {
        return Err("automatic_map_invalid_edge_geometry".into());
    }
    let anchors: Vec<_> = knots.iter().enumerate().map(|(i, k)| json!({
        "id":format!("automatic-knot-{i}"), "source_seconds_bits":bits(k.seconds),
        "beat_bits":bits(k.beat), "raw_beat_index":k.raw_index, "review":"estimated", "sample_edit":null
    })).collect();
    let intervals: Vec<_> = supported.iter().map(|&usable| json!({
        "coverage":if usable {"estimated"} else {"gap_unknown_count"},
        "meter":{"status":"unknown","downbeat_label":null,
            "evidence":"Automatic event-order estimate; source meter and downbeats are unverified"}
    })).collect();
    let mut value = json!({"schema_version":4,"encoding":"source-beatmap-pwa-v4","record":{
        "source":{"decoder_identity":raw.decoder_identity(),"decoder_descriptor":descriptor},
        "raw":{"raw_revision":"0".repeat(64),"capture":raw.embedded_value()?},
        "accepted":{"map_revision":"0".repeat(64),"parent_revision":null,"beat_unit":"quarter_note",
            "interpolation_policy":"pwa-source-seconds-quarter-v1",
            "boundary_policy":"right-interior-left-final-gap-unavailable-v1",
            "snap_policy":"nearest-quarter-subdivision-later-tie-v1","anchors":anchors,"intervals":intervals,
            "lineage":[{"id":POLICY,"parent_revision":null,"kind":"refinement",
                "payload":{"policy":POLICY,"fit_radius_events":RADIUS,"knot_stride_events":STRIDE,
                    "count_hypothesis":"one-estimated-quarter-per-detector-event-no-renumbering",
                    "max_residual_seconds":0.025,"max_residual_period_fraction":0.05,
                    "adjacent_period_ratio":[0.5,1.5],"max_edge_beats":1},
                "evidence":"Automatic local timing estimates, not reviewed counts, verified meter or musical acceptance",
                "author":"native-beat-this"}]}}
    });
    editor::seal_with_context(&mut value, held, raw_bytes * 2)
}

#[cfg(test)]
mod tests;
