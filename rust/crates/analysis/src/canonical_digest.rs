//! Shared canonical identities for immutable offline analysis evidence.

use sha2::{Digest, Sha256};

/// Length-delimited, little-endian encoding with unmodified floating-point bits.
pub(crate) struct CanonicalDigest(Sha256);

impl CanonicalDigest {
    pub(crate) fn new(domain: &str) -> Self {
        let mut digest = Self(Sha256::new());
        digest.text(domain);
        digest
    }

    pub(crate) fn number(&mut self, number: u64) {
        self.0.update(number.to_le_bytes());
    }

    pub(crate) fn signed(&mut self, number: i64) {
        self.0.update(number.to_le_bytes());
    }

    pub(crate) fn text(&mut self, value: &str) {
        self.number(value.len() as u64);
        self.0.update(value.as_bytes());
    }

    pub(crate) fn float(&mut self, value: f64) {
        self.number(value.to_bits());
    }

    pub(crate) fn float_array(&mut self, values: &[f64]) {
        self.sequence(values, |hash, value| hash.float(*value));
    }

    pub(crate) fn sequence<T>(&mut self, values: &[T], mut write: impl FnMut(&mut Self, &T)) {
        self.number(values.len() as u64);
        for value in values {
            write(self, value);
        }
    }

    pub(crate) fn optional<T>(&mut self, value: Option<T>, write: impl FnOnce(&mut Self, T)) {
        self.number(u64::from(value.is_some()));
        if let Some(value) = value {
            write(self, value);
        }
    }

    pub(crate) fn finish(self) -> String {
        format!("{:x}", self.0.finalize())
    }
}
