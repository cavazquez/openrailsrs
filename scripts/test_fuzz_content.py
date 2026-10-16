"""Regression seeds for installer trust boundaries; no downloads or listeners."""
import unittest
import zipfile
import download_official_content as content
import fuzz_content

class FuzzBoundaries(unittest.TestCase):
    def test_own_corpus_exercises_production_url_and_archive_validation(self):
        for data in fuzz_content.seeds():
            fuzz_content.exercise(data)

    def test_nul_not_hidden_by_zipinfo_filename_normalization(self):
        class Archive:
            def infolist(self):
                return [zipfile.ZipInfo('safe\x00../escape')]
        with self.assertRaises(ValueError):
            content.safe_members(Archive(), 65536)

    def test_dot_and_control_characters_do_not_become_installed_content(self):
        class Archive:
            def infolist(self):
                return [zipfile.ZipInfo('.')]
        with self.assertRaises(ValueError):
            content.safe_members(Archive(), 65536)
        for text in ['https://github.com/a/b\x00','\x01https://github.com/a/b']:
            with self.assertRaises(ValueError):
                content.source_from_url(text)

if __name__ == '__main__':
    unittest.main()
