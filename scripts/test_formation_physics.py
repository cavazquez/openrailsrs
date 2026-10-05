"""Original Include safety and final authored numeric values are preserved."""
from pathlib import Path
import tempfile
import unittest
import shutil
import tomllib
from verify_formation_capture import BASELINE, ROOT, verify_capture
from unittest.mock import patch
import prepare_formation_physics as physics

class FormationPhysicsTests(unittest.TestCase):
    def test_includes_cannot_escape_or_cycle(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            stock=root/'stock'; stock.mkdir()
            (root/'outside.inc').write_text('Mass ( 999t )')
            item=stock/'car.eng'
            item.write_text('Include ( ../outside.inc )')
            with self.assertRaisesRegex(ValueError, 'escapes'):
                physics.expand(item,stock,{})
            item.write_text('Include ( car.eng )')
            with self.assertRaisesRegex(ValueError, 'cycles'):
                physics.expand(item,stock,{})

    def test_last_scalar_override_and_partial_supply_blocks_survive(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);stock=root/'stock';(stock/'engine').mkdir(parents=True)
            (stock/'engine/common.inc').write_text('Mass ( 20t )\nORTSElectricTrainSupply ( DieselEngineMinRpm ( 450 ) )\n')
            (stock/'engine/car.eng').write_text('Include ( common.inc )\nMass ( 30t )\nORTSElectricTrainSupply ( Mode ( Switch ) )\n')
            with patch.object(physics,'VEHICLES', [('engine','car.eng')]):
                physics.prepare(stock,root/'output')
            result=(root/'output/trains/engine/car.eng').read_text()
            self.assertIn('Mass ( 30t )',result)
            self.assertNotIn('Mass ( 20t )',result)
            self.assertIn('DieselEngineMinRpm ( 450 )',result)
            self.assertIn('Mode ( Switch )',result)
            self.assertFalse(any(p.suffix.lower() in ('.ace','.s','.wav') for p in (root/'output').rglob('*')))

class FormationCaptureTests(unittest.TestCase):
    def test_fixed_original_capture_has_complete_coverage(self):
        pin=tomllib.loads((ROOT/'oracles/openrails-reference.toml').read_text())
        self.assertEqual(verify_capture(BASELINE,pin),{'samples':5001,'full_service':False,'vehicles':7})

    def test_changed_capture_is_rejected(self):
        pin=tomllib.loads((ROOT/'oracles/openrails-reference.toml').read_text())
        with tempfile.TemporaryDirectory() as directory:
            copy=Path(directory)/'capture'
            shutil.copytree(BASELINE,copy)
            with (copy/'capture/trace.csv').open('a') as stream: stream.write('tampered\n')
            with self.assertRaisesRegex(ValueError,'changed'):
                verify_capture(copy,pin)

if __name__ == '__main__': unittest.main()
