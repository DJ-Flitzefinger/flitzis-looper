"""flitzis_looper.ui.toolbar - Toolbar Controls.

Enthält:
- create_toolbar(): Erstellt rechte Toolbar (BPM, Speed, Locks)
- create_master_volume(): Erstellt Master Volume Slider
- create_multi_loop_button(): Erstellt MULTI LOOP Button
"""

import tkinter as tk

from flitzis_looper.core.state import (
    COLOR_BG,
    COLOR_BTN_ACTIVE,
    COLOR_LOCK_OFF,
    COLOR_LOCK_ON,
    COLOR_RESET_RED,
    COLOR_TEXT,
    COLOR_TEXT_ACTIVE,
    get_master_bpm_value,
    get_master_volume,
    get_root,
    get_speed_value,
)

# Module-level references to widgets (set during create_*)
_bpm_display = None
_speed_slider = None
_reset_btn = None
_key_lock_btn = None
_bpm_lock_btn = None
_bpm_entry = None
_multi_loop_btn = None
_master_volume_slider = None
_volume_label = None


def get_bpm_display():
    """Gibt das BPM-Display Widget zurück."""
    return _bpm_display


def get_speed_slider():
    """Gibt den Speed-Slider zurück."""
    return _speed_slider


def create_toolbar(
    on_speed_change_callback,
    toggle_key_lock_callback,
    toggle_bpm_lock_callback,
    update_speed_from_master_bpm_callback,
    on_bpm_up_click_callback,
    on_bpm_down_click_callback,
    reset_pitch_callback,
):
    """Erstellt die rechte Toolbar mit BPM Display, Speed Slider, Lock Buttons.

    Args:
        on_speed_change_callback: Callback für Speed-Änderung
        toggle_key_lock_callback: Callback für KEY LOCK Toggle
        toggle_bpm_lock_callback: Callback für BPM LOCK Toggle
        update_speed_from_master_bpm_callback: Callback für BPM Entry Enter
        on_bpm_up_click_callback: Callback für BPM Up Button
        on_bpm_down_click_callback: Callback für BPM Down Button
        reset_pitch_callback: Callback für Pitch Reset

    Returns:
        dict: Dictionary mit Widget-Referenzen
    """
    global _bpm_display, _speed_slider, _reset_btn, _key_lock_btn, _bpm_lock_btn, _bpm_entry

    root = get_root()
    speed_value = get_speed_value()
    master_bpm_value = get_master_bpm_value()

    # Right frame for controls
    right_frame = tk.Frame(root, bg=COLOR_BG)
    right_frame.place(relx=1.0, rely=0.0, anchor="ne", x=-20, y=10)

    # BPM Display
    _bpm_display = tk.Label(
        right_frame,
        text="------",
        font=("Courier", 28, "bold"),
        fg="#ff3333",
        bg="black",
        width=6,
        height=1,
        anchor="e",
    )
    _bpm_display.pack(pady=(0, 20))

    # Container für Slider + Reset Button nebeneinander
    slider_reset_frame = tk.Frame(right_frame, bg=COLOR_BG)
    slider_reset_frame.pack(padx=(8, 0))

    # Pitch-Slider (links im Container)
    _speed_slider = tk.Scale(
        slider_reset_frame,
        from_=2.0,
        to=0.5,
        resolution=0.01,
        orient="vertical",
        length=400,
        variable=speed_value,
        command=on_speed_change_callback,
        bg=COLOR_BG,
        fg=COLOR_TEXT,
        troughcolor="#444",
        highlightthickness=0,
        sliderlength=50,
        width=40,
    )
    _speed_slider.pack(side="left")

    # Middle-click on slider resets pitch
    _speed_slider.bind("<Button-2>", lambda e: reset_pitch_callback())

    # Rechter Bereich neben Slider: BPM Up/Down Buttons + Reset
    reset_frame = tk.Frame(slider_reset_frame, bg=COLOR_BG, width=60, height=400)
    reset_frame.pack(side="left", padx=(5, 0))
    reset_frame.pack_propagate(flag=False)

    # Spacer oben bis zu den Buttons (216px für korrekte 1.00 Position)
    spacer_top = tk.Frame(reset_frame, bg=COLOR_BG, height=216)
    spacer_top.pack()

    # BPM Up Button (Pfeil nach oben) - grau
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
    bpm_up_btn.bind("<Button-1>", on_bpm_up_click_callback)
    bpm_up_btn.bind("<Button-3>", on_bpm_up_click_callback)

    # Reset-Button - auf Höhe 1.00, startet grün (da Pitch initial 1.0)
    _reset_btn = tk.Button(
        reset_frame,
        text="Reset",
        font=("Arial", 9, "bold"),
        fg=COLOR_TEXT_ACTIVE,
        bg=COLOR_BTN_ACTIVE,
        activebackground=COLOR_BTN_ACTIVE,
        activeforeground=COLOR_TEXT_ACTIVE,
        command=reset_pitch_callback,
        relief="flat",
        bd=2,
        width=5,
    )
    _reset_btn.pack(pady=(0, 4))

    # BPM Down Button (Pfeil nach unten) - grau
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
    bpm_down_btn.bind("<Button-1>", on_bpm_down_click_callback)
    bpm_down_btn.bind("<Button-3>", on_bpm_down_click_callback)

    # Lock Buttons Frame (unter Slider)
    lock_frame = tk.Frame(right_frame, bg=COLOR_BG)
    lock_frame.pack(pady=(15, 0))

    # KEY LOCK Button
    _key_lock_btn = tk.Button(
        lock_frame,
        text="KEY LOCK",
        command=toggle_key_lock_callback,
        bg=COLOR_LOCK_OFF,
        fg=COLOR_TEXT,
        width=12,
    )
    _key_lock_btn.pack(pady=(0, 5))

    # BPM LOCK Button
    _bpm_lock_btn = tk.Button(
        lock_frame,
        text="BPM LOCK",
        command=toggle_bpm_lock_callback,
        bg=COLOR_LOCK_OFF,
        fg=COLOR_TEXT,
        width=12,
    )
    _bpm_lock_btn.pack(pady=(0, 5))

    # BPM Entry mit Validierung (nur Zahlen, Punkt, Komma)
    def validate_bpm_entry(new_value):
        """Erlaubt nur Zahlen, Punkt und Komma im BPM-Eingabefeld."""
        if not new_value:
            return True
        # Erlaube Zahlen, einen Punkt oder ein Komma
        for char in new_value:
            if char not in "0123456789.,":
                return False
        # Maximal ein Dezimaltrennzeichen
        return not new_value.count(".") + new_value.count(",") > 1

    bpm_validate_cmd = root.register(validate_bpm_entry)
    _bpm_entry = tk.Entry(
        lock_frame,
        textvariable=master_bpm_value,
        width=12,
        justify="center",
        bg="#333",
        fg=COLOR_TEXT,
        validate="key",
        validatecommand=(bpm_validate_cmd, "%P"),
        insertbackground=COLOR_TEXT,
    )
    _bpm_entry.pack()
    _bpm_entry.bind("<Return>", lambda e: update_speed_from_master_bpm_callback())

    return {
        "bpm_display": _bpm_display,
        "speed_slider": _speed_slider,
        "reset_btn": _reset_btn,
        "key_lock_btn": _key_lock_btn,
        "bpm_lock_btn": _bpm_lock_btn,
        "bpm_entry": _bpm_entry,
    }


