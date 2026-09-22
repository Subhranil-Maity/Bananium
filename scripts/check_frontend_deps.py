#!/usr/bin/env python3
"""Enforce PLAN.md's frontend contract, rule 1: a frontend crate may depend
on bananium-api and its own UI libraries, and nothing else in the workspace.

Exits non-zero (and prints exactly what's wrong) the moment a frontend
crate's Cargo.toml names another `bananium-*` workspace crate as a
dependency. That's the one check standing between "the contract is a
comment in PLAN.md" and "the contract is actually true."
"""

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRONTEND_CRATES = ["bananium-cli", "bananium-tui", "bananium-rpc"]
ALLOWED_WORKSPACE_DEP = "bananium-api"


def dependency_tables(manifest: dict) -> list[dict]:
    tables = [manifest.get("dependencies", {})]
    for target in manifest.get("target", {}).values():
        tables.append(target.get("dependencies", {}))
    return tables


def check_crate(name: str) -> list[str]:
    manifest_path = ROOT / "crates" / name / "Cargo.toml"
    if not manifest_path.is_file():
        return [f"{name}: Cargo.toml not found at {manifest_path}"]

    manifest = tomllib.loads(manifest_path.read_text())
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
    for crate in FRONTEND_CRATES:
        violations.extend(check_crate(crate))

    if violations:
        print("frontend-dependency check FAILED:")
        for v in violations:
            print(f"  - {v}")
        return 1

    print(f"frontend-dependency check OK: {', '.join(FRONTEND_CRATES)} depend only on {ALLOWED_WORKSPACE_DEP}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
