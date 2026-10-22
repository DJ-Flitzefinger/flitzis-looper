//! Checked cold-job peak admission, before JSON/index/copy growth.

use serde::Serialize;
use std::io::{self, Write};

use super::{MAX_ANCHORS, MAX_RECORD_BYTES};

pub(in crate::audio_engine) const MAX_BUILD_BYTES: usize = 512 * 1024 * 1024;
pub(super) const FIXED_BYTES: usize = 16 * 1024 * 1024;
pub(super) const EDIT_ALLOWANCE: usize = 128 * 1024;
mod shape;
pub(in crate::audio_engine) use shape::{scan, tree_admission};
pub(in crate::audio_engine) use shape::{value_admission, value_tree};

pub(in crate::audio_engine) fn sum(values: &[usize]) -> Result<usize, String> {
    values
        .iter()
        .try_fold(0_usize, |sum, value| sum.checked_add(*value))
        .ok_or_else(|| "beatmap_review_budget_overflow".into())
}

/// Preserve the original interpretation guard's arithmetic and public values.
pub(in crate::audio_engine) fn peak(
    serialized: usize,
    held: usize,
    labels: usize,
    index: usize,
    retained: usize,
) -> Result<usize, String> {
    let trees = serialized
        .checked_mul(32)
        .ok_or("beatmap_review_budget_overflow")?;
    let labels = labels
        .checked_mul(8192)
        .ok_or("beatmap_review_budget_overflow")?;
    let peak = sum(&[trees, held, labels, FIXED_BYTES, index, retained])?;
    if peak > MAX_BUILD_BYTES {
        return Err("beatmap review exceeds 512-MiB simultaneous job peak".into());
    }
    Ok(peak)
}

/// Metadata, geometry and observations all coexist with the canonical record.
/// One byte of serialized input is a conservative upper bound on each row count.
pub(in crate::audio_engine) fn index_bound(serialized: usize) -> Result<usize, String> {
    serialized
        .min(MAX_ANCHORS)
        .checked_mul(128)
        .ok_or_else(|| "beatmap_review_budget_overflow".into())
}

#[cfg(test)]
pub(in crate::audio_engine) fn parse(serialized: usize, index: usize) -> Result<usize, String> {
    if serialized > MAX_RECORD_BYTES {
        return Err("beatmap_byte_limit".into());
    }
    peak(serialized, 0, 0, index, 0)
}

/// A current record plus its complete accepted geometry bounds every old
/// position/interval/window inverse. The allowance covers at most 63 new rows,
/// their intervals, a bounded intent and the closed lineage wrapper.
pub(in crate::audio_engine) fn edit_bound(base: usize) -> Result<usize, String> {
    let bound = sum(&[base, base, EDIT_ALLOWANCE])?;
    if bound > MAX_RECORD_BYTES {
        return Err("beatmap_byte_limit".into());
    }
    Ok(bound)
}

pub(in crate::audio_engine) fn edit(
    serialized_bound: usize,
    held: usize,
    retained: usize,
) -> Result<usize, String> {
    if serialized_bound > MAX_RECORD_BYTES {
        return Err("beatmap_byte_limit".into());
    }
    peak(
        serialized_bound,
        held,
        0,
        index_bound(serialized_bound)?,
        retained,
    )
}

/// Counting does not construct another output buffer or clone old provenance.
pub(in crate::audio_engine) fn serialized_len(value: &impl Serialize) -> Result<usize, String> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|size| *size <= MAX_RECORD_BYTES)
                .ok_or_else(|| io::Error::other("beatmap_byte_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|_| "beatmap_byte_limit")?;
    Ok(counter.0)
}

#[cfg(test)]
mod shape_tests;
