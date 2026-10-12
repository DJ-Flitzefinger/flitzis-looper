"""Prove the file inventory of a recognized schema-one Portable runtime."""

import argparse
import ast
import base64
import csv
import hashlib
import io
import json
import os
import re
import stat
import subprocess
from dataclasses import dataclass
from email.parser import Parser
from pathlib import Path
from zipfile import ZipFile


@dataclass(slots=True)
class GitSource:
    executable: str
    directory: Path
    commit: str

    def read(self, *arguments: str) -> bytes:
        return subprocess.check_output([
            self.executable,
            "-C",
            str(self.directory),
            *arguments,
        ])

    def blob(self, relative: str) -> bytes:
        return self.read("show", f"{self.commit}:{relative}")


def digest(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def candidate_digest(path: Path, preserved: Path) -> str:
    try:
        return digest(path)
    except OSError as error:
        message = f"Candidate content proof unavailable; legacy file preserved: {preserved}"
        raise ValueError(message) from error


def files(root: Path) -> list[Path]:
    if root.exists() and (
        root.stat(follow_symlinks=False).st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT
    ):
        message = f"Legacy proof directory contains a link; preserved: {root}"
        raise ValueError(message)
    result = []
    for directory, children, names in os.walk(root, followlinks=False):
        for name in [*children, *names]:
            path = Path(directory) / name
            attributes = path.stat(follow_symlinks=False).st_file_attributes
            if attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                message = f"Legacy runtime contains a link; preserved: {path}"
                raise ValueError(message)
        result.extend(Path(directory) / name for name in names)
        if len(result) > 200_000:
            message = "Legacy file inventory exceeds its bounded limit"
            raise ValueError(message)
    return result


def read_release(legacy: Path, commit: str) -> dict:
    old = json.loads((legacy / "release-manifest.json").read_text(encoding="utf-8-sig"))
    if old.get("schema_version") != 1 or old.get("git_commit") != commit or old.get("source_dirty"):
        message = "Legacy release does not prove the accepted exact source commit"
        raise ValueError(message)
    if old.get("python_version") != "3.14.2":
        message = "Unrecognized legacy interpreter; installation preserved"
        raise ValueError(message)
    if old.get("native_source_commit") != commit or not re.fullmatch(
        r"[a-f0-9]{64}",
        str(old.get("native_sha256", "")),
    ):
        message = "Legacy Native source/checksum identity is not authoritative"
        raise ValueError(message)
    return old


def read_builder(git: GitSource) -> dict:
    builder = ast.parse(git.blob("scripts/portable/build_bundle.py").decode("utf-8"))
    constants = {
        node.targets[0].id: ast.literal_eval(node.value)
        for node in builder.body
        if isinstance(node, ast.Assign)
        and len(node.targets) == 1
        and isinstance(node.targets[0], ast.Name)
        and node.targets[0].id in {"DLL_NAMES", "LICENSE_NAMES", "STARTER"}
    }
    if set(constants) != {"DLL_NAMES", "LICENSE_NAMES", "STARTER"}:
        message = "Unrecognized legacy builder ownership contract"
        raise ValueError(message)
    return constants


def add_proof(expected: dict[str, str], relative: str, checksum: str) -> None:
    path = Path(relative)
    if path.is_absolute() or ".." in path.parts or ":" in relative or "\\" in relative:
        message = f"Unsafe legacy file inventory: {relative}"
        raise ValueError(message)
    if not re.fullmatch(r"[a-f0-9]{64}", checksum):
        message = f"Missing authoritative legacy content proof: {relative}"
        raise ValueError(message)
    relative = relative.casefold()
    previous = expected.get(relative)
    if previous and checksum and previous != checksum:
        message = f"Conflicting legacy file proofs: {relative}"
        raise ValueError(message)
    expected[relative] = checksum


def prove_private_tools(expected: dict[str, str], candidate: Path) -> None:
    # The pinned interpreter excludes dependencies, which have their own RECORDs.
    for path in files(candidate / "python"):
        relative = path.relative_to(candidate / "python").as_posix()
        if not relative.startswith("Lib/site-packages/"):
            add_proof(expected, "python/" + relative, digest(path))
    for path in files(candidate / "runtime/ffmpeg"):
        relative = path.relative_to(candidate / "runtime/ffmpeg").as_posix()
        add_proof(expected, "runtime/ffmpeg/" + relative, digest(path))


def prove_source(expected: dict[str, str], git: GitSource) -> None:
    tracked = (
        git
        .read(
            "ls-tree",
            "-r",
            "--name-only",
            "-z",
            git.commit,
            "--",
            "src/flitzis_looper",
            "src/flitzis_looper_audio",
        )
        .decode("utf-8")
        .split("\0")
    )
    for relative in tracked:
        if relative and Path(relative).suffix in {".py", ".pyi"}:
            target = "app/" + Path(relative).relative_to("src").as_posix()
            add_proof(expected, target, hashlib.sha256(git.blob(relative)).hexdigest())
    for name in ["bootstrap.py", "launch.ps1", "dispatch.ps1", "common.ps1", "build-path.ps1"]:
        checksum = hashlib.sha256(git.blob(f"scripts/portable/{name}")).hexdigest()
        add_proof(expected, name, checksum)
    checksum = hashlib.sha256(git.blob("assets/models/keynet.onnx")).hexdigest()
    add_proof(expected, "python/keynet.onnx", checksum)
    checksum = hashlib.sha256(git.blob("scripts/portable/sitecustomize.py")).hexdigest()
    add_proof(expected, "python/Lib/site-packages/sitecustomize.py", checksum)


def prove_builder_outputs(
    expected: dict[str, str],
    legacy: Path,
    old: dict,
    constants: dict,
    git: GitSource,
    candidate: Path,
) -> None:
    native_files = list((legacy / "app/flitzis_looper_audio").glob("*.pyd"))
    if len(native_files) != 1:
        message = "Legacy Native extension is not unique"
        raise ValueError(message)
    add_proof(expected, native_files[0].relative_to(legacy).as_posix(), old["native_sha256"])
    for name in constants["DLL_NAMES"]:
        checksum = candidate_digest(
            git.directory / "src/flitzis_looper_audio" / name,
            legacy / "app/flitzis_looper_audio" / name,
        )
        add_proof(expected, "app/flitzis_looper_audio/" + name, checksum)
        if name.startswith("vcruntime"):
            add_proof(expected, "python/" + name, checksum)
    checksum = hashlib.sha256(constants["STARTER"].encode("ascii")).hexdigest()
    add_proof(expected, "START-LOOPER.bat", checksum)
    for name in constants["LICENSE_NAMES"]:
        checksum = candidate_digest(candidate / "licenses" / name, legacy / "licenses" / name)
        add_proof(expected, "licenses/" + name, checksum)
    for name, relative in [
        ("flitzis-looper.txt", "LICENSE.txt"),
        ("bs-roformer-NOTICE.md", "vendor/bs_roformer/NOTICE.md"),
    ]:
        add_proof(expected, "licenses/" + name, hashlib.sha256(git.blob(relative)).hexdigest())


def observe_distribution(
    distribution: Path,
    base: Path,
    site: Path,
    declared: dict[str, str],
    observed: dict[str, str],
) -> None:
    metadata = Parser().parsestr((distribution / "METADATA").read_text(encoding="utf-8"))
    name = metadata["Name"].lower().replace("_", "-")
    version = metadata["Version"]
    if base == site:
        if declared.get(name) != version or name in observed:
            message = f"Unrecognized legacy dependency: {distribution}"
            raise ValueError(message)
        observed[name] = version
    elif name != "flitzis-looper":
        message = f"Unrecognized Native wheel metadata: {distribution}"
        raise ValueError(message)


def record_hash(checksum: str, record: Path) -> str | None:
    if not checksum:
        return None
    algorithm, encoded = checksum.split("=", 1)
    if algorithm != "sha256":
        message = f"Unsupported legacy RECORD checksum: {record}"
        raise ValueError(message)
    return base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4)).hex()


