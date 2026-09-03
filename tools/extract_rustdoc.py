#!/usr/bin/env python3
"""Extract public documentation text for the prose gate."""

import pathlib
import sys


def main() -> None:
    source = pathlib.Path(sys.argv[1])
    destination = pathlib.Path(sys.argv[2])
    lines = []
    for path in sorted(source.glob("*.rs")):
        for line in path.read_text().splitlines():
            text = line.lstrip()
            if text.startswith("///") or text.startswith("//!"):
                lines.append(text[3:].lstrip())
    destination.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()

