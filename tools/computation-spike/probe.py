"""Runs only inside the experimental AppContainer, with synthetic inputs."""
import contextlib
import ctypes
import importlib.metadata
import io
import json
import math
import os
import pathlib
import socket
import winreg
import subprocess
import sys
import time

CONFIG = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
BASE = pathlib.Path(CONFIG["base"])
MODE = sys.argv[2]
sys.stdout = open(BASE / f"output/{MODE}.log", "w", encoding="utf-8", buffering=1)
sys.stderr = sys.stdout


def denied(name, operation, *, quota=False):
    try:
        operation()
    except OSError as error:
        assert (error.winerror in (5, 10013, 1260) or error.errno == 13
                or (quota and error.winerror == 1816)), (name, repr(error))
        print(json.dumps({"test": name, "denied": True, "error": str(error)}), flush=True)
        return
    raise AssertionError(f"Forbidden operation succeeded: {name}")


def isolation():
    assert (BASE / "input/value.txt").read_text() == "approved input"
    output = BASE / "output/approved.txt"
    output.write_text("derived output")
    assert output.read_text() == "derived output"
    denied("overwrite approved input", lambda: (BASE / "input/value.txt").write_text("changed"))
    denied("modify runtime", lambda: (pathlib.Path(CONFIG["runtime"]) / "python312._pth").write_text("changed"))
    for path in CONFIG["canaries"]:
        denied(f"read {pathlib.Path(path).parent.name}", lambda p=path: pathlib.Path(p).read_text())
        denied(f"write {pathlib.Path(path).parent.name}", lambda p=path: pathlib.Path(p).write_text("changed"))
    assert "GOODTYPE_SPIKE_SECRET" not in os.environ
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, CONFIG["registry_canary"]) as key:
            winreg.QueryValueEx(key, "canary")
    except OSError as error:
        assert error.winerror in (2, 5), repr(error)
    else:
        raise AssertionError("Private host registry canary is readable")
    # Use native APIs as well. Removing Python functions would not establish an OS boundary.
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p,
                                 ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
    kernel.CreateFileW.restype = ctypes.c_void_p
    handle = kernel.CreateFileW(CONFIG["canaries"][0], 0x80000000, 1, None, 3, 0, None)
    assert handle == ctypes.c_void_p(-1).value, "Native file access escaped restriction"
    assert ctypes.get_last_error() == 5
    kernel.OpenProcess.restype = ctypes.c_void_p
    handle = kernel.OpenProcess(0x10, False, CONFIG["parent_pid"])
    assert not handle and ctypes.get_last_error() == 5, "Parent process memory accessible"
    denied("spawn Python", lambda: subprocess.run([sys.executable, "-c", "print('escape')"], check=True), quota=True)
    denied("break away from Job Object", lambda: subprocess.run(
        [sys.executable, "-c", "print('escape')"], creationflags=0x01000000, check=True), quota=True)
    token = ctypes.c_void_p()
    kernel.GetCurrentProcess.restype = ctypes.c_void_p
    advapi = ctypes.WinDLL("advapi32", use_last_error=True)
    advapi.OpenProcessToken.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.POINTER(ctypes.c_void_p)]
    assert advapi.OpenProcessToken(kernel.GetCurrentProcess(), 8, ctypes.byref(token))
    advapi.GetTokenInformation.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p,
                                         ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32)]
    size, is_container = ctypes.c_uint32(), ctypes.c_uint32()
    assert advapi.GetTokenInformation(token, 29, ctypes.byref(is_container), 4, ctypes.byref(size))
    assert is_container.value == 1
    advapi.GetTokenInformation(token, 30, None, 0, ctypes.byref(size))
    groups = ctypes.create_string_buffer(size.value)
    assert advapi.GetTokenInformation(token, 30, groups, size.value, ctypes.byref(size))
    assert ctypes.c_uint32.from_buffer(groups).value == 1, "Unexpected capability count"
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    advapi.ConvertStringSidToSidW.argtypes = [ctypes.c_wchar_p, ctypes.POINTER(ctypes.c_void_p)]
    kernelbase = ctypes.WinDLL("kernelbase", use_last_error=True)
    kernelbase.CheckTokenCapability.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_int)]
    kernel.LocalFree.argtypes = [ctypes.c_void_p]
    for capability in ("S-1-15-3-1", "S-1-15-3-2", "S-1-15-3-3"):
        sid, granted = ctypes.c_void_p(), ctypes.c_int()
        assert advapi.ConvertStringSidToSidW(capability, ctypes.byref(sid))
        assert kernelbase.CheckTokenCapability(None, sid, ctypes.byref(granted))
        kernel.LocalFree(sid)
        assert not granted.value, "Network capability present"
    kernel.CloseHandle(token)
    try:
        with socket.socket() as connection:
            connection.settimeout(1)
            connection.connect(("127.0.0.1", CONFIG["canary_port"]))
    except OSError as error:
        # Windows filtering can drop packets instead of returning WSAEACCES.
        # The host proves the listener is reachable and separately observes zero connections.
        assert isinstance(error, TimeoutError) or error.winerror == 10013, repr(error)
        print(json.dumps({"test": "live loopback canary", "connected": False,
                          "result": type(error).__name__}), flush=True)
    else:
        raise AssertionError("Sandbox connected to the host's live canary")
    print("isolation checks passed", flush=True)


def science():
    started = time.perf_counter()
    import numpy as np
    import scipy.linalg
    import sympy as sp
    import pandas as pd
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    x = np.linspace(0, 2 * np.pi, 200)
    assert np.allclose(scipy.linalg.solve([[2, 0], [0, 4]], [6, 8]), [3, 2])
    t = sp.symbols("t")
    assert sp.integrate(t ** 2, (t, 0, 3)) == 9
    assert pd.DataFrame({"value": [1, 2, 3]}).value.sum() == 6
    figure, axes = plt.subplots()
    axes.plot(x, np.sin(x))
    axes.set(xlabel="x", ylabel="sin(x)")
    figure.savefig(BASE / "output/sine.png")
    figure.savefig(BASE / "output/sine.svg")
    plt.close(figure)
    versions = {name: importlib.metadata.version(name) for name in
                ["numpy", "scipy", "sympy", "pandas", "matplotlib", "ipython"]}
    print(json.dumps({"versions": versions, "seconds": time.perf_counter() - started}), flush=True)


def session():
    from IPython.core.interactiveshell import InteractiveShell
    shell = InteractiveShell(user_ns={})
    captured = io.StringIO()
    with contextlib.redirect_stdout(captured), contextlib.redirect_stderr(captured):
        assert shell.run_cell("x = 2", store_history=True).success
        assert shell.run_cell("y = x * 3", store_history=True).success
        assert shell.user_ns["y"] == 6
        assert shell.run_cell("x = 99\nraise ValueError('test')", store_history=True).error_in_exec
        assert shell.user_ns["x"] == 99
    assert captured.getvalue(), "No traceback output"
    print(json.dumps({"shared_cells": True, "partial_mutation": True,
                      "execution_count": shell.execution_count}), flush=True)


if MODE == "isolation":
    isolation()
elif MODE == "science":
    science()
elif MODE == "session":
    session()
elif MODE == "loop":
    while True:
        pass
elif MODE == "memory":
    try:
        oversized = bytearray(1024 * 1024 * 1024 + 1)
    except MemoryError:
        print("one-GiB process memory limit enforced", flush=True)
    else:
        raise AssertionError("Process allocation exceeded the configured memory limit")
else:
    raise ValueError("Unknown probe mode")
