use super::*;
use crate::audio_engine::app_tempo::{estimate, estimate_v4, estimate_with_budget};

#[derive(Default, Serialize)]
struct SearchObserver {
    stage: (&'static str, Option<usize>),
    attempts: Vec<serde_json::Value>,
    memo_hits: Vec<serde_json::Value>,
    memo_hit_count: usize,
    capacity_refusals: Vec<serde_json::Value>,
    capacity_refusal_count: usize,
    harmonics: Vec<serde_json::Value>,
    snapshot: Option<serde_json::Value>,
    replay_passes: Vec<serde_json::Value>,
}

std::thread_local! {
    static SEARCH_OBSERVER: std::cell::RefCell<Option<SearchObserver>> = const { std::cell::RefCell::new(None) };
}

struct ObserverScope;

impl ObserverScope {
    fn begin() -> Self {
        SEARCH_OBSERVER.with(|state| {
            let mut state = state.borrow_mut();
            assert!(state.is_none(), "search observer must not be nested");
            *state = Some(SearchObserver::default());
        });
        Self
    }

    fn finish(self) -> SearchObserver {
        SEARCH_OBSERVER.with(|state| state.borrow_mut().take().expect("active search observer"))
    }
}

impl Drop for ObserverScope {
    fn drop(&mut self) {
        SEARCH_OBSERVER.with(|state| {
            state.borrow_mut().take();
        });
    }
}

fn with_observer(callback: impl FnOnce(&mut SearchObserver)) {
    SEARCH_OBSERVER.with(|state| {
        if let Some(observer) = state.borrow_mut().as_mut() {
            callback(observer);
        }
    });
}

fn observed_fit(fitted: &FitResult<CandidateSummary>) -> serde_json::Value {
    match fitted {
        Err(SearchFailure::Allocation) => serde_json::json!({"outcome":"allocation_unavailable"}),
        Err(SearchFailure::Cancelled) => serde_json::json!({"outcome":"cancelled"}),
        Ok(None) => serde_json::json!({"outcome":"no_fixed_fit"}),
        Ok(Some(candidate)) => serde_json::json!({
            "outcome":"fitted", "line":candidate,
            "period_bits":candidate.period_seconds.to_bits(), "anchor_bits":candidate.anchor_seconds.to_bits(),
            "score":candidate.global.score(), "global_band":candidate.global.band(),
        }),
    }
}

pub(super) fn observe_search_stage(stage: &'static str, index: Option<usize>) {
    with_observer(|observer| observer.stage = (stage, index));
}

pub(super) fn observe_replay(status: Visit, before: usize, after: usize, admitted: usize) {
    with_observer(|observer| {
        assert!(observer.replay_passes.len() <= SEARCH_FITS);
        let status = match status {
            Visit::Continue => "completed",
            Visit::Restart => "restart",
            Visit::Limited => "limited",
        };
        observer.replay_passes.push(serde_json::json!({"status":status,"fits_before":before,"fits_after":after,"admitted":admitted}));
    });
}

pub(super) fn observe_search_memo(period: f64, anchor: f64) {
    with_observer(|observer| {
        observer.memo_hit_count += 1;
        if observer.memo_hits.len() == 512 {
            return;
        }
        observer
            .memo_hits
            .push(serde_json::json!({"stage":observer.stage,
            "period_bits":period.to_bits(),"anchor_bits":anchor.to_bits()}));
    });
}

pub(super) fn observe_search_capacity(period: f64, anchor: f64) {
    with_observer(|observer| {
        observer.capacity_refusal_count += 1;
        if observer.capacity_refusals.len() == 512 {
            return;
        }
        observer
            .capacity_refusals
            .push(serde_json::json!({"stage":observer.stage,
            "period_bits":period.to_bits(),"anchor_bits":anchor.to_bits()}));
    });
}

pub(super) fn observe_search_fit(
    period: f64,
    anchor: f64,
    ordinal: usize,
    pool_index: usize,
    fitted: &FitResult<CandidateSummary>,
) {
    with_observer(|observer| {
        assert!(observer.attempts.len() < SEARCH_FITS && ordinal <= MAX_FITS);
        observer.attempts.push(serde_json::json!({"stage":observer.stage,"fit_ordinal":ordinal,
            "period_seconds":period,"anchor_seconds":anchor,"period_bits":period.to_bits(),"anchor_bits":anchor.to_bits(),
            "pool_index":if matches!(fitted, Ok(Some(_))) { Some(pool_index) } else { None },"result":observed_fit(fitted)}));
    });
}

pub(super) fn observe_harmonic_fit(
    side: usize,
    period: f64,
    anchor: f64,
    ordinal: Option<usize>,
    fitted: Option<&FitResult<CandidateSummary>>,
) {
    with_observer(|observer| {
        assert!(observer.harmonics.len() < 2);
        observer.harmonics.push(serde_json::json!({"stage":"final_harmonic","side":side,"fit_ordinal":ordinal,
            "period_seconds":period,"anchor_seconds":anchor,"period_bits":period.to_bits(),"anchor_bits":anchor.to_bits(),
            "result":fitted.map_or_else(|| serde_json::json!({"outcome":"out_of_range"}), observed_fit)}));
    });
}

pub(super) fn observe_search_snapshot(
    discovery: &Discovery,
    attempts: &[(u64, u64)],
    pool: &[CandidateSummary],
    winner_index: Option<usize>,
) {
    with_observer(|observer| {
        assert!(discovery.seeds.len() <= MAX_COARSE_SEEDS && pool.len() <= SEARCH_FITS);
        let winner = winner_index.map(|index| &pool[index]);
        let mut unresolved = Vec::new();
        let mut attempted_seed_count = 0;
        for (index, seed) in discovery.seeds.iter().enumerate() {
            let attempted = was_attempted(attempts, seed.period, seed.anchor);
            attempted_seed_count += usize::from(attempted);
            if attempted || coarse_margin(seed, winner).is_none() {
                continue;
            }
            let global = seed.sections >= 3
                && seed.upper >= winner.map_or(0.0, |winner| winner.global.band().0);
            let local: Vec<_> = (0..SECTIONS)
                .filter(|&section| {
                    seed.local_sustained[section]
                        && seed.local_upper[section]
                            > winner.map_or(0.0, |winner| winner.sections[section].band().1)
                })
                .collect();
            unresolved.push(serde_json::json!({"seed_id":index,"family":seed.family,"refined_from":seed.refined_from,
                "source_pair":seed.source_pair,"source_ordinal_gap":seed.source_count,
                "period_seconds":seed.period,"anchor_seconds":seed.anchor,"period_bits":seed.period.to_bits(),"anchor_bits":seed.anchor.to_bits(),
                "attempted":attempted,"sample_matched":seed.matched,"sample_sections":seed.sections,
                "global_upper":seed.upper,"local_upper":seed.local_upper,"local_sustained":seed.local_sustained,
                "coarse_margin":coarse_margin(seed,winner),"global_veto":global,"section_vetoes":local}));
        }
        for attempt in &mut observer.attempts {
            let period = attempt["period_bits"].as_u64().unwrap();
            let anchor = attempt["anchor_bits"].as_u64().unwrap();
            let ids: Vec<_> = discovery
                .seeds
                .iter()
                .enumerate()
                .filter(|(_, seed)| {
                    seed.period.to_bits() == period && seed.anchor.to_bits() == anchor
                })
                .map(|(index, _)| index)
                .collect();
            attempt["seed_ids"] = serde_json::json!(ids);
        }
        observer.snapshot = Some(
            serde_json::json!({"seed_count":discovery.seeds.len(),"family_count":discovery.family_count,
            "discovery_limited":discovery.limited,"attempted_seed_count":attempted_seed_count,
            "winner_index":winner_index,"winner":winner,"unresolved":unresolved}),
        );
    });
}

fn interrupted(bpm: f64, extra: bool) -> Vec<f64> {
    let period = 60.0 / bpm;
    let mut events = Vec::new();
    for beat in 0..300 {
        let t = 0.07 + beat as f64 * period;
        if beat % 17 != 5 {
            events.push((t / 0.02).round() * 0.02);
        }
        if extra && beat % 4 == 1 {
            events.push(((t + 0.137) / 0.02).round() * 0.02);
        }
    }
    events.sort_by(f64::total_cmp);
    events.dedup();
    events
}

#[test]
fn dense_fast_pulses_with_holes_beat_their_three_interleaved_slow_aliases() {
    let bpm = 173.125;
    let period = 60.0 / bpm;
    let events: Vec<_> = (0..900)
        .filter(|i| i % 29 != 7 && !(401..451).contains(i))
        .map(|i| ((0.13 + i as f64 * period) / 0.02).round() * 0.02)
        .collect();
    let duration = 901.0 * period;
    assert!(estimate_v4(&events, duration).bpm.is_none());
    let result = estimate(&events, duration);
    assert!(result.bpm.is_some(), "{result:?}");
    assert!((result.bpm.unwrap() - bpm).abs() < 0.04, "{result:?}");
    let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
        panic!("inferred evidence missing");
    };
    assert_eq!(proof.outcome, "supported");
    assert!(proof.evaluated_candidate_count <= MAX_FITS);
}

