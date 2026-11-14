"""Authenticate the launchers of a Portable version retained for rollback."""

import argparse
import ast
import hashlib
import json
import os
import re
import subprocess
from pathlib import Path


def legacy_launchers(git: str, source: Path, commit: str) -> dict[str, str]:
    """Use the exact historical source rather than the preserved runtime's bytes."""

    def blob(relative: str) -> bytes:
        return subprocess.check_output([git, "-C", str(source), "show", f"{commit}:{relative}"])

    builder = ast.parse(blob("scripts/portable/build_bundle.py").decode("utf-8"))
    definitions = [
        node.value
        for node in builder.body
        if isinstance(node, ast.Assign)
        and len(node.targets) == 1
        and isinstance(node.targets[0], ast.Name)
        and node.targets[0].id == "STARTER"
    ]
    if len(definitions) != 1:
        message = "Unrecognized legacy launcher declaration; existing starters are preserved."
        raise ValueError(message)
    starter = ast.literal_eval(definitions[0])
    if not isinstance(starter, str) or not starter:
        message = "Invalid legacy launcher declaration; existing starters are preserved."
        raise ValueError(message)
    dispatcher = blob("scripts/portable/dispatch.ps1")
    proof = {
        "START-LOOPER.bat": hashlib.sha256(starter.encode("ascii")).hexdigest(),
        "dispatch.ps1": hashlib.sha256(dispatcher).hexdigest(),
    }
    # Historical builders copied the checkout's dispatcher as bytes. For a
    # pure-LF blob, Git can emit the same file with CRLF on Windows.
    if b"\n" in dispatcher and b"\r" not in dispatcher and b"\0" not in dispatcher:
        proof["dispatch.ps1.crlf"] = hashlib.sha256(dispatcher.replace(b"\n", b"\r\n")).hexdigest()
    return proof


def main() -> None:
    """Write only the launcher ownership proof in the transaction's cache."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--git", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    if (
        args.output.resolve().parent != args.source.resolve().parent
        or not re.fullmatch(r"legacy-launchers-[a-f0-9]{32}\.json", args.output.name)
        or not re.fullmatch(r"[a-f0-9]{40}", args.commit)
    ):
        message = "Legacy launcher proof leaves the owned installation transaction"
        raise ValueError(message)
    proof = legacy_launchers(args.git, args.source, args.commit)
    temporary = args.output.with_suffix(".tmp")
    with temporary.open("x", encoding="utf-8") as stream:
        json.dump(proof, stream, sort_keys=True)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.rename(args.output)


if __name__ == "__main__":
    main()
