"""Independent field controls for explicit analytic group amplitude replacement."""

import cmath
import math

import numpy as np
from pitch_causal_model_probe import emit
from pitch_group_magnitude import replace_group_magnitude
from pitch_independent_modes import sample_complex

_FRAMES = 1037


def _components() -> tuple[tuple[np.ndarray, ...], tuple[np.ndarray, ...]]:
    clock = np.arange(_FRAMES)
    spatial = np.array([0.8, 0.6 * np.exp(0.37j)])
    sources, transported = [], []
    for amplitude, start, end, width, frequency, phase in (
        (0.31, 50, 850, 15, 0.031, 0.2),
        (0.19, 71, 911, 23, 0.047, -0.6),
    ):
        rise = 0.5 * (1 - np.cos(np.pi * np.clip((clock - start) / width, 0, 1)))
        fall = 0.5 * (1 - np.cos(np.pi * np.clip((end - clock) / (width + 6), 0, 1)))
        values = amplitude * rise * fall * np.exp(1.0j * (2 * np.pi * frequency * clock + phase))
        source = values[:, None] * spatial
        pitched = source * np.exp(1.0j * (0.012 * clock - phase))[:, None]
        source.setflags(write=False)
        pitched.setflags(write=False)
        sources.append(source)
        transported.append(pitched)
    return tuple(sources), tuple(transported)


def _scalar_reference(sources: tuple, transported: tuple) -> np.ndarray:
    result = np.zeros((_FRAMES, 2), dtype=np.complex128)
    for frame in range(_FRAMES):
        original = [sum(complex(item[frame, channel]) for item in sources) for channel in (0, 1)]
        pitched = [sum(complex(item[frame, channel]) for item in transported) for channel in (0, 1)]
        amplitude = math.hypot(*(part for value in original for part in (value.real, value.imag)))
        norm = math.hypot(*(part for value in pitched for part in (value.real, value.imag)))
        if norm:
            result[frame] = [amplitude * value / norm for value in pitched]
        else:
            assert amplitude == 0.0
    return result


def _positive_controls(sources: tuple, transported: tuple) -> dict:
    snapshots = [item.tobytes() for item in (*sources, *transported)]
    actual, report = replace_group_magnitude(sources, transported)
    assert actual is not None
    expected = _scalar_reference(sources, transported)
    error = float(np.max(np.abs(actual - expected)))
    assert error <= 2e-12
    assert snapshots == [item.tobytes() for item in (*sources, *transported)]
    scale_errors = []
    for exponent in (-400, 400):
        scale = 2.0**exponent
        scaled, _ = replace_group_magnitude(
            tuple(item * scale for item in sources), tuple(item * scale for item in transported)
        )
        assert scaled is not None
        distance = float(np.max(np.abs(scaled / scale - actual)))
        assert distance <= 2e-12
        scale_errors.append({"binary_exponent": exponent, "normalized_error": distance})
    return {
        "independent_scalar_equation_max_abs_error": error,
        "inputs_preserved_bitwise": True,
        "scale_invariance": scale_errors,
        "numeric_report": report,
    }


def _identity_controls(sources: tuple) -> list[dict]:
    records = []
    for rate in (0.75, 1.0, 1.25, 2.0):
        positions = rate * np.arange(math.ceil((_FRAMES + 1) / rate)) - 0.25
        samples = tuple(sample_complex(item, positions) for item in sources)
        actual, _ = replace_group_magnitude(samples, samples)
        assert actual is not None
        expected = sum(samples, np.zeros_like(samples[0]))
        error = float(np.max(np.abs(actual - expected)))
        assert error <= 2e-12
        records.append({
            "rate_equals_shift": rate,
            "fractional_coordinates": True,
            "group_complex_identity_max_abs_error": error,
        })
    return records


def _stereo_controls(sources: tuple, transported: tuple) -> dict:
    baseline, _ = replace_group_magnitude(sources, transported)
    assert baseline is not None
    angle, phase = 0.47, cmath.exp(0.41j)
    unitary = np.array([
        [math.cos(angle), math.sin(angle) * phase],
        [-math.sin(angle) * phase.conjugate(), math.cos(angle)],
    ])
    rotated, _ = replace_group_magnitude(
        tuple(item @ unitary for item in sources), tuple(item @ unitary for item in transported)
    )
    assert rotated is not None
    rotation_error = float(np.max(np.abs(rotated - baseline @ unitary)))
    assert rotation_error <= 2e-12
    shifted, _ = replace_group_magnitude(
        tuple(item * cmath.exp(0.731j) for item in sources),
        tuple(item * cmath.exp(0.731j) for item in transported),
    )
    assert shifted is not None
    phase_error = float(np.max(np.abs(shifted - baseline * cmath.exp(0.731j))))
    assert phase_error <= 2e-12
    spatial = tuple(np.zeros_like(item) for item in sources)
    pitched = tuple(np.zeros_like(item) for item in transported)
    for channel in (0, 1):
        spatial[channel][:, channel] = sources[channel][:, channel]
        pitched[channel][:, channel] = transported[channel][:, channel]
    separate, _ = replace_group_magnitude(spatial, pitched)
    assert separate is not None
    separate_error = float(np.max(np.abs(separate - sum(pitched, np.zeros_like(separate)))))
    assert separate_error <= 2e-12
    return {
        "unitary_stereo_covariance_max_abs_error": rotation_error,
        "global_phase_covariance_max_abs_error": phase_error,
        "orthogonal_channel_operator_identity_max_abs_error": separate_error,
        "kernel_operator_only_not_same_spatial_admission": True,
    }


