"""Replay declared period-first selection without repeating Native discovery."""

import math
from typing import TYPE_CHECKING

from flitzis_looper.analysis.app_tempo_inferred import (
    BudgetOrdinalFit,
    CandidateSummary,
    EvaluatedOrdinalFit,
    PeriodHarmonicCheck,
    PeriodSelection,
    PeriodTrial,
    _assign,
    _band,
    _bounded_fit,
    _check_candidate,
    _check_same_candidate,
    _eligible,
    _equivalent,
    _invalid,
    _outcome,
    _require_phase_only,
    _section_lattice_compatible,
    _sustained,
    _validate_budget,
    _validate_checks,
    _validate_empty,
    _validate_result,
)

if TYPE_CHECKING:
    from flitzis_looper.analysis.app_tempo_inferred import OrdinalFit
    from flitzis_looper.analysis.app_tempo_probe import TempoMathView

_TOP_TRIAL_FIELDS = (
    "evidence_tier",
    "probe_windows",
    "fractional_check",
    "integer_check",
    "integer_candidate_bpm",
    "integer_preferred",
    "conflicting_sections",
)


def unit_counterpart(a: CandidateSummary, b: CandidateSummary, duration: float) -> bool:
    """One direct factor-two relationship with one winding over the entire source."""
    fine, coarse = (a, b) if a.period_seconds <= b.period_seconds else (b, a)
    pf, pc = fine.period_seconds, coarse.period_seconds
    drift = abs(pc - 2.0 * pf) * (duration / pc)
    return (
        math.isfinite(duration)
        and duration > 0
        and math.isfinite(drift)
        and drift <= 0.005
        and round(pc / pf) == 2
        and _section_lattice_compatible(a, b, [0.0, duration])
    )


def _score(candidate: CandidateSummary) -> float:
    span = candidate.global_span
    denominator = span.raw_count + span.expected_slots
    return 2.0 * span.matched / denominator if denominator else 0.0


def _local_values(
    pool: list[CandidateSummary], incompatible: list[int], winner: CandidateSummary, section: int
) -> dict[int, float]:
    return {
        index: _band(pool[index].sections[section])[0]
        for index in incompatible
        if _sustained(pool[index].sections[section])
        and not _section_lattice_compatible(
            pool[index], winner, winner.sections[section].window_seconds
        )
    }


def _density_counterpart(candidate: CandidateSummary, winner: CandidateSummary) -> bool:
    fine, coarse = sorted((candidate.period_seconds, winner.period_seconds))
    return round(coarse / fine) >= 2 and any(
        _sustained(a)
        and _sustained(b)
        and _section_lattice_compatible(candidate, winner, b.window_seconds)
        for a, b in zip(candidate.sections, winner.sections, strict=True)
    )


def _critical_values(
    pool: list[CandidateSummary], candidate_index: int, duration: float
) -> list[dict[int, float]]:
    winner = pool[candidate_index]
    different = [
        index
        for index, candidate in enumerate(pool)
        if index != candidate_index and not _equivalent(candidate, winner, duration)
    ]
    units = {index for index in different if unit_counterpart(pool[index], winner, duration)}
    incompatible = [index for index in different if index not in units]
    whole = {
        index: _band(pool[index].global_span)[1]
        for index in incompatible
        if _sustained(pool[index].global_span)
    }
    local = [_local_values(pool, incompatible, winner, section) for section in range(5)]
    phase = {
        index: _score(pool[index])
        for index in different
        if abs(pool[index].period_seconds / winner.period_seconds - 1.0) <= 0.0001
    }
    density = {
        index: _score(pool[index])
        for index in different
        if _sustained(pool[index].global_span) and _density_counterpart(pool[index], winner)
    }
    unit = {
        index: _band(pool[index].global_span)[1]
        for index in units
        if _sustained(pool[index].global_span)
    }
    return [whole, *local, phase, density, unit]


def _validate_references(trial: PeriodTrial, pool_size: int) -> None:
    references = trial.critical_indices
    if (
        trial.candidate_index >= pool_size
        or len(set(references)) != len(references)
        or any(index >= pool_size or index == trial.candidate_index for index in references)
    ):
        _invalid("Period trial contains invalid or repeated candidate references")


def _validate_critical(trial: PeriodTrial, groups: list[dict[int, float]]) -> None:
    for values in groups:
        if values and not any(
            index in values and values[index] == max(values.values())
            for index in trial.critical_indices
        ):
            _invalid("Period trial omitted a declared extremal critical witness")


