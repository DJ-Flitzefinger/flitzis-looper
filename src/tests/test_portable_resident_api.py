"""Reject stale Native interfaces before a portable runtime can activate."""

import runpy
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

REPO = Path(__file__).resolve().parents[2]


class _CompatibleEngine:
    RESIDENT_PCM_ADMISSION_VERSION = 1

    def __init__(self) -> None:
        pytest.fail("Preflight must not construct an engine or open a device")

    def set_resident_pcm_budget_mib(self, value: int) -> None:
        pass

    def get_resident_pcm_budget_status(self) -> dict[str, int]:
        return {}

    def request_committed_stem_residency(
        self, ticket: object, *, interest: str = "explicit"
    ) -> None:
        pass


def test_compatible_class_is_checked_without_an_engine() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_resident_api"]
    check(_CompatibleEngine)


@pytest.mark.parametrize(
    ("attribute", "value", "message"),
    [
        ("RESIDENT_PCM_ADMISSION_VERSION", 0, "protocol missing"),
        ("set_resident_pcm_budget_mib", None, "set_resident_pcm_budget_mib"),
        ("get_resident_pcm_budget_status", None, "get_resident_pcm_budget_status"),
        ("request_committed_stem_residency", lambda self, ticket: None, "preload interest"),
    ],
)
def test_incompatible_native_is_rejected_before_activation(
    attribute: str,
    value: object,
    message: str,
) -> None:
    engine_type = type("IncompatibleEngine", (_CompatibleEngine,), {attribute: value})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_resident_api"]
    with pytest.raises(AssertionError, match=message):
        check(engine_type)


class _CompatibleCopyEngine(_CompatibleEngine):
    def capture_pad_copy(self, binding: object, target_id: int, intent_json: str) -> object:
        raise NotImplementedError

    def prepare_pad_copy(self, captured: object) -> object:
        raise NotImplementedError

    def publish_pad_copy(self, ticket: object) -> None:
        raise NotImplementedError

    def reconcile_pad_copy(self, ticket: object) -> bool:
        raise NotImplementedError

    def finish_pad_copy(self, ticket: object) -> None:
        raise NotImplementedError

    def cancel_pad_copy(self, captured: object) -> bool:
        raise NotImplementedError


def test_complete_copy_interface_is_checked_without_an_engine() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    check(_CompatibleCopyEngine)


@pytest.mark.parametrize(
    "method",
    [
        "capture_pad_copy",
        "prepare_pad_copy",
        "publish_pad_copy",
        "reconcile_pad_copy",
        "finish_pad_copy",
        "cancel_pad_copy",
    ],
)
def test_incomplete_copy_interface_cannot_activate(method: str) -> None:
    engine_type = type("IncompatibleCopyEngine", (_CompatibleCopyEngine,), {method: None})
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    with pytest.raises(AssertionError, match=method):
        check(engine_type)


def test_old_residency_interface_alone_cannot_activate_copy_controls() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_copy_api"]
    with pytest.raises(AssertionError, match="capture_pad_copy"):
        check(_CompatibleEngine)


_PAIR_HOLD_METHODS = (
    "_capture_pad_pair",
    "_prepare_pad_pair",
    "_publish_pad_pair",
    "_reconcile_pad_pair",
    "_validate_pad_pair_acknowledgement",
    "_finish_pad_pair",
    "_cancel_pad_pair",
    "capture_recovered_pad_pair",
    "validate_recovered_pad_pair",
    "release_recovered_pad_pair",
    "pause_waveform_hold",
    "release_waveform_hold",
    "trigger_captured_input_pad",
    "prepare_captured_input_trigger",
    "trigger_prepared_input_pad",
    "_resolve_pad_feedback",
    "_resolve_loader_event",
)


def _forbidden_native_work(*_arguments: object, **_keywords: object) -> None:
    pytest.fail("Preflight must reflect the Native class without executing Native work")


