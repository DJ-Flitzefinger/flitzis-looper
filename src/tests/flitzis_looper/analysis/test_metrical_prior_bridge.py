"""Borrowed request metadata, isolated environments and optional failure ownership."""

import json
import struct
from concurrent.futures import ThreadPoolExecutor
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis import metrical_prior as bridge
from flitzis_looper.analysis.metrical_prior_setup import NUMERIC_PROFILE_SHA256, PriorProfileToken

if TYPE_CHECKING:
    from pathlib import Path


def _request(root: Path) -> dict:
    return {
        "schema_version": 1,
        "identity": {
            "pad_id": 0,
            "request_id": 2**64 - 1,
            "source_id": "original-source",
            "source_generation": 2**63 + 7,
        },
        "installation_root": str(root.parent / ("installation-" + "a" * 32)),
        "profile_sha256": "a" * 64,
        "pcm": {
            "path": str(root / "borrowed.f32le"),
            "source_sha256": "b" * 64,
            "mono_sha256": "c" * 64,
            "sample_rate_hz": 48000,
            "frame_count": 48000,
            "origin_seconds": 0.0,
            "dtype": "float32-le",
            "channels": 1,
            "downmix_revision": "arithmetic-channel-mean-f64-v1",
        },
        "request_directory": str(root),
        "native_live_bytes": 0,
        "aggregate_limit_bytes": 536870912,
    }


class _Cancel:
    cancelled = False

    def is_cancelled(self) -> bool:
        return self.cancelled


def _run(value: dict, cancel: _Cancel) -> dict:
    with ThreadPoolExecutor(max_workers=1) as executor:
        return json.loads(
            executor.submit(bridge.run_metrical_prior, json.dumps(value), cancel).result()
        )


def test_prior_environment_removes_external_preloads_and_mutable_caches(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    for name in (
        "NODE_OPTIONS",
        "NODE_PATH",
        "NODE_COMPILE_CACHE",
        "NODE_V8_COVERAGE",
        "NODE_REPL_HISTORY",
        "PYTHONPATH",
        "PYTHONHOME",
        "NUMBA_CACHE_DIR",
        "LIBROSA_CACHE_DIR",
        "NUMBA_DISABLE_JIT",
    ):
        monkeypatch.setenv(name, "must-not-be-used")
    environment = bridge._environment()
    assert not any(value == "must-not-be-used" for value in environment.values())
    assert environment["NODE_DISABLE_COMPILE_CACHE"] == "1"
    assert environment["NUMBA_NUM_THREADS"] == "1"


def test_known_unfit_parent_refuses_before_profile_scan_or_heavy_worker(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    value = _request(tmp_path)
    value["native_live_bytes"] = 327000000
    verify, command = Mock(), Mock()
    monkeypatch.setattr(bridge, "verify_profile", verify)
    monkeypatch.setattr(bridge, "run_owned_command", command)
    result = _run(value, _Cancel())
    assert result["status"] == "failed"
    assert result["reason"] == "prior_known_frontend_memory_unavailable"
    assert result["resources_released"] is True
    assert "prior" not in result
    verify.assert_not_called()
    command.assert_not_called()


def test_profile_failure_is_bounded_and_cannot_become_a_completed_result(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        bridge, "verify_profile", Mock(side_effect=ValueError("corrupt file/private path"))
    )
    result = _run(_request(tmp_path), _Cancel())
    assert result["status"] == "failed"
    assert result["reason"] == "prior_profile_unavailable"
    assert result["profile"]["installation_root"] == _request(tmp_path)["installation_root"]
    assert "prior" not in result
    assert result["resources_released"] is True


def test_cancel_after_frontend_prevents_classifier_and_retains_original_input(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    value = _request(tmp_path)
    source = tmp_path / "borrowed.f32le"
    source.write_bytes(bytes(value["pcm"]["frame_count"] * 4))
    cancel = _Cancel()
    monkeypatch.setattr(bridge, "verify_profile", Mock(return_value={}))

    def frontend(*_args):
        cancel.cancelled = True
        return {}, tmp_path / "unused-receipt.json", 0

    monkeypatch.setattr(bridge, "_frontend", frontend)
    classifier = Mock()
    monkeypatch.setattr(bridge, "_infer", classifier)
    result = _run(value, cancel)
    assert result["status"] == "cancelled"
    assert "prior" not in result
    assert result["resources_released"] is True
    assert source.is_file()
    classifier.assert_not_called()


def test_node_manifest_keeps_uint64_identity_and_normalization_bits_lossless(
    tmp_path: Path,
) -> None:
    value = _request(tmp_path)
    request = bridge._Request.model_validate_json(json.dumps(value), strict=True)
    token = PriorProfileToken(
        tmp_path / ("installation-" + "a" * 32), "a" * 64, NUMERIC_PROFILE_SHA256
    )
    receipt_path = tmp_path / "frontend.json"
    receipt_path.write_text("{}", encoding="utf-8")
    receipt = {
        "result": {
            "global_normalization": {"mean": -0.0, "std": 0.12345678901234567},
            "features": {
                "path": "features.f32le",
                "shape": [1, 40, 256, 1],
                "sha256": "d" * 64,
                "bytes": 40960,
            },
        }
    }
    path, manifest = bridge._node_manifest(request, token, receipt, receipt_path, tmp_path)
    assert path.is_file()
    assert manifest["identity"]["request_id"] == str(2**64 - 1)
    assert manifest["identity"]["source_generation"] == str(2**63 + 7)
    stats = manifest["cases"][0]["normalization"]
    assert stats["mean_f64_le_hex"] == struct.pack("<d", -0.0).hex()
    assert stats["std_f64_le_hex"] == struct.pack("<d", 0.12345678901234567).hex()
    assert "mean" not in stats
    assert "std" not in stats


@pytest.mark.parametrize(
    ("field", "value"), [("native_live_bytes", True), ("aggregate_limit_bytes", 536870913)]
)
def test_prior_request_rejects_ambiguous_resource_geometry(
    tmp_path: Path, field: str, value: object
) -> None:
    request = _request(tmp_path)
    request[field] = value
    with pytest.raises(ValidationError):
        bridge._Request.model_validate_json(json.dumps(request), strict=True)
