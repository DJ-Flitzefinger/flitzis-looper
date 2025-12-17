"""flitzis_looper.ui.dialogs.waveform - WaveformEditor Dialog.

Enthält WaveformEditor Klasse für Loop-Punkte setzen, Intro-Marker, etc.
"""

import os
import tkinter as tk

import matplotlib.pyplot as plt
import numpy as np
import soundfile as sf
from matplotlib.backends.backend_tkagg import FigureCanvasTkAgg
from matplotlib.figure import Figure

from flitzis_looper.core.state import (
    COLOR_BG,
    COLOR_BTN_ACTIVE,
    COLOR_BTN_INACTIVE,
    COLOR_LOCK_OFF,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    get_all_banks_data,
    get_button_data,
    get_buttons,
    get_current_bank,
    get_key_lock_active,
    get_loaded_loops,
    get_multi_loop_active,
    get_open_loop_editor_windows,
    register_open_loop_editor_window,
    unregister_open_loop_editor_window,
)
from flitzis_looper.utils.logging import logger
from flitzis_looper.utils.threading import io_executor, schedule_gui_update


class WaveformEditor:  # noqa: PLR0904
    """Waveform Editor Dialog für Loop-Punkte setzen und Intro-Marker."""

    def __init__(
        self, parent, button_id, update_button_label_callback=None, save_config_async_callback=None
    ):
        """Init function.

        Args:
        parent: Tk Parent Widget (root)
        button_id: ID des zu bearbeitenden Buttons
        update_button_label_callback: Callback für Label-Updates
        save_config_async_callback: Callback für Config-Speicherung.
        """
        self.update_button_label_callback = update_button_label_callback
        self.save_config_async_callback = save_config_async_callback

        button_data = get_button_data()
        open_loop_editor_windows = get_open_loop_editor_windows()

        if button_id in open_loop_editor_windows:
            existing_editor = open_loop_editor_windows[button_id]
            try:
                if existing_editor.window.winfo_exists():
                    existing_editor.window.lift()
                    existing_editor.window.focus_force()
                    return
            except (tk.TclError, AttributeError):
                pass
            unregister_open_loop_editor_window(button_id)

        self.parent = parent
        self.button_id = button_id
        self.audio_data = None
        self.sample_rate = None
        self.duration = 0
        self.loop_start = button_data[button_id].get("loop_start", 0.0)
        self.loop_end = button_data[button_id].get("loop_end", None)
        self.zoom_start = 0.0
        self.zoom_end = None
        self._loading = True

        # Ursprüngliche Werte speichern für UNDO
        self._original_loop_start = self.loop_start
        self._original_loop_end = self.loop_end
        self._original_auto_loop_active = button_data[button_id].get("auto_loop_active", True)
        self._original_auto_loop_bars = button_data[button_id].get("auto_loop_bars", 8)
        self._original_auto_loop_custom_mode = button_data[button_id].get(
            "auto_loop_custom_mode", False
        )
        self._original_intro_active = button_data[button_id].get("intro_active", False)
        self._original_intro_bars = button_data[button_id].get("intro_bars", 4)
        self._original_intro_custom_mode = button_data[button_id].get("intro_custom_mode", False)

        self.auto_loop_active = tk.BooleanVar(
            value=button_data[button_id].get("auto_loop_active", True)
        )
        self.auto_loop_bars = tk.IntVar(value=button_data[button_id].get("auto_loop_bars", 8))
        self.auto_loop_custom_mode = tk.BooleanVar(
            value=button_data[button_id].get("auto_loop_custom_mode", False)
        )

        # Intro Variablen
        self.intro_active = tk.BooleanVar(value=button_data[button_id].get("intro_active", False))
        self.intro_bars = tk.DoubleVar(value=button_data[button_id].get("intro_bars", 4))
        self.intro_custom_mode = tk.BooleanVar(
            value=button_data[button_id].get("intro_custom_mode", False)
        )

        self.waveform_cache = {}
        self.cache_levels = [1000, 5000, 20000, 50000]

        if self.setup_window():
            register_open_loop_editor_window(button_id, self)
            self.show_loading_message()
            self.load_audio_async()

    def setup_window(self):
        try:
            self.window = tk.Toplevel(self.parent)
            self.window.title(f"Loop Editor - Button {self.button_id}")
            self.window.configure(bg=COLOR_BG)
            self.window.geometry("900x650")
            self.window.protocol("WM_DELETE_WINDOW", self.on_window_close)
        except tk.TclError as e:
            logger.error(f"Error creating window: {e}")
            return False

        return True

    def show_loading_message(self):
        self.loading_label = tk.Label(
            self.window, text="Loading waveform", fg="#ffff00", bg=COLOR_BG, font=("Arial", 14)
        )
        self.loading_label.pack(expand=True)

    def load_audio_async(self):
        button_data = get_button_data()

        def do_load():
            try:
                filepath = button_data[self.button_id]["file"]
                if not os.path.exists(filepath):
                    schedule_gui_update(lambda: self.on_load_error("File not found"))
                    return

                # OPTIMIERUNG: Prüfe ob Waveform-Cache vorhanden ist
                cached_waveform = button_data[self.button_id].get("waveform_cache")
                if cached_waveform is not None:
                    # Cache verwenden
                    audio_data = cached_waveform["audio_data"]
                    sample_rate = cached_waveform["sample_rate"]
                    duration = cached_waveform["duration"]
                    waveform_cache = cached_waveform["cache"]
                    schedule_gui_update(
                        lambda: self.on_load_complete(
                            audio_data, sample_rate, duration, waveform_cache
                        )
                    )
                    return

                audio_data, sample_rate = sf.read(filepath)
                if len(audio_data) == 0:
                    schedule_gui_update(lambda: self.on_load_error("Empty file"))
                    return
                if len(audio_data.shape) > 1:
                    audio_data = np.mean(audio_data, axis=1)
                duration = len(audio_data) / sample_rate

                waveform_cache = {}
                for target_samples in self.cache_levels:
                    if len(audio_data) > target_samples:
                        factor = len(audio_data) // target_samples
                        downsampled = audio_data[::factor]
                        time_axis = np.linspace(0, duration, len(downsampled))
                        waveform_cache[target_samples] = (downsampled, time_axis)
                    else:
                        time_axis = np.linspace(0, duration, len(audio_data))
                        waveform_cache[target_samples] = (audio_data.copy(), time_axis)

                # OPTIMIERUNG: Waveform-Cache in button_data speichern
                button_data[self.button_id]["waveform_cache"] = {
                    "audio_data": audio_data,
                    "sample_rate": sample_rate,
                    "duration": duration,
                    "cache": waveform_cache,
                }

                schedule_gui_update(
                    lambda: self.on_load_complete(audio_data, sample_rate, duration, waveform_cache)
                )
            except Exception as e:
                err_msg = str(e)
                schedule_gui_update(lambda: self.on_load_error(err_msg))

        io_executor.submit(do_load)

    def on_load_complete(self, audio_data, sample_rate, duration, waveform_cache):
        self.audio_data = audio_data
        self.sample_rate = sample_rate
        self.duration = duration
        self.zoom_end = duration
        self.waveform_cache = waveform_cache
        self._loading = False
        if self.loop_end is None:
            self.loop_end = self.duration
        self.loading_label.destroy()
        self.create_controls()
        self.create_waveform()
        self.update_auto_loop_display()
        self.update_intro_display()
        self.update_play_button()

    def on_load_error(self, error_msg):
        self.loading_label.config(text=f"Error: {error_msg}")

    def create_controls(self):
        control_frame = tk.Frame(self.window, bg=COLOR_BG)
        control_frame.pack(fill="x", padx=10, pady=5)

        self.play_btn = tk.Button(
            control_frame, text="> Play", command=self.toggle_playback, bg="#444", fg=COLOR_TEXT
        )
        self.play_btn.pack(side="left", padx=5)

        tk.Button(
            control_frame, text="Reset", command=self.reset_loop, bg="#444", fg=COLOR_TEXT
        ).pack(side="left", padx=5)
        tk.Button(control_frame, text="Fit", command=self.zoom_fit, bg="#444", fg=COLOR_TEXT).pack(
            side="left", padx=5
        )
        tk.Button(
            control_frame, text="|<", command=self.jump_to_start, bg="#444", fg=COLOR_TEXT, width=3
        ).pack(side="left", padx=2)
        tk.Button(
            control_frame, text=">|", command=self.jump_to_end, bg="#444", fg=COLOR_TEXT, width=3
        ).pack(side="left", padx=2)
        tk.Button(
            control_frame,
            text="+",
            command=lambda: self.zoom_by_factor(0.5),
            bg="#444",
            fg=COLOR_TEXT,
            width=3,
        ).pack(side="left", padx=2)
        tk.Button(
            control_frame,
            text="-",
            command=lambda: self.zoom_by_factor(2.0),
            bg="#444",
            fg=COLOR_TEXT,
            width=3,
        ).pack(side="left", padx=2)

        # UNDO und APPLY Buttons in der Mitte
        tk.Button(
            control_frame,
            text="UNDO",
            command=self.undo_and_close,
            bg=COLOR_LOCK_OFF,
            fg=COLOR_TEXT,
            width=6,
        ).pack(side="left", padx=(20, 2))
        tk.Button(
            control_frame,
            text="APPLY",
            command=self.apply_and_close,
            bg=COLOR_BTN_ACTIVE,
            fg=COLOR_TEXT_ACTIVE,
            width=6,
        ).pack(side="left", padx=2)

        auto_frame = tk.Frame(control_frame, bg=COLOR_BG)
        auto_frame.pack(side="right", padx=10)

        self.auto_loop_cb = tk.Checkbutton(
            auto_frame,
            text="Auto-Loop",
            variable=self.auto_loop_active,
            bg=COLOR_BG,
            fg=COLOR_TEXT,
            selectcolor="#444",
            command=self.on_auto_loop_toggle,
        )
        self.auto_loop_cb.pack(side="left")

        tk.Button(
            auto_frame, text="-", width=2, command=self.bars_down, bg="#444", fg=COLOR_TEXT
        ).pack(side="left", padx=2)
        self.bars_label = tk.Label(auto_frame, text="8 bars", fg=COLOR_TEXT, bg=COLOR_BG, width=7)
        self.bars_label.pack(side="left")
        tk.Button(
            auto_frame, text="+", width=2, command=self.bars_up, bg="#444", fg=COLOR_TEXT
        ).pack(side="left", padx=2)

        self.custom_cb = tk.Checkbutton(
            auto_frame,
            text="Custom",
            variable=self.auto_loop_custom_mode,
            bg=COLOR_BG,
            fg=COLOR_TEXT,
            selectcolor="#444",
            command=self.on_custom_mode_toggle,
        )
        self.custom_cb.pack(side="left", padx=(10, 0))

        # Intro-Zeile (zweite Reihe rechts)
        intro_frame = tk.Frame(self.window, bg=COLOR_BG)
        intro_frame.pack(fill="x", padx=10, pady=(0, 5))

        # Spacer links um Intro-Controls rechtsbündig zu machen
        intro_spacer = tk.Frame(intro_frame, bg=COLOR_BG)
        intro_spacer.pack(side="left", expand=True, fill="x")

        intro_controls = tk.Frame(intro_frame, bg=COLOR_BG)
        intro_controls.pack(side="right", padx=10)

        self.intro_cb = tk.Checkbutton(
            intro_controls,
            text="Intro",
            variable=self.intro_active,
            bg=COLOR_BG,
            fg="#ffff00",
            selectcolor="#444",
            activeforeground="#ffff00",
            command=self.on_intro_toggle,
        )
        self.intro_cb.pack(side="left")

        tk.Button(
            intro_controls,
            text="-",
            width=2,
            command=self.intro_bars_down,
            bg="#444",
            fg=COLOR_TEXT,
        ).pack(side="left", padx=2)
        self.intro_bars_label = tk.Label(
            intro_controls, text="4 bars", fg="#ffff00", bg=COLOR_BG, width=7
        )
        self.intro_bars_label.pack(side="left")
        tk.Button(
            intro_controls, text="+", width=2, command=self.intro_bars_up, bg="#444", fg=COLOR_TEXT
        ).pack(side="left", padx=2)

        self.intro_custom_cb = tk.Checkbutton(
            intro_controls,
            text="Custom",
            variable=self.intro_custom_mode,
            bg=COLOR_BG,
            fg="#ffff00",
            selectcolor="#444",
            activeforeground="#ffff00",
            command=self.on_intro_custom_mode_toggle,
        )
        self.intro_custom_cb.pack(side="left", padx=(10, 0))

        self.time_label = tk.Label(self.window, text="", fg=COLOR_TEXT, bg=COLOR_BG)
        self.time_label.pack(pady=5)

    def create_waveform(self):
        self.fig = Figure(figsize=(10, 5), dpi=100, facecolor="#1e1e1e")
        self.ax = self.fig.add_subplot(111)
        self.ax.set_facecolor("#1e1e1e")
        self.canvas = FigureCanvasTkAgg(self.fig, self.window)
        self.canvas.get_tk_widget().pack(fill="both", expand=True, padx=10, pady=10)
        self.canvas.mpl_connect("button_press_event", self.on_click)
        self.canvas.mpl_connect("scroll_event", self.on_scroll)
        self._waveform_update_pending = None
        self.update_waveform()

    def get_cached_waveform(self, visible_samples):
        for level in sorted(self.cache_levels, reverse=True):
            if visible_samples <= level * 2:
                return self.waveform_cache.get(level, (self.audio_data, None))
        return self.waveform_cache.get(self.cache_levels[-1], (self.audio_data, None))

    def update_waveform_throttled(self):
        if self._waveform_update_pending:
            self.window.after_cancel(self._waveform_update_pending)
        self._waveform_update_pending = self.window.after(30, self.update_waveform)

    def update_waveform(self):
        if self._loading or self.audio_data is None:
            return
        self.ax.clear()
        start_idx = max(0, int(self.zoom_start * self.sample_rate))
        end_idx = min(len(self.audio_data), int(self.zoom_end * self.sample_rate))
        visible_samples = end_idx - start_idx
        cached_data, cached_time = self.get_cached_waveform(visible_samples)
        mask = (cached_time >= self.zoom_start) & (cached_time <= self.zoom_end)
        zoomed_data = cached_data[mask] if np.any(mask) else cached_data
        zoomed_time = cached_time[mask] if np.any(mask) else cached_time

        self.ax.plot(zoomed_time, zoomed_data, color="#00ff00", linewidth=0.5)
        self.ax.fill_between(zoomed_time, zoomed_data, alpha=0.3, color="#00ff00")

        # Keep y=0 centered by enforcing symmetric y-limits around 0.
        max_abs = float(np.max(np.abs(zoomed_data))) if len(zoomed_data) else 1.0
        max_abs = max(max_abs, 1e-6)
        self.ax.set_ylim(-max_abs, max_abs)

        # Intro-Linie und Highlight zeichnen (gelb/orange)
        intro_start = self.calculate_intro_start_position()
        if intro_start is not None:
            # Gelbe gestrichelte Linie für Intro-Start
            if self.zoom_start <= intro_start <= self.zoom_end:
                self.ax.axvline(x=intro_start, color="#ffff00", linewidth=2, linestyle="--")

            # Orange Highlight für Intro-Bereich (zwischen Intro-Start und Loop-Start)
            if (
                intro_start < self.loop_start
                and intro_start < self.zoom_end
                and self.loop_start > self.zoom_start
            ):
                hl_intro_start = max(intro_start, self.zoom_start)
                hl_intro_end = min(self.loop_start, self.zoom_end)
                self.ax.axvspan(hl_intro_start, hl_intro_end, alpha=0.15, color="#ff8800")

        # Loop Start/End Linien
        if self.zoom_start <= self.loop_start <= self.zoom_end:
            self.ax.axvline(x=self.loop_start, color="#00ffff", linewidth=2)
        if self.zoom_start <= self.loop_end <= self.zoom_end:
            self.ax.axvline(x=self.loop_end, color="#ff0000", linewidth=2)

        # Loop-Highlight (gelb)
        if self.loop_start < self.zoom_end and self.loop_end > self.zoom_start:
            hl_start = max(self.loop_start, self.zoom_start)
            hl_end = min(self.loop_end, self.zoom_end)
            self.ax.axvspan(hl_start, hl_end, alpha=0.2, color="#ffff00")

        self.ax.set_xlim(self.zoom_start, self.zoom_end)
        self.ax.set_xlabel("Time (s)", color=COLOR_TEXT)
        self.ax.tick_params(colors=COLOR_TEXT)
        self.ax.grid(visible=True, alpha=0.3)
        self.fig.tight_layout()
        self.canvas.draw_idle()

        # Info-Label aktualisieren
        loop_dur = self.loop_end - self.loop_start
        bpm = self.get_bpm()
        bars = loop_dur / self.calculate_bar_duration() if bpm > 0 else 0

        # Intro-Info hinzufügen wenn aktiv
        intro_text = ""
        if intro_start is not None:
            intro_bars = self.intro_bars.get()
            # Schöne Darstellung: ganze Zahlen ohne Dezimalpunkt
            if intro_bars == int(intro_bars):
                intro_text = f" | Intro: {intro_start:.3f}s ({int(intro_bars)} bars)"
            else:
                intro_text = f" | Intro: {intro_start:.3f}s ({intro_bars:.3g} bars)"

        self.time_label.config(
            text=f"Loop: {self.loop_start:.3f}s - {self.loop_end:.3f}s | "
            f"Duration: {loop_dur:.3f}s | {bars:.1f} bars{intro_text}"
        )

    def get_bpm(self):
        button_data = get_button_data()
        return button_data[self.button_id].get("bpm", 120.0) or 120.0

    def calculate_bar_duration(self):
        bpm = self.get_bpm()
        return 4.0 / (bpm / 60.0)

    def calculate_auto_loop_duration(self):
        return self.calculate_bar_duration() * self.auto_loop_bars.get()

    def update_auto_loop_display(self):
        self.bars_label.config(text=f"{self.auto_loop_bars.get()} bars")

    def on_auto_loop_toggle(self):
        button_data = get_button_data()
        button_data[self.button_id]["auto_loop_active"] = self.auto_loop_active.get()
        if self.auto_loop_active.get():
            self.apply_auto_loop_to_current_settings()
        self.update_waveform()

    def on_custom_mode_toggle(self):
        button_data = get_button_data()
        button_data[self.button_id]["auto_loop_custom_mode"] = self.auto_loop_custom_mode.get()

    def apply_auto_loop_to_current_settings(self):
        if not self.auto_loop_active.get() or self._loading:
            return
        new_dur = self.calculate_auto_loop_duration()
        if self.loop_start + new_dur <= self.duration:
            self.loop_end = self.loop_start + new_dur
        else:
            self.loop_start = max(0, self.duration - new_dur)
            self.loop_end = self.duration
        self.apply_loop_changes_realtime()

    def bars_up(self):
        button_data = get_button_data()
        current = self.auto_loop_bars.get()
        if self.auto_loop_custom_mode.get():
            new_val = min(64, current + 1)
        else:
            valid = [4, 8, 16, 32, 64]
            idx = valid.index(current) if current in valid else 1
            new_val = valid[min(len(valid) - 1, idx + 1)]
        self.auto_loop_bars.set(new_val)
        button_data[self.button_id]["auto_loop_bars"] = new_val
        self.update_auto_loop_display()
        if self.auto_loop_active.get():
            self.apply_auto_loop_to_current_settings()
            self.update_waveform()

    def bars_down(self):
        button_data = get_button_data()
        current = self.auto_loop_bars.get()
        if self.auto_loop_custom_mode.get():
            new_val = max(1, current - 1)
        else:
            valid = [4, 8, 16, 32, 64]
            idx = valid.index(current) if current in valid else 1
            new_val = valid[max(0, idx - 1)]
        self.auto_loop_bars.set(new_val)
        button_data[self.button_id]["auto_loop_bars"] = new_val
        self.update_auto_loop_display()
        if self.auto_loop_active.get():
            self.apply_auto_loop_to_current_settings()
            self.update_waveform()

    # ===== INTRO METHODEN =====
    def calculate_intro_start_position(self):
        """Berechnet die Intro-Startposition basierend auf intro_bars."""
        if not self.intro_active.get():
            return None
        intro_duration = self.calculate_bar_duration() * self.intro_bars.get()
        intro_start = self.loop_start - intro_duration
        return max(0, intro_start)  # Nicht vor Trackstart

    def update_intro_display(self):
        """Aktualisiert das Intro-Bars Label."""
        bars = self.intro_bars.get()
        # Schöne Darstellung: ganze Zahlen ohne Dezimalpunkt, Brüche mit einer Stelle
        if bars == int(bars):
            self.intro_bars_label.config(text=f"{int(bars)} bars")
        else:
            self.intro_bars_label.config(text=f"{bars:.3g} bars")

    def on_intro_toggle(self):
        """Callback wenn Intro-Checkbox getoggelt wird."""
        button_data = get_button_data()
        button_data[self.button_id]["intro_active"] = self.intro_active.get()
        self.update_waveform()

    def on_intro_custom_mode_toggle(self):
        """Callback wenn Intro Custom-Mode getoggelt wird."""
        button_data = get_button_data()
        button_data[self.button_id]["intro_custom_mode"] = self.intro_custom_mode.get()

    def intro_bars_up(self):
        """Erhöht die Intro-Bars (Custom: 1/8 Bar Schritte, Normal: 1 Bar Schritte)."""
        button_data = get_button_data()
        current = self.intro_bars.get()
        new_val = min(
            64,
            current
            + (
                # Custom: 1/8 Bar Schritte (0.125)
                0.125
                if self.intro_custom_mode.get()
                # Normal: 1 Bar Schritte
                else 1
            ),
        )
        self.intro_bars.set(new_val)
        button_data[self.button_id]["intro_bars"] = new_val
        self.update_intro_display()
        self.update_waveform()

    def intro_bars_down(self):
        """Verringert die Intro-Bars (Custom: 1/8 Bar Schritte, Normal: 1 Bar Schritte)."""
        button_data = get_button_data()
        current = self.intro_bars.get()
        if self.intro_custom_mode.get():
            # Custom: 1/8 Bar Schritte (0.125), Minimum 0.125
            new_val = max(0.125, current - 0.125)
        else:
            # Normal: 1 Bar Schritte, Minimum 1
            new_val = max(1, current - 1)
        self.intro_bars.set(new_val)
        button_data[self.button_id]["intro_bars"] = new_val
        self.update_intro_display()
        self.update_waveform()

    def jump_to_start(self):
        self.zoom_start = 0.0
        self.zoom_end = min(self.duration, self.zoom_end - self.zoom_start)
        self.update_waveform()

    def jump_to_end(self):
        span = self.zoom_end - self.zoom_start
        self.zoom_end = self.duration
        self.zoom_start = max(0, self.duration - span)
        self.update_waveform()

    def on_click(self, event):
        if event.inaxes and event.xdata is not None:
            click_time = float(event.xdata)

            if event.button == 2:
                self.jump_to_position_smart(click_time)
                return

            if self.auto_loop_active.get():
                loop_dur = self.calculate_auto_loop_duration()

                if event.button == 1:
                    new_start = click_time
                    new_end = new_start + loop_dur

                    if new_end > self.duration:
                        new_end = self.duration
                        new_start = max(0, new_end - loop_dur)

                    self.loop_start = new_start
                    self.loop_end = new_end

                elif event.button == 3:
                    new_end = click_time
                    new_start = new_end - loop_dur

                    if new_start < 0:
                        new_start = 0
                        new_end = min(self.duration, loop_dur)

                    self.loop_start = new_start
                    self.loop_end = new_end
            elif event.button == 1:
                self.loop_start = max(0, min(click_time, self.duration))
                if self.loop_start >= self.loop_end:
                    self.loop_end = min(self.duration, self.loop_start + 0.1)
            elif event.button == 3:
                self.loop_end = max(0, min(click_time, self.duration))
                if self.loop_end <= self.loop_start:
                    self.loop_start = max(0, self.loop_end - 0.1)

            self.apply_loop_changes_realtime()
            self.update_waveform()

    def jump_to_position_smart(self, click_pos):
        """Smart jump: plays to loop end, then loops properly."""
        button_data = get_button_data()
        loop = button_data[self.button_id]["pyo"]
        if not loop or not loop._is_playing:
            return

        try:
            original_start = self.loop_start
            original_end = self.loop_end

            if click_pos < original_start or (
                click_pos >= original_start and click_pos < original_end
            ):
                temp_start = click_pos
                temp_end = original_end
            else:
                temp_start = click_pos
                temp_end = self.duration

            loop.stop()
            loop.update_loop_points(temp_start, temp_end)
            loop.play()

            duration_until_loop = temp_end - temp_start
            speed = loop._pending_speed or 1.0
            wait_ms = int((duration_until_loop / abs(speed)) * 1000)

            def restore_loop():
                if loop._is_playing:
                    loop.stop()
                    loop.update_loop_points(original_start, original_end)
                    loop.play()

            self.window.after(wait_ms, restore_loop)

        except Exception as e:
            logger.error(f"Error in smart jump: {e}")

    def apply_loop_changes_realtime(self):
        button_data = get_button_data()
        button_data[self.button_id]["loop_start"] = self.loop_start
        button_data[self.button_id]["loop_end"] = self.loop_end
        loop = button_data[self.button_id]["pyo"]
        if loop:
            loop.update_loop_points(self.loop_start, self.loop_end)

    def on_scroll(self, event):
        if event.inaxes and event.xdata:
            mouse_time = event.xdata
            zoom_factor = 0.8 if event.button == "up" else 1.25
            current_span = self.zoom_end - self.zoom_start
            new_span = current_span * zoom_factor
            mouse_ratio = (mouse_time - self.zoom_start) / current_span
            new_start = mouse_time - (new_span * mouse_ratio)
            new_end = new_start + new_span
            self.zoom_start = max(0, new_start)
            self.zoom_end = min(self.duration, new_end)
            self.update_waveform_throttled()

    def zoom_by_factor(self, factor):
        center = (self.zoom_start + self.zoom_end) / 2
        span = (self.zoom_end - self.zoom_start) * factor
        self.zoom_start = max(0, center - span / 2)
        self.zoom_end = min(self.duration, center + span / 2)
        self.update_waveform()

    def zoom_fit(self):
        self.zoom_start = 0.0
        self.zoom_end = self.duration
        self.update_waveform()

    def reset_loop(self):
        self.loop_start = 0.0
        self.loop_end = self.duration
        self.apply_loop_changes_realtime()
        self.update_waveform()

    def toggle_playback(self):
        """Startet oder stoppt die Wiedergabe, synchronisiert mit dem Haupt-Button."""
        button_data = get_button_data()
        buttons = get_buttons()
        all_banks_data = get_all_banks_data()
        loaded_loops = get_loaded_loops()
        current_bank = get_current_bank()
        multi_loop_active = get_multi_loop_active()
        key_lock_active = get_key_lock_active()

        loop = button_data[self.button_id]["pyo"]
        if loop:
            if button_data[self.button_id]["active"]:
                # Stop
                loop.stop()
                button_data[self.button_id]["active"] = False
                buttons[self.button_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                if self.update_button_label_callback:
                    self.update_button_label_callback(self.button_id)
            else:
                # Play
                if not multi_loop_active.get():
                    # OPTIMIERUNG: Nur loaded_loops durchsuchen
                    for (bank_id, btn_id), other_loop in list(loaded_loops.items()):
                        if btn_id != self.button_id:
                            data = all_banks_data[bank_id][btn_id]
                            if data["active"]:
                                other_loop.stop()
                            data["active"] = False
                            if bank_id == current_bank.get():
                                buttons[btn_id].config(bg=COLOR_BTN_INACTIVE, fg=COLOR_TEXT)
                                if self.update_button_label_callback:
                                    self.update_button_label_callback(btn_id)
                loop.set_key_lock(key_lock_active.get())
                loop.play()
                button_data[self.button_id]["active"] = True
                buttons[self.button_id].config(bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
                if self.update_button_label_callback:
                    self.update_button_label_callback(self.button_id)
            self.update_play_button()

    def update_play_button(self):
        """Aktualisiert die Farbe des Play-Buttons basierend auf dem Wiedergabestatus."""
        button_data = get_button_data()
        if button_data[self.button_id]["active"]:
            self.play_btn.config(text="> Play", bg=COLOR_BTN_ACTIVE, fg=COLOR_TEXT_ACTIVE)
        else:
            self.play_btn.config(text="> Play", bg="#444", fg=COLOR_TEXT)

    def undo_and_close(self):
        """Stellt die ursprünglichen Loop-Einstellungen wieder her und schließt das Fenster."""
        button_data = get_button_data()

        # Ursprüngliche Werte wiederherstellen
        button_data[self.button_id]["loop_start"] = self._original_loop_start
        button_data[self.button_id]["loop_end"] = self._original_loop_end
        button_data[self.button_id]["auto_loop_active"] = self._original_auto_loop_active
        button_data[self.button_id]["auto_loop_bars"] = self._original_auto_loop_bars
        button_data[self.button_id]["auto_loop_custom_mode"] = self._original_auto_loop_custom_mode
        # Intro-Werte wiederherstellen
        button_data[self.button_id]["intro_active"] = self._original_intro_active
        button_data[self.button_id]["intro_bars"] = self._original_intro_bars
        button_data[self.button_id]["intro_custom_mode"] = self._original_intro_custom_mode

        # Loop-Punkte im PyoLoop aktualisieren
        loop = button_data[self.button_id].get("pyo")
        if loop:
            loop.update_loop_points(self._original_loop_start, self._original_loop_end)

        # Fenster schließen ohne zu speichern
        unregister_open_loop_editor_window(self.button_id)

        try:
            plt.close(self.fig)
            self.window.destroy()
        except (tk.TclError, AttributeError):
            pass

    def apply_and_close(self):
        """Speichert die aktuellen Loop-Einstellungen und schließt das Fenster."""
        self.on_window_close()

    def on_window_close(self):
        if self.save_config_async_callback:
            self.save_config_async_callback()

        unregister_open_loop_editor_window(self.button_id)

        try:
            plt.close(self.fig)
            self.window.destroy()
        except (tk.TclError, AttributeError):
            pass  # Fenster bereits geschlossen