_CompatiblePairHoldEngine = type(
    "CompatiblePairHoldEngine",
    (_CompatibleCopyEngine,),
    dict.fromkeys(_PAIR_HOLD_METHODS, _forbidden_native_work),
)


def test_complete_pair_hold_input_interface_is_reflected_without_native_work() -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    for name in ("check_resident_api", "check_pad_copy_api", "check_pad_pair_hold_api"):
        bootstrap[name](_CompatiblePairHoldEngine)


@pytest.mark.parametrize("method", _PAIR_HOLD_METHODS)
@pytest.mark.parametrize("missing", [False, True], ids=["not-callable", "absent"])
def test_incomplete_pair_hold_input_interface_cannot_activate(
    method: str, *, missing: bool
) -> None:
    methods: dict[str, object] = dict.fromkeys(_PAIR_HOLD_METHODS, _forbidden_native_work)
    if missing:
        del methods[method]
    else:
        methods[method] = None
    engine_type = type("IncompatiblePairHoldEngine", (_CompatibleCopyEngine,), methods)
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_pair_hold_api"]
    with pytest.raises(AssertionError, match=method):
        check(engine_type)


def test_copy_interface_alone_cannot_activate_pair_hold_controls() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))["check_pad_pair_hold_api"]
    with pytest.raises(AssertionError, match="_capture_pad_pair"):
        check(_CompatibleCopyEngine)


def test_old_native_is_rejected_before_any_app_or_device_activation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    checkout = tmp_path / "flitzis-looper"
    native = SimpleNamespace(
        flitzis_looper_audio=SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper_audio/native.pyd")
        ),
        native_build_profile=lambda: "release",
        AudioEngine=_CompatibleCopyEngine,
    )
    imported = Mock(return_value=native)
    monkeypatch.setattr(bootstrap["importlib"], "import_module", imported)
    with pytest.raises(AssertionError, match="_capture_pad_pair"):
        bootstrap["check_runtime"](tmp_path, checkout, tmp_path / "Runtime")
    imported.assert_called_once_with("flitzis_looper_audio")


def test_runtime_preflight_accepts_complete_reflected_native_before_activation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    checkout = tmp_path / "flitzis-looper"
    runtime = tmp_path / "Runtime"
    (checkout / "assets/models").mkdir(parents=True)
    (checkout / "assets/models/keynet.onnx").write_bytes(b"preflight path fixture")
    monkeypatch.chdir(checkout)
    monkeypatch.setattr(bootstrap["sys"], "executable", str(runtime / "python/python.exe"))
    native = SimpleNamespace(
        flitzis_looper_audio=SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper_audio/native.pyd")
        ),
        native_build_profile=lambda: "release",
        AudioEngine=_CompatiblePairHoldEngine,
    )
    modules: dict[str, object] = {
        "flitzis_looper_audio": native,
        "sitecustomize": SimpleNamespace(__file__=str(runtime / "python/sitecustomize.py")),
        "flitzis_looper.controller.stems": SimpleNamespace(StemController=object()),
        "flitzis_looper.ui.render.sidebar_left": object(),
        "flitzis_looper.controller.stem_generation": SimpleNamespace(
            default_demucs_model_cache_dir=lambda: tmp_path / "models/torch/hub/checkpoints"
        ),
        "flitzis_looper": SimpleNamespace(
            __file__=str(checkout / "src/flitzis_looper/__init__.py")
        ),
    }
    for name in ("imgui_bundle", "torch", "torchaudio", "torchcodec"):
        modules[name] = SimpleNamespace(__file__=str(runtime / f"python/{name}/__init__.py"))
    monkeypatch.setattr(bootstrap["importlib"], "import_module", modules.__getitem__)
    monkeypatch.setitem(bootstrap["check_runtime"].__globals__, "private_dlls", lambda *_: {})
    runner = Mock()
    monkeypatch.setattr(bootstrap["subprocess"], "run", runner)
    bootstrap["check_runtime"](tmp_path, checkout, runtime)
    assert runner.call_count == 3
    assert '"portable_preflight": "passed"' in capsys.readouterr().out
