"""Exercise the owned completion wrapper with a declared non-neural apply substitute."""

from importlib import import_module
from types import SimpleNamespace

import pytest

from flitzis_looper import demucs_streaming
from flitzis_looper.stem_cancellation import CANCEL_PATH_ENV
from flitzis_looper.stem_execution import STEM_EXECUTION_RESULT, completed_execution


@pytest.mark.parametrize("failure", [False, True])
def test_owned_demucs_completion_requires_successful_explicit_recursive_apply(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str], *, failure: bool
) -> None:
    # No model/source/device inference runs. Only original_apply is substituted;
    # main, the scoped wrappers, keyword forwarding and receipt producer are real.
    apply = import_module("demucs.apply")
    separate = demucs_streaming._separate
    calls = []
    monkeypatch.delenv(CANCEL_PATH_ENV, raising=False)
    monkeypatch.setattr(separate, "get_model_from_args", lambda _args: object())

    def original_apply(model: object, mix: object, *, device: str, split: bool) -> object:
        calls.append(device)
        if split:
            return apply.apply_model(model, mix, device=device, split=False)
        if failure:
            message = "declared apply failure"
            raise RuntimeError(message)
        return SimpleNamespace(device="cpu")

    monkeypatch.setattr(apply, "apply_model", original_apply)

    def cli() -> None:
        model = separate.get_model_from_args(SimpleNamespace(name="htdemucs"))
        separate.apply_model(model, object(), device="cuda:0", split=True)

    monkeypatch.setattr(separate, "main", cli)
    if failure:
        with pytest.raises(RuntimeError, match="declared apply failure"):
            demucs_streaming.main()
        assert STEM_EXECUTION_RESULT not in capsys.readouterr().out
    else:
        demucs_streaming.main()
        receipt = completed_execution(
            capsys.readouterr().out,
            backend="demucs",
            model_name="htdemucs",
            device="cuda",
            cpu_fallback=False,
        )
        assert receipt is not None
        assert receipt.device == "cuda"
    assert calls == ["cuda:0", "cuda:0"]
    assert apply.apply_model is original_apply
