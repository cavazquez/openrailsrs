import copy
import unittest

from check_weather_journey import validate_weather_journey


def report():
    samples = [{"simulation_s": i * 10, "odometer_m": i * 100, "speed_mps": 10,
                "rain": .6, "snow": 0, "visibility_m": 3000, "applied_brake": .45, "wheel_rail_brake_n": 1000,
                "rail_factor": .76, "rail_target": .75, "completed_stops": 4 if i == 24 else 0}
               for i in range(25)]
    return {"service_complete": True, "station_results": [{}] * 4,
            "weather_journey": {"samples": samples, "sample_capacity": 720,
                                "weather_frames": {"clear": {}, "rain": {}, "fog": {}},
                                "maximum_grip_change_per_simulation_s": .02,
                                "minimum_rail_factor": .7, "wet_braking_simulation_s": 30}}


class WeatherJourneyTests(unittest.TestCase):
    def test_complete_variable_service_is_accepted(self):
        self.assertTrue(validate_weather_journey(report())["passed"])

    def test_missing_contact_missed_stops_and_nonfinite_motion_are_rejected(self):
        original = report()
        for mutate in (lambda r: r.update(service_complete=False),
                       lambda r: r.update(drive_controls={"missed_stops": ["Station"]}),
                       lambda r: r["weather_journey"].update(maximum_grip_change_per_simulation_s=.4),
                       lambda r: r["weather_journey"]["samples"][2].update(speed_mps=float("nan")),
                       lambda r: [s.update(rail_target=None) for s in r["weather_journey"]["samples"]],
                       lambda r: [s.update(speed_mps=0) for s in r["weather_journey"]["samples"]],
                       lambda r: [s.update(wheel_rail_brake_n=0) for s in r["weather_journey"]["samples"]],
                       lambda r: r["weather_journey"]["samples"][-1].update(completed_stops=3)):
            changed = copy.deepcopy(original)
            mutate(changed)
            with self.assertRaises(ValueError):
                validate_weather_journey(changed)


if __name__ == "__main__":
    unittest.main()