def _validate_trial(
    method: TempoMathView,
    fit: EvaluatedOrdinalFit,
    pool: list[CandidateSummary],
    trial: PeriodTrial,
) -> float:
    _validate_references(trial, len(pool))
    winner = pool[trial.candidate_index]
    duration = method.frame_count / method.sample_rate_hz
    points = _assign(method.raw_beats, winner.period_seconds, winner.anchor_seconds, duration)
    if trial.candidate_index == 0 and fit.assignments != [[p[0], p[1]] for p in points]:
        _invalid("Selected period assignments differ from the original observations")
    view = fit.model_copy(
        update={
            field: getattr(trial, field)
            for field in ("outcome", "candidate_limit_reached", *_TOP_TRIAL_FIELDS)
        }
    )
    passed, bpm = _validate_checks(method, view, winner, points)
    if (
        trial.candidate_limit_reached
        and not fit.candidate_limit_reached
        and not (passed and _eligible(winner, duration))
    ):
        _invalid("Alias coverage cannot precede its own supported precision and eligibility")
    groups = _critical_values(pool, trial.candidate_index, duration)
    _validate_critical(trial, groups)
    conflicts = [
        section
        for section, values in enumerate(groups[1:6])
        if values and max(values.values()) > _band(winner.sections[section])[1]
    ]
    if trial.conflicting_sections != conflicts:
        _invalid("Period trial section contradictions differ from its declared candidates")
    ambiguous = bool(groups[0]) and max(groups[0].values()) >= _band(winner.global_span)[0]
    expected = _outcome(view, passed=passed and _eligible(winner, duration), ambiguous=ambiguous)
    if trial.outcome != expected:
        _invalid("Period trial outcome differs from its own precision and critical evidence")
    return bpm


def _validate_pool(method: TempoMathView, fit: EvaluatedOrdinalFit) -> list[CandidateSummary]:
    if fit.winner is None:
        _invalid("Period selection requires its retained candidate")
    pool = [fit.winner, *fit.alternatives]
    if fit.evaluated_candidate_count < len(pool):
        _invalid("Period discovery count is smaller than its distinct retained summaries")
    duration = method.frame_count / method.sample_rate_hz
    for index, candidate in enumerate(pool):
        if candidate in pool[:index]:
            _invalid("Period summaries must be distinct and cannot duplicate the selected winner")
        # Drop each regenerated mapping before checking the next summary.
        _check_candidate(method.raw_beats, candidate, duration)
    return pool


def _harmonic_candidate(
    method: TempoMathView,
    canonical: CandidateSummary,
    harmonic: PeriodHarmonicCheck,
    pool: list[CandidateSummary],
) -> int | None:
    period = canonical.period_seconds * harmonic.factor
    if (
        harmonic.seed_period_seconds != period
        or harmonic.seed_anchor_seconds != canonical.anchor_seconds
    ):
        _invalid("Period harmonic seed differs from its original canonical reference")
    index = harmonic.candidate_index
    if not 0.2 <= period <= 2.0:
        if harmonic.outcome != "out_of_range" or index is not None:
            _invalid("An out-of-range canonical start cannot carry a fitted candidate")
        return None
    expected = _bounded_fit(
        method.raw_beats,
        period,
        canonical.anchor_seconds,
        method.frame_count / method.sample_rate_hz,
    )
    if expected is None:
        if harmonic.outcome != "no_fixed_fit" or index is not None:
            _invalid("Canonical harmonic refusal differs from its bounded actual start")
        return None
    if harmonic.outcome != "fitted" or index is None or index >= len(pool):
        _invalid("A fitted canonical harmonic requires its retained scalar reference")
    _check_same_candidate(pool[index], expected)
    return index


def _validate_harmonics(
    method: TempoMathView,
    fit: EvaluatedOrdinalFit,
    selection: PeriodSelection,
    pool: list[CandidateSummary],
) -> list[int]:
    if selection.canonical_index >= len(pool) or [h.factor for h in selection.harmonics] != [
        0.5,
        2.0,
    ]:
        _invalid("Period selection must retain its canonical reference and both actual starts")
    canonical = pool[selection.canonical_index]
    attempts, queue = 0, []
    for harmonic in selection.harmonics:
        index = _harmonic_candidate(method, canonical, harmonic, pool)
        attempts += 0.2 <= canonical.period_seconds * harmonic.factor <= 2.0
        if index is not None and index != selection.canonical_index and index not in queue:
            queue.append(index)
    if fit.evaluated_candidate_count < attempts + 1:
        _invalid("Period count omits its mandatory canonical harmonic starts")
    duration = method.frame_count / method.sample_rate_hz
    # Stable sort retains declaration order for exact eligibility/F1 ties.
    queue.sort(
        key=lambda index: (_eligible(pool[index], duration), _score(pool[index])), reverse=True
    )
    return [selection.canonical_index, *queue]


