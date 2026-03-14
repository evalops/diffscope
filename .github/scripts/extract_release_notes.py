#!/usr/bin/env python3
"""Extract the release notes section for a given version from RELEASE_NOTES.md.

Usage:
  python3 .github/scripts/extract_release_notes.py 0.5.27

Prints the section for "# Release Notes - v0.5.27" up to the next "---" or
"# Release Notes - v". If no section is found, exits 1 (caller can use a fallback).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: extract_release_notes.py <version>", file=sys.stderr)
        return 1
    version = sys.argv[1].strip()
    if version.startswith("v"):
        version = version[1:]
    path = Path("RELEASE_NOTES.md")
    if not path.exists():
        print("RELEASE_NOTES.md not found", file=sys.stderr)
        return 1
    text = path.read_text()
    # Match "# Release Notes - v0.5.27" or "# Release Notes - v0.5.27 "
    pattern = rf"^# Release Notes - v{re.escape(version)}\s*$"
    match = re.search(pattern, text, re.MULTILINE)
    if not match:
        return 1
    start = match.end()
    # End at next "---" (horizontal rule) or next "# Release Notes - v"
    end_match = re.search(r"\n---\s*\n|^# Release Notes - v", text[start:], re.MULTILINE)
    end = start + end_match.start() if end_match else len(text)
    section = text[start:end].strip()
    if not section:
        return 1
    print(section)
    return 0


if __name__ == "__main__":
    sys.exit(main())
