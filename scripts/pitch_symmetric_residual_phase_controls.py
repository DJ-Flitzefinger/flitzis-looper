"""Independent two-sided field and signed-support controls for the fixed mirror."""

import math

import numpy as np
import pitch_joint_residual_phase as causal
import pitch_joint_residual_phase_controls as reference
import pitch_symmetric_residual_phase as model
from pitch_causal_model_probe import emit


def _scalar_field(source: np.ndarray) -> np.ndarray:
    result = np.zeros((len(source) + 2046, 2), dtype=np.complex128)
    result.real[1023 : 1023 + len(source)] = source
    for lag in range(1, 1023, 2):
        window = 0.5 + 0.5 * math.cos(math.pi * lag / 1023)
        coefficient = 2 * window / 1024 / math.tan(math.pi * lag / 1024)
        for frame in range(len(source)):
            for channel in (0, 1):
                value = 1.0j * coefficient * float(source[frame, channel])
                result[1023 + frame + lag, channel] += value
                result[1023 + frame - lag, channel] -= value
    return result


def _kernel_controls() -> dict:
    source = np.array([[1.0, 0.5]])
    prepared = model.prepare(source)
    field = prepared.analytic_pcm
    positive = causal.prepare(source).analytic_pcm[:, 0].imag
    expected = np.concatenate((-positive[1:][::-1], positive))
    assert prepared.field_origin == -1023
    assert np.array_equal(field[:, 0].imag, expected)
    assert np.array_equal(field.imag, -field.imag[::-1])
    assert np.array_equal(field.imag[[0, -1]], np.zeros((2, 2)))
    assert np.array_equal(field.imag[1::2], np.zeros_like(field.imag[1::2]))
    assert np.array_equal(field[:, 1], 0.5 * field[:, 0])
    real = np.zeros((2047, 2))
    real[1023] = source[0]
    assert np.array_equal(field.real, real)
    return {
        "exact_antisymmetric_mirror_of_fixed_positive_taps": True,
        "even_center_and_final_quadrature_taps_exact_zero": True,
        "real_field_is_exact_original_impulse": True,
        "nonzero_quadrature_taps": int(np.count_nonzero(field[:, 0].imag)),
        "field_origin": prepared.field_origin,
        "declared_lookahead_source_frames": 1023,
        "original_513_band_sum_claim": False,
    }


def _source_controls(source: np.ndarray):
    original = source.tobytes()
    prepared = model.prepare(source)
    error = float(np.max(np.abs(prepared.analytic_pcm - _scalar_field(source))))
    assert error <= 2e-12
    assert source.tobytes() == original
    snapshot = prepared.analytic_pcm.tobytes()
    saved = float(source[3, 0])
    source[3, 0] += 1.0
    assert prepared.source.tobytes() == original
    assert prepared.analytic_pcm.tobytes() == snapshot
    source[3, 0] = saved
    for field in (prepared.source, prepared.analytic_pcm):
        try:
            field.setflags(write=True)
        except ValueError:
            pass
        else:
            message = "Prepared symmetric source or field can become writeable"
            raise AssertionError(message)
    return prepared, {
        "independent_two_sided_scalar_convolution_max_abs_error": error,
        "immutable_backing_and_caller_mutation_checked": True,
        "source_unchanged_by_prepare": True,
    }


def _phase_and_identity_controls(prepared) -> dict:
    records = []
    for rate, semitones in ((0.75, 7), (2.0, -7), (2.0, 0)):
        actual, report = model.render_prepared(prepared, rate, semitones)
        expected = reference._scalar_render(
            prepared.analytic_pcm,
            rate,
            2 ** (semitones / 12),
            report["capture_origin"],
            len(actual),
            prepared.field_origin,
        )
        error = float(np.max(np.abs(actual - expected)))
        assert error <= 2e-10
        records.append({"rate": rate, "semitones": semitones, "scalar_phase_sampling_error": error})
    identities = []
    for rate in (0.75, 1.0, 1.25, 2.0):
        actual, report = model.render_prepared(prepared, rate, 12 * math.log2(rate))
        expected = np.array([
            [
                reference._sample(
                    prepared.source[:, channel], rate * (frame + report["capture_origin"])
                ).real
                for channel in (0, 1)
            ]
            for frame in range(len(actual))
        ])
        error = float(np.max(np.abs(actual - expected)))
        assert error <= 2e-10
        identities.append({"rate_equals_shift": rate, "whole_signed_capture_identity_error": error})
    return {"scalar_phase_sampling": records, "fractional_combined_identity": identities}


def _capture_controls() -> list[dict]:
    records = []
    for frames, last in ((29, 28), (29, 5), (1500, 5)):
        source = np.zeros((frames, 2))
        source[last] = (0.4, 0.2)
        prepared = model.prepare(source)
        for rate in (0.75, 1.25, 2.0):
            origin = min(0, math.floor(-1024 / rate) + 1)
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


def _resource_controls(prepared) -> list[dict]:
    backing = np.zeros((1, 2))
    records = []
    for label, operation, reason in (
        (
            "actual_readonly_2M",
            lambda: model.prepare(np.broadcast_to(backing, (2_000_000, 2))),
            "memory_limit",
        ),
        (
            "actual_direct_convolution_work_limit",
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
            multiplier = report.get("phase_multiplier")
            assert multiplier is None or math.isfinite(multiplier)
            records.append({
                "control": label,
                "actual_backing_bytes": backing.nbytes,
                "report": report,
            })
        else:
            message = f"Unbounded symmetric resource input accepted: {label}"
            raise AssertionError(message)
    return records


def integrity_controls() -> dict:
    """Check the fixed two-sided field without claiming acoustic or live acceptance."""
    source = 0.03 * np.random.default_rng(140217).normal(size=(43, 2))
    source[:3] = 0.0
    source[17:20] = 0.0
    source[-5:] = 0.0
    prepared, source_report = _source_controls(source)
    for frames in (0, 17):
        output, _ = model.render_prepared(model.prepare(np.zeros((frames, 2))), 2.0, -7)
        assert np.array_equal(output, np.zeros_like(output))
        assert output.shape == (math.ceil(frames / 2), 2)
    return {
        "stage": "symmetric_residual_phase_integrity_controls",
        "passed": True,
        "kernel": _kernel_controls(),
        "source": source_report,
        "phase_and_identity": _phase_and_identity_controls(prepared),
        "independent_capture_bounds": _capture_controls(),
        "resource_refusals": _resource_controls(prepared),
        "empty_and_silent_exact": True,
        "finite_realtime_alias_or_product_acceptance": False,
    }


if __name__ == "__main__":
    emit(integrity_controls())
