"""Reflect editor compatibility without creating an engine, source, GUI or device."""

import runpy
from inspect import Parameter, Signature, signature
from pathlib import Path
from types import SimpleNamespace
from typing import Never

import pytest

REPO = Path(__file__).resolve().parents[2]
_REQUIRE_FOUR_FOUR = True


def _forbidden(*_args: object, **_kwargs: object) -> Never:
    pytest.fail("Editor preflight must only reflect Native methods, never execute them")


def _create(
    self: object,
    sample_id: int,
    origin_seconds: float,
    period_seconds: float,
    *,
    expected_source_generation: int | None = None,
) -> Never:
    _forbidden()


def _edit(self: object, sample_id: int, receipt_json: str, operation_json: str) -> Never:
    _forbidden()


def _info(self: object) -> Never:
    _forbidden()


def _span(self: object, first_anchor_index: int, last_anchor_index: int) -> Never:
    _forbidden()


def _view(
    self: object,
    start_s: float,
    end_s: float,
    loop_start_s: float,
    px_per_s: float,
    max_points: int = 2048,
) -> Never:
    _forbidden()


def _after(
    self: object,
    source_seconds: float,
    beats: float,
    require_four_four: object = _REQUIRE_FOUR_FOUR,
) -> Never:
    _forbidden()


def source_beatmap_editor_native_module(engine_base: type = object) -> SimpleNamespace:
    """Return fresh nonexecuting capabilities for direct and full-runtime preflight."""
    return SimpleNamespace(
        AudioEngine=type(
            "EditorEngine",
            (engine_base,),
            {
                "__init__": _forbidden,
                "begin_source_beatmap_create_async": _create,
                "begin_source_beatmap_edit_async": _edit,
            },
        ),
        SourceBeatmap=type(
            "SourceBeatmap",
            (),
            {
                "__init__": _forbidden,
                "EDITOR_CORRECTION_API_VERSION": 2,
                "editor_info": _info,
                "editor_span": _span,
                "editor_view": _view,
                "source_after_beats": _after,
                **dict.fromkeys(
                    (
                        "revision",
                        "source_identity",
                        "evaluate_source",
                        "evaluate_beat",
                        "local_tempo_at_source",
                        "nearest_grid_source",
                    ),
                    _forbidden,
                ),
            },
        ),
    )


def _check(native: object) -> None:
    runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_source_beatmap_editor_api"](
        native
    )


def _replace_signature(owner: type, method: str, replacement: Signature) -> None:
    implementation = type(
        "NonexecutingEditorMethod",
        (),
        {"__call__": _forbidden, "__signature__": replacement},
    )()
    setattr(owner, method, implementation)


def test_current_editor_contract_is_reflected_without_any_native_work() -> None:
    _check(source_beatmap_editor_native_module())


def test_class_attribute_descriptor_can_expose_the_native_integer_capability() -> None:
    class CorrectionCapability:
        def __get__(self, _instance: object, _owner: type) -> int:
            return 2

    native = source_beatmap_editor_native_module()
    native.SourceBeatmap.EDITOR_CORRECTION_API_VERSION = CorrectionCapability()
    _check(native)


@pytest.mark.parametrize(
    ("owner_name", "method"),
    [
        ("AudioEngine", "begin_source_beatmap_create_async"),
        ("AudioEngine", "begin_source_beatmap_edit_async"),
        ("SourceBeatmap", "editor_info"),
        ("SourceBeatmap", "editor_span"),
        ("SourceBeatmap", "editor_view"),
        ("SourceBeatmap", "source_after_beats"),
        ("SourceBeatmap", "nearest_grid_source"),
    ],
)
def test_missing_editor_or_shared_snap_capability_rejects_stale_native(
    owner_name: str, method: str
) -> None:
    native = source_beatmap_editor_native_module()
    delattr(getattr(native, owner_name), method)
    with pytest.raises(AssertionError, match=method):
        _check(native)