#[test]
fn count_ranges_preserve_admissible_integer_counts_and_clipped_octaves() {
    let period = 60.0 / 173.125;
    let counts = admissible_counts(121.0 * period + 0.01, period * 0.995, period * 1.005).unwrap();
    assert!(counts.contains(&121));
    assert!(octave_bands(0.795, 0.805).any(|(lo, hi)| lo == MIN_PERIOD && hi > lo));
    assert!(octave_bands(f64::NAN, 0.8).next().is_none());
    assert!(admissible_counts(f64::INFINITY, 0.3, 0.4).is_none());
    assert!(admissible_counts(1.0, 0.5, 0.4).is_none());
    assert!(admissible_counts(EXACT_INTEGER * 4.0, 0.2, 0.3).is_none());
}

#[test]
fn exact_start_universe_deduplicates_before_truthful_saturation() {
    let mut discovery = Discovery {
        seeds: reserve(MAX_COARSE_SEEDS).unwrap(),
        representatives: reserve(MAX_COARSE_FAMILIES).unwrap(),
        limited: false,
        family_count: 0,
    };
    let base = screen(
        &[0.1, 0.6, 1.1, 1.6],
        &[0, 1, 2, 3],
        2.0,
        0.5,
        0.1,
        &[1, 1, 0, 1, 1],
    )
    .unwrap();
    for i in 0..MAX_COARSE_SEEDS {
        let mut seed = base;
        seed.anchor = i as f64 / 10000.0;
        assert_eq!(retain_seed(&mut discovery, seed), Some(i));
    }
    let mut duplicate = base;
    duplicate.anchor = 0.0;
    assert_eq!(retain_seed(&mut discovery, duplicate), Some(0));
    assert!(!discovery.limited);
    duplicate.anchor = -0.1;
    assert!(retain_seed(&mut discovery, duplicate).is_none());
    assert!(discovery.limited);
    assert_eq!(discovery.seeds.len(), MAX_COARSE_SEEDS);
}

#[test]
fn enormous_admissible_count_ranges_stop_after_first_distinct_admission_refusal() {
    let events: Vec<_> = (0..64)
        .flat_map(|cluster| {
            [0.0, 0.5, 1.001, 1.501]
                .into_iter()
                .map(move |offset| cluster as f64 * 10_000_000_000.0 + 0.1 + offset)
        })
        .collect();
    let duration = 640_000_000_000.0;
    let sample = sample_indices(&events, duration).unwrap();
    let calls = std::cell::Cell::new(0);
    let probe = || {
        calls.set(calls.get() + 1);
        calls.get() > 200_000
    };
    let control = Control::new(&probe);
    let result = discover_control(&events, duration, &sample, &control)
        .expect("bounded admission must finish before watchdog cancellation");
    assert!(result.limited);
    assert!(result.seeds.len() <= MAX_COARSE_SEEDS);
}

#[test]
fn census_keeps_first_exact_provenance_and_original_emissions_spend_fuel() {
    let events = [0.1, 0.6, 1.1];
    let sample = [0, 1, 2];
    let full_counts = [1, 1, 1, 0, 0];
    let mut census = Census::new().unwrap();
    let first = screen(&events, &sample, 2.0, 0.5, 0.1, &full_counts).unwrap();
    assert!(census.accept(first).unwrap().is_some());
    let mut duplicate = first;
    duplicate.refined_from = Some((0.4_f64.to_bits(), 0.2_f64.to_bits()));
    assert!(census.accept(duplicate).unwrap().is_none());
    assert!(census.representatives[0].refined_from.is_none());
    let mut fuel = 2;
    let mut visits = 0;
    let control = Control::new(&never_cancel);
    for expected in [Visit::Continue, Visit::Continue, Visit::Limited] {
        let status = visit_observed_start(
            &events,
            &sample,
            2.0,
            0.5,
            0.1,
            [0, 2],
            2,
            &full_counts,
            &control,
            &mut fuel,
            &mut |seed| {
                assert!(seed.refined_from.is_none());
                visits += 1;
                Ok(Visit::Continue)
            },
        )
        .unwrap();
        assert_eq!(status, expected);
    }
    assert_eq!((fuel, visits), (0, 2));
    assert!(admissible_counts(0.001, 0.5, 0.5).unwrap().is_empty());
}

#[test]
fn long_missing_pulse_geometry_finishes_without_admitting_every_transient() {
    let events: Vec<_> = (0..1200)
        .filter(|i| i % 37 != 7 && !(32..72).contains(&(i % 300)))
        .map(|i| (6 + 25 * i) as f64 * 0.02)
        .collect();
    assert!(estimate_v4(&events, 601.0).bpm.is_none());
    let result = estimate(&events, 601.0);
    assert_eq!(result.bpm, Some(120.0), "{result:?}");
    let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
        panic!("inferred evidence missing");
    };
    assert!(!proof.candidate_limit_reached);
}

#[test]
fn replay_reconsiders_an_earlier_local_obligation_after_winner_bands_weaken() {
    let mut events: Vec<_> = (0..67).map(|i| 0.1 + i as f64 * 0.6).collect();
    events.extend((0..400).map(|i| 40.1 + i as f64 * 0.4));
    let duration = 200.0;
    let sample = sample_indices(&events, duration).unwrap();
    let control = Control::new(&never_cancel);
    let geometry = build_geometry(&events, duration, &sample, &control).unwrap();
    let old = fit(&events, duration, 0.6, 0.1).unwrap().unwrap();
    let candidate = screen(&events, &sample, duration, 0.4, 0.1, &geometry.full_counts).unwrap();
    let edge = screen(&events, &sample, duration, 1.8, 0.1, &geometry.full_counts).unwrap();
    assert!(coarse_margin(&edge, Some(&old)).is_none());
    let mut discovery = Discovery {
        seeds: reserve(MAX_COARSE_SEEDS).unwrap(),
        representatives: Vec::new(),
        family_count: 0,
        limited: false,
    };
    let mut evidence = empty_evidence();
    let mut pool = reserve(MAX_FITS).unwrap();
    pool.push(old.clone());
    let mut attempts = reserve(SEARCH_FITS).unwrap();
    assert_eq!(
        replay_seed(
            edge,
            Some(&old),
            &events,
            duration,
            &mut discovery,
            &mut evidence,
            &mut pool,
            &mut attempts,
            &control
        )
        .unwrap(),
        Visit::Continue
    );
    assert!(discovery.seeds.is_empty());
    assert_eq!(
        replay_seed(
            candidate,
            Some(&old),
            &events,
            duration,
            &mut discovery,
            &mut evidence,
            &mut pool,
            &mut attempts,
            &control
        )
        .unwrap(),
        Visit::Restart
    );
    let changed = pool[winner_index(&pool, duration).unwrap()].clone();
    assert!(changed.sections[0].band().1 < old.sections[0].band().1);
    assert!(coarse_margin(&edge, Some(&changed)).is_some());
    assert_eq!(
        replay_seed(
            edge,
            Some(&changed),
            &events,
            duration,
            &mut discovery,
            &mut evidence,
            &mut pool,
            &mut attempts,
            &control
        )
        .unwrap(),
        Visit::Continue
    );
    assert!(
        discovery
            .seeds
            .iter()
            .any(|seed| seed.period.to_bits() == edge.period.to_bits()
                && seed.anchor.to_bits() == edge.anchor.to_bits())
    );
    let veto = strongest(&pool, &changed, duration, Some(0))
        .expect("earlier coherent edge remains visible");
    assert!(pool[veto].sections[0].band().0 > changed.sections[0].band().1);
}

