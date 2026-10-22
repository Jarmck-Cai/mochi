"""Team-frozen, stdlib-only behavior regressions. Never runs the real Brain."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

CANDIDATE_PATH = Path(__file__).with_name("gate.py")


class GateOutputIntegrityRegression(unittest.TestCase):
    def _reject_old_output(self, old_output):
        with tempfile.TemporaryDirectory(prefix="gate-seed-", dir=CANDIDATE_PATH.parent) as value:
            root = Path(value)
            spec = importlib.util.spec_from_file_location("team_gate_candidate", CANDIDATE_PATH)
            gate = importlib.util.module_from_spec(spec)
            with patch.object(subprocess, "run", side_effect=RuntimeError("unexpected import subprocess")):
                try:
                    spec.loader.exec_module(gate)
                except AssertionError as exc:
                    raise RuntimeError("candidate assertion during import") from exc
            gate.ROOT = root
            gate.GATE_DIR = root / "reports"
            gate.BRAIN = root / "never-executed-brain.exe"
            gate.DEFAULT_NEW = root / "new"
            old, new = root / "old", root / "new"
            old.mkdir(); new.mkdir()
            control, target, report = root / "control.tsv", root / "target.tsv", root / "result.md"
            control.write_text("controlkey\tCONTROL_GOLD\n", encoding="utf-8")
            target.write_text("targetkey\tTARGET_GOLD\n", encoding="utf-8")
            calls = []

            def fake_brain(argv, **kwargs):
                side = (old, new)[len(calls)] if len(calls) < 2 else None
                expected = [str(gate.BRAIN), "--artifacts", str(side), "--no-user-data",
                            "--topn", "1", "--decode", "controlkey", "targetkey"]
                if argv != expected or kwargs != dict(capture_output=True, text=True, encoding="utf-8"):
                    raise RuntimeError("unexpected decode subprocess")
                calls.append(str(side))
                output = old_output if side == old else "controlkey\t#1\tWRONG\ntargetkey\t#1\tTARGET_GOLD\n"
                return subprocess.CompletedProcess(argv, 0, output, "")

            output, error = io.StringIO(), io.StringIO()
            with patch.object(subprocess, "run", side_effect=fake_brain), contextlib.redirect_stdout(output), contextlib.redirect_stderr(error):
                with self.assertRaises(SystemExit) as caught:
                    try:
                        gate.run_gate(old, new, control, target, report)
                    except AssertionError as exc:
                        raise RuntimeError("candidate assertion before integrity decision") from exc
            self.assertNotIn(caught.exception.code, (None, 0), "technical output failure must be nonzero")
            self.assertIn("decode output", (str(caught.exception.code) + error.getvalue()).lower())
            self.assertFalse(report.exists(), "technical output failure must not write a report")
            self.assertFalse(report.with_suffix(".json").exists(), "technical output failure must not write a PASS report")

    def test_missing_old_control_is_decode_failure(self):
        self._reject_old_output("targetkey\t#1\tWRONG\n")

    def test_conflicting_old_top1_is_decode_failure(self):
        self._reject_old_output("controlkey\t#1\tCONTROL_GOLD\ntargetkey\t#1\tWRONG\ncontrolkey\t#1\tWRONG\n")


if __name__ == "__main__":
    unittest.main()
