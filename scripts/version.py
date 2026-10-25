#!/usr/bin/env python3
"""Version helper for the release workflow.

  version.py current                 print the workspace version
  version.py bump <patch|minor|major> bump Cargo.toml, CHANGELOG.md and the site, print new version
  version.py site <version>          set the version shown on the site
  version.py notes <version>         print the CHANGELOG section for a version
  version.py check <tag>             fail unless tag == "v" + workspace version
"""
import datetime
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CARGO = ROOT / "Cargo.toml"
CHANGELOG = ROOT / "CHANGELOG.md"
SITE = ROOT / "site" / "index.html"
# The version shown on the site: the download line and the footer.
SITE_VERSIONS = [
    re.compile(r'(class="meta">v)\d+\.\d+\.\d+'),
    re.compile(r"(<span>Ngambang )\d+\.\d+\.\d+"),
]
SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
VERSION_LINE = re.compile(r'(\[workspace\.package\][^\[]*?\nversion\s*=\s*")([^"]+)(")', re.S)


def current() -> str:
    m = VERSION_LINE.search(CARGO.read_text(encoding="utf-8"))
    if not m or not SEMVER.match(m.group(2)):
        sys.exit("Cargo.toml: no semver version in [workspace.package]")
    return m.group(2)


def bump(kind: str) -> str:
    major, minor, patch = map(int, SEMVER.match(current()).groups())
    if kind == "major":
        new = f"{major + 1}.0.0"
    elif kind == "minor":
        new = f"{major}.{minor + 1}.0"
    elif kind == "patch":
        new = f"{major}.{minor}.{patch + 1}"
    else:
        sys.exit(f"unknown bump {kind!r}")
    text = CARGO.read_text(encoding="utf-8")
    CARGO.write_text(VERSION_LINE.sub(lambda m: m.group(1) + new + m.group(3), text, count=1), encoding="utf-8")
    if CHANGELOG.exists():
        log = CHANGELOG.read_text(encoding="utf-8")
        today = datetime.date.today().isoformat()
        log = log.replace("## [Unreleased]", f"## [Unreleased]\n\n## [{new}] - {today}", 1)
        CHANGELOG.write_text(log, encoding="utf-8")
    set_site_version(new)
    return new


def set_site_version(version: str) -> None:
    if not SITE.exists():
        return
    html = SITE.read_text(encoding="utf-8")
    for pattern in SITE_VERSIONS:
        html = pattern.sub(lambda m: m.group(1) + version, html)
    SITE.write_text(html, encoding="utf-8")


def notes(version: str) -> str:
    if not CHANGELOG.exists():
        return f"Ngambang {version}"
    lines = CHANGELOG.read_text(encoding="utf-8").splitlines()
    out, inside = [], False
    for line in lines:
        if line.startswith("## ["):
            if inside:
                break
            inside = line.startswith(f"## [{version}]")
            continue
        if inside:
            out.append(line)
    body = "\n".join(out).strip()
    return body or f"Ngambang {version}"


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    cmd = argv[1]
    if cmd == "current":
        print(current())
    elif cmd == "bump" and len(argv) == 3:
        print(bump(argv[2]))
    elif cmd == "site" and len(argv) == 3:
        set_site_version(argv[2])
    elif cmd == "notes" and len(argv) == 3:
        print(notes(argv[2]))
    elif cmd == "check" and len(argv) == 3:
        want = "v" + current()
        if argv[2] != want:
            sys.exit(f"tag {argv[2]} does not match Cargo.toml version ({want})")
        print(current())
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
