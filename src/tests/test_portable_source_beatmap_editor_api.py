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


def _raw(self: object, sample_id: int, *, expected_source_generation: int) -> Never:
    _forbidden()


def _beat_this_raw(
    self: object, sample_id: int, installation_root: str, *, expected_source_generation: int
) -> Never:
    _forbidden()


def _review(
    self: object,
    sample_id: int,
    candidate: object,
    review_json: str,
    *,
    expected_source_generation: int,
) -> Never:
    _forbidden()


def _raw_completion(self: object, delivery: object) -> Never:
    _forbidden()


def _raw_inspect(self: object, start_index: int, max_events: int = 128) -> Never:
    _forbidden()


def _info(self: object) -> Never:
    _forbidden()


def _runtime(
    self: object,
    multi_loop: object,
    loaded: list[bool],
    loop_starts: list[float],
    loop_ends: list[float | None],
    bindings: object = None,
    pitch_rows: object = None,
    source_map_rows: object = None,
    *,
    timing_policy_json: str | None = None,
) -> Never:
    _forbidden()


def _span(self: object, first_anchor_index: int, last_anchor_index: int) -> Never:
    _forbidden()


def _review_page(
    self: object,
    first_anchor_index: int,
    last_anchor_index: int,
    start_index: int,
    max_events: int = 128,
) -> Never:
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


class _SyncStatus:
    # Preserve the Native keyword spelling without declaring a shadowing Python argument.
    __signature__ = Signature([
        Parameter("self", Parameter.POSITIONAL_OR_KEYWORD),
        Parameter("id", Parameter.POSITIONAL_OR_KEYWORD),
    ])

    def __call__(self, *_args: object, **_kwargs: object) -> Never:
        _forbidden()


