import unittest
import tempfile
from pathlib import Path
from types import SimpleNamespace
from benchmark_viewer import cases, validate
from check_viewer_streaming import drm_client_vram, process_gpu_vram


class ViewerBenchmarkTests(unittest.TestCase):
    def test_gpu_accounting_deduplicates_clients_and_keeps_devices_separate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            primary = 'drm-client-id: 1\ndrm-pdev: 0000:03:00.0\ndrm-resident-vram: 2048 KiB\n'
            (root / '4').write_text(primary)
            (root / '5').write_text(primary)
            (root / '6').write_text('drm-client-id: 2\ndrm-pdev: 0000:0e:00.0\ndrm-memory-vram: 1024 bytes\n')
            (root / '7').write_text('pos: 0\nflags: 0\n')
            usage = process_gpu_vram(root)
            self.assertEqual(usage['0000:03:00.0'], 2)
            self.assertEqual(usage['0000:0e:00.0'], 1 / 1024)
            self.assertEqual(max(usage, key=usage.get), '0000:03:00.0')

    def test_missing_or_invalid_drm_values_do_not_mean_zero(self):
        for value in ['nan KiB', '-5 KiB', '1 unknown', '']:
            self.assertIsNone(drm_client_vram(f'drm-client-id: 2\ndrm-pdev: 0000:03:00.0\ndrm-memory-vram: {value}\n'))
        self.assertIsNone(drm_client_vram('drm-client-id: 1\ndrm-pdev: ../../etc\ndrm-memory-vram: 1 KiB\n'))
    def test_pacing_matrix_labels_vsync_and_weather_quality_budgets(self):
        pacing = dict(cases("pacing"))
        self.assertEqual(pacing["off"]["present_mode"], "fifo")
        self.assertEqual(pacing["unlimited"]["framepace"], "unlimited")
        self.assertEqual(len(cases("rain")), 9)
        self.assertEqual(len(cases("snow")), 9)
        self.assertEqual(dict(cases("vfx"))["off"]["train_effects_enabled"], 0)

    def test_missing_samples_and_software_or_failed_shader_are_rejected(self):
        report = {"dev_diagnostics": {"samples": 180, "frame_ms": {"p50": 16, "p95": 17, "p99": 20}},
                  "shader_pipelines": {"pending": 0, "failed": 0}, "renderer": {"hardware": True}}
        validate(report, 120)
        with self.assertRaisesRegex(ValueError, "Too few"):
            validate(report, 900)
        report["renderer"]["hardware"] = False
        with self.assertRaisesRegex(ValueError, "software"):
            validate(report, 120)

    def test_inactive_renderer_and_changed_regenerated_grass_cannot_pass(self):
        report = {"dev_diagnostics": {"samples": 180, "frame_ms": {"p50": 16, "p95": 17, "p99": 20}},
                  "shader_pipelines": {"pending": 0, "failed": 0}, "renderer": {"hardware": True},
                  "weather_state": {"atmosphere": {"rain": 1}},
                  "weather_particles": {"cpu_particles": 0, "gpu_particles": 0}}
        with self.assertRaisesRegex(ValueError, "no particles"):
            validate(report, 120)
        report["weather_particles"]["gpu_particles"] = 1024
        with self.assertRaisesRegex(ValueError, "combined"):
            validate(report, 120, SimpleNamespace(particle_budget=512))
        report["enhanced_scenery"] = {"instances": 100, "maximum_instances": 8192, "hash_mismatches": 1}
        with self.assertRaisesRegex(ValueError, "returning"):
            validate(report, 120)

    def test_incomplete_camera_journey_and_effects_off_allocations_are_rejected(self):
        report = {"dev_diagnostics": {"samples": 180, "frame_ms": {"p50": 16, "p95": 17, "p99": 20}},
                  "shader_pipelines": {"pending": 0, "failed": 0}, "renderer": {"hardware": True}}
        with self.assertRaisesRegex(ValueError, "round trip"):
            validate(report, 120, SimpleNamespace(camera_journey="aba"))
        report["camera_journey"] = {"complete": True, "distance_m": 8000}
        report["train_effects"] = {"enabled": False, "gpu_capacity": 512}
        with self.assertRaisesRegex(ValueError, "allocate"):
            validate(report, 120, SimpleNamespace(train_effects_enabled=0))


if __name__ == "__main__":
    unittest.main()
