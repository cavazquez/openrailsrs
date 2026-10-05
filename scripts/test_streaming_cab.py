"""Regression for the cab loss that asset/CPU-visibility counters missed."""
import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageDraw
from check_viewer_streaming import compare_pullman_cab_foreground


class StreamingCabTests(unittest.TestCase):
    def compare(self, reference, candidate):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            reference.save(directory / "reference.png")
            candidate.save(directory / "candidate.png")
            return compare_pullman_cab_foreground(
                directory / "reference.png", directory / "candidate.png"
            )

    def test_daylight_change_keeps_opaque_cab_panels(self):
        result = self.compare(Image.new("RGB", (1280, 720), (55, 60, 65)),
                              Image.new("RGB", (1280, 720), (83, 88, 93)))
        self.assertTrue(result["passed"])

    def test_sky_replacing_cab_fails(self):
        result = self.compare(Image.new("RGB", (1280, 720), (55, 60, 65)),
                              Image.new("RGB", (1280, 720), (150, 185, 210)))
        self.assertFalse(result["passed"])

    def test_ground_replacing_cab_fails(self):
        result = self.compare(Image.new("RGB", (1280, 720), (55, 60, 65)),
                              Image.new("RGB", (1280, 720), (105, 90, 40)))
        self.assertFalse(result["passed"])

    def test_two_lit_panels_can_differ(self):
        reference = Image.new("RGB", (1280, 720), (55, 60, 65))
        candidate = reference.copy()
        draw = ImageDraw.Draw(candidate)
        draw.rectangle((115, 260, 230, 330), fill=(105, 110, 115))
        draw.rectangle((50, 305, 200, 368), fill=(105, 110, 115))
        result = self.compare(reference, candidate)
        self.assertTrue(result["passed"])
        self.assertEqual(result["matching_regions"], 4)

    def test_majority_of_panels_missing_fails(self):
        reference = Image.new("RGB", (1280, 720), (55, 60, 65))
        candidate = reference.copy()
        draw = ImageDraw.Draw(candidate)
        for box in ((115, 260, 230, 330), (50, 305, 200, 368), (943, 331, 1015, 392)):
            draw.rectangle(box, fill=(150, 185, 210))
        result = self.compare(reference, candidate)
        self.assertFalse(result["passed"])
        self.assertEqual(result["matching_regions"], 3)

    def test_different_resolution_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "1280×720"):
            self.compare(Image.new("RGB", (1280, 720)), Image.new("RGB", (640, 360)))


if __name__ == "__main__":
    unittest.main()
