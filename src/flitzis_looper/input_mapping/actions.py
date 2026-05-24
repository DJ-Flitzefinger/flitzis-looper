from typing import TYPE_CHECKING, Literal

from pydantic import BaseModel, Field

if TYPE_CHECKING:
    from flitzis_looper.models import StemMaskDisplayMode, StemMixMode, TriggerQuantizationMode

type LooperActionSource = Literal["direct", "python"]


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
