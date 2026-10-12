# Portable runtime and updates

The Windows x64 bundle contains relocatable CPython matching the native ABI,
the non-editable Release extension, Python application source, the installed
non-development dependency closure, required native DLLs, FFmpeg/FFprobe,
keynet and licenses. A builder records actual source/build identities and
creates a hashed ZIP plus channel manifest. It does not copy private audio,
settings, project assets, model checkpoints or the developer virtual environment.

The one-file BAT contains its PowerShell installer. An installation owns
`versions`, `data`, `models`, `cache` and `logs`. A new bundle is extracted into
a fresh version directory and verified before atomic activation via
`current.json`. Existing runtime directories are retained, including for an
already running process. A lock serializes installers; ordinary launches only
read the selected version and run its private Python. No updater starts the GUI
or opens an audio device.

A stable root dispatcher resolves the pointer once and invokes that exact
version's launch script, including after an update. Staging and actual starts
therefore use the same version-specific setup. The base interpreter's existing
site-packages is excluded before copying the declared dependency closure.

Launches use stable `data` as their working directory and explicit local model,
Torch/cache, FFmpeg and DLL paths. The existing standard model cache remains the
default for developer launches; Portable supplies an explicit override. Optional
Beat This diagnostic setup is separate from the required normal App runtime.

A generated personal installer embeds the local channel URI, allowing the Human
to copy that BAT into another folder and install there. Generic distribution
uses the GitHub HTTPS release channel after a binary release is authorized and
published. It cannot honestly promise downloads before that artifact exists.
