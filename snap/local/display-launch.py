#!/usr/bin/env python3
"""Resolve the desktop connection before winit selects its Linux backend."""
import os
from pathlib import Path
import socket
import stat
import sys


def reachable(path):
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(0.5)
            connection.connect(str(path))
        return True
    except OSError:
        return False


def inherited_wayland_socket(value):
    try:
        return stat.S_ISSOCK(os.fstat(int(value)).st_mode)
    except (OSError, ValueError, TypeError):
        return False


def display_environment(environment):
    result = dict(environment)
    mode = result.get('OPENRAILSRS_WINDOW_BACKEND', 'auto').strip().lower()
    if mode not in ('auto', 'wayland', 'x11'):
        raise ValueError('OPENRAILSRS_WINDOW_BACKEND debe ser auto, wayland o x11.')
    x11 = bool(result.get('DISPLAY'))
    if mode == 'x11':
        if not x11:
            raise ValueError('No hay una conexión X11 disponible (DISPLAY vacío).')
        result.pop('WAYLAND_DISPLAY', None)
        result.pop('WAYLAND_SOCKET', None)
        return result, 'Pantalla X11 solicitada.'

    if inherited_wayland_socket(result.get('WAYLAND_SOCKET')):
        return result, None
    result.pop('WAYLAND_SOCKET', None)
    name = result.get('WAYLAND_DISPLAY') or ('wayland-0' if mode == 'wayland' else '')
    if name:
        path = Path(name)
        candidates = [path] if path.is_absolute() else []
        runtime = Path(result.get('XDG_RUNTIME_DIR', ''))
        if not path.is_absolute() and runtime.is_absolute():
            candidates.append(runtime / path)
            # snapd isolates client runtime files, while the compositor's socket
            # remains in its parent directory. Do not replace XDG_RUNTIME_DIR.
            instance = result.get('SNAP_INSTANCE_NAME') or result.get('SNAP_NAME')
            if instance and runtime.name == f'snap.{instance}':
                candidates.append(runtime.parent / path)
        for candidate in candidates:
            if reachable(candidate):
                result['WAYLAND_DISPLAY'] = str(candidate)
                return result, None

    if x11 and mode == 'auto':
        result.pop('WAYLAND_DISPLAY', None)
        return result, ('Wayland no está disponible; usando X11.' if name else None)
    raise ValueError('No se pudo conectar con la pantalla. Comprobá las interfaces '
                     'wayland/x11 del Snap y ejecutá el juego desde tu sesión gráfica.')


def audio_environment(environment):
    result = dict(environment)
    if not result.get('PULSE_SERVER'):
        runtime = Path(result.get('XDG_RUNTIME_DIR', ''))
        instance = result.get('SNAP_INSTANCE_NAME') or result.get('SNAP_NAME')
        if runtime.is_absolute() and instance and runtime.name == f'snap.{instance}':
            server = runtime.parent / 'pulse/native'
            if reachable(server):
                result['PULSE_SERVER'] = f'unix:{server}'
    return result


def main():
    if len(sys.argv) < 2:
        print('openrailsrs: falta el comando de la aplicación.', file=sys.stderr)
        return 2
    environment = audio_environment(os.environ)
    if Path(sys.argv[1]).name == 'openrailsrs-viewer3d':
        try:
            environment, message = display_environment(environment)
        except ValueError as error:
            print(f'openrailsrs: {error}', file=sys.stderr)
            return 1
        if message:
            print(f'openrailsrs: {message}', file=sys.stderr)
    os.execvpe(sys.argv[1], sys.argv[1:], environment)


if __name__ == '__main__':
    sys.exit(main())
