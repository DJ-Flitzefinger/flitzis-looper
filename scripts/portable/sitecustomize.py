"""Windows long DLL paths for the private runtime and its offline child processes."""

# Copied only into Portable's private site-packages, never the developer runtime.
# ruff: noqa: INP001
import ctypes
import os

if os.name == "nt":
    _original_library_init = ctypes.CDLL.__init__

    def _long_library_init(self, name, *arguments, **keywords):
        # Some packages canonicalize SUBST paths before ctypes.LoadLibrary.
        # Normalize only long absolute filenames; preserve names, handles, modes,
        # errors and already-verbatim paths. WinDLL inherits this constructor.
        if isinstance(name, (str, os.PathLike)):
            path = os.fspath(name)
            if isinstance(path, str) and len(path) >= 248 and os.path.isabs(path):
                path = os.path.abspath(path)
                if not path.startswith("\\\\?\\"):
                    name = (
                        "\\\\?\\UNC\\" + path[2:] if path.startswith("\\\\") else "\\\\?\\" + path
                    )
        _original_library_init(self, name, *arguments, **keywords)

    ctypes.CDLL.__init__ = _long_library_init