def assert_private_pth(path: Path, base: Path) -> None:
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith(("#", "import ")):
            continue
        if not (base / line).resolve().is_relative_to(base.resolve()):
            message = f"External legacy Python path preserved: {path}"
            raise ValueError(message)


def record_rows(record: Path) -> list[tuple[str, str, str]]:
    rows = csv.reader(io.StringIO(record.read_text(encoding="utf-8")))
    return [(relative.replace("\\", "/"), checksum, extent) for relative, checksum, extent in rows]


def included_row(relative: str) -> bool:
    path = Path(relative)
    return path.name != "direct_url.json" and "__pycache__" not in path.parts


def candidate_distribution(distribution: Path, site: Path) -> Path:
    old = Parser().parsestr((distribution / "METADATA").read_text(encoding="utf-8"))
    candidates = []
    for candidate in site.glob("*.dist-info"):
        metadata = Parser().parsestr((candidate / "METADATA").read_text(encoding="utf-8"))
        if metadata["Name"] == old["Name"] and metadata["Version"] == old["Version"]:
            candidates.append(candidate)
    if len(candidates) != 1:
        message = f"Matching candidate distribution proof unavailable; preserved: {distribution}"
        raise ValueError(message)
    return candidates[0]


def prove_dependency_record(
    expected: dict[str, str],
    distribution: Path,
    base: Path,
    legacy: Path,
    candidate: Path,
) -> None:
    record = distribution / "RECORD"
    authority = candidate_distribution(distribution, candidate / "python/Lib/site-packages")
    rows = record_rows(record)
    authenticated = record_rows(authority / "RECORD")
    # The builders omit direct_url and bytecode leaves, while retaining RECORD.
    if sorted(row for row in rows if included_row(row[0])) != sorted(
        row for row in authenticated if included_row(row[0])
    ):
        message = f"Changed legacy RECORD preserved; activation stopped: {record}"
        raise ValueError(message)
    for relative, checksum, _extent in rows:
        path = (base / relative).resolve()
        if not path.is_relative_to(base.resolve()) or not included_row(relative):
            continue
        if path == record.resolve():
            continue
        if path.suffix == ".pth":
            assert_private_pth(path, base)
        file_hash = candidate_digest(authority.parent / relative, path)
        claimed = record_hash(checksum, record)
        if claimed and claimed != file_hash:
            message = f"Candidate RECORD content proof unavailable; preserved: {record}: {relative}"
            raise ValueError(message)
        add_proof(expected, path.relative_to(legacy.resolve()).as_posix(), file_hash)
    add_proof(expected, record.relative_to(legacy).as_posix(), digest(record))


