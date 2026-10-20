"""Reflected Mix Capture compatibility using explicit nonexecuting Native doubles."""

import runpy
from inspect import Parameter, Signature, signature
from pathlib import Path
from types import SimpleNamespace
from typing import Never

import pytest

REPO = Path(__file__).resolve().parents[2]
_ENGINE_METHODS = (
    "audio_stream_failed",
    "quiesce_failed_audio_stream",
    "prepare_mix_capture",
    "start_mix_capture",
    "stop_mix_capture",
    "cancel_mix_capture",
    "mix_capture_status",
)
_PUBLIC_METHODS = (
    ("MixCaptureTicket", "session_id"),
    *(("AudioEngine", method) for method in _ENGINE_METHODS),
    (None, "inspect_mix_capture_spool"),
)


def _forbidden(*_args: object, **_kwargs: object) -> Never:
    pytest.fail("Mix Capture preflight must not construct or execute Native/capture/file work")


def _session_id(self: object) -> Never:
    _forbidden()


def _stream_lifecycle(self: object) -> Never:
    _forbidden()


def _prepare(
    self: object,
    spool_path: str,
    block_frames: int = 4096,
    block_count: int = 32,
    max_pool_pcm_bytes: int = 8388608,
) -> Never:
    _forbidden()


def _target(self: object, ticket: object, target_frame: int | None = None) -> Never:
    _forbidden()


def _ticket(self: object, ticket: object) -> Never:
    _forbidden()


def _inspect_spool(path: str) -> Never:
    _forbidden()


def mix_capture_native_module(engine_base: type = object) -> SimpleNamespace:
    """Provide a fresh reflected family; it proves no installed Native behavior."""
    return SimpleNamespace(
        AudioEngine=type(
            "MixCaptureEngine",
            (engine_base,),
            {
                "__init__": _forbidden,
                "audio_stream_failed": _stream_lifecycle,
                "quiesce_failed_audio_stream": _stream_lifecycle,
                "prepare_mix_capture": _prepare,
                "start_mix_capture": _target,
                "stop_mix_capture": _target,
                "cancel_mix_capture": _ticket,
                "mix_capture_status": _ticket,
            },
        ),
        MixCaptureTicket=type(
            "MixCaptureTicket", (), {"__init__": _forbidden, "session_id": _session_id}
        ),
        inspect_mix_capture_spool=_inspect_spool,
    )


def _check(native: object) -> None:
    runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_mix_capture_api"](native)


def _owner(native: SimpleNamespace, name: str | None) -> object:
    return getattr(native, name) if name is not None else native


def _replace_signature(owner: object, method: str, replacement: Signature) -> None:
    implementation = type(
        "NonexecutingCaptureMethod",
        (),
        {"__call__": _forbidden, "__signature__": replacement},
    )()
    setattr(owner, method, implementation)


def test_complete_mix_capture_family_is_only_reflected_without_native_work() -> None:
    _check(mix_capture_native_module())


@pytest.mark.parametrize("name", ["AudioEngine", "MixCaptureTicket"])
@pytest.mark.parametrize("invalid", ["absent", "none", "instance"])
def test_missing_or_non_type_capture_owner_cannot_activate(name: str, invalid: str) -> None:
    native = mix_capture_native_module()
    if invalid == "absent":
        delattr(native, name)
    else:
        setattr(native, name, None if invalid == "none" else object())
    with pytest.raises(AssertionError, match=name):
        _check(native)


@pytest.mark.parametrize(("name", "method"), _PUBLIC_METHODS)
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_missing_capture_operation_or_inspector_cannot_activate(
    name: str | None, method: str, *, missing: bool
) -> None:
    native = mix_capture_native_module()
    owner = _owner(native, name)
    if missing:
        delattr(owner, method)
    else:
        setattr(owner, method, None)
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize(("name", "method"), _PUBLIC_METHODS)
def test_extra_capture_arguments_cannot_activate(name: str | None, method: str) -> None:
    native = mix_capture_native_module()
    owner = _owner(native, name)
    complete = signature(getattr(owner, method))
    extra = Parameter("replacement_owner", Parameter.KEYWORD_ONLY, default=None)
    _replace_signature(
        owner, method, complete.replace(parameters=[*complete.parameters.values(), extra])
    )
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize(
    ("name", "method", "argument"),
    [
        ("AudioEngine", "prepare_mix_capture", "spool_path"),
        ("AudioEngine", "start_mix_capture", "target_frame"),
        ("AudioEngine", "stop_mix_capture", "target_frame"),
        ("AudioEngine", "cancel_mix_capture", "ticket"),
        ("AudioEngine", "mix_capture_status", "ticket"),
        (None, "inspect_mix_capture_spool", "path"),
    ],
)
def test_missing_original_capture_argument_cannot_activate(
    name: str | None, method: str, argument: str
) -> None:
    native = mix_capture_native_module()
    owner = _owner(native, name)
    complete = signature(getattr(owner, method))
    parameters = [p for p in complete.parameters.values() if p.name != argument]
    _replace_signature(owner, method, complete.replace(parameters=parameters))
    with pytest.raises(AssertionError, match=method):
        _check(native)


