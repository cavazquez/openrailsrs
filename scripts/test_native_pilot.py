"""Native route pilots must preserve PAT order and physical platform endpoints."""
import tempfile
from pathlib import Path
import unittest
from prepare_native_pilot import main_path_points, station_markers, resolve
from prepare_chiltern_service import msts_text


class NativePilotTests(unittest.TestCase):
    def test_pat_walks_main_links_instead_of_file_pdp_order(self):
        text = """TrackPDP ( 0 0 40 0 0 1 0 )
        TrackPDP ( 0 0 0 0 0 1 0 )
        TrackPDP ( 0 0 100 0 0 1 0 )
        TrPathNode ( 00000000 2 4294967295 1 )
        TrPathNode ( 00000000 4294967295 4294967295 2 )
        TrPathNode ( 00000000 1 4294967295 0 )"""
        self.assertEqual(main_path_points(text), [(0, 0), (40, 0), (100, 0)])
        with self.assertRaisesRegex(ValueError, "cyclic"):
            main_path_points(text.replace("00000000 1 4294967295 0", "00000000 0 4294967295 0"))
        with self.assertRaisesRegex(ValueError, "reversal"):
            main_path_points(text.replace("00000000 2", "00000001 2"))

    def test_platform_pair_uses_departure_end_in_each_direction(self):
        text = 'TrackNode ( 7 TrItemRef ( 1 ) TrItemRef ( 2 ) )'
        for item, pair, distance in ((1, 2, 20), (2, 1, 80)):
            text += f'''PlatformItem ( TrItemId ( {item} ) Station ( "José León" )
            PlatformName ( "Andén (1)" ) TrItemSData ( {distance} 0 ) PlatformTrItemData ( 0 {pair} ) )'''
        edge = {"id": "e7", "length_m": 100}
        forward = station_markers(text, [edge])[0]
        reverse = station_markers(text, [{**edge, "id": "e7_r"}])[0]
        self.assertEqual((forward["item_id"], forward["chainage_m"]), (2, 80))
        self.assertEqual((reverse["item_id"], reverse["chainage_m"]), (1, 80))
        self.assertEqual(reverse["platform_length_m"], 60)
        with self.assertRaisesRegex(ValueError, "outside"):
            station_markers(text.replace("80 0", "180 0"), [edge])

    def test_native_accents_and_windows_case_are_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp)/"ROUTE.TDB"
            for encoding in ("utf-16", "utf-8-sig", "cp1252"):
                p.write_bytes("Estación José León".encode(encoding))
                self.assertEqual(msts_text(p), "Estación José León")
            self.assertEqual(resolve(Path(tmp)/"route.tdb"), p)


if __name__ == "__main__":
    unittest.main()