def wheel_metadata(archive: ZipFile, distribution: Path) -> str | None:
    old = Parser().parsestr((distribution / "METADATA").read_text(encoding="utf-8"))
    for entry in archive.namelist():
        if not entry.endswith(".dist-info/METADATA"):
            continue
        metadata = Parser().parsestr(archive.read(entry).decode("utf-8"))
        if metadata["Name"] == old["Name"] and metadata["Version"] == old["Version"]:
            return str(Path(entry).parent).replace("\\", "/")
    return None


def native_wheel_proofs(source: Path, distribution: Path) -> dict[str, str]:
    if not re.fullmatch(r"s-[a-f0-9]{8}", source.name):
        message = f"Native wheel transaction proof unavailable; preserved: {distribution}"
        raise ValueError(message)
    wheels = source.parent / ("wheels-" + source.name[2:])
    proofs = []
    for wheel in files(wheels):
        if wheel.suffix != ".whl":
            continue
        with ZipFile(wheel) as archive:
            prefix = wheel_metadata(archive, distribution)
            if prefix:
                proofs.append({
                    entry.removeprefix(prefix + "/"): hashlib.sha256(
                        archive.read(entry),
                    ).hexdigest()
                    for entry in archive.namelist()
                    if entry.startswith(prefix + "/") and not entry.endswith(("/", "/RECORD"))
                })
    if len(proofs) != 1:
        message = f"Matching Native wheel proof unavailable; preserved: {distribution}"
        raise ValueError(message)
    return proofs[0]


def verify_native_row(
    expected: dict[str, str],
    path: Path,
    record: Path,
    checksum: str,
    extent: str,
) -> None:
    if path == record.resolve():
        if checksum or extent:
            message = f"Native RECORD self-entry is not canonical; preserved: {record}"
            raise ValueError(message)
        return
    key = path.relative_to(record.parents[2].resolve()).as_posix().casefold()
    claimed = record_hash(checksum, record)
    if key not in expected or claimed != expected[key]:
        message = f"Unauthenticated Native RECORD leaf preserved: {record}: {path}"
        raise ValueError(message)


