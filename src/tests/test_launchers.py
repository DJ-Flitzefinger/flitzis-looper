"""Execute Windows BAT routing with harmless modules/tools, without an app or device."""

import json
import os
import shutil
import subprocess
import venv
from dataclasses import dataclass
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows BAT entrypoints")

_REPOSITORY = Path(__file__).resolve().parents[2]
_NATIVE_STUB = """
import os
from pathlib import Path
Path(os.environ['LAUNCHER_PROFILE_LOG']).write_text('checked', encoding='utf-8')
if os.environ['LAUNCHER_NATIVE_MODE'] == 'import-error':
    raise ImportError('deliberate native import failure')
if os.environ['LAUNCHER_NATIVE_MODE'] != 'older':
    def native_build_profile():
        mode = os.environ['LAUNCHER_NATIVE_MODE']
        if (
            mode in {'old-piano', 'old-recording', 'old-editor', 'old-residency', 'old-ordinary'}
            or mode.startswith('bpm-runtime-')
            or mode.startswith('ordinary-')
        ):
            return 'release'
        return mode
class CapturedPitchAction:
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'old-piano':
        resets_key_correction = property(lambda self: False)
class AudioEngine:
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'bpm-runtime-missing':
        BPM_ONLY_RUNTIME_API_VERSION = {
            'bpm-runtime-bool': True, 'bpm-runtime-old': 0,
        }.get(os.environ['LAUNCHER_NATIVE_MODE'], 1)
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'old-ordinary':
        ORDINARY_TRANSPORT_API_VERSION = {
            'ordinary-version-false': False,
            'ordinary-version-true': True,
            'ordinary-version-old': 1,
            'ordinary-version-future': 3,
        }.get(os.environ['LAUNCHER_NATIVE_MODE'], 2)
    def pause_sample(self, id):
        raise AssertionError('launcher must never pause audio')
    def resume_sample(self, id):
        raise AssertionError('launcher must never resume audio')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'ordinary-missing-resolver':
        def _resolve_ordinary_transport_feedback(self, message):
            raise AssertionError('launcher must never resolve playback')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'ordinary-missing-current-query':
        def _current_ordinary_transport_observation(self, id):
            raise AssertionError('launcher must never query current content playback')
    RESIDENT_PCM_ADMISSION_VERSION = (
        1 if os.environ['LAUNCHER_NATIVE_MODE'] == 'old-residency' else 2
    )
    def __init__(self):
        raise AssertionError('launcher must never construct an engine')
    def set_input_runtime_state(
        self, multi_loop, loaded, loop_starts, loop_ends, bindings=None,
        pitch_rows=None,
    ):
        raise AssertionError('launcher must only reflect timing policy')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'old-recording':
        def audio_stream_failed(self):
            raise AssertionError('launcher must only reflect stream lifecycle')
        def quiesce_failed_audio_stream(self):
            raise AssertionError('launcher must never close a stream')
class OrdinaryTransportAdmission:
    content_lifetime = property(lambda self: 1)
    transport_revision = property(lambda self: 1)
    requested_paused = property(lambda self: True)
    def current_control_slot(self):
        raise AssertionError('launcher must never inspect content placement')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'ordinary-missing-query':
        def current_observation(self):
            raise AssertionError('launcher must never inspect playback state')
class OrdinaryTransportObservation:
    sample_id = property(lambda self: 0)
    content_lifetime = property(lambda self: 1)
    transport_revision = property(lambda self: 1)
    observation_sequence = property(lambda self: 1)
    status_kind = property(lambda self: 'transport')
    requested_effect = property(lambda self: 'pause')
    outcome = property(lambda self: 'applied')
    active = property(lambda self: True)
    ordinary_paused = property(lambda self: True)
    def matches_admission(self, admission):
        raise AssertionError('launcher must never match a playback admission')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'ordinary-missing-settlement':
        def settles_admission(self, admission):
            raise AssertionError('launcher must never settle a playback admission')
    def current_control_slot(self):
        raise AssertionError('launcher must never inspect observed content placement')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'ordinary-missing-observation-query':
        def current_observation(self):
            raise AssertionError('launcher must never query observed content state')
class AudioMessage:
    class OrdinaryTransportFeedback:
        owner = property(lambda self: 1)
        cell = property(lambda self: 0)
        lifetime = property(lambda self: 1)
        transport_revision = property(lambda self: 1)
        observation_sequence = property(lambda self: 1)
        def feedback_kind(self):
            raise AssertionError('launcher must only reflect playback feedback')
"""
_APP_STUB = """
import json
import os
import sys
from pathlib import Path
Path(os.environ['LAUNCHER_APP_LOG']).write_text(
    json.dumps({'cwd': str(Path.cwd()), 'python': sys.executable}), encoding='utf-8',
)
raise SystemExit(int(os.environ['LAUNCHER_APP_EXIT']))
"""
_UV_STUB = """@echo off
echo %*>>"%LAUNCHER_TOOL_LOG%"
if "%~1"=="sync" exit /b %LAUNCHER_SYNC_EXIT%
exit /b %LAUNCHER_BUILD_EXIT%
"""


