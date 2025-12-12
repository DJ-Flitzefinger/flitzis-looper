"""VUMeter Widget - Level-Anzeige mit Segmenten.
Canvas-basiertes Widget für Audio-Level-Visualisierung.
"""

import tkinter as tk


class VUMeter:
    def __init__(self, parent, width=20, height=200):
        self.canvas = tk.Canvas(parent, width=width, height=height, bg="#222")
        self.width = width
        self.height = height
        self.segments = []
        self.create_segments()

    def create_segments(self):
        segment_height = 6
        gap = 1
        total_segments = (self.height - 20) // (segment_height + gap)
        db_per_segment = 60 / total_segments

        for i in range(total_segments):
            y_top = self.height - 10 - (i * (segment_height + gap))
            y_bottom = y_top - segment_height

            db_value = -60 + (i * db_per_segment)

            if db_value < -18:
                color = "#00ff00"
            elif db_value < -6:
                color = "#ffff00"
            elif db_value < -3:
                color = "#ff8800"
            else:
                color = "#ff0000"

            segment = self.canvas.create_rectangle(
                2, y_bottom, self.width - 2, y_top, fill="#333", outline="#555"
            )
            self.segments.append((segment, color, db_value))

    def update_level(self, db_level):
        for segment, color, db_threshold in self.segments:
            if db_level >= db_threshold:
                self.canvas.itemconfig(segment, fill=color)
            else:
                self.canvas.itemconfig(segment, fill="#333")
