"""Verify provenance of the original STF/PAT reader capture."""
import hashlib
import json
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]

class ParserReference(unittest.TestCase):
    def test_pinned_original_dll_capture(self):
        provenance = json.loads((ROOT / 'oracles/openrails-parser-provenance.json').read_text())
        pin = tomllib.loads((ROOT / 'oracles/openrails-reference.toml').read_text())
        self.assertEqual(provenance['reference_commit'], pin['commit'])
        for name, field in [('oracles/openrails-parser.json', 'output_sha256'),
                            ('tools/openrails-reference/ParserReference.cs', 'helper_sha256')]:
            self.assertEqual(hashlib.sha256((ROOT / name).read_bytes()).hexdigest(), provenance[field])
        data = json.loads((ROOT / 'oracles/openrails-parser.json').read_text())
        self.assertEqual(data['flags'], [0, 655361, 1966082])
        self.assertIn('42t', data['include'])
        self.assertEqual(len(data['point_flags']), 6)
        binaries = {b['name']: b['sha256'] for b in pin['binaries']}
        for name, digest in provenance['sha256'].items():
            if name in binaries:
                self.assertEqual(digest, binaries[name])
