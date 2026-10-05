"""Fast checks on real accepted render pixels; hardware fault captures are separate."""
import tempfile
from pathlib import Path
import unittest
from PIL import Image, ImageDraw, ImageOps
from check_visual_goldens import FIXTURES, check_fixtures, compare, VIEWS

class PlayerGoldenTests(unittest.TestCase):
    def test_versioned_goldens_have_all_seven_views_masks_and_fixed_resolution(self):
        spec=check_fixtures()
        self.assertEqual(set(spec['views']),set(VIEWS))
        self.assertEqual(spec['resolution'],[1280,720])
        self.assertEqual(len(spec['semantic_sources']),2)
        for name,view in spec['views'].items():
            with Image.open(FIXTURES/(name+'.png')) as image:self.assertEqual(image.size,(1280,720))
            self.assertTrue(compare(FIXTURES/(name+'.png'),FIXTURES/(name+'.png'),view['regions'])['pass'])

    def test_real_cab_pixels_detect_mirror_forward_change_and_occlusion(self):
        spec=check_fixtures();reference=FIXTURES/'cab-front.png';regions=spec['views']['cab-front']['regions']
        with tempfile.TemporaryDirectory() as d, Image.open(reference) as image:
            mirrored=image.copy();box=(345,340,985,538);mirrored.paste(ImageOps.mirror(image.crop(box)),box)
            mirror=Path(d)/'mirror.png';mirrored.save(mirror)
            self.assertFalse(compare(mirror,reference,regions)['pass'])
            self.assertFalse(compare(FIXTURES/'cab-left.png',reference,regions)['pass'])
            blocked=image.copy();ImageDraw.Draw(blocked).rectangle((300,5,975,665),fill=(0,0,0))
            occluder=Path(d)/'occluder.png';blocked.save(occluder)
            self.assertFalse(compare(occluder,reference,regions)['pass'])

    def test_train_pixels_are_required_independently_of_landscape(self):
        spec=check_fixtures();reference=FIXTURES/'orbit.png';regions=spec['views']['orbit']['regions']
        with tempfile.TemporaryDirectory() as d,Image.open(reference) as image:
            removed=image.copy()
            for region in regions.values():
                if 'box' in region:ImageDraw.Draw(removed).rectangle(region['box'],fill=(70,90,45))
                else:ImageDraw.Draw(removed).polygon([tuple(p) for p in region['polygon']],fill=(70,90,45))
            path=Path(d)/'absent.png';removed.save(path)
            self.assertFalse(compare(path,reference,regions)['pass'])

if __name__=='__main__':unittest.main()