def test_previous_methods_without_correction_version_reject_stale_native() -> None:
    native = source_beatmap_editor_native_module()
    delattr(native.SourceBeatmap, "EDITOR_CORRECTION_API_VERSION")
    with pytest.raises(AssertionError, match="EDITOR_CORRECTION_API_VERSION"):
        _check(native)


@pytest.mark.parametrize("invalid", [None, False, True, 2.0, "2", 0, 1, 3])
def test_correction_version_requires_exact_current_integer(invalid: object) -> None:
    native = source_beatmap_editor_native_module()
    native.SourceBeatmap.EDITOR_CORRECTION_API_VERSION = invalid
    with pytest.raises(AssertionError, match="EDITOR_CORRECTION_API_VERSION"):
        _check(native)


@pytest.mark.parametrize(
    "invalid", ["missing-first", "renamed-last", "optional-last", "keyword-only", "positional-only"]
)
def test_span_requires_exact_required_anchor_indices(invalid: str) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.SourceBeatmap.editor_span)
    parameters = list(complete.parameters.values())
    if invalid == "missing-first":
        parameters.pop(1)
    elif invalid == "renamed-last":
        parameters[-1] = parameters[-1].replace(name="anchor_index")
    elif invalid == "optional-last":
        parameters[-1] = parameters[-1].replace(default=None)
    elif invalid == "keyword-only":
        parameters[1:] = [
            parameter.replace(kind=Parameter.KEYWORD_ONLY) for parameter in parameters[1:]
        ]
    else:
        parameters = [parameter.replace(kind=Parameter.POSITIONAL_ONLY) for parameter in parameters]
    _replace_signature(native.SourceBeatmap, "editor_span", complete.replace(parameters=parameters))
    with pytest.raises(AssertionError, match="editor_span"):
        _check(native)


@pytest.mark.parametrize("invalid", ["missing", "positional", "wrong-default", "required"])
def test_create_requires_the_optional_keyword_only_source_generation_fence(invalid: str) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.AudioEngine.begin_source_beatmap_create_async)
    parameters = list(complete.parameters.values())
    if invalid == "missing":
        parameters.pop()
    else:
        generation = parameters[-1]
        parameters[-1] = (
            generation.replace(kind=Parameter.POSITIONAL_OR_KEYWORD)
            if invalid == "positional"
            else generation.replace(default=False)
            if invalid == "wrong-default"
            else generation.replace(default=Parameter.empty)
        )
    _replace_signature(
        native.AudioEngine,
        "begin_source_beatmap_create_async",
        complete.replace(parameters=parameters),
    )
    with pytest.raises(AssertionError, match="begin_source_beatmap_create_async"):
        _check(native)


@pytest.mark.parametrize(
    ("method", "argument", "bad_default"),
    [("editor_view", "max_points", 2048.0), ("source_after_beats", "require_four_four", False)],
)
def test_projection_bound_and_bar_meter_defaults_must_match_native_contract(
    method: str, argument: str, bad_default: object
) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(getattr(native.SourceBeatmap, method))
    parameters = [
        parameter.replace(default=bad_default) if parameter.name == argument else parameter
        for parameter in complete.parameters.values()
    ]
    _replace_signature(native.SourceBeatmap, method, complete.replace(parameters=parameters))
    with pytest.raises(AssertionError, match=argument):
        _check(native)


def test_edit_requires_exact_receipt_and_operation_arguments_without_replacement_geometry() -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.AudioEngine.begin_source_beatmap_edit_async)
    parameters = list(complete.parameters.values())
    parameters[-1] = parameters[-1].replace(name="record_json")
    _replace_signature(
        native.AudioEngine,
        "begin_source_beatmap_edit_async",
        complete.replace(parameters=parameters),
    )
    with pytest.raises(AssertionError, match="begin_source_beatmap_edit_async"):
        _check(native)
