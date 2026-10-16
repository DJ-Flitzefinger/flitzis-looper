"""Inspect fixed source-window pencils without changing the measured renderer."""

import time
from collections import Counter
from typing import TYPE_CHECKING

from pitch_causal_model_probe import emit
from pitch_independent_modes_probe import cases
from pitch_mode_carriers import HOP_FRAMES, WINDOW_FRAMES
from pitch_mode_certificate import inspect_window, integrity_controls

if TYPE_CHECKING:
    from pitch_overlap_probe import Fixture

GEOMETRIES = ("compact", "spread")


def inspect_fixture(fixture: Fixture) -> dict:
    """Report every original-raster source window and retain all refusal reasons."""
    counters = {geometry: Counter() for geometry in GEOMETRIES}
    for start in range(0, len(fixture.source) - WINDOW_FRAMES + 1, HOP_FRAMES):
        for geometry in GEOMETRIES:
            record = inspect_window(fixture.source, start, geometry)
            accepted = record["accepted_certificate"]
            counters[geometry]["windows"] += 1
            counters[geometry]["accepted"] += int(accepted)
            counters[geometry]["refused"] += int(not accepted)
            counters[geometry][record.get("rejection", "accepted_certificate")] += 1
            if accepted and not record["original_energy_guard_pass"]:
                counters[geometry]["accepted_after_energy_refusal"] += 1
            emit({"stage": "window_certificate", "fixture": fixture.name, **record})
    summary = {
        "stage": "fixture_summary",
        "fixture": fixture.name,
        "geometries": {name: dict(counts) for name, counts in counters.items()},
        "renderer_or_carrier_admission_changed": False,
    }
    emit(summary)
    return summary


def main() -> None:
    """Run independent algebra controls before inspecting original and unseen PCM."""
    emit({
        "stage": "contract",
        "window_frames": WINDOW_FRAMES,
        "hop_frames": HOP_FRAMES,
        "rows_per_channel": 4096,
        "columns": 32,
        "column_offsets": {"compact": "j", "spread": "floor(4095*j/31), j=0..31"},
        "pencil_shift_original_samples": 1,
        "thresholds": "unchanged rank, pole, conditioning and complete-window source-fit rules",
        "energy_crest_refusal_retained": True,
        "wet_pcm_or_scorer_targets_used": False,
        "renderer_or_carrier_admission_changed": False,
    })
    emit(integrity_controls())
    started = time.perf_counter()
    summaries = [inspect_fixture(fixture) for fixture in cases()]
    emit({
        "stage": "summary",
        "fixtures": len(summaries),
        "window_certificates": sum(
            record["windows"] for item in summaries for record in item["geometries"].values()
        ),
        "elapsed_seconds": time.perf_counter() - started,
        "renderer_or_carrier_admission_changed": False,
        "transformed_quality_or_live_gate_closed": False,
    })


if __name__ == "__main__":
    main()