@dataclass(slots=True)
class _Launchers:
    repository: Path
    caller: Path
    environment: dict[str, str]
    tool_log: Path
    profile_log: Path
    app_log: Path
    pause_prompt: str

    def run(self, name: str, *arguments: str) -> subprocess.CompletedProcess[str]:
        command = f'call "{self.repository / name}"'
        if arguments:
            command += " " + " ".join(arguments)
        command_processor = os.environ.get("COMSPEC", "cmd.exe")
        # cmd parses quotes itself; list2cmdline's backslash-quote escaping is not its grammar.
        command_line = f'"{command_processor}" /d /s /c "{command}"'
        return subprocess.run(
            command_line,
            cwd=self.caller,
            env=self.environment,
            input="\n",
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )

    def tool_calls(self) -> list[str]:
        if not self.tool_log.exists():
            return []
        return [line.rstrip() for line in self.tool_log.read_text().splitlines()]

    def assert_no_app_or_tools(self) -> None:
        assert not self.app_log.exists()
        assert self.tool_calls() == []


@pytest.fixture(scope="session")
def pause_prompt() -> str:
    result = subprocess.run(
        [os.environ.get("COMSPEC", "cmd.exe"), "/d", "/c", "pause"],
        input="\n",
        capture_output=True,
        text=True,
        timeout=15,
        check=True,
    )
    prompt = result.stdout.strip()
    assert prompt
    return prompt


@pytest.fixture
def launchers(tmp_path: Path, pause_prompt: str) -> _Launchers:
    repository = tmp_path / "repository with spaces"
    caller = tmp_path / "unrelated working directory"
    tools = tmp_path / "harmless tools"
    repository.mkdir()
    caller.mkdir()
    tools.mkdir()
    (repository / "scripts").mkdir()
    (repository / "scripts/portable").mkdir()
    for relative in (
        "start.bat",
        "start-dev.bat",
        "start-release.bat",
        "build-release.bat",
        "scripts/start-app.bat",
        "scripts/portable/bootstrap.py",
    ):
        shutil.copyfile(_REPOSITORY / relative, repository / relative)
    venv.EnvBuilder(with_pip=False).create(repository / ".venv")
    (repository / "flitzis_looper_audio.py").write_text(_NATIVE_STUB, encoding="utf-8")
    (repository / "flitzis_looper").mkdir()
    (repository / "flitzis_looper/__init__.py").write_text("", encoding="utf-8")
    (repository / "flitzis_looper/__main__.py").write_text(_APP_STUB, encoding="utf-8")
    (tools / "uv.cmd").write_text(_UV_STUB, encoding="ascii")
    tool_log = tmp_path / "tool-calls.txt"
    profile_log = tmp_path / "profile-check.txt"
    app_log = tmp_path / "app-receipt.json"
    environment = os.environ.copy()
    environment.update(
        {
            "PATH": str(tools) + os.pathsep + environment.get("PATH", ""),
            "LAUNCHER_NATIVE_MODE": "release",
            "LAUNCHER_SYNC_EXIT": "0",
            "LAUNCHER_BUILD_EXIT": "0",
            "LAUNCHER_APP_EXIT": "0",
            "LAUNCHER_TOOL_LOG": str(tool_log),
            "LAUNCHER_PROFILE_LOG": str(profile_log),
            "LAUNCHER_APP_LOG": str(app_log),
        },
    )
    environment.pop("PYTHONHOME", None)
    environment.pop("PYTHONPATH", None)
    return _Launchers(repository, caller, environment, tool_log, profile_log, app_log, pause_prompt)


