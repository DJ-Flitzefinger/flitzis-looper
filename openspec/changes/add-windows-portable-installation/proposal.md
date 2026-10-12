# Windows portable installation from current main

The developer launchers rebuild the shared checkout and cannot provide an
independent installation for Human testing during development. Provide the
requested single-file `INSTALL-LOOPER-PORTABLE.bat`: copy it to a chosen folder,
double-click it and install the current remote `main` into a separate private
Release runtime. Repeat runs follow `main`, independently of project Releases,
local bundle channels or a developer checkout.

Check and automatically acquire missing Git, uv, CPython 3.14.2, Rust 1.95.0,
vcpkg/Rubber Band and FFmpeg into installation-owned tools/caches. Acquire
missing Microsoft C++ Build Tools and Windows SDK through their supported signed
installer; Windows elevation and a possible restart are explicit installation
conditions. A novice user does not need to execute developer terminal commands.

Build and preflight an exact source snapshot before atomically activating it.
Retain old runtime files for existing processes, and preserve persistent
projects, samples, settings and models through failed and successful updates.
The completed private application can move as a whole; shared Microsoft build
components and system drivers remain on Windows. Explain substantial first-run
downloads, compilation time and disk use without promising fixed totals.

Keep installer/BAT/helper/test/docs compatibility part of every dependency, ABI,
DLL-layout or launch/persistence change. The old bundle builder may remain an
advanced maintainer utility, but is not the ordinary installation path.

Non-goals: change audio/DSP or the realtime callback, copy developer projects or
private models, replace developer environments or persistent user PATH, publish
a project Release or set up remotes, rewrite developer launchers, or claim
clean-Windows/Human GUI, device or hearing acceptance without an actual run.
All network, disk, dependency/build and preflight work runs in installer/control
processes. The audio callback remains free of installation, Python/GIL, disk
and blocking work.
