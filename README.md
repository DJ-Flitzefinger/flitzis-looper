# Dj Flitzefinger's Scratch-Looper

A professional DJ looping application for live performance and music production.

## 🎛️ Overview

Dj Flitzefinger's Scratch-Looper is a powerful audio looping tool designed for DJs, producers, and live performers. It provides a grid-based interface for triggering audio loops with advanced features like stem separation, BPM control, pitch adjustment, and real-time mixing.

## 🚀 Features

- **Grid-based Loop Triggering**: 9x9 button grid for instant loop playback
- **Stem Separation**: Isolate vocals, drums, bass, and other elements from any audio file
- **BPM Detection & Control**: Automatic BPM detection with manual override
- **Pitch & Speed Control**: Adjust playback speed and pitch independently
- **Multi-Bank System**: Organize loops across multiple banks for complex performances
- **Real-time Stem Mixing**: Control individual stem volumes and EQ on the fly
- **Master Volume & Effects**: Global volume control and audio processing

## 📦 Installation

### Prerequisites

- Python 3.13.9+
- liblo (system dependency)

### Install from Source

```bash
git clone https://github.com/your-repo/flitzis-looper.git
cd flitzis-looper
pip install -e .
```

### Dependencies

The project requires these Python packages:

- demucs
- madmom
- matplotlib
- numpy
- pedalboard
- pyo
- setuptools
- soundfile
- torch
- torchaudio

## 🎧 Usage

### Starting the Application

```bash
python -m flitzis_looper
```

### Basic Controls

- **Left-click** on grid buttons: Trigger/stop loops
- **Right-click** on grid buttons: Stop loops
- **Middle-click** on grid buttons: Open context menu (load/unload audio, BPM detection, etc.)
- **Pitch Slider**: Adjust playback speed (0.5x to 2.0x)
- **BPM Controls**: Set and lock tempo
- **Stem Buttons**: Toggle individual stems (vocals, drums, bass, other)
- **Master Volume**: Control global output volume

### Advanced Features

**Stem Separation:**
1. Load an audio file onto a grid button
2. Set the BPM (auto-detected or manual)
3. Generate stems from the context menu
4. Use stem buttons to mix individual elements

**Multi-Loop Mode:**
- Toggle multi-loop mode to play multiple loops simultaneously
- Adjust individual loop volumes and EQ settings

## 🔧 Development

### Setup

```bash
uv sync --frozen
```

### Running Tests

```bash
mypy .
ruff check .
```

### Project Structure

```
flitzis_looper/
├── audio/          # Audio processing modules
├── core/           # Core application logic
├── ui/             # User interface components
└── utils/          # Utility functions
```

## 🤝 Contributing

Contributions are welcome! Please follow these guidelines:

1. Fork the repository
2. Create a feature branch
3. Implement your changes
4. Write tests if applicable
5. Submit a pull request
