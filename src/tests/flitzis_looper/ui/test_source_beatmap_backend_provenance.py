"""Backend-aware inspector text never invents QM or internal frontend coordinates."""

from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

from flitzis_looper.controller.source_beatmap_selection import RawSourceBeatmapState
from flitzis_looper.ui.render import source_beatmap_editor as editor
from tests.flitzis_looper.controller.raw_source_beatmap_fakes import FakeRawCandidate

if TYPE_CHECKING:
    import pytest

    from flitzis_looper.ui.context import UiContext
    from flitzis_looper_audio import SourceBeatmapRawInfo


def _frame() -> editor.SourceMapEditorFrame:
    info = dict(FakeRawCandidate().info())
    info.update(
        backend="beat_this",
        capture_encoding="source-beatmap-unreviewed-beat-this-raw-v2",
        analyzer_sha256=None,
        analyzer_sample_rate_hz=None,
        analyzer_frame_count=None,
        conversion_revision=None,
        odf_hop_samples=None,
        configuration=None,
        detector_id="beat-this-1.1.0-final0-minimal",
        worker_input_sha256="b" * 64,
        worker_input_sample_rate_hz=48000,
        worker_input_frame_count=48000,
        worker_input_source_zero_frame=0,
        frontend_identity="beat-this-1.1.0-soxr-hq-logmel-v1",
        model={"checkpoint": "final0"},
        beat_logit_count=51,
        downbeat_logit_count=51,
    )
    return editor.SourceMapEditorFrame(
        pad_id=0,
        identity=object(),
        selected=False,
        status="none",
        geometry=None,
        revision=None,
        rate=None,
        anchor_count=0,
        can_undo=False,
        raw=RawSourceBeatmapState(
            attempt=1,
            status="unreviewed",
            occupied=True,
            revision="d" * 64,
            info=cast("SourceBeatmapRawInfo", info),
        ),
    )


def test_beat_this_provenance_displays_actual_worker_input_without_fictional_analyzer(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    text: list[str] = []
    monkeypatch.setattr(
        editor,
        "imgui",
        SimpleNamespace(
            text_colored=lambda _color, value: text.append(value),
            text_wrapped=text.append,
            tree_node=lambda _label: True,
            tree_pop=lambda: None,
        ),
    )
    editor._raw_provenance(_frame())
    shown = "\n".join(text)
    assert "Worker input: 48000 frames at 48000 Hz, frame zero 0" in shown
    assert "Worker input SHA-256: " + "b" * 64 in shown
    assert "Complete logits: 51 beat / 51 downbeat" in shown
    assert "Frontend identity: beat-this-1.1.0-soxr-hq-logmel-v1" in shown
    assert "Analyzer:" not in shown
    assert "ODF" not in shown
    assert "Hop:" not in shown
    assert "22050" not in shown


def test_beat_this_page_uses_source_seconds_when_no_odf_coordinate_exists(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    text: list[str] = []
    monkeypatch.setattr(
        editor,
        "imgui",
        SimpleNamespace(
            button=lambda _label: False,
            same_line=lambda **_kwargs: None,
            begin_disabled=lambda _disabled: None,
            end_disabled=lambda: None,
            text_colored=lambda _color, value: text.append(value),
            get_text_line_height_with_spacing=lambda: 1,
            begin_child=lambda *_args: None,
            end_child=lambda: None,
            text_unformatted=text.append,
        ),
    )
    monkeypatch.setattr(
        editor,
        "_read_raw_page",
        Mock(
            return_value={
                "status": "ready",
                "raw_revision": "d" * 64,
                "start_index": 0,
                "returned_count": 1,
                "total_event_count": 1,
                "has_more": False,
                "events": [
                    {
                        "raw_index": 0,
                        "odf_frame": None,
                        "source_seconds": 0.125,
                        "detected_downbeat": False,
                        "physical_domain": "inside",
                    }
                ],
            }
        ),
    )
    editor._show_raw_page(cast("UiContext", object()), _frame(), editor._RawInputs((1, "d" * 64)))
    assert any("source 0.125 s" in value for value in text)
    assert all("ODF frame" not in value for value in text)
