"""Explicit development-only acquisition. Does not alter the application's dependencies."""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import urllib.request
import zipfile

URL = "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip"
PYTHON_SHA256 = "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=pathlib.Path)
    parser.add_argument("--download", action="store_true", required=True)
    args = parser.parse_args()
    if sys.version_info[:2] != (3, 12):
        parser.error("Use Python 3.12 to select wheels for this experimental interpreter.")
    directory = args.directory.resolve()
    if (directory / "runtime-manifest.json").exists():
        raise ValueError("Use a fresh directory. Preserve the previous experiment runtime and evidence.")
    directory.mkdir(parents=True, exist_ok=True)
    runtime = directory / "runtime"
    runtime.mkdir(exist_ok=True)
    archive = directory / "python-3.12.10-embed-amd64.zip"
    if not archive.exists():
        with urllib.request.urlopen(URL, timeout=60) as response:
            archive.write_bytes(response.read(32 * 1024 * 1024 + 1))
    if archive.stat().st_size > 32 * 1024 * 1024:
        raise ValueError("Runtime archive exceeds the experimental download budget.")
    if hashlib.sha256(archive.read_bytes()).hexdigest() != PYTHON_SHA256:
        raise ValueError("Runtime archive does not match the experiment's pinned hash.")
    with zipfile.ZipFile(archive) as bundle:
        if sum(entry.file_size for entry in bundle.infolist()) > 128 * 1024 * 1024:
            raise ValueError("Runtime extraction exceeds the experimental size budget.")
        for entry in bundle.infolist():
            target = (runtime / entry.filename).resolve()
            if not target.is_relative_to(runtime):
                raise ValueError("Unsafe archive path")
        bundle.extractall(runtime)
    # Explicit search paths. Do not process third-party .pth startup code with import site.
    (runtime / "python312._pth").write_text(
        "python312.zip\n.\nsite-packages\n", encoding="utf-8"
    )
    report = directory / "acquisition.json"
    subprocess.run(
        [sys.executable, "-m", "pip", "--isolated", "install", "--disable-pip-version-check",
         "--only-binary=:all:", "--no-compile", "--ignore-installed", "--upgrade",
         "--index-url", "https://pypi.org/simple", "--target", str(runtime / "site-packages"),
         "--require-hashes", "--report", str(report), "-r",
         str(pathlib.Path(__file__).with_name("requirements.txt"))],
        check=True,
    )
    acquisition = json.loads(report.read_text(encoding="utf-8"))
    manifest = {
        "experimental_only": True,
        "python_url": URL,
        "python_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        "python_bytes": archive.stat().st_size,
        "packages": [{"name": item["metadata"]["name"], "version": item["metadata"]["version"],
                      "source": item["download_info"]} for item in acquisition["install"]],
    }
    (directory / "runtime-manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print(f"Prepared experimental runtime at {runtime}")


if __name__ == "__main__":
    main()
