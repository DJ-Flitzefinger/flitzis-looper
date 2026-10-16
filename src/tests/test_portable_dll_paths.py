"""Exercise private-runtime DLL path normalization in isolated Windows processes."""

import ctypes
import json
import os
import subprocess
from pathlib import Path
from typing import cast

import pytest

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows verbatim DLL paths")
REPO = Path(__file__).resolve().parents[2]
LONG_DIRECTORIES = "\\".join(["private-runtime-segment"] * 14)
LONG_LOCAL = "C:\\private runtime\\" + LONG_DIRECTORIES + "\\dependency.dll"
LONG_UNC = "\\\\server\\private share\\" + LONG_DIRECTORIES + "\\dependency.dll"
VERBATIM_LOCAL = "\\\\?\\" + LONG_LOCAL
VERBATIM_UNC = "\\\\?\\UNC\\" + LONG_UNC[2:]
CHILD_SETUP = """
import ctypes
import json
import runpy
import sys
from pathlib import Path

calls = []
failure = None

def capture_init(self, name, *arguments, **keywords):
    calls.append((self, name, arguments, keywords))
    if failure is not None:
        raise failure

# Install the fake before executing the real customization: no DLL is loaded.
ctypes.CDLL.__init__ = capture_init
runpy.run_path(sys.argv[1])
assert ctypes.CDLL.__init__ is not capture_init
payload = json.loads(sys.argv[2])
name = payload.get("name")
if payload.get("kind") == "pathlike":
    name = Path(name)
elif payload.get("kind") == "bytes":
    name = name.encode("utf-8")
"""


def _run_child(commands: str, payload: dict[str, str | None]) -> dict[str, object]:
    original_init = ctypes.CDLL.__init__
    environment = dict(os.environ, UV_NO_SYNC="1")
    result = subprocess.run(
        [
            str(REPO / ".venv/Scripts/python.exe"),
            "-I",
            "-S",
            "-B",
            "-c",
            CHILD_SETUP + commands,
            str(REPO / "scripts/portable/sitecustomize.py"),
            json.dumps(payload),
        ],
        cwd=REPO,
        env=environment,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=15,
        check=False,
    )
    assert ctypes.CDLL.__init__ is original_init
    assert result.returncode == 0, result.stdout + result.stderr
    # The isolated child fixture emits one JSON object with named fields.
    return cast("dict[str, object]", json.loads(result.stdout))


@pytest.mark.parametrize("kind", ["string", "pathlike"])
@pytest.mark.parametrize(
    ("name", "expected"),
    [
        pytest.param(LONG_LOCAL, VERBATIM_LOCAL, id="local"),
        pytest.param(LONG_UNC, VERBATIM_UNC, id="unc"),
        pytest.param(
            "C:/private runtime/discard/../"
            + LONG_DIRECTORIES.replace("\\", "/")
            + "\\dependency.dll",
            VERBATIM_LOCAL,
            id="mixed-local",
        ),
        pytest.param(
            "//server/private share/" + LONG_DIRECTORIES.replace("\\", "/") + "\\dependency.dll",
            VERBATIM_UNC,
            id="mixed-unc",
        ),
    ],
)
def test_long_absolute_paths_reach_the_loader_as_normalized_verbatim_paths(
    name: str, expected: str, kind: str
) -> None:
    result = _run_child(
        """
ctypes.CDLL(name)
sys.stdout.write(json.dumps({"name": calls[0][1], "calls": len(calls)}))
""",
        {"name": name, "kind": kind},
    )
    assert result == {"name": expected, "calls": 1}


@pytest.mark.parametrize(
    ("name", "kind"),
    [
        pytest.param("dependency.dll", "string", id="short-name"),
        pytest.param(r"C:\private runtime\dependency.dll", "string", id="short-absolute"),
        pytest.param(LONG_DIRECTORIES + "\\dependency.dll", "string", id="long-relative"),
        pytest.param(VERBATIM_LOCAL, "string", id="verbatim-local"),
        pytest.param(VERBATIM_UNC, "string", id="verbatim-unc"),
        pytest.param(None, "string", id="none"),
        pytest.param("dependency.dll", "bytes", id="short-bytes"),
        pytest.param(LONG_LOCAL, "bytes", id="long-bytes"),
        pytest.param(r"C:\private runtime\dependency.dll", "pathlike", id="short-pathlike"),
        pytest.param(VERBATIM_LOCAL, "pathlike", id="verbatim-pathlike"),
    ],
)
def test_other_library_names_are_forwarded_as_the_original_object(
    name: str | None, kind: str
) -> None:
    result = _run_child(
        """
ctypes.CDLL(name)
sys.stdout.write(json.dumps({"same_object": calls[0][1] is name, "calls": len(calls)}))
""",
        {"name": name, "kind": kind},
    )
    assert result == {"same_object": True, "calls": 1}


@pytest.mark.parametrize("library", ["CDLL", "WinDLL"])
def test_constructor_arguments_and_keywords_reach_the_original_loader(library: str) -> None:
    result = _run_child(
        """
handle = object()
arguments = (7, handle)
keywords = {"use_errno": True, "use_last_error": True, "winmode": 0x1000}
instance = getattr(ctypes, payload["library"])(name, *arguments, **keywords)
assert calls[0][0] is instance
assert calls[0][2] == arguments
assert calls[0][2][1] is handle
assert calls[0][3] == keywords
sys.stdout.write(json.dumps({"name": calls[0][1], "calls": len(calls)}))
""",
        {"name": LONG_LOCAL, "library": library},
    )
    assert result == {"name": VERBATIM_LOCAL, "calls": 1}


@pytest.mark.parametrize("library", ["CDLL", "WinDLL"])
def test_original_loader_error_is_propagated_without_replacement(library: str) -> None:
    result = _run_child(
        """
failure = OSError(126, "fixture missing dependency")
try:
    getattr(ctypes, payload["library"])(name, use_last_error=True)
except OSError as caught:
    assert caught is failure
else:
    raise AssertionError("Expected the original loader failure")
assert calls[0][3] == {"use_last_error": True}
sys.stdout.write(json.dumps({"name": calls[0][1], "calls": len(calls)}))
""",
        {"name": LONG_UNC, "library": library},
    )
    assert result == {"name": VERBATIM_UNC, "calls": 1}