#[test]
fn distinct_admitted_attempts_can_exceed_the_retired_144_cutoff() {
    let mut evidence = empty_evidence();
    let mut pool = reserve(MAX_FITS).unwrap();
    let mut attempts = reserve(SEARCH_FITS).unwrap();
    let control = Control::new(&never_cancel);
    for i in 0..170 {
        assert!(
            search_attempt(
                &[0.1, 0.6],
                2.0,
                0.5,
                i as f64 / 10000.0,
                &mut evidence,
                &mut pool,
                &mut attempts,
                &control
            )
            .unwrap()
        );
    }
    assert_eq!(evidence.evaluated_candidate_count, 170);
    assert_eq!(SEARCH_FITS, MAX_COARSE_SEEDS);
    assert_eq!(MAX_FITS, MAX_COARSE_SEEDS + 2);
}

#[test]
fn cancellation_is_latched_and_distinct_from_math_and_allocation_refusal() {
    let calls = std::cell::Cell::new(0);
    let probe = || {
        let old = calls.get();
        calls.set(old + 1);
        old == 0
    };
    let control = Control::new(&probe);
    assert!(control.cancelled() && control.cancelled());
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        discover_control(&[0.1, 0.6, 1.1, 1.6], 2.0, &[0, 1, 2, 3], &control),
        Err(SearchFailure::Cancelled)
    ));
    assert!(matches!(
        allocate::<u64>(usize::MAX),
        Err(SearchFailure::Allocation)
    ));
    assert!(matches!(recover(&[0.1, 0.6], 2.0, 10, 0, &control), Err(_)));
    let never = Control::new(&never_cancel);
    assert!(matches!(
        fit_control(&[0.1, 0.6], 2.0, 0.5, 0.1, &never),
        Ok(None)
    ));
    let (_, refusal) = recover(&[0.1, 0.6], 2.0, 10, 0, &never).unwrap();
    assert!(matches!(
        refusal,
        OrdinalFit::Budget(BudgetUnavailable {
            outcome: "budget_unavailable",
            ..
        })
    ));
    let calls = std::cell::Cell::new(0);
    let later = || {
        calls.set(calls.get() + 1);
        calls.get() == 4
    };
    let control = Control::new(&later);
    assert!(matches!(
        fit_control(&[0.1, 0.6, 1.1, 1.6], 2.0, 0.5, 0.1, &control),
        Err(SearchFailure::Cancelled)
    ));
    assert!(control.cancelled());
}

#[test]
fn cancelled_priority_dispatch_never_returns_unavailable_evidence() {
    let scope = ObserverScope::begin();
    let probe = || {
        SEARCH_OBSERVER.with(|state| {
            state
                .borrow()
                .as_ref()
                .is_some_and(|observer| observer.stage.0 == "exact_octave")
        })
    };
    let events = interrupted(97.125, false);
    let result = crate::audio_engine::app_tempo::estimate_with_budget_cancelled(
        &events,
        187.0,
        0,
        usize::MAX,
        &probe,
    );
    assert!(result.is_err());
    drop(scope);
    SEARCH_OBSERVER.with(|state| assert!(state.borrow().is_none()));
}

#[test]
fn fixed_search_layout_and_point_lifetimes_fit_the_unchanged_reserve() {
    assert!(constant_storage_bytes().unwrap() <= FIXED_RESERVE);
    assert!(4 * std::mem::size_of::<Point>() <= 96);
    println!(
        "PERIOD_STORAGE fixed={} reserve={} trial={} selection={} harmonic={} summary={} span={} point={}",
        constant_storage_bytes().unwrap(),
        FIXED_RESERVE,
        std::mem::size_of::<PeriodTrial>(),
        std::mem::size_of::<PeriodSelection>(),
        std::mem::size_of::<PeriodHarmonic>(),
        std::mem::size_of::<CandidateSummary>(),
        std::mem::size_of::<SpanDiagnostics>(),
        std::mem::size_of::<Point>()
    );
    if usize::BITS == 64 {
        assert_eq!(std::mem::size_of::<Seed>(), 144);
        assert_eq!(std::mem::size_of::<SpanDiagnostics>(), 112);
        assert_eq!(std::mem::size_of::<CandidateSummary>(), 168);
        assert_eq!(std::mem::size_of::<ObservedInterval>(), 24);
        assert_eq!(std::mem::size_of::<IntervalFamily>(), 32);
    }
}

#[test]
fn local_lattices_are_symmetric_and_keep_shifted_or_drifting_rivals() {
    let fine = bounded_summary(0.5, 200.0);
    let mut coarse = bounded_summary(1.0, 200.0);
    coarse.anchor_seconds = 50.0;
    assert_eq!(section_stride(&fine, &coarse, [0.0, 200.0]), Some(2));
    assert_eq!(section_stride(&coarse, &fine, [0.0, 200.0]), Some(2));
    coarse.anchor_seconds += 0.25;
    assert!(section_stride(&fine, &coarse, [0.0, 200.0]).is_none());
    coarse.period_seconds = 0.75;
    coarse.anchor_seconds = 0.0;
    assert!(section_stride(&fine, &coarse, [0.0, 200.0]).is_none());
    let a = bounded_summary(1.0, 100_000_000.0);
    let b = bounded_summary(1.000_000_05, 100_000_000.0);
    assert!(section_stride(&a, &b, [0.0, 100_000_000.0]).is_none());
    coarse.anchor_seconds = f64::INFINITY;
    assert!(section_stride(&fine, &coarse, [0.0, 200.0]).is_none());
}

#[test]
fn compatible_stronger_density_cannot_hide_an_incompatible_local_veto() {
    let mut winner = bounded_summary(0.5, 200.0);
    winner.sections[0].matched = 40;
    let mut compatible = bounded_summary(1.0, 200.0);
    compatible.sections[0].matched = 100;
    let incompatible = bounded_summary(0.8, 200.0);
    assert!(compatible.sections[0].band().0 > incompatible.sections[0].band().0);
    assert_eq!(
        strongest(&[compatible, incompatible], &winner, 200.0, Some(0)),
        Some(1)
    );
}

