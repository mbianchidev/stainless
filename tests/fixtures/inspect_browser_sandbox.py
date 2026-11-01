import argparse
import ctypes
import json
import subprocess


parser = argparse.ArgumentParser()
parser.add_argument("--browser-pid", type=int, required=True)
parser.add_argument("--profile", required=True)
args = parser.parse_args()
if not 0 < args.browser_pid < 2**31:
    raise ValueError("Invalid owned browser PID")
browser = subprocess.check_output(
    ["/bin/ps", "-p", str(args.browser_pid), "-o", "command="], text=True
)
if f"--user-data-dir={args.profile}" not in browser or "--headless" not in browser:
    raise RuntimeError("Refusing to inspect a browser outside the synthetic headless profile")
children = subprocess.check_output(
    ["/usr/bin/pgrep", "-P", str(args.browser_pid)], text=True
).splitlines()
sandbox = ctypes.CDLL("/usr/lib/libsandbox.1.dylib", use_errno=True)
sandbox.sandbox_check.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int]
sandbox.sandbox_check.restype = ctypes.c_int
renderers = []
for child in children:
    command = subprocess.check_output(
        ["/bin/ps", "-p", child, "-o", "command="], text=True
    )
    if "--type=renderer" in command:
        state = sandbox.sandbox_check(int(child), None, 0)
        if state != 1:
            raise RuntimeError(f"Owned renderer {child} is not sandboxed: {state}; errno {ctypes.get_errno()}")
        renderers.append(int(child))
if not renderers:
    raise RuntimeError("No owned renderer was available for native sandbox inspection")
print(json.dumps({"native_sandboxed_renderers": len(renderers), "method": "sandbox_check"}))
