#!/usr/bin/env python3
import argparse
import re
from pathlib import Path


def prerelease_key(value):
    if value is None:
        return ()
    key = []
    for part in value.split("."):
        if part.isdigit():
            key.append((0, int(part)))
        else:
            key.append((1, part))
    return tuple(key)


def latest_any():
    pattern = re.compile(
        r"^lua-qjson-(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?-(\d+)\.rockspec$"
    )
    matches = []
    for path in Path("rockspec").glob("lua-qjson-*.rockspec"):
        match = pattern.match(path.name)
        if match:
            major, minor, patch, prerelease, revision = match.groups()
            matches.append(
                (
                    int(major),
                    int(minor),
                    int(patch),
                    prerelease is None,
                    prerelease_key(prerelease),
                    int(revision),
                    str(path),
                )
            )
    if not matches:
        raise SystemExit("no lua-qjson rockspec found")
    return max(matches)[-1]


def latest_for_version(version):
    version = version.removeprefix("v")
    pattern = re.compile(r"^lua-qjson-" + re.escape(version) + r"-(\d+)\.rockspec$")
    matches = []
    for path in Path("rockspec").glob("lua-qjson-" + version + "-*.rockspec"):
        match = pattern.match(path.name)
        if match:
            matches.append((int(match.group(1)), str(path)))
    if not matches:
        raise SystemExit("rockspec file not found for version " + version)
    return max(matches)[1]


def main():
    parser = argparse.ArgumentParser(description="Print the newest lua-qjson rockspec path.")
    parser.add_argument("--version", help="Select the newest revision for a release version.")
    args = parser.parse_args()

    if args.version:
        print(latest_for_version(args.version))
    else:
        print(latest_any())


if __name__ == "__main__":
    main()