@pytest.mark.parametrize("option", ["", "--check"])
def test_release_start_uses_existing_python_without_any_uv_call(
    launchers: _Launchers,
    option: str,
) -> None:
    launchers.environment["LAUNCHER_SYNC_EXIT"] = "91"
    launchers.environment["LAUNCHER_BUILD_EXIT"] = "92"
    result = launchers.run("start.bat", *([option] if option else []))
    assert result.returncode == 0, result.stdout + result.stderr
    assert launchers.profile_log.read_text() == "checked"
    assert launchers.tool_calls() == []
    assert launchers.pause_prompt not in result.stdout
    assert launchers.app_log.exists() is (not option)
    if not option:
        receipt = json.loads(launchers.app_log.read_text())
        assert Path(receipt["cwd"]) == launchers.repository
        assert Path(receipt["python"]) == launchers.repository / ".venv/Scripts/python.exe"


def test_double_click_release_build_checks_profile_and_keeps_success_visible(
    launchers: _Launchers,
) -> None:
    result = launchers.run("build-release.bat")
    assert result.returncode == 0, result.stdout + result.stderr
    assert launchers.tool_calls() == [
        "sync --locked",
        "run --no-sync maturin develop --locked --release",
    ]
    assert launchers.profile_log.read_text() == "checked"
    assert "Native build profile: release" in result.stdout
    assert "Release build completed. The app was not started." in result.stdout
    assert launchers.pause_prompt in result.stdout
    assert not launchers.app_log.exists()


