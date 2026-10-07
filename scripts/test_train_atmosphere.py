"""Do not accept a clear/software scene or unchanged lamps as volumetric evidence."""
import copy
from pathlib import Path
import tempfile
import unittest
from check_train_atmosphere import compare_lights, validate_fog


class AtmosphereEvidence(unittest.TestCase):
    def test_fog_requires_dense_hardware_volume_and_punctual_shader_fix(self):
        r = {"renderer":{"hardware":True},"shader_pipelines":{"pending":0,"failed":0},
             "fog":{"volumetric":True,"visibility_m":120,"density_factor":.065,"punctual_light_attenuation_corrected":True}}
        validate_fog(r)
        for section, key, value in (("renderer","hardware",False),("fog","volumetric",False),
                                    ("fog","visibility_m",500),("fog","density_factor",.001),
                                    ("fog","punctual_light_attenuation_corrected",False),("shader_pipelines","failed",1)):
            broken = copy.deepcopy(r); broken[section][key] = value
            with self.assertRaises(ValueError): validate_fog(broken)

    def test_pair_rejects_no_response_and_changes_only_in_the_hud(self):
        from PIL import Image, ImageDraw
        with tempfile.TemporaryDirectory() as directory:
            off, high = Path(directory)/'off.png', Path(directory)/'high.png'
            image = Image.new('RGB',(1280,720),(10,10,10)); image.save(off); image.save(high)
            with self.assertRaisesRegex(ValueError,'did not brighten'): compare_lights(off,high)
            ImageDraw.Draw(image).rectangle((0,0,260,250),fill=(255,255,255)); image.save(high)
            with self.assertRaisesRegex(ValueError,'did not brighten'): compare_lights(off,high)
            ImageDraw.Draw(image).rectangle((450,300,850,600),fill=(80,90,120)); image.save(high)
            self.assertGreater(compare_lights(off,high)['mean_rgb_increase'],1)
            ImageDraw.Draw(image).rectangle((450,300,850,600),fill=(255,255,255)); image.save(high)
            with self.assertRaisesRegex(ValueError,'clips the nearby view white'): compare_lights(off,high)


if __name__ == '__main__':
    unittest.main()
