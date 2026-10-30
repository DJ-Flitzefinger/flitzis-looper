# Windows portable installation from current main

The requested standalone INSTALL-LOOPER-PORTABLE.bat builds current remote main
in a private installation. Repeat starts and updates must preserve the application's
native project layout and permit UI polling while long Native worker operations run.
The historical version-pointer/data layout is superseded by one complete Git checkout
at Root/flitzis-looper and one fixed Root/Runtime. Samples, settings and input mappings
live in the checkout's native locations; model caches remain outside the checkout.

Prepare and preflight the exact candidate before activation. The installed starter
holds a shared lifetime lock, and the updater obtains the exclusive lock and checks
legacy processes. A running or unqueryable old/new runtime stops the update without
killing it. Journalled swaps, Native FileID-preserving data relocation and verified
rollback/recovery protect failed or interrupted updates. Cleanup requires complete
ownership evidence; unknown files cause a concrete preserved-data stop.

Automatically repair the two authoritative separator checkpoints, then load each in
its own fresh CPU process, sequentially. An unchanged healthy source/runtime with
missing models repairs models without rebuilding Native. Shared cache resolution
must agree in the controller and both offline children.

Long Native file/JSON/preparation work uses a short public Engine capture, owned
detached work and fresh guarded adoption. Actual public receive_msg and mutable
control, independent rendering, current-source/request/timing guards and own callback
acknowledgements are preserved. The reported worker's identity remains unproven.

Missing pinned tools are acquired privately; supported Microsoft C++/SDK setup may
need UAC or restart. Keep installer, dependencies, helpers, tests and user docs in
one coherent delivery. Substantial downloads/build disk usage are explained without
fixed time or size promises.

Non-goals: full Rust port/Slice8, plugin infrastructure, new DSP, copying developer
samples/settings, replacing the Human environment/Native/App, persistent PATH changes,
remote setup, PRs or binary Release publication. No GUI/device/hearing/clean-Windows
acceptance is claimed without its actual Human run. Installer, inference and file
work remain outside realtime; the callback performs none of this work.
