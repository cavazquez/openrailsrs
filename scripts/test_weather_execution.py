"""Reject misleading hardware/software and particle-budget comparisons."""
import copy
import unittest
from benchmark_weather_execution import comparable, validate_mode


class WeatherTests(unittest.TestCase):
    def test_software_cannot_be_reported_as_hardware_or_gpu_particle_work(self):
        report = {"renderer": {"hardware": True}, "weather_particles": {
            "gpu_particles": 2048, "cpu_particles": 0, "gpu_mesh_updates": 0, "gpu_backend":"bevy_hanabi 0.19.0", "gpu_seed_initializations":1, "gpu_simulation_delta_s":0.0},
            "performance": {"gameplay_frames": 120}}
        validate_mode("gpu", report, 2048)
        with self.assertRaisesRegex(ValueError, "wrong graphics adapter"):
            validate_mode("software", report, 2048)
        report["renderer"]["hardware"] = False
        with self.assertRaisesRegex(ValueError, "particle budget"):
            validate_mode("software", report, 2048)
        report["weather_particles"].update(gpu_particles=0, cpu_particles=2048, cpu_mesh_updates=120)
        validate_mode("software", report, 2048)

    def test_different_scene_or_repeated_gpu_uploads_are_rejected(self):
        a = {"camera": {"position_world": [1, 2, 3]}, "clock_time_s": 35700}
        b = copy.deepcopy(a)
        comparable(a, b)
        b["clock_time_s"] += 1
        with self.assertRaisesRegex(ValueError, "clock_time_s"):
            comparable(a, b)
        report = {"renderer": {"hardware": True}, "weather_particles": {
            "gpu_particles": 2048, "cpu_particles": 0, "gpu_mesh_updates": 120, "gpu_backend":"bevy_hanabi 0.19.0", "gpu_seed_initializations":1, "gpu_simulation_delta_s":0.0}}
        with self.assertRaisesRegex(ValueError, "uploaded repeatedly"):
            validate_mode("gpu", report, 2048)


if __name__ == "__main__":
    unittest.main()
