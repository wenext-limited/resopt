#!/usr/bin/env python3
"""Smoke-test and package the native CLI built by this matrix runner."""
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


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
