from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from pydantic import BaseModel, Field

from flitzis_looper.audio_gain import clamp_gain_db, format_gain_db
from flitzis_looper.constants import PAD_GAIN_DB_MAX, PAD_GAIN_DB_MIN, SPEED_MAX, SPEED_MIN
from flitzis_looper.key_intent import MusicalKey

if TYPE_CHECKING:
    from flitzis_looper.models import StemMaskDisplayMode, StemMixMode, TriggerQuantizationMode

type LooperActionSource = Literal["direct", "python"]
type PadEqBand = Literal["low", "mid", "high"]
type PitchCcMode = Literal["button", "absolute", "twos_complement", "offset_binary"]


@dataclass(frozen=True)
class PitchAction:
    """Parsed stable pitch target and the variant fixed when the input was learned."""

    kind: Literal["base", "extra", "reset_base", "note", "original", "note_step", "extra_step"]
    target: MusicalKey | int | None
    retrigger: bool
    cc_mode: PitchCcMode = "button"


class LooperAction(BaseModel):
    """Serializable performer action used by MIDI, keyboard, and Learn mode."""

    key: str = Field(min_length=1)
    label: str = Field(default="")
    source: LooperActionSource = "python"

    @classmethod
    def trigger_pad(cls, pad_id: int) -> LooperAction:
        return cls(key=f"pad.trigger:{pad_id}", label=f"Trigger pad {pad_id + 1}", source="direct")

    @classmethod
    def stop_pad(cls, pad_id: int) -> LooperAction:
        return cls(key=f"pad.stop:{pad_id}", label=f"Stop pad {pad_id + 1}", source="direct")

    @classmethod
    def unload_pad(cls, pad_id: int) -> LooperAction:
        return cls(key=f"pad.unload:{pad_id}", label=f"Unload pad {pad_id + 1}")

    @classmethod
    def analyze_pad(cls, pad_id: int) -> LooperAction:
        return cls(key=f"pad.analyze:{pad_id}", label=f"Analyze pad {pad_id + 1}")

    @classmethod
    def adjust_loop(cls, pad_id: int) -> LooperAction:
        return cls(key=f"pad.adjust_loop:{pad_id}", label=f"Adjust loop {pad_id + 1}")

    @classmethod
    def select_pad(cls, pad_id: int) -> LooperAction:
        return cls(key=f"ui.select_pad:{pad_id}", label=f"Select pad {pad_id + 1}")

    @classmethod
    def select_bank(cls, bank_id: int) -> LooperAction:
        return cls(key=f"ui.select_bank:{bank_id}", label=f"Select bank {bank_id + 1}")

    @classmethod
    def toggle_multi_loop(cls) -> LooperAction:
        return cls(key="global.multi_loop.toggle", label="Toggle Multi Loop")

    @classmethod
    def toggle_key_lock(cls) -> LooperAction:
        return cls(key="global.key_lock.toggle", label="Toggle Key Lock")

    @classmethod
    def toggle_bpm_lock(cls) -> LooperAction:
        return cls(key="global.bpm_lock.toggle", label="Toggle BPM Lock")

    @classmethod
    def toggle_trigger_quantization(cls) -> LooperAction:
        return cls(key="global.trigger_quantization.toggle", label="Toggle Trigger Quantize")

    @classmethod
    def stop_all(cls) -> LooperAction:
        return cls(key="global.stop_all", label="Stop all")

    @classmethod
    def speed_delta(cls, direction: Literal["increase", "decrease"]) -> LooperAction:
        return cls(key=f"global.speed.{direction}", label=f"Speed {direction}")

    @classmethod
    def reset_speed(cls) -> LooperAction:
        return cls(key="global.speed.reset", label="Reset speed")

    @classmethod
    def trigger_quantization(cls, mode: TriggerQuantizationMode) -> LooperAction:
        return cls(key=f"global.trigger_quantization:{mode}", label=f"Trigger quantize {mode}")

    @classmethod
    def generate_stems(cls, pad_id: int) -> LooperAction:
        return cls(key=f"stem.generate:{pad_id}", label=f"Generate stems {pad_id + 1}")

    @classmethod
    def delete_stems(cls, pad_id: int) -> LooperAction:
        return cls(key=f"stem.delete:{pad_id}", label=f"Delete stems {pad_id + 1}")

    @classmethod
    def stem_mix(cls, pad_id: int, mode: StemMixMode) -> LooperAction:
        return cls(key=f"stem.mix:{pad_id}:{mode}", label=f"Stem mix {mode}")

    @classmethod
    def stem_mask(
        cls,
        pad_id: int,
        mask: int,
        display_mode: StemMaskDisplayMode,
    ) -> LooperAction:
        return cls(
            key=f"stem.mask:{pad_id}:{mask}:{display_mode}",
            label=f"Stem mask {pad_id + 1}",
        )

    @classmethod
    def from_key(cls, key: str) -> LooperAction:
        return cls(key=key, source="direct" if parse_pitch_action(key) is not None else "python")


