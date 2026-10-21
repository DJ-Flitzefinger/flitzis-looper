//! Immutable backend observations, never musical qualification or audio authority.

pub(super) mod beat_this;
mod codec;
use super::source_beatmap_raw_data::{MAX_RAW_BYTES, RawData};
use beat_this::BeatThisRawData;
use serde::Deserialize;
use serde_json::{Value, json};

const QM_ENCODING: &str = "source-beatmap-unreviewed-qm-raw-v1";

#[derive(Debug)]
pub(super) enum RawCapture {
    Qm(Box<RawData>),
    BeatThis(Box<BeatThisRawData>),
}
impl From<RawData> for RawCapture {
    fn from(raw: RawData) -> Self {
        Self::Qm(Box::new(raw))
    }
}
impl From<BeatThisRawData> for RawCapture {
    fn from(raw: BeatThisRawData) -> Self {
        Self::BeatThis(Box::new(raw))
    }
}

/// Geometry-only seam shared with the original direct QM builder fixtures.
pub(in crate::audio_engine) trait CaptureView {
    fn revision(&self) -> &str;
    fn decoder_identity(&self) -> &str;
    fn source_descriptor(&self) -> &Value;
    fn source_sample_rate_hz(&self) -> u32;
    fn source_frame_count(&self) -> usize;
    fn event_count(&self) -> usize;
    fn source_seconds(&self, index: usize) -> Option<f64>;
    fn encoded_len(&self) -> Result<usize, String>;
    fn embedded_value(&self) -> Result<Value, String>;
    fn map_envelope_version(&self) -> u32;
}

impl CaptureView for RawData {
    fn revision(&self) -> &str {
        &self.raw_revision
    }
    fn decoder_identity(&self) -> &str {
        &self.decoder_identity
    }
    fn source_descriptor(&self) -> &Value {
        &self.source_descriptor
    }
    fn source_sample_rate_hz(&self) -> u32 {
        self.source_sample_rate_hz
    }
    fn source_frame_count(&self) -> usize {
        self.source_frame_count
    }
    fn event_count(&self) -> usize {
        self.beat_frames.len()
    }
    fn source_seconds(&self, index: usize) -> Option<f64> {
        self.beat_frames
            .get(index)
            .map(|f| RawData::source_seconds(self, *f))
    }
    fn encoded_len(&self) -> Result<usize, String> {
        RawData::encoded_len(self)
    }
    fn embedded_value(&self) -> Result<Value, String> {
        RawData::embedded_value(self)
    }
    fn map_envelope_version(&self) -> u32 {
        2
    }
}

