![Flitzis Looper](docs/assets/hero1.webp)

# Flitzis Looper

A live scratch looper for DJs and turntablists. Build your own performance
instrument from musical loops, prepared stems, tempo and pitch controls, and
keyboard/MIDI mappings.

Six banks of 36 PADs provide **216 assignment slots**. Play one PAD at a time or
layer them with MultiLoop; use constant-rate BPM Lock, quantized launches,
waveform loop editing, Piano/transposition and Rubber Band Key Lock. Each PAD
has Gain/Trim and a three-band isolator. Offline BS-RoFormer or Demucs separation
provides component stems, and mix recording saves WAV, FLAC or MP3 takes.

The productive performance target is **up to six simultaneous PADs**. Synthetic
stress uses at most ten; more than ten can cause glitches or other performance
problems. Actual headroom depends on the computer, interface and processing.
Slot count is not a concurrent-playback guarantee.

Read the **[user guide](docs/user-guide.md)** for controls, defaults, preparation
feedback, recording, recovery and troubleshooting. Automatic BPM/Key results are
estimates/classifications: check them against the music and correct them when
needed. Constant-BPM playback does not follow changing recorded tempo.

## Installation

### Windows Portable

For an independent Windows x64 installation without preinstalled Git, Python or
Rust:

1. Download the current [INSTALL-LOOPER-PORTABLE.bat](https://github.com/DJ-Flitzefinger/flitzis-looper/raw/refs/heads/main/INSTALL-LOOPER-PORTABLE.bat).
2. Put **only that BAT file** in your chosen installation-parent folder, outside
   a source clone, and double-click it.
3. Wait for acquisition, dependency setup, source build and model checks. Missing
   Microsoft C++ Build Tools/Windows SDK can require administrator approval and
   a restart. Initial setup needs substantial download time and disk space.
4. Start the resulting `Flitzis-Looper/START-LOOPER.bat`.

Run the installer again with the app closed to update from current remote `main`
or repair resources. The installation keeps its own runtime, models, workers and
user data; keep the complete directory together when relocating it. A running
current or legacy app prevents updating. No project binary release or personal
developer workspace is required. See the [full Portable guide](docs/portable-windows.md)
for destination rules, locks, repair, recovery and actual validation limits.

### Install from source

Public source prerequisites:

- Git, [uv](https://docs.astral.sh/uv/) and Python **3.14+** for the main app.
- Rust and the platform C/C++ build toolchain; Windows needs MSVC/Windows SDK.
- Rubber Band with the **LiveShifter C API**, such as version 4.0.0. Build metadata
  and runtime DLL/library discovery must be configured; see the
  [native dependency instructions](docs/development.md#native-rubber-band-dependency).
- The separately locked Python 3.12 BeatThis worker for automatic BPM; the setup
  command below provisions it.
- FFmpeg and separator model assets for optional stem generation.

On Windows, after configuring those prerequisites:

```powershell
git clone https://github.com/DJ-Flitzefinger/flitzis-looper.git
cd flitzis-looper
.\start-release.bat --build-only
uv run --no-sync python scripts/setup-runtime.py
.\start.bat
```

`start.bat` is a launch-only entry for the prepared Release. It does not build or
synchronize dependencies. Close Looper before building another Native profile.
For editable source setup, Linux dependency discovery and individual checks, use
[Development](docs/development.md#setup).

The runtime setup command verifies the required analysis worker without opening
the app, an audio device or model inference. Add `--with-stems` to acquire and
CPU-load both optional separator models; install FFmpeg as described in
[stem setup](docs/stem-generation-setup.md). Re-run setup to repair, or use
`--check` for non-acquiring verification. Public source defaults use ignored
`models/` and `.cache/runtime-setup/` beneath the checkout; explicitly configured
developer resource overrides are separate. See [BeatThis setup](docs/beat-this-setup.md).

## First loop

1. Choose the operating system's default output before launching.
2. Middle-click a PAD to select it, then choose **Load Audio** in the left sidebar.
3. Left-click a loaded PAD to play/restart; right-click to stop.
4. Open **Adjust Loop** to set BPM, Grid Offset and loop boundaries.
5. Enable **MULTI LOOP**, **BPM LOCK** and **Q** as needed. Check pending and
   confirmed **Audio** feedback before assuming a change is effective.

The [complete manual](docs/user-guide.md) covers the exact mouse gestures and
Learn workflow. Back up your set's config, material and input mappings, as well
as any recordings you want to retain.

## Documentation and contribution

- [Documentation map](docs/README.md): performer, setup, architecture and evidence reading paths.
- [Architecture](docs/architecture.md): Python control/persistence and Rust audio ownership.
- [Development](docs/development.md): setup, package layout, checks and OpenSpec workflow.
- [Behavior contracts](openspec/specs/): current product requirements.
- [Explicit user TODO notes](docs/todos.md): notes, not an automatic work queue.

Archived changes and research reports retain their original failures and
unfinished acceptance gates. They do not activate historical work programs.

## License

[GNU General Public License](LICENSE.txt).