def global_source_sync_toggle_action() -> LooperAction:
    """Toggle the global gate while retaining independent PAD preferences."""
    return LooperAction(key="global.source_sync.toggle", label="Toggle global SYNC")


def global_realtime_bpm_toggle_action() -> LooperAction:
    """Toggle variable source timing independently of the global SYNC gate."""
    return LooperAction(key="global.realtime_bpm.toggle", label="Toggle Realtime BPM")


def source_sync_toggle_action(pad_id: int) -> LooperAction:
    """Toggle the independent content request at the captured PAD target."""
    return LooperAction(
        key=f"pad.source_sync.toggle:{pad_id}", label=f"Toggle SYNC pad {pad_id + 1}"
    )


def pitch_base_action(target: MusicalKey, *, retrigger: bool) -> LooperAction:
    """Return an absolute same-mode target, independent of menu and selected content."""
    variant = _pitch_variant(retrigger=retrigger)
    return LooperAction(
        key=f"pitch.base:{target.root}:{target.mode}:{variant}",
        label=f"Key {target.label}{' + retrigger' if retrigger else ''}",
        source="direct",
    )


def pitch_extra_action(semitones: int, *, retrigger: bool) -> LooperAction:
    """Return one strict signed SET or SET_RETRIGGER transposition target."""
    if isinstance(semitones, bool) or not isinstance(semitones, int):
        msg = "semitones must be an integer"
        raise TypeError(msg)
    if not -18 <= semitones <= 18:
        msg = "extra semitones must be between -18 and +18"
        raise ValueError(msg)
    variant = _pitch_variant(retrigger=retrigger)
    return LooperAction(
        key=f"pitch.extra:{semitones}:{variant}",
        label=f"Transpose {semitones:+d}{' + retrigger' if retrigger else ''}",
        source="direct",
    )


def pitch_reset_base_action(*, retrigger: bool) -> LooperAction:
    """Return the source-neutral numeric base reset, including unknown source keys."""
    variant = _pitch_variant(retrigger=retrigger)
    return LooperAction(
        key=f"pitch.base.reset:{variant}",
        label=f"Reset base pitch{' + retrigger' if retrigger else ''}",
        source="direct",
    )


def pitch_note_action(semitones: int, *, retrigger: bool) -> LooperAction:
    """Choose an octave-bearing relative piano note through the existing pitch route."""
    if type(semitones) is not int or not -23 <= semitones <= 23:
        msg = "piano semitones must be an integer between -23 and +23"
        raise ValueError(msg)
    return LooperAction(
        key=f"pitch.note:{semitones}:{_pitch_variant(retrigger=retrigger)}",
        label=f"Piano {semitones:+d} st{' + retrigger' if retrigger else ''}",
        source="direct",
    )


def pitch_original_action(*, retrigger: bool) -> LooperAction:
    """Reset the original note, offset and correction as one captured action."""
    return LooperAction(
        key=f"pitch.original:{_pitch_variant(retrigger=retrigger)}",
        label=f"Original Key{' + retrigger' if retrigger else ''}",
        source="direct",
    )


def pitch_step_action(
    semitones: int, *, piano: bool = False, retrigger: bool, cc_mode: PitchCcMode = "button"
) -> LooperAction:
    """Capture a chromatic or octave step against the original Native requested tuple."""
    if type(semitones) is not int or semitones not in {-12, -1, 1, 12}:
        msg = "pitch step must be -12, -1, +1 or +12 semitones"
        raise ValueError(msg)
    lane = "note" if piano else "extra"
    if cc_mode not in {"button", "absolute", "twos_complement", "offset_binary"}:
        msg = "unknown MIDI pitch step encoding"
        raise ValueError(msg)
    target = f"pitch.{lane}.step:{semitones}"
    if cc_mode != "button":
        target = f"pitch.{lane}.encoder:{semitones}:{cc_mode}"
    return LooperAction(
        key=f"{target}:{_pitch_variant(retrigger=retrigger)}",
        label=f"{'Piano' if piano else 'Transpose'} step {semitones:+d}"
        f"{' + retrigger' if retrigger else ''}",
        source="direct",
    )