def _validate_sequence(
    method: TempoMathView,
    fit: EvaluatedOrdinalFit,
    pool: list[CandidateSummary],
    selection: PeriodSelection,
    order: list[int],
) -> float | None:
    selected_bpm = None
    selected_index = None
    if len(selection.trials) > len(order):
        _invalid("Period selection contains an undeclared promotion member")
    for position, trial in enumerate(selection.trials):
        if trial.candidate_index != order[position]:
            _invalid("Period trials must follow canonical-first deterministic support order")
        if (position == 0 and trial.candidate_limit_reached != fit.candidate_limit_reached) or (
            fit.candidate_limit_reached and not trial.candidate_limit_reached
        ):
            _invalid("Period trial search flags contradict the original canonical discovery")
        bpm = _validate_trial(method, fit, pool, trial)
        if (
            trial.outcome in {"supported", "search_limited"}
            and position != len(selection.trials) - 1
        ):
            _invalid("Period selection must stop at its first supported or limited trial")
        if trial.outcome == "supported":
            selected_bpm, selected_index = bpm, position
    final = selection.trials[-1]
    if final.outcome not in {"supported", "search_limited"} and len(selection.trials) != len(order):
        _invalid("Period selection omitted a higher-priority performed harmonic trial")
    if selection.selected_trial_index != selected_index:
        _invalid("Period selected trial differs from its first admissible candidate")
    return selected_bpm


def _validate_top(fit: EvaluatedOrdinalFit, selection: PeriodSelection) -> None:
    chosen = selection.selected_trial_index
    trial = selection.trials[chosen if chosen is not None else 0]
    if trial.candidate_index != 0 or (chosen is None and selection.canonical_index != 0):
        _invalid("Top period evidence must retain its selected or unavailable canonical candidate")
    if any(getattr(fit, field) != getattr(trial, field) for field in _TOP_TRIAL_FIELDS):
        _invalid("Top period precision and coverage differ from their performed trial")
    expected = (
        "supported"
        if chosen is not None
        else (
            "search_limited"
            if any(t.candidate_limit_reached for t in selection.trials)
            else selection.trials[0].outcome
        )
    )
    if fit.outcome != expected:
        _invalid("Overall period outcome contradicts its finite performed selection")


def _validate_phase_only(method: TempoMathView, fit: EvaluatedOrdinalFit) -> None:
    if fit.winner is None or (
        method.frame_count / method.sample_rate_hz < 30.0
        or fit.outcome != "insufficient"
        or fit.evidence_tier != "two_ten_second"
        or not _eligible(fit.winner, method.frame_count / method.sample_rate_hz)
    ):
        _invalid("Acoustic corroboration requires normal long-source period support")
    _require_phase_only(fit.fractional_check, 60.0 / fit.winner.period_seconds)


def validate_period_fit(method: TempoMathView, *, phase_only: bool = False) -> None:
    """Validate declared finite choices; never certify complete search or coverage audit."""
    if method.selected is not None or method.integer_probe is not None:
        _invalid("Period selection cannot impersonate original-index regional evidence")
    fit = method.ordinal_fit
    if isinstance(fit, BudgetOrdinalFit):
        if phase_only:
            _invalid("Acoustic corroboration cannot bypass unavailable period preparation")
        _validate_budget(method, fit)
        return
    if not isinstance(fit, EvaluatedOrdinalFit) or fit.harmonics:
        _invalid("Version nine requires its explicit period evidence and canonical declarations")
    if fit.winner is None:
        if phase_only or fit.period_selection is not None:
            _invalid("An absent period winner cannot carry performed selection evidence")
        _validate_empty(fit)
        _validate_result(method, None)
        return
    selection = fit.period_selection
    if selection is None:
        _invalid("A retained version-nine winner requires its bounded period selection")
    pool = _validate_pool(method, fit)
    order = _validate_harmonics(method, fit, selection, pool)
    bpm = _validate_sequence(method, fit, pool, selection, order)
    _validate_top(fit, selection)
    if phase_only:
        _validate_phase_only(method, fit)
    _validate_result(method, bpm)


def has_unit_counterpart(fit: OrdinalFit | None, duration: float) -> bool:
    """Inspect only bounded validated summaries; no raw-event scan during rendering."""
    if (
        not isinstance(fit, EvaluatedOrdinalFit)
        or fit.winner is None
        or fit.period_selection is None
    ):
        return False
    return fit.outcome == "supported" and any(
        _sustained(candidate.global_span)
        and unit_counterpart(candidate, fit.winner, duration)
        and _band(candidate.global_span)[1] >= _band(fit.winner.global_span)[0]
        for candidate in fit.alternatives
    )
