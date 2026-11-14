"""Verify module metadata against the released SDK in an isolated registry consumer."""
from pathlib import Path
import argparse
import os
import json
import platform
import shutil
import subprocess
import tempfile

SDK_VERSION = "0.1.2"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, default=Path(".local/module-metadata"))
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
cargo = subprocess.check_output(["rustup", "which", "--toolchain", "1.99.0", "cargo"], text=True).strip()
env = {**os.environ, "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": "", "CARGO_TARGET_DIR": str(output / "target"), "LENSO_MODULE_METADATA_REPORT_DIR": str(output)}

def run(label, command, cwd, extra=None):
    print(f"{label}: {' '.join(map(str, command))}", flush=True)
    log = output / f"{label}.log"
    with log.open("w") as stream:
        result = subprocess.run(command, cwd=cwd, env={**env, **(extra or {})}, stdout=stream, stderr=subprocess.STDOUT)
    if result.returncode:
        print(log.read_text()[-12000:], flush=True)
        raise SystemExit(result.returncode)

with tempfile.TemporaryDirectory(prefix="lenso-modules-sdk-") as directory:
    temp = Path(directory)
    consumer = temp / "modules"
    shutil.copytree(root, consumer, ignore=shutil.ignore_patterns(".git", ".local", "node_modules", "dist", "target", "__pycache__"))
    run("install", ["npm", "ci"], consumer)
    installed = json.loads((consumer / "node_modules/@lenso/app/package.json").read_text())["version"]
    assert installed == SDK_VERSION, f"Expected registry App {SDK_VERSION}, got {installed}"
    run("typescript", ["npm", "test"], consumer)
    run("lint", ["npm", "run", "lint"], consumer)
    manifest = consumer / "rust/Cargo.toml"
    native = [cargo]
    run("rust-check", native + ["check", "--locked", "--manifest-path", str(manifest), "--all-targets"], consumer)
    for label, flags in [("rust-tests", ["test"]), ("rust-clippy", ["clippy", "--all-targets", "--", "-D", "warnings"]), ("rust-doc", ["doc", "--no-deps"]), ("rust-build", ["build", "--bins"])]:
        command = native + [flags[0], "--locked", "--manifest-path", str(manifest)] + flags[1:]
        run(label, command, consumer)
    run("rust-format", [cargo, "fmt", "--manifest-path", str(manifest), "--check"], consumer)
    if env.get("AUTH_POSTGRES_URL"):
        run("postgres-typescript", ["npm", "run", "test:postgres:typescript"], consumer)
        run("postgres-rust", native + ["test", "--locked", "--manifest-path", str(manifest), "--test", "postgres", "--", "--ignored"], consumer)
    run("integration", ["npm", "run", "test:integration"], consumer, {"RUST_AUTH_BIN": str(output / "target/debug/auth-server"), "RUST_METADATA_BIN": str(output / "target/debug/metadata-server")})
reports = [json.loads((output / f"{runtime}.json").read_text()) for runtime in ("typescript", "rust")]
assert {k: v for k, v in reports[0].items() if k != "runtime"} == {k: v for k, v in reports[1].items() if k != "runtime"}, "TS/Rust public descriptions and failures differ"
(output / "verification.json").write_text(json.dumps({"schema_version": 1, "status": "passed", "sdk_registry_version": SDK_VERSION, "platform": {"system": platform.system(), "machine": platform.machine(), "node": subprocess.check_output(["node", "--version"], text=True).strip(), "cargo": subprocess.check_output([cargo, "--version"], text=True).strip()}, "checks": ["typescript", "lint", "rust-check", "rust-tests", "rust-clippy", "rust-doc", "rust-format", "integration"], "postgres_executed": bool(env.get("AUTH_POSTGRES_URL")), "shared_contract": "TS/Rust reports match except runtime"}, indent=2) + "\n")
print(f"All module metadata checks passed; evidence: {output}")
