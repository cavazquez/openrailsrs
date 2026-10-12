import copy
import unittest
from check_perf_contract import compare


def report():
    return {"contract": {"machine": "fixed-gpu-driver", "resolution": [1280, 720], "route_sha256": "a" * 64, "consist_sha256": "b" * 64, "camera": "cab-55", "seed": 42, "quality": "high"}, "version": "test", "binary_sha256": "c" * 64, "capture": {"performance": {"gameplay_frames": 600, "gameplay_frame_p50_ms": 16, "gameplay_frame_p95_ms": 22, "gameplay_frame_p99_ms": 30, "gameplay_hitches_over_100_ms": 1, "rss_mib": 1024, "peak_rss_mib": 1100, "startup_longest_frame_ms": 300}}}


class Contracts(unittest.TestCase):
    def test_same_machine_detects_percentile_and_memory_regressions(self):
        a, b = report(), report()
        b["capture"]["performance"]["gameplay_frame_p99_ms"] = 55
        b["capture"]["performance"]["peak_rss_mib"] = 2000
        result = compare(a, b)
        self.assertFalse(result["passed"])
        self.assertEqual(result["regressions"], ["gameplay_frame_p99_ms", "peak_rss_mib"])
        self.assertEqual(result["metrics"]["vram_mib"]["status"], "unavailable")

    def test_different_seed_or_quality_is_not_a_comparison(self):
        a = report()
        for field in ("seed", "quality", "machine", "camera", "resolution"):
            b = copy.deepcopy(a)
            b["contract"][field] = "different"
            with self.assertRaises(ValueError):
                compare(a, b)

    def test_non_finite_timings_cannot_hide_a_regression(self):
        for value in (float('nan'), float('inf'), -1):
            a, b = report(), report()
            b['capture']['performance']['gameplay_frame_p95_ms'] = value
            with self.assertRaises(ValueError):
                compare(a, b)

    def test_loading_time_is_kept_separate_from_steady_driving(self):
        a, b = report(), report()
        b["capture"]["performance"]["startup_longest_frame_ms"] = 9000
        self.assertTrue(compare(a, b)["passed"])
        b["capture"]["performance"]["gameplay_frames"] = 1
        with self.assertRaises(ValueError):
            compare(a, b)


if __name__ == "__main__":
    unittest.main()