@pytest.mark.parametrize("argument", ["block_frames", "block_count", "max_pool_pcm_bytes"])
@pytest.mark.parametrize("invalid", ["required", "wrong-value", "wrong-type"])
def test_incompatible_capture_pool_default_cannot_activate(argument: str, invalid: str) -> None:
    native = mix_capture_native_module()
    complete = signature(native.AudioEngine.prepare_mix_capture)
    parameters = list(complete.parameters.values())
    index = next(i for i, parameter in enumerate(parameters) if parameter.name == argument)
    original = parameters[index].default
    default = (
        Parameter.empty
        if invalid == "required"
        else float(original)
        if invalid == "wrong-type"
        else 1
    )
    # A required parameter after a default must be keyword-only to form a valid
    # inspect.Signature; its incompatible position/default must still be rejected.
    if invalid == "required":
        parameters[index:] = [p.replace(kind=Parameter.KEYWORD_ONLY) for p in parameters[index:]]
    parameters[index] = parameters[index].replace(default=default)
    _replace_signature(
        native.AudioEngine, "prepare_mix_capture", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="prepare_mix_capture"):
        _check(native)


@pytest.mark.parametrize(
    ("name", "method", "argument"),
    [
        ("MixCaptureTicket", "session_id", "self"),
        ("AudioEngine", "prepare_mix_capture", "spool_path"),
        ("AudioEngine", "cancel_mix_capture", "ticket"),
        ("AudioEngine", "mix_capture_status", "ticket"),
        (None, "inspect_mix_capture_spool", "path"),
    ],
)
def test_optional_original_capture_arguments_cannot_activate(
    name: str | None, method: str, argument: str
) -> None:
    native = mix_capture_native_module()
    owner = _owner(native, name)
    complete = signature(getattr(owner, method))
    parameters = [
        p.replace(default=None) if p.name == argument else p for p in complete.parameters.values()
    ]
    _replace_signature(owner, method, complete.replace(parameters=parameters))
    with pytest.raises(AssertionError, match=argument):
        _check(native)


@pytest.mark.parametrize("method", ["start_mix_capture", "stop_mix_capture"])
@pytest.mark.parametrize("default", [Parameter.empty, 0, False, "next"])
def test_capture_target_requires_native_next_admissible_default(
    method: str, default: object
) -> None:
    native = mix_capture_native_module()
    complete = signature(getattr(native.AudioEngine, method))
    parameters = [
        p.replace(default=default) if p.name == "target_frame" else p
        for p in complete.parameters.values()
    ]
    _replace_signature(native.AudioEngine, method, complete.replace(parameters=parameters))
    with pytest.raises(AssertionError, match="target_frame"):
        _check(native)


@pytest.mark.parametrize("kind", [Parameter.POSITIONAL_ONLY, Parameter.KEYWORD_ONLY])
def test_capture_target_must_retain_positional_and_keyword_calling_contract(
    kind: object,
) -> None:
    native = mix_capture_native_module()
    complete = signature(native.AudioEngine.start_mix_capture)
    parameters = list(complete.parameters.values())
    if kind == Parameter.POSITIONAL_ONLY:
        parameters = [p.replace(kind=Parameter.POSITIONAL_ONLY) for p in parameters]
    else:
        parameters[-1] = parameters[-1].replace(kind=Parameter.KEYWORD_ONLY)
    _replace_signature(
        native.AudioEngine, "start_mix_capture", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="start_mix_capture"):
        _check(native)


def test_reordered_capture_ticket_and_target_cannot_activate() -> None:
    native = mix_capture_native_module()
    complete = signature(native.AudioEngine.stop_mix_capture)
    parameters = list(complete.parameters.values())
    parameters[1:] = [p.replace(kind=Parameter.KEYWORD_ONLY) for p in parameters[1:]]
    parameters[1], parameters[2] = parameters[2], parameters[1]
    _replace_signature(
        native.AudioEngine, "stop_mix_capture", complete.replace(parameters=parameters)
    )
    with pytest.raises(AssertionError, match="stop_mix_capture"):
        _check(native)
