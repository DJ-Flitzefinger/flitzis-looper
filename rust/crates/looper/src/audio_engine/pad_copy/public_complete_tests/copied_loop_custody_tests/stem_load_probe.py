"""Real Copy/Config owners followed by the public Loader and PAD selectors.

The enclosing Rust fixture alone supplies files, callback effects and Native ACKs.
This envelope uses a standalone Loader, rather than claiming complete App/device
acceptance or supplying replacement event receipts.
"""
# Executed by Rust PyModule::from_code; this file is intentionally not a Python package.
# ruff: noqa: INP001

import hashlib
import json
from pathlib import Path

from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.project_materials import original_asset
from flitzis_looper.ui.context import PadSelectors


class StemLoadProbe:
    """Share the genuine Copy's project, persistence and asset lifecycle owners."""

    def __init__(self, copy_probe):
        self.copy_probe = copy_probe
        self.project = copy_probe.project
        self.session = copy_probe.session
        self.persistence = copy_probe.persistence
        self.assets = copy_probe.assets
        self.audio = copy_probe.audio
        # The shared Copy envelope's saved +8 pitch is unrelated to loading.
        # Match this test's actual neutral Native source before capturing Copy;
        # preserve all source key/correction/retrigger metadata and other intent.
        key = self.project.pad_key_intent[0]
        self.project.pad_key_intent[0] = key.changed(base_shift=0, extra_shift=0)
        self.persistence.mark_dirty()
        self.persistence.flush()
        self.loader = LoaderController(
            self.project,
            self.session,
            self.audio,
            lambda _sample_id: None,
            on_project_changed=self.persistence.mark_dirty,
        )
        self.loader.bind_asset_lifecycle(self.assets)
        self.selectors = PadSelectors(self, self.project, self.session)
        self.request = None
        self.before = None
        self.input_path = None
        self.copied_identity = None
        self.completed = None

    def begin_copy(self, target):
        self.copy_probe.begin(target)

    def complete_copy(self, target):
        self.copy_probe.complete(target)
        assert self.project.pad_content[target].copied_from == "8" * 32

    def begin_load(self, target, path):
        """Request an external stem WAV without clearing its copied predecessor."""
        self.target = target
        self.input_path = Path(path)
        assert self.input_path.is_file()
        assert not self.input_path.is_relative_to(Path.cwd() / "samples")
        self.before = self.project.model_dump(mode="json")
        self.copied_identity = self.project.pad_content[target].instance_id
        self.loader.load_sample_async(target, str(self.input_path))
        assert target not in self.session.sample_load_errors, self.session.sample_load_errors
        self.request = self.loader.incoming_load_request_id(target)
        assert type(self.request) is int
        assert self.request > 0
        assert target in self.session.loading_sample_ids
        assert self.session.pending_sample_paths[target] == str(self.input_path)
        assert self.project.model_dump(mode="json") == self.before
        assert self.selectors.is_loaded(target)
        assert self.selectors.is_loading(target)
        assert self.selectors.label(target) == Path(self.before["sample_paths"][target]).name
        return self.request

    def poll_failure(self):
        """Observe a genuine decode error while retaining the complete copied row."""
        self.loader.poll_loader_events()
        assert self.project.model_dump(mode="json") == self.before
        if self.target in self.session.loading_sample_ids:
            return False
        assert self.session.sample_load_errors.get(self.target)
        assert self.target not in self.session.pending_sample_paths
        assert self.loader.incoming_load_request_id(self.target) is None
        assert self.selectors.is_loaded(self.target)
        assert not self.selectors.is_loading(self.target)
        assert (
            self.selectors.label(self.target) == Path(self.before["sample_paths"][self.target]).name
        )
        self.assert_cleanup()
        self.persistence.flush()
        saved = json.loads(self.persistence.config_path.read_text())
        for field, rows in self.before.items():
            if isinstance(rows, list):
                assert saved[field][self.target] == rows[self.target], field
        return True

    def poll_success(self):
        """Apply only the public Loader's own success after real callback adoption."""
        self.loader.poll_loader_events()
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        if self.target in self.session.loading_sample_ids:
            return False
        self.assert_replacement()
        self.persistence.flush()
        self.completed = self.project.model_dump(mode="json")
        return True

    def assert_replacement(self):
        target = self.target
        assert target not in self.session.pending_sample_paths
        assert self.loader.incoming_load_request_id(target) is None
        assert self.selectors.is_loaded(target)
        assert not self.selectors.is_loading(target)
        assert self.selectors.label(target) == "drums.wav"
        reference = self.project.sample_paths[target]
        assert reference is not None
        assert reference != self.before["sample_paths"][target]
        asset = original_asset(reference)
        assert asset.layout == "readable"
        assert asset.path.name == "drums.wav"
        assert asset.path.read_bytes() == self.input_path.read_bytes()
        content = self.project.pad_content[target]
        assert content.instance_id != self.copied_identity
        assert content.copied_from is None
        assert content.material_id == asset.material_id
        identity = self.audio.waveform_source_identity(target)
        assert identity is not None
        assert identity[0] == self.request
        assert (
            self.audio.loaded_source_digest(target)
            == hashlib.sha256(self.input_path.read_bytes()).hexdigest()
        )
        binding = self.audio.current_input_runtime_pad_binding(target)
        assert binding is not None
        assert binding.is_current()
        assert binding.metadata()["source_generation"] == self.request
        assert self.project.stem_cache[target] is None
        assert self.project.pad_stem_mix_mode[target] == "full_mix"
        current = self.project.model_dump(mode="json")
        for field, rows in self.before.items():
            if isinstance(rows, list):
                assert current[field][0] == rows[0], field
        self.assert_cleanup()

    def assert_retained(self):
        """Later polling/collection must keep the replacement visible and durable."""
        self.loader.poll_loader_events()
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        self.assert_replacement()
        current = self.project.model_dump(mode="json")
        # Material birth polling may add its genuine receipt after source success.
        assert current["sample_paths"] == self.completed["sample_paths"]
        assert (
            current["pad_content"][self.target]["instance_id"]
            == self.completed["pad_content"][self.target]["instance_id"]
        )
        self.persistence.flush()
        saved = json.loads(self.persistence.config_path.read_text())
        assert saved["sample_paths"][self.target] == current["sample_paths"][self.target]
        assert saved["pad_content"][self.target] == current["pad_content"][self.target]
        assert saved["sample_paths"][0] == self.before["sample_paths"][0]
        assert saved["pad_content"][0] == self.before["pad_content"][0]

    def assert_cleanup(self):
        self.assets.retry_retirements()
        assert not self.assets.retirement_errors, self.assets.retirement_errors
        _pending, _retained, _deleted, errors = self.audio.project_asset_cleanup_status()
        assert not errors, errors

    def diagnostics(self):
        return json.dumps({
            "loading": sorted(self.session.loading_sample_ids),
            "pending": self.session.pending_sample_paths,
            "errors": self.session.sample_load_errors,
            "stage": self.session.sample_load_stage,
            "request": self.request,
        })

    def close(self):
        self.loader.shut_down()
        self.loader = None
        self.selectors = None
        self.copy_probe.close()
        self.copy_probe = None
        self.assets = None
        self.persistence = None
        self.audio = None
