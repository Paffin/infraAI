#!/usr/bin/env python3
"""Checks SARIF files against the official 2.1.0 schema of OASIS (vendored as deploy/sarif-schema-2.1.0.json, MIT licence).

    python3 deploy/check-sarif.py results.sarif [more.sarif ...]

Exit code 1 and the list of what is wrong when a file does not conform; needs `pip install jsonschema`. It is how CI knows that what
`infra-architect check --format sarif` writes is what the code-scanning tools read.
"""
import json
import pathlib
import sys

try:
    import jsonschema
except ImportError:  # pragma: no cover
    sys.exit("jsonschema is needed: pip install jsonschema")

SCHEMA = json.loads((pathlib.Path(__file__).resolve().parent / "sarif-schema-2.1.0.json").read_text())


def main(paths):
    if not paths:
        sys.exit(__doc__)
    validator = jsonschema.Draft7Validator(SCHEMA)
    bad = 0
    for path in paths:
        doc = json.loads(pathlib.Path(path).read_text())
        errors = sorted(validator.iter_errors(doc), key=lambda e: [str(p) for p in e.path])
        # a document that conforms to the schema but says nothing is no use to anybody
        if not errors and not doc.get("runs"):
            errors = [type("E", (), {"path": [], "message": "no runs"})()]
        if errors:
            bad += 1
            print(f"{path}: {len(errors)} problem(s)")
            for e in errors[:10]:
                print("  ", "/".join(str(p) for p in e.path) or "(document)", "-", e.message[:200])
        else:
            n = sum(len(r.get("results", [])) for r in doc["runs"])
            print(f"{path}: valid SARIF 2.1.0, {n} result(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
