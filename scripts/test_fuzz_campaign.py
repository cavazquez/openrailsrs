"""Executor regressions with isolated fake harnesses, never campaign evidence."""
import gzip
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import run_fuzz_campaign as fuzz


class FuzzCampaignTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = dict(out=str(self.root / "report"), corpus=str(self.root / "corpus"),
                           findings=str(self.root / "findings"), targets=["stf", "simisa"],
                           seconds_per_target=2, slice_seconds=1, seed=123,
                           timeout=1, rss_limit_mb=128, resume=False)
        self.trace = self.root / "trace.jsonl"
        self.binaries = {}
        for target in self.config["targets"]:
            corpus = self.root / "corpus" / target
            corpus.mkdir(parents=True)
            (corpus / "own-seed").write_bytes(b"Train ( )")
            executable = self.root / target
            executable.write_text(f"#!{sys.executable}\n" + r'''
import json, pathlib, sys, time
target = pathlib.Path(sys.argv[0]).name
args = dict(s[1:].split('=', 1) for s in sys.argv[1:] if s.startswith('-'))
with pathlib.Path(__file__).with_name('trace.jsonl').open('a') as stream:
    stream.write(json.dumps(['start', target]) + '\n')
assert args['fork'] == '0' and args['jobs'] == '0' and args['workers'] == '1'
time.sleep(.02)
mode_file = pathlib.Path(__file__).with_name('mode')
mode = mode_file.read_text() if mode_file.exists() else 'pass'
if mode == 'crash' and target == 'stf':
    pathlib.Path(args['artifact_prefix'], 'crash-own-case').write_bytes(b'own failing case')
    print('ERROR: libFuzzer: deadly signal', file=sys.stderr)
    sys.exit(77)
print('#100 DONE cov: 42 ft: 75 corp: 2/20b exec/s: 100 rss: 50Mb', file=sys.stderr)
print('Done 100 runs in %s second(s)' % (0 if mode == 'early' else args['max_total_time']), file=sys.stderr)
print('stat::number_of_executed_units: 100', file=sys.stderr)
print('stat::peak_rss_mb: 50', file=sys.stderr)
with pathlib.Path(__file__).with_name('trace.jsonl').open('a') as stream:
    stream.write(json.dumps(['end', target]) + '\n')
''')
            executable.chmod(0o700)
            self.binaries[target] = executable
        self.sources = {"own-test-source": "own-sha"}

    def run_campaign(self):
        with patch.object(fuzz, "source_snapshot", return_value=self.sources):
            with fuzz.campaign_lock(Path(self.config["corpus"])):
                return fuzz.campaign(self.config, self.binaries, self.sources)

    def test_sequential_slices_and_persistent_corpus(self):
        result = self.run_campaign()
        self.assertEqual(result["status"], "complete")
        events = [json.loads(line) for line in self.trace.read_text().splitlines()]
        self.assertEqual(events, [[event, target] for target in ("stf", "stf", "simisa", "simisa")
                                  for event in ("start", "end")])
        for target in self.config["targets"]:
            current = result["targets"][target]
            self.assertEqual(current["completed_budget_seconds"], 2)
            self.assertEqual(current["corpus_before"], current["corpus_after"])
            self.assertEqual(current["attempts"][0]["stats"]["coverage_edges"], 42)

    def test_resume_skips_completed_targets_and_rejects_changed_binary(self):
        self.run_campaign()
        original = self.trace.read_bytes()
        self.config["resume"] = True
        self.assertEqual(self.run_campaign()["status"], "complete")
        self.assertEqual(original, self.trace.read_bytes())
        self.binaries["stf"].write_text(self.binaries["stf"].read_text() + "\n# changed\n")
        with self.assertRaisesRegex(ValueError, "changed"):
            self.run_campaign()

    def test_bounded_process_checkpoints_cleanly_and_resumes_full_budget(self):
        self.config["max_slices"] = 1
        first = self.run_campaign()
        self.assertEqual(first["status"], "checkpoint")
        self.assertEqual(first["targets"]["stf"]["completed_budget_seconds"], 1)
        self.assertEqual(first["targets"]["stf"]["attempts"][0]["status"], "pass")
        self.assertEqual(first["targets"]["simisa"]["status"], "pending")
        self.config.update(resume=True, max_slices=0)
        final = self.run_campaign()
        self.assertEqual(final["status"], "complete")
        self.assertTrue(all(t["completed_budget_seconds"] == 2 for t in final["targets"].values()))
        self.assertTrue(all(a["status"] == "pass" for t in final["targets"].values() for a in t["attempts"]))

    def test_resume_repeats_only_uncredited_interrupted_slice(self):
        self.run_campaign()
        path = self.root / "report/progress.json"
        state = json.loads(path.read_text())
        state["status"] = "interrupted"
        state["targets"]["simisa"]["status"] = "interrupted"
        state["targets"]["simisa"]["completed_budget_seconds"] = 1
        state["targets"]["simisa"]["attempts"][-1]["status"] = "running"
        fuzz.write_json(path, state)
        original_events = len(self.trace.read_text().splitlines())
        self.config["resume"] = True
        result = self.run_campaign()
        self.assertEqual(result["status"], "complete")
        self.assertEqual(len(result["targets"]["stf"]["attempts"]), 2)
        self.assertEqual(len(result["targets"]["simisa"]["attempts"]), 3)
        self.assertEqual(result["targets"]["simisa"]["attempts"][1]["status"], "interrupted")
        self.assertEqual(len(self.trace.read_text().splitlines()) - original_events, 2)

    def test_failure_retains_case_and_continues_next_target(self):
        (self.root / "mode").write_text("crash")
        first = self.run_campaign()
        self.assertEqual(first["status"], "complete_with_findings")
        self.assertEqual(first["targets"]["stf"]["completed_budget_seconds"], 0)
        self.assertEqual(first["targets"]["simisa"]["status"], "pass")
        self.config["out"] = str(self.root / "second-report")
        self.run_campaign()
        findings = list((self.root / "findings/stf").glob("*/finding.json"))
        self.assertEqual(len(findings), 1)
        metadata = json.loads(findings[0].read_text())
        self.assertEqual(len(metadata["occurrences"]), 2)
        self.assertFalse(metadata["confirmed_bug"])
        self.assertEqual((findings[0].parent / "input.bin").read_bytes(), b"own failing case")
        self.assertIn("-minimize_crash=1", metadata["minimize_command"])
        self.assertIn("-runs=1", metadata["replay_command"])

    def test_early_success_does_not_certify_requested_time(self):
        (self.root / "mode").write_text("early")
        result = self.run_campaign()
        self.assertEqual(result["status"], "incomplete")
        self.assertEqual(result["targets"]["stf"]["completed_budget_seconds"], 0)

    def test_lock_prevents_two_campaigns_sharing_corpus(self):
        with fuzz.campaign_lock(self.root / "corpus"):
            with self.assertRaisesRegex(ValueError, "Another campaign"):
                with fuzz.campaign_lock(self.root / "corpus"):
                    self.fail("second campaign acquired the lock")

    def test_artifact_and_corpus_hashes_reject_symlinks_and_oversized_inputs(self):
        directory = self.root / "bad-corpus"
        directory.mkdir()
        (directory / "symlink").symlink_to(self.trace)
        with self.assertRaisesRegex(ValueError, "regular files"):
            fuzz.inventory(directory)
        (directory / "symlink").unlink()
        (directory / "oversized").write_bytes(b"x" * (fuzz.MAX_INPUT + 1))
        with self.assertRaisesRegex(ValueError, "exceeds"):
            fuzz.inventory(directory)

    def test_watchdog_reaps_owned_child(self):
        child = self.root / "sleeper.py"
        pid_file = self.root / "child.pid"
        child.write_text("import os,time\nfrom pathlib import Path\n" +
                         f"Path({str(pid_file)!r}).write_text(str(os.getpid()))\ntime.sleep(30)\n")
        code, elapsed, watchdog = fuzz.execute([sys.executable, str(child)],
                                              self.root / "timeout.log", .2)
        self.assertTrue(watchdog)
        self.assertLess(elapsed, 6)
        self.assertNotEqual(code, 0)
        with self.assertRaises(ProcessLookupError):
            os.kill(int(pid_file.read_text()), 0)

    def test_compressed_logs_preserve_bytes_and_extract_metrics(self):
        raw = (b"Expected rejected synthetic input\n" * 40000 +
               b"#123 DONE cov: 42 ft: 75\nDone 123 runs in 3 second(s)\n" +
               b"stat::peak_rss_mb: 50\n")
        source = self.root / "raw-test-log"
        source.write_bytes(raw)
        log = self.root / "compressed.log.gz"
        summary = {}
        code, _, watchdog = fuzz.execute(
            [sys.executable, "-c", f"import sys;sys.stdout.buffer.write(open({str(source)!r},'rb').read())"],
            log, 5, stream_summary=summary)
        self.assertEqual(code, 0)
        self.assertFalse(watchdog)
        self.assertEqual(gzip.decompress(log.read_bytes()), raw)
        self.assertEqual(summary["uncompressed_log_sha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(summary["uncompressed_log_bytes"], len(raw))
        self.assertEqual(summary["stats"]["executions"], 123)
        self.assertEqual(summary["stats"]["reported_seconds"], 3)
        self.assertLess(log.stat().st_size, len(raw) // 10)

    def test_sigterm_reaps_child_and_runner(self):
        pid_file = self.root / "signal-child.pid"
        child_code = f"import os,time;from pathlib import Path;Path({str(pid_file)!r}).write_text(str(os.getpid()));time.sleep(30)"
        outer_code = ("import signal,sys;from pathlib import Path;"
                      f"sys.path.insert(0,{str(Path(fuzz.__file__).parent)!r});"
                      "import run_fuzz_campaign as f;signal.signal(signal.SIGTERM,f.interrupted);"
                      f"f.execute([sys.executable,'-c',{child_code!r}],Path({str(self.root / 'signal.log')!r}),60)")
        process = subprocess.Popen([sys.executable, "-c", outer_code],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(lambda: process.poll() is None and process.kill())
        deadline = time.monotonic() + 5
        while not pid_file.exists() and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertTrue(pid_file.exists())
        process.send_signal(signal.SIGTERM)
        process.wait(timeout=6)
        with self.assertRaises(ProcessLookupError):
            os.kill(int(pid_file.read_text()), 0)


if __name__ == "__main__":
    unittest.main()
