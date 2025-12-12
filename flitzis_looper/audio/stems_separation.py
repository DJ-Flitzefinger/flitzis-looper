"""Stem Separation for flitzis_looper.
Uses demucs for high-quality audio source separation.

IMPORTANT: Stems can only be generated while the loop is stopped!
This avoids synchronization issues and simplifies the logic.
"""

import builtins
import contextlib
import gc
from tkinter import messagebox

import numpy as np
import torch
import torchaudio

from flitzis_looper.core.state import (
    STEM_NAMES,
    ensure_stems_structure,
    get_button_data,
    get_buttons,
)
from flitzis_looper.utils.logging import logger
from flitzis_looper.utils.threading import io_executor, schedule_gui_update


def generate_stems(
    button_id,
    update_button_label_callback,
    update_stem_buttons_state_callback,
    save_config_async_callback,
):
    """Generiert Stems für einen Loop im Hintergrund.
    Verwendet demucs für hochwertige Audio-Separation.

    WICHTIG: Stems können nur bei gestopptem Loop generiert werden!
    Das vermeidet Synchronisationsprobleme und macht die Logik viel einfacher.

    Args:
        button_id: The button ID to generate stems for
        update_button_label_callback: Callback to update button label
        update_stem_buttons_state_callback: Callback to update stem button states
        save_config_async_callback: Callback to save config asynchronously
    """
    button_data = get_button_data()
    buttons = get_buttons()
    data = button_data[button_id]

    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)

    # === BLOCKIERUNG: Loop muss gestoppt sein! ===
    if data["active"]:
        messagebox.showwarning(
            "Loop läuft",
            "Bitte stoppe den Loop zuerst, bevor du Stems generierst.\n\n"
            "Stems können nur bei gestopptem Loop erzeugt werden.",
        )
        return

    if not data["file"] or not data["bpm"]:
        messagebox.showwarning("Cannot Generate Stems", "Audio file and BPM must be set first.")
        return

    if data["stems"]["generating"]:
        messagebox.showinfo(
            "Already Generating", "Stems are already being generated for this loop."
        )
        return

    # Bei Regenerate: Alte Stems und Player zuerst freigeben
    if data["stems"]["available"]:
        # Stoppe alle Player
        if data["stems"].get("master_phasor"):
            with contextlib.suppress(builtins.BaseException):
                data["stems"]["master_phasor"].stop()
            data["stems"]["master_phasor"] = None

        if data["stems"].get("main_player"):
            with contextlib.suppress(builtins.BaseException):
                data["stems"]["main_player"].stop()
            data["stems"]["main_player"] = None
        data["stems"]["main_table"] = None

        for stem in STEM_NAMES:
            player = data["stems"]["players"].get(stem)
            if player:
                with contextlib.suppress(builtins.BaseException):
                    player.stop()
                data["stems"]["players"][stem] = None
            data["stems"]["tables"][stem] = None
            data["stems"]["outputs"][stem] = None
            if data["stems"]["gains"].get(stem):
                with contextlib.suppress(builtins.BaseException):
                    data["stems"]["gains"][stem].stop()
                data["stems"]["gains"][stem] = None
            data["stems"]["dry"][stem] = None
            data["stems"]["pitched"][stem] = None

        if data["stems"].get("main_gain"):
            with contextlib.suppress(builtins.BaseException):
                data["stems"]["main_gain"].stop()
            data["stems"]["main_gain"] = None

        data["stems"]["initialized"] = False
        data["stems"]["available"] = False
        gc.collect()

    # Setze generating Flag
    data["stems"]["generating"] = True

    # Visuelles Feedback
    original_bg = buttons[button_id].cget("bg")
    buttons[button_id].config(bg="#ff8800")

    def do_generate():
        try:
            # Import demucs here to avoid import errors if not installed
            from demucs.apply import apply_model
            from demucs.pretrained import get_model

            loop = data.get("pyo")
            if not loop or loop._audio_data is None:
                msg = "Audio data not loaded"
                raise Exception(msg)

            # Audio-Segment extrahieren (Loop-Bereich)
            audio_data = loop._audio_data
            audio_sr = loop._audio_sr
            loop_start = data.get("loop_start", 0.0)
            loop_end = data.get("loop_end", loop._duration)

            start_sample = int(loop_start * audio_sr)
            end_sample = int(loop_end * audio_sr)
            audio_segment = audio_data[start_sample:end_sample]

            # Stelle sicher, dass Audio float32 ist
            if audio_segment.dtype != np.float32:
                audio_segment = audio_segment.astype(np.float32)

            # Demucs für Separation verwenden

            # Modell laden (htdemucs_6s hat: vocals, drums, bass, guitar, piano, other)
            # Wir verwenden htdemucs für 4-stem: vocals, drums, bass, other
            model = get_model("htdemucs")
            model.eval()

            # Audio vorbereiten für demucs
            # Erwartet: (batch, channels, samples)
            if len(audio_segment.shape) == 1:
                # Mono -> Stereo
                audio_tensor = torch.from_numpy(audio_segment).unsqueeze(0).repeat(2, 1)
            else:
                # (samples, channels) -> (channels, samples)
                audio_tensor = torch.from_numpy(audio_segment.T)

            # Batch-Dimension hinzufügen
            audio_tensor = audio_tensor.unsqueeze(0)

            # Resample falls nötig (demucs erwartet 44100)
            if audio_sr != 44100:
                resampler = torchaudio.transforms.Resample(audio_sr, 44100)
                audio_tensor = resampler(audio_tensor)

            # Separation durchführen
            with torch.no_grad():
                sources = apply_model(model, audio_tensor, device="cpu", progress=False)

            # sources shape: (batch, sources, channels, samples)
            # htdemucs sources: drums, bass, other, vocals
            sources = sources.squeeze(0).numpy()

            # Stems extrahieren (htdemucs: drums=0, bass=1, other=2, vocals=3)
            drums = sources[0].T  # (samples, channels)
            bass = sources[1].T
            other = sources[2].T  # "melody" - enthält Melodie-Instrumente
            vocals = sources[3].T

            # Instrumental = alles außer Vocals (drums + bass + other)
            instrumental = drums + bass + other

            # Stems als float32 speichern
            stems_dry = {
                "vocals": vocals.astype(np.float32),
                "melody": other.astype(np.float32),
                "bass": bass.astype(np.float32),
                "drums": drums.astype(np.float32),
                "instrumental": instrumental.astype(np.float32),
            }

            # GUI Update
            def update_gui():
                data["stems"]["dry"] = stems_dry
                data["stems"]["available"] = True
                data["stems"]["generating"] = False
                buttons[button_id].config(bg=original_bg)
                update_button_label_callback(button_id)
                update_stem_buttons_state_callback()
                save_config_async_callback()
                # Keine MessageBox mehr - stört Live-Performance

            schedule_gui_update(update_gui)

        except ImportError:

            def show_error():
                data["stems"]["generating"] = False
                buttons[button_id].config(bg=original_bg)
                messagebox.showerror(
                    "Missing Dependencies",
                    f"Please install required packages:\npip install demucs torch torchaudio\n\nError: {e}",
                )

            schedule_gui_update(show_error)
        except Exception:

            def show_error():
                data["stems"]["generating"] = False
                buttons[button_id].config(bg=original_bg)
                messagebox.showerror("Stem Generation Failed", f"Error: {e}")
                logger.error(f"Stem generation failed: {e}")

            schedule_gui_update(show_error)

    io_executor.submit(do_generate)