#[test]
fn unchanged_v4_success_bypasses_new_budget_and_serializes_identically() {
    let events: Vec<_> = (0..220)
        .map(|i| (0.07 + i as f64 * 60.0 / 94.0) / 0.02)
        .map(|t| t.round() * 0.02)
        .collect();
    let old = estimate_v4(&events, 142.0);
    assert!(old.bpm.is_some());
    let new = estimate_with_budget(&events, 142.0, 0, 0);
    assert_eq!(
        serde_json::to_string(&old).unwrap(),
        serde_json::to_string(&new).unwrap()
    );
    assert!(!serde_json::to_string(&new).unwrap().contains("ordinal_fit"));
}

#[test]
fn missing_and_extra_events_recover_scalar_tempo_without_mutating_observations() {
    for extra in [false, true] {
        let events = interrupted(97.125, extra);
        let before = events.clone();
        let old = estimate_v4(&events, 187.0);
        assert!(old.bpm.is_none());
        let result = estimate(&events, 187.0);
        assert!(result.bpm.is_some(), "{result:?}");
        assert!((result.bpm.unwrap() - 97.125).abs() < 0.02, "{result:?}");
        assert_eq!(events, before);
        let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
            panic!("inferred evidence missing");
        };
        assert_eq!(proof.outcome, "supported");
        assert_eq!(proof.winner.as_ref().unwrap().sections.len(), 5);
        assert!(
            proof.evaluated_candidate_count <= MAX_FITS && proof.alternatives.len() < MAX_SUMMARIES
        );
        assert_eq!(proof.assignments[0].1, 0);
    }
}

#[test]
fn canonical_interval_ranking_preserves_low_and_high_missing_event_tempi() {
    for bpm in [37.25, 273.5] {
        for extra in [false, true] {
            let events = interrupted(bpm, extra);
            let before = events.clone();
            let duration = 300.0 * 60.0 / bpm + 0.5;
            assert!(estimate_v4(&events, duration).bpm.is_none());
            let result = estimate(&events, duration);
            assert!(
                result.bpm.is_some(),
                "authored BPM {bpm}, extras {extra}: {result:?}"
            );
            assert!(
                (result.bpm.unwrap() - bpm).abs() < 0.04,
                "authored BPM {bpm}, extras {extra}: {result:?}"
            );
            assert_eq!(events, before);
            let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
                panic!("inferred evidence missing");
            };
            assert_eq!(proof.outcome, "supported");
            assert!(
                proof.evaluated_candidate_count <= MAX_FITS
                    && proof.alternatives.len() < MAX_SUMMARIES
            );
        }
    }
}

#[test]
fn majority_incoherent_extras_do_not_define_a_fixed_inlier_fraction_gate() {
    let mut events = interrupted(97.125, false);
    let period = 60.0 / 97.125;
    let mut state = 0x1937_u64;
    let extra_count = events.len() + events.len() / 4;
    for i in 0..extra_count {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let unit = (state >> 11) as f64 / ((1_u64 << 53) as f64);
        events.push(0.07 + (i as f64 + unit) * 300.0 / extra_count as f64 * period);
    }
    events.sort_by(f64::total_cmp);
    let result = estimate(&events, 187.0);
    assert!(result.bpm.is_some(), "{result:?}");
    assert!((result.bpm.unwrap() - 97.125).abs() < 0.03, "{result:?}");
}

#[test]
fn odd_and_even_alternating_parities_never_become_a_strong_half_tempo() {
    for count in [600, 601] {
        let events: Vec<_> = (0..count)
            .map(|i| 0.05 + i as f64 * 0.25 + if i % 2 == 0 { 0.016 } else { -0.016 })
            .collect();
        let result = estimate(&events, 151.0);
        assert!(result.bpm.is_none(), "{result:?}");
        if let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit {
            assert_ne!(proof.outcome, "supported");
        }
    }
}

#[test]
fn short_original_has_explicit_short_evidence_without_claiming_long_probes() {
    let events = [0.1, 0.6, 1.1, 1.6, 2.1, 2.6];
    let result = estimate(&events, 2.8);
    let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
        panic!("short evidence missing");
    };
    assert!(proof.winner.is_some());
    assert_eq!(proof.evidence_tier, "short_source");
    assert_eq!(proof.probe_windows, vec![[0.0, 1.4], [1.4, 2.8]]);
}

#[test]
fn eight_second_missing_event_clip_keeps_its_nonunit_ambiguity() {
    let events: Vec<_> = (0..16)
        .filter(|i| ![5, 11].contains(i))
        .map(|i| 0.1 + i as f64 * 0.5)
        .collect();
    assert!(estimate_v4(&events, 8.0).bpm.is_none());
    let result = estimate(&events, 8.0);
    assert!(result.bpm.is_none(), "{result:?}");
    let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
        panic!("short evidence missing");
    };
    assert_eq!(proof.evidence_tier, "short_source");
    assert_eq!(proof.outcome, "ambiguous");
    let selection = proof.period_selection.as_ref().unwrap();
    assert!(selection.selected_trial_index.is_none());
    let winner = proof.winner.as_ref().unwrap();
    assert!(
        proof
            .alternatives
            .iter()
            .any(|candidate| candidate.global.sustained()
                && !equivalent(candidate, winner, 8.0)
                && !unit_counterpart(candidate, winner, 8.0)
                && candidate.global.band().1 >= winner.global.band().0)
    );
}

#[test]
fn complete_short_clip_keeps_tempo_when_only_unit_bands_overlap() {
    // Eight complete observations cannot meet the old four-second observed-span
    // gate. Their coherent half/double uncertainty is independent of precision.
    let events: Vec<_> = (0..8).map(|i| 0.125 + i as f64 * 0.5).collect();
    assert!(estimate_v4(&events, 4.0).bpm.is_none());
    let result = estimate(&events, 4.0);
    assert_eq!(result.bpm, Some(120.0), "{result:?}");
    let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
        panic!("period evidence missing");
    };
    assert_eq!(proof.outcome, "supported");
    let winner = proof.winner.as_ref().unwrap();
    assert!(
        proof
            .alternatives
            .iter()
            .any(|candidate| unit_counterpart(candidate, winner, 4.0)
                && candidate.global.band().1 >= winner.global.band().0)
    );
}

#[test]
fn sixteen_second_missing_event_clip_separates_the_supported_harmonic() {
    let events: Vec<_> = (0..32)
        .filter(|i| ![11, 23].contains(i))
        .map(|i| 0.1 + i as f64 * 0.5)
        .collect();
    assert!(estimate_v4(&events, 16.0).bpm.is_none());
    let result = estimate(&events, 16.0);
    assert_eq!(result.bpm, Some(120.0), "{result:?}");
    let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
        panic!("short evidence missing");
    };
    assert_eq!(proof.evidence_tier, "short_source");
}

#[test]
fn boundary_straddlers_do_not_inflate_half_open_section_counts() {
    let events = [0.249, 0.75, 1.251, 1.75];
    let points = assignment(&events, 2.0, 0.5, 0.25).unwrap();
    let left = span(&events, &points, 0.5, 0.25, [0.0, 1.25]).unwrap();
    let right = span(&events, &points, 0.5, 0.25, [1.25, 2.0]).unwrap();
    assert_eq!(
        (left.raw_count, left.expected_slots, left.matched),
        (2, 2, 2)
    );
    assert_eq!(
        (right.raw_count, right.expected_slots, right.matched),
        (2, 2, 2)
    );
    let across = span(&events, &points, 0.5, 0.25, [0.25, 1.75]).unwrap();
    assert_eq!(
        (across.raw_count, across.expected_slots, across.matched),
        (2, 3, 2)
    );
    assert!(ordinal(f64::INFINITY).is_none());
    assert!(ordinal(EXACT_INTEGER * 2.0).is_none());
    assert_eq!(ordinal(2.5), Some(2));
    assert_eq!(ordinal(3.5), Some(4));
}

