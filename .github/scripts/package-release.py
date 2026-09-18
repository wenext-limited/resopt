#!/usr/bin/env python3
"""Smoke-test and package the native CLI built by this matrix runner."""
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


def write_png(path, width=96, height=96):
    """A stored-deflate RGBA gradient: valid, and trivially compressible."""
    import struct
    import zlib

    def chunk(kind, data):
        body = kind + data
        return struct.pack('>I', len(data)) + body + struct.pack('>I', zlib.crc32(body))

    rows = b''.join(
        b'\x00' + bytes(value for x in range(width) for value in (x * 2 % 256, y * 2 % 256, 120, 255))
        for y in range(height)
    )
    path.write_bytes(
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0))
        + chunk(b'IDAT', zlib.compress(rows, 0))
        + chunk(b'IEND', b'')
    )


def workflow_smoke_test(binary):
    """Analyze, batch-apply and restore a tiny project with the packaged binary."""
    import json

    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary).resolve()
        project = root / 'project'
        (project / 'images').mkdir(parents=True)
        source = project / 'images' / 'gradient.png'
        write_png(source)
        original = source.read_bytes()
        report = root / 'report'
        run = lambda *args: subprocess.run([str(binary), *args], check=True, capture_output=True, text=True)
        run('doctor')
        run('analyze', str(project), '--out', str(report), '--webp', '--no-cache', '--qualities', '85')
        analysis = json.loads((report / 'analysis.json').read_text(encoding='utf-8'))
        row = next(r for r in analysis['resources'] if r['resource']['path'].endswith('gradient.png'))
        formats = {c['format'] for c in row['candidates'] if c.get('artifact')}
        if not {'png', 'webp'} <= formats:
            raise SystemExit(f'Expected PNG and WebP candidates, got {formats}')
        run('apply', str(report))
        if len(source.read_bytes()) >= len(original):
            raise SystemExit('Lossless batch apply did not shrink the file')
        run('restore', str(report))
        if source.read_bytes() != original:
            raise SystemExit('Restore was not byte-exact')


def main():
    target = os.environ['RELEASE_TARGET']
    version = os.environ['RELEASE_VERSION']
    name = f'resopt-v{version}-{target}'
    windows = target.endswith('windows-msvc')
    binary_name = 'resopt.exe' if windows else 'resopt'
    target_dir = Path(os.environ.get('CARGO_TARGET_DIR', 'target'))
    binary = target_dir / target / 'release' / binary_name
    result = subprocess.check_output([str(binary.resolve()), '--version'], text=True).strip()
    if result != f'resopt {version}':
        raise SystemExit(f'Wrong binary version: {result!r}')
    subprocess.run([str(binary.resolve()), 'serve', '--help'], check=True)
    subprocess.run([str(binary.resolve()), 'web', '--help'], check=True)
    workflow_smoke_test(binary.resolve())
    out = Path('dist')
    out.mkdir(exist_ok=True)
    archive = out / f'{name}{".zip" if windows else ".tar.gz"}'
    with tempfile.TemporaryDirectory() as temporary:
        package = Path(temporary) / name
        package.mkdir()
        for source in [binary, Path('README.md'), Path('LICENSE')]:
            shutil.copy2(source, package / source.name)
        if windows:
            with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as bundle:
                for source in sorted(package.iterdir()):
                    bundle.write(source, f'{name}/{source.name}')
        else:
            with tarfile.open(archive, 'w:gz') as bundle:
                bundle.add(package, arcname=name)
    print(archive)


if __name__ == '__main__':
    main()
