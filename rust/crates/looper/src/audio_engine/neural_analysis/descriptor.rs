//! Bounded immutable source geometry for neural analysis.
use super::super::cold_store::descriptor_digest;
use serde::Deserializer;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;
use std::io::{self, Write};
pub(in crate::audio_engine) const MAX_RAW_BYTES: usize = 16 * 1024 * 1024;
pub(in crate::audio_engine) const MAX_RAW_EVENTS: usize = 250_000;
const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

struct DescriptorCounter(usize);
impl Write for DescriptorCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("raw descriptor size overflow"))?;
        if self.0 > MAX_DESCRIPTOR_BYTES {
            return Err(io::Error::other("raw descriptor exceeds 64 KiB"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn digest_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(in crate::audio_engine) fn validate_source_descriptor(
    descriptor: &Value,
    identity: &str,
) -> Result<(), String> {
    let mut descriptor_budget = MAX_DESCRIPTOR_BYTES;
    validate_descriptor_tree(descriptor, 0, &mut descriptor_budget)?;
    serde_json::to_writer(&mut DescriptorCounter(0), descriptor)
        .map_err(|error| error.to_string())?;
    if !digest_text(identity)
        || descriptor_digest(descriptor).map_err(|error| error.to_string())? != identity
    {
        return Err("raw source decoder identity differs".into());
    }
    let original = &descriptor["original"];
    let pcm = &descriptor["pcm"];
    let frames = pcm["full_frames"]
        .as_u64()
        .ok_or("raw decoder extent missing")?;
    let channels = pcm["channels"]
        .as_u64()
        .ok_or("raw decoder channels missing")?;
    let full_bytes = frames.checked_mul(channels).and_then(|n| n.checked_mul(4));
    if descriptor["schema_version"] != 1
        || original["sha256"]
            .as_str()
            .is_none_or(|value| !digest_text(value))
        || original["bytes"].as_u64().is_none_or(|value| value == 0)
        || pcm["format"] != "f32-le-interleaved-v1"
        || pcm["channel_layout"] != "interleaved-channel-index-order-v1"
        || pcm["source_zero_bits"] != "0000000000000000"
        || !pcm["provenance"].is_object()
        || pcm["mono_revision"].as_str().is_none_or(str::is_empty)
        || pcm["rate_hz"]
            .as_u64()
            .is_none_or(|value| value == 0 || value > u64::from(u32::MAX))
        || !(1..=32).contains(&channels)
        || frames == 0
        || full_bytes.is_none()
        || pcm["full_bytes"].as_u64() != full_bytes
        || ["interleaved_sha256", "mono_sha256"]
            .iter()
            .any(|key| pcm[*key].as_str().is_none_or(|value| !digest_text(value)))
    {
        return Err("raw source decoder descriptor invalid".into());
    }
    Ok(())
}

fn validate_descriptor_tree(
    value: &Value,
    depth: usize,
    remaining: &mut usize,
) -> Result<(), String> {
    if depth > 32 {
        return Err("raw descriptor nesting bound exceeded".into());
    }
    *remaining = remaining
        .checked_sub(16)
        .ok_or("raw descriptor allocation bound exceeded")?;
    match value {
        Value::String(text) => {
            *remaining = remaining
                .checked_sub(text.len())
                .ok_or("raw descriptor allocation bound exceeded")?;
        }
        Value::Array(values) => {
            for value in values {
                validate_descriptor_tree(value, depth + 1, remaining)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let key_bytes = key
                    .len()
                    .checked_add(32)
                    .ok_or("raw descriptor allocation bound exceeded")?;
                *remaining = remaining
                    .checked_sub(key_bytes)
                    .ok_or("raw descriptor allocation bound exceeded")?;
                validate_descriptor_tree(value, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(in crate::audio_engine) fn bounded_descriptor<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Value, D::Error> {
    let mut remaining = MAX_DESCRIPTOR_BYTES;
    DescriptorSeed {
        remaining: &mut remaining,
        depth: 0,
    }
    .deserialize(deserializer)
}

struct DescriptorSeed<'a> {
    remaining: &'a mut usize,
    depth: usize,
}
impl DescriptorSeed<'_> {
    fn spend<E: de::Error>(&mut self, bytes: usize) -> Result<(), E> {
        *self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| E::custom("raw descriptor allocation bound exceeded"))?;
        Ok(())
    }
}
impl<'de> DeserializeSeed<'de> for DescriptorSeed<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(mut self, deserializer: D) -> Result<Value, D::Error> {
        if self.depth > 32 {
            return Err(de::Error::custom("raw descriptor nesting bound exceeded"));
        }
        self.spend::<D::Error>(16)?;
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for DescriptorSeed<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("bounded source decoder descriptor")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("nonfinite descriptor number"))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_str<E: de::Error>(mut self, value: &str) -> Result<Value, E> {
        self.spend::<E>(value.len())?;
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: de::Error>(mut self, value: String) -> Result<Value, E> {
        self.spend::<E>(value.len())?;
        Ok(Value::String(value))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(DescriptorSeed {
            remaining: self.remaining,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            self.spend::<A::Error>(key.len().saturating_add(32))?;
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate raw descriptor field"));
            }
            let value = map.next_value_seed(DescriptorSeed {
                remaining: self.remaining,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}
