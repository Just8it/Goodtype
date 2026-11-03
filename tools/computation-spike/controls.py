"""Run fixed scientific/session controls outside AppContainer. Never a sandbox fallback."""
import json
import os
import pathlib
import subprocess
import sys

directory = pathlib.Path(sys.argv[1]).resolve()
base = directory / "controls"
base.mkdir()
(base / "output").mkdir()
config = base / "config.json"
config.write_text(json.dumps({"base": str(base), "runtime": str(directory / "runtime")}), encoding="utf-8")
allowed = ["SYSTEMROOT", "WINDIR", "SYSTEMDRIVE", "LOCALAPPDATA", "APPDATA", "COMSPEC"]
environment = {key: value for key, value in os.environ.items() if key.upper() in allowed}
environment.update({"TEMP": str(base / "output"), "TMP": str(base / "output"),
                    "MPLCONFIGDIR": str(base / "output"), "IPYTHONDIR": str(base / "output"),
                    "USERPROFILE": str(base / "output"), "OPENBLAS_NUM_THREADS": "1"})
results = []
for mode in ["science", "session"]:
    result = subprocess.run([str(directory / "runtime/python.exe"), "-I", "-B",
                             str(pathlib.Path(__file__).with_name("probe.py")), str(config), mode],
                            cwd=base, env=environment, timeout=60, capture_output=True, text=True)
    results.append({"mode": mode, "exit_code": result.returncode, "stderr": result.stderr,
                    "log": (base / f"output/{mode}.log").read_text(encoding="utf-8")
                    if (base / f"output/{mode}.log").exists() else ""})
evidence = {"sandboxed": False, "fixed_test_code_only": True, "results": results}
(base / "evidence.json").write_text(json.dumps(evidence, indent=2), encoding="utf-8")
print(json.dumps(evidence, indent=2))
assert all(result["exit_code"] == 0 for result in results), "Trusted control failed"
