//! Bounded lossless capture encoding. No inference or source authority.

use base64::engine::general_purpose::STANDARD;
use serde::de::{self, Visitor};
use serde::{Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::{self, Write};

use super::descriptor::{MAX_RAW_BYTES, MAX_RAW_EVENTS};

struct Counter(usize);

impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_RAW_BYTES)
            .ok_or_else(|| io::Error::other("raw capture exceeds 16 MiB"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn encoded_len(value: &impl Serialize) -> Result<usize, String> {
    let mut count = Counter(0);
    serde_json::to_writer(&mut count, value).map_err(|error| error.to_string())?;
    Ok(count.0)
}

pub(super) fn encode(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let length = encoded_len(value)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut bytes, value).map_err(|error| error.to_string())?;
    Ok(bytes)
}

struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn revision(encoding: &str, value: &impl Serialize) -> Result<String, String> {
    let mut writer = HashWriter(Sha256::new());
    writer.0.update(encoding.as_bytes());
    writer.0.update([0]);
    serde_json::to_writer(&mut writer, value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", writer.0.finalize()))
}

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096
}

/// The base64 extent is checked before decoding, and the decoded extent before
/// allocating f64 storage. Padding must have its unique canonical spelling.
pub(super) mod binary64 {
    use super::*;
    use base64::Engine as _;

    const MAX_BYTES: usize = MAX_RAW_EVENTS * 8;
    const MAX_ENCODED: usize = MAX_BYTES.div_ceil(3) * 4;

    pub(crate) fn serialize<S: Serializer>(
        values: &[f64],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if values.len() > MAX_RAW_EVENTS || values.iter().any(|value| !value.is_finite()) {
            return Err(serde::ser::Error::custom(
                "invalid bounded binary64 capture array",
            ));
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(values.len() * 8)
            .map_err(serde::ser::Error::custom)?;
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<f64>, D::Error> {
        struct Packed;
        impl Visitor<'_> for Packed {
            type Value = Vec<f64>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("canonical base64 containing at most 250000 binary64 values")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > MAX_ENCODED {
                    return Err(E::custom("binary64 capture array exceeds 250000 values"));
                }
                let bytes = STANDARD.decode(value).map_err(E::custom)?;
                if bytes.len() > MAX_BYTES
                    || !bytes.len().is_multiple_of(8)
                    || STANDARD.encode(&bytes) != value
                {
                    return Err(E::custom("noncanonical or partial binary64 capture array"));
                }
                let mut values = Vec::new();
                values
                    .try_reserve_exact(bytes.len() / 8)
                    .map_err(E::custom)?;
                for chunk in bytes.chunks_exact(8) {
                    let value = f64::from_le_bytes(chunk.try_into().expect("complete f64 chunk"));
                    if !value.is_finite() {
                        return Err(E::custom("nonfinite binary64 capture value"));
                    }
                    values.push(value);
                }
                Ok(values)
            }
        }
        deserializer.deserialize_str(Packed)
    }
}
