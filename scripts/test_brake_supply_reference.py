"""Protect the provenance and safe numeric-only export of native brake fixtures."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import prepare_brake_supply_fixtures as export

ROOT=Path(__file__).resolve().parents[1]


class BrakeSupplyReferenceTests(unittest.TestCase):
    def test_numeric_export_preserves_order_and_keeps_original_assets_out(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);content=root/'content';stock=content/'Demo/TRAINS/TRAINSET'
            (stock/'Motor').mkdir(parents=True);(stock/'Coach').mkdir()
            cons=content/'Demo/TRAINS/CONSISTS';cons.mkdir()
            (stock/'Motor/m.eng').write_text('Engine ( m Type ( Diesel ) Mass ( 40t ) MaxForce ( 100kN ) Sound ( secret.sms ) CabView ( original.cvf ) ORTSPowerSupply ( Default.cs ) )')
            (stock/'Coach/c.wag').write_text('Wagon ( c Mass ( 20t ) BrakeSystemType ( EP ) )')
            (cons/'test.con').write_text('Train ( TrainCfg ( test Engine ( EngineData ( m Motor ) ) Wagon ( WagonData ( c Coach ) ) Engine ( EngineData ( m Motor ) ) ) )')
            before=(stock/'Motor/m.eng').read_bytes()
            with patch.object(export,'FORMATIONS',{'Demo':['test.con']}):
                export.prepare(content,root/'result')
            profiles=json.loads((root/'result/profiles.json').read_text())
            con=(root/'result/test-con.con').read_text()
            self.assertLess(con.index('Engine'),con.index('Wagon'))
            self.assertEqual(con.count('Engine'),2)
            self.assertEqual(len(profiles),2)
            fixture=(root/'result'/profiles[0]['file']).read_text()
            self.assertNotIn('Sound (',fixture);self.assertNotIn('CabView (',fixture)
            self.assertNotIn('secret.sms',fixture)
            self.assertEqual((stock/'Motor/m.eng').read_bytes(),before)
            with self.assertRaisesRegex(ValueError,'outside'):
                export.prepare(content,content/'generated')
            self.assertFalse((content/'generated').exists())

    def test_reference_scope_includes_vacuum_ep_and_exact_power_states(self):
        reference=json.loads((ROOT/'oracles/openrails-brake-power.json').read_text())
        manifest=json.loads((ROOT/'oracles/openrails-brake-power-provenance.json').read_text())
        self.assertEqual(manifest['reference_commit'],'d16e670da333d26d2edfc97d5631a19dadf49ce5')
        self.assertEqual(len(reference['vacuum']),5)
        self.assertEqual(len(reference['legacy_ep']),6)
        self.assertEqual({r['kind'] for r in reference['power']},{'diesel','electric','steam'})
        self.assertEqual(len(reference['power']),303)


if __name__=='__main__':
    unittest.main()
