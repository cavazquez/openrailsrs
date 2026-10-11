"""Keep original LIRR DLL parameters and speed-post directions immutable."""
import hashlib
import json
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ActivityParametersReference(unittest.TestCase):
    def test_pinned_original_dll_capture(self):
        p = json.loads((ROOT / 'oracles/openrails-lirr-parameters-provenance.json').read_text())
        pin = tomllib.loads((ROOT / 'oracles/openrails-reference.toml').read_text())
        self.assertEqual(p['reference_commit'], pin['commit'])
        for file, key in [
            ('oracles/openrails-lirr-parameters.json', 'output_sha256'),
            ('tools/openrails-reference/ActivityParameters.cs', 'helper_sha256'),
            ('tools/openrails-reference/HeadlessCapture.cs', 'loader_sha256'),
        ]:
            self.assertEqual(hashlib.sha256((ROOT / file).read_bytes()).hexdigest(), p[key])
        binaries = {b['name']: b['sha256'] for b in pin['binaries']}
        for name, digest in p['sha256'].items():
            if name in binaries:
                self.assertEqual(digest, binaries[name])
        data = json.loads((ROOT / 'oracles/openrails-lirr-parameters.json').read_text())
        self.assertEqual(len(data['speed_posts']), 778)
        self.assertEqual({r['direction'] for r in data['speed_posts']}, {0, 1})
        self.assertEqual(data['cars'], 7)
        self.assertAlmostEqual(data['initial_allowed_speed_mps'], 15 * 0.44704)
        self.assertEqual(data['duration_estimate_s'], 1560)
        self.assertEqual(data['timetable'][-1]['arrive_s'], 1575)
