"""Productive arrangement/persistence/assets with the real embedded Native engine."""
# Executed by Rust PyModule::from_code; this file is intentionally not a Python package.
# ruff: noqa: INP001

import json
from pathlib import Path

from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.persistence import PersistenceFenceError, ProjectPersistence
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.key_intent import PadKeyIntent, SourceKeyVersion
from flitzis_looper.models import (
    PadContentIdentity,
    PadStemIntent,
    ProjectState,
    SessionState,
    StemCacheEntry,
    StemPairSelection,
)


class Probe:
    def __init__(self, audio, input_json):
        self.audio = audio
        fixture = json.loads(input_json)
        self.project = ProjectState(key_lock=False, selected_pad=0, volume=0.7)
        project = self.project
        project.sample_paths[0] = fixture["original_reference"]
        project.sample_durations[0] = fixture["frames"] / fixture["rate"]
        project.pad_content[0] = PadContentIdentity(
            instance_id="8" * 32,
            material_id=fixture["material_id"],
            material_creation=fixture["material_creation"],
        )
        project.pad_key_lock[0] = False
        project.pad_timing_intent[0] = "tap"
        project.manual_bpm[0] = 137.25
        project.pad_loop_auto[0] = False
        project.pad_loop_start_s[0] = 128 / fixture["rate"]
        project.pad_loop_end_s[0] = 2048 / fixture["rate"]
        project.pad_loop_bars[0] = 1.5
        project.pad_grid_anchor_s[0] = 0.003
        project.pad_grid_offset_samples[0] = 7
        project.pad_gain_db[0] = -7.0
        project.pad_eq_low_db[0] = 3.0
        project.pad_eq_mid_db[0] = -4.0
        project.pad_eq_high_db[0] = 1.5
        project.pad_stem_intent[0] = PadStemIntent(
            enabled_mask=0, last_custom_mask=5, display_mode="custom"
        )
        project.pad_key_intent[0] = PadKeyIntent(
            source=SourceKeyVersion(version=7, raw_key="Bb"),
            correction="Fm",
            base_shift=-4,
            extra_shift=12,
            retrigger=True,
        )
        pair = StemPairSelection.model_validate(fixture["pair"])
        project.stem_cache[0] = StemCacheEntry(
            source_version=fixture["source_version"],
            cache_dir=pair.wav_generation,
            stems=expected_stem_files(pair.wav_generation),
            available=True,
            pair=pair,
        )
        project.pad_stem_mix_mode[0] = "all_stems"
        self.session = SessionState()
        self.session.active_sample_ids.add(0)
        self.persistence = ProjectPersistence(project)
        self.persistence.config_path = Path("samples/copy-public.json")
        self.persistence.bind_audio(audio)
        self.assets = ProjectAssetLifecycle(project, audio)
        self.assets.sync_assignments()
        self.persistence.mark_dirty()
        self.persistence.flush()
        self.callbacks = []
        self.identities = []
        self.lifetimes = []
        self.loader = None
        self.arrangement = self._arrangement(lambda work: work())

    def _arrangement(self, runner):
        return PadArrangementController(
            self.persistence,
            self.session,
            self.audio,
            self.assets,
            stem_task_runner=runner,
            on_copy_committed=self._committed,
        )

    def _committed(self, target, ticket):
        metadata = json.loads(ticket.metadata_json())
        assert metadata["status"] == "accepted"
        assert metadata["control_mirrored"]
        assert self.project.pad_content[target] is not None
        assert self.persistence.config_path.read_bytes() != self.before
        self.callbacks.append(target)
        self.lifetimes.append(metadata["content_lifetime"])

    def begin(self, target):
        self.target = target
        self.before = self.persistence.config_path.read_bytes()
        self.before_callbacks = list(self.callbacks)
        assert self.audio.current_input_runtime_pad_binding(0).is_current()
        assert self.audio.current_input_runtime_pad_binding(target) is None
        assert self.arrangement.copy_to_empty(0, target)
        self.arrangement.poll()
        self.assert_waiting()

    def assert_waiting(self):
        for _ in range(3):
            self.arrangement.poll()
        assert self.arrangement.active
        assert self.callbacks == self.before_callbacks
        assert self.project.sample_paths[self.target] is None
        assert self.project.pad_content[self.target] is None
        assert self.persistence.config_path.read_bytes() == self.before
        try:
            self.persistence.flush()
        except PersistenceFenceError:
            pass
        else:
            message = "pending Native Copy did not fence the config writer"
            raise AssertionError(message)

    def complete(self, target):
        self.arrangement.poll()
        assert self.arrangement.status == "completed", self.arrangement.error
        assert not self.arrangement.active
        assert self.callbacks == [*self.before_callbacks, target]
        binding = self.audio.current_input_runtime_pad_binding(target)
        assert binding is not None
        assert binding.is_current()
        identity = self.project.pad_content[target]
        assert identity.instance_id != "8" * 32
        assert identity.instance_id not in self.identities
        assert identity.copied_from == "8" * 32
        self.identities.append(identity.instance_id)
        assert len(set(self.lifetimes)) == len(self.lifetimes)
        assert self.project.selected_pad == 0
        assert self.project.volume == 0.7
        assert self.session.active_sample_ids == {0}
        assert not self.session.paused_sample_ids
        saved = json.loads(self.persistence.config_path.read_text())
        for field in (
            "sample_paths",
            "sample_durations",
            "pad_key_lock",
            "manual_bpm",
            "pad_timing_intent",
            "pad_key_intent",
            "pad_gain_db",
            "pad_eq_low_db",
            "pad_eq_mid_db",
            "pad_eq_high_db",
            "pad_loop_start_s",
            "pad_loop_end_s",
            "pad_loop_auto",
            "pad_loop_bars",
            "pad_grid_offset_samples",
            "pad_grid_anchor_s",
            "pad_stem_intent",
            "pad_stem_mix_mode",
        ):
            assert saved[field][target] == saved[field][0], field
        assert saved["pad_content"][target]["copied_from"] == "8" * 32
        assert saved["pad_stem_intent"][target]["enabled_mask"] == 0
        assert saved["pad_stem_intent"][target]["last_custom_mask"] == 5
        assert saved["pad_key_intent"][target]["retrigger"]
        assert saved["stem_cache"][target]["pair"] == saved["stem_cache"][0]["pair"]

    def origin_unloaded(self):
        self.loader = LoaderController(
            self.project,
            self.session,
            self.audio,
            lambda _sample_id: None,
            on_project_changed=self.persistence.mark_dirty,
        )
        self.loader.bind_asset_lifecycle(self.assets)
        self.loader.unload_sample(0)
        self.persistence.flush()
        self.assets.sync_assignments()
        assert self.audio.current_input_runtime_pad_binding(0) is None
        for target in (36, 215):
            assert self.audio.current_input_runtime_pad_binding(target).is_current()
            assert Path(self.project.sample_paths[target]).is_file()
            pair = self.project.stem_cache[target].pair
            assert Path(pair.descriptor_reference).is_file()
            assert Path(pair.wav_generation).is_dir()
            assert Path(pair.pcm_generation).is_dir()

    def shutdown_before_worker(self):
        self.queued = []
        self.arrangement = self._arrangement(self.queued.append)
        self.before = self.persistence.config_path.read_bytes()
        assert self.arrangement.copy_to_empty(0, 36)
        assert len(self.queued) == 1
        assert self.arrangement.prepare_shutdown() is False
        assert self.arrangement.active
        assert self.project.sample_paths[36] is None
        assert self.persistence.config_path.read_bytes() == self.before
        try:
            self.persistence.flush()
        except PersistenceFenceError:
            pass
        else:
            message = "shutdown released the writer before actual worker return"
            raise AssertionError(message)
        self.queued.pop()()  # The real Native preparation observes its proven cancellation.
        assert self.arrangement.prepare_shutdown() is True
        assert self.arrangement.status == "cancelled", self.arrangement.error
        assert not self.arrangement.copy_to_empty(0, 215)
        assert self.project.pad_content[36] is None
        assert not self.callbacks
        assert self.persistence.config_path.read_bytes() == self.before
        self.persistence.flush()
        self.arrangement = self._arrangement(lambda work: work())

    def shutdown_after_native_ack(self):
        assert self.arrangement.prepare_shutdown() is True
        self.complete(36)
        assert not self.arrangement.copy_to_empty(0, 215)

    def close(self):
        # Break the bound completion-callback cycle before dropping sealed fixture
        # files. The completed transactions and their real persisted results stand.
        assert not self.arrangement.active
        if self.loader is not None:
            self.loader.shut_down()
            self.loader = None
        self.arrangement = None
        self.assets = None
        self.persistence = None
        self.audio = None


