import copy
from pathlib import Path
import tempfile
import unittest

from check_viewer_soak import validate_capture, validate_trace


def row(wall=7200.01, cycles=120):
    return dict(schema=3, wall_s=wall, cycles=cycles, save_restore_cycles=cycles,
                memory_restore_failures=0, memory_restore_last_error=None,
                disk_save_attempts=cycles, disk_save_restore_cycles=cycles,
                disk_save_failures=0, disk_save_last_error=None,
                disk_save_last_bytes=65536 if cycles else None,
                disk_save_last_sha256='a'*64 if cycles else None,
                route_changes=39, pending_uploads=0)


class ActualSavedGameEndurance(unittest.TestCase):
    def test_full120_real_cycles_pass_but119_and_partial_time_fail(self):
        self.assertEqual(validate_trace([row()],7200)['disk_restores'],120)
        for altered in (row(cycles=119),row(wall=7199.99,cycles=119)):
            with self.assertRaises(RuntimeError):validate_trace([altered],7200)

    def test_legacy_or_missing_disk_fields_never_produce_final_acceptance(self):
        for key in ('schema','memory_restore_failures','memory_restore_last_error',
                    'disk_save_attempts','disk_save_restore_cycles',
                    'disk_save_failures','disk_save_last_bytes','disk_save_last_sha256'):
            altered=row();altered.pop(key)
            with self.assertRaises(RuntimeError):validate_trace([altered],7200)
        altered=row();altered['schema']=2
        with self.assertRaises(RuntimeError):validate_trace([altered],7200)

    def test_a_failed_or_lost_executed_cycle_is_rejected(self):
        for key,value in [('memory_restore_failures',1),('memory_restore_last_error','snapshot mismatch'),
                          ('disk_save_failures',1),('disk_save_attempts',119),
                          ('disk_save_restore_cycles',119),('save_restore_cycles',119),
                          ('disk_save_last_error','read failed'),('pending_uploads',1)]:
            altered=row();altered[key]=value
            with self.assertRaises(RuntimeError):validate_trace([altered],7200)
        with self.assertRaises(RuntimeError):validate_trace([row(cycles=121)],7200)

    def test_unavailable_nonfinite_and_boolean_data_are_not_accepted_as_zero(self):
        for key in ('wall_s','cycles','memory_restore_failures','disk_save_attempts','disk_save_restore_cycles',
                    'disk_save_failures','disk_save_last_bytes','pending_uploads'):
            for value in (None,float('nan'),float('inf'),False):
                altered=row();altered[key]=value
                with self.assertRaises(RuntimeError):validate_trace([altered],7200)

    def test_smoke_below_a_minute_explicitly_did_not_exercise_disk(self):
        result=validate_trace([row(wall=30.01,cycles=0)],30,smoke=True)
        self.assertFalse(result['endurance'])
        self.assertEqual(result['disk_restores'],0)
        self.assertIn('Not exercised',result['disk_scope'])
        with self.assertRaises(RuntimeError):validate_trace([row(wall=60.01,cycles=0)],60,smoke=True)

    def test_transient_uploads_are_allowed_but_final_readiness_is_required(self):
        first=row(wall=30.01,cycles=0);first['pending_uploads']=4
        self.assertTrue(validate_trace([first,row()],7200)['endurance'])
        last=row();last['pending_uploads']=4
        with self.assertRaises(RuntimeError):validate_trace([first,last],7200)

    def test_bad_route_counters_and_nonmonotonic_samples_fail(self):
        altered=row();altered['route_changes']=38
        with self.assertRaises(RuntimeError):validate_trace([altered],7200,require_routes=True)
        for rows in ([row(),row()],[row(),row(wall=7201,cycles=119)]):
            with self.assertRaises(RuntimeError):validate_trace(rows,7200)

    def test_capture_requires_actual_clean_exit_fresh_png_observer_and_ready_queues(self):
        with tempfile.TemporaryDirectory() as directory:
            image=Path(directory)/'new.png';image.write_bytes(b'\x89PNG\r\n\x1a\n' + b'new-pixels')
            report=dict(elapsed_s=7201,shader_pipelines=dict(pending=0,failed=0),
                        pending_gpu_uploads=0,pending_terrain_tiles=0,
                        unactivated_near_shapes=0,native_signal_errors=[])
            args=dict(returncode=0,image=image,log=f'screenshot saved → {image}',
                      started_ns=image.stat().st_mtime_ns-1,seconds=7200)
            validate_capture(report,**args)
            for key,value in [('returncode',1),('started_ns',image.stat().st_mtime_ns+1),('log','')]:
                altered=dict(args);altered[key]=value
                with self.assertRaises(RuntimeError):validate_capture(report,**altered)
            for key,value in [('elapsed_s',None),('pending_gpu_uploads',None),
                              ('pending_terrain_tiles',float('nan')),('unactivated_near_shapes',1),
                              ('shader_pipelines',dict(pending=None,failed=0)),('native_signal_errors',None)]:
                altered=copy.deepcopy(report);altered[key]=value
                with self.assertRaises(RuntimeError):validate_capture(altered,**args)


if __name__ == '__main__':unittest.main()
