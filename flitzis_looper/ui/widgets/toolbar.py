"""ToolbarWidget - Kapselt die Toolbar-Kontrollen.

Enthält:
- BPM Display
- Speed Slider mit Reset und BPM +/- Buttons
- Key Lock / BPM Lock Buttons
- BPM Entry
- Master Volume Slider
- Multi Loop Button
"""

import tkinter as tk
from collections.abc import Callable
from typing import Any

from flitzis_looper.core.state import (
    COLOR_BG,
    COLOR_BTN_ACTIVE,
    COLOR_LOCK_OFF,
    COLOR_RESET_RED,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    get_master_bpm_value,
    get_master_volume,
    get_speed_value,
)


class ToolbarWidget:
    """Widget-Sammlung für die Toolbar-Kontrollen.

    Da die Toolbar-Elemente an verschiedenen Stellen platziert werden,
    ist dies keine einzelne Frame-Klasse, sondern eine Sammlung von Widgets.

    Attributes:
        bpm_display: Label für BPM-Anzeige
        speed_slider: Scale für Speed-Kontrolle
        reset_btn: Button für Speed-Reset
        key_lock_btn: Button für Key Lock
        bpm_lock_btn: Button für BPM Lock
        bpm_entry: Entry für manuelle BPM-Eingabe
        master_volume_slider: Scale für Master-Volume
        multi_loop_btn: Button für Multi-Loop-Toggle
    """

    def __init__(
        self,
        root: tk.Tk,
        callbacks: dict[str, Callable],
    ):
        """Initialisiert die Toolbar-Widgets.

        Args:
            root: Das Root-Fenster
            callbacks: Dict mit Callback-Funktionen:
                - on_speed_change: Callable[[str, dict, dict], None]
                - reset_pitch: Callable[[dict], None]
                - adjust_bpm_by_delta: Callable[[float, dict, dict], None]
                - toggle_key_lock: Callable[[tk.Button], None]
                - toggle_bpm_lock: Callable[[tk.Button], None]
                - validate_bpm_entry: Callable[[str], bool]
                - update_speed_from_master_bpm: Callable[[tk.Entry, dict], None]
                - on_master_volume_change: Callable[[str, tk.Label], None]
                - reset_master_volume: Callable[[tk.Scale], None]
                - toggle_multi_loop: Callable[[tk.Button, dict], None]
                - apply_stem_mix: Callable[[int], None]
                - invalidate_stem_caches: Callable[[int], None]
                - update_button_label: Callable[[int], None]
        """
        self._root = root
        self._callbacks = callbacks
        self._widgets: dict[str, Any] = {}

        # Tk-Variablen holen
        self._speed_value = get_speed_value()
        self._master_volume = get_master_volume()
        self._master_bpm_value = get_master_bpm_value()

        # Ensure variables are not None (should never happen in practice)
        if self._speed_value is None:
            msg = "Speed value variable is not initialized"
            raise RuntimeError(msg)
        if self._master_volume is None:
            msg = "Master volume variable is not initialized"
            raise RuntimeError(msg)
        if self._master_bpm_value is None:
            msg = "Master BPM value variable is not initialized"
            raise RuntimeError(msg)

        self._create_right_frame()
        self._create_master_volume()
        self._create_multi_loop_button()

    def _create_right_frame(self) -> None:
        """Erstellt den rechten Frame mit BPM Display, Slider und Lock-Buttons."""
        right_frame = tk.Frame(self._root, bg=COLOR_BG)
        right_frame.place(relx=1.0, rely=0.0, anchor="ne", x=-20, y=10)

        # BPM Display
        bpm_display = tk.Label(
            right_frame,
            text="------",
            font=("Courier", 28, "bold"),
            fg="#ff3333",
            bg="black",
            width=6,
            height=1,
            anchor="e",
        )
        bpm_display.pack(pady=(0, 20))
        self._widgets["bpm_display"] = bpm_display

        # Container für Slider + Reset Button
        slider_reset_frame = tk.Frame(right_frame, bg=COLOR_BG)
        slider_reset_frame.pack(padx=(8, 0))

        # Speed Slider
        self._callbacks.get("on_speed_change")
        self._callbacks.get("apply_stem_mix")
        self._callbacks.get("invalidate_stem_caches")

        # Assert that the variable is not None (it should never be after initialization)
        assert self._speed_value is not None, "Speed value variable should not be None"

        speed_slider = tk.Scale(
            slider_reset_frame,
            from_=2.0,
            to=0.5,
            resolution=0.01,
            orient="vertical",
            length=400,
            variable=self._speed_value,
            command=self._on_speed_change_wrapper,
            bg=COLOR_BG,
            fg=COLOR_TEXT,
            troughcolor="#444",
            highlightthickness=0,
            sliderlength=50,
            width=40,
        )
        speed_slider.pack(side="left")
        self._widgets["speed_slider"] = speed_slider

        # Middle-click on slider resets pitch
        reset_pitch = self._callbacks.get("reset_pitch")
        if reset_pitch:
            speed_slider.bind("<Button-2>", lambda e: reset_pitch(self._widgets))

        # Rechter Bereich neben Slider
        reset_frame = tk.Frame(slider_reset_frame, bg=COLOR_BG, width=60, height=400)
        reset_frame.pack(side="left", padx=(5, 0))
        reset_frame.pack_propagate(flag=False)

        # Spacer oben
        spacer_top = tk.Frame(reset_frame, bg=COLOR_BG, height=216)
        spacer_top.pack()

        # BPM Up Button
        adjust_bpm = self._callbacks.get("adjust_bpm_by_delta")
        bpm_up_btn = tk.Button(
            reset_frame,
            text="+",
            font=("Arial", 10),
            fg=COLOR_TEXT,
            bg="#444",
            activebackground="#444",
            activeforeground=COLOR_TEXT,
            relief="raised",
            bd=2,
            width=5,
        )
        bpm_up_btn.pack(pady=(0, 4))
        if adjust_bpm:
            bpm_up_btn.bind("<Button-1>", lambda e: adjust_bpm(1.0, self._widgets, {}))
            bpm_up_btn.bind("<Button-3>", lambda e: adjust_bpm(0.1, self._widgets, {}))

        # Reset Button
        reset_btn = tk.Button(
            reset_frame,
            text="Reset",
            font=("Arial", 9, "bold"),
            fg=COLOR_TEXT_ACTIVE,
            bg=COLOR_BTN_ACTIVE,
            activebackground=COLOR_BTN_ACTIVE,
            activeforeground=COLOR_TEXT_ACTIVE,
            command=lambda: reset_pitch(self._widgets) if reset_pitch else None,
            relief="flat",
            bd=2,
            width=5,
        )
        reset_btn.pack(pady=(0, 4))
        self._widgets["reset_btn"] = reset_btn

        # BPM Down Button
        bpm_down_btn = tk.Button(
            reset_frame,
            text="-",
            font=("Arial", 10),
            fg=COLOR_TEXT,
            bg="#444",
            activebackground="#444",
            activeforeground=COLOR_TEXT,
            relief="raised",
            bd=2,
            width=5,
        )
        bpm_down_btn.pack(pady=(0, 0))
        if adjust_bpm:
            bpm_down_btn.bind("<Button-1>", lambda e: adjust_bpm(-1.0, self._widgets, {}))
            bpm_down_btn.bind("<Button-3>", lambda e: adjust_bpm(-0.1, self._widgets, {}))

        # Lock Buttons Frame
        lock_frame = tk.Frame(right_frame, bg=COLOR_BG)
        lock_frame.pack(pady=(15, 0))

        # Key Lock Button
        toggle_key_lock = self._callbacks.get("toggle_key_lock")
        key_lock_btn = tk.Button(
            lock_frame,
            text="KEY LOCK",
            command=lambda: toggle_key_lock(key_lock_btn) if toggle_key_lock else None,
            bg=COLOR_LOCK_OFF,
            fg=COLOR_TEXT,
            width=12,
        )
        key_lock_btn.pack(pady=(0, 5))
        self._widgets["key_lock_btn"] = key_lock_btn

        # BPM Lock Button
        toggle_bpm_lock = self._callbacks.get("toggle_bpm_lock")
        bpm_lock_btn = tk.Button(
            lock_frame,
            text="BPM LOCK",
            command=lambda: toggle_bpm_lock(bpm_lock_btn) if toggle_bpm_lock else None,
            bg=COLOR_LOCK_OFF,
            fg=COLOR_TEXT,
            width=12,
        )
        bpm_lock_btn.pack(pady=(0, 5))
        self._widgets["bpm_lock_btn"] = bpm_lock_btn

        # BPM Entry
        validate_bpm = self._callbacks.get("validate_bpm_entry")
        update_speed_from_bpm = self._callbacks.get("update_speed_from_master_bpm")

        bpm_validate_cmd = self._root.register(validate_bpm) if validate_bpm else None
        # Assert that the variable is not None (it should never be after initialization)
        assert self._master_bpm_value is not None, "Master BPM value variable should not be None"

        bpm_entry = tk.Entry(
            lock_frame,
            textvariable=self._master_bpm_value,
            width=12,
            justify="center",
            bg="#333",
            fg=COLOR_TEXT,
            validate="key" if bpm_validate_cmd else "none",
            validatecommand=(bpm_validate_cmd, "%P") if bpm_validate_cmd else (),
            insertbackground=COLOR_TEXT,
        )
        bpm_entry.pack()
        if update_speed_from_bpm:
            bpm_entry.bind("<Return>", lambda e: update_speed_from_bpm(bpm_entry, self._widgets))
        self._widgets["bpm_entry"] = bpm_entry

    def _on_speed_change_wrapper(self, value: str) -> None:
        """Wrapper für Speed-Change mit Widgets-Dict."""
        on_speed_change = self._callbacks.get("on_speed_change")
        apply_stem_mix = self._callbacks.get("apply_stem_mix")
        invalidate_stem_caches = self._callbacks.get("invalidate_stem_caches")

        if on_speed_change:
            on_speed_change(
                value,
                self._widgets,
                {
                    "apply_stem_mix": apply_stem_mix,
                    "invalidate_stem_caches": invalidate_stem_caches,
                },
            )

    def _create_master_volume(self) -> None:
        """Erstellt den Master-Volume-Slider."""
        volume_frame = tk.Frame(self._root, bg=COLOR_BG)
        volume_frame.place(relx=0.02, rely=0.95, anchor="sw")

        volume_label = tk.Label(
            volume_frame,
            text="Master Volume 100%",
            fg=COLOR_TEXT,
            bg=COLOR_BG,
            font=("Arial", 10),
        )
        volume_label.pack()
        self._widgets["volume_label"] = volume_label

        on_volume_change = self._callbacks.get("on_master_volume_change")
        reset_volume = self._callbacks.get("reset_master_volume")

        # Assert that the variable is not None (it should never be after initialization)
        assert self._master_volume is not None, "Master volume variable should not be None"

        master_volume_slider = tk.Scale(
            volume_frame,
            from_=0.0,
            to=1.0,
            resolution=0.01,
            orient="horizontal",
            length=200,
            variable=self._master_volume,
            command=lambda v: on_volume_change(v, volume_label) if on_volume_change else None,
            bg=COLOR_BG,
            fg=COLOR_TEXT,
            troughcolor="#444",
            highlightthickness=0,
            showvalue=False,
        )
        master_volume_slider.pack()
        if reset_volume:
            master_volume_slider.bind(
                "<Double-Button-1>",
                lambda e: reset_volume(master_volume_slider),
            )
        self._widgets["master_volume_slider"] = master_volume_slider

    def _create_multi_loop_button(self) -> None:
        """Erstellt den Multi-Loop-Button."""
        multi_loop_frame = tk.Frame(self._root, bg=COLOR_BG)
        multi_loop_frame.place(relx=0.02, rely=0.95, anchor="sw", x=250)

        toggle_multi_loop = self._callbacks.get("toggle_multi_loop")
        update_button_label = self._callbacks.get("update_button_label")

        multi_loop_btn = tk.Button(
            multi_loop_frame,
            text="MULTI LOOP",
            command=lambda: toggle_multi_loop(
                multi_loop_btn,
                {"update_button_label": update_button_label},
            )
            if toggle_multi_loop
            else None,
            bg=COLOR_LOCK_OFF,
            fg=COLOR_TEXT,
            width=12,
        )
        multi_loop_btn.pack()
        self._widgets["multi_loop_btn"] = multi_loop_btn

    def update_reset_button_style(self) -> None:
        """Reset-Button: Grün bei 1.00, sonst rot."""
        reset_btn = self._widgets.get("reset_btn")
        if reset_btn is None:
            return

        # Assert that the variable is not None (it should never be after initialization)
        assert self._speed_value is not None, "Speed value variable should not be None"
        current = self._speed_value.get()
        if current == 1.0 or abs(current - 1.0) < 0.001:
            reset_btn.config(
                bg=COLOR_BTN_ACTIVE,
                fg=COLOR_TEXT_ACTIVE,
                activebackground=COLOR_BTN_ACTIVE,
                activeforeground=COLOR_TEXT_ACTIVE,
            )
        else:
            reset_btn.config(
                bg=COLOR_RESET_RED,
                fg=COLOR_TEXT,
                activebackground=COLOR_RESET_RED,
                activeforeground=COLOR_TEXT,
            )

    def get_widgets(self) -> dict[str, Any]:
        """Gibt alle Widgets als Dict zurück (für Backward-Kompatibilität)."""
        # Füge update_reset_button_style zum Dict hinzu
        widgets = self._widgets.copy()
        widgets["update_reset_button_style"] = self.update_reset_button_style
        return widgets

    @property
    def bpm_display(self) -> tk.Label:
        """Gibt das BPM-Display-Label zurück."""
        widget = self._widgets.get("bpm_display")
        if widget is None:
            msg = "BPM display widget not initialized"
            raise RuntimeError(msg)
        return widget

    @property
    def speed_slider(self) -> tk.Scale:
        """Gibt den Speed-Slider zurück."""
        widget = self._widgets.get("speed_slider")
        if widget is None:
            msg = "Speed slider widget not initialized"
            raise RuntimeError(msg)
        return widget

    @property
    def reset_btn(self) -> tk.Button:
        """Gibt den Reset-Button zurück."""
        widget = self._widgets.get("reset_btn")
        if widget is None:
            msg = "Reset button widget not initialized"
            raise RuntimeError(msg)
        return widget

    @property
    def master_volume_slider(self) -> tk.Scale:
        """Gibt den Master-Volume-Slider zurück."""
        widget = self._widgets.get("master_volume_slider")
        if widget is None:
            msg = "Master volume slider widget not initialized"
            raise RuntimeError(msg)
        return widget
