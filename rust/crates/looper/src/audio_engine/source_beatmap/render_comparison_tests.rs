//! Shared, bounded accepted-map inputs for the existing offline renderer comparison.
//! Hash equality proves synthetic consistency, never current producer or audio custody.

use super::SourceBeatmap;
use crate::audio_engine::cold_store::descriptor_digest;
use crate::messages::SampleBuffer;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) const RATE: u32 = 48_000;
pub(crate) const SPAN: usize = 32_768;
pub(crate) const OUTPUT: usize = 4 * SPAN;
pub(crate) const SOURCE: usize = 163_840;
const ANCHORS: [usize; 5] = [0, 16_384, 49_152, 114_688, SOURCE];

#[derive(Clone, Copy, Debug)]
pub(crate) enum Signal {
    StereoTone,
    IsolatedAttack(usize),
}

pub(crate) struct CanonicalFixture {
    pub(crate) map: SourceBeatmap,
    pub(crate) sample: SampleBuffer,
    pub(crate) coordinates: Vec<f64>,
    pub(crate) rates: Vec<f64>,
    pub(crate) keyframes: Vec<(u32, u32)>,
    pub(crate) pcm_hash: String,
    pub(crate) mono_hash: String,
    record: Value,
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

pub(crate) fn pcm_hash(samples: &[f32]) -> String {
    let mut digest = Sha256::new();
    for value in samples {
        digest.update(value.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

/// Independent frozen arithmetic; it never supplies the renderer's addresses.
fn oracle(output: usize) -> f64 {
    if output == OUTPUT {
        return SOURCE as f64;
    }
    let segment = output / SPAN;
    ANCHORS[segment] as f64 + (output % SPAN) as f64 * [0.5, 1.0, 2.0, 1.5][segment]
}

fn seal(value: &mut Value) {
    let record = &mut value["record"];
    record["source"]["decoder_identity"] =
        json!(descriptor_digest(&record["source"]["decoder_descriptor"]).unwrap());
    record["raw"]
        .as_object_mut()
        .unwrap()
        .remove("raw_revision");
    record["raw"]["raw_revision"] = json!(descriptor_digest(&record["raw"]).unwrap());
    record["accepted"]
        .as_object_mut()
        .unwrap()
        .remove("map_revision");
    record["accepted"]["map_revision"] = json!(descriptor_digest(record).unwrap());
}

pub(crate) fn checked_coordinates(map: &SourceBeatmap) -> Result<Vec<f64>, &'static str> {
    let mut coordinates = Vec::with_capacity(OUTPUT + 1);
    for output in 0..=OUTPUT {
        let beat = output as f64 * 2.0 / SPAN as f64;
        let evaluated = map.inner.at_beat(beat);
        if !evaluated.supported()
            || !matches!(
                evaluated.physical_domain,
                "within_source" | "exclusive_source_end"
            )
        {
            return Err("unsupported canonical trajectory before Native construction");
        }
        let coordinate = evaluated.source_seconds.ok_or("missing inverse")? * f64::from(RATE);
        // A direct integer/half-integer oracle checks EVERY used position, including endpoint.
        if (coordinate - oracle(output)).abs() > 1.0e-8 {
            return Err("canonical inverse differs from independent frozen oracle");
        }
        coordinates.push(coordinate);
    }
    Ok(coordinates)
}

impl CanonicalFixture {
    pub(crate) fn new(signal: Signal) -> Self {
        let samples = (0..SOURCE)
            .flat_map(|frame| {
                std::array::from_fn::<_, 2, _>(|channel| match signal {
                    Signal::StereoTone => {
                        if frame >= SOURCE - 8192 {
                            0.0
                        } else {
                            (0.2 * (std::f64::consts::TAU * [160.0, 211.0][channel] * frame as f64
                                / f64::from(RATE))
                            .sin()) as f32
                        }
                    }
                    Signal::IsolatedAttack(at) => {
                        let first = oracle(at) as usize;
                        if frame < first || frame >= first + 384 {
                            0.0
                        } else {
                            let n = frame - first;
                            let envelope = (-7.0 * n as f64 / 384.0).exp();
                            let carrier =
                                (std::f64::consts::TAU * 900.0 * n as f64 / f64::from(RATE) + 0.37)
                                    .sin();
                            (envelope * carrier * if channel == 0 { 0.7 } else { 0.35 }) as f32
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        let pcm_digest = pcm_hash(&samples);
        let mono = samples
            .chunks_exact(2)
            .map(|pair| (pair[0] + pair[1]) * 0.5)
            .collect::<Vec<_>>();
        let mono_hash = pcm_hash(&mono);
        let decoder = json!({"schema_version":1,
            "original":{"sha256":pcm_digest,"bytes":samples.len()*4},
            "pcm":{"format":"f32-le-interleaved-v1",
                "channel_layout":"interleaved-channel-index-order-v1",
                "rate_hz":RATE,"channels":2,"full_frames":SOURCE,"full_bytes":samples.len()*4,
                "interleaved_sha256":pcm_digest,"mono_sha256":mono_hash,
                "mono_revision":"synthetic-stereo-average-f32-v1","source_zero_bits":bits(0.0),
                "provenance":{"synthetic":true,"original_is_exact_generated_pcm_bytes":true,
                    "current_native_source_custody":false}}});
        let anchors = ANCHORS.into_iter().enumerate().map(|(index, frame)| json!({
            "id":format!("frozen-{index}"),"source_seconds_bits":bits(frame as f64/f64::from(RATE)),
            "beat_bits":bits(index as f64*2.0),"raw_beat_index":null,"review":"reviewed","sample_edit":null
        })).collect::<Vec<_>>();
        let intervals = (0..4).map(|_| json!({"coverage":"trusted",
            "meter":{"status":"unknown","downbeat_label":null,"evidence":"synthetic quarter coordinates"}}
        )).collect::<Vec<_>>();
        let mut record = json!({"schema_version":1,"encoding":"source-beatmap-pwa-v1","record":{
            "source":{"decoder_identity":"0".repeat(64),"decoder_descriptor":decoder},
            "raw":{"raw_revision":"0".repeat(64),"detector":{"detector_id":"manual",
                "code_revision":"accepted-map-render-comparison-v1","configuration":{},"model":null,"preprocessing":{}},
                "analysis_input":null,"timestamp_resolution_seconds_bits":null,"beat_seconds_bits":[],
                "downbeat_seconds_bits":[],"uncertainty":"synthetic only; no musical or current source acceptance"},
            "accepted":{"map_revision":"0".repeat(64),"parent_revision":null,"beat_unit":"quarter_note",
                "interpolation_policy":"pwa-source-seconds-quarter-v1",
                "boundary_policy":"right-interior-left-final-gap-unavailable-v1",
                "snap_policy":"nearest-quarter-subdivision-later-tie-v1",
                "anchors":anchors,"intervals":intervals,"lineage":[]}}});
        seal(&mut record);
        let map = SourceBeatmap::parse(&record.to_string()).unwrap();
        let coordinates = checked_coordinates(&map).unwrap();
        let keyframes = (1..=4)
            .map(|index| {
                let source = coordinates[index * SPAN];
                assert!((source - source.round()).abs() <= 1.0e-8);
                (
                    u32::try_from(source.round() as usize).unwrap(),
                    u32::try_from(index * SPAN).unwrap(),
                )
            })
            .collect();
        let rates = (0..4)
            .map(|index| {
                let midpoint = index * SPAN + SPAN / 2;
                let left = map.inner.at_beat(midpoint as f64 * 2.0 / SPAN as f64);
                // Inverse derivative from the SAME accepted evaluator, not a copied rate table.
                let derivative =
                    f64::from(RATE) * 60.0 / left.local_bpm.unwrap() * 2.0 / SPAN as f64;
                // These predeclared fixture anchors are representable loaded integer frames.
                // Conversion occurs once after proving the inverse is within 1e-8 frame.
                let rate = (coordinates[(index + 1) * SPAN].round()
                    - coordinates[index * SPAN].round())
                    / SPAN as f64;
                assert!((rate - derivative).abs() < 1.0e-12);
                rate
            })
            .collect();
        let fixture = Self {
            map,
            sample: SampleBuffer {
                samples: samples.into(),
                channels: 2,
                residency: None,
            },
            coordinates,
            rates,
            keyframes,
            pcm_hash: pcm_digest,
            mono_hash,
            record,
        };
        fixture.verify_pcm();
        fixture
    }

    pub(crate) fn verify_pcm(&self) {
        assert_eq!(self.sample.frame_count(), SOURCE);
        assert_eq!(pcm_hash(&self.sample.samples), self.pcm_hash);
        let mono = self
            .sample
            .samples
            .chunks_exact(2)
            .map(|p| (p[0] + p[1]) * 0.5)
            .collect::<Vec<_>>();
        assert_eq!(pcm_hash(&mono), self.mono_hash);
        assert_eq!(self.map.inner.sample_rate_hz, RATE);
        assert_eq!(self.map.inner.source_frames, SOURCE as u64);
        let descriptor = &self.record["record"]["source"]["decoder_descriptor"];
        assert_eq!(descriptor["pcm"]["interleaved_sha256"], self.pcm_hash);
        assert_eq!(descriptor["pcm"]["mono_sha256"], self.mono_hash);
        assert_eq!(
            descriptor_digest(descriptor).unwrap(),
            self.map.source_identity()
        );
    }
}

#[test]
fn canonical_render_fixture_verifies_every_address_and_unknown_meter() {
    let fixture = CanonicalFixture::new(Signal::StereoTone);
    assert_eq!(fixture.keyframes.len(), 4);
    assert_eq!(
        fixture.keyframes.last(),
        Some(&(SOURCE as u32, OUTPUT as u32))
    );
    assert_eq!(fixture.map.inner.at_beat(3.0).meter_status, "unknown");
    assert!(fixture.map.inner.at_beat(3.0).supported());
}

#[test]
fn canonical_render_fixture_refuses_gap_before_any_native_owner() {
    let mut fixture = CanonicalFixture::new(Signal::IsolatedAttack(SPAN / 4));
    fixture.record["record"]["accepted"]["intervals"][1]["coverage"] = json!("gap_unknown_count");
    seal(&mut fixture.record);
    let map = SourceBeatmap::parse(&fixture.record.to_string()).unwrap();
    assert!(checked_coordinates(&map).is_err());
}
