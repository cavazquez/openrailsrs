import unittest

from capture_player_menu import check_layout


class PlayerMenuCaptureTests(unittest.TestCase):
    def report(self):
        return {
            "ui_layout": [
                {"name": "player-menu", "min": [10, 10], "max": [790, 590]},
                {"name": "launch-footer", "min": [20, 500], "max": [780, 580]},
                {"name": "ui-Start", "min": [30, 530], "max": [110, 560]},
            ],
            "ui_text": [{"text": "Intensidad", "size": [100, 20], "visible": True}],
            "shader_pipelines": {"pending": 0, "failed": 0},
        }

    def test_hidden_fps_overlay_does_not_fail_the_menu_layout(self):
        report = self.report()
        report["ui_text"].append({"text": "FPS: ", "size": [0, 0], "visible": False})
        check_layout(report, "weather", 800, 600)

    def test_visible_collapsed_controls_still_fail(self):
        for visibility in ({"visible": True}, {}):
            report = self.report()
            report["ui_text"].append({"text": "Intensidad", "size": [0, 0], **visibility})
            with self.assertRaisesRegex(AssertionError, "Collapsed text: Intensidad"):
                check_layout(report, "weather", 800, 600)


if __name__ == "__main__":
    unittest.main()