def _pitch_variant(*, retrigger: bool) -> Literal["set", "set_retrigger"]:
    if not isinstance(retrigger, bool):
        msg = "retrigger must be a boolean"
        raise TypeError(msg)
    return "set_retrigger" if retrigger else "set"


def parse_pitch_action(key: str) -> PitchAction | None:
    """Decode a canonical semantic action without consulting checkbox or selection state."""
    parts = key.split(":")
    if parts[-1] not in {"set", "set_retrigger"}:
        return None
    retrigger = parts[-1] == "set_retrigger"
    if len(parts) == 2 and parts[0] == "pitch.base.reset":
        return PitchAction("reset_base", None, retrigger)
    if len(parts) == 2 and parts[0] == "pitch.original":
        return PitchAction("original", None, retrigger)
    if len(parts) == 4 and parts[0] == "pitch.base" and parts[2] in {"major", "minor"}:
        root = _pitch_integer(parts[1], minimum=0, maximum=11)
        if root is not None:
            mode: Literal["major", "minor"] = "major" if parts[2] == "major" else "minor"
            return PitchAction("base", MusicalKey(root=root, mode=mode), retrigger)
    return _parse_numeric_pitch_action(parts, retrigger=retrigger)


def _parse_numeric_pitch_action(parts: list[str], *, retrigger: bool) -> PitchAction | None:
    """Parse octave-aware fixed and relative numeric targets independently of legacy roots."""
    if len(parts) == 3 and parts[0] == "pitch.note":
        semitones = _pitch_integer(parts[1], minimum=-23, maximum=23)
        if semitones is not None:
            return PitchAction("note", semitones, retrigger)
    if len(parts) == 3 and parts[0] in {"pitch.note.step", "pitch.extra.step"}:
        semitones = _pitch_integer(parts[1], minimum=-12, maximum=12)
        if semitones in {-12, -1, 1, 12}:
            return PitchAction(
                "note_step" if parts[0] == "pitch.note.step" else "extra_step",
                semitones,
                retrigger,
            )
    if len(parts) == 3 and parts[0] == "pitch.extra":
        semitones = _pitch_integer(parts[1], minimum=-18, maximum=18)
        if semitones is not None:
            return PitchAction("extra", semitones, retrigger)
    return _parse_encoder_pitch_action(parts, retrigger=retrigger)


def _parse_encoder_pitch_action(parts: list[str], *, retrigger: bool) -> PitchAction | None:
    """Retain explicit CC encoding in saved actions without guessing from a single value."""
    if len(parts) != 4 or parts[0] not in {"pitch.note.encoder", "pitch.extra.encoder"}:
        return None
    modes: dict[str, PitchCcMode] = {
        "absolute": "absolute",
        "twos_complement": "twos_complement",
        "offset_binary": "offset_binary",
    }
    mode = modes.get(parts[2])
    step = _pitch_integer(parts[1], minimum=-12, maximum=12)
    if mode is None or step not in {-12, -1, 1, 12}:
        return None
    return PitchAction(
        "note_step" if parts[0] == "pitch.note.encoder" else "extra_step",
        step,
        retrigger,
        mode,
    )


def _pitch_integer(raw: str, *, minimum: int, maximum: int) -> int | None:
    try:
        value = int(raw)
    except ValueError:
        return None
    return value if str(value) == raw and minimum <= value <= maximum else None


