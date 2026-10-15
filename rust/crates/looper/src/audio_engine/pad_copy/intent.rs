//! Worker/control validation of the musical snapshot; no JSON enters realtime.
use super::CopyRtIntent;
use crate::audio_engine::constant_timing::AcceptedTimingProjection;
use crate::audio_engine::constants::{
    PAD_EQ_DB_MAX, PAD_EQ_DB_MIN, PAD_GAIN_DB_MAX, PAD_GAIN_DB_MIN,
};
use crate::audio_engine::pitch_intent::PitchIntent;
use crate::audio_engine::source_grid::SourceGrid;
use crate::messages::{STEM_COMPONENT_MASK, SampleBuffer, StemMixMode};
use flitzis_looper_analysis::tempo_acceptance::TimingIntent;
use serde_json::Value;

const FIELDS: [&str; 21] = [
    "source_path",
    "source_identity",
    "duration",
    "analysis",
    "stem_cache",
    "stem_mix_mode",
    "stem_intent",
    "key_lock",
    "manual_bpm",
    "timing_intent",
    "key_intent",
    "gain_db",
    "eq_low_db",
    "eq_mid_db",
    "eq_high_db",
    "loop_start_s",
    "loop_end_s",
    "loop_auto",
    "loop_bars",
    "grid_offset_samples",
    "grid_anchor_s",
];

#[derive(Clone)]
pub(super) struct CopyIntent {
    pub(super) snapshot: Value,
    pub(super) key_lock: bool,
    pub(super) timing_intent: TimingIntent,
}

fn finite(value: &Value, name: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or_else(|| format!("copy {name} must be finite numeric data"))
}

fn optional_finite(value: &Value, name: &str) -> Result<Option<f64>, String> {
    if value.is_null() {
        Ok(None)
    } else {
        finite(value, name).map(Some)
    }
}

