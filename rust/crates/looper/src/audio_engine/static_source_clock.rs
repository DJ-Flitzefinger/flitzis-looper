//! Exact, non-owning static-grid convergence on the existing master projection.
//!
//! Admission uses a common dyadic grid g in [-126,0]. Every aligned coefficient
//! of F, O, the raw fraction, rates and every actual M over the complete u64
//! horizon is below 2^126. The raw whole and period have at most 64 bits. U has
//! fewer than 253 coefficient bits at 2g. With 6<=k<=64 the finest grid is
//! h=2g-k>=-316; whole/period terms need fewer than 380 bits, and signed sums,
//! residues and the final numerator fewer than 384. Thus 512-bit scratch is
//! sufficient. The denominator is a power of two at most 2^316. A nonzero tail
//! is normal binary64, and the final ties-even split has <=2^-54 frame error.
//! These bounds are checked at admission; this is not arbitrary-f64 support.
//!
//! PhysicalNormalized is a separate integral-Q<=u32::MAX class. It multiplies
//! raw pairs and physical periods by Q and uses exact P*master progress. At the
//! same grid its stored correction is <349 bits, final numerator <414 (cap448)
//! in checked512 scratch. Final absolute N/D uses <414/349 bits within the
//! existing physical642/636,1024-bit splitter theorem; P/Q is never a clock float.

use super::compact_source_numerics::{
    Grid, GridStats, decode_f64_at_grid, exact_f64_parts, master_grid_exponent,
};
use super::general_source_numerics::{
    Integer, Magnitude, PhysicalInteger, split_physical_coordinate,
};
use super::source_playback::FractionalSourcePosition;
use super::source_reader::{ExplicitSeekMode, SourceLoopDomain, SourceReaderPeriod};
use super::transport::TransportBeatProjection;

const MAX_PERIOD: f64 = 18_446_744_073_709_551_616.0;
const MAX_COEFFICIENT_BITS: u32 = 384;
const MAX_NORMALIZED_POSITION_BITS: u32 = 448;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StaticClockError {
    InvalidGeometry,
    InvalidEpoch,
    UnsupportedRate,
    NumericalUnavailable,
}

/// Coordinates only. The caller's original claim separately owns source,
/// timing, request, readers and every actual-read observation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StaticPhaseClock {
    projection: TransportBeatProjection,
    raw: FractionalSourcePosition,
    frames_per_beat: f64,
    origin_frame: f64,
    quarters: f64,
    master_at_first: f64,
    nominal_ratio: f64,
    steady_ratio: f64,
    physical_normalized: bool,
    quarter_scale: u32,
    physical_period: u64,
    correcting_ratio: f64,
    correction: [u8; 48],
    marker: u64,
    first_frame: u64,
    intended_frame: u64,
    capture_frame: u64,
    grid: Grid,
    k: u8,
    negative: bool,
}