#[test]
fn new_reserve_refusal_is_honest_and_does_not_allocate_search_vectors() {
    let events = interrupted(97.125, true);
    let needed = preparation_bytes(events.len(), 3).unwrap();
    let result = estimate_with_budget(&events, 187.0, 3, needed - 1);
    let Some(OrdinalFit::Budget(refusal)) = result.ordinal_fit else {
        panic!("budget refusal missing");
    };
    assert_eq!(refusal.outcome, "budget_unavailable");
    assert_eq!(refusal.required_bytes, needed);
    assert_eq!(refusal.available_bytes, needed - 1);
    assert!(result.bpm.is_none());
    assert!(preparation_bytes(usize::MAX, 0).is_none());
    assert!(reserve::<u64>(usize::MAX).is_none());
}

#[test]
fn only_fixedpoint_mappings_can_claim_their_reported_free_line() {
    // An independently authored binary pulse train supplies a valid close seed.
    // Arbitrary distant seeds are allowed to miss the bounded six-iteration fit.
    let mut events: Vec<_> = (0..300)
        .filter(|i| i % 17 != 5)
        .map(|i| 0.125 + i as f64 * 0.625)
        .collect();
    events.extend(
        (0..300)
            .filter(|i| i % 4 == 1)
            .map(|i| 0.3125 + i as f64 * 0.625),
    );
    events.sort_by(f64::total_cmp);
    let candidate = fit(&events, 188.0, 0.625, 0.125).unwrap().unwrap();
    let points = assignment(
        &events,
        188.0,
        candidate.period_seconds,
        candidate.anchor_seconds,
    )
    .unwrap();
    let (mx, my, p) = line(&points).unwrap();
    assert_eq!(p.to_bits(), candidate.period_seconds.to_bits());
    assert_eq!(mx.to_bits(), candidate.pivot_ordinal.to_bits());
    assert_eq!(my.to_bits(), candidate.pivot_seconds.to_bits());
    assert_eq!(points[0].ordinal, 0);
}

#[test]
fn acoustic_gate_recomputes_phase_only_failure_and_keeps_other_guards() {
    let duration = 180.0;
    let beats: Vec<_> = (0..360)
        .map(|i| {
            let time = 0.05 + i as f64 * 0.5;
            time + if (2.0..34.0).contains(&time) {
                0.01
            } else if (146.0..178.0).contains(&time) {
                -0.01
            } else {
                0.0
            }
        })
        .collect();
    let winner = fit(&beats, duration, 0.5, 0.05).unwrap().unwrap();
    let points = assignment(
        &beats,
        duration,
        winner.period_seconds,
        winner.anchor_seconds,
    )
    .unwrap();
    let never = || false;
    let control = Control::new(&never);
    let mut evidence = empty_evidence();
    evidence.outcome = "insufficient";
    evidence.evidence_tier = "two_ten_second";
    evidence.probe_windows = probes(&beats, duration, &winner, &points, &control).unwrap();
    evidence.assignments = points.iter().map(|p| (p.raw, p.ordinal)).collect();
    evidence.winner = Some(winner);
    assert!(
        phase_only_period(&beats, duration, &evidence, &control)
            .unwrap()
            .is_some()
    );
    evidence.candidate_limit_reached = true;
    assert!(
        phase_only_period(&beats, duration, &evidence, &control)
            .unwrap()
            .is_none()
    );
    evidence.candidate_limit_reached = false;
    evidence.outcome = "ambiguous";
    assert!(
        phase_only_period(&beats, duration, &evidence, &control)
            .unwrap()
            .is_none()
    );
    evidence.outcome = "insufficient";
    evidence.conflicting_sections.push(0);
    assert!(
        phase_only_period(&beats, duration, &evidence, &control)
            .unwrap()
            .is_none()
    );
    evidence.conflicting_sections.clear();
    let mut changed = beats.clone();
    changed[180] += 0.1;
    assert!(
        phase_only_period(&changed, duration, &evidence, &control)
            .unwrap()
            .is_none()
    );
    assert!(phase_only_period(&beats, duration, &evidence, &Control::new(&|| true)).is_err());
}

#[test]
fn phase_only_gate_respects_the_effective_trial_period_at_a_strict_boundary() {
    let free_period = 0.4000000000000002_f64;
    let candidate_bpm = 60.0 / free_period;
    let trial_period = 60.0 / candidate_bpm;
    assert!(limit(trial_period) < limit(free_period));
    let points = [
        Point {
            raw: 0,
            ordinal: 0,
            seconds: limit(free_period),
        },
        Point {
            raw: 1,
            ordinal: 1,
            seconds: trial_period + 0.01,
        },
        Point {
            raw: 2,
            ordinal: 101,
            seconds: trial_period * 101.0 - 0.01,
        },
    ];
    let never = || false;
    let checked = check(
        &points,
        &[[0.0, 1.0], [40.0, 41.0]],
        (0.0, 0.0, free_period),
        candidate_bpm,
        free_period,
        0.0,
        &Control::new(&never),
    )
    .unwrap()
    .unwrap();
    assert_eq!(checked.status, "fail");
    assert!(checked.max_residual_seconds.unwrap() > limit(trial_period));
    assert!(phase_failure_only(&checked, free_period));
    assert!(!phase_failure_only(&checked, trial_period));
}

#[test]
fn critical_witnesses_use_veto_bands_instead_of_point_score_order() {
    let candidate = |period: f64, raw, expected, matched| {
        let diagnostics = SpanDiagnostics {
            window_seconds: [0.0, 100.0],
            raw_count: raw,
            expected_slots: expected,
            matched,
            observed_span_seconds: Some(90.0),
            fitted_bpm: Some(60.0 / period),
            max_residual_seconds: Some(0.0),
            rms_residual_seconds: Some(0.0),
            max_gap_seconds: period,
        };
        CandidateSummary {
            period_seconds: period,
            anchor_seconds: 0.1,
            pivot_ordinal: 10.0,
            pivot_seconds: 10.0,
            global: diagnostics.clone(),
            sections: vec![diagnostics; 5],
        }
    };
    let winner = candidate(0.6, 100, 100, 100);
    let pool = [candidate(0.8, 8, 9, 4), candidate(1.0, 220, 220, 100)];
    assert!(pool[0].global.score() > pool[1].global.score());
    assert!(pool[0].global.band().1 > pool[1].global.band().1);
    assert!(pool[0].global.band().0 < pool[1].global.band().0);
    assert_eq!(strongest(&pool, &winner, 100.0, None), Some(0));
    assert_eq!(strongest(&pool, &winner, 100.0, Some(2)), Some(1));
}

fn bounded_summary(period: f64, duration: f64) -> CandidateSummary {
    let diagnostics = SpanDiagnostics {
        window_seconds: [0.0, duration],
        raw_count: 100,
        expected_slots: 100,
        matched: 80,
        observed_span_seconds: Some(duration),
        fitted_bpm: Some(60.0 / period),
        max_residual_seconds: Some(0.0),
        rms_residual_seconds: Some(0.0),
        max_gap_seconds: period,
    };
    CandidateSummary {
        period_seconds: period,
        anchor_seconds: 0.0,
        pivot_ordinal: 10.0,
        pivot_seconds: 10.0,
        global: diagnostics.clone(),
        sections: vec![diagnostics; 5],
    }
}

#[test]
fn common_winding_refuses_whole_cycle_aliases_on_long_extents() {
    let a = bounded_summary(1.0, 100_000_000.0);
    let mut b = a.clone();
    b.period_seconds = 1.000_000_05;
    // The old independent modulo checks at20%/80% can see one/four completed
    // cycles and conceal the accumulated drift across the complete source.
    assert!(!equivalent(&a, &b, 100_000_000.0));
    b.period_seconds = 1.0;
    b.anchor_seconds = 1000.0;
    assert!(equivalent(&a, &b, 100_000_000.0));
}