impl RawCapture {
    pub(super) fn revision(&self) -> &str {
        match self {
            Self::Qm(r) => &r.raw_revision,
            Self::BeatThis(r) => &r.raw_revision,
        }
    }
    pub(super) fn decoder_identity(&self) -> &str {
        match self {
            Self::Qm(r) => &r.decoder_identity,
            Self::BeatThis(r) => &r.source.decoder_identity,
        }
    }
    pub(super) fn source_descriptor(&self) -> &Value {
        match self {
            Self::Qm(r) => &r.source_descriptor,
            Self::BeatThis(r) => &r.source.source_descriptor,
        }
    }
    pub(super) fn source_sample_rate_hz(&self) -> u32 {
        match self {
            Self::Qm(r) => r.source_sample_rate_hz,
            Self::BeatThis(r) => r.source.source_sample_rate_hz,
        }
    }
    pub(super) fn source_frame_count(&self) -> usize {
        match self {
            Self::Qm(r) => r.source_frame_count,
            Self::BeatThis(r) => r.source.source_frame_count,
        }
    }
    pub(super) fn source_zero_frame(&self) -> u64 {
        match self {
            Self::Qm(r) => r.source_zero_frame,
            Self::BeatThis(r) => r.source.source_zero_frame,
        }
    }
    pub(super) fn event_count(&self) -> usize {
        match self {
            Self::Qm(r) => r.beat_frames.len(),
            Self::BeatThis(r) => r.predictions().beat_seconds.len(),
        }
    }
    pub(super) fn downbeat_count(&self) -> usize {
        match self {
            Self::Qm(r) => r.downbeat_indices.len(),
            Self::BeatThis(r) => r.predictions().downbeat_seconds.len(),
        }
    }
    pub(super) fn source_seconds(&self, index: usize) -> Option<f64> {
        match self {
            Self::Qm(r) => r.beat_frames.get(index).map(|f| r.source_seconds(*f)),
            Self::BeatThis(r) => r.predictions().beat_seconds.get(index).copied(),
        }
    }
    pub(super) fn downbeat_seconds(&self, index: usize) -> Option<f64> {
        match self {
            Self::Qm(r) => r
                .downbeat_indices
                .get(index)
                .map(|i| r.source_seconds(r.beat_frames[*i])),
            Self::BeatThis(r) => r.predictions().downbeat_seconds.get(index).copied(),
        }
    }
    pub(super) fn odf_frame(&self, index: usize) -> Option<f64> {
        match self {
            Self::Qm(r) => r.beat_frames.get(index).copied(),
            Self::BeatThis(_) => None,
        }
    }
    pub(super) fn detected_downbeat(&self, index: usize) -> bool {
        match self {
            Self::Qm(r) => r.downbeat_indices.binary_search(&index).is_ok(),
            Self::BeatThis(r) => self.source_seconds(index).is_some_and(|s| {
                r.predictions()
                    .downbeat_seconds
                    .binary_search_by(|v| v.total_cmp(&s))
                    .is_ok()
            }),
        }
    }
    pub(super) fn exact_matching_beat_index(&self, seconds: f64) -> Option<usize> {
        let (mut left, mut right) = (0, self.event_count());
        while left < right {
            let mid = left + (right - left) / 2;
            match self.source_seconds(mid)?.total_cmp(&seconds) {
                std::cmp::Ordering::Less => left = mid + 1,
                std::cmp::Ordering::Greater => right = mid,
                std::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }
    pub(super) fn outside_source_event_count(&self) -> usize {
        if let Self::Qm(raw) = self {
            return raw.outside_source_event_count();
        }
        let duration = self.source_frame_count() as f64 / f64::from(self.source_sample_rate_hz());
        let (mut left, mut right) = (0, self.event_count());
        while left < right {
            let mid = left + (right - left) / 2;
            if self.source_seconds(mid).is_some_and(|v| v < duration) {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        self.event_count() - left
    }
    pub(super) fn capture_encoding(&self) -> &'static str {
        match self {
            Self::Qm(_) => QM_ENCODING,
            Self::BeatThis(_) => beat_this::ENCODING,
        }
    }
    pub(super) fn artifact_kind(&self) -> &'static str {
        match self {
            Self::Qm(_) => "source-beatmap-raw-qm-v1",
            Self::BeatThis(_) => "source-beatmap-raw-beat-this-v2",
        }
    }
    pub(super) fn map_envelope_version(&self) -> u32 {
        match self {
            Self::Qm(_) => 2,
            Self::BeatThis(_) => 3,
        }
    }
    pub(super) fn encoded_len(&self) -> Result<usize, String> {
        match self {
            Self::Qm(r) => r.encoded_len(),
            Self::BeatThis(r) => r.encoded_len(),
        }
    }
    pub(super) fn embedded_value(&self) -> Result<Value, String> {
        match self {
            Self::Qm(r) => r.embedded_value(),
            Self::BeatThis(r) => r.embedded_value(),
        }
    }
    pub(super) fn encode(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Qm(r) => r.encode(),
            Self::BeatThis(r) => r.encode(),
        }
    }

    pub(super) fn decode(input: &[u8]) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Header {
            schema_version: u32,
            encoding: String,
            #[serde(rename = "raw_revision")]
            _revision: serde::de::IgnoredAny,
            #[serde(rename = "data")]
            _data: serde::de::IgnoredAny,
        }
        if input.is_empty() || input.len() > MAX_RAW_BYTES {
            return Err("raw capture byte limit".into());
        }
        let h: Header = serde_json::from_slice(input).map_err(|e| e.to_string())?;
        match (h.schema_version, h.encoding.as_str()) {
            (1, QM_ENCODING) => RawData::decode(input).map(Self::from),
            (2, beat_this::ENCODING) => BeatThisRawData::decode(input).map(Self::from),
            _ => Err("unsupported raw capture envelope".into()),
        }
    }