def create_master_volume(on_master_volume_change_callback):
    """Erstellt den Master Volume Slider.

    Args:
        on_master_volume_change_callback: Callback für Volume-Änderung

    Returns:
        dict: Dictionary mit Widget-Referenzen
    """
    global _master_volume_slider, _volume_label

    root = get_root()
    master_volume = get_master_volume()

    # Master Volume Control at bottom left
    volume_frame = tk.Frame(root, bg=COLOR_BG)
    volume_frame.place(relx=0.02, rely=0.95, anchor="sw")

    _volume_label = tk.Label(
        volume_frame, text="Master Volume 100%", fg=COLOR_TEXT, bg=COLOR_BG, font=("Arial", 10)
    )
    _volume_label.pack()

    _master_volume_slider = tk.Scale(
        volume_frame,
        from_=0.0,
        to=1.0,
        resolution=0.01,
        orient="horizontal",
        length=200,
        variable=master_volume,
        command=on_master_volume_change_callback,
        bg=COLOR_BG,
        fg=COLOR_TEXT,
        troughcolor="#444",
        highlightthickness=0,
        showvalue=False,
    )
    _master_volume_slider.pack()

    def reset_master_volume():
        _master_volume_slider.set(1.0)

    _master_volume_slider.bind("<Double-Button-1>", lambda e: reset_master_volume())

    return {
        "volume_slider": _master_volume_slider,
        "volume_label": _volume_label,
    }


def create_multi_loop_button(toggle_multi_loop_callback):
    """Erstellt den MULTI LOOP Button.

    Args:
        toggle_multi_loop_callback: Callback für Multi Loop Toggle

    Returns:
        tk.Button: Der MULTI LOOP Button
    """
    global _multi_loop_btn

    root = get_root()

    # MULTI LOOP Button
    multi_loop_frame = tk.Frame(root, bg=COLOR_BG)
    multi_loop_frame.place(relx=0.02, rely=0.95, anchor="sw", x=250)

    _multi_loop_btn = tk.Button(
        multi_loop_frame,
        text="MULTI LOOP",
        command=toggle_multi_loop_callback,
        bg=COLOR_LOCK_OFF,
        fg=COLOR_TEXT,
        width=12,
    )
    _multi_loop_btn.pack()

    return _multi_loop_btn


def update_reset_button_style():
    """Reset-Button: Grün bei 1.00, sonst rot."""
    if _reset_btn is None:
        return

    speed_value = get_speed_value()
    current = speed_value.get()
    if current == 1.0 or abs(current - 1.0) < 0.001:
        _reset_btn.config(
            bg=COLOR_BTN_ACTIVE,
            fg=COLOR_TEXT_ACTIVE,
            activebackground=COLOR_BTN_ACTIVE,
            activeforeground=COLOR_TEXT_ACTIVE,
        )
    else:
        _reset_btn.config(
            bg=COLOR_RESET_RED,
            fg=COLOR_TEXT,
            activebackground=COLOR_RESET_RED,
            activeforeground=COLOR_TEXT,
        )


def update_key_lock_button_style(is_active):
    """Aktualisiert den KEY LOCK Button Style."""
    if _key_lock_btn is None:
        return

    if is_active:
        _key_lock_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        _key_lock_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)


def update_bpm_lock_button_style(is_active):
    """Aktualisiert den BPM LOCK Button Style."""
    if _bpm_lock_btn is None:
        return

    if is_active:
        _bpm_lock_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        _bpm_lock_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)


def update_multi_loop_button_style(is_active):
    """Aktualisiert den MULTI LOOP Button Style."""
    if _multi_loop_btn is None:
        return

    if is_active:
        _multi_loop_btn.config(bg=COLOR_LOCK_ON, fg=COLOR_TEXT_ACTIVE)
    else:
        _multi_loop_btn.config(bg=COLOR_LOCK_OFF, fg=COLOR_TEXT)


def update_volume_label(volume_percent):
    """Aktualisiert das Volume Label."""
    if _volume_label is None:
        return
    _volume_label.config(text=f"Master Volume {volume_percent}%")