fn identity(value: &Value) -> bool {
    value.as_str().is_some_and(|id| {
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

impl CopyIntent {
    fn pitch_intent(&self) -> Result<PitchIntent, String> {
        let key = &self.snapshot["key_intent"];
        // Existing snapshots represent the neutral inherited intent with null.
        if key.is_null() {
            return Ok(PitchIntent::default());
        }
        let shift = |name: &str| {
            key[name]
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| format!("copy key {name} must be a supported integer"))
        };
        PitchIntent::new(shift("base_shift")?, shift("extra_shift")?)
            .map_err(|error| error.to_string())
    }
    pub(super) fn parse(json: &str) -> Result<Self, String> {
        if json.len() > 32 * 1024 * 1024 {
            return Err("copy snapshot exceeds its admission limit".into());
        }
        let envelope: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
        let object = envelope
            .as_object()
            .ok_or("copy envelope must be an object")?;
        if object.len() != 3
            || !["schema_version", "snapshot", "resolved_key_lock"]
                .iter()
                .all(|key| object.contains_key(*key))
            || envelope["schema_version"].as_u64() != Some(1)
        {
            return Err("unsupported copy envelope".into());
        }
        let snapshot = envelope["snapshot"].clone();
        let data = snapshot
            .as_object()
            .ok_or("copy snapshot must be an object")?;
        if data.len() != FIELDS.len() || !FIELDS.iter().all(|key| data.contains_key(*key)) {
            return Err("copy snapshot fields differ from the complete musical schema".into());
        }
        let path = snapshot["source_path"]
            .as_str()
            .ok_or("copy source path missing")?;
        if path.is_empty()
            || path.len() > 8192
            || !identity(&snapshot["source_identity"]["instance_id"])
        {
            return Err("copy durable source identity is invalid".into());
        }
        let key_lock = envelope["resolved_key_lock"]
            .as_bool()
            .ok_or("copy resolved Key Lock must be boolean")?;
        if !snapshot["key_lock"].is_null() && snapshot["key_lock"].as_bool() != Some(key_lock) {
            return Err("copy Key Lock override differs from its resolved mode".into());
        }
        let timing_intent = match snapshot["timing_intent"].as_str() {
            Some("legacy") => TimingIntent::Legacy,
            Some("automatic") => TimingIntent::Automatic,
            Some("manual") => TimingIntent::Manual,
            Some("tap") => TimingIntent::Tap,
            _ => return Err("copy timing intent is invalid".into()),
        };
        let result = Self {
            snapshot,
            key_lock,
            timing_intent,
        };
        result.mask()?;
        result.pitch_intent()?;
        Ok(result)
    }

    pub(super) fn descriptor(&self) -> Result<Option<&str>, String> {
        let cache = &self.snapshot["stem_cache"];
        if cache.is_null() {
            return Ok(None);
        }
        if cache["available"].as_bool() != Some(false) {
            return Err("copy metadata cannot claim current target stem availability".into());
        }
        if cache["pair"].is_null() {
            return Err("copy requires a verified canonical stem pair".into());
        }
        cache["pair"]["descriptor_reference"]
            .as_str()
            .map(Some)
            .ok_or_else(|| "copy selected pair descriptor missing".into())
    }

    fn mask(&self) -> Result<u8, String> {
        let intent = &self.snapshot["stem_intent"];
        let mask = intent["enabled_mask"]
            .as_u64()
            .filter(|mask| *mask <= u64::from(STEM_COMPONENT_MASK))
            .ok_or("copy component mask is invalid")? as u8;
        let remembered = intent["last_custom_mask"].as_u64();
        if intent.as_object().is_none_or(|object| object.len() != 4)
            || intent["schema_version"].as_u64() != Some(1)
            || remembered.is_none_or(|mask| mask > u64::from(STEM_COMPONENT_MASK))
            || !matches!(
                (intent["display_mode"].as_str(), mask),
                (Some("custom"), _) | (Some("all"), 15) | (Some("instrumental"), 14)
            )
        {
            return Err("copy stem intent is invalid".into());
        }
        Ok(mask)
    }

    pub(super) fn rt_intent(
        &self,
        sample: &SampleBuffer,
        rate: u32,
        projection: Option<AcceptedTimingProjection>,
    ) -> Result<CopyRtIntent, String> {
        if rate == 0 || sample.frame_count() == 0 {
            return Err("copy source geometry unavailable".into());
        }
        let data = &self.snapshot;
        let manual = optional_finite(&data["manual_bpm"], "manual BPM")?;
        if manual.is_some_and(|bpm| bpm <= 0.0) {
            return Err("copy BPM must be positive".into());
        }
        let bpm =
            manual
                .or(optional_finite(&data["analysis"]["bpm"], "analysis BPM")?
                    .filter(|bpm| *bpm > 0.0));
        let period_seconds = bpm.map(|bpm| 60.0 / bpm);
        let first = |name: &str| {
            data["analysis"]["beat_grid"][name]
                .as_array()
                .and_then(|values| values.first())
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0)
        };
        let anchor = optional_finite(&data["grid_anchor_s"], "grid anchor")?
            .or_else(|| {
                first("downbeats")
                    .or_else(|| first("beats"))
                    .map(|anchor| if anchor <= 0.05 { 0.0 } else { anchor })
            })
            .unwrap_or(0.0);
        let offset = data["grid_offset_samples"]
            .as_i64()
            .ok_or("copy grid offset exceeds native coordinates")?;
        let origin_frame = (anchor * f64::from(rate)).round_ties_even() + offset as f64;
        let start_s = finite(&data["loop_start_s"], "loop start")?;
        let saved_end_s = optional_finite(&data["loop_end_s"], "loop end")?;
        let auto = data["loop_auto"]
            .as_bool()
            .ok_or("copy loop mode must be boolean")?;
        let bars = finite(&data["loop_bars"], "loop bars")?;
        if anchor < 0.0
            || start_s < 0.0
            || saved_end_s.is_some_and(|end| end < 0.0)
            || bars < 0.5
            || (bars * 2.0 - (bars * 2.0).round()).abs() > 1e-9
        {
            return Err("copy loop or grid coordinates are invalid".into());
        }
        let grid = projection
            .map(|timing| (timing.period_seconds, timing.origin_seconds))
            .or_else(|| period_seconds.map(|period| (period, origin_frame / f64::from(rate))));
        let end_s = if auto {
            grid.and_then(|(period, origin)| SourceGrid::from_period(period, origin))
                .and_then(|grid| grid.source_after_beats(start_s, bars * 4.0))
        } else {
            saved_end_s
        };
        let frame = |seconds: f64| -> Result<usize, String> {
            let frame = (seconds * f64::from(rate)).round_ties_even();
            if !frame.is_finite() || frame < 0.0 {
                return Err("copy loop exceeds finite source coordinates".into());
            }
            Ok(frame.clamp(0.0, sample.frame_count() as f64) as usize)
        };
        let scalar = |name: &str, min: f32, max: f32| -> Result<f32, String> {
            let value = finite(&data[name], name)?;
            if value < f64::from(min) || value > f64::from(max) {
                return Err(format!("copy {name} is out of range"));
            }
            Ok(value as f32)
        };
        let stem_mix_mode = match data["stem_mix_mode"].as_str() {
            Some("full_mix") => StemMixMode::FullMix,
            Some("all_stems") => StemMixMode::AllStems,
            _ => return Err("copy stem mode is invalid".into()),
        };
        Ok(CopyRtIntent {
            period_seconds,
            origin_frame,
            loop_start_frame: frame(start_s)?,
            loop_end_frame: end_s.map(frame).transpose()?,
            key_lock: self.key_lock,
            pitch_intent: self.pitch_intent()?,
            timing_intent: self.timing_intent,
            stem_mix_mode,
            source_version_hash: 0,
            enabled_mask: self.mask()?,
            gain_db: scalar("gain_db", PAD_GAIN_DB_MIN, PAD_GAIN_DB_MAX)?,
            eq_low_db: scalar("eq_low_db", PAD_EQ_DB_MIN, PAD_EQ_DB_MAX)?,
            eq_mid_db: scalar("eq_mid_db", PAD_EQ_DB_MIN, PAD_EQ_DB_MAX)?,
            eq_high_db: scalar("eq_high_db", PAD_EQ_DB_MIN, PAD_EQ_DB_MAX)?,
        })
    }
}
