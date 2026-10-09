"""Distribution must include resources while excluding downloads and private files."""
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import tempfile
import subprocess
import unittest
from unittest.mock import patch
import package_linux
import package_snap

DISPLAY_LAUNCHER = Path(__file__).resolve().parents[1] / 'snap/local/display-launch.py'
display_environment = runpy.run_path(str(DISPLAY_LAUNCHER))['display_environment']


class PackagingTests(unittest.TestCase):
    def test_snap_launcher_uses_base_python_and_trusted_certificates(self):
        with tempfile.TemporaryDirectory() as directory:
            # Reproduce Snapcraft's removal of Python/CA files already in core24.
            root = Path(directory) / "Snap con espacios"
            (root/'bin').mkdir(parents=True)
            shutil.copyfile(DISPLAY_LAUNCHER, root/'bin/display-launch.py')
            launcher = Path(__file__).resolve().parents[1] / 'snap/local/desktop-launch'
            environment = dict(os.environ, SNAP=str(root),
                               PYTHONHOME=str(root/'missing-python'),
                               SSL_CERT_FILE=str(root/'missing-certificates.pem'))
            probe = (
                'import os, ssl, subprocess; '
                'subprocess.run([os.environ["OPENRAILSRS_PYTHON"], "-c", '
                '"import ssl; assert ssl.create_default_context().cert_store_stats()[\'x509_ca\'] > 0"], '
                'check=True); '
                'assert ssl.create_default_context().cert_store_stats()["x509_ca"] > 0'
            )
            result = subprocess.run(['/bin/sh', str(launcher), '/usr/bin/python3', '-c', probe],
                                    env=environment, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_snap_wayland_resolves_compositor_outside_private_runtime(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory)/'snap.openrailsrs'
            runtime.mkdir()
            path = Path(directory)/'wayland-7'
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
                server.bind(str(path))
                server.listen(4)
                environment = {'SNAP_NAME':'openrailsrs', 'XDG_RUNTIME_DIR':str(runtime),
                               'WAYLAND_DISPLAY':'wayland-7', 'DISPLAY':':0'}
                selected, message = display_environment(environment)
                self.assertEqual(selected['WAYLAND_DISPLAY'], str(path))
                self.assertEqual(selected['XDG_RUNTIME_DIR'], str(runtime))
                self.assertIsNone(message)

    def test_snap_wayland_preserves_absolute_and_inherited_connections(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'compositor'
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
                server.bind(str(path))
                server.listen(4)
                selected, _ = display_environment({'WAYLAND_DISPLAY':str(path)})
                self.assertEqual(selected['WAYLAND_DISPLAY'], str(path))
        left, right = socket.socketpair()
        with left, right:
            selected, _ = display_environment({'WAYLAND_SOCKET':str(left.fileno())})
            self.assertEqual(selected['WAYLAND_SOCKET'], str(left.fileno()))

    def test_snap_stale_wayland_falls_back_to_x11_before_winit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'wayland-0'
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stale:
                stale.bind(str(path))  # socket exists, but no compositor listens.
                selected, message = display_environment({'WAYLAND_DISPLAY':str(path),
                    'WAYLAND_SOCKET':'invalid', 'DISPLAY':':1'})
            self.assertNotIn('WAYLAND_DISPLAY', selected)
            self.assertNotIn('WAYLAND_SOCKET', selected)
            self.assertEqual(selected['DISPLAY'], ':1')
            self.assertIn('X11', message)
            with self.assertRaisesRegex(ValueError, 'conectar'):
                display_environment({'WAYLAND_DISPLAY':str(path),
                                     'OPENRAILSRS_WINDOW_BACKEND':'wayland', 'DISPLAY':':1'})

    def test_snap_x11_override_removes_wayland_and_requires_display(self):
        selected, _ = display_environment({'OPENRAILSRS_WINDOW_BACKEND':'x11',
            'DISPLAY':':2', 'WAYLAND_DISPLAY':'wayland-0', 'WAYLAND_SOCKET':'3'})
        self.assertNotIn('WAYLAND_DISPLAY', selected)
        self.assertNotIn('WAYLAND_SOCKET', selected)
        with self.assertRaisesRegex(ValueError, 'DISPLAY'):
            display_environment({'OPENRAILSRS_WINDOW_BACKEND':'x11'})
        with self.assertRaisesRegex(ValueError, 'auto, wayland o x11'):
            display_environment({'OPENRAILSRS_WINDOW_BACKEND':'invalid'})

    def test_snap_display_launcher_preserves_arguments_and_exit_status(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)/'Snap con espacios'
            (root/'bin').mkdir(parents=True)
            shutil.copyfile(DISPLAY_LAUNCHER, root/'bin/display-launch.py')
            viewer = root/'bin/openrailsrs-viewer3d'
            viewer.write_text('#!/usr/bin/python3\nimport json,os,sys\n'
                'print(json.dumps([sys.argv[1:], os.environ.get("WAYLAND_DISPLAY"), '
                'os.environ.get("DISPLAY")]))\nsys.exit(7)\n')
            viewer.chmod(0o755)
            launcher = DISPLAY_LAUNCHER.parent/'desktop-launch'
            environment = dict(os.environ, SNAP=str(root), DISPLAY=':1',
                WAYLAND_DISPLAY=str(root/'missing-wayland'), WAYLAND_SOCKET='',
                OPENRAILSRS_WINDOW_BACKEND='auto')
            result = subprocess.run(['/bin/sh',str(launcher),str(viewer),
                '--scenario','ruta con espacios.toml'],env=environment,
                text=True,capture_output=True)
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertEqual(json.loads(result.stdout),
                             [['--scenario','ruta con espacios.toml'],None,':1'])

    def test_portable_bundle_is_independent_and_excludes_content_and_outcomes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in ('packaging/resources.json', 'examples/smoke/scenario.toml',
                             'examples/smoke/run.json', 'crates/openrailsrs-bevy-scenery/assets/shaders/test.wgsl',
                             'examples/smoke/run_variant.toml', 'examples/smoke/capture.log',
                             'examples/baselines/test/capture/initial.json',
                             'official-content/route/private.dat', '.aws/credentials'):
                path = root/relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('original')
            resources = ['examples/smoke/scenario.toml', 'examples/smoke/run.json',
                         'examples/smoke/run_variant.toml', 'examples/smoke/capture.log',
                         'examples/baselines/test/capture/initial.json',
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
            self.assertTrue((output/'share/openrailsrs/examples/smoke/run_variant.toml').is_file())
            self.assertFalse((output/'share/openrailsrs/examples/baselines').exists())
            self.assertFalse(any(p.suffix == '.log' for p in output.rglob('*')))
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

    def test_privacy_guard_checks_binaries_and_utf16_without_disclosing_values(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            token = 'ghp_' + 'a' * 36
            (root/'binary').write_bytes(b'\x7fELF\0' + token.encode())
            (root/'scenario.toml').write_text('/home/tester/private/scenario', encoding='utf-16')
            (root/'LICENSE').write_text('Copyright Public Author <author@example.org>')
            findings = package_linux.audit_privacy(root)
            self.assertIn(('binary', 'token de acceso'), findings)
            self.assertIn(('scenario.toml', 'ruta personal del equipo'), findings)
            self.assertFalse(any(name == 'LICENSE' for name, _ in findings))
            with self.assertRaisesRegex(ValueError, 'Private data') as caught:
                package_linux.require_private_data_free(root)
            self.assertNotIn(token, str(caught.exception))
            self.assertNotIn('/home/tester', str(caught.exception))

    def test_resource_manifest_includes_all_shaders_and_new_playable_examples(self):
        root = Path(__file__).resolve().parents[1]
        resources = set(json.loads((root/'packaging/resources.json').read_text()))
        shaders = root/'crates/openrailsrs-bevy-scenery/assets/shaders'
        self.assertTrue(all(str(p.relative_to(root)) in resources for p in shaders.glob('*.wgsl')))
        for relative in ('examples/electric_supply/scenario.toml',
                         'examples/traction_operation/scenario_steam.toml'):
            self.assertIn(relative, resources)

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
