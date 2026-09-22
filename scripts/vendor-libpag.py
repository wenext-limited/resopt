#!/usr/bin/env python3
"""Reproduce pinned offline PAG artifacts without running package install hooks."""
import base64
import hashlib
import io
from pathlib import Path
import tarfile
import urllib.request

VERSION = "4.3.51"
INTEGRITY = "MGPMkSDpJxnrLxKjhAwtPITFEJrS0BVuUTNa6GcRuDzZoMgrYP9sIB6DMnb+O0HXgrtkxRJp3Askj4focqIfgw=="
URL = f"https://registry.npmjs.org/libpag/-/libpag-{VERSION}.tgz"
OUTPUT = Path(__file__).resolve().parents[1] / "src/ui/vendor/libpag"
FILES = {"libpag.js": "package/lib/libpag.min.js", "libpag.wasm": "package/lib/libpag.wasm", "LICENSE.txt": "package/LICENSE.txt"}
with urllib.request.urlopen(URL, timeout=60) as response:
    data = response.read(16 * 1024 * 1024 + 1)
if len(data) > 16 * 1024 * 1024 or base64.b64encode(hashlib.sha512(data).digest()).decode() != INTEGRITY:
    raise SystemExit("Pinned libpag integrity mismatch")
OUTPUT.mkdir(parents=True, exist_ok=True)
with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as package:
    for name, member in FILES.items():
        entry = package.getmember(member)
        if not entry.isfile() or entry.size > 8 * 1024 * 1024:
            raise SystemExit("Invalid runtime entry")
        payload = package.extractfile(entry).read()
        (OUTPUT / name).write_bytes(payload)
        print(f"{name}: {hashlib.sha256(payload).hexdigest()}")
