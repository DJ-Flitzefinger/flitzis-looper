"""Independent finite-kernel and scalar phase controls for the joint experiment."""

import cmath
import math

import numpy as np
import pitch_amfm_model as bank
import pitch_joint_residual_phase as model
from pitch_causal_model_probe import emit
from pitch_overlap_model import source_cues

_ABS_LIMIT = 2e-10


def _kernel_controls() -> dict:
    impulse = np.array([[1.0, 0.5]])
    prepared = model.prepare(impulse)
    field = prepared.analytic_pcm
    reference = sum(
        (bank._kernel(band) for band in range(bank.BAND_COUNT)),
        np.zeros(bank.BANK_FRAMES, dtype=np.complex128),
    )
    error = float(np.max(np.abs(field[:, 0] - reference)))
    assert error <= 2e-12
    assert np.array_equal(field.imag[::2], np.zeros_like(field.imag[::2]))
    assert np.array_equal(field.imag[-1], np.zeros(2))
    assert np.array_equal(field.real[1:], np.zeros_like(field.real[1:]))
    assert np.array_equal(field[:, 1], 0.5 * field[:, 0])
    return {
        "complete_513_band_kernel_sum_max_abs_error": error,
        "comparison_absolute_limit": 2e-12,
        "even_quadrature_and_final_tap_exactly_zero": True,
        "real_kernel_is_exact_unit_impulse": True,
        "proportional_stereo_exact": True,
        "nonzero_odd_quadrature_taps": int(np.count_nonzero(field[:, 0].imag)),
    }


def _scalar_field(source: np.ndarray) -> np.ndarray:
    count = len(source) + bank.BANK_FRAMES - 1
    result = np.zeros((count, 2), dtype=np.complex128)
    result[: len(source)].real = source
    for lag in range(1, bank.BANK_FRAMES - 1, 2):
        window = 0.5 + 0.5 * math.cos(math.pi * lag / (bank.BANK_FRAMES - 1))
        coefficient = 2 * window / bank.BANK_FRAMES / math.tan(math.pi * lag / bank.BANK_FRAMES)
        for frame in range(len(source)):
            for channel in (0, 1):
                result[frame + lag, channel] += 1.0j * coefficient * float(source[frame, channel])
    return result


def _scalar_phase(field: np.ndarray) -> np.ndarray:
    elapsed = np.zeros(len(field))
    previous_active = False
    for frame, values in enumerate(field):
        active = any(complex(value) != 0.0 for value in values)
        if active and previous_active:
            products = [
                complex(field[frame - 1, channel]).conjugate() * complex(values[channel])
                for channel in (0, 1)
            ]
            pooled = sum(products)
            if pooled == 0.0:
                pooled = products[max(range(2), key=lambda index: abs(products[index]))]
            edge = (cmath.phase(pooled) + math.pi) % (2 * math.pi) - math.pi if pooled else 0.0
            elapsed[frame] = elapsed[frame - 1] + edge
        previous_active = active
    return elapsed


def _sample(values: np.ndarray, position: float) -> complex:
    lower = math.floor(position)
    fraction = position - lower
    left = complex(values[lower]) if 0 <= lower < len(values) else 0.0j
    right = complex(values[lower + 1]) if 0 <= lower + 1 < len(values) else 0.0j
    return (1 - fraction) * left + fraction * right


def _scalar_render(
    field: np.ndarray, rate: float, shift: float, origin: int, count: int
) -> np.ndarray:
    elapsed = _scalar_phase(field)
    result = np.zeros((count, 2))
    for frame in range(count):
        position = rate * (origin + frame)
        phase = _sample(elapsed, position).real
        correction = cmath.exp(1.0j * (shift / rate - 1) * phase)
        for channel in (0, 1):
            result[frame, channel] = (_sample(field[:, channel], position) * correction).real
    return result


