import os
import subprocess
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.controller.stem_generation import (
    FFMPEG_DIR_ENV_VAR,
    MODEL_CACHE_DIR_ENV_VAR,
    default_stem_model_cache_dir,
    stem_model_environment,
)

if TYPE_CHECKING:
    from pathlib import Path


def test_default_stem_model_cache_uses_clone_models_checkpoint_dir(tmp_path: Path) -> None:
    project_root = tmp_path / "repo"

    cache_dir = default_stem_model_cache_dir(
        env={"LOCALAPPDATA": str(tmp_path / "LocalAppData")},
        project_root=project_root,
    )

    assert cache_dir == project_root / "models/stem-checkpoints"


@pytest.mark.parametrize(
    "relative", ["samples/models", "record/models", "config/models", ".git", "src"]
)
def test_model_override_rejects_protected_repository_paths(tmp_path: Path, relative: str) -> None:
    project_root = tmp_path / "repo"
    project_root.mkdir()

    with pytest.raises(ValueError, match="owned models directory"):
        default_stem_model_cache_dir(
            env={MODEL_CACHE_DIR_ENV_VAR: str(project_root / relative)}, project_root=project_root
        )


def test_model_override_preserves_owned_clone_models_and_explicit_external_cache(
    tmp_path: Path,
) -> None:
    project = tmp_path / "repo"
    for cache in (project / "models/custom", tmp_path / "dev-cache/models/checkpoints"):
        assert (
            default_stem_model_cache_dir(
                env={MODEL_CACHE_DIR_ENV_VAR: str(cache)}, project_root=project
            )
            == cache
        )


@pytest.mark.parametrize("aliased", ["models", "record"])
def test_model_alias_cannot_overlap_models_and_recordings(tmp_path: Path, aliased: str) -> None:
    project = tmp_path / "repo"
    target = project / ("record" if aliased == "models" else "models")
    target.mkdir(parents=True)
    alias = project / aliased
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
    try:
        for environment in ({}, {MODEL_CACHE_DIR_ENV_VAR: str(alias / "checkpoints")}):
            with pytest.raises(ValueError, match="owned models directory"):
                default_stem_model_cache_dir(env=environment, project_root=project)
    finally:
        alias.rmdir() if os.name == "nt" else alias.unlink()


@pytest.mark.parametrize("project_directory", ["data", "flitzis-looper"])
def test_explicit_portable_model_cache_survives_runtime_version_change(
    tmp_path: Path, project_directory: str
) -> None:
    cache = tmp_path / "portable" / "models" / "stem-checkpoints"
    assert (
        default_stem_model_cache_dir(
            env={MODEL_CACHE_DIR_ENV_VAR: str(cache)},
            project_root=tmp_path / "portable" / project_directory,
        )
        == cache
    )


def test_stem_model_environment_adds_explicit_ffmpeg_dir_to_path(tmp_path: Path) -> None:
    cache_dir = tmp_path / ".cache" / "stem-checkpoints"
    ffmpeg_root = tmp_path / "tools" / "ffmpeg"
    ffmpeg_bin = ffmpeg_root / "bin"
    _write_ffmpeg_tools(ffmpeg_bin)

    env = stem_model_environment(
        cache_dir,
        base_env={
            FFMPEG_DIR_ENV_VAR: str(ffmpeg_root),
            "PATH": "existing",
        },
    )

    assert env["PATH"].split(os.pathsep)[0] == str(ffmpeg_bin.resolve(strict=False))


def test_stem_model_environment_falls_back_to_winget_ffmpeg_package(
    tmp_path: Path,
) -> None:
    cache_dir = tmp_path / ".cache" / "stem-checkpoints"
    local_appdata = tmp_path / "LocalAppData"
    ffmpeg_bin = (
        local_appdata
        / "Microsoft"
        / "WinGet"
        / "Packages"
        / "Gyan.FFmpeg.Shared_Microsoft.Winget.Source_8wekyb3d8bbwe"
        / "ffmpeg-8.1.1-full_build-shared"
        / "bin"
    )
    _write_ffmpeg_tools(ffmpeg_bin)

    env = stem_model_environment(
        cache_dir,
        base_env={
            "LOCALAPPDATA": str(local_appdata),
            "PATH": "",
        },
    )

    assert env["PATH"] == str(ffmpeg_bin.resolve(strict=False))


def _write_ffmpeg_tools(directory: Path) -> None:
    directory.mkdir(parents=True)
    suffix = ".exe" if os.name == "nt" else ""
    (directory / f"ffmpeg{suffix}").write_bytes(b"placeholder")
    (directory / f"ffprobe{suffix}").write_bytes(b"placeholder")
