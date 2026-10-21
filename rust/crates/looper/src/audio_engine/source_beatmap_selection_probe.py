"""Real Loader/Copy/Config/map selection against the embedded Native test engine."""
# Executed only by the bounded Rust source-custody integration fixture.
# ruff: noqa: INP001

import json
from pathlib import Path

from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState


class Probe:
    def __init__(self, audio, input_json=None):
        self.audio = audio
        self.path = Path("samples/selected-map-vertical.json")
        self.restored = set()
        if input_json is None:
            self.before = self.path.read_bytes()
            self.persistence = ProjectPersistence.from_config_path(self.path)
            assert self.persistence.load_error is None, self.persistence.load_error
            self.project = self.persistence.project
            assert self.project.sample_paths[0] is None
            assert self.project.pad_content[0] is None
            assert {i for i, row in enumerate(self.project.sample_paths) if row} == {36}
        else:
            fixture = json.loads(input_json)
            self.project = ProjectState(key_lock=False)
            self.project.sample_paths[0] = fixture["reference"]
            self.project.sample_durations[0] = 128 / 48000
            self.project.pad_content[0] = PadContentIdentity(
                instance_id="8" * 32,
                material_id=fixture["material_id"],
                material_creation=fixture["material_creation"],
            )
            self.project.pad_timing_intent[0] = "tap"
            self.project.manual_bpm[0] = 137.25
            self.project.pad_key_lock[0] = False
            self.project.sample_analysis[0] = None
            self.persistence = ProjectPersistence(self.project)
            audio.set_pad_bpm(0, 137.25)
            audio.set_pad_timing_intent(0, "tap")
        self.persistence.config_path = self.path
        self.persistence.bind_audio(audio)
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
        self.events = []
        deliver = self.loader._deliver_loader_event

        def observe_delivery(event, handlers, deferred):
            if event.get("type") == "success" and event.get("id") == 36:
                content = self.project.pad_content[36]
                if content is not None and 36 in self.session.loading_sample_ids:
                    assert content.instance_id not in self.loader.source_beatmaps._pending
            self.events.append({
                "type": event.get("type"),
                "id": event.get("id"),
                "request_id": event.get("request_id"),
                "private_owner": event.get("_native_event_owner") is not None,
            })
            self.events[:] = self.events[-16:]
            deliver(event, handlers, deferred)

        self.loader._deliver_loader_event = observe_delivery
        self.loader.source_beatmaps.bind(
            self.persistence.commit_source_beatmap_selection,
            blocked=lambda: False,
            edit_guard=lambda _sample_id: None,
        )
        self.loader.set_restored_sample_loaded_callback(self._restored)
        self.arrangement = PadArrangementController(
            self.persistence,
            self.session,
            audio,
            self.assets,
            stem_task_runner=lambda work: work(),
        )
        if input_json is not None:
            self.persistence.mark_dirty()
            self.persistence.flush()

    def _restored(self, sample_id):
        self.audio.set_pad_bpm(sample_id, self.project.manual_bpm[sample_id])
        self.audio.set_pad_timing_intent(sample_id, self.project.pad_timing_intent[sample_id])
        self.restored.add(sample_id)
        return True

    def poll(self):
        self.arrangement.poll()
        self.loader.poll_loader_events()
        assert not self.session.sample_load_errors, self.session.sample_load_errors
        assert not self.loader.source_beatmaps.errors, self.loader.source_beatmaps.errors

    def select(self, record_json):
        self.loader.source_beatmaps.select(0, record_json)
        assert self.loader.source_beatmaps.status(0) == "pending"

    def selected(self):
        self.poll()
        if self.loader.source_beatmaps.status(0) != "effective":
            return False
        saved = json.loads(self.path.read_text())
        assert saved["sample_analysis"][0] is None
        assert saved["manual_bpm"][0] == 137.25
        assert saved["pad_timing_intent"][0] == "tap"
        assert saved["pad_source_beatmap_intent"][0] == (
            self.project.pad_source_beatmap_intent[0].model_dump(mode="json")
        )
        return True

    def copy(self):
        assert self.arrangement.copy_to_empty(0, 36)

    def copied(self):
        self.poll()
        if self.arrangement.active:
            return False
        assert self.arrangement.status == "completed", self.arrangement.error
        if self.loader.source_beatmaps.status(36) != "effective":
            return False
        origin, copied = self.project.pad_content[0], self.project.pad_content[36]
        assert copied.instance_id != origin.instance_id
        assert copied.copied_from == origin.instance_id
        origin_intent = self.project.pad_source_beatmap_intent[0]
        copied_intent = self.project.pad_source_beatmap_intent[36]
        assert copied_intent == origin_intent
        assert copied_intent is not origin_intent
        assert self.project.sample_analysis[36] is None
        assert self.project.manual_bpm[36] == 137.25
        assert self.project.pad_timing_intent[36] == "tap"
        return True

    def unload_origin(self):
        self.loader.unload_sample(0)
        self.persistence.mark_dirty()
        self.persistence.flush()
        self.assets.sync_assignments()
        assert self.project.pad_source_beatmap_intent[0] is None
        assert self.project.pad_content[0] is None
        assert self.project.sample_paths[0] is None
        assert self.project.pad_source_beatmap_intent[36].selected is not None
        self.retained = self.path.read_bytes()

    def retained_after_collection(self):
        self.poll()
        self.assets.retry_retirements()
        assert self.path.read_bytes() == self.retained
        assert self.loader.source_beatmaps.status(36) == "effective"
        assert self.audio.current_input_runtime_pad_binding(0) is None
        assert self.audio.current_input_runtime_pad_binding(36).is_current()
        selected = self.project.pad_source_beatmap_intent[36].selected
        assert Path(selected.reference).is_file()
        assert Path(self.project.sample_paths[36]).is_file()
        return self.loader.source_beatmaps.effective(36).record_json()

    def restore(self):
        assert self.loader.source_beatmaps.effective(36) is None
        self.loader.restore_samples_from_project_state()
        assert self.session.loading_sample_ids == {36}

    def restored_copy(self):
        self.poll()
        if self.session.loading_sample_ids or self.restored != {36}:
            return False
        if self.loader.source_beatmaps.status(36) != "effective":
            return False
        assert self.project.sample_analysis[36] is None
        assert self.project.manual_bpm[36] == 137.25
        assert self.project.pad_timing_intent[36] == "tap"
        assert self.project.pad_content[0] is None
        assert self.audio.current_input_runtime_pad_binding(0) is None
        assert self.audio.current_input_runtime_pad_binding(36).is_current()
        assert self.path.read_bytes() == self.before
        return True

    def restored_record(self):
        return self.loader.source_beatmaps.effective(36).record_json()

    def diagnostics(self):
        maps = self.loader.source_beatmaps
        pending = {
            content: {
                "restoring": item.restoring,
                "captured_context": item.source_context,
                "status": item.job.source_beatmap_async_status(),
                "owner_cancelled": item.job.is_cancelled(),
                "original_job": item.job.metadata(),
            }
            for content, item in maps._pending.items()
        }
        return json.dumps(
            {
                "loading": sorted(self.session.loading_sample_ids),
                "events": self.events,
                "load_errors": self.session.sample_load_errors,
                "restored": sorted(self.restored),
                "map_status": {str(i): maps.status(i) for i in (0, 36)},
                "map_errors": maps.errors,
                "pending": pending,
                "failed": maps._failed,
                "retry_after": maps._retry_after,
                "current_source": self.audio.waveform_source_identity(36),
                "intent": self.project.pad_source_beatmap_intent[36],
                "copied_content": self.project.pad_content[36],
            },
            default=repr,
            sort_keys=True,
        )

    def close(self):
        assert not self.arrangement.active
        self.loader.shut_down()
        self.arrangement = None
        self.loader = None
        self.assets = None
        self.persistence = None
        self.audio = None
