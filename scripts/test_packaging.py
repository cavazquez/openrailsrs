"""Distribution must include resources while excluding downloads and private files."""
import json
from pathlib import Path
import tempfile
import subprocess
import unittest
from unittest.mock import patch
import package_linux
import package_snap


class PackagingTests(unittest.TestCase):
    def test_portable_bundle_is_independent_and_excludes_content_and_outcomes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in ('packaging/resources.json', 'examples/smoke/scenario.toml',
                             'examples/smoke/run.json', 'crates/openrailsrs-bevy-scenery/assets/shaders/test.wgsl',
                             'official-content/route/private.dat', '.aws/credentials'):
                path = root/relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('original')
            resources = ['examples/smoke/scenario.toml', 'examples/smoke/run.json',
                         'crates/openrailsrs-bevy-scenery/assets/shaders/test.wgsl']
            (root/'packaging/resources.json').write_text(json.dumps(resources))
            binaries = root/'compiled'
            binaries.mkdir()
            for name in package_linux.BINS:
                (binaries/name).write_text('executable')
                (binaries/name).chmod(0o755)
            with patch.object(package_linux, 'ROOT', root):
                output = package_linux.assemble(binaries, root/'bundle')
            self.assertTrue((output/'share/openrailsrs/assets/shaders/test.wgsl').is_file())
            self.assertTrue((output/'share/openrailsrs/examples/smoke/scenario.toml').is_file())
            self.assertFalse(any(p.name in ('run.json', 'credentials', 'private.dat') for p in output.rglob('*')))
            self.assertTrue((output/'bin/openrailsrs').stat().st_mode & 0o111)
            self.assertFalse(json.loads((output/'manifest.json').read_text())['downloaded_content_included'])

    def test_launcher_handles_spaces_and_preserves_player_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "Juego con espacios"
            (root / "bin").mkdir(parents=True)
            launcher = root / "Jugar.sh"
            launcher.write_text(package_linux.LAUNCHER)
            launcher.chmod(0o755)
            viewer = root / "bin/openrailsrs-viewer3d"
            viewer.write_text('#!/bin/sh\nprintf "%s\n" "$OPENRAILSRS_RENDERER" "$@"\n')
            viewer.chmod(0o755)
            result = subprocess.run([str(launcher), "--cpu", "--scenario", "ruta con espacios.toml"],
                                    cwd=directory, text=True, capture_output=True, check=True)
            self.assertEqual(result.stdout.splitlines(), ["cpu", "--menu", "--scenario", "ruta con espacios.toml"])

    def test_launcher_check_reports_loader_failures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "bin").mkdir()
            for name in package_linux.BINS:
                binary = root / "bin" / name
                binary.write_text('#!/bin/sh\nexit 0\n')
                binary.chmod(0o755)
            launcher = root / "Jugar.sh"
            launcher.write_text(package_linux.LAUNCHER)
            launcher.chmod(0o755)
            # ldd must reject scripts/invalid ELF rather than claim readiness.
            result = subprocess.run([str(launcher), "--check"], text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("No se pudo comprobar", result.stderr)

    def test_resource_manifest_cannot_copy_outside_resources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'packaging').mkdir()
            (root/'packaging/resources.json').write_text('["examples/../../credentials"]')
            for name in package_linux.BINS:
                (root/name).touch()
            with patch.object(package_linux, 'ROOT', root):
                with self.assertRaisesRegex(ValueError, 'Invalid resource'):
                    package_linux.assemble(root, root/'bundle')

    def test_snap_build_tree_excludes_previous_builds_and_untracked_downloads(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in ('Cargo.toml', 'snap/snapcraft.yaml', 'target/huge.o', 'tmp/route.dat', '.git/config'):
                path = root/relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('source')
            with patch.object(package_snap, 'ROOT', root), patch.object(package_snap.subprocess, 'check_output', return_value=b'Cargo.toml\0snap/snapcraft.yaml\0target/huge.o\0tmp/route.dat\0.git/config\0'):
                package_snap.prepare(root/'sources')
            self.assertTrue((root/'sources/Cargo.toml').is_file())
            self.assertFalse((root/'sources/target').exists())
            self.assertFalse((root/'sources/tmp').exists())
            self.assertFalse((root/'sources/.git').exists())


if __name__ == '__main__':
    unittest.main()
