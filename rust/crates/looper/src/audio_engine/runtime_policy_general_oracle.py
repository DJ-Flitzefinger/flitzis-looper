"""Independent original-knot rational oracle for bounded Rust regression fixtures.

Only Python's standard-library Fraction, integer rounding and struct are used.
Actual binary64 master evaluations are supplied by the authoritative Rust timeline.
"""

# Embedded by a Rust test; this resource is not a Python import package.
# ruff: noqa: INP001
import struct
from fractions import Fraction


def _f64_bits(value: float) -> int:
    return int(struct.unpack(">Q", struct.pack(">d", value))[0])


def _rn32_bits(value: Fraction) -> int:
    assert 0 <= value <= 1
    if not value:
        return 0
    n, d = value.numerator, value.denominator
    exponent = n.bit_length() - d.bit_length()
    if exponent >= 0:
        if n < d << exponent:
            exponent -= 1
    elif n << -exponent < d:
        exponent -= 1
    scale = max(-149, exponent - 23)
    scaled_n = n << max(0, -scale)
    scaled_d = d << max(0, scale)
    whole, remainder = divmod(scaled_n, scaled_d)
    if 2 * remainder > scaled_d or (2 * remainder == scaled_d and whole & 1):
        whole += 1
    if scale == -149 and whole < 1 << 23:
        return whole
    if whole >= 1 << 24:
        assert whole == 1 << 24
        whole >>= 1
        scale += 1
    assert 1 << 23 <= whole < 1 << 24
    exponent = scale + 23
    return ((exponent + 127) << 23) | (whole - (1 << 23))


def _axes(anchors: list[tuple[float, float]], rate: int) -> list[tuple[Fraction, Fraction]]:
    return [(Fraction(seconds) * rate, Fraction(beat)) for seconds, beat in anchors]


def _affine(
    points: list[tuple[Fraction, Fraction]], value: Fraction, *, inverse: bool = False
) -> Fraction:
    if inverse:
        points = [(beat, source) for source, beat in points]
    for index in range(len(points) - 1):
        left, right = points[index], points[index + 1]
        if left[0] <= value <= right[0]:
            return left[1] + (value - left[0]) * (right[1] - left[1]) / (right[0] - left[0])
    message = "original trusted fixture interval is required"
    raise AssertionError(message)


def metadata(
    anchors: list[tuple[float, float]], rate: int, start: int, quarters: float
) -> tuple[str, str, str, str]:
    points = _axes(anchors, rate)
    beginning = _affine(points, Fraction(start))
    period = _affine(points, beginning + Fraction(quarters), inverse=True) - start
    return (
        str(beginning.numerator),
        str(beginning.denominator),
        str(period.numerator),
        str(period.denominator),
    )


def _position(
    points: list[tuple[Fraction, Fraction]],
    beginning: Fraction,
    quarters: Fraction,
    initial: Fraction,
    delta: Fraction,
    raw_fraction: float,
    start: int,
    period: Fraction,
) -> tuple[int, float]:
    phase = (initial - beginning + delta) % quarters
    exact = _affine(points, beginning + phase, inverse=True)
    assert start <= exact < start + period
    whole = exact.numerator // exact.denominator
    fraction_exact = exact - whole
    fraction = raw_fraction if fraction_exact == Fraction(raw_fraction) else float(fraction_exact)
    if fraction == 1:
        whole += 1
        fraction = 0.0
    if Fraction(whole) + Fraction(fraction) >= start + period:
        whole, fraction = start, 0.0
    return whole, fraction


def _taps(
    start: int, physical_length: int, period: Fraction, whole: int, fraction: float
) -> tuple[int, int | None, int]:
    ceil_period = -(-period.numerator // period.denominator)
    last = min(ceil_period - 1, physical_length - 1)
    phase = Fraction(whole - start) + Fraction(fraction)
    if period != physical_length and phase >= last:
        left = start + last
        right: int | None = start
        alpha = (phase - last) / (period - last)
        gain = _rn32_bits(alpha)
    else:
        left = start + (whole - start) % physical_length
        right = start + (whole + 1 - start) % physical_length if fraction else None
        # The input is already an exact binary64 reader fraction; preserve -0.
        gain = int(struct.unpack(">I", struct.pack(">f", fraction))[0])
    return left, right, gain


def evaluate(
    anchors: list[tuple[float, float]],
    rate: int,
    start: int,
    physical_length: int,
    quarters: float,
    raw_frame: int,
    raw_fraction: float,
    master0: float,
    masters: list[float],
) -> list[tuple[int, int, int, int | None, int]]:
    points = _axes(anchors, rate)
    beginning = _affine(points, Fraction(start))
    extent = Fraction(quarters)
    period = _affine(points, beginning + extent, inverse=True) - start
    initial = _affine(points, Fraction(raw_frame) + Fraction(raw_fraction))
    answers = []
    for master in masters:
        whole, fraction = _position(
            points,
            beginning,
            extent,
            initial,
            Fraction(master) - Fraction(master0),
            raw_fraction,
            start,
            period,
        )
        answers.append((
            whole,
            _f64_bits(fraction),
            *_taps(start, physical_length, period, whole, fraction),
        ))
    return answers