@pytest.mark.parametrize("profile", ["debug", "older", "old-piano", "import-error", "missing"])
def test_release_rejects_wrong_older_or_unimportable_native_build(
    launchers: _Launchers,
    profile: str,
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = profile
    if profile == "missing":
        (launchers.repository / "flitzis_looper_audio.py").unlink()
    result = launchers.run("start.bat", "--check")
    assert result.returncode != 0
    assert "start-release.bat --build-only" in result.stdout
    assert launchers.pause_prompt not in result.stdout
    assert launchers.profile_log.exists() is (profile != "missing")
    launchers.assert_no_app_or_tools()


def test_missing_environment_does_not_install_or_start(launchers: _Launchers) -> None:
    (launchers.repository / ".venv/Scripts/python.exe").unlink()
    result = launchers.run("start.bat", "--check")
    assert result.returncode == 3
    assert "start-release.bat --build-only" in result.stdout
    assert not launchers.profile_log.exists()
    launchers.assert_no_app_or_tools()


@pytest.mark.parametrize("profile", ["debug", "release"])
@pytest.mark.parametrize("option", ["", "--build-only"])
def test_build_starters_keep_locked_build_and_optional_app(
    launchers: _Launchers,
    profile: str,
    option: str,
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = profile
    starter = "start-dev.bat" if profile == "debug" else "start-release.bat"
    result = launchers.run(starter, *([option] if option else []))
    assert result.returncode == 0, result.stdout + result.stderr
    build_call = "run --no-sync maturin develop --locked"
    if profile == "release":
        build_call += " --release"
    assert launchers.tool_calls() == ["sync --locked", build_call]
    assert launchers.profile_log.read_text() == "checked"
    assert launchers.app_log.exists() is (not option)
    assert launchers.pause_prompt not in result.stdout


@pytest.mark.parametrize(
    ("failed_step", "exit_code", "call_count"),
    [("SYNC", 41, 1), ("BUILD", 42, 2)],
)
@pytest.mark.parametrize("starter", ["start-release.bat", "build-release.bat"])
def test_failed_build_step_returns_exact_exit_without_app_or_profile_check(
    launchers: _Launchers,
    failed_step: str,
    exit_code: int,
    call_count: int,
    starter: str,
) -> None:
    launchers.environment[f"LAUNCHER_{failed_step}_EXIT"] = str(exit_code)
    arguments = ("--build-only",) if starter == "start-release.bat" else ()
    result = launchers.run(starter, *arguments)
    assert result.returncode == exit_code
    assert len(launchers.tool_calls()) == call_count
    assert not launchers.profile_log.exists()
    assert not launchers.app_log.exists()
    assert f"failed (exit code {exit_code})." in result.stdout
    assert (launchers.pause_prompt in result.stdout) is (starter == "build-release.bat")


@pytest.mark.parametrize("starter", ["start-release.bat", "build-release.bat"])
@pytest.mark.parametrize(
    ("profile", "exit_code"),
    [("debug", 3), ("older", 3), ("import-error", 1), ("missing", 1), ("no-python", 3)],
)
def test_build_success_must_match_actual_installed_profile(
    launchers: _Launchers,
    starter: str,
    profile: str,
    exit_code: int,
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = profile
    if profile == "missing":
        (launchers.repository / "flitzis_looper_audio.py").unlink()
    if profile == "no-python":
        (launchers.repository / ".venv/Scripts/python.exe").unlink()
    arguments = ("--build-only",) if starter == "start-release.bat" else ()
    result = launchers.run(starter, *arguments)
    assert result.returncode == exit_code
    assert len(launchers.tool_calls()) == 2
    assert launchers.profile_log.exists() is (profile not in {"missing", "no-python"})
    assert not launchers.app_log.exists()
    assert "A usable Release native build is required." in result.stdout
    assert f"failed (exit code {exit_code})." in result.stdout
    assert (launchers.pause_prompt in result.stdout) is (starter == "build-release.bat")


@pytest.mark.parametrize("starter", ["start.bat", "start-release.bat"])
def test_automatic_memory_requires_current_native_before_application(
    launchers: _Launchers, starter: str
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = "old-residency"
    result = launchers.run(starter)
    assert result.returncode != 0
    assert "Native automatic residency admission requires a current Release build" in (
        result.stdout + result.stderr
    )
    assert not launchers.app_log.exists()
    assert launchers.profile_log.exists()


@pytest.mark.parametrize("starter", ["start.bat", "start-release.bat"])
def test_recording_lifecycle_requires_current_native_before_application(
    launchers: _Launchers, starter: str
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = "old-recording"
    result = launchers.run(starter)
    assert result.returncode != 0
    assert (
        "Native recording lifecycle API requires a current build" in result.stdout + result.stderr
    )
    assert not launchers.app_log.exists()
    assert launchers.profile_log.exists()


@pytest.mark.parametrize("starter", ["start.bat", "start-release.bat"])
@pytest.mark.parametrize(
    "mode",
    [
        "old-ordinary",
        "ordinary-version-false",
        "ordinary-version-true",
        "ordinary-version-old",
        "ordinary-version-future",
        "ordinary-missing-resolver",
        "ordinary-missing-current-query",
        "ordinary-missing-query",
        "ordinary-missing-settlement",
        "ordinary-missing-observation-query",
    ],
)
def test_ordinary_transport_requires_current_native_before_application(
    launchers: _Launchers, starter: str, mode: str
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = mode
    result = launchers.run(starter)
    assert result.returncode != 0
    assert "Native ordinary transport API" in result.stdout + result.stderr
    assert not launchers.app_log.exists()
    assert launchers.profile_log.exists()


@pytest.mark.parametrize("starter", ["start.bat", "start-release.bat"])
def test_application_exit_code_is_preserved(launchers: _Launchers, starter: str) -> None:
    launchers.environment["LAUNCHER_APP_EXIT"] = "17"
    result = launchers.run(starter)
    assert result.returncode == 17
    assert launchers.profile_log.exists()
    assert launchers.app_log.exists()
    assert launchers.pause_prompt in result.stdout


@pytest.mark.parametrize(
    ("starter", "arguments"),
    [
        ("start.bat", ("--build-only",)),
        ("start-release.bat", ("--check",)),
        ("start-dev.bat", ("--unknown",)),
        ("start.bat", ("--check", "extra")),
        ("build-release.bat", ("--build-only",)),
        ("build-release.bat", ("--check",)),
        ("build-release.bat", ("--unknown",)),
        ("build-release.bat", ("extra", "more")),
        ("build-release.bat", ('""',)),
        ("build-release.bat", ('"two words"',)),
    ],
)
def test_invalid_arguments_stop_before_setup_or_python(
    launchers: _Launchers,
    starter: str,
    arguments: tuple[str, ...],
) -> None:
    result = launchers.run(starter, *arguments)
    assert result.returncode == 2
    assert "Usage:" in result.stdout
    assert not launchers.profile_log.exists()
    launchers.assert_no_app_or_tools()
    if starter == "build-release.bat":
        assert "build-release.bat" in result.stdout
        assert launchers.pause_prompt in result.stdout


@pytest.mark.parametrize("starter", ["start.bat", "start-release.bat"])
@pytest.mark.parametrize("mode", ["bpm-runtime-missing", "bpm-runtime-bool", "bpm-runtime-old"])
def test_retired_runtime_cannot_start_before_rebuild(
    launchers: _Launchers, starter: str, mode: str
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = mode
    result = launchers.run(starter)
    assert result.returncode != 0
    assert "Native constant-BPM runtime requires a current Release build" in (
        result.stdout + result.stderr
    )
    assert not launchers.app_log.exists()