#[test]
fn one_discarded_sustained_edge_family_is_a_search_limit_witness() {
    let mut winner = bounded_summary(0.5, 200.0);
    winner.sections[0].matched = 0;
    winner.sections[0].observed_span_seconds = None;
    let edge = Seed {
        period: 0.8,
        anchor: 0.1,
        matched: 8,
        sections: 1,
        upper: 0.1,
        local_upper: [0.8, 0.0, 0.0, 0.0, 0.0],
        local_sustained: [true, false, false, false, false],
        family: 0,
        source_pair: [0, 1],
        source_count: 1,
        refined_from: None,
    };
    assert!(coarse_could_veto(&edge, &winner));
}

#[test]
fn coherence_and_upper_bound_remain_on_the_same_concrete_seed() {
    let mut winner = bounded_summary(0.5, 200.0);
    winner.sections[0].matched = 40;
    let seed = Seed {
        period: 0.8,
        anchor: 0.1,
        matched: 4,
        sections: 1,
        upper: 0.1,
        local_upper: [0.2, 0.0, 0.0, 0.0, 0.0],
        local_sustained: [true, false, false, false, false],
        family: 0,
        source_pair: [0, 1],
        source_count: 1,
        refined_from: None,
    };
    let mut other_phase = seed;
    other_phase.anchor = 0.3;
    other_phase.local_upper[0] = 0.9;
    other_phase.local_sustained[0] = false;
    let mut discovery = Discovery {
        seeds: reserve(MAX_COARSE_SEEDS).unwrap(),
        representatives: reserve(MAX_COARSE_FAMILIES).unwrap(),
        limited: false,
        family_count: 0,
    };
    retain_seed(&mut discovery, seed).unwrap();
    retain_seed(&mut discovery, other_phase).unwrap();
    assert_eq!(discovery.seeds.len(), 2);
    assert!(
        !discovery
            .seeds
            .iter()
            .any(|candidate| coarse_could_veto(candidate, &winner))
    );
    other_phase.anchor += 0.01;
    other_phase.local_sustained[0] = true;
    retain_seed(&mut discovery, other_phase).unwrap();
    assert!(
        discovery
            .seeds
            .iter()
            .any(|candidate| coarse_could_veto(candidate, &winner))
    );
    assert!(constant_storage_bytes().unwrap() <= FIXED_RESERVE);
}

#[test]
fn exact_start_memo_never_discharges_another_phase_in_the_same_family() {
    let attempts = [(0.5_f64.to_bits(), 0.1_f64.to_bits())];
    assert!(was_attempted(&attempts, 0.5, 0.1));
    assert!(!was_attempted(&attempts, 0.5, 0.13));
    assert!(!was_attempted(&attempts, 0.5_f64.next_up(), 0.1));
}

#[test]
#[ignore = "explicit owned cached binary64 input/output paths; coarse diagnostics only"]
fn trace_cached_coarse_witnesses() {
    let input = std::path::PathBuf::from(
        std::env::var_os("FLITZI_TEMPO_COARSE_TRACE_INPUT").expect("explicit cached input"),
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("FLITZI_TEMPO_COARSE_TRACE_OUTPUT").expect("explicit owned output"),
    );
    assert!(input.is_absolute() && output.is_absolute());
    assert!(
        output.parent().unwrap().join("OWNER.md").is_file(),
        "owned output block required"
    );
    let filter = std::env::var("FLITZI_TEMPO_COARSE_TRACE_ID").ok();
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(&input).unwrap()).unwrap();
    let mut cases = Vec::new();
    for case in data["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if filter.as_deref().is_some_and(|filter| filter != id) {
            continue;
        }
        let beats: Vec<_> = case["beat_bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bits| f64::from_bits(bits.as_u64().unwrap()))
            .collect();
        let duration = f64::from_bits(case["duration_bits"].as_u64().unwrap());
        let sample = sample_indices(&beats, duration).unwrap();
        let discovery = discover(&beats, duration, &sample).unwrap();
        let rows: Vec<_> = discovery.seeds.iter().enumerate().map(|(index,seed)| {
            let rank = discovery.representatives.iter().position(|&representative| discovery.seeds[representative].family == seed.family).unwrap();
            serde_json::json!({
                "seed_id":index,"family":seed.family,"family_rank":rank,
                "representative":discovery.representatives[rank]==index,
                "period_seconds":seed.period,"bpm":60.0/seed.period,"anchor_seconds":seed.anchor,
                "refined_from":seed.refined_from,"source_pair":seed.source_pair,"source_ordinal_gap":seed.source_count,
                "sample_matched":seed.matched,"sample_sections":seed.sections,
                "global_upper":seed.upper,"local_upper":seed.local_upper,"local_sustained":seed.local_sustained,
            })
        }).collect();
        cases.push(serde_json::json!({"id":id,"duration_seconds":duration,"raw_count":beats.len(),"sample_indices":sample,"limited":discovery.limited,"rows":rows}));
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer_pretty(file, &serde_json::json!({"scope":"coarse numeric discovery only; no inference or playback authority","constant_storage_bytes":constant_storage_bytes(),"cases":cases})).unwrap();
}

