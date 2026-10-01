#!/usr/bin/env python3
"""Create and serve a synthetic loopback HTTPS P017 release fixture."""

import argparse
import hashlib
import http.server
import json
import shutil
import ssl
from pathlib import Path


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


parser = argparse.ArgumentParser()
parser.add_argument("--artifact", type=Path, required=True)
parser.add_argument("--artifact-type", choices=("deb", "rpm"), required=True)
parser.add_argument("--version", required=True)
parser.add_argument("--source-commit", required=True)
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--certificate", required=True)
parser.add_argument("--key", required=True)
parser.add_argument("--port", type=int, default=4443)
args = parser.parse_args()
args.root.mkdir(mode=0o700, parents=True, exist_ok=True)
target = args.root / args.artifact.name
shutil.copyfile(args.artifact, target)
package = ({"format": "deb", "package_name": "synveil", "package_version": args.version,
            "package_architecture": "amd64"} if args.artifact_type == "deb" else
           {"format": "rpm", "package_name": "synveil", "package_version": args.version,
            "package_release": "1", "package_architecture": "x86_64"})
payload = target.read_bytes()
manifest = {"schema_version": 1, "product": "Synveil", "product_version": args.version,
            "source_commit": args.source_commit, "artifacts": [{
                "id": f"linux-x86_64-{args.artifact_type}", "artifact_type": args.artifact_type,
                "filename": target.name, "platform": "linux", "architecture": "x86_64",
                "role": "native_package", "product_version": args.version, "size_bytes": len(payload),
                "sha256": hashlib.sha256(payload).hexdigest(), "components": ["synveil-client", "synveil-desktop"],
                "package_metadata": package}]}
manifest_raw = encoded(manifest)
(args.root / "SYNVEIL-RELEASE-MANIFEST.json").write_bytes(manifest_raw)
channel = {"schema_version": 1, "product": "Synveil", "channel": "stable", "generation": 17,
           "releases": [{"product_version": args.version, "source_commit": args.source_commit,
                         "manifest_filename": "SYNVEIL-RELEASE-MANIFEST.json",
                         "manifest_size_bytes": len(manifest_raw),
                         "manifest_sha256": hashlib.sha256(manifest_raw).hexdigest(),
                         "fresh_install": True, "upgrade_from": []}]}
channel_raw = encoded(channel)
(args.root / "SYNVEIL-RELEASE-CHANNEL.json").write_bytes(channel_raw)
print(hashlib.sha256(channel_raw).hexdigest(), flush=True)
handler = lambda *values, **options: http.server.SimpleHTTPRequestHandler(*values, directory=args.root, **options)
server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(args.certificate, args.key)
server.socket = context.wrap_socket(server.socket, server_side=True)
server.serve_forever()
