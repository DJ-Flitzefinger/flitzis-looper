"""EQKnob Widget - Drehregler für EQ (Low/Mid/High).
Canvas-basiertes Widget mit Maus-Event-Handlern.
"""

import math
import tkinter as tk

from flitzis_looper.core.state import COLOR_BG, COLOR_TEXT


class EQKnob:
    def __init__(self, parent, label, width=50, height=65):
        self.frame = tk.Frame(parent, bg=COLOR_BG)
        self.label = label
        self.value = 0.0
        self.width = width
        self.height = height
        self.dragging = False
        self.last_y = 0
        self.last_x = 0

        self.canvas = tk.Canvas(
            self.frame, width=width, height=height - 15, bg=COLOR_BG, highlightthickness=0
        )
        self.canvas.pack()

        self.text_label = tk.Label(
            self.frame, text=label, fg=COLOR_TEXT, bg=COLOR_BG, font=("Arial", 8)
        )
        self.text_label.pack()

        self.canvas.bind("<Button-1>", self.start_drag)
        self.canvas.bind("<B1-Motion>", self.on_drag)
        self.canvas.bind("<ButtonRelease-1>", self.end_drag)
        self.canvas.bind("<Button-2>", self.reset)
        self.canvas.bind("<Button-3>", self.kill)

        self.draw_knob()

    def draw_knob(self):
        self.canvas.delete("all")
        cx, cy = self.width // 2, (self.height - 15) // 2
        radius = min(cx, cy) - 3

        self.canvas.create_oval(
            cx - radius, cy - radius, cx + radius, cy + radius, fill="#333", outline="#555", width=2
        )

        val = self.value
        angle = 250 - (val + 1) * 160
        rad = math.radians(angle)
        x2 = cx + (radius - 4) * math.cos(rad)
        y2 = cy - (radius - 4) * math.sin(rad)

        if val <= -0.98:
            color = "#ff0000"
        elif abs(val) < 0.02:
            color = "#00ff00"
        else:
            color = "#ffaa00"

        self.canvas.create_line(cx, cy, x2, y2, fill=color, width=3)
        self.canvas.create_oval(cx - 2, cy - 2, cx + 2, cy + 2, fill=color, outline="")

    def start_drag(self, event):
        self.dragging = True
        self.last_y = event.y
        self.last_x = event.x

    def on_drag(self, event):
        if not self.dragging:
            return

        dy = self.last_y - event.y
        dx = event.x - self.last_x
        delta = (dy + dx) * 0.012

        new_value = self.value + delta

        if abs(new_value) < 0.03 and abs(self.value) >= 0.03:
            new_value = 0.0

        self.value = max(-1.0, min(1.0, new_value))
        self.last_y = event.y
        self.last_x = event.x
        self.draw_knob()

    def end_drag(self, event):
        self.dragging = False

    def reset(self, event):
        self.value = 0.0
        self.draw_knob()

    def kill(self, event):
        self.value = -1.0
        self.draw_knob()

    def set_value(self, val):
        self.value = max(-1.0, min(1.0, val))
        self.draw_knob()

    def get_value(self):
        return self.value
