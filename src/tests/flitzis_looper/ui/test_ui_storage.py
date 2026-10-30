"""UI state paths stay with their owner without starting a GUI or audio device."""

import os
import subprocess
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest
from imgui_bundle import hello_imgui

from flitzis_looper.ui import run

if TYPE_CHECKING:
    from imgui_bundle.hello_imgui import RunnerParams


@pytest.mark.parametrize("external", [False, True])
def test_ui_directory_uses_clone_cache_or_explicit_developer_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, external: bool
) -> None:
    checkout = tmp_path / "clone"
    checkout.mkdir()
    monkeypatch.chdir(checkout)
    expected = tmp_path / "developer-cache/ui" if external else checkout / ".cache/ui"
    if external:
        monkeypatch.setenv("FLITZIS_UI_STATE_DIR", str(expected))
    else:
        monkeypatch.delenv("FLITZIS_UI_STATE_DIR", raising=False)
    assert run.prepare_ui_state_directory() == expected
    assert expected.is_dir()
    assert Path.cwd() == checkout


@pytest.mark.parametrize(
    "relative", ["samples/ui", "record/ui", "src/ui", "models/ui", "config/ui"]
)
def test_ui_directory_rejects_protected_paths_before_creating_them(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, relative: str
) -> None:
    checkout = tmp_path / "clone"
    checkout.mkdir()
    monkeypatch.chdir(checkout)
    target = checkout / relative
    monkeypatch.setenv("FLITZIS_UI_STATE_DIR", str(target))
    with pytest.raises(ValueError, match="protected project data"):
        run.prepare_ui_state_directory()
    assert not target.exists()


@pytest.mark.parametrize("reverse", [False, True])
def test_ui_directory_rejects_aliases_into_or_from_recordings(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, reverse: bool
) -> None:
    checkout = tmp_path / "clone"
    target = checkout / (".cache/ui" if reverse else "record")
    target.mkdir(parents=True)
    sentinel = target / "original.wav"
    sentinel.write_bytes(b"preserve unknown original")
    alias = checkout / ("record" if reverse else ".cache")
    if os.name == "nt":
        result = subprocess.run(
            ["cmd.exe", "/d", "/c", "mklink", "/J", str(alias), str(target)],
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stdout + result.stderr
    else:
        alias.symlink_to(target, target_is_directory=True)
    monkeypatch.chdir(checkout)
    monkeypatch.delenv("FLITZIS_UI_STATE_DIR", raising=False)
    try:
        with pytest.raises(ValueError, match=r"links|protected project data"):
            run.prepare_ui_state_directory()
        assert sentinel.read_bytes() == b"preserve unknown original"
        assert list(target.iterdir()) == [sentinel]
    finally:
        alias.rmdir() if os.name == "nt" else alias.unlink()


def test_invalid_ini_leaf_is_rejected_before_default_controller_construction(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    checkout = tmp_path / "clone"
    checkout.joinpath(".cache/ui/imgui.ini").mkdir(parents=True)
    monkeypatch.chdir(checkout)
    monkeypatch.delenv("FLITZIS_UI_STATE_DIR", raising=False)
    controller = Mock()
    monkeypatch.setattr(run, "AppController", controller)
    with pytest.raises(ValueError, match="ordinary file"):
        run.run_ui()
    controller.assert_not_called()


def test_actual_runner_uses_absolute_owned_ini_before_default_controller(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    checkout = tmp_path / "clone"
    checkout.mkdir()
    expected = checkout / ".cache/ui"
    monkeypatch.chdir(checkout)
    monkeypatch.delenv("FLITZIS_UI_STATE_DIR", raising=False)
    controller = Mock()

    def construct() -> Mock:
        assert expected.is_dir()
        return controller

    factory = Mock(side_effect=construct)
    loop = Mock()
    monkeypatch.setattr(run, "AppController", factory)
    monkeypatch.setattr(run, "UiContext", Mock())
    monkeypatch.setattr(run, "_run_ui_loop", loop)
    run.run_ui()
    factory.assert_called_once_with()
    params = cast("RunnerParams", loop.call_args.args[1])
    assert params.ini_folder_type == hello_imgui.IniFolderType.absolute_path
    assert params.ini_filename == str(expected / "imgui.ini")
    assert params.ini_filename_use_app_window_title is False
    assert params.app_window_params.restore_previous_geometry is True
    assert Path.cwd() == checkout
