"""Bundled protocol worker. Windows enforces permissions outside this interpreter."""
import base64
import ast
import builtins
import contextlib
import io
import json
import os
import pathlib
import re
import sys
import time
import traceback
import types

INPUT = pathlib.Path(sys.argv[1])
SCRATCH = pathlib.Path.cwd()
MAX_TEXT = 1024 * 1024
MAX_IMAGES = 10 * 1024 * 1024
MAX_RESPONSE = 16 * 1024 * 1024

class BoundedText(io.TextIOBase):
    def __init__(self):
        self.parts = []
        self.size = 0
        self.shortened = False
    def write(self, value):
        text = str(value)
        data = text.encode("utf-8", errors="replace")
        remaining = max(0, MAX_TEXT - self.size)
        if remaining and data:
            self.parts.append(data[:remaining].decode("utf-8", errors="ignore"))
        self.size += min(len(data), remaining)
        self.shortened |= len(data) > remaining
        return len(text)
    def flush(self):
        pass
    def text(self):
        return "".join(self.parts)

def response(name, value):
    data = json.dumps(value, ensure_ascii=False).encode("utf-8")
    if len(data) > MAX_RESPONSE:
        raise ValueError("Response exceeds the protocol limit")
    temporary = SCRATCH / (name + ".tmp")
    temporary.write_bytes(data)
    temporary.replace(SCRATCH / name)

def _unsupported_input(prompt=None):
    raise RuntimeError("Interactive input() is not supported in computational pages.")
builtins.input = _unsupported_input

# AppContainer LPAC disallows dynamic executable trampolines (libffi in _ctypes).
# Provide a safe fallback stub for console title setting and process utilities if _ctypes fails to load.
try:
    import ctypes
except Exception:
    dummy_ctypes = types.ModuleType("ctypes")
    dummy_wintypes = types.ModuleType("ctypes.wintypes")
    class DummyType:
        def __init__(self, *args, **kwargs):
            pass
        def __call__(self, *args, **kwargs):
            return self
    dummy_ctypes.POINTER = lambda x: DummyType
    dummy_ctypes.c_int = DummyType
    dummy_ctypes.c_wchar_p = DummyType
    dummy_ctypes.c_void_p = DummyType
    dummy_ctypes.c_char_p = DummyType
    dummy_ctypes.Structure = DummyType
    dummy_ctypes.byref = lambda x: x
    dummy_wintypes.HLOCAL = DummyType
    dummy_wintypes.LPCWSTR = DummyType
    class DummyFunc:
        def __init__(self):
            self.argtypes = []
            self.restype = None
        def __call__(self, *args, **kwargs):
            return None
    class DummyDll:
        def __getattr__(self, name):
            return DummyFunc()
    class DummyWindll:
        def __getattr__(self, name):
            return DummyDll()
    dummy_ctypes.windll = DummyWindll()
    dummy_ctypes.cdll = DummyWindll()
    sys.modules["ctypes"] = dummy_ctypes
    sys.modules["ctypes.wintypes"] = dummy_wintypes

# In headless AppContainer LPAC, matplotlib's Win32 GUI helper C extension (_c_internal_utils)
# may fail to initialize. Provide a safe headless fallback so Agg rendering proceeds cleanly.
dummy_c_utils = types.ModuleType("matplotlib._c_internal_utils")
dummy_c_utils.display_is_valid = lambda: False
dummy_c_utils.xdisplay_is_valid = lambda: False
dummy_c_utils.Win32_GetCurrentProcessExplicitAppUserModelID = lambda: None
dummy_c_utils.Win32_SetCurrentProcessExplicitAppUserModelID = lambda appid: None
dummy_c_utils.Win32_GetForegroundWindow = lambda: 0
dummy_c_utils.Win32_SetForegroundWindow = lambda hwnd: None
dummy_c_utils.Win32_SetProcessDpiAwareness_max = lambda: None
try:
    import matplotlib._c_internal_utils
except Exception:
    sys.modules["matplotlib._c_internal_utils"] = dummy_c_utils

# Import diagnostics cannot grow a log file without a limit.
startup = BoundedText()
sys.stdout = startup
sys.stderr = startup
os.environ["MPLBACKEND"] = "Agg"
# Figures are captured after every cell, as in Jupyter's inline mode, so plt.show() has nothing
# to do. Agg would otherwise print "FigureCanvasAgg is non-interactive" into every plot's output.
import warnings
warnings.filterwarnings("ignore", message=r".*is non-interactive, and thus cannot be shown", category=UserWarning)
try:
    shell = None
    try:
        from IPython.core.interactiveshell import InteractiveShell
        from traitlets.config import Config
        config = Config()
        config.HistoryManager.hist_file = ":memory:"
        shell = InteractiveShell.instance(config=config)
        # As in Jupyter, a failure's traceback belongs to the error output, not to printed text;
        # otherwise every error appears twice. IPython calls this hook for runtime and syntax errors.
        shown_traceback = [None]
        shell._showtraceback = lambda etype, evalue, stb: shown_traceback.__setitem__(0, stb)
    except ImportError:
        # Plain Python needs no downloaded notebook engine. Normal imports use this staged
        # environment; a library that is absent raises its ordinary import error in the cell.
        pass
    namespace = {"__name__": "__main__", "__builtins__": builtins}
    execution_count = 0
    import importlib.metadata
    versions = {}
    for name in ("IPython", "numpy", "scipy", "sympy", "pandas", "matplotlib"):
        try:
            versions[name] = importlib.metadata.version(name)
        except Exception:
            pass
    response("ready.json", {"protocol": 1, "python": sys.version.split()[0], "packages": versions,
                            "engine": "ipython" if shell is not None else "python"})