    /// Bounded metadata only; full arrays stay in their single typed owner.
    pub(super) fn backend_info(&self) -> Value {
        let mut info = json!({"raw_revision":self.revision(),"decoder_identity":self.decoder_identity(),
            "source_sample_rate_hz":self.source_sample_rate_hz(),"source_frame_count":self.source_frame_count(),
            "source_zero_frame":self.source_zero_frame(),"capture_encoding":self.capture_encoding(),
            "event_count":self.event_count(),"detected_downbeat_count":self.downbeat_count(),
            "outside_source_event_count":self.outside_source_event_count(),"interpretation":"unverified","timing_uncertainty":"unknown"});
        let backend = match self {
            Self::Qm(r) => {
                json!({"backend":"legacy_qm","mono_sha256":r.mono_sha256,"mono_rule":r.mono_rule,
                "analyzer_sha256":r.analyzer_sha256,"analyzer_sample_rate_hz":r.analyzer_sample_rate_hz,
                "analyzer_frame_count":r.analyzer_frame_count,"conversion_revision":r.conversion_revision,
                "odf_hop_samples":r.odf_hop_samples,"detector_id":r.detector_id,"code_revision":r.code_revision,
                "configuration":r.configuration.value(),"worker_input_sha256":null,"worker_input_sample_rate_hz":null,
                "worker_input_frame_count":null,"worker_input_source_zero_frame":null,"model":null,"frontend_identity":null,
                "beat_logit_count":0,"downbeat_logit_count":0})
            }
            Self::BeatThis(r) => {
                json!({"backend":"beat_this","mono_sha256":r.source.mono_sha256,"mono_rule":r.source.mono_rule,
                "analyzer_sha256":null,"analyzer_sample_rate_hz":null,"analyzer_frame_count":null,"conversion_revision":null,
                "odf_hop_samples":null,"detector_id":"beat-this","code_revision":r.code_revision(),"configuration":null,
                "worker_input_sha256":r.source.mono_sha256,"worker_input_sample_rate_hz":r.source.source_sample_rate_hz,
                "worker_input_frame_count":r.source.source_frame_count,"worker_input_source_zero_frame":r.source.source_zero_frame,
                "model":r.model(),"frontend_identity":r.frontend(),"beat_logit_count":r.predictions().beat_logits.len(),
                "downbeat_logit_count":r.predictions().downbeat_logits.len()})
            }
        };
        info.as_object_mut()
            .expect("typed info")
            .extend(backend.as_object().expect("typed backend").clone());
        info
    }

    /// Native heap bound: source, incoming result, capture/reparse arrays and
    /// three full encoded buffers coexist. This does not bound subprocess RSS.
    pub(super) fn publication_peak_bytes(
        &self,
        held: usize,
        worker_bytes: usize,
    ) -> Result<usize, String> {
        if worker_bytes > MAX_RAW_BYTES {
            return Err("raw worker byte limit".into());
        }
        let arrays = match self {
            Self::Qm(r) => r.beat_frames.capacity().checked_mul(8).and_then(|n| {
                r.downbeat_indices
                    .capacity()
                    .checked_mul(size_of::<usize>())
                    .and_then(|d| n.checked_add(d))
            }),
            Self::BeatThis(r) => {
                let p = r.predictions();
                [
                    p.beat_seconds.capacity(),
                    p.downbeat_seconds.capacity(),
                    p.beat_logits.capacity(),
                    p.downbeat_logits.capacity(),
                ]
                .into_iter()
                .try_fold(0_usize, |s, c| {
                    c.checked_mul(8).and_then(|n| s.checked_add(n))
                })
            }
        }
        .ok_or("raw capture array budget overflow")?;
        let peak = held
            .checked_add(worker_bytes)
            .and_then(|n| n.checked_add(3 * MAX_RAW_BYTES))
            .and_then(|n| arrays.checked_mul(3).and_then(|a| n.checked_add(a)))
            .and_then(|n| n.checked_add(8 * 1024 * 1024))
            .ok_or("raw capture publication budget overflow")?;
        if peak > 512 * 1024 * 1024 {
            return Err("raw capture exceeds 512-MiB Native publication peak".into());
        }
        Ok(peak)
    }
}

impl CaptureView for RawCapture {
    fn revision(&self) -> &str {
        RawCapture::revision(self)
    }
    fn decoder_identity(&self) -> &str {
        RawCapture::decoder_identity(self)
    }
    fn source_descriptor(&self) -> &Value {
        RawCapture::source_descriptor(self)
    }
    fn source_sample_rate_hz(&self) -> u32 {
        RawCapture::source_sample_rate_hz(self)
    }
    fn source_frame_count(&self) -> usize {
        RawCapture::source_frame_count(self)
    }
    fn event_count(&self) -> usize {
        RawCapture::event_count(self)
    }
    fn source_seconds(&self, index: usize) -> Option<f64> {
        RawCapture::source_seconds(self, index)
    }
    fn encoded_len(&self) -> Result<usize, String> {
        RawCapture::encoded_len(self)
    }
    fn embedded_value(&self) -> Result<Value, String> {
        RawCapture::embedded_value(self)
    }
    fn map_envelope_version(&self) -> u32 {
        RawCapture::map_envelope_version(self)
    }
}

#[cfg(test)]
mod tests;