def describe_pitch_rejection(reason: str) -> str:
    """Translate native admission feedback into a short performer-facing explanation."""
    descriptions = {
        "invalid_action": "The saved pitch choice is invalid.",
        "no_selected_content": "Select a loaded pad first.",
        "stale_content": "The selected audio has been removed or replaced.",
        "source_changed": "The audio changed before this pitch choice could be applied.",
        "timing_changed": "Loop timing changed before this pitch choice could be applied.",
        "timing_unavailable": "Audio timing is not ready yet.",
        "unknown_source_key": "Source key is unknown; choose a source correction first.",
        "mode_mismatch": "Key must keep the source's major/minor mode.",
        "unsupported_pitch": "This pitch and playback speed combination is not supported.",
        "input_disabled": "Input mapping is disabled.",
        "placement_pending": "Audio is moving; try again after the move.",
        "window_pending": "Audio is not ready for this pitch choice yet.",
        "capacity": "Too many pitch choices are waiting; try again.",
        "preparation_failed": "Pitch preparation failed; try again.",
        "prepared_mismatch": "Audio changed during pitch preparation; try again.",
        "already_published": "This pitch choice has already been applied.",
        "sequence_exhausted": "Restart the app before making another pitch choice.",
        "launch_cancelled": "The pending pitch attack was cancelled by Stop.",
    }
    return descriptions.get(reason, reason)


def tap_bpm_action(pad_id: int) -> LooperAction:
    """Return a serializable Tap BPM action for one pad."""
    return LooperAction(key=f"pad.tap_bpm:{pad_id}", label=f"Tap BPM {pad_id + 1}")


def selected_tap_bpm_action() -> LooperAction:
    """Return a serializable Tap BPM action for the currently selected pad."""
    return LooperAction(key="pad.tap_bpm.selected", label="Tap BPM selected pad")


def start_stop_action() -> LooperAction:
    """Return the serializable bottom-bar START/STOP primary action."""
    return LooperAction(key="global.start_stop", label="START/STOP")


def pad_eq_action(pad_id: int, band: PadEqBand, db: float) -> LooperAction:
    """Return a serializable per-pad EQ set-value action."""
    value = round(float(db), 1)
    return LooperAction(
        key=f"pad.eq:{pad_id}:{band}:{value}",
        label=f"Pad {pad_id + 1} {band} EQ {value:.1f} dB",
    )


def pad_eq_delta_action(pad_id: int, band: PadEqBand) -> LooperAction:
    """Return a serializable per-pad EQ relative-step action."""
    return LooperAction(
        key=f"pad.eq.delta:{pad_id}:{band}",
        label=f"Pad {pad_id + 1} {band} EQ relative",
    )


def selected_pad_eq_delta_action(band: PadEqBand) -> LooperAction:
    """Return a serializable selected-pad EQ relative-step action."""
    return LooperAction(
        key=f"pad.eq.selected.delta:{band}",
        label=f"Selected pad {band} EQ relative",
    )


def pad_gain_action(pad_id: int, gain_db: float) -> LooperAction:
    """Return a serializable per-pad Gain/Trim set-value action."""
    gain_db = clamp_gain_db(gain_db)
    min_tenths = round(PAD_GAIN_DB_MIN * 10)
    max_tenths = round(PAD_GAIN_DB_MAX * 10)
    tenths_db = max(min_tenths, min(max_tenths, round(gain_db * 10)))
    return LooperAction(
        key=f"pad.gain_db:{pad_id}:{tenths_db}",
        label=f"Pad {pad_id + 1} gain {format_gain_db(tenths_db / 10.0)}",
    )


def pad_gain_delta_action(pad_id: int) -> LooperAction:
    """Return a serializable per-pad gain relative-step action."""
    return LooperAction(key=f"pad.gain.delta:{pad_id}", label=f"Pad {pad_id + 1} gain relative")


def master_volume_action(volume: float) -> LooperAction:
    """Return a serializable master-volume set-value action."""
    percent = max(0, min(100, round(float(volume) * 100)))
    return LooperAction(key=f"global.volume:{percent}", label=f"Master volume {percent} %")


def master_volume_delta_action() -> LooperAction:
    """Return a serializable master-volume relative-step action."""
    return LooperAction(key="global.volume.delta", label="Master volume relative")


def global_speed_action(speed: float) -> LooperAction:
    """Return a serializable global speed/pitch set-value action."""
    min_percent = round(SPEED_MIN * 100)
    max_percent = round(SPEED_MAX * 100)
    percent = max(min_percent, min(max_percent, round(float(speed) * 100)))
    return LooperAction(key=f"global.speed:{percent}", label=f"Pitch {percent} %")


def global_speed_delta_action() -> LooperAction:
    """Return a serializable global speed/pitch relative-step action."""
    return LooperAction(key="global.speed.delta", label="Pitch relative")
