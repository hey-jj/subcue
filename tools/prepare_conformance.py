#!/usr/bin/env python3
"""Create the packaged conformance data from the frozen build input."""

import json
import pathlib
import sys


def main() -> None:
    source = pathlib.Path(sys.argv[1])
    destination = pathlib.Path(sys.argv[2])
    document = json.loads(source.read_text())
    vectors = []
    for record in document["vectors"]:
        kept = {"id", "file", "format", "kind", "len", "sha256", "expect", "hex"}
        vectors.append({key: value for key, value in record.items() if key in kept})
    packaged = {
        "schema": "subtitle-conformance-vectors/1",
        "counts": document["counts"],
        "vectors": vectors,
    }
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(packaged, ensure_ascii=False, indent=1) + "\n")


if __name__ == "__main__":
    main()