def _source_controls(source: np.ndarray) -> tuple[object, dict]:
    original = source.tobytes()
    prepared = model.prepare(source)
    expected = _scalar_field(source)
    error = float(np.max(np.abs(prepared.analytic_pcm - expected)))
    assert error <= 2e-12
    assert source.tobytes() == original
    field_before = prepared.analytic_pcm.tobytes()
    saved = float(source[3, 0])
    source[3, 0] += 1.0
    assert prepared.source.tobytes() == original
    assert prepared.analytic_pcm.tobytes() == field_before
    source[3, 0] = saved
    for value in (prepared.source, prepared.analytic_pcm):
        assert not value.flags.writeable
        try:
            value.setflags(write=True)
        except ValueError:
            pass
        else:
            message = "Prepared field backing can become writeable"
            raise AssertionError(message)
    return prepared, {
        "independent_scalar_convolution_max_abs_error": error,
        "source_unchanged": True,
        "prepared_arrays_survive_caller_mutation": True,
        "prepared_source_and_field_have_immutable_backing": True,
    }


def _phase_controls(prepared) -> list[dict]:
    records = []
    for rate, semitones in ((0.75, 7), (2.0, -7), (2.0, 0)):
        actual, report = model.render_prepared(prepared, rate, semitones)
        expected = _scalar_render(
            prepared.analytic_pcm,
            rate,
            2 ** (semitones / 12),
            report["capture_origin"],
            len(actual),
        )
        error = float(np.max(np.abs(actual - expected)))
        assert error <= _ABS_LIMIT
        records.append({
            "rate": rate,
            "semitones": semitones,
            "scalar_complete_phase_and_sampling_max_abs_error": error,
            "comparison_absolute_limit": _ABS_LIMIT,
        })
    return records


def _identity_controls(prepared) -> list[dict]:
    records = []
    for rate in (0.75, 1.0, 1.25, 2.0):
        actual, report = model.render_prepared(prepared, rate, 12 * math.log2(rate))
        expected = np.array([
            [
                _sample(prepared.source[:, channel], rate * (frame + report["capture_origin"])).real
                for channel in (0, 1)
            ]
            for frame in range(len(actual))
        ])
        error = float(np.max(np.abs(actual - expected)))
        assert error <= _ABS_LIMIT
        records.append({"rate_equals_shift": rate, "full_fractional_identity_max_abs_error": error})
    return records


def _capture_controls() -> list[dict]:
    records = []
    for frames, last in ((29, 28), (29, 5), (1500, 5)):
        source = np.zeros((frames, 2))
        source[last] = (0.4, 0.2)
        prepared = model.prepare(source)
        for rate in (0.75, 1.25, 2.0):
            origin = min(0, math.floor(-1 / rate) + 1)
            nominal = math.ceil(frames / rate)
            retired = max(nominal, math.floor((last + 1023) / rate) + 1)
            actual, report = model.render_prepared(prepared, rate, 7)
            assert report["capture_origin"] == origin
            assert report["nominal_output_frames"] == nominal
            assert report["retired_output_frame_exclusive"] == retired
            assert len(actual) == retired - origin
            records.append({
                "source_frames": frames,
                "last_active_frame": last,
                "rate": rate,
                "independent_origin_nominal_retired": [origin, nominal, retired],
            })
    return records


def _global_run_controls(source: np.ndarray) -> dict:
    gapped = np.zeros((2300, 2))
    gapped[3:46] = source
    gapped[1300:1343] = source
    prepared = model.prepare(gapped)
    active = np.any(prepared.analytic_pcm != 0.0, axis=1)
    starts = active & ~np.concatenate(([False], active[:-1]))
    assert np.count_nonzero(starts) == 2
    gap_records = _phase_controls(prepared)
    clock = np.arange(2300)
    continuous = 0.04 * np.sin(2 * np.pi * 0.073 * clock + 0.2)
    continuous[1300] += 1.0
    stereo = np.column_stack((continuous, 0.5 * continuous))
    cues = source_cues(stereo)
    assert 1300 in cues
    return {
        "genuine_exact_common_zero_runs": int(np.count_nonzero(starts)),
        "global_birth_and_gap_scalar_checks": gap_records,
        "unrelated_source_cues": cues,
        "continuous_field_across_cue_scalar_checks": _phase_controls(model.prepare(stereo)),
    }


