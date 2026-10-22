//! Streaming JSON density accounting before any full Value construction.

use super::*;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

const ALLOCATION_BYTES: usize = 16;
const BTREE_NODE_BYTES: usize = 1024;
const MAX_JSON_DEPTH: usize = 128;

/// The resolved serde_json Map is BTreeMap, without preserve_order. The pinned
/// 64-bit layout has Value=32, String=24, B=6: a leaf holds 11 key/value slots
/// (632 bytes), an internal node adds 12 child pointers (728 bytes). A 1024-byte
/// node plus allocation allowance bounds both. Non-root occupancy is at least
/// five; the deliberately loose 1+ceil(n/5) bound also charges the root. Refuse
/// a changed representation rather than silently retaining this memory proof.
fn supported_layout() -> Result<(), String> {
    if size_of::<usize>() != 8
        || size_of::<Value>() != 32
        || size_of::<String>() != 24
        || size_of::<serde_json::Map<String, Value>>()
            != size_of::<std::collections::BTreeMap<String, Value>>()
    {
        return Err("beatmap JSON memory layout is unsupported".into());
    }
    Ok(())
}

fn product(a: usize, b: usize) -> Result<usize, String> {
    a.checked_mul(b)
        .ok_or_else(|| "beatmap_review_budget_overflow".into())
}

fn string_heap(length: usize) -> Result<usize, String> {
    if length == 0 {
        return Ok(0);
    }
    sum(&[product(length, 2)?.max(8), ALLOCATION_BYTES])
}

fn object_heap(entries: usize) -> Result<usize, String> {
    let nodes = match entries {
        0 => 0,
        1..=11 => 1,
        _ => sum(&[1, entries.div_ceil(5)])?,
    };
    product(nodes, BTREE_NODE_BYTES + ALLOCATION_BYTES)
}

/// At the largest nested stage, seal's input Value coexists with parse's current
/// record, Undo's preview, reconstructed predecessor and canonical hash copy:
/// five full-tree equivalents. Six also covers typed-to-Value/external Raw data.
/// Twelve serialized extents charge caller/read/output buffers, canonical Vec
/// growth, binary64/base64 decode/encode scratch and sorted-key pointer scratch.
/// The original 32x bound remains a floor, not a generic node-density proof.
fn generic_peak(
    serialized: usize,
    tree: usize,
    held: usize,
    index: usize,
    retained: usize,
) -> Result<usize, String> {
    if serialized > MAX_RECORD_BYTES {
        return Err("beatmap_byte_limit".into());
    }
    let work = sum(&[product(tree, 6)?, product(serialized, 12)?])?.max(product(serialized, 32)?);
    let result = sum(&[work, FIXED_BYTES, held, index, retained])?;
    if result > MAX_BUILD_BYTES {
        return Err("beatmap review exceeds 512-MiB simultaneous job peak".into());
    }
    Ok(result)
}

pub(in crate::audio_engine) fn tree_admission(
    serialized: usize,
    tree: usize,
    held: usize,
    retained: usize,
) -> Result<usize, String> {
    generic_peak(serialized, tree, held, index_bound(serialized)?, retained)
}

struct ShapeCounter {
    heap: usize,
    serialized: usize,
    held: usize,
    retained: usize,
}

impl ShapeCounter {
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        self.heap = sum(&[self.heap, bytes])?;
        tree_admission(
            self.serialized,
            sum(&[size_of::<Value>(), self.heap])?,
            self.held,
            self.retained,
        )?;
        Ok(())
    }
}

struct ShapeSeed<'a> {
    counter: &'a mut ShapeCounter,
    depth: usize,
    key: bool,
}

