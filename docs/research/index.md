# Research Documents

This collection of documents outlines the possible strategic architectural evolution of Flitzis Looper to meet real-time audio performance goals and improve testability.

Key motivations:
- [UI Toolkit Migration](./ui-toolkit-migration.md)
  - Migrate from Tkinter to ImGui for deterministic low-latency UI rendering
  - Decouple core audio logic from UI components for testable architecture
- [Cross-Platform Audio Backends and I/O](./audio-backends.md)
  - Adopt native audio backends (miniaudio/CPAL) for sub-10ms latency across platforms
  - Implement ring-buffer IPC between Python UI and native audio engine

These changes enable comprehensive testing (>90% core coverage) and real-time performance required for live performance applications.