def prove_native_record(
    expected: dict[str, str],
    distribution: Path,
    base: Path,
    legacy: Path,
    git: GitSource,
) -> None:
    for relative, checksum in native_wheel_proofs(git.directory, distribution).items():
        expected_checksum = (
            hashlib.sha256(git.blob("LICENSE.txt")).hexdigest()
            if Path(relative).name == "LICENSE.txt"
            else checksum
        )
        add_proof(
            expected, (distribution / relative).relative_to(legacy).as_posix(), expected_checksum
        )
    record = distribution / "RECORD"
    seen = set()
    for relative, checksum, extent in record_rows(record):
        path = (base / relative).resolve()
        if not path.is_relative_to(base.resolve()):
            message = f"Escaping Native RECORD leaf preserved: {record}: {relative}"
            raise ValueError(message)
        if path in seen:
            message = f"Duplicate Native RECORD leaf preserved: {record}: {relative}"
            raise ValueError(message)
        seen.add(path)
        verify_native_row(expected, path, record, checksum, extent)
    if record.resolve() not in seen:
        message = f"Native RECORD self-entry missing; preserved: {record}"
        raise ValueError(message)
    add_proof(expected, record.relative_to(legacy).as_posix(), digest(record))


def prove_dependencies(
    expected: dict[str, str],
    legacy: Path,
    old: dict,
    candidate: Path,
    git: GitSource,
) -> None:
    declared = {
        name.lower().replace("_", "-"): version
        for name, version in old["runtime_dependencies"].items()
    }
    observed: dict[str, str] = {}
    site = legacy / "python/Lib/site-packages"
    records = [(directory, site) for directory in site.glob("*.dist-info")]
    records.extend(
        (directory, legacy / "app") for directory in (legacy / "app").glob("*.dist-info")
    )
    for distribution, base in records:
        observe_distribution(distribution, base, site, declared, observed)
        if base == site:
            prove_dependency_record(expected, distribution, base, legacy, candidate)
        else:
            prove_native_record(expected, distribution, base, legacy, git)
    if declared != observed:
        message = "Legacy dependency inventory is incomplete"
        raise ValueError(message)


def verify_legacy(expected: dict[str, str], legacy: Path) -> dict[str, str]:
    result = {}
    for path in files(legacy):
        relative = path.relative_to(legacy).as_posix()
        if relative == "release-manifest.json":
            continue
        key = relative.casefold()
        if key not in expected:
            message = f"Unknown legacy file preserved; activation stopped: {path}"
            raise ValueError(message)
        checksum = digest(path)
        if expected[key] and checksum != expected[key]:
            message = f"Changed legacy file preserved; activation stopped: {path}"
            raise ValueError(message)
        result[relative] = checksum
    return result


def inventory(
    git_exe: str,
    source: Path,
    candidate: Path,
    legacy: Path,
    commit: str,
) -> dict[str, str]:
    """Require every legacy leaf to match an authoritative installed file path."""
    git = GitSource(git_exe, source, commit)
    old = read_release(legacy, commit)
    constants = read_builder(git)
    expected: dict[str, str] = {}
    prove_private_tools(expected, candidate)
    prove_source(expected, git)
    prove_builder_outputs(expected, legacy, old, constants, git, candidate)
    prove_dependencies(expected, legacy, old, candidate, git)
    return verify_legacy(expected, legacy)


def main() -> None:
    """Write a bounded proof for the installer before its first activation rename."""
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["source", "candidate", "legacy", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--git", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    root = args.source.resolve().parents[1]
    if (
        args.candidate.resolve().parents[1] != root
        or args.legacy.resolve().parents[1] != root
        or args.output.resolve().parent != root / "cache"
        or not re.fullmatch(r"legacy-owner-[a-f0-9]{32}\.json", args.output.name)
        or not re.fullmatch(r"[a-f0-9]{40}", args.commit)
    ):
        message = "Legacy proof paths leave the owned installation transaction"
        raise ValueError(message)
    result = inventory(
        args.git,
        args.source.resolve(),
        args.candidate.resolve(),
        args.legacy.resolve(),
        args.commit,
    )
    temporary = args.output.with_suffix(".tmp")
    with temporary.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, sort_keys=True)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(args.output)


if __name__ == "__main__":
    main()
