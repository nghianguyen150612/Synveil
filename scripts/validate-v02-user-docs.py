#!/usr/bin/env python3
"""Deterministic checks for the bilingual v0.2 user and distribution guides."""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
EN = ROOT / "docs/en/v0.2"
VI = ROOT / "docs/vi/v0.2"
TOPICS = {
    "README.md",
    "INSTALLATION.md",
    "FIRST_RUN.md",
    "TROUBLESHOOTING.md",
    "UPGRADE_REPAIR_UNINSTALL.md",
    "ADVANCED_INSTALLATION.md",
    "DISTRIBUTION_READINESS.md",
}
LINK = re.compile(r"(?<!!)\[[^\]]*\]\(([^)]+)\)")
INLINE_CODE = re.compile(chr(96) + r"([^" + chr(96) + r"]+)" + chr(96))
FLAG = re.compile(r"--[A-Za-z][A-Za-z0-9-]*")
FENCE = chr(96) * 3


def fail(message: str) -> None:
    print(f"V02-DOCS: {message}", file=sys.stderr)
    raise SystemExit(1)


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        fail(f"cannot read {path.relative_to(ROOT)}: {error}")


def links_in(path: Path, content: str) -> None:
    for match in LINK.finditer(content):
        raw = match.group(1).strip().split(maxsplit=1)[0].strip("<>")
        if not raw or raw.startswith("#"):
            continue
        parsed = urlsplit(raw)
        if parsed.scheme in {"http", "https", "mailto"}:
            if parsed.scheme == "mailto":
                continue
            allowed = (
                parsed.scheme == "https"
                and parsed.netloc == "github.com"
                and re.fullmatch(
                    r"/nghianguyen150612/Synveil/actions/runs/[0-9]+", parsed.path
                )
            ) or (
                parsed.scheme == "https"
                and parsed.netloc == "github.com"
                and parsed.path == "/nghianguyen150612/Synveil/pull/78"
            )
            if not allowed:
                fail(f"unreviewed external URL in {path.relative_to(ROOT)}: {raw}")
            continue
        target, _, _fragment = raw.partition("#")
        candidate = (path.parent / target).resolve()
        if not candidate.exists():
            fail(f"broken relative link in {path.relative_to(ROOT)}: {raw}")


def fenced_blocks(content: str) -> list[str]:
    blocks: list[str] = []
    current: list[str] | None = None
    for line in content.splitlines():
        if line.startswith(FENCE):
            if current is None:
                current = []
            else:
                blocks.append("\n".join(current))
                current = None
        elif current is not None:
            current.append(line)
    if current is not None:
        fail("unclosed Markdown code fence")
    return blocks


