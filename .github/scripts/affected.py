#!/usr/bin/env python3
"""Decide which checks a change needs (CI's "What changed" job; docs/notes/checks.md).

Reads the changed file paths (one per line on stdin, or the files changed since a git commit given as the first
argument; `--all` runs everything) and prints GitHub Actions outputs:

  code=true|false     false when only notes and docs changed: no build, lint or tests at all
  full=true|false     true when a shared file changed (Cargo.lock, the workspace Cargo.toml, CI, shared assets ...)
  packages=a b ...    the crates to test: the changed crates plus every crate that depends on them
  filter=...          the cargo-nextest filterset for those crates (all() on a full run)
  goldens_kit, goldens_sound, windows = true|false

Anything this script doesn't recognise runs everything: a skipped test must never be a surprise.
"""

import subprocess
import sys
import tomllib
from fnmatch import fnmatch
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# Paths that can't change what the code does. Anything else outside a crate runs every check.
DOCS = ["docs/*", "*.md", "LICENSE*", ".gitignore"]
# Files inside a crate that other crates read too (bpe-kit builds bpe-studio's fonts in).
SHARED_IN_CRATES = ["crates/*/assets/*"]


def workspace_crates():
    """Crate folder name -> (package name, set of workspace crates it depends on, any kind)."""
    crates = {}
    for manifest in sorted(ROOT.glob("crates/*/Cargo.toml")):
        data = tomllib.loads(manifest.read_text())
        tables = [data]
        tables += list(data.get("target", {}).values())
        deps = set()
        for table in tables:
            for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
                deps.update(table.get(kind, {}).keys())
        crates[manifest.parent.name] = (data["package"]["name"], deps)
    return crates


def decide(paths):
    crates = workspace_crates()
    names = {name for name, _ in crates.values()}
    changed, full, code = set(), False, False
    for path in paths:
        if any(fnmatch(path, pattern) for pattern in DOCS) and not path.startswith("crates/"):
            continue
        if path.startswith("crates/") and path.endswith(".md") and "/src/" not in path:
            continue  # a crate's README or notes, not code
        code = True
        parts = path.split("/")
        if any(fnmatch(path, pattern) for pattern in SHARED_IN_CRATES):
            full = True
        elif len(parts) >= 3 and parts[0] == "crates" and parts[1] in crates:
            changed.add(crates[parts[1]][0])
        else:
            full = True  # Cargo.lock, Cargo.toml, rust-toolchain.toml, .github, .config, assets, a removed crate ...

    if full:
        packages = set(names)
    else:
        # Everything that depends on a changed crate, directly or through others.
        packages = set(changed)
        grew = True
        while grew:
            grew = False
            for name, deps in crates.values():
                if name not in packages and deps & packages:
                    packages.add(name)
                    grew = True

    flag = lambda b: "true" if b else "false"
    return {
        "code": flag(code),
        "full": flag(full),
        "packages": " ".join(sorted(packages)),
        "filter": "all()" if full else " | ".join(f"package({p})" for p in sorted(packages)) or "none()",
        "goldens_kit": flag(code and "bpe-kit" in packages),
        "goldens_sound": flag(code and "bpe-sound" in packages),
        "windows": flag(code and bool({"bpe-studio", "bpe-cli"} & packages)),
    }


def main():
    if sys.argv[1:] == ["--all"]:
        paths = ["Cargo.toml"]  # a shared file: every check
    elif len(sys.argv) > 1:
        # --no-renames: a moved file counts at both its old and its new place.
        out = subprocess.run(
            ["git", "diff", "--name-only", "--no-renames", sys.argv[1], "HEAD"], cwd=ROOT, check=True, capture_output=True
        )
        paths = out.stdout.decode().splitlines()
    else:
        paths = sys.stdin.read().splitlines()
    paths = [p.strip() for p in paths if p.strip()]
    for key, value in decide(paths).items():
        print(f"{key}={value}")


if __name__ == "__main__":
    main()
