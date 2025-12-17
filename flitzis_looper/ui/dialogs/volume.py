"""flitzis_looper.ui.dialogs.volume - Volume + EQ Dialog.

Enthält set_volume() Funktion für Gain-Slider + VU-Meter + EQ-Knobs.
"""

import tkinter as tk

from flitzis_looper.core.state import (
    COLOR_BG,
    COLOR_TEXT,
    get_button_data,
    get_open_volume_windows,
    get_root,
    register_open_volume_window,
    unregister_open_volume_window,
)
from flitzis_looper.ui.widgets import EQKnob, VUMeter
from flitzis_looper.utils.logging import logger


def set_volume(button_id, update_stem_eq_callback=None, save_config_async_callback=None):
    """Öffnet den Volume + EQ Dialog für einen Button.

    Args:
        button_id: ID des Buttons
        update_stem_eq_callback: Callback für Stem-EQ Updates (optional)
        save_config_async_callback: Callback für Config-Speicherung (optional)
    """
    root = get_root()
    button_data = get_button_data()
    open_volume_windows = get_open_volume_windows()

    if button_id in open_volume_windows:
        existing_window = open_volume_windows[button_id]
        try:
            if existing_window.winfo_exists():
                existing_window.lift()
                existing_window.focus_force()
                return
        except tk.TclError:
            pass
        unregister_open_volume_window(button_id)

    try:
        vol_win = tk.Toplevel(root)
        vol_win.title(f"Volume + EQ - Button {button_id}")
        vol_win.configure(bg=COLOR_BG)
        vol_win.geometry("600x280")
        vol_win.resizable(width=False, height=False)

        register_open_volume_window(button_id, vol_win)

        root.update_idletasks()
        x = root.winfo_x() + (root.winfo_width() // 2) - 300
        y = root.winfo_y() + (root.winfo_height() // 2) - 140
        vol_win.geometry(f"+{x}+{y}")

        main_frame = tk.Frame(vol_win, bg=COLOR_BG)
        main_frame.pack(fill="both", expand=True, padx=10, pady=10)

        vu_frame = tk.Frame(main_frame, bg=COLOR_BG)
        vu_frame.pack(side="left", padx=(0, 15))

        vu_label = tk.Label(vu_frame, text="Level", fg=COLOR_TEXT, bg=COLOR_BG, font=("Arial", 8))
        vu_label.pack()

        vu_meter = VUMeter(vu_frame, width=25, height=200)
        vu_meter.canvas.pack()

        controls_frame = tk.Frame(main_frame, bg=COLOR_BG)
        controls_frame.pack(side="left", fill="both", expand=True)

        current_db = button_data[button_id]["gain_db"]

        update_scheduled = [None]

        def on_slide(val):
            try:
                value = round(float(val), 1)

                if update_scheduled[0]:
                    root.after_cancel(update_scheduled[0])

                def do_update():
                    button_data[button_id]["gain_db"] = value
                    loop = button_data[button_id].get("pyo")
                    if loop:
                        loop.set_gain(value)
                    if save_config_async_callback:
                        save_config_async_callback()

                update_scheduled[0] = root.after(50, do_update)

            except Exception as e:
                logger.error(f"Error setting volume: {e}")

        # Custom Label-Formatierung für den Slider (zeigt "X.X dB")
        class GainScale(tk.Scale):
            def __init__(self, *args, **kwargs):
                super().__init__(*args, **kwargs)

            def set(self, value):
                super().set(value)

        scale = tk.Scale(
            controls_frame,
            from_=-20.0,
            to=20.0,
            resolution=0.1,
            orient="horizontal",
            length=400,
            width=30,
            command=on_slide,
            bg=COLOR_BG,
            fg=COLOR_TEXT,
            troughcolor="#444",
            highlightthickness=0,
            showvalue=False,
        )
        scale.set(current_db)
        scale.pack(pady=(10, 0))

        # dB-Label das dem Slider-Wert folgt
        def update_db_label(*args):
            val = scale.get()
            db_label.config(text=f"{val:.1f} dB")
            # Position des Labels aktualisieren
            slider_length = 400
            slider_min = -20.0
            slider_max = 20.0
            # Berechne relative Position (0-1)
            rel_pos = (val - slider_min) / (slider_max - slider_min)
            # Pixel-Position (mit etwas Offset für den Slider-Rand)
            x_pos = int(15 + rel_pos * (slider_length - 10))
            db_label.place(x=x_pos, y=20, anchor="s")

        db_label_frame = tk.Frame(controls_frame, bg=COLOR_BG, height=30, width=430)
        db_label_frame.pack()
        db_label_frame.pack_propagate(flag=False)

        db_label = tk.Label(
            db_label_frame,
            text=f"{current_db:.1f} dB",
            fg=COLOR_TEXT,
            bg=COLOR_BG,
            font=("Arial", 9, "bold"),
        )
        db_label.place(x=215, y=20, anchor="s")

        # Original on_slide erweitern
        original_on_slide = on_slide

        def on_slide_with_label(val):
            original_on_slide(val)
            update_db_label()

        scale.config(command=on_slide_with_label)
        update_db_label()

        midpoint = tk.Label(
            controls_frame,
            text="<< quieter       normal       louder >>",
            fg="#888",
            bg=COLOR_BG,
            font=("Arial", 8),
        )
        midpoint.pack()

        reset_vol_btn = tk.Button(
            controls_frame,
            text="Reset to 0 dB",
            command=lambda: scale.set(0.0),
            bg="#444",
            fg=COLOR_TEXT,
        )
        reset_vol_btn.pack(pady=5)

        eq_frame = tk.Frame(controls_frame, bg=COLOR_BG)
        eq_frame.pack(pady=(15, 0))

        tk.Label(eq_frame, text="EQ", fg="#ffaa00", bg=COLOR_BG, font=("Arial", 10, "bold")).pack()

        eq_knobs_frame = tk.Frame(eq_frame, bg=COLOR_BG)
        eq_knobs_frame.pack(pady=5)

        eq_low_knob = EQKnob(eq_knobs_frame, "LOW", width=50, height=70)
        eq_low_knob.set_value(button_data[button_id].get("eq_low", 0.0))
        eq_low_knob.frame.pack(side="left", padx=5)

        eq_mid_knob = EQKnob(eq_knobs_frame, "MID", width=50, height=70)
        eq_mid_knob.set_value(button_data[button_id].get("eq_mid", 0.0))
        eq_mid_knob.frame.pack(side="left", padx=5)

        eq_high_knob = EQKnob(eq_knobs_frame, "HIGH", width=50, height=70)
        eq_high_knob.set_value(button_data[button_id].get("eq_high", 0.0))
        eq_high_knob.frame.pack(side="left", padx=5)

        # OPTIMIERUNG: EQ nur updaten wenn sich Werte ändern
        last_eq_values = [
            button_data[button_id].get("eq_low", 0.0),
            button_data[button_id].get("eq_mid", 0.0),
            button_data[button_id].get("eq_high", 0.0),
        ]

        def update_eq():
            try:
                low = eq_low_knob.get_value()
                mid = eq_mid_knob.get_value()
                high = eq_high_knob.get_value()

                # OPTIMIERUNG: Nur updaten wenn sich Werte geändert haben
                if (
                    abs(low - last_eq_values[0]) > 0.001
                    or abs(mid - last_eq_values[1]) > 0.001
                    or abs(high - last_eq_values[2]) > 0.001
                ):
                    last_eq_values[0] = low
                    last_eq_values[1] = mid
                    last_eq_values[2] = high

                    button_data[button_id]["eq_low"] = low
                    button_data[button_id]["eq_mid"] = mid
                    button_data[button_id]["eq_high"] = high

                    loop = button_data[button_id].get("pyo")
                    if loop:
                        loop.set_eq(low, mid, high)

                    # STEMS: Auch Stem-EQs aktualisieren
                    if update_stem_eq_callback:
                        update_stem_eq_callback(button_id, low, mid, high)

                if vol_win.winfo_exists():
                    vol_win.after(50, update_eq)
            except tk.TclError:
                pass  # Fenster wurde geschlossen

        update_eq()

        loop = button_data[button_id].get("pyo")
        if loop:
            loop.enable_level_meter()

        def update_vu_meter():
            try:
                if vol_win.winfo_exists():
                    loop = button_data[button_id].get("pyo")
                    if loop and button_data[button_id]["active"]:
                        db_level = loop.get_level_db()
                        vu_meter.update_level(db_level)
                    else:
                        vu_meter.update_level(-80)
                    vol_win.after(50, update_vu_meter)
            except tk.TclError:
                pass
            except Exception as e:
                logger.error(f"Error updating VU meter: {e}")

        def on_volume_window_close():
            button_data[button_id]["eq_low"] = eq_low_knob.get_value()
            button_data[button_id]["eq_mid"] = eq_mid_knob.get_value()
            button_data[button_id]["eq_high"] = eq_high_knob.get_value()
            if save_config_async_callback:
                save_config_async_callback()

            loop = button_data[button_id].get("pyo")
            if loop:
                loop.disable_level_meter()

            unregister_open_volume_window(button_id)

            vol_win.destroy()

        vol_win.protocol("WM_DELETE_WINDOW", on_volume_window_close)
        update_vu_meter()

    except Exception as e:
        logger.error(f"Error opening volume control: {e}")
