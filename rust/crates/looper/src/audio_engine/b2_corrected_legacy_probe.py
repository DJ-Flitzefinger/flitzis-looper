"""Explicit private complete-source native QM production/finish probe."""
# Compiled into the ignored native probe; never imported by the application.
# ruff: noqa: INP001

import base64
import ctypes
import json
import os
import shutil
import sys
import time
from ctypes import wintypes
from pathlib import Path
from typing import Any

from flitzis_looper.analysis.reference_inputs_validation import (
    frozen_corpus,
    loaded_identities,
    private_path,
    read_source_aliases,
    sha256_file,
)


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, allow_nan=False), encoding="utf-8")


def read_json(path: Path) -> Any:
    return json.loads(path.read_bytes())


def runtime_modules() -> dict[str, object]:
    """Observe actual loaded native/Python modules; inspect no external file bytes."""
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    psapi = ctypes.WinDLL("psapi", use_last_error=True)
    kernel.GetCurrentProcess.argtypes = []
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    psapi.EnumProcessModulesEx.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(wintypes.HMODULE),
        wintypes.DWORD,
        ctypes.POINTER(wintypes.DWORD),
        wintypes.DWORD,
    ]
    psapi.EnumProcessModulesEx.restype = wintypes.BOOL
    psapi.GetModuleFileNameExW.argtypes = [
        wintypes.HANDLE,
        wintypes.HMODULE,
        wintypes.LPWSTR,
        wintypes.DWORD,
    ]
    psapi.GetModuleFileNameExW.restype = wintypes.DWORD
    process = kernel.GetCurrentProcess()
    count = 256
    needed = wintypes.DWORD()
    while True:
        modules = (wintypes.HMODULE * count)()
        if not psapi.EnumProcessModulesEx(
            process, modules, ctypes.sizeof(modules), ctypes.byref(needed), 3
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        if needed.value <= ctypes.sizeof(modules):
            break
        count = (needed.value + ctypes.sizeof(wintypes.HMODULE) - 1) // ctypes.sizeof(
            wintypes.HMODULE
        )
        if count > 8192:
            msg = "Native loaded-module observation exceeds bounded extent"
            raise RuntimeError(msg)
    loaded = []
    for module in modules[: needed.value // ctypes.sizeof(wintypes.HMODULE)]:
        path = ctypes.create_unicode_buffer(32768)
        if not psapi.GetModuleFileNameExW(process, module, path, len(path)):
            raise ctypes.WinError(ctypes.get_last_error())
        loaded.append({"module_base_address": module, "path": path.value})
    python_modules = [
        {"name": name, "path": str(module.__file__)}
        for name, module in sorted(sys.modules.items())
        if getattr(module, "__file__", None)
    ]
    return {
        "pid": os.getpid(),
        "native_observation": (
            "WinAPI EnumProcessModulesEx/GetModuleFileNameExW actual current process"
        ),
        "loaded_native_modules": loaded,
        "imported_python_modules": python_modules,
        "external_file_bytes_read": False,
    }


class Probe:
    """Run only the genuine corrected native legacy procedure, without a model worker."""

    def __init__(self, bridge: Any, config_path: str) -> None:
        self.bridge = bridge
        self.config_path = Path(config_path)
        self.config = read_json(self.config_path)
        self.workspace = Path(self.config["workspace"]).resolve(strict=True)
        self.output = Path(self.config["output_directory"]).resolve(strict=True)
        self.track_id = self.config["track_id"]
        self.corpus = frozen_corpus(self.workspace)
        self.track = next(item for item in self.corpus.tracks if item.id == self.track_id)
        aliases = read_source_aliases(
            self.workspace, self.config["source_aliases_path"], self.corpus
        )
        alias = next((item for item in aliases if item.track_id == self.track_id), None)
        self.source = private_path(
            self.workspace, alias.actual_source_path if alias else self.track.source_relative
        )
        assert self.source == private_path(self.workspace, self.config["actual_source_path"])
        assert sha256_file(self.source) == self.track.source_sha256 == self.config["source_sha256"]
        assert self.source.stat().st_size == self.track.source_bytes == self.config["source_bytes"]
        identity = next(
            item
            for item in loaded_identities(
                self.workspace, self.config["loaded_identities_path"]
            ).tracks
            if item.track_id == self.track_id
        )
        self.source_proof = read_json(self.output / "native-source-proof.json")
        assert self.source_proof["complete_mono_sha256"] == identity.pcm.sha256
        assert self.source_proof["complete_loaded_pcm"]["full_frames"] == identity.pcm.frame_count
        assert self.source_proof["complete_loaded_pcm"]["rate_hz"] == identity.pcm.sample_rate_hz
        self.events: list[dict[str, object]] = []
        self.observations: list[dict[str, object]] = []
        self.jobs = self.output / "jobs"
        self.jobs.mkdir()
        self.native = self.bridge.begin_offline_analysis(0)
        self.started = time.monotonic()
        self.observe("admitted")
        self.admitted = self.native.metadata()
        self.identity = {
            name: self.admitted[name]
            for name in ("pad_id", "request_id", "source_id", "source_generation")
        }
        self.run(identity.pcm.sha256, identity.pcm.frame_count)

    def run(self, mono_sha256: str, full_frames: int) -> None:
        temporary = self.jobs / "complete-native-export.f32le"
        analyzer = self.output / "complete-analyzer-input.f64le"
        write_json(
            self.output / "native-request.raw.json",
            {
                "schema_version": 1,
                "backend": "corrected-qm-native-v1",
                "identity": self.identity,
                "native_metadata": self.admitted,
                "pcm_export_path": str(temporary),
                "analyzer_export_path": str(analyzer),
            },
        )
        self.observe("before_prepare_export")
        self.native.prepare_export(str(temporary))
        self.observe("after_prepare_export")
        retained = self.output / "complete-native-export.f32le"
        shutil.copyfile(temporary, retained)
        assert sha256_file(retained, pcm=True) == mono_sha256
        assert retained.stat().st_size == full_frames * 4
        self.observe("before_analyze_corrected_legacy")
        self.raw = self.native.analyze_corrected_legacy(str(analyzer))
        (self.output / "native-result.raw.json").write_text(self.raw, encoding="utf-8")
        self.result = json.loads(self.raw)
        assert self.result["identity"] == self.identity
        assert self.result["loaded"]["mono_sha256"] == mono_sha256
        assert self.result["analyzer"]["sha256"] == sha256_file(analyzer)
        assert analyzer.stat().st_size == self.result["analyzer"]["frame_count"] * 8
        self.observe("after_analyze_corrected_legacy")
        self.observe("before_retire_pcm")
        self.native.retire_pcm()
        self.observe("after_retire_pcm")
        temporary.unlink()
        self.observe("before_finish")
        (self.output / "native-finish-input.raw.json").write_text(self.raw, encoding="utf-8")
        self.accepted = self.native.finish_corrected_legacy(self.raw)
        assert self.accepted is True
        self.observe("after_finish", finish_accepted=self.accepted)
        (self.output / "result-envelope.raw.json").write_text(self.raw, encoding="utf-8")
        self.observe("job_done")
        self.finished = time.monotonic()

    def observe(self, stage: str, **extra: object) -> None:
        self.observations.append({
            "stage": stage,
            "monotonic_seconds": time.monotonic(),
            "metadata": self.native.metadata(),
            "staging_stats": self.native.staging_stats(),
            **extra,
        })
        write_json(self.output / "native-observations.json", self.observations)

    def poll(self) -> bool:
        while (event := self.bridge.engine.poll_loader_events()) is not None:
            self.events.append(event)
            if event["type"] == "offline_analysis_completed":
                number = sum(item["type"] == "offline_analysis_completed" for item in self.events)
                (self.output / f"completion-event-{number - 1}.raw.json").write_text(
                    event["result_json"], encoding="utf-8"
                )
        write_json(self.output / "loader-events.json", self.events)
        return True

    def finish(self) -> None:
        self.poll()
        complete = [item for item in self.events if item["type"] == "offline_analysis_completed"]
        assert len(complete) == 1
        event = complete[0]
        assert (event["id"], event["request_id"]) == (
            self.identity["pad_id"],
            self.identity["request_id"],
        )
        completed = json.loads(event["result_json"])
        assert completed == self.result
        for name in ("raw", "compatibility"):
            for key, value in self.result[name].items():
                if key != "encoding":
                    assert base64.b64decode(
                        completed[name][key], validate=True
                    ) == base64.b64decode(value, validate=True)
        assert not list(self.jobs.iterdir())
        subsequent = self.bridge.begin_offline_analysis(0)
        metadata = subsequent.metadata()
        assert metadata["request_id"] == self.identity["request_id"] + 1
        assert all(
            metadata[name] == value for name, value in self.admitted.items() if name != "request_id"
        )
        subsequent.abort_unstarted()
        runtime = read_json(self.output / "native-runtime.json")
        runtime["embedded_python"] = {
            "version": sys.version,
            "executable": sys.executable,
            "base_prefix": sys.base_prefix,
        }
        runtime["keynet"] = "not_executed"
        runtime["beat_this_worker"] = "not_executed"
        runtime["installed_pyd"] = "not_imported_or_executed"
        write_json(self.output / "loaded-runtime-modules.json", runtime_modules())
        write_json(self.output / "native-runtime.json", runtime)
        write_json(
            self.output / "summary.json",
            {
                "schema_version": 1,
                "measurement_slice": "b2-corrected-legacy-hardware-free-native-provenance",
                "track_id": self.track_id,
                "source": str(self.source),
                "source_sha256": self.track.source_sha256,
                "source_bytes": self.track.source_bytes,
                "finish_accepted": self.accepted,
                "completion_event_count": len(complete),
                "retired": True,
                "subsequent_metadata": metadata,
                "job_wall_seconds": self.finished - self.started,
                "raw_counts": {
                    name: len(base64.b64decode(value, validate=True)) // 8
                    for name, value in self.result["raw"].items()
                    if name != "encoding"
                },
                "compatibility_counts": {
                    name: len(base64.b64decode(value, validate=True)) // 4
                    for name, value in self.result["compatibility"].items()
                    if name != "encoding"
                },
                "remaining_pcm_directories": [],
                "native_key_worker": "not_executed",
                "beat_this_worker": "not_executed",
                "musical_acceptance": "pending",
                "default_adoption": "blocked",
                "limitations": [
                    (
                        "Actual execution time is an observation, not model-process peak RSS "
                        "or resource acceptance."
                    ),
                    (
                        "Corrected native QM uses the complete G2 f64 channel-mean export "
                        "and shared normal converter/tracker."
                    ),
                    (
                        "No independent labels, scores, human sessions, device/hearing "
                        "or default acceptance."
                    ),
                ],
            },
        )
