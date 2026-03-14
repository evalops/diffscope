#!/usr/bin/env python3

from __future__ import annotations

import argparse
import subprocess
import sys
import tomllib
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify Cargo.toml version is aligned with git tags."
    )
    parser.add_argument(
        "--cargo-toml",
        default="Cargo.toml",
        help="Path to the Cargo.toml file to validate.",
    )
    parser.add_argument(
        "--tag",
        help="Require Cargo.toml to match this exact git tag (for release workflows).",
    )
    return parser.parse_args()


def normalize_version(raw: str) -> str:
    value = raw.strip()
    return value[1:] if value.startswith("v") else value


def parse_version(raw: str) -> tuple[int, ...]:
    normalized = normalize_version(raw)
    parts = normalized.split(".")
    if not parts or any(not part.isdigit() for part in parts):
        raise ValueError(
            f"Unsupported version '{raw}'. Expected dotted numeric versions like 0.5.26."
        )
    return tuple(int(part) for part in parts)


def read_cargo_version(cargo_toml: Path) -> str:
    with cargo_toml.open("rb") as handle:
        data = tomllib.load(handle)
    try:
        return str(data["package"]["version"])
    except KeyError as error:
        raise KeyError(f"Missing [package].version in {cargo_toml}") from error


def latest_release_tag() -> str | None:
    completed = subprocess.run(
        ["git", "tag", "--list", "v*", "--sort=-v:refname"],
        check=True,
        capture_output=True,
        text=True,
    )
    for line in completed.stdout.splitlines():
        candidate = line.strip()
        if candidate:
            return candidate
    return None


def main() -> int:
    args = parse_args()
    cargo_toml = Path(args.cargo_toml)
    cargo_version = read_cargo_version(cargo_toml)
    cargo_tuple = parse_version(cargo_version)

    if args.tag:
        tag_version = normalize_version(args.tag)
        if cargo_version != tag_version:
            print(
                f"Cargo.toml version {cargo_version} does not match release tag {args.tag}.",
                file=sys.stderr,
            )
            return 1
        print(f"Cargo.toml version {cargo_version} matches release tag {args.tag}.")
        return 0

    latest_tag = latest_release_tag()
    if latest_tag is None:
        print(f"Cargo.toml version {cargo_version} validated (no release tags found).")
        return 0

    latest_tuple = parse_version(latest_tag)
    if cargo_tuple < latest_tuple:
        print(
            (
                f"Cargo.toml version {cargo_version} is behind the latest tag {latest_tag}. "
                "Bump [package].version before merging."
            ),
            file=sys.stderr,
        )
        return 1

    print(
        f"Cargo.toml version {cargo_version} is aligned with latest tag {latest_tag}."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
