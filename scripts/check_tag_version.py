#!/usr/bin/env python3
"""Verify that a git tag matches the version in Cargo.toml."""

import os
import pathlib
import sys
import tomllib


def main() -> int:
    tag = os.environ.get("GIT_TAG", "")
    if not tag.startswith("v"):
        print(f"Tag must start with 'v', got: {tag}", file=sys.stderr)
        return 1

    tag_version = tag[1:]

    data = tomllib.loads(pathlib.Path("Cargo.toml").read_text())
    version = data["package"]["version"]
    if tag_version != version:
        print(
            f"Tag version {tag_version} does not match Cargo.toml version {version}",
            file=sys.stderr,
        )
        return 1

    print(f"Tag {tag} matches Cargo.toml version {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
