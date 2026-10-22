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
            mode in {'old-piano', 'old-recording', 'old-editor', 'old-residency'}
            or mode.startswith('editor-')
        ):
            return 'release'
        return mode
class CapturedPitchAction:
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'old-piano':
        resets_key_correction = property(lambda self: False)
class AudioEngine:
    GLOBAL_TIMING_POLICY_API_VERSION = 1
    RESIDENT_PCM_ADMISSION_VERSION = (
        1 if os.environ['LAUNCHER_NATIVE_MODE'] == 'old-residency' else 2
    )
    def __init__(self):
        raise AssertionError('launcher must never construct an engine')
    def set_input_runtime_state(
        self, multi_loop, loaded, loop_starts, loop_ends, bindings=None,
        pitch_rows=None, source_map_rows=None, *, timing_policy_json=None,
    ):
        raise AssertionError('launcher must only reflect timing policy')
    def pad_source_sync_status(self, id):
        raise AssertionError('launcher must only reflect sync status')
    def begin_source_beatmap_raw_async(self, sample_id, *, expected_source_generation):
        raise AssertionError('launcher must never analyze source audio')
    def begin_source_beatmap_beat_this_raw_async(
        self, sample_id, installation_root, *, expected_source_generation,
    ):
        raise AssertionError('launcher must never run inference')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'old-recording':
        def audio_stream_failed(self):
            raise AssertionError('launcher must only reflect stream lifecycle')
        def quiesce_failed_audio_stream(self):
            raise AssertionError('launcher must never close a stream')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'old-editor':
        def begin_source_beatmap_create_async(
            self, sample_id, origin_seconds, period_seconds, *, expected_source_generation=None,
        ):
            raise AssertionError('launcher must never create a source beatmap')
        def begin_source_beatmap_edit_async(self, sample_id, receipt_json, operation_json):
            raise AssertionError('launcher must never edit a source beatmap')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'editor-missing-review':
        def begin_source_beatmap_review_async(
            self, sample_id, candidate, review_json, *, expected_source_generation,
        ):
            raise AssertionError('launcher must never build a source beatmap')
class SourceBeatmap:
    def __init__(self):
        raise AssertionError('launcher must never construct a map')
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'editor-missing-version':
        EDITOR_CORRECTION_API_VERSION = {
            'editor-version-none': None,
            'editor-version-false': False,
            'editor-version-true': True,
            'editor-version-float': 2.0,
            'editor-version-string': '2',
            'editor-version-old': 1,
            'editor-version-future': 3,
        }.get(os.environ['LAUNCHER_NATIVE_MODE'], 2)
    if os.environ['LAUNCHER_NATIVE_MODE'] != 'editor-missing-span':
        if os.environ['LAUNCHER_NATIVE_MODE'] == 'editor-span-optional':
            def editor_span(self, first_anchor_index, last_anchor_index=None):
                raise AssertionError('launcher must never query a span')
        elif os.environ['LAUNCHER_NATIVE_MODE'] == 'editor-span-keyword-only':
            def editor_span(self, *, first_anchor_index, last_anchor_index):
                raise AssertionError('launcher must never query a span')
        elif os.environ['LAUNCHER_NATIVE_MODE'] == 'editor-span-positional-only':
            def editor_span(self, first_anchor_index, last_anchor_index, /):
                raise AssertionError('launcher must never query a span')
        elif os.environ['LAUNCHER_NATIVE_MODE'] == 'editor-span-renamed':
            def editor_span(self, first_anchor_index, anchor_index):
                raise AssertionError('launcher must never query a span')
        else:
            def editor_span(self, first_anchor_index, last_anchor_index):
                raise AssertionError('launcher must never query a span')
    def editor_info(self):
        raise AssertionError('launcher must only reflect map capabilities')
    def editor_view(self, start_s, end_s, loop_start_s, px_per_s, max_points=2048):
        raise AssertionError('launcher must never query source geometry')
    def source_after_beats(self, source_seconds, beats, require_four_four=True):
        raise AssertionError('launcher must never query source geometry')
    revision = editor_info
    source_identity = editor_info
    evaluate_source = editor_info
    evaluate_beat = editor_info
    local_tempo_at_source = editor_info
    nearest_grid_source = editor_info
class SourceBeatmapRawCandidate:
    RAW_REVIEW_API_VERSION = 2
    def info(self):
        raise AssertionError('launcher must only reflect raw results')
    def inspect(self, start_index, max_events=128):
        raise AssertionError('launcher must never inspect raw results')
    inspect_downbeats = inspect
class OfflineAnalysisJob:
    def resolve_source_beatmap_raw_completion(self, delivery):
        raise AssertionError('launcher must never resolve a worker result')
    def source_beatmap_async_status(self):
        raise AssertionError('launcher must only reflect worker status')
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
        "old-editor",
        "editor-missing-version",
        "editor-version-none",
        "editor-version-false",
        "editor-version-true",
        "editor-version-float",
        "editor-version-string",
        "editor-version-old",
        "editor-version-future",
        "editor-missing-span",
        "editor-missing-review",
        "editor-span-optional",
        "editor-span-keyword-only",
        "editor-span-positional-only",
        "editor-span-renamed",
    ],
)
def test_source_beatmap_editor_requires_current_native_before_application(
    launchers: _Launchers, starter: str, mode: str
) -> None:
    launchers.environment["LAUNCHER_NATIVE_MODE"] = mode
    result = launchers.run(starter)
    assert result.returncode != 0
    assert "Native source beatmap editor API requires a current build" in (
        result.stdout + result.stderr
    )
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