def main() -> None:
    en_files = {path.name for path in EN.glob("*.md")}
    vi_files = {path.name for path in VI.glob("*.md")}
    if en_files != TOPICS or vi_files != TOPICS:
        fail("English/Vietnamese topic sets differ from the required v0.2 guide set")

    all_content: dict[tuple[str, str], str] = {}
    for locale, directory in (("en", EN), ("vi", VI)):
        for name in sorted(TOPICS):
            path = directory / name
            content = read(path)
            all_content[(locale, name)] = content
            links_in(path, content)

            for block in fenced_blocks(content):
                if re.search(
                    r"(?i)(--no-verify|--skip-signature|--force-install|chmod\s+777|curl\s*\|\s*sh)",
                    block,
                ):
                    fail(f"unsafe install/recovery command in {path.relative_to(ROOT)}")

            if re.search(
                r"(?i)(example\.(?:com|net|org)|BEGIN (?:RSA|OPENSSH|EC) PRIVATE KEY|"
                r"gh[pousr]_[A-Za-z0-9_]{12,}|sk-[A-Za-z0-9]{12,}|\b[a-f0-9]{64}\b)",
                content,
            ):
                fail(f"placeholder URL, credential, private key, or fabricated digest in {path.relative_to(ROOT)}")

    for name in sorted(TOPICS):
        en = all_content[("en", name)]
        vi = all_content[("vi", name)]
        if Counter(INLINE_CODE.findall(en)) != Counter(INLINE_CODE.findall(vi)):
            fail(f"English/Vietnamese inline commands, filenames, or flags differ in {name}")
        if fenced_blocks(en) != fenced_blocks(vi):
            fail(f"English/Vietnamese command/code examples differ in {name}")

    troubleshooting_categories = (
        ("Unsupported operating system", "Hệ điều hành không được hỗ trợ"),
        ("Unsupported architecture", "Kiến trúc không được hỗ trợ"),
        ("Download failure", "Tải xuống thất bại"),
        ("Artifact integrity failure", "Không xác minh được tệp cài đặt"),
        ("Insufficient disk space", "Không đủ dung lượng đĩa"),
        ("Missing authorization", "Thiếu quyền cấp phép"),
        ("Installation failure", "Cài đặt thất bại"),
        ("Interrupted installation", "Cài đặt bị gián đoạn"),
        ("Recovery required", "Yêu cầu phục hồi"),
        ("Existing installation conflict", "Xung đột với cài đặt hiện có"),
        ("Runtime dependency problem", "Sự cố môi trường chạy"),
        ("First-run connection failure", "Kết nối lần đầu thất bại"),
        ("Authentication failure", "Xác thực thất bại"),
        ("Initial synchronization does not start", "Đồng bộ lần đầu không bắt đầu"),
    )
    for locale, index in (("en", 0), ("vi", 1)):
        troubleshooting = all_content[(locale, "TROUBLESHOOTING.md")]
        rows = [line for line in troubleshooting.splitlines() if line.startswith("| **")]
        if len(rows) != len(troubleshooting_categories) or any(line.count("|") != 5 for line in rows):
            fail(f"{locale} troubleshooting table must contain all 14 four-column cases")
        for pair in troubleshooting_categories:
            if pair[index] not in troubleshooting:
                fail(f"{locale} troubleshooting guide omits a required recovery category: {pair[index]}")

    required_platform_facts = (
        "v0.1.0",
        "v0.2",
        "Ubuntu 24.04",
        "Fedora 42",
        "Debian",
        "x86_64",
        "ARM64/aarch64",
        "SynveilSetup.exe",
        "synveil_<version>_amd64.deb",
        "synveil-<version>-1.x86_64.rpm",
        "Synveil-<version>-x86_64.AppImage",
    )
    for locale, name in (
        ("en", "INSTALLATION.md"),
        ("vi", "INSTALLATION.md"),
        ("en", "DISTRIBUTION_READINESS.md"),
        ("vi", "DISTRIBUTION_READINESS.md"),
    ):
        content = all_content[(locale, name)]
        for fact in required_platform_facts:
            if fact not in content:
                fail(f"{locale}/{name} omits required platform/artifact fact: {fact}")

    for locale, name in (
        ("en", "README.md"),
        ("en", "INSTALLATION.md"),
        ("en", "DISTRIBUTION_READINESS.md"),
    ):
        content = all_content[(locale, name)]
        if "v0.1.0" not in content or "v0.2" not in content or not re.search(r"not (?:been )?released", content.lower()):
            fail(f"{locale}/{name} does not clearly distinguish released v0.1.0 from upcoming v0.2")
    for locale, name in (
        ("vi", "README.md"),
        ("vi", "INSTALLATION.md"),
        ("vi", "DISTRIBUTION_READINESS.md"),
    ):
        content = all_content[(locale, name)]
        if "v0.1.0" not in content or "v0.2" not in content or "chưa được phát hành" not in content:
            fail(f"{locale}/{name} does not clearly distinguish released v0.1.0 from upcoming v0.2")

    for locale, name in (("en", "README.md"), ("en", "INSTALLATION.md"),
                         ("en", "DISTRIBUTION_READINESS.md")):
        if re.search(r"\bv0\.2(?:\.0)?\s+(?:is\s+)?(?:released|shipped|current)\b",
                     all_content[(locale, name)], re.IGNORECASE):
            fail(f"{locale}/{name} makes a false positive v0.2 release claim")
    for locale, name in (("vi", "README.md"), ("vi", "INSTALLATION.md"),
                         ("vi", "DISTRIBUTION_READINESS.md")):
        if re.search(r"\bv0\.2(?:\.0)?\s+đã\s+(?:được\s+)?phát hành\b",
                     all_content[(locale, name)], re.IGNORECASE):
            fail(f"{locale}/{name} makes a false positive v0.2 release claim")

    for locale, name in (("en", "INSTALLATION.md"), ("vi", "INSTALLATION.md")):
        content = all_content[(locale, name)]
        support_limit = (
            "No exact Windows version" in content and "not qualified" in content
            if locale == "en"
            else "Chưa có phiên bản Windows cụ thể" in content and "chưa được chứng nhận" in content
        )
        if not support_limit:
            fail(f"{locale}/{name} omits exact OS qualification limits")

    statuses = (
        "Implemented",
        "Build validated",
        "Native tested",
        "Native qualified",
        "Blocked",
        "Not yet published",
        "Unsupported",
    )
    for locale in ("en", "vi"):
        readiness = all_content[(locale, "DISTRIBUTION_READINESS.md")]
        for status in statuses:
            if status not in readiness:
                fail(f"{locale} distribution checklist omits the status label {status}")

    documented_flags = {
        flag
        for content in all_content.values()
        for flag in FLAG.findall(" ".join(INLINE_CODE.findall(content)))
    }
    source_text = "\n".join(
        read(ROOT / source)
        for source in ("scripts/linux_quick_install.py", "deploy/install/uninstall.sh")
    )
    implemented_flags = set(FLAG.findall(source_text))
    unknown_flags = sorted(documented_flags - implemented_flags)
    if unknown_flags:
        fail("documented CLI flags are absent from inspected entrypoints: " + ", ".join(unknown_flags))

    for path in (ROOT / "README.md", ROOT / "docs/README.md"):
        content = read(path)
        links_in(path, content)
    print(
        f"v0.2 bilingual docs validated: {len(TOPICS)} matched topics, "
        "14 troubleshooting cases, relative links, code/flag parity, release status, "
        "platform limits, and trust hygiene"
    )


if __name__ == "__main__":
    main()
