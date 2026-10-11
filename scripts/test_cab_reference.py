"""Reject altered reference data or altered capture helpers."""
import hashlib
import json
from pathlib import Path
import unittest
ROOT=Path(__file__).resolve().parents[1]
class CabReference(unittest.TestCase):
    def test_pinned_original_dll_capture(self):
        p=json.loads((ROOT/'oracles/openrails-cab-provenance.json').read_text())
        self.assertEqual(p['reference_commit'],'d16e670da333d26d2edfc97d5631a19dadf49ce5')
        self.assertEqual(hashlib.sha256((ROOT/'oracles/openrails-cab.json').read_bytes()).hexdigest(),p['output_sha256'])
        self.assertEqual(hashlib.sha256((ROOT/'tools/openrails-reference/CabReference.cs').read_bytes()).hexdigest(),p['helper_sha256'])
        data=json.loads((ROOT/'oracles/openrails-cab.json').read_text())
        self.assertEqual(len(data['meters']),48)
        self.assertTrue(all(r['result'] > 0 for r in data['meters'] if r['motor_force_n'] > 0))
        pin=__import__('tomllib').loads((ROOT/'oracles/openrails-reference.toml').read_text())
        binaries={b['name']:b['sha256'] for b in pin['binaries']}
        for name,digest in p['sha256'].items():
            if name in binaries:self.assertEqual(digest,binaries[name])
