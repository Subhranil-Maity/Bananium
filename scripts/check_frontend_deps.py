#!/usr/bin/env python3
"""Enforce PLAN.md's frontend contract, rule 1: a frontend crate may depend
on bananium-api and its own UI libraries, and nothing else in the workspace.

Exits non-zero (and prints exactly what's wrong) the moment a frontend
crate's Cargo.toml names a `bananium-*` workspace crate as a dependency
other than bananium-api. That's the one check standing between "the
contract is a comment in PLAN.md" and "the contract is actually true."
"""

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Frontend crate name -> its manifest, relative to the repo root. The Tauri
# desktop app lives outside `crates/` because its Rust crate sits inside the
# web project (`desktop/src-tauri`), per Tauri's standard layout.
FRONTEND_CRATES = {
    "bananium-cli": "crates/bananium-cli/Cargo.toml",
    "bananium-tui": "crates/bananium-tui/Cargo.toml",
    "bananium-rpc": "crates/bananium-rpc/Cargo.toml",
    "bananium-desktop": "desktop/src-tauri/Cargo.toml",
}
ALLOWED_WORKSPACE_DEP = "bananium-api"


def dependency_tables(manifest: dict) -> list[dict]:
    tables = [manifest.get("dependencies", {})]
    for target in manifest.get("target", {}).values():
        tables.append(target.get("dependencies", {}))
    return tables


def check_crate(name: str, manifest_rel: str) -> list[str]:
    manifest_path = ROOT / manifest_rel
    if not manifest_path.is_file():
        return [f"{name}: Cargo.toml not found at {manifest_path}"]

    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    violations = []
    for deps in dependency_tables(manifest):
        for dep_name in deps:
            if dep_name.startswith("bananium-") and dep_name != ALLOWED_WORKSPACE_DEP:
                violations.append(
                    f"{name} depends on {dep_name!r}, but frontend crates may only depend on "
                    f"{ALLOWED_WORKSPACE_DEP!r} within the workspace (see PLAN.md: 'The frontend contract')"
                )
    return violations


def main() -> int:
    violations = []
    for crate, manifest in FRONTEND_CRATES.items():
        violations.extend(check_crate(crate, manifest))

    if violations:
        print("frontend-dependency check FAILED:")
        for v in violations:
            print(f"  - {v}")
        return 1

    print(
        f"frontend-dependency check OK: {', '.join(FRONTEND_CRATES)} depend only on "
        f"{ALLOWED_WORKSPACE_DEP}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
