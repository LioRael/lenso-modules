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
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
consumers = []
for suite in ("consumers", "apps", "files", "files-upgrade"):
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
assert consumers, "No installed SDK consumers found"
assert all(tuple(map(int, item["sdk_version"].split("."))) >= (0, 1, 2) for item in consumers), consumers
report = {"schema_version": 1, "status": "passed", "repository": os.environ.get("GITHUB_REPOSITORY"), "run_id": os.environ.get("GITHUB_RUN_ID"), "checkout_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(), "platform": {"system": platform.system(), "machine": platform.machine(), "node": subprocess.check_output(["node", "--version"], text=True).strip(), "rust": subprocess.check_output(["rustc", "--version"], text=True).strip()}, "consumers": consumers}
args.output.write_text(json.dumps(report, indent=2) + "\n")
print(f"Verified {len(consumers)} registry SDK consumers")
