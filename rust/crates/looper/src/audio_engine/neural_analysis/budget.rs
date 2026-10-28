//! Checked neural metadata byte accounting, with the existing 512-MiB owner.
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};
const ALLOCATION_BYTES: usize = 16;
const BTREE_NODE_BYTES: usize = 1024;
const MAX_JSON_DEPTH: usize = 128;
const MAX_RECORD_BYTES: usize = 64 * 1024 * 1024;

pub(in crate::audio_engine) fn sum(values: &[usize]) -> Result<usize, String> {
    values
        .iter()
        .try_fold(0_usize, |sum, value| sum.checked_add(*value))
        .ok_or_else(|| "neural_review_budget_overflow".into())
}

pub(in crate::audio_engine) fn serialized_len(value: &impl Serialize) -> Result<usize, String> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|size| *size <= MAX_RECORD_BYTES)
                .ok_or_else(|| io::Error::other("neural_byte_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|_| "neural_byte_limit")?;
    Ok(counter.0)
}

fn supported_layout() -> Result<(), String> {
    if size_of::<usize>() != 8
        || size_of::<Value>() != 32
        || size_of::<String>() != 24
        || size_of::<serde_json::Map<String, Value>>()
            != size_of::<std::collections::BTreeMap<String, Value>>()
    {
        return Err("neural JSON memory layout is unsupported".into());
    }
    Ok(())
}

fn product(a: usize, b: usize) -> Result<usize, String> {
    a.checked_mul(b)
        .ok_or_else(|| "neural_review_budget_overflow".into())
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

pub(in crate::audio_engine) fn value_tree(value: &Value) -> Result<usize, String> {
    fn heap(value: &Value, depth: usize) -> Result<usize, String> {
        if depth >= MAX_JSON_DEPTH {
            return Err("neural JSON nesting limit".into());
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
                        .ok_or("neural_review_budget_overflow")?,
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