def _invalid_controls() -> dict:
    valid = np.ones((3, 2), dtype=np.complex128)
    cases = (
        ("non_tuple", [valid], (valid,)),
        ("unequal_components", (valid,), (valid, valid)),
        ("three_components", (valid, valid, valid), (valid, valid, valid)),
        ("real_dtype", (valid.real,), (valid,)),
        ("different_frames", (valid,), (valid[:2],)),
        ("wrong_channels", (valid[:, :1],), (valid[:, :1],)),
        ("nan", (np.full_like(valid, complex("nan")),), (valid,)),
        ("infinity", (valid,), (np.full_like(valid, complex("inf")),)),
    )
    refused = []
    for label, source, transported in cases:
        try:
            replace_group_magnitude(source, transported)
        except ValueError:
            refused.append(label)
        else:
            message = f"Malformed group input accepted: {label}"
            raise AssertionError(message)
    dynamic = valid.copy()
    dynamic[0, 0] = np.finfo(np.float64).max / 4
    dynamic[1, 1] = np.nextafter(0.0, 1.0)
    lost, report = replace_group_magnitude((dynamic,), (dynamic,))
    assert lost is None
    assert report["refusal_reason"] == "normalization_range_loss"
    return {"malformed_contracts_refused": refused, "actual_extreme_dynamic_range_refusal": report}


def _resource_controls() -> dict:
    backing = np.zeros((1, 2), dtype=np.complex128)
    huge = np.broadcast_to(backing, (2_000_000, 2))
    guarded, memory_report = replace_group_magnitude((huge,), (huge,))
    assert guarded is None
    assert backing.nbytes == 32
    maximum = np.finfo(np.float64).max
    large = np.full((3, 2), 0.8 * maximum, dtype=np.complex128)
    overflow, range_report = replace_group_magnitude((large, large), (large, large))
    assert overflow is None
    return {
        "actual_readonly_broadcast_memory_guard": memory_report,
        "unrepresentable_group_amplitude": range_report,
    }


def _normalization_cancellation_controls() -> dict:
    first = np.array([[0.7, 2.7]], dtype=np.complex128)
    second = np.array([[-np.nextafter(0.7, np.inf), -2.7]], dtype=np.complex128)
    assert first[0, 0].real / 2.7 == -second[0, 0].real / 2.7
    assert (first + second)[0, 0] != 0.0
    cancelled, adjacent_report = replace_group_magnitude((first, second), (first, second))
    assert cancelled is None
    assert adjacent_report["refusal_reason"] == "uncertain_transported_phase"
    assert adjacent_report["source_active_frames"] == 1
    tiny = np.finfo(np.float64).tiny
    first = np.array([[np.nextafter(2 * tiny, 0.0), 2.0]], dtype=np.complex128)
    second = np.array([[-2 * tiny, -2.0]], dtype=np.complex128)
    assert first[0, 0].real / 2 == -second[0, 0].real / 2 == tiny
    assert (first + second)[0, 0] != 0.0
    cancelled, boundary_report = replace_group_magnitude((first, second), (first, second))
    assert cancelled is None
    assert boundary_report["refusal_reason"] == "normalization_range_loss"
    assert boundary_report["normalization_inexact_components"] == 1
    return {
        "adjacent_floats_do_not_manufacture_zero": adjacent_report,
        "subnormal_rounding_up_to_normal_boundary_refused": boundary_report,
    }


def _phase_controls() -> dict:
    one = np.ones((19, 2), dtype=np.complex128)
    zero = np.zeros_like(one)
    all_zero, _ = replace_group_magnitude((zero, zero), (zero, zero))
    assert all_zero is not None
    assert np.array_equal(all_zero, zero)
    source_cancellation, _ = replace_group_magnitude((one, -one), (one, one))
    assert source_cancellation is not None
    assert np.array_equal(source_cancellation, zero)
    undefined, exact_report = replace_group_magnitude((one, one), (one, -one))
    assert undefined is None
    almost = one * cmath.exp(1.0j * (math.pi - 2.0**-48))
    unresolved, near_report = replace_group_magnitude((one, one), (one, almost))
    assert unresolved is None
    return {
        "exact_zero_fields_and_source_cancellation_retained": True,
        "undefined_phase": exact_report,
        "near_cancellation_phase": near_report,
        "normalization_cancellation": _normalization_cancellation_controls(),
    }


def integrity_controls() -> dict:
    """Check independent arithmetic, source interpolation and genuine refusals."""
    sources, transported = _components()
    return {
        "stage": "source_group_magnitude_integrity_controls",
        "passed": True,
        "positive_fields": _positive_controls(sources, transported),
        "fractional_identity": _identity_controls(sources),
        "stereo_and_global_phase": _stereo_controls(sources, transported),
        "invalid_contracts_and_range": _invalid_controls(),
        "phase_refusals": _phase_controls(),
        "resource_refusals": _resource_controls(),
        "real_PCM_2ms_or_tonal_or_alias_acceptance_claim": False,
    }


if __name__ == "__main__":
    emit(integrity_controls())
