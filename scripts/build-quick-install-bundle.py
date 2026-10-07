#!/usr/bin/env python3
"""Produce the closed P017 bootstrap bundle; authenticate its digest independently."""
import argparse
import gzip
import hashlib
import io
from pathlib import Path
import tarfile

ROOT = Path(__file__).resolve().parents[1]
FILES = (
    'deploy/install/quick-install.sh',
    'deploy/install/linux-platforms-v1.json',
    'scripts/linux_quick_install.py',
    'scripts/linux_platform_detection.py',
    'scripts/release_channel.py',
    'scripts/release_download.py',
    'scripts/release_manifest.py',
    'scripts/release_signature.py',
    'scripts/requirements-installer-security.txt',
)


def build(output: Path) -> str:
    # No discovery, arbitrary archive names, symlinks, or user/build state.
    with output.open('xb') as file, gzip.GzipFile(filename='',mode='wb',fileobj=file,mtime=0) as compressed:
        with tarfile.open(mode='w',fileobj=compressed,format=tarfile.USTAR_FORMAT) as archive:
            for name in FILES:
                source=ROOT/name
                if source.is_symlink() or not source.is_file(): raise ValueError('invalid bootstrap source')
                data=source.read_bytes()
                info=tarfile.TarInfo(name)
                info.size=len(data); info.mode=0o755 if name.endswith('.sh') else 0o644
                info.uid=info.gid=0; info.mtime=0
                archive.addfile(info,io.BytesIO(data))
    return hashlib.sha256(output.read_bytes()).hexdigest()


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    print(build(args.output))
