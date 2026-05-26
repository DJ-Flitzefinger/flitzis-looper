import json
import os
import tempfile
from contextlib import suppress
from pathlib import Path

from pydantic import BaseModel, ConfigDict, ValidationError

from flitzis_looper.input_mapping.actions import LooperAction  # noqa: TC001
from flitzis_looper.input_mapping.bindings import KeyboardBinding, MidiBinding  # noqa: TC001

INPUT_MAPPING_DIR = Path("config") / "input"
KEYBOARD_MAPPING_PATH = INPUT_MAPPING_DIR / "keyboard.json"
MIDI_MAPPING_PATH = INPUT_MAPPING_DIR / "midi.json"
MAPPING_SCHEMA_VERSION = 1


class MidiMappingEntry(BaseModel):
    """One MIDI input-to-action mapping."""

    input: MidiBinding
    action: LooperAction


class KeyboardMappingEntry(BaseModel):
    """One keyboard input-to-action mapping."""

    input: KeyboardBinding
    action: LooperAction


class MidiMappingFile(BaseModel):
    """Schema-versioned MIDI mapping file."""

    model_config = ConfigDict(validate_assignment=True)

    schema_version: int = MAPPING_SCHEMA_VERSION
    device_mode: str = "device_neutral"
    mappings: list[MidiMappingEntry] = []


class KeyboardMappingFile(BaseModel):
    """Schema-versioned keyboard mapping file."""

    model_config = ConfigDict(validate_assignment=True)

    schema_version: int = MAPPING_SCHEMA_VERSION
    ignore_when_typing: bool = True
    mappings: list[KeyboardMappingEntry] = []


def load_midi_mapping_file(path: Path = MIDI_MAPPING_PATH) -> MidiMappingFile:
    """Load the MIDI mapping file, creating defaults when missing or invalid."""
    return _load_mapping_file(path, MidiMappingFile())


def load_keyboard_mapping_file(path: Path = KEYBOARD_MAPPING_PATH) -> KeyboardMappingFile:
    """Load the keyboard mapping file, creating defaults when missing or invalid."""
    return _load_mapping_file(path, KeyboardMappingFile())


def save_midi_mapping_file(data: MidiMappingFile, path: Path = MIDI_MAPPING_PATH) -> None:
    """Persist the MIDI mapping file atomically."""
    _atomic_write_json(path, data.model_dump(mode="json"))


def save_keyboard_mapping_file(
    data: KeyboardMappingFile,
    path: Path = KEYBOARD_MAPPING_PATH,
) -> None:
    """Persist the keyboard mapping file atomically."""
    _atomic_write_json(path, data.model_dump(mode="json"))


def clear_midi_mappings(path: Path = MIDI_MAPPING_PATH) -> MidiMappingFile:
    """Clear MIDI mappings while preserving schema/default top-level fields."""
    data = load_midi_mapping_file(path)
    data.mappings = []
    save_midi_mapping_file(data, path)
    return data


def clear_keyboard_mappings(path: Path = KEYBOARD_MAPPING_PATH) -> KeyboardMappingFile:
    """Clear keyboard mappings while preserving schema/default top-level fields."""
    data = load_keyboard_mapping_file(path)
    data.mappings = []
    save_keyboard_mapping_file(data, path)
    return data


def _load_mapping_file[T: BaseModel](path: Path, default: T) -> T:
    try:
        raw = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        _atomic_write_json(path, default.model_dump(mode="json"))
        return default

    try:
        parsed_json = json.loads(raw)
        return type(default).model_validate(parsed_json)
    except json.JSONDecodeError, ValidationError, OSError, ValueError:
        _backup_invalid_file(path)
        _atomic_write_json(path, default.model_dump(mode="json"))
        return default


def _backup_invalid_file(path: Path) -> None:
    if not path.exists():
        return
    backup = path.with_suffix(f"{path.suffix}.invalid")
    with suppress(OSError):
        path.replace(backup)


def _atomic_write_json(path: Path, data: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"

    tmp_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as tmp:
            tmp_path = Path(tmp.name)
            tmp.write(text)
            tmp.flush()
            os.fsync(tmp.fileno())
        if tmp_path is not None:
            os.replace(tmp_path, path)
    finally:
        if tmp_path is not None:
            with suppress(OSError):
                tmp_path.unlink(missing_ok=True)