#[test]
#[ignore = "explicit owned cached binary64 paths; observes the existing bounded evaluator"]
fn trace_cached_search_witnesses() {
    let input = std::path::PathBuf::from(
        std::env::var_os("FLITZI_TEMPO_SEARCH_TRACE_INPUT").expect("explicit cached input"),
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("FLITZI_TEMPO_SEARCH_TRACE_OUTPUT").expect("explicit owned output"),
    );
    assert!(input.is_absolute() && output.is_absolute());
    let parent = output
        .parent()
        .unwrap()
        .canonicalize()
        .expect("existing output directory");
    let owner =
        std::fs::read_to_string(parent.join("OWNER.md")).expect("owned output block required");
    assert!(
        !owner.trim().is_empty(),
        "nonempty owner declaration required"
    );
    let output = parent.join(output.file_name().unwrap());
    let filter = std::env::var("FLITZI_TEMPO_SEARCH_TRACE_ID").ok();
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let mut cases = Vec::new();
    for case in data["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if filter.as_deref().is_some_and(|filter| filter != id) {
            continue;
        }
        let beats: Vec<_> = case["beat_bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bits| f64::from_bits(bits.as_u64().unwrap()))
            .collect();
        let duration = f64::from_bits(case["duration_bits"].as_u64().unwrap());
        let scope = ObserverScope::begin();
        let result = estimate(&beats, duration);
        let trace = scope.finish();
        let outcome = result.ordinal_fit.as_ref().map(|fit| match fit {
            OrdinalFit::Budget(unavailable) => serde_json::json!({"outcome":unavailable.outcome}),
            OrdinalFit::Evaluated(evidence) => serde_json::json!({"outcome":evidence.outcome,
                "candidate_limit_reached":evidence.candidate_limit_reached,"evaluated_candidate_count":evidence.evaluated_candidate_count}),
        });
        cases.push(serde_json::json!({"id":id,"duration_seconds":duration,"raw_count":beats.len(),
            "policy":result.policy,"status":result.status,"bpm":result.bpm,"ordinal_fit":outcome,"trace":trace}));
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer_pretty(file, &serde_json::json!({"scope":"Observes the unchanged bounded evaluator; no extra fits, inference or playback authority",
        "max_search_fits":SEARCH_FITS,"max_harmonic_checks":2,"max_coarse_seeds":MAX_COARSE_SEEDS,
        "layout":{"seed":std::mem::size_of::<Seed>(),"span":std::mem::size_of::<SpanDiagnostics>(),
            "candidate":std::mem::size_of::<CandidateSummary>(),"observed_interval":std::mem::size_of::<ObservedInterval>(),
            "interval_family":std::mem::size_of::<IntervalFamily>(),"fixed_search_bytes":constant_storage_bytes(),
            "fixed_admitted_bytes":FIXED_RESERVE,"point":std::mem::size_of::<Point>()},"cases":cases})).unwrap();
}

#[test]
fn harmonic_declarations_replay_only_the_two_exact_bounded_starts() {
    let events = interrupted(97.125, true);
    let (_, evidence) = evaluate(&events, 187.0).unwrap();
    let selection = evidence.period_selection.as_ref().unwrap();
    let summaries: Vec<_> = std::iter::once(evidence.winner.as_ref().unwrap())
        .chain(evidence.alternatives.iter())
        .collect();
    let canonical = summaries[selection.canonical_index];
    assert!(evidence.harmonics.is_empty());
    assert_eq!(selection.harmonics.len(), 2);
    assert!(evidence.evaluated_candidate_count <= MAX_FITS);
    for (factor, declared) in [0.5, 2.0].into_iter().zip(&selection.harmonics) {
        assert_eq!(declared.factor, factor);
        assert_eq!(
            declared.seed_period_seconds,
            canonical.period_seconds * factor
        );
        assert_eq!(declared.seed_anchor_seconds, canonical.anchor_seconds);
        if !(MIN_PERIOD..=MAX_PERIOD).contains(&declared.seed_period_seconds) {
            assert_eq!(declared.outcome, "out_of_range");
            assert!(declared.candidate_index.is_none());
        } else {
            match fit(
                &events,
                187.0,
                declared.seed_period_seconds,
                declared.seed_anchor_seconds,
            )
            .unwrap()
            {
                Some(candidate) => {
                    assert_eq!(declared.outcome, "fitted");
                    assert_eq!(*summaries[declared.candidate_index.unwrap()], candidate);
                }
                None => {
                    assert_eq!(declared.outcome, "no_fixed_fit");
                    assert!(declared.candidate_index.is_none());
                }
            }
        }
    }
    assert!(fit(&events, 187.0, f64::NAN, 0.0).unwrap().is_none());
}

#[test]
fn direct_unit_proof_has_one_global_winding_and_a_strict_drift_boundary() {
    let fine = bounded_summary(0.46875, 0.08);
    let coarse = bounded_summary(1.0, 0.08);
    assert!(unit_counterpart(&fine, &coarse, 0.08));
    assert!(unit_counterpart(&coarse, &fine, 0.08));
    assert!(!unit_counterpart(&fine, &coarse, 0.08_f64.next_up()));
    let fine = bounded_summary(0.5, 180.0);
    let mut coarse = bounded_summary(1.0, 180.0);
    coarse.anchor_seconds = 40.0;
    assert!(unit_counterpart(&fine, &coarse, 180.0));
    assert!(strongest(&[coarse.clone()], &fine, 180.0, None).is_none());
    for section in 0..5 {
        assert!(strongest(&[coarse.clone()], &fine, 180.0, Some(section)).is_none());
    }
    coarse.anchor_seconds += 0.25;
    assert!(!unit_counterpart(&fine, &coarse, 180.0));
    assert!(!unit_counterpart(
        &fine,
        &bounded_summary(2.0, 180.0),
        180.0
    ));
    assert!(!unit_counterpart(
        &fine,
        &bounded_summary(0.75, 180.0),
        180.0
    ));
    assert!(!unit_counterpart(&fine, &coarse, f64::INFINITY));
}

fn biased_subdivision_events() -> Vec<f64> {
    (0..360)
        .filter(|i| i % 2 == 0 || i % 6 != 5)
        .map(|i| {
            let time = 0.125 + i as f64 * 0.5;
            // Equal repeated support in four blocks gives exactly zero bias
            // sum and ordinal/bias covariance. Binary amplitudes preserve the
            // .5-second canonical slope while its distant probes still differ.
            let bias = if i % 2 == 0 {
                0.0
            } else {
                [1.0, -3.0, 3.0, -1.0][i as usize / 90] / 128.0
            };
            time + bias
        })
        .collect()
}

#[test]
fn a_precise_half_period_survives_the_failed_higher_support_canonical() {
    let events = biased_subdivision_events();
    let before = events.clone();
    let result = estimate(&events, 180.0);
    assert_eq!(result.bpm, Some(60.0), "{result:?}");
    assert_eq!(events, before);
    let Some(OrdinalFit::Evaluated(proof)) = result.ordinal_fit else {
        panic!("period evidence missing");
    };
    let selection = proof.period_selection.as_ref().unwrap();
    let canonical = if selection.canonical_index == 0 {
        proof.winner.as_ref().unwrap()
    } else {
        &proof.alternatives[selection.canonical_index - 1]
    };
    assert_eq!(canonical.period_seconds, 0.5);
    assert_eq!(canonical.anchor_seconds, 0.125);
    let chosen = selection.selected_trial_index.unwrap();
    assert!(chosen > 0);
    assert_eq!(selection.trials[chosen].candidate_index, 0);
    assert_eq!(
        selection.trials[0]
            .fractional_check
            .as_ref()
            .unwrap()
            .status,
        "fail"
    );
    assert_eq!(
        selection.trials[chosen]
            .fractional_check
            .as_ref()
            .unwrap()
            .status,
        "pass"
    );
    assert!(proof.alternatives.len() < MAX_SUMMARIES);
    assert_eq!(selection.harmonics.len(), 2);
    assert!(proof.harmonics.is_empty());
}

#[test]
fn failed_alias_precision_spends_no_coverage_fuel_or_false_limit() {
    let events: Vec<_> = (0..360)
        .map(|i| {
            let t = 0.05 + i as f64 * 0.5;
            t + if (2.0..34.0).contains(&t) {
                0.01
            } else if (146.0..178.0).contains(&t) {
                -0.01
            } else {
                0.0
            }
        })
        .collect();
    let canonical = fit(&events, 180.0, 0.5, 0.05).unwrap().unwrap();
    let mut pool = vec![canonical.clone()];
    let mut declared = Vec::new();
    for factor in [0.5, 2.0] {
        let candidate = fit(
            &events,
            180.0,
            canonical.period_seconds * factor,
            canonical.anchor_seconds,
        )
        .unwrap()
        .unwrap();
        let index = pool.len();
        pool.push(candidate);
        declared.push(PeriodHarmonic {
            factor,
            seed_period_seconds: canonical.period_seconds * factor,
            seed_anchor_seconds: canonical.anchor_seconds,
            outcome: "fitted",
            candidate_index: Some(index),
        });
    }
    let calls = std::cell::Cell::new(0);
    let (trials, selected) = select_period_trials(
        &events,
        180.0,
        0,
        &declared,
        false,
        &mut pool,
        &Control::new(&never_cancel),
        |_, _| {
            calls.set(calls.get() + 1);
            Ok(false)
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 0);
    assert_eq!(trials.len(), 3);
    assert!(selected.is_none());
    assert!(trials.iter().all(|trial| !trial.candidate_limit_reached
        && trial.fractional_check.as_ref().unwrap().status == "fail"));
}

#[test]
fn a_new_rival_from_alias_coverage_cannot_be_promoted_or_hidden() {
    let events = biased_subdivision_events();
    let canonical = fit(&events, 180.0, 0.5, 0.125).unwrap().unwrap();
    let half = fit(
        &events,
        180.0,
        canonical.period_seconds * 2.0,
        canonical.anchor_seconds,
    )
    .unwrap()
    .unwrap();
    let mut pool = vec![canonical.clone(), half];
    let declared = [PeriodHarmonic {
        factor: 2.0,
        seed_period_seconds: canonical.period_seconds * 2.0,
        seed_anchor_seconds: canonical.anchor_seconds,
        outcome: "fitted",
        candidate_index: Some(1),
    }];
    let (trials, selected) = select_period_trials(
        &events,
        180.0,
        0,
        &declared,
        false,
        &mut pool,
        &Control::new(&never_cancel),
        |_, pool| {
            let mut rival = bounded_summary(0.8, 180.0);
            rival.global.raw_count = 300;
            rival.global.expected_slots = 225;
            rival.global.matched = 220;
            pool.push(rival);
            Ok(true)
        },
    )
    .unwrap();
    assert!(selected.is_none());
    assert_eq!(trials.len(), 2);
    assert!(trials[1].critical_indices.contains(&2));
    assert_ne!(trials[1].outcome, "supported");
}

#[test]
fn canonical_success_is_kept_and_alias_coverage_refusal_stops_selection() {
    let events = biased_subdivision_events();
    let fast = fit(&events, 180.0, 0.5, 0.125).unwrap().unwrap();
    let half = fit(&events, 180.0, 1.0, 0.125).unwrap().unwrap();
    let control = Control::new(&never_cancel);
    let mut pool = vec![half.clone(), fast.clone()];
    let declared = [PeriodHarmonic {
        factor: 0.5,
        seed_period_seconds: half.period_seconds * 0.5,
        seed_anchor_seconds: half.anchor_seconds,
        outcome: "fitted",
        candidate_index: Some(1),
    }];
    let (trials, selected) = select_period_trials(
        &events,
        180.0,
        0,
        &declared,
        false,
        &mut pool,
        &control,
        |_, _| panic!("a precise canonical does not start alias coverage"),
    )
    .unwrap();
    assert_eq!(selected, Some(0));
    assert_eq!(trials.len(), 1);
    let mut pool = vec![fast.clone(), half];
    let declared = [PeriodHarmonic {
        factor: 2.0,
        seed_period_seconds: fast.period_seconds * 2.0,
        seed_anchor_seconds: fast.anchor_seconds,
        outcome: "fitted",
        candidate_index: Some(1),
    }];
    let (trials, selected) = select_period_trials(
        &events,
        180.0,
        0,
        &declared,
        false,
        &mut pool,
        &control,
        |_, _| Ok(false),
    )
    .unwrap();
    assert!(selected.is_none());
    assert!(!trials[0].candidate_limit_reached);
    assert!(trials.last().unwrap().candidate_limit_reached);
    assert_eq!(trials.last().unwrap().outcome, "search_limited");
}

#[test]
fn a_stronger_unit_counterpart_cannot_hide_nonunit_global_or_local_vetoes() {
    let winner = bounded_summary(1.0, 180.0);
    let unit = bounded_summary(0.5, 180.0);
    let rival = bounded_summary(0.8, 180.0);
    let pool = [unit, rival];
    assert_eq!(strongest(&pool, &winner, 180.0, None), Some(1));
    for section in 0..5 {
        assert_eq!(strongest(&pool, &winner, 180.0, Some(section)), Some(1));
    }
}

#[test]
fn fixed_alias_coverage_completes_without_canonical_winner_restarts() {
    let events = interrupted(97.125, true);
    let duration = 187.0;
    let control = Control::new(&never_cancel);
    let sample = sample_indices(&events, duration).unwrap();
    let geometry = build_geometry(&events, duration, &sample, &control).unwrap();
    let (families, _, limited) =
        representative_census(&events, duration, &sample, &geometry, &control).unwrap();
    assert!(!limited);
    let basis = fit(&events, duration, 60.0 / 97.125, 0.08)
        .unwrap()
        .unwrap();
    let mut pool = reserve(MAX_FITS).unwrap();
    pool.push(basis.clone());
    let mut discovery = Discovery {
        seeds: reserve(MAX_COARSE_SEEDS).unwrap(),
        limited: false,
        representatives: Vec::new(),
        family_count: 0,
    };
    let mut evidence = empty_evidence();
    let mut attempts = reserve(SEARCH_FITS).unwrap();
    assert!(
        cover_alias(
            &events,
            duration,
            &sample,
            &geometry,
            &families,
            &basis,
            &mut discovery,
            &mut evidence,
            &mut pool,
            &mut attempts,
            &control
        )
        .unwrap()
    );
    assert!(!attempts.is_empty());
    assert_eq!(pool[0], basis);
    assert!(attempts.len() <= SEARCH_FITS);
}

#[test]
fn shared_search_fuel_still_allows_4096_starts_after_two_canonical_checks() {
    let mut evidence = empty_evidence();
    evidence.evaluated_candidate_count = 2;
    let mut pool = reserve(MAX_FITS).unwrap();
    let mut attempts = reserve(SEARCH_FITS).unwrap();
    let control = Control::new(&never_cancel);
    for index in 0..SEARCH_FITS {
        assert!(
            search_attempt(
                &[0.1, 0.6],
                2.0,
                0.5,
                index as f64 / 10000.0,
                &mut evidence,
                &mut pool,
                &mut attempts,
                &control
            )
            .unwrap()
        );
    }
    assert_eq!(evidence.evaluated_candidate_count, MAX_FITS);
    assert!(
        !search_attempt(
            &[0.1, 0.6],
            2.0,
            0.5,
            -1.0,
            &mut evidence,
            &mut pool,
            &mut attempts,
            &control
        )
        .unwrap()
    );
    assert_eq!(evidence.evaluated_candidate_count, MAX_FITS);
}

#[test]
#[ignore = "explicit owned lossless cached observations; scalar math only, no inference or OwnACK"]
fn replay_cached_scalar_periods() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let input = std::path::PathBuf::from(
        std::env::var_os("FLITZI_TEMPO_REPLAY_INPUT").ok_or("explicit input required")?,
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("FLITZI_TEMPO_REPLAY_OUTPUT").ok_or("explicit output required")?,
    );
    assert!(input.is_absolute() && output.is_absolute());
    let input = input.canonicalize()?;
    let parent = output
        .parent()
        .ok_or("output parent required")?
        .canonicalize()?;
    assert!(
        !std::fs::read_to_string(input.parent().unwrap().join("OWNER.md"))?
            .trim()
            .is_empty()
    );
    assert!(
        !std::fs::read_to_string(parent.join("OWNER.md"))?
            .trim()
            .is_empty()
    );
    assert!(std::fs::metadata(&input)?.len() <= 16 * 1024 * 1024);
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(input)?)?;
    let cases = data["cases"].as_array().ok_or("cases array required")?;
    assert!(!cases.is_empty() && cases.len() <= 64);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(parent.join(output.file_name().ok_or("output filename required")?))?;
    for case in cases {
        let bits = case["beat_bits"].as_array().ok_or("beat_bits required")?;
        assert!(bits.len() <= 250_000);
        let beats: Vec<_> = bits
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .map(f64::from_bits)
                    .ok_or("binary64 bits required")
            })
            .collect::<Result<_, _>>()?;
        let duration = f64::from_bits(
            case["duration_bits"]
                .as_u64()
                .ok_or("duration_bits required")?,
        );
        let downbeats = case.get("downbeat_count").map_or(Ok(0), |value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or("downbeat count required")
        })?;
        let start = std::time::Instant::now();
        let estimate = estimate_with_budget(&beats, duration, downbeats, 512 * 1024 * 1024);
        serde_json::to_writer(
            &mut file,
            &serde_json::json!({"id":case["id"],"estimate":estimate,
            "elapsed_seconds":start.elapsed().as_secs_f64(),
            "scope":"Cached scalar math only; no source validation, decode, inference, playback or OwnACK"}),
        )?;
        writeln!(&mut file)?;
        file.flush()?;
    }
    Ok(())
}
