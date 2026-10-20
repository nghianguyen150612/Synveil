#!/usr/bin/env python3
"""Fetch and authenticate the pinned upstream AppImageKit type-2 runtime."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

UPSTREAM = "AppImage/type2-runtime"
RELEASE_TAG = "20251108"
SOURCE_REVISION = "dd6cebedcbddde9c82f89b011e8e1d40b6e43868"
RELEASE_API = f"https://api.github.com/repos/{UPSTREAM}/releases/tags/{RELEASE_TAG}"
TAG_REF_API = f"https://api.github.com/repos/{UPSTREAM}/git/ref/tags/{RELEASE_TAG}"
RELEASE_BASE = f"https://github.com/{UPSTREAM}/releases/download/{RELEASE_TAG}"
LICENSE_URL = f"https://raw.githubusercontent.com/{UPSTREAM}/{SOURCE_REVISION}/LICENSE"
SIGNER_FINGERPRINT = "570C77ACEA40C0F1B758902CBF96CCA56490F695"
LICENSE_SHA256 = "aa154fc9070614bbe7921f89db11efd1dba7a1f3a41685958110e2230f9c0ca1"
ASSETS = {
    "runtime-x86_64": {
        "url": f"{RELEASE_BASE}/runtime-x86_64",
        "size": 944632,
        "sha256": "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d",
    },
    "runtime-x86_64.sig": {
        "url": f"{RELEASE_BASE}/runtime-x86_64.sig",
        "size": 119,
        "sha256": "de8262d60b5f82bcff11b77598aedef86d2c375fc246d6a675f6cac68b6dfda7",
    },
    "signing-pubkey.asc": {
        "url": f"{RELEASE_BASE}/signing-pubkey.asc",
        "size": 685,
        "sha256": "db8b615eb5bbf5e8418d52906b4a492960926370e2c5041ad6acaccda56ef0c6",
    },
}
MAX_METADATA_BYTES = 256 * 1024
MAX_REDIRECTS = 5
RELEASE_ASSET_HOSTS = frozenset({"github.com", "release-assets.githubusercontent.com"})


class AcquisitionError(ValueError):
    pass


def _unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise AcquisitionError("upstream metadata contains a duplicate field")
        result[key] = value
    return result


class _RestrictedRedirect(HTTPRedirectHandler):
    def __init__(self, allowed_hosts: frozenset[str]):
        super().__init__()
        self.allowed_hosts = allowed_hosts
        self.redirects = 0

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        target = urlsplit(newurl)
        if (target.scheme != "https" or target.username is not None or target.password is not None
                or (target.hostname or "").lower() not in self.allowed_hosts):
            raise AcquisitionError("upstream redirect escaped the approved HTTPS hosts")
        self.redirects += 1
        if self.redirects > MAX_REDIRECTS:
            raise AcquisitionError("upstream redirect limit exceeded")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def _read_url(url: str, *, limit: int, allowed_hosts: frozenset[str],
              accept: str = "application/octet-stream") -> tuple[bytes, str]:
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or parsed.username is not None or parsed.password is not None
            or (parsed.hostname or "").lower() not in allowed_hosts):
        raise AcquisitionError("upstream URL is outside the approved HTTPS hosts")
    redirect = _RestrictedRedirect(allowed_hosts)
    opener = build_opener(redirect)
    request = Request(url, headers={
        "Accept": accept,
        "User-Agent": "Synveil-R03-AppImage-runtime-acquisition/1",
    })
    try:
        with opener.open(request, timeout=30) as response:
            final = urlsplit(response.geturl())
            if final.scheme != "https" or (final.hostname or "").lower() not in allowed_hosts:
                raise AcquisitionError("upstream response origin is not approved")
            data = response.read(limit + 1)
            if len(data) > limit:
                raise AcquisitionError("upstream response exceeded its size limit")
            return data, response.geturl()
    except AcquisitionError:
        raise
    except (HTTPError, URLError, TimeoutError, OSError) as error:
        raise AcquisitionError("upstream HTTPS request failed") from error


def _check_bytes(name: str, data: bytes, expected_size: int, expected_sha256: str) -> None:
    if len(data) != expected_size or hashlib.sha256(data).hexdigest() != expected_sha256:
        raise AcquisitionError(f"{name}: size or pinned upstream SHA-256 mismatch")


def _api_json(url: str) -> dict:
    data, _ = _read_url(url, limit=MAX_METADATA_BYTES,
                        allowed_hosts=frozenset({"api.github.com"}),
                        accept="application/vnd.github+json")
    try:
        value = json.loads(data.decode("utf-8"), object_pairs_hook=_unique_object)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise AcquisitionError("GitHub release metadata is not valid UTF-8 JSON") from error
    if not isinstance(value, dict):
        raise AcquisitionError("GitHub release metadata has an invalid shape")
    return value


def _verify_release_metadata() -> None:
    release = _api_json(RELEASE_API)
    if release.get("tag_name") != RELEASE_TAG or release.get("target_commitish") != SOURCE_REVISION:
        raise AcquisitionError("upstream release tag or source revision changed")
    ref = _api_json(TAG_REF_API)
    target = ref.get("object")
    if (ref.get("ref") != f"refs/tags/{RELEASE_TAG}" or not isinstance(target, dict)
            or target.get("type") != "commit" or target.get("sha") != SOURCE_REVISION):
        raise AcquisitionError("upstream release tag does not resolve to the pinned source commit")
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise AcquisitionError("upstream release has no asset inventory")
    by_name = {item.get("name"): item for item in assets if isinstance(item, dict)}
    for name, expected in ASSETS.items():
        actual = by_name.get(name)
        if (not isinstance(actual, dict) or actual.get("size") != expected["size"]
                or actual.get("digest") != f"sha256:{expected['sha256']}"
                or actual.get("browser_download_url") != expected["url"]):
            raise AcquisitionError(f"GitHub release asset evidence changed for {name}")


def _primary_fingerprint(public_key: Path) -> str:
    try:
        result = subprocess.run(
            ["gpg", "--batch", "--with-colons", "--show-keys", str(public_key)],
            check=True, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=15, env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C"},
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise AcquisitionError("GnuPG is required to inspect the upstream runtime key") from error
    primary = False
    for line in result.stdout.decode("ascii", "strict").splitlines():
        fields = line.split(":")
        if fields[0] == "pub":
            primary = True
        elif fields[0] == "sub":
            primary = False
        elif fields[0] == "fpr" and primary and len(fields) > 9:
            return fields[9]
    raise AcquisitionError("upstream runtime key has no primary fingerprint")


def _verify_signature(work: Path, runtime: Path, signature: Path, public_key: Path) -> None:
    if _primary_fingerprint(public_key) != SIGNER_FINGERPRINT:
        raise AcquisitionError("upstream runtime public key fingerprint mismatch")
    home = work / "gnupg"
    home.mkdir(mode=0o700)
    try:
        subprocess.run(
            ["gpg", "--homedir", str(home), "--batch", "--no-tty", "--import", str(public_key)],
            check=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, timeout=15,
            env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C"},
        )
        result = subprocess.run(
            ["gpg", "--homedir", str(home), "--batch", "--no-tty", "--status-fd=1",
             "--verify", str(signature), str(runtime)],
            check=True, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=15,
            env={"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LC_ALL": "C"},
        )
    except (OSError, subprocess.SubprocessError) as error:
        raise AcquisitionError("upstream AppImage runtime signature verification failed") from error
    valid = []
    for line in result.stdout.decode("ascii", "strict").splitlines():
        fields = line.split()
        if len(fields) >= 3 and fields[:2] == ["[GNUPG:]", "VALIDSIG"]:
            valid.append(fields[2:])
    if not valid or not any(SIGNER_FINGERPRINT in (entry[0], entry[-1]) for entry in valid):
        raise AcquisitionError("upstream signature is not from the pinned AppImage runtime key")


def _verify_elf_x86_64(runtime: Path) -> None:
    data = runtime.read_bytes()[:20]
    if (len(data) < 20 or data[:4] != b"\x7fELF" or data[4] != 2
            or data[5] != 1 or int.from_bytes(data[16:18], "little") not in {2, 3}
            or int.from_bytes(data[18:20], "little") != 62):
        raise AcquisitionError("upstream runtime is not a little-endian ELF64 x86_64 executable")


def _write_file(path: Path, data: bytes, mode: int = 0o644) -> None:
    with path.open("xb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    path.chmod(mode)


def acquire(output: Path, github_env: Path | None = None) -> dict:
    repo = Path(__file__).resolve().parents[1]
    license_file = repo / "deploy/packages/appimage/APPIMAGE-RUNTIME-LICENSE.txt"
    if not license_file.is_file() or hashlib.sha256(license_file.read_bytes()).hexdigest() != LICENSE_SHA256:
        raise AcquisitionError("vendored AppImage runtime license notice differs from the pinned source")

    output = output.absolute()
    if any(parent.is_symlink() for parent in (output, *output.parents)):
        raise AcquisitionError("runtime output path contains a symlink")
    if output.exists():
        raise AcquisitionError("runtime output already exists; existing files are preserved")
    output.parent.mkdir(parents=True, exist_ok=True)
    _verify_release_metadata()

    with tempfile.TemporaryDirectory(prefix="synveil-appimage-runtime-") as temp_name:
        work = Path(temp_name)
        downloaded = {}
        final_urls = {}
        for name, expected in ASSETS.items():
            data, final_url = _read_url(expected["url"], limit=expected["size"], allowed_hosts=RELEASE_ASSET_HOSTS)
            _check_bytes(name, data, expected["size"], expected["sha256"])
            path = work / name
            _write_file(path, data, 0o600)
            downloaded[name] = path
            final_urls[name] = final_url

        license_bytes, license_final = _read_url(
            LICENSE_URL, limit=64 * 1024,
            allowed_hosts=frozenset({"raw.githubusercontent.com"}),
        )
        if hashlib.sha256(license_bytes).hexdigest() != LICENSE_SHA256 or license_bytes != license_file.read_bytes():
            raise AcquisitionError("upstream runtime license bytes differ from the pinned notice")
        (work / "APPIMAGE-RUNTIME-LICENSE.txt").write_bytes(license_bytes)

        runtime = downloaded["runtime-x86_64"]
        _verify_elf_x86_64(runtime)
        _verify_signature(work, runtime, downloaded["runtime-x86_64.sig"], downloaded["signing-pubkey.asc"])
        runtime_digest = ASSETS["runtime-x86_64"]["sha256"]
        provenance = {
            "schema_version": 1,
            "component": "AppImageKit type-2 runtime",
            "version": RELEASE_TAG,
            "source_repository": f"https://github.com/{UPSTREAM}",
            "source_revision": SOURCE_REVISION,
            "release_url": f"https://github.com/{UPSTREAM}/releases/tag/{RELEASE_TAG}",
            "release_api": RELEASE_API,
            "runtime_url": ASSETS["runtime-x86_64"]["url"],
            "runtime_delivery_host": (urlsplit(final_urls["runtime-x86_64"]).hostname or "").lower(),
            "size_bytes": ASSETS["runtime-x86_64"]["size"],
            "sha256": runtime_digest,
            "github_release_asset_digest": f"sha256:{runtime_digest}",
            "signature_url": ASSETS["runtime-x86_64.sig"]["url"],
            "signer_fingerprint": SIGNER_FINGERPRINT,
            "signature_verified": True,
            "license": "MIT",
            "license_url": LICENSE_URL,
            "license_delivery_host": (urlsplit(license_final).hostname or "").lower(),
            "license_sha256": LICENSE_SHA256,
            "license_notice_path": "deploy/packages/appimage/APPIMAGE-RUNTIME-LICENSE.txt",
            "bundled_third_party_notices": "The upstream MIT notice enumerates musl, libfuse, squashfuse, libzstd, and zlib license sources.",
        }

        stage = Path(tempfile.mkdtemp(prefix=".synveil-appimage-runtime.", dir=output.parent))
        try:
            _write_file(stage / "runtime-x86_64", runtime.read_bytes(), 0o755)
            _write_file(stage / "runtime-x86_64.sig", downloaded["runtime-x86_64.sig"].read_bytes())
            _write_file(stage / "signing-pubkey.asc", downloaded["signing-pubkey.asc"].read_bytes())
            _write_file(stage / "APPIMAGE-RUNTIME-LICENSE.txt", license_bytes)
            _write_file(stage / "runtime-provenance.json", (json.dumps(provenance, sort_keys=True, indent=2) + "\n").encode())
            directory_fd = os.open(stage, os.O_RDONLY | os.O_DIRECTORY)
            try:
                stage.chmod(0o755)
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
            os.rename(stage, output)
        finally:
            if stage.exists():
                shutil.rmtree(stage)

    if github_env is not None:
        values = (
            f"SYNVEIL_APPIMAGE_RUNTIME_FILE={output / 'runtime-x86_64'}",
            f"SYNVEIL_APPIMAGE_RUNTIME_SHA256={ASSETS['runtime-x86_64']['sha256']}",
        )
        if any("\n" in value or "\r" in value for value in values):
            raise AcquisitionError("GitHub environment values are not single-line")
        with github_env.open("a", encoding="utf-8", newline="\n") as stream:
            stream.write("\n".join(values) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
    return provenance


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("target/appimage-runtime"))
    parser.add_argument("--github-env", type=Path)
    args = parser.parse_args()
    try:
        identity = acquire(args.output, args.github_env)
    except (AcquisitionError, OSError, UnicodeError, subprocess.SubprocessError) as error:
        print(f"BLOCKED_APPIMAGE_RUNTIME: {error}", file=sys.stderr)
        return 2
    print(json.dumps(identity, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
