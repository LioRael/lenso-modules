"""Record the actual registry SDK installed by completed CLI acceptance projects."""
from pathlib import Path
import argparse
import json
import os
import platform
import subprocess
import tomllib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--language", choices=["typescript", "rust"], required=True)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--legacy-preset", action="store_true")
parser.add_argument("--module-upgrades", action="store_true")
args = parser.parse_args()
suites = ("",) if args.legacy_preset else ("consumers", "apps", "files", "files-upgrade") + (("module-upgrade",) if args.module_upgrades else ())
consumers = []
for suite in suites:
    base = args.root / suite
    for directory, children, files in os.walk(base):
        children[:] = [name for name in children if name not in {"node_modules", "target", ".git", "cache", "author", "served"}]
        project = Path(directory)
        installed = project / "node_modules/@lenso/app/package.json"
        if installed.is_file():
            version = json.loads(installed.read_text())["version"]
            lock = json.loads((project / "package-lock.json").read_text())
            entry = lock["packages"]["node_modules/@lenso/app"]
            assert entry["resolved"].startswith("https://registry.npmjs.org/"), entry
            assert entry["integrity"] and entry["version"] == version
            consumers.append({"project": str(project.relative_to(args.root)), "ecosystem": "npm", "sdk_version": version, "registry": "https://registry.npmjs.org/"})
        if "Cargo.lock" in files:
            packages = tomllib.loads((project / "Cargo.lock").read_text())["package"]
            for package in packages:
                if package["name"] == "lenso-app":
                    assert package["source"].startswith("registry+") and package["checksum"], package
                    consumers.append({"project": str(project.relative_to(args.root)), "ecosystem": "crates.io", "sdk_version": package["version"], "registry": package["source"]})
if args.legacy_preset:
    expected = {f"{args.language}-{storage}" for storage in ("sqlite", "postgres")}
else:
    expected = {f"consumers/{args.language}-{storage}" for storage in ("sqlite", "postgres")}
    expected |= {f"{suite}/{args.language}-{storage}/app" for suite in suites if suite != "consumers" for storage in ("sqlite", "postgres")}

actual = {item["project"] for item in consumers}
assert actual == expected and len(consumers) == len(expected), {"missing": sorted(expected - actual), "unexpected": sorted(actual - expected), "consumer_count": len(consumers)}
assert all(tuple(map(int, item["sdk_version"].split("."))) >= (0, 1, 2) for item in consumers), consumers
report = {"schema_version": 1, "status": "passed", "repository": os.environ.get("GITHUB_REPOSITORY"), "run_id": os.environ.get("GITHUB_RUN_ID"), "checkout_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(), "platform": {"system": platform.system(), "machine": platform.machine(), "node": subprocess.check_output(["node", "--version"], text=True).strip(), "rust": subprocess.check_output(["rustc", "--version"], text=True).strip()}, "consumers": consumers}
args.output.write_text(json.dumps(report, indent=2) + "\n")
print(f"Verified {len(consumers)} registry SDK consumers")