def source_beatmap_editor_native_module(engine_base: type = object) -> SimpleNamespace:
    """Return fresh nonexecuting capabilities for direct and full-runtime preflight."""
    return SimpleNamespace(
        AudioEngine=type(
            "EditorEngine",
            (engine_base,),
            {
                "__init__": _forbidden,
                "GLOBAL_TIMING_POLICY_API_VERSION": 1,
                "set_input_runtime_state": _runtime,
                "begin_source_beatmap_create_async": _create,
                "begin_source_beatmap_edit_async": _edit,
                "begin_source_beatmap_raw_async": _raw,
                "begin_source_beatmap_beat_this_raw_async": _beat_this_raw,
                "begin_source_beatmap_review_async": _review,
                "pad_source_sync_status": _SyncStatus(),
            },
        ),
        OfflineAnalysisJob=type(
            "OfflineAnalysisJob",
            (),
            {
                "__init__": _forbidden,
                "resolve_source_beatmap_raw_completion": _raw_completion,
                "source_beatmap_async_status": _info,
            },
        ),
        SourceBeatmapRawCandidate=type(
            "SourceBeatmapRawCandidate",
            (),
            {
                "__init__": _forbidden,
                "RAW_REVIEW_API_VERSION": 2,
                "info": _info,
                "inspect": _raw_inspect,
                "inspect_downbeats": _raw_inspect,
            },
        ),
        SourceBeatmap=type(
            "SourceBeatmap",
            (),
            {
                "__init__": _forbidden,
                "EDITOR_CORRECTION_API_VERSION": 4,
                "editor_info": _info,
                "editor_review_info": _info,
                "editor_review_page": _review_page,
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
            return 4

    native = source_beatmap_editor_native_module()
    native.SourceBeatmap.EDITOR_CORRECTION_API_VERSION = CorrectionCapability()
    _check(native)


@pytest.mark.parametrize(
    ("owner_name", "method"),
    [
        ("AudioEngine", "begin_source_beatmap_create_async"),
        ("AudioEngine", "begin_source_beatmap_edit_async"),
        ("AudioEngine", "pad_source_sync_status"),
        ("AudioEngine", "begin_source_beatmap_raw_async"),
        ("AudioEngine", "begin_source_beatmap_beat_this_raw_async"),
        ("AudioEngine", "begin_source_beatmap_review_async"),
        ("OfflineAnalysisJob", "resolve_source_beatmap_raw_completion"),
        ("OfflineAnalysisJob", "source_beatmap_async_status"),
        ("SourceBeatmapRawCandidate", "info"),
        ("SourceBeatmapRawCandidate", "inspect"),
        ("SourceBeatmapRawCandidate", "inspect_downbeats"),
        ("SourceBeatmap", "editor_info"),
        ("SourceBeatmap", "editor_review_info"),
        ("SourceBeatmap", "editor_review_page"),
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


@pytest.mark.parametrize("invalid", [None, False, True, 1.0, "1", 0, 2])
def test_global_policy_requires_exact_integer_capability(invalid: object) -> None:
    native = source_beatmap_editor_native_module()
    native.AudioEngine.GLOBAL_TIMING_POLICY_API_VERSION = invalid
    with pytest.raises(AssertionError, match="GLOBAL_TIMING_POLICY_API_VERSION"):
        _check(native)


@pytest.mark.parametrize("invalid", ["missing", "positional", "required", "default-false"])
def test_global_policy_requires_optional_keyword_only_publication(invalid: str) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.AudioEngine.set_input_runtime_state)
    parameters = list(complete.parameters.values())
    parameter = parameters.pop()
    if invalid == "positional":
        parameters.append(parameter.replace(kind=Parameter.POSITIONAL_OR_KEYWORD))
    elif invalid == "required":
        parameters.append(parameter.replace(default=Parameter.empty))
    elif invalid == "default-false":
        parameters.append(parameter.replace(default=False))
    _replace_signature(
        native.AudioEngine, "set_input_runtime_state", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="timing_policy_json"):
        _check(native)


@pytest.mark.parametrize("invalid", [None, False, True, 4.0, "4", 0, 1, 2, 3, 5])
def test_correction_version_requires_exact_current_integer(invalid: object) -> None:
    native = source_beatmap_editor_native_module()
    native.SourceBeatmap.EDITOR_CORRECTION_API_VERSION = invalid
    with pytest.raises(AssertionError, match="EDITOR_CORRECTION_API_VERSION"):
        _check(native)


@pytest.mark.parametrize("invalid", [None, False, True, 2.0, "2", 0, 1, 3])
def test_backend_aware_raw_version_requires_exact_current_integer(invalid: object) -> None:
    native = source_beatmap_editor_native_module()
    native.SourceBeatmapRawCandidate.RAW_REVIEW_API_VERSION = invalid
    with pytest.raises(AssertionError, match="RAW_REVIEW_API_VERSION"):
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


@pytest.mark.parametrize(
    "invalid",
    ["missing-start", "renamed-first", "optional-last", "keyword-only", "positional-only", "limit"],
)
def test_review_page_requires_exact_bounded_signature(invalid: str) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.SourceBeatmap.editor_review_page)
    parameters = list(complete.parameters.values())
    if invalid == "missing-start":
        parameters.pop(3)
    elif invalid == "renamed-first":
        parameters[1] = parameters[1].replace(name="first_index")
    elif invalid == "optional-last":
        parameters[3] = parameters[3].replace(default=0)
    elif invalid == "keyword-only":
        parameters[-1] = parameters[-1].replace(kind=Parameter.KEYWORD_ONLY)
    elif invalid == "positional-only":
        parameters = [parameter.replace(kind=Parameter.POSITIONAL_ONLY) for parameter in parameters]
    else:
        parameters[-1] = parameters[-1].replace(default=64)
    _replace_signature(
        native.SourceBeatmap, "editor_review_page", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="editor_review_page"):
        _check(native)


def test_review_info_accepts_no_client_geometry_or_optional_arguments() -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.SourceBeatmap.editor_review_info)
    parameters = [*complete.parameters.values(), Parameter("rows", Parameter.POSITIONAL_OR_KEYWORD)]
    _replace_signature(
        native.SourceBeatmap, "editor_review_info", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="editor_review_info"):
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


@pytest.mark.parametrize("invalid", ["missing-id", "optional-id", "positional-id"])
def test_sync_feedback_requires_a_current_pad_query_without_executing_it(invalid: str) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.AudioEngine.pad_source_sync_status)
    parameters = list(complete.parameters.values())
    if invalid == "missing-id":
        parameters.pop()
    elif invalid == "optional-id":
        parameters[-1] = parameters[-1].replace(default=0)
    else:
        parameters = [parameter.replace(kind=Parameter.POSITIONAL_ONLY) for parameter in parameters]
    _replace_signature(
        native.AudioEngine, "pad_source_sync_status", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="pad_source_sync_status"):
        _check(native)


@pytest.mark.parametrize("owner", ["SourceBeatmapRawCandidate", "OfflineAnalysisJob"])
def test_raw_review_requires_opaque_native_result_and_original_job_types(owner: str) -> None:
    native = source_beatmap_editor_native_module()
    delattr(native, owner)
    with pytest.raises(AssertionError, match=owner):
        _check(native)


@pytest.mark.parametrize(
    "method",
    [
        "begin_source_beatmap_raw_async",
        "begin_source_beatmap_review_async",
        "begin_source_beatmap_beat_this_raw_async",
    ],
)
@pytest.mark.parametrize("invalid", ["missing-generation", "optional-generation", "positional"])
def test_raw_acquisition_and_review_require_exact_current_source_fence(
    method: str, invalid: str
) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(getattr(native.AudioEngine, method))
    parameters = list(complete.parameters.values())
    if invalid == "missing-generation":
        parameters.pop()
    elif invalid == "optional-generation":
        parameters[-1] = parameters[-1].replace(default=None)
    else:
        parameters[-1] = parameters[-1].replace(kind=Parameter.POSITIONAL_OR_KEYWORD)
    _replace_signature(
        native.AudioEngine,
        method,
        complete.replace(parameters=parameters),
    )
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize("argument", ["candidate", "review_json"])
def test_review_requires_original_candidate_and_separate_intent_arguments(argument: str) -> None:
    native = source_beatmap_editor_native_module()
    method = "begin_source_beatmap_review_async"
    complete = signature(getattr(native.AudioEngine, method))
    parameters = [
        parameter.replace(name="record_json") if parameter.name == argument else parameter
        for parameter in complete.parameters.values()
    ]
    _replace_signature(native.AudioEngine, method, complete.replace(parameters=parameters))
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize("default", [127, 129, 128.0, None, Parameter.empty])
def test_raw_inspection_requires_the_bounded_native_page_default(default: object) -> None:
    native = source_beatmap_editor_native_module()
    complete = signature(native.SourceBeatmapRawCandidate.inspect)
    parameters = list(complete.parameters.values())
    parameters[-1] = parameters[-1].replace(default=default)
    _replace_signature(
        native.SourceBeatmapRawCandidate, "inspect", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="max_events"):
        _check(native)