impl<'de> DeserializeSeed<'de> for ShapeSeed<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        if self.depth >= MAX_JSON_DEPTH {
            return Err(de::Error::custom("beatmap JSON nesting limit"));
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ShapeSeed<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON memory shape")
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        self.counter
            .charge(string_heap(value.len()).map_err(E::custom)?)
            .map_err(E::custom)
    }
    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<(), E> {
        self.visit_str(value)
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<(), E> {
        self.visit_str(&value)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        if self.key {
            return Err(de::Error::custom("JSON object key must be text"));
        }
        self.counter
            .charge(ALLOCATION_BYTES)
            .map_err(de::Error::custom)?;
        let mut count = 0_usize;
        let mut capacity = 0_usize;
        loop {
            // Charge the next slot before its child is visited. A terminal
            // speculative slot can only overestimate the real Vec capacity.
            if count == capacity {
                let next = capacity
                    .checked_mul(2)
                    .ok_or_else(|| de::Error::custom("beatmap_review_budget_overflow"))?
                    .max(4);
                self.counter
                    .charge(
                        product(next - capacity, size_of::<Value>()).map_err(de::Error::custom)?,
                    )
                    .map_err(de::Error::custom)?;
                capacity = next;
            }
            if seq
                .next_element_seed(ShapeSeed {
                    counter: self.counter,
                    depth: self.depth + 1,
                    key: false,
                })?
                .is_none()
            {
                break;
            }
            count = count
                .checked_add(1)
                .ok_or_else(|| de::Error::custom("beatmap_review_budget_overflow"))?;
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        if self.key {
            return Err(de::Error::custom("JSON object key must be text"));
        }
        let mut count = 0_usize;
        let mut charged = 0_usize;
        loop {
            let next = count
                .checked_add(1)
                .ok_or_else(|| de::Error::custom("beatmap_review_budget_overflow"))?;
            let storage = object_heap(next).map_err(de::Error::custom)?;
            self.counter
                .charge(storage - charged)
                .map_err(de::Error::custom)?;
            charged = storage;
            if map
                .next_key_seed(ShapeSeed {
                    counter: self.counter,
                    depth: self.depth + 1,
                    key: true,
                })?
                .is_none()
            {
                break;
            }
            map.next_value_seed(ShapeSeed {
                counter: self.counter,
                depth: self.depth + 1,
                key: false,
            })?;
            count = next;
        }
        Ok(())
    }
}

/// Streaming admission visits every generic JSON value, including arbitrary
/// durable lineage/configuration. Only parser string scratch and a bounded
/// recursion stack exist; no array, object, key list or full Value is retained.
pub(in crate::audio_engine) fn scan(
    encoded: &str,
    held: usize,
    retained: usize,
) -> Result<usize, String> {
    supported_layout()?;
    edit(encoded.len(), held, retained)?;
    let mut counter = ShapeCounter {
        heap: 0,
        serialized: encoded.len(),
        held,
        retained,
    };
    let mut deserializer = serde_json::Deserializer::from_str(encoded);
    ShapeSeed {
        counter: &mut counter,
        depth: 0,
        key: false,
    }
    .deserialize(&mut deserializer)
    .map_err(|e| e.to_string())?;
    deserializer.end().map_err(|e| e.to_string())?;
    let tree = sum(&[size_of::<Value>(), counter.heap])?;
    tree_admission(encoded.len(), tree, held, retained)?;
    Ok(tree)
}

/// Borrow existing Value storage without cloning it. The array allowance covers
/// both current capacity and future clone/deserialization doubling/minimum four.
pub(in crate::audio_engine) fn value_tree(value: &Value) -> Result<usize, String> {
    fn heap(value: &Value, depth: usize) -> Result<usize, String> {
        if depth >= MAX_JSON_DEPTH {
            return Err("beatmap JSON nesting limit".into());
        }
        match value {
            Value::String(text) => {
                Ok(string_heap(text.len())?.max(sum(&[text.capacity(), ALLOCATION_BYTES])?))
            }
            Value::Array(values) => {
                let capacity = product(values.len(), 2)?.max(4).max(values.capacity());
                values.iter().try_fold(
                    sum(&[product(capacity, size_of::<Value>())?, ALLOCATION_BYTES])?,
                    |n, value| sum(&[n, heap(value, depth + 1)?]),
                )
            }
            Value::Object(fields) => fields.iter().try_fold(
                object_heap(
                    fields
                        .len()
                        .checked_add(1)
                        .ok_or("beatmap_review_budget_overflow")?,
                )?,
                |n, (key, value)| {
                    sum(&[
                        n,
                        string_heap(key.len())?.max(sum(&[key.capacity(), ALLOCATION_BYTES])?),
                        heap(value, depth + 1)?,
                    ])
                },
            ),
            _ => Ok(0),
        }
    }
    supported_layout()?;
    sum(&[size_of::<Value>(), heap(value, 0)?])
}

pub(in crate::audio_engine) fn value_admission(
    value: &Value,
    held: usize,
    retained: usize,
) -> Result<usize, String> {
    tree_admission(serialized_len(value)?, value_tree(value)?, held, retained)
}