def _invalid_controls(prepared) -> dict:
    invalid_sources = (
        np.zeros(3),
        np.zeros((3, 1)),
        np.zeros((3, 3)),
        np.full((3, 2), np.nan),
        np.full((3, 2), np.inf),
        np.ones((3, 2), dtype=np.complex128),
    )
    refused_sources = 0
    for source in invalid_sources:
        try:
            model.prepare(source)
        except ValueError:
            refused_sources += 1
        else:
            message = "Malformed source accepted"
            raise AssertionError(message)
    invalid_profiles = ((0.0, 0), (-1.0, 0), (math.inf, 0), (math.nan, 0), (1.0, math.inf))
    refused_profiles = 0
    for rate, semitones in invalid_profiles:
        try:
            model.render_prepared(prepared, rate, semitones)
        except ValueError:
            refused_profiles += 1
        else:
            message = "Malformed profile accepted"
            raise AssertionError(message)
    return {
        "malformed_sources_refused": refused_sources,
        "malformed_profiles_refused": refused_profiles,
    }


def _resource_controls(prepared) -> dict:
    backing = np.zeros((1, 2))
    huge = np.broadcast_to(backing, (2_000_000, 2))
    records = []
    for label, operation, reason in (
        ("actual_readonly_broadcast", lambda: model.prepare(huge), "memory_limit"),
        (
            "direct_convolution_work_limit",
            lambda: model.prepare(np.broadcast_to(backing, (model.MAX_SOURCE_FRAMES + 1, 2))),
            "direct_convolution_source_length_limit",
        ),
        ("tiny_rate_capture", lambda: model.render_prepared(prepared, 1e-12, 0), "memory_limit"),
        (
            "minimum_subnormal_rate_capture",
            lambda: model.render_prepared(prepared, float(np.nextafter(0.0, 1.0)), 0),
            "memory_limit",
        ),
        (
            "actual_phase_product_overflow",
            lambda: model.render_prepared(model.prepare(np.full((3, 2), 1e200)), 1.0, 7),
            "phase_or_output_numeric_range",
        ),
    ):
        try:
            operation()
        except ValueError as error:
            report = getattr(error, "report", {})
            assert report.get("refusal_reason") == reason
            assert report.get("phase_multiplier") is None or math.isfinite(
                report["phase_multiplier"]
            )
            records.append({"control": label, "refusal_report": report})
        else:
            message = f"Unbounded resource input accepted: {label}"
            raise AssertionError(message)
    return {"actual_broadcast_backing_bytes": backing.nbytes, "actual_resource_refusals": records}


def integrity_controls() -> dict:
    """Check the new field and phase action independently of acoustic acceptance."""
    source = 0.03 * np.random.default_rng(140217).normal(size=(43, 2))
    source[:3] = 0.0
    source[17:20] = 0.0
    source[-5:] = 0.0
    prepared, source_report = _source_controls(source)
    for frames in (0, 17):
        silent = model.prepare(np.zeros((frames, 2)))
        output, _ = model.render_prepared(silent, 2.0, -7)
        assert np.array_equal(output, np.zeros_like(output))
        assert output.shape == (math.ceil(frames / 2), 2)
    return {
        "stage": "joint_residual_phase_integrity_controls",
        "passed": True,
        "kernel": _kernel_controls(),
        "source": source_report,
        "scalar_phase_and_sampling": _phase_controls(prepared),
        "fractional_combined_identity": _identity_controls(prepared),
        "independent_complete_capture_bounds": _capture_controls(),
        "global_runs_and_unrelated_cues": _global_run_controls(source),
        "malformed_contracts": _invalid_controls(prepared),
        "resource_limits": _resource_controls(prepared),
        "empty_and_silent_inputs_exact": True,
        "acoustic_alias_or_realtime_acceptance_claim": False,
    }


if __name__ == "__main__":
    emit(integrity_controls())
