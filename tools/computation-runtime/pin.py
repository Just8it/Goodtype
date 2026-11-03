"""Development tool: pin and stage the optional Windows Python runtime without installing it on the host."""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import urllib.request
import zipfile
import tempfile
import time

VERSION = "3.14.10"
URL = f"https://www.python.org/ftp/python/{VERSION}/python-{VERSION}-embed-amd64.zip"
# Published on the official Python 3.14.10 release page, independently checked before pinning.
SHA = "90905e64a840557d8c1bc4f5b74db3d7ee85e3ffb217611cac1c7813acea4159"

def fetch(url, destination, expected):
    if not destination.exists():
        last_error = None
        for attempt in range(3):
            try:
                with urllib.request.urlopen(url, timeout=60) as response:
                    if response.url.split("/")[2] not in ("www.python.org", "files.pythonhosted.org"):
                        raise ValueError("Unexpected download host")
                    data = response.read(80 * 1024 * 1024 + 1)
                if len(data) > 80 * 1024 * 1024:
                    raise ValueError("Archive exceeds the download limit")
                if hashlib.sha256(data).hexdigest() != expected:
                    raise ValueError("Archive hash mismatch")
                destination.write_bytes(data)
                break
            except (OSError, TimeoutError) as error:
                last_error = error
                print(f"Retry {attempt + 1}: {destination.name}: {type(error).__name__}", flush=True)
        else:
            raise last_error
    data = destination.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Archive hash mismatch")
    return data

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=pathlib.Path)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--download", action="store_true", required=True)
    args = parser.parse_args()
    base = args.destination.resolve()
    base.mkdir(parents=True, exist_ok=True)
    runtime = base / "runtime"
    if (base / "manifest.json").exists():
        raise ValueError("Preserve completed runtimes. Use a new destination.")
    if runtime.exists():
        if not args.resume:
            raise ValueError("Use --resume to preserve the incomplete staging directory and try again.")
        retained = base / f"runtime-incomplete-{time.time_ns()}"
        if not runtime.resolve().is_relative_to(base) or not retained.resolve().is_relative_to(base):
            raise ValueError("Runtime staging path escaped its explicit destination")
        runtime.rename(retained)
    report = base / "resolution.json"
    pins = []
    for line in pathlib.Path("tools/computation-spike/requirements.txt").read_text().splitlines():
        if line and not line.startswith("#"):
            pins.append(line.split()[0])
    if not report.exists() or not args.resume:
        subprocess.run([sys.executable, "-m", "pip", "install", "--dry-run", "--ignore-installed",
        "--report", str(report), "--platform", "win_amd64", "--python-version", "3.14",
        "--implementation", "cp", "--abi", "cp314", "--only-binary=:all:", *pins], check=True)
    resolution = json.loads(report.read_text(encoding="utf-8"))
    archives = [{"name": "python", "version": VERSION, "url": URL, "sha256": SHA, "target": "."}]
    for item in resolution["install"]:
        archives.append({"name": item["metadata"]["name"], "version": item["metadata"]["version"],
            "url": item["download_info"]["url"], "sha256": item["download_info"]["archive_info"]["hashes"]["sha256"],
            "target": "site-packages"})
    runtime.mkdir()
    total = 0
    for archive in archives:
        filename = archive["url"].rsplit("/", 1)[1]
        destination = base / filename
        print(f"Verifying {archive['name']} {archive['version']}", flush=True)
        fetch(archive["url"], destination, archive["sha256"])
        archive["bytes"] = destination.stat().st_size
        with zipfile.ZipFile(destination) as bundle:
            for entry in bundle.infolist():
                if entry.is_dir():
                    continue
                name = entry.filename
                if ".data/" in name:
                    prefix, name = name.split(".data/", 1)
                    category, _, name = name.partition("/")
                    if category not in ("purelib", "platlib"):
                        continue
                target = (runtime / archive["target"] / name).resolve()
                if not target.is_relative_to(runtime):
                    raise ValueError("Unsafe archive path")
                total += entry.file_size
                if total > 768 * 1024 * 1024:
                    raise ValueError("Extraction exceeds the limit")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(bundle.read(entry))
    (runtime / "python314._pth").write_text("python314.zip\n.\nsite-packages\n", encoding="utf-8")
    manifest = {"id": "python-3.14.10-science-1", "python": VERSION, "archives": archives}
    (base / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(f"Prepared {len(archives)} pinned archives, {total} extracted bytes at {runtime}")

if __name__ == "__main__":
    main()