def delete_stems(
    button_id,
    update_button_label_callback,
    update_stem_buttons_state_callback,
    save_config_async_callback,
):
    """Löscht alle Stems eines Loops und gibt RAM frei.
    WICHTIG: Explizit alle Referenzen auf None setzen für Garbage Collection!

    Args:
        button_id: The button ID to delete stems for
        update_button_label_callback: Callback to update button label
        update_stem_buttons_state_callback: Callback to update stem button states
        save_config_async_callback: Callback to save config asynchronously
    """
    button_data = get_button_data()
    data = button_data[button_id]

    # Stems-Struktur sicherstellen
    ensure_stems_structure(data)

    # Master-Phasor stoppen und freigeben
    if data["stems"].get("master_phasor"):
        with contextlib.suppress(builtins.BaseException):
            data["stems"]["master_phasor"].stop()
        data["stems"]["master_phasor"] = None

    # Alle Stem-Player stoppen und Referenzen explizit freigeben
    for stem in STEM_NAMES:
        # Player stoppen
        player = data["stems"]["players"].get(stem)
        if player:
            with contextlib.suppress(builtins.BaseException):
                player.stop()
            data["stems"]["players"][stem] = None

        # Table freigeben
        table = data["stems"]["tables"].get(stem)
        if table:
            data["stems"]["tables"][stem] = None

        # Gain-Signal freigeben
        gain = data["stems"]["gains"].get(stem)
        if gain:
            with contextlib.suppress(builtins.BaseException):
                gain.stop()
            data["stems"]["gains"][stem] = None

        # Output freigeben
        data["stems"]["outputs"][stem] = None

        # WICHTIG: Numpy Arrays explizit freigeben
        data["stems"]["dry"][stem] = None
        data["stems"]["pitched"][stem] = None

    # Main-Gain freigeben
    if data["stems"].get("main_gain"):
        with contextlib.suppress(builtins.BaseException):
            data["stems"]["main_gain"].stop()
        data["stems"]["main_gain"] = None

    # Main Player freigeben (synchroner Original-Player)
    if data["stems"].get("main_player"):
        with contextlib.suppress(builtins.BaseException):
            data["stems"]["main_player"].stop()
        data["stems"]["main_player"] = None
    data["stems"]["main_table"] = None

    # Master Phasor freigeben
    if data["stems"].get("master_phasor"):
        with contextlib.suppress(builtins.BaseException):
            data["stems"]["master_phasor"].stop()
        data["stems"]["master_phasor"] = None

    # PyoLoop stem_mute zurücksetzen
    loop = data.get("pyo")
    if loop:
        loop._stem_mute = None

    # Flags zurücksetzen
    data["stems"]["available"] = False
    data["stems"]["generating"] = False
    data["stems"]["initialized"] = False
    data["stems"]["cached_speed"] = None
    data["stems"]["stop_active"] = False
    data["stems"]["saved_states"] = None

    # Alle States zurücksetzen
    for stem in STEM_NAMES:
        data["stems"]["states"][stem] = False

    # Garbage Collection erzwingen
    gc.collect()

    # Wenn der Loop noch läuft, PyoLoop wieder aktivieren
    loop = data.get("pyo")
    if loop and data.get("active"):
        # Stoppe und starte den Loop neu, damit er wieder hörbar ist
        loop._stem_mute = None
        # EQ-Kette neu aufbauen ohne stem_mute
        if loop._is_playing:
            loop._create_eq_chain()
            if loop.output:
                loop.output.out()

    # GUI Update
    update_button_label_callback(button_id)
    update_stem_buttons_state_callback()
    save_config_async_callback()