impl StaticPhaseClock {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        raw: FractionalSourcePosition,
        domain: SourceLoopDomain,
        projection: TransportBeatProjection,
        execution: u64,
        marker: u64,
        frames_per_beat: f64,
        origin_frame: f64,
        quarters: f64,
        nominal_ratio: f64,
        launch_master: f64,
    ) -> Result<Self, StaticClockError> {
        Self::new_with_class(
            raw,
            domain,
            projection,
            execution,
            marker,
            frames_per_beat,
            origin_frame,
            quarters,
            nominal_ratio,
            launch_master,
            false,
        )
    }

    /// Separate physical class: exact Q-scaled progress, with no new reader period.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_physical_normalized(
        raw: FractionalSourcePosition,
        domain: SourceLoopDomain,
        projection: TransportBeatProjection,
        execution: u64,
        marker: u64,
        frames_per_beat: f64,
        origin_frame: f64,
        quarters: f64,
        nominal_ratio: f64,
        launch_master: f64,
    ) -> Result<Self, StaticClockError> {
        Self::new_with_class(
            raw,
            domain,
            projection,
            execution,
            marker,
            frames_per_beat,
            origin_frame,
            quarters,
            nominal_ratio,
            launch_master,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_class(
        raw: FractionalSourcePosition,
        domain: SourceLoopDomain,
        projection: TransportBeatProjection,
        execution: u64,
        marker: u64,
        frames_per_beat: f64,
        origin_frame: f64,
        quarters: f64,
        nominal_ratio: f64,
        launch_master: f64,
        physical_normalized: bool,
    ) -> Result<Self, StaticClockError> {
        use StaticClockError::{
            InvalidEpoch, InvalidGeometry, NumericalUnavailable, UnsupportedRate,
        };
        if marker == 0 || raw.seek_mode != ExplicitSeekMode::Normal {
            return Err(InvalidEpoch);
        }
        if !domain.is_valid()
            || domain.musical_period.is_mapped()
            || !frames_per_beat.is_finite()
            || frames_per_beat <= 0.0
            || !origin_frame.is_finite()
            || !quarters.is_finite()
            || !(0.0..MAX_PERIOD).contains(&quarters)
            || quarters == 0.0
            || !nominal_ratio.is_finite()
            || !(0.5..=2.0).contains(&nominal_ratio)
            || !raw.fraction.is_finite()
            || !(0.0..1.0).contains(&raw.fraction)
            || raw.frame < domain.region.start
        {
            return Err(InvalidGeometry);
        }
        let quarter_scale = if physical_normalized {
            if domain.musical_period != SourceReaderPeriod::Physical
                || quarters.fract() != 0.0
                || quarters < 1.0
                || quarters > f64::from(u32::MAX)
            {
                return Err(InvalidGeometry);
            }
            quarters as u32
        } else {
            1
        };
        let (m0, last) = projection
            .finite_beats_for_horizon(execution, u64::MAX)
            .ok_or(NumericalUnavailable)?;
        let output_fpb = projection.frames_per_beat();
        let mut stats = GridStats::new();
        for value in [
            frames_per_beat,
            origin_frame,
            raw.fraction,
            m0,
            launch_master,
            last,
            output_fpb,
            nominal_ratio,
            0.05,
        ] {
            stats = stats.include_f64(value).ok_or(NumericalUnavailable)?;
        }
        if physical_normalized {
            stats = stats.include_f64(quarters).ok_or(NumericalUnavailable)?;
        }
        let required = master_grid_exponent(output_fpb, projection.original_beat_anchor())
            .ok_or(NumericalUnavailable)?;
        let grid = stats
            .finish()
            .and_then(|base| base.refine(required))
            .ok_or(NumericalUnavailable)?;
        let g = grid.exponent();
        let scale = Integer::from_u128(u128::from(quarter_scale)).ok_or(NumericalUnavailable)?;
        let source = if physical_normalized {
            Integer::from_u128(domain.region.len() as u128)
                .and_then(|value| value.shl(u32::try_from(-g).ok()?))
        } else {
            on_grid(frames_per_beat, grid)
        }
        .ok_or(NumericalUnavailable)?;
        let output = on_grid(output_fpb, grid)
            .and_then(|value| value.mul(scale))
            .ok_or(NumericalUnavailable)?;
        let minimum = output;
        if source.shl(1).ok_or(NumericalUnavailable)? < minimum
            || source > output.shl(1).ok_or(NumericalUnavailable)?
        {
            return Err(UnsupportedRate);
        }
        let physical = period_at(domain, 2 * g).ok_or(InvalidGeometry)?;
        let half = scalar_at(0.5, 2 * g).ok_or(NumericalUnavailable)?;
        let maximum = scalar_at(MAX_PERIOD, 2 * g).ok_or(NumericalUnavailable)?;
        if physical < half || physical > maximum {
            return Err(InvalidGeometry);
        }
        // The exact class retains F*Q=P. The explicit normalized class retains
        // physical P and permits only the existing <=1-frame rounding extent.
        let product =
            exact_product_at(frames_per_beat, quarters, 2 * g).ok_or(NumericalUnavailable)?;
        if physical_normalized {
            let rounding = product.sub(physical).ok_or(NumericalUnavailable)?;
            if rounding.magnitude > scalar_at(1.0, 2 * g).ok_or(NumericalUnavailable)?.magnitude {
                return Err(InvalidGeometry);
            }
        } else if product != physical {
            return Err(InvalidGeometry);
        }
        let period = physical.mul(scale).ok_or(NumericalUnavailable)?;
        let raw_absolute = pair_at(raw.frame, raw.fraction, 2 * g)
            .and_then(|value| value.mul(scale))
            .ok_or(NumericalUnavailable)?;
        let raw_relative = pair_at(raw.frame - domain.region.start, raw.fraction, 2 * g)
            .and_then(|value| value.mul(scale))
            .ok_or(NumericalUnavailable)?;
        if raw_relative >= period {
            return Err(InvalidGeometry);
        }
        let target = pair_at(domain.region.start, 0.0, 2 * g)
            .and_then(|value| value.mul(scale))
            .and_then(|origin| {
                on_grid(m0, grid)?
                    .sub(on_grid(launch_master, grid)?)?
                    .mul(source)?
                    .add(origin)
            })
            .ok_or(NumericalUnavailable)?;
        let positive = residue(
            target.sub(raw_absolute).ok_or(NumericalUnavailable)?,
            period,
        )
        .ok_or(NumericalUnavailable)?;
        let negative = period.sub(positive).ok_or(NumericalUnavailable)?;
        let max_progress = on_grid(last, grid)
            .and_then(|value| value.sub(on_grid(m0, grid)?))
            .and_then(|value| value.mul(source))
            .ok_or(NumericalUnavailable)?;
        let directions = if positive <= negative {
            [(false, positive), (true, negative)]
        } else {
            [(true, negative), (false, positive)]
        };
        let mut selected = None;
        for (is_negative, distance) in directions {
            for k in 6_u8..=64 {
                if distance != Integer::ZERO
                    && max_progress < distance.shl(u32::from(k)).ok_or(NumericalUnavailable)?
                {
                    continue;
                }
                if rate_admitted(
                    source,
                    output,
                    nominal_ratio,
                    grid,
                    k,
                    is_negative,
                    distance == Integer::ZERO,
                )
                .ok_or(NumericalUnavailable)?
                {
                    selected = Some((is_negative, distance, k));
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        let (negative, distance, k) = selected.ok_or(UnsupportedRate)?;
        if distance.negative || distance.magnitude.bit_width() > MAX_COEFFICIENT_BITS {
            return Err(NumericalUnavailable);
        }
        let mut correction = [0; 48];
        correction.copy_from_slice(&distance.magnitude.as_bytes()[..48]);
        let factor = if negative {
            1.0 - 2.0_f64.powi(-i32::from(k))
        } else {
            1.0 + 2.0_f64.powi(-i32::from(k))
        };
        // Display/derivative metadata only. Authoritative positions never use
        // this rounded quotient: they retain exact Q-scaled integer arithmetic.
        let steady_ratio = if physical_normalized {
            (domain.region.len() as f64 / quarters / output_fpb).clamp(0.5, 2.0)
        } else {
            nominal_ratio
        };
        let mut clock = Self {
            projection,
            raw,
            frames_per_beat,
            origin_frame,
            quarters,
            master_at_first: m0,
            nominal_ratio,
            steady_ratio,
            physical_normalized,
            quarter_scale,
            physical_period: if physical_normalized {
                domain.region.len() as u64
            } else {
                0
            },
            correcting_ratio: (steady_ratio * factor).clamp(0.5, 2.0),
            correction,
            marker,
            first_frame: execution,
            intended_frame: execution,
            capture_frame: execution,
            grid,
            k,
            negative,
        };
        if distance != Integer::ZERO {
            // The unchanged master evaluator is nondecreasing over this proven horizon.
            // At most64 comparisons find its first captured frame, including plateaus.
            let mut low = execution;
            let mut high = u64::MAX;
            while low < high {
                let mid = low + (high - low) / 2;
                if clock.captured_at(mid).ok_or(NumericalUnavailable)? {
                    high = mid;
                } else {
                    low = mid + 1;
                }
            }
            clock.capture_frame = low;
        }
        if !same_pair(
            clock
                .position_at(domain, execution)
                .ok_or(NumericalUnavailable)?,
            raw,
        ) {
            return Err(NumericalUnavailable);
        }
        Ok(clock)
    }

    fn distance(self) -> Integer {
        let mut bytes = [0; 64];
        bytes[..48].copy_from_slice(&self.correction);
        Integer {
            negative: false,
            magnitude: Magnitude::from_bytes(bytes),
        }
    }

    fn progress_at(self, frame: u64) -> Option<Integer> {
        if frame < self.first_frame {
            return None;
        }
        let master = self.projection.beat_position_at_frame(frame);
        let source = if self.physical_normalized {
            Integer::from_u128(u128::from(self.physical_period))?
                .shl(u32::try_from(-self.grid.exponent()).ok()?)?
        } else {
            on_grid(self.frames_per_beat, self.grid)?
        };
        on_grid(master, self.grid)?
            .sub(on_grid(self.master_at_first, self.grid)?)?
            .mul(source)?
            .filter_nonnegative()
    }

    fn captured_at(self, frame: u64) -> Option<bool> {
        Some(self.progress_at(frame)? >= self.distance().shl(u32::from(self.k))?)
    }

    pub(crate) fn position_at(
        self,
        domain: SourceLoopDomain,
        frame: u64,
    ) -> Option<FractionalSourcePosition> {
        let h = 2 * self.grid.exponent() - i16::from(self.k);
        if self.physical_normalized
            && (domain.musical_period != SourceReaderPeriod::Physical
                || domain.region.len() as u64 != self.physical_period)
        {
            return None;
        }
        let scale = Integer::from_u128(u128::from(self.quarter_scale))?;
        let progress = self.progress_at(frame)?;
        let distance = self.distance().shl(u32::from(self.k))?;
        let correction = if progress < distance {
            progress
        } else {
            distance
        };
        let base = pair_at(
            self.raw.frame.checked_sub(domain.region.start)?,
            self.raw.fraction,
            h,
        )?
        .mul(scale)?;
        let advanced = base.add(progress.shl(u32::from(self.k))?)?;
        let corrected = if self.negative {
            advanced.sub(correction)?
        } else {
            advanced.add(correction)?
        };
        let limit = if self.physical_normalized {
            MAX_NORMALIZED_POSITION_BITS
        } else {
            MAX_COEFFICIENT_BITS
        };
        if corrected.magnitude.bit_width() > limit {
            return None;
        }
        let period = period_at(domain, h)?.mul(scale)?;
        if self.physical_normalized {
            let denominator = scale.shl(u32::try_from(-h).ok()?)?;
            let absolute = residue(corrected, period)?
                .add(Integer::from_u128(domain.region.start as u128)?.mul(denominator)?)?;
            let coordinate = split_physical_coordinate(
                PhysicalInteger::from_integer(absolute),
                PhysicalInteger::from_integer(denominator),
                self.raw.fraction,
                domain.region,
            )?;
            return Some(FractionalSourcePosition {
                frame: usize::try_from(coordinate.whole).ok()?,
                fraction: coordinate.fraction,
                seek_mode: ExplicitSeekMode::Normal,
            });
        }
        split_pair(
            residue(corrected, period)?,
            period,
            h,
            self.raw.fraction,
            domain.region.start,
        )
    }

    pub(crate) fn advance(&mut self, frames: usize) -> bool {
        let Some(next) = self.intended_frame.checked_add(frames as u64) else {
            return false;
        };
        self.intended_frame = next;
        true
    }

    pub(crate) fn chunk(self, max_frames: usize) -> (usize, f64) {
        let frames = if self.intended_frame < self.capture_frame {
            max_frames.min(
                usize::try_from(self.capture_frame - self.intended_frame).unwrap_or(usize::MAX),
            )
        } else {
            max_frames
        };
        (frames, self.ratio())
    }

    pub(crate) fn ratio(self) -> f64 {
        if self.settled() {
            self.steady_ratio
        } else {
            self.correcting_ratio
        }
    }
    pub(crate) fn nominal_ratio(self) -> f64 {
        self.nominal_ratio
    }
    pub(crate) fn is_physical_normalized(self) -> bool {
        self.physical_normalized
    }
    pub(crate) fn intended_frame(self) -> u64 {
        self.intended_frame
    }
    pub(crate) fn first_frame(self) -> u64 {
        self.first_frame
    }
    pub(crate) fn marker(self) -> u64 {
        self.marker
    }
    pub(crate) fn settled(self) -> bool {
        self.intended_frame >= self.capture_frame
    }
    pub(crate) fn master_period(self) -> f64 {
        self.projection.master_period_seconds()
    }
    pub(crate) fn matches_projection(self, other: TransportBeatProjection) -> bool {
        self.projection.matches_exact(other)
    }
    pub(crate) fn grid_matches(self, f: f64, o: f64, q: f64) -> bool {
        self.frames_per_beat.to_bits() == f.to_bits()
            && self.origin_frame.to_bits() == o.to_bits()
            && self.quarters.to_bits() == q.to_bits()
    }
    pub(crate) fn matches_exact(self, other: Self) -> bool {
        self.intended_frame == other.intended_frame && self.matches_program(other)
    }
    pub(crate) fn matches_program(self, other: Self) -> bool {
        self.projection.matches_exact(other.projection)
            && same_pair(self.raw, other.raw)
            && self.grid_matches(other.frames_per_beat, other.origin_frame, other.quarters)
            && self.master_at_first.to_bits() == other.master_at_first.to_bits()
            && self.nominal_ratio.to_bits() == other.nominal_ratio.to_bits()
            && self.steady_ratio.to_bits() == other.steady_ratio.to_bits()
            && self.physical_normalized == other.physical_normalized
            && self.quarter_scale == other.quarter_scale
            && self.physical_period == other.physical_period
            && self.correcting_ratio.to_bits() == other.correcting_ratio.to_bits()
            && self.correction == other.correction
            && self.marker == other.marker
            && self.first_frame == other.first_frame
            && self.capture_frame == other.capture_frame
            && self.grid.exponent() == other.grid.exponent()
            && self.k == other.k
            && self.negative == other.negative
    }
}

trait Nonnegative {
    fn filter_nonnegative(self) -> Option<Self>
    where
        Self: Sized;
}
impl Nonnegative for Integer {
    fn filter_nonnegative(self) -> Option<Self> {
        (!self.negative).then_some(self)
    }
}

fn on_grid(value: f64, grid: Grid) -> Option<Integer> {
    Integer::from_i128(decode_f64_at_grid(value, grid)?)
}

fn scalar_at(value: f64, exponent: i16) -> Option<Integer> {
    let (coefficient, actual) = exact_f64_parts(value)?;
    if coefficient == 0 {
        return Some(Integer::ZERO);
    }
    Integer::from_i128(coefficient)?.shl(u32::try_from(actual.checked_sub(exponent)?).ok()?)
}

fn pair_at(whole: usize, fraction: f64, exponent: i16) -> Option<Integer> {
    Integer::from_u128(whole as u128)?
        .shl(u32::try_from(-exponent).ok()?)?
        .add(scalar_at(fraction, exponent)?)
}

fn exact_product_at(left: f64, right: f64, exponent: i16) -> Option<Integer> {
    let (a, ae) = exact_f64_parts(left)?;
    let (b, be) = exact_f64_parts(right)?;
    Integer::from_i128(a)?
        .mul(Integer::from_i128(b)?)?
        .shl(u32::try_from(ae.checked_add(be)?.checked_sub(exponent)?).ok()?)
}

fn period_at(domain: SourceLoopDomain, exponent: i16) -> Option<Integer> {
    match domain.musical_period {
        SourceReaderPeriod::Physical => {
            Integer::from_u128(domain.region.len() as u128)?.shl(u32::try_from(-exponent).ok()?)
        }
        SourceReaderPeriod::ScalarMusical(bits) => scalar_at(f64::from_bits(bits), exponent),
        SourceReaderPeriod::MappedRational(_) | SourceReaderPeriod::MappedPhysicalQuarters(_) => {
            None
        }
    }
}

fn residue(value: Integer, period: Integer) -> Option<Integer> {
    if period.negative || period.magnitude.is_zero() {
        return None;
    }
    let remainder = value.magnitude.checked_rem(period.magnitude)?;
    Some(Integer {
        negative: false,
        magnitude: if value.negative && !remainder.is_zero() {
            period.magnitude.checked_sub(remainder)?
        } else {
            remainder
        },
    })
}

fn rate_admitted(
    source: Integer,
    output: Integer,
    previous: f64,
    grid: Grid,
    k: u8,
    negative: bool,
    zero: bool,
) -> Option<bool> {
    let power = 1_u128 << k;
    let factor = if zero {
        power
    } else if negative {
        power - 1
    } else {
        power + 1
    };
    let numerator = source.mul(Integer::from_u128(factor)?)?;
    let denominator = output.shl(u32::from(k))?;
    if numerator.shl(1)? < denominator || numerator > denominator.shl(1)? {
        return Some(false);
    }
    if !zero && source.shl(5)? > denominator {
        return Some(false);
    }
    let difference = numerator
        .shl((-grid.exponent()) as u32)?
        .sub(on_grid(previous, grid)?.mul(denominator)?)?;
    let limit = on_grid(0.05, grid)?.mul(denominator)?;
    Some(difference.magnitude <= limit.magnitude)
}

fn split_pair(
    value: Integer,
    period: Integer,
    exponent: i16,
    raw_fraction: f64,
    start: usize,
) -> Option<FractionalSourcePosition> {
    if value.negative || value >= period || exponent > 0 || exponent < -316 {
        return None;
    }
    let shift = u32::try_from(-exponent).ok()?;
    let whole = value.magnitude.checked_shr(shift)?;
    let mut frame = start.checked_add(usize::try_from(whole).ok()?)?;
    let remainder = value.magnitude.checked_sub(whole.checked_shl(shift)?)?;
    let raw = scalar_at(raw_fraction, exponent)?;
    let mut fraction = if !raw.negative && raw.magnitude == remainder {
        raw_fraction
    } else if remainder.is_zero() {
        0.0
    } else {
        let bits = remainder.bit_width();
        let dropped = bits.saturating_sub(53);
        let significand = remainder.checked_shr(dropped)?;
        let mut significand = u64::try_from(significand).ok()?;
        if dropped != 0 {
            let tail = remainder.checked_sub(
                Magnitude::try_from(significand)
                    .ok()?
                    .checked_shl(dropped)?,
            )?;
            let half = Magnitude::try_from(1_u64).ok()?.checked_shl(dropped - 1)?;
            if tail > half || (tail == half && significand & 1 != 0) {
                significand += 1;
            }
        }
        significand as f64 * 2.0_f64.powi(i32::try_from(dropped).ok()? + i32::from(exponent))
    };
    if fraction == 1.0 {
        frame = frame.checked_add(1)?;
        fraction = 0.0;
    }
    // Rounding to a fractional musical seam belongs to the next loop beginning.
    if pair_at(frame.checked_sub(start)?, fraction, exponent)? >= period {
        frame = start;
        fraction = 0.0;
    }
    Some(FractionalSourcePosition {
        frame,
        fraction,
        seek_mode: ExplicitSeekMode::Normal,
    })
}

fn same_pair(left: FractionalSourcePosition, right: FractionalSourcePosition) -> bool {
    left.frame == right.frame
        && left.fraction.to_bits() == right.fraction.to_bits()
        && left.seek_mode == right.seek_mode
}
