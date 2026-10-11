#!/usr/bin/env python3
"""Convert checked-in or captured benchmark observations to a reproducible JSON report.

Input CSV columns: protocol,scenario,operation,compute_units,account_metas,
transaction_bytes,source. Blank measurements mean unavailable, not zero.
No fabricated Phoenix/OpenBook/Manifest measurements are permitted.
"""
import argparse
import csv
import json
from datetime import datetime, timezone
from pathlib import Path

FIELDS = ("protocol", "scenario", "operation", "compute_units", "account_metas",
          "transaction_bytes", "source")
MEASUREMENTS = ("compute_units", "account_metas", "transaction_bytes")


def read_rows(path):
    with path.open(newline="", encoding="utf-8") as source:
        reader = csv.DictReader(source)
        if tuple(reader.fieldnames or ()) != FIELDS:
            raise ValueError("unexpected CSV header")
        rows = []
        for number, row in enumerate(reader, start=2):
            if any(value is None for value in row.values()):
                raise ValueError(f"CSV row {number} has missing fields")
            for field in ("protocol", "scenario", "operation", "source"):
                if not row[field].strip():
                    raise ValueError(f"CSV row {number} missing {field}")
            measurement = {}
            for field in MEASUREMENTS:
                raw = row[field].strip()
                if raw and (not raw.isascii() or not raw.isdecimal()):
                    raise ValueError(f"CSV row {number} invalid {field}")
                measurement[field] = int(raw) if raw else None
            rows.append({
                "protocol": row["protocol"],
                "scenario": row["scenario"],
                "operation": row["operation"],
                "source": row["source"],
                "measurements": measurement,
                "status": "measured" if all(x is not None for x in measurement.values())
                          else "incomplete",
            })
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("observations", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commit", required=True, help="40-character git commit SHA")
    parser.add_argument("--toolchain", required=True, help="Pinned Rust/Solana toolchain label")
    args = parser.parse_args()
    if len(args.commit) != 40 or any(c not in "0123456789abcdef" for c in args.commit):
        parser.error("--commit must be a lowercase full SHA")
    report = {
        "schema_version": 1,
        "provenance": {
            "commit": args.commit,
            "toolchain": args.toolchain,
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "source_file": args.observations.name,
        },
        "observations": read_rows(args.observations),
        "comparability": "Not established by this collector; protocols require matched fixtures.",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