except BaseException:
    response("ready.json", {"protocol": 1, "error": traceback.format_exc()[-8192:]})
    raise SystemExit(1)

last_request = None
while True:
    try:
        path = INPUT / "request.json"
        if not path.exists():
            time.sleep(0.02)
            continue
        with path.open("rb") as request_file:
            data = request_file.read(2 * 1024 * 1024 + 1)
        if len(data) > 2 * 1024 * 1024:
            raise ValueError("Request exceeds the protocol limit")
        request = json.loads(data)
        identity = request["requestId"]
        if identity == last_request:
            time.sleep(0.02)
            continue
        last_request = identity
        source = request["source"]
        if not isinstance(source, str) or len(source.encode("utf-8")) > MAX_TEXT:
            raise ValueError("Cell source exceeds the protocol limit")
        outputs = []
        image_size = [0]
        text_size = [0]
        shortened = [False]
        def publish(data, metadata=None, **kwargs):
            allowed = {}
            plain = data.get("text/plain")
            if isinstance(plain, str):
                remaining = max(0, MAX_TEXT - text_size[0])
                encoded = plain.encode("utf-8", errors="replace")
                allowed["text/plain"] = encoded[:remaining].decode("utf-8", errors="ignore")
                text_size[0] += min(len(encoded), remaining)
                shortened[0] |= len(encoded) > remaining
            png = data.get("image/png")
            if isinstance(png, bytes):
                png = base64.b64encode(png).decode("ascii")
            if isinstance(png, str):
                if len(png) > (MAX_IMAGES - image_size[0]) * 4 // 3 + 4:
                    shortened[0] = True
                    png = ""
                decoded = base64.b64decode(png, validate=True)
                if image_size[0] + len(decoded) <= MAX_IMAGES:
                    allowed["image/png"] = png
                    image_size[0] += len(decoded)
                else:
                    shortened[0] = True
            if allowed and len(outputs) < 100:
                outputs.append({"output_type": "display_data", "data": allowed, "metadata": {}})
            else:
                shortened[0] = True
        if shell is not None:
            shell.display_pub.publish = publish
            # DisplayHook routes the final expression through the same static MIME admission.
            shell.displayhook.write_format_data = lambda data, md=None: publish(data, md)
            # Jupyter front ends draw "Out[n]:" themselves; printed, it lands in the output text.
            shell.displayhook.write_output_prompt = lambda: None
        captured = BoundedText()
        error = None
        with contextlib.redirect_stdout(captured), contextlib.redirect_stderr(captured):
            if shell is not None:
                shown_traceback[0] = None
                result = shell.run_cell(source, store_history=True)
                error = result.error_before_exec or result.error_in_exec
                execution_count = shell.execution_count - 1
            else:
                execution_count += 1
                try:
                    tree = ast.parse(source, filename="<cell>", mode="exec")
                    expression = tree.body.pop() if tree.body and isinstance(tree.body[-1], ast.Expr) else None
                    exec(compile(tree, "<cell>", "exec"), namespace)
                    if expression is not None:
                        value = eval(compile(ast.Expression(expression.value), "<cell>", "eval"), namespace)
                        if value is not None:
                            publish({"text/plain": repr(value)})
                except BaseException as caught:
                    error = caught
            if "matplotlib.pyplot" in sys.modules:
                plt = sys.modules["matplotlib.pyplot"]
                try:
                    for number in plt.get_fignums():
                        buffer = io.BytesIO()
                        plt.figure(number).savefig(buffer, format="png", dpi=120, bbox_inches="tight")
                        publish({"image/png": base64.b64encode(buffer.getvalue()).decode("ascii")})
                finally:
                    plt.close("all")
        if captured.text():
            outputs.insert(0, {"output_type": "stream", "name": "stdout", "text": re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", captured.text())})
        if error:
            lines = shown_traceback[0] if shell is not None and isinstance(shown_traceback[0], list) else []
            lines = [str(line)[:8192] for line in lines[-40:]]
            outputs.append({"output_type": "error", "ename": type(error).__name__, "evalue": str(error)[:8192], "traceback": lines})
        if shortened[0] or captured.shortened:
            outputs.append({"output_type": "stream", "name": "stderr", "text": request["shortenedMessage"]})
        response("response.json", {"protocol": 1, "requestId": identity, "cellId": request["cellId"],
                 "sequence": execution_count, "outputs": outputs})
    except BaseException:
        response("response.json", {"protocol": 1, "requestId": last_request, "fatal": traceback.format_exc()[-8192:]})
        raise SystemExit(1)
