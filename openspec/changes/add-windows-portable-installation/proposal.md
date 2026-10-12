# Windows portable installation

The developer launchers rebuild the shared checkout and cannot provide an
independent installation for Human testing during development. Add the requested
single-file `INSTALL-LOOPER-PORTABLE.bat`, a prebuilt private runtime bundle,
separate persistent data and safe repeatable updates.

The installer supports a published HTTPS manifest and a generated personal local
manifest channel. A personal BAT remains usable from another folder on the same
computer. Publishing a public binary release remains a separate authorized step;
the current repository has no such release. Rust compilation and developer tools
are not required on the target machine.

Non-goals: change audio/DSP, copy developer projects or private models, install
machine-wide runtimes, alter the existing developer starters, publish a GitHub
release without authority, or claim Human device/hearing acceptance.
