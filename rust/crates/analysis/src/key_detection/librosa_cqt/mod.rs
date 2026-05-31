//! Librosa-compatible CQT implementation (extracted from rosa).
//!
//! Provides `cqt()` that matches `librosa.cqt()` output numerically.

mod dsp;
mod matrix;
mod stft;
mod windows;

pub mod convert;
pub mod cqt;

#[cfg(test)]
mod tests;

pub use cqt::{CqtConfig, cqt};
