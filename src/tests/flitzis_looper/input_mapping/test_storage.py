import json
from typing import TYPE_CHECKING

from flitzis_looper.input_mapping.actions import LooperAction
from flitzis_looper.input_mapping.bindings import KeyboardBinding, MidiBinding
from flitzis_looper.input_mapping.storage import (
    KEYBOARD_MAPPING_PATH,
    MIDI_MAPPING_PATH,
    KeyboardMappingEntry,
    MidiMappingEntry,
    clear_keyboard_mappings,
    clear_midi_mappings,
    load_keyboard_mapping_file,
    load_midi_mapping_file,
    save_keyboard_mapping_file,
    save_midi_mapping_file,
)

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def test_default_mapping_files_are_created(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)

    keyboard = load_keyboard_mapping_file()
    midi = load_midi_mapping_file()

    assert keyboard.schema_version == 1
    assert keyboard.ignore_when_typing is True
    assert keyboard.mappings == []
    assert KEYBOARD_MAPPING_PATH.is_file()

    assert midi.schema_version == 1
    assert midi.device_mode == "device_neutral"
    assert midi.mappings == []
    assert MIDI_MAPPING_PATH.is_file()


def test_clear_keyboard_mappings_preserves_schema_fields(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    data = load_keyboard_mapping_file()
    data.mappings.append(
        KeyboardMappingEntry(
            input=KeyboardBinding(key_name="A", ctrl=True),
            action=LooperAction.trigger_pad(0),
        )
    )
    save_keyboard_mapping_file(data)

    cleared = clear_keyboard_mappings()

    assert cleared.schema_version == 1
    assert cleared.ignore_when_typing is True
    assert cleared.mappings == []
    raw = json.loads(KEYBOARD_MAPPING_PATH.read_text(encoding="utf-8"))
    assert raw["mappings"] == []
    assert raw["ignore_when_typing"] is True


def test_clear_midi_mappings_preserves_schema_fields(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    data = load_midi_mapping_file()
    data.mappings.append(
        MidiMappingEntry(
            input=MidiBinding(kind="note", channel=1, number=60),
            action=LooperAction.trigger_pad(0),
        )
    )
    save_midi_mapping_file(data)

    cleared = clear_midi_mappings()

    assert cleared.schema_version == 1
    assert cleared.device_mode == "device_neutral"
    assert cleared.mappings == []
    raw = json.loads(MIDI_MAPPING_PATH.read_text(encoding="utf-8"))
    assert raw["mappings"] == []
    assert raw["device_mode"] == "device_neutral"


def test_invalid_mapping_file_is_backed_up_and_recreated(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    MIDI_MAPPING_PATH.parent.mkdir(parents=True)
    MIDI_MAPPING_PATH.write_text("{not-json}", encoding="utf-8")

    loaded = load_midi_mapping_file()

    assert loaded.mappings == []
    assert MIDI_MAPPING_PATH.with_suffix(".json.invalid").is_file()
