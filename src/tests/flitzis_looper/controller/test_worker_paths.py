"""Worker path binding needs no Engine, model acquisition or UI construction."""

from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import AppController

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("explicit", [False, True])
def test_app_worker_uses_clone_models_or_retains_explicit_cache_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, explicit: bool
) -> None:
    checkout = tmp_path / "clone"
    checkout.mkdir()
    monkeypatch.chdir(checkout)
    loader, assets = Mock(), object()
    controller = cast("AppController", SimpleNamespace(loader=loader, _assets=assets))
    override = tmp_path / "project-cache/analysis/beat-this" if explicit else None
    expected = override if override is not None else checkout / "models/beat-this"

    AppController._bind_loader_assets(controller, beat_this_installation_root=override)

    loader.bind_asset_lifecycle.assert_called_once_with(
        assets, beat_this_installation_root=expected
    )
    assert controller._beat_this_installation_root == expected
    assert not expected.exists()
    monkeypatch.chdir(tmp_path)
    loader.reset_mock()
    AppController._bind_loader_assets(controller)
    loader.bind_asset_lifecycle.assert_called_once_with(
        assets, beat_this_installation_root=expected
    )
