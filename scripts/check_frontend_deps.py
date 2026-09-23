#!/usr/bin/env python3
"""Enforce PLAN.md's frontend contract, rule 1: a frontend crate may depend
on bananium-api and its own UI libraries, and nothing else in the workspace
— except bananium-cli, which may also embed bananium-egui (see
EXTRA_ALLOWED_DEPS below for why that's not actually an exception to the
rule's substance).

Exits non-zero (and prints exactly what's wrong) the moment a frontend
crate's Cargo.toml names a `bananium-*` workspace crate as a dependency
that isn't on its allowed list. That's the one check standing between "the
contract is a comment in PLAN.md" and "the contract is actually true."
"""

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRONTEND_CRATES = ["bananium-cli", "bananium-tui", "bananium-rpc", "bananium-egui"]
ALLOWED_WORKSPACE_DEP = "bananium-api"

# bananium-cli embeds bananium-egui directly so `bananium --gui` is one
# binary rather than a second executable to build and ship. This is still
# the frontend contract holding, not an exception to it: bananium-egui is
# itself a frontend crate constrained to depend on nothing but
# bananium-api, so no business logic reaches bananium-cli that doesn't
# already flow through bananium-api. Only this one composition is allowed —
# frontend crates may not depend on each other freely.
EXTRA_ALLOWED_DEPS = {
    "bananium-cli": {"bananium-egui"},
}


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
    allowed = {ALLOWED_WORKSPACE_DEP} | EXTRA_ALLOWED_DEPS.get(name, set())
    violations = []
    for deps in dependency_tables(manifest):
        for dep_name in deps:
            if dep_name.startswith("bananium-") and dep_name not in allowed:
                violations.append(
                    f"{name} depends on {dep_name!r}, but frontend crates may only depend on "
                    f"{sorted(allowed)!r} within the workspace (see PLAN.md: 'The frontend contract')"
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

    print(
        f"frontend-dependency check OK: {', '.join(FRONTEND_CRATES)} depend only on "
        f"{ALLOWED_WORKSPACE_DEP} (bananium-cli may also embed bananium-egui for --gui)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
