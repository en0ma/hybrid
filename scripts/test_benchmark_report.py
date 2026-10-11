"""Host-side tests for benchmark report ingestion; no network or Solana runtime required."""
import csv
import tempfile
import unittest
from pathlib import Path
from scripts.benchmark_report import read_rows, FIELDS


class BenchmarkReportTests(unittest.TestCase):
    def write_rows(self, rows):
        self.tmp = tempfile.TemporaryDirectory()
        path = Path(self.tmp.name) / "observations.csv"
        with path.open("w", newline="", encoding="utf-8") as stream:
            writer = csv.writer(stream)
            writer.writerow(FIELDS)
            writer.writerows(rows)
        self.addCleanup(self.tmp.cleanup)
        return path

    def test_explicitly_marks_missing_comparator_measurements(self):
        path = self.write_rows([
            ["Hybrid", "one-page", "buy", "1200", "12", "460", "runtime-log"],
            ["Phoenix", "one-page", "buy", "", "", "", "not-run"],
        ])
        rows = read_rows(path)
        self.assertEqual(rows[0]["status"], "measured")
        self.assertEqual(rows[0]["measurements"]["compute_units"], 1200)
        self.assertEqual(rows[1]["status"], "incomplete")
        self.assertIsNone(rows[1]["measurements"]["compute_units"])

    def test_rejects_invalid_or_negative_numbers(self):
        path = self.write_rows([
            ["Hybrid", "one-page", "sell", "-1", "12", "400", "runtime-log"]
        ])
        with self.assertRaises(ValueError):
            read_rows(path)

    def test_rejects_surplus_csv_fields(self):
        path = self.write_rows([
            ["Hybrid", "one-page", "buy", "100", "12", "400", "log", "extra"]
        ])
        with self.assertRaises(ValueError):
            read_rows(path)

    def test_rejects_missing_source(self):
        path = self.write_rows([
            ["Hybrid", "one-page", "sell", "1200", "12", "400", ""]
        ])
        with self.assertRaises(ValueError):
            read_rows(path)


if __name__ == "__main__":
    unittest.main()
