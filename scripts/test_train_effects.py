import copy
import unittest

from check_train_effects import validate


def gpu_report():
    return {
        "renderer": {"hardware": True},
        "shader_pipelines": {"pending": 0, "failed": 0},
        "train_effects": {
            "execution": "gpu", "gpu_capacity": 512, "cpu_capacity": 0,
            "gpu_emitters": 2, "gpu_spawn_requests": 120,
            "cpu_live_particles": 0, "cpu_mesh_updates": 0,
            "gpu_simulation_delta_s": 0.0,
        },
    }


class TrainEffectEvidence(unittest.TestCase):
    def test_gpu_requires_emission_and_no_cpu_vertex_rebuild(self):
        report = gpu_report()
        validate(report, "gpu")
        for key, value in [("gpu_spawn_requests", 0), ("cpu_mesh_updates", 20),
                           ("cpu_live_particles", 1), ("gpu_capacity", 0)]:
            broken = copy.deepcopy(report)
            broken["train_effects"][key] = value
            with self.assertRaises(ValueError):
                validate(broken, "gpu")

    def test_mixed_runs_both_paths_with_one_capacity(self):
        report = gpu_report()
        report["train_effects"].update(execution="hybrid", gpu_capacity=384,
            cpu_capacity=128, cpu_live_particles=20, cpu_mesh_updates=30)
        validate(report, "hybrid")
        report["train_effects"]["gpu_capacity"] = 512
        with self.assertRaises(ValueError):
            validate(report, "hybrid")

    def test_software_must_fall_back_even_when_gpu_is_requested(self):
        report = gpu_report()
        with self.assertRaises(ValueError):
            validate(report, "software")
        report["renderer"]["hardware"] = False
        report["train_effects"].update(execution="cpu", gpu_capacity=0,
            gpu_emitters=0, cpu_capacity=512, cpu_live_particles=80, cpu_mesh_updates=40)
        validate(report, "software")

    def test_unfinished_shaders_and_unpaused_clock_are_rejected(self):
        for field, value in [("shader_pipelines", {"pending": 1, "failed": 0}),
                             ("shader_pipelines", {"pending": 0, "failed": 1})]:
            report = gpu_report()
            report[field] = value
            with self.assertRaises(ValueError):
                validate(report, "gpu")
        report = gpu_report()
        report["train_effects"]["gpu_simulation_delta_s"] = 0.01
        with self.assertRaises(ValueError):
            validate(report, "gpu")


if __name__ == "__main__":
    unittest.main()
