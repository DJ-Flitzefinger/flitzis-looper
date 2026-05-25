from typing import TYPE_CHECKING, Literal

from pydantic import BaseModel, Field

from flitzis_looper.constants import PAD_GAIN_MAX, PAD_GAIN_MIN, SPEED_MAX, SPEED_MIN

if TYPE_CHECKING:
    from flitzis_looper.models import StemMaskDisplayMode, StemMixMode, TriggerQuantizationMode

type LooperActionSource = Literal["direct", "python"]
type PadEqBand = Literal["low", "mid", "high"]


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
        return cls(key="global.stop_all", label="Stop all", source="direct")

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
        return cls(key=key)


def tap_bpm_action(pad_id: int) -> LooperAction:
    """Return a serializable Tap BPM action for one pad."""
    return LooperAction(key=f"pad.tap_bpm:{pad_id}", label=f"Tap BPM {pad_id + 1}")


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


def pad_gain_action(pad_id: int, gain: float) -> LooperAction:
    """Return a serializable per-pad gain set-value action."""
    min_percent = round(PAD_GAIN_MIN * 100)
    max_percent = round(PAD_GAIN_MAX * 100)
    percent = max(min_percent, min(max_percent, round(float(gain) * 100)))
    return LooperAction(key=f"pad.gain:{pad_id}:{percent}", label=f"Pad {pad_id + 1} gain")


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
