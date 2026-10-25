//! Checked signed fixed storage shared by independently proved numeric classes.
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BoundedInteger<const N: usize> {
    pub(super) negative: bool,
    pub(super) magnitude: bnum::Uint<N>,
}

impl<const N: usize> BoundedInteger<N> {
    pub(super) const ZERO: Self = Self {
        negative: false,
        magnitude: bnum::Uint::<N>::MIN,
    };
    pub(super) fn canonical(magnitude: bnum::Uint<N>, negative: bool) -> Self {
        Self {
            negative: negative && !magnitude.is_zero(),
            magnitude,
        }
    }
    pub(super) fn from_i128(value: i128) -> Option<Self> {
        Some(Self::canonical(
            bnum::Uint::<N>::try_from(value.unsigned_abs()).ok()?,
            value < 0,
        ))
    }
    pub(super) fn from_u128(value: u128) -> Option<Self> {
        Some(Self::canonical(
            bnum::Uint::<N>::try_from(value).ok()?,
            false,
        ))
    }
    pub(super) fn add(self, other: Self) -> Option<Self> {
        if self.negative == other.negative {
            return Some(Self::canonical(
                self.magnitude.checked_add(other.magnitude)?,
                self.negative,
            ));
        }
        let (large, small) = if self.magnitude >= other.magnitude {
            (self, other)
        } else {
            (other, self)
        };
        Some(Self::canonical(
            large.magnitude.checked_sub(small.magnitude)?,
            large.negative,
        ))
    }
    pub(super) fn sub(self, other: Self) -> Option<Self> {
        self.add(Self::canonical(other.magnitude, !other.negative))
    }
    pub(super) fn mul(self, other: Self) -> Option<Self> {
        Some(Self::canonical(
            self.magnitude.checked_mul(other.magnitude)?,
            self.negative != other.negative,
        ))
    }
    pub(super) fn shl(self, shift: u32) -> Option<Self> {
        if shift >= bnum::Uint::<N>::BITS
            || self.magnitude.bit_width().checked_add(shift)? > bnum::Uint::<N>::BITS
        {
            return None;
        }
        Some(Self::canonical(
            self.magnitude.checked_shl(shift)?,
            self.negative,
        ))
    }
    pub(super) fn quotient(self, other: Self) -> Option<Self> {
        if self.negative || other.negative || other.magnitude.is_zero() {
            return None;
        }
        Some(Self::canonical(
            self.magnitude.checked_div(other.magnitude)?,
            false,
        ))
    }
    pub(super) fn u128(self) -> Option<u128> {
        (!self.negative).then_some(())?;
        u128::try_from(self.magnitude).ok()
    }
    pub(super) fn resize<const M: usize>(self) -> Option<BoundedInteger<M>> {
        if self.magnitude.bit_width() > bnum::Uint::<M>::BITS {
            return None;
        }
        let mut bytes = [0; M];
        let count = N.min(M);
        bytes[..count].copy_from_slice(&self.magnitude.as_bytes()[..count]);
        Some(BoundedInteger::<M>::canonical(
            bnum::Uint::<M>::from_bytes(bytes),
            self.negative,
        ))
    }
    pub(super) fn bounded(self, bits: u32) -> Option<Self> {
        (self.magnitude.bit_width() <= bits).then_some(self)
    }
}
impl<const N: usize> Ord for BoundedInteger<N> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (true, true) => other.magnitude.cmp(&self.magnitude),
            (false, false) => self.magnitude.cmp(&other.magnitude),
        }
    }
}
impl<const N: usize> PartialOrd for BoundedInteger<N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
