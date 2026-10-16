"""Compare cached flank bounds with independent dense observation equations."""

from typing import TYPE_CHECKING

import numpy as np
from pitch_causal_model_probe import emit
from pitch_joint_finite_gates import JointGateProblem, enumerate_joint_flanks
from pitch_joint_gate_plateau import JointPlateauCertificate

if TYPE_CHECKING:
    from pitch_joint_finite_gates import Edges

_FRAMES, _START, _END = 2307, 1024, 1282
_ONSETS = ((0, 3), (0, 512), (254, 3), (255, 512), (511, 256), (1021, 3))
_RELEASES = ((1284, 3), (1793, 512), (2048, 256), (2306, 3), (2306, 512))
_EPS = np.finfo(np.float64).eps


def _problem(carriers: int, rng: np.random.Generator) -> JointGateProblem:
    centers = rng.normal(size=(_FRAMES, 2, carriers))
    radii = 0.03 + 0.01 * rng.random(size=centers.shape)
    source = rng.normal(size=(_FRAMES, 2))
    certificate = JointPlateauCertificate(_START, _END, np.empty(0), np.empty(0), {})
    return JointGateProblem(
        source,
        np.zeros((_FRAMES, 2, 2 * carriers)),
        certificate,
        centers - radii,
        centers + radii,
        1e9,
        float(np.linalg.norm(source)),
        {},
    )


def _dense_stats(problem: JointGateProblem, edges: Edges, *, release: bool) -> tuple[float, float]:
    positions = np.arange(_END, _FRAMES) if release else np.arange(_START)
    gates = np.column_stack([
        0.5
        * (
            1
            - np.cos(
                np.pi
                * np.clip((anchor - positions if release else positions - anchor) / width, 0, 1)
            )
        )
        for anchor, width in edges
    ])
    lower = (problem.lower[positions] * gates[:, None, :]).sum(axis=2)
    upper = (problem.upper[positions] * gates[:, None, :]).sum(axis=2)
    pcm = problem.source[positions]
    pad = 4096 * _EPS * (1 + abs(pcm) + abs(lower) + abs(upper))
    distance = np.maximum(np.maximum(lower - pad - pcm, pcm - upper - pad), 0)
    errors = (distance * distance).sum(axis=1)
    return float(errors.sum()), float(errors.max(initial=0))


def _check(problem: JointGateProblem, carriers: int, *, release: bool) -> dict:
    tables = tuple((_ONSETS, _RELEASES) for _ in range(carriers))
    results = enumerate_joint_flanks(problem, tables, release=release)
    expected = (len(_RELEASES) if release else len(_ONSETS)) ** carriers
    assert results is not None
    assert len(results) == expected
    for result in results:
        total, maximum = _dense_stats(problem, result.edges, release=release)
        assert result.total <= total * (1 + 8 * _EPS)
        assert result.largest >= maximum * (1 - 8 * _EPS)
        assert result.total - result.largest <= total - maximum + 1e-10
    return {"carriers": carriers, "release": release, "tuples": len(results)}


def flank_partition_controls() -> dict:
    """Check bound direction and complete tuples at nonbinding error budgets.

    These synthetic interval problems test partition arithmetic, rather than
    certify carrier models. The dense oracle does not use cached interval costs,
    merged ramps, block prefixes or fitter selection. It covers representative
    widths and block endpoints, not all float64 inputs or all 510 ramp widths.
    """
    rng = np.random.default_rng(3819701)
    records = []
    for carriers in (1, 2):
        problem = _problem(carriers, rng)
        records.extend(_check(problem, carriers, release=release) for release in (False, True))
    return {
        "stage": "joint_flank_dense_partition_controls",
        "passed": True,
        "independent_dense_full_flank_oracle": records,
        "all_requested_tuples_retained_at_nonbinding_budget": True,
        "block_boundary_and_extreme_widths": [3, 256, 512],
        "first_variable_release_frame_at_certificate_end": True,
        "numeric_bound_universality_claim": False,
    }


if __name__ == "__main__":
    emit(flank_partition_controls())