class RestoreProbe:
    """A fresh process restores saved C instances; no origin instance is consulted."""

    def __init__(self, audio):
        self.audio = audio
        self.path = Path("samples/copy-public.json")
        self.before = self.path.read_bytes()
        self.persistence = ProjectPersistence.from_config_path(self.path)
        assert self.persistence.load_error is None
        self.persistence.config_path = self.path
        self.persistence.bind_audio(audio)
        self.project = self.persistence.project
        self.saved = self.project.model_dump(mode="json")
        assert self.project.sample_paths[0] is None
        assert self.project.pad_content[0] is None
        assigned = {i for i, path in enumerate(self.project.sample_paths) if path is not None}
        assert assigned == {36, 215}
        self.session = SessionState()
        self.assets = ProjectAssetLifecycle(self.project, audio)
        self.assets.sync_assignments()
        self.loader = LoaderController(
            self.project,
            self.session,
            audio,
            lambda _sample_id: None,
            on_project_changed=self.persistence.mark_dirty,
        )
        self.loader.bind_asset_lifecycle(self.assets)
        self.loader.set_restored_sample_loaded_callback(self._restored)
        self.pairs = {}
        self.sources = {}
        self.selections = {}
        self.residencies = {}
        self.restored = set()
        self.held_reader = None

    def _restored(self, sample_id):
        project, audio = self.project, self.audio
        audio.set_pad_bpm(sample_id, project.manual_bpm[sample_id])
        audio.set_pad_timing_metadata(
            sample_id,
            project.pad_grid_anchor_s[sample_id]
            + project.pad_grid_offset_samples[sample_id] / audio.output_sample_rate(),
        )
        audio.set_pad_loop_region(
            sample_id, project.pad_loop_start_s[sample_id], project.pad_loop_end_s[sample_id]
        )
        audio.set_pad_gain(sample_id, project.pad_gain_db[sample_id])
        audio.set_pad_eq(
            sample_id,
            project.pad_eq_low_db[sample_id],
            project.pad_eq_mid_db[sample_id],
            project.pad_eq_high_db[sample_id],
        )
        audio.set_pad_timing_intent(sample_id, project.pad_timing_intent[sample_id])
        audio.set_pad_key_lock(sample_id, project.pad_key_lock[sample_id])
        self.restored.add(sample_id)
        return True

    def start(self):
        self.loader.restore_samples_from_project_state()
        assert self.session.loading_sample_ids == {36, 215}
        assert not self.session.sample_load_errors

    def poll_sources(self):
        self.loader.poll_loader_events()
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        return not self.session.loading_sample_ids and self.restored == {36, 215}

    def register_pair(self, sample_id):
        entry = self.project.stem_cache[sample_id]
        source = self.audio.capture_prepared_source(sample_id, entry.source_version)
        pair = self.audio.prepare_stem_pair(
            sample_id,
            entry.source_version,
            entry.cache_dir,
            source,
            components=True,
            descriptor_reference=entry.pair.descriptor_reference,
        )
        assert json.loads(pair.selection_json()) == entry.pair.model_dump(mode="json")
        pair.select()
        self.sources[sample_id] = source
        self.pairs[sample_id] = pair
        self.selections[sample_id] = self.audio.register_committed_stem_pair(pair, source)
        assert self.selections[sample_id].publication_status() == "pending"

    def request_components(self, sample_id):
        selection = self.selections[sample_id]
        assert selection.publication_status() == "accepted"
        assert selection.is_current()
        self.residencies[sample_id] = self.audio.request_committed_stem_residency(
            selection,
            mode="all_stems",
            enabled_mask=0,
        )

    def components_ready(self, sample_id):
        ticket = self.residencies[sample_id]
        assert ticket.residency_status() != "rejected", ticket.error()
        return ticket.residency_status() == "accepted"

    def assert_restored(self):
        assert self.audio.current_input_runtime_pad_binding(0) is None
        for sample_id in (36, 215):
            assert self.audio.current_input_runtime_pad_binding(sample_id).is_current()
            assert self.project.pad_content[sample_id].copied_from == "8" * 32
            for field in (
                "sample_paths",
                "pad_content",
                "sample_analysis",
                "manual_bpm",
                "pad_timing_intent",
                "pad_key_intent",
                "pad_stem_intent",
                "pad_stem_mix_mode",
                "pad_key_lock",
                "pad_gain_db",
                "pad_eq_low_db",
                "pad_eq_mid_db",
                "pad_eq_high_db",
                "pad_loop_start_s",
                "pad_loop_end_s",
                "pad_loop_auto",
                "pad_loop_bars",
                "pad_grid_anchor_s",
                "pad_grid_offset_samples",
            ):
                actual = self.project.model_dump(mode="json")[field][sample_id]
                assert actual == self.saved[field][sample_id], field
            assert (
                self.project.stem_cache[sample_id].pair.model_dump(mode="json")
                == self.saved["stem_cache"][sample_id]["pair"]
            )
            manifest = json.loads(self.audio.cold_source_manifest(sample_id))
            assert manifest["integrity"]["warm"]
            for key in (
                "source_copied_bytes",
                "original_copied_bytes",
                "assignment_copy_bytes",
                "stream_scratch_reserved_bytes",
            ):
                assert manifest["integrity"][key] == 0, (key, manifest["integrity"])
        assert not self.session.analyzing_sample_ids
        assert not self.session.stem_generating_sample_ids
        assert self.path.read_bytes() == self.before

    def request_audible_mask(self):
        self.residencies[36] = self.audio.request_committed_stem_residency(
            self.selections[36],
            mode="all_stems",
            enabled_mask=15,
        )

    def audible_mask_ready(self):
        ticket = self.residencies[36]
        assert ticket.residency_status() != "rejected", ticket.error()
        return ticket.effective_mode_status() == "effective"

    def hold_last_reader(self):
        self.held_reader = self.pairs[215].instrumental_reader()

    def unload(self, sample_id):
        self.loader.unload_sample(sample_id)
        self.sources.pop(sample_id, None)
        self.residencies.pop(sample_id, None)
        self.selections.pop(sample_id, None)
        self.pairs.pop(sample_id, None)
        self.persistence.flush()

    def assert_held_reader(self):
        assert not any(path is not None for path in self.project.sample_paths)
        assert self.held_reader is not None
        _start, frames, _rate, channels = self.held_reader.geometry()
        assert len(self.held_reader.read_f32le()) == frames * channels * 4

    def release_last_reader(self):
        self.held_reader = None
        self.assets.retry_retirements()

    def poll_retirements(self):
        # The productive loader poll reconciles revoked Native selection owners
        # before the asset worker can retire their last source and pair readers.
        self.loader.poll_loader_events()
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        self.assets.retry_retirements()

    def retirement_diagnostics(self):
        assets = self.assets
        with assets._retirement_lock:
            assignment_paths = [
                (kind, sample_id, str(path))
                for (kind, sample_id), (path, _lease) in assets._assignments.items()
            ]
            pending = [
                (str(path), recursive, len(owners))
                for (path, recursive), owners in assets._pending.items()
            ]
            pending_groups = len(assets._pending_groups)
            reserved = assets._reserved
            errors = {str(path): message for path, message in assets.retirement_errors.items()}
        return json.dumps({
            "assignments": assignment_paths,
            "pending": pending,
            "pending_groups": pending_groups,
            "reserved": reserved,
            "errors": errors,
            "native_budget": self.audio.get_resident_pcm_budget_status(),
            "sources": sorted(self.sources),
            "pairs": sorted(self.pairs),
            "selections": sorted(self.selections),
            "residencies": sorted(self.residencies),
            "held_reader": self.held_reader is not None,
            "load_retirements": sorted(self.loader._load_retirements),
            "material_creation_pending": sorted(self.loader._material_creation_pending),
            "sample_load_errors": self.session.sample_load_errors,
        })

    def close(self):
        self.loader.shut_down()
        self.loader = None
        self.pairs.clear()
        self.sources.clear()
        self.selections.clear()
        self.residencies.clear()
        self.held_reader = None
        self.assets = None
        self.persistence = None
        self.audio = None
