"""Negative controls for the joint visual and full-activity capture protocols."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

from capture_equivalent_views import load_plan, reference_inputs
from capture_lirr_full_service import compare, read_trace

ROOT=Path(__file__).resolve().parents[1]


class ComparisonProtocols(unittest.TestCase):
    def test_view_plan_rejects_unknown_station_changed_conditions_and_path_escape(self):
        original=json.loads((ROOT/'examples/visual_comparison/chiltern.json').read_text())
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'plan.json'
            path.write_text(json.dumps(original))
            with self.assertRaises(ValueError):load_plan(path,['unselected'])
            for mutate in [lambda p:p['conditions'].update(cab_fov_deg=90),lambda p:p['stations'][0].update(slug='../outside'),lambda p:p['stations'].append(p['stations'][0])]:
                plan=copy.deepcopy(original);mutate(plan);path.write_text(json.dumps(plan))
                with self.assertRaises(ValueError):load_plan(path)

    def test_frozen_reference_is_hashed_before_use_and_tampering_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            base=Path(directory)
            for view in ('cab','exterior'):
                (base/f'a-{view}.png').write_bytes(b'not the captured bytes')
                (base/f'a-{view}.json').write_text(json.dumps({'version':'1.6.1','png_sha256':'0'*64}))
            with self.assertRaises(ValueError):reference_inputs(base,[{'slug':'a'}])

    def test_full_trace_refuses_nonfinite_time_gaps_and_controls(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'trace.csv'
            for body in ['0,0,0\n0.1,0,0\n','0,0,0\n0.05,nan,0\n','0,0,0\n0.05,1.1,0\n']:
                path.write_text('time_s,throttle,brake\n'+body)
                with self.assertRaises(ValueError):read_trace(path)

    def test_comparison_cannot_pass_truncation_control_drift_or_missing_station(self):
        protocol=json.loads((ROOT/'oracles/lirr-full-service-protocol.json').read_text())
        protocol.update(minimum_duration_s=0.1,minimum_distance_m=1)
        rows=[dict(time_s=t,velocity_mps=10,odometer_m=t*10,throttle=0.5,brake=0,doors_open=0,brake_pipe_psi=90,cylinder_psi=0,motive_force_n=1000,brake_force_n=0) for t in (0.,0.05,0.1)]
        outcome={'reached_all_platforms':True,'arrivals':[{},{}]}
        initial={'cars':7}
        self.assertTrue(compare(rows,rows,protocol,outcome,initial)['passed'])
        with self.assertRaises(ValueError):compare(rows,rows[:-1],protocol,outcome,initial)
        bad=copy.deepcopy(rows);bad[1]['throttle']=0.4
        self.assertFalse(compare(rows,bad,protocol,outcome,initial)['passed'])
        bad=copy.deepcopy(rows);bad[1]['cylinder_psi']=100
        self.assertFalse(compare(rows,bad,protocol,outcome,initial)['passed'])
        self.assertFalse(compare(rows,rows,protocol,{'reached_all_platforms':False,'arrivals':[{}]},initial)['passed'])

    def test_existing_native_tolerances_stay_predeclared(self):
        protocol=json.loads((ROOT/'oracles/lirr-full-service-protocol.json').read_text())
        self.assertEqual(protocol['thresholds']['velocity_rms_mps'],0.75)
        self.assertEqual(protocol['thresholds']['maximum_odometer_error_m'],45)
        self.assertEqual(protocol['reference_version'],'1.6.1')


if __name__=='__main__':unittest.main()
