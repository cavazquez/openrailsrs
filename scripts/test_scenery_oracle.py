import unittest

from run_scenery_oracle import differences


class SceneryOracleTests(unittest.TestCase):
    def test_lost_wall_is_rejected_even_when_windows_survive(self):
        expected = {"sub_objects": [[{"prim_state_idx": 1, "triangles": 38},
                                    {"prim_state_idx": 2, "triangles": 22}]]}
        actual = {"sub_objects": [[{"prim_state_idx": 2, "triangles": 22}]]}
        self.assertTrue(differences(expected, actual))

    def test_lod_distance_budget_is_one_millimetre(self):
        self.assertFalse(differences({"selection_m": 260.0}, {"selection_m": 260.0005}))
        self.assertTrue(differences({"selection_m": 260.0}, {"selection_m": 260.01}))

    def test_primitive_ids_are_not_interchangeable(self):
        self.assertTrue(differences({"prim_state_idx": 1}, {"prim_state_idx": 2}))

    def test_extra_native_diagnostics_are_allowed(self):
        self.assertFalse(differences({"triangles": 38}, {"triangles": 38, "file": "house.s"}))


if __name__ == "__main__":
    unittest.main()
