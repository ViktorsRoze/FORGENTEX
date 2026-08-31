#!/usr/bin/env python
"""Minimal DAP client to exercise `fgx dap` end to end."""
import json
import subprocess
import sys

FGX = sys.argv[1]
PROGRAM = sys.argv[2].replace("\\", "/")

proc = subprocess.Popen(
    [FGX, "dap"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)

seq = 0


def send(command, arguments=None):
    global seq
    seq += 1
    msg = {"seq": seq, "type": "request", "command": command}
    if arguments is not None:
        msg["arguments"] = arguments
    body = json.dumps(msg)
    proc.stdin.write(f"Content-Length: {len(body)}\r\n\r\n{body}".encode())
    proc.stdin.flush()


def read():
    length = None
    while True:
        line = proc.stdout.readline()
        if not line:
            return None
        line = line.decode().strip()
        if not line:
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(proc.stdout.read(length).decode())


def pump_until(pred, limit=60):
    """Read messages until one satisfies pred; return it and the log."""
    seen = []
    for _ in range(limit):
        msg = read()
        if msg is None:
            return None, seen
        seen.append(msg)
        if pred(msg):
            return msg, seen
    return None, seen


log = []

send("initialize", {"adapterID": "forgentex"})
m, s = pump_until(lambda m: m.get("type") == "event" and m.get("event") == "initialized")
log += s
assert m, "no initialized event"

# breakpoint on the `return b` line (line 8)
send("setBreakpoints", {"source": {"path": PROGRAM}, "breakpoints": [{"line": 8}]})
send("launch", {"program": PROGRAM, "entry": "run"})
send("setBreakpoints", {"source": {"path": PROGRAM}, "breakpoints": [{"line": 8}]})
send("configurationDone")

m, s = pump_until(lambda m: m.get("type") == "event" and m.get("event") == "stopped")
log += s
assert m, f"no stopped event; saw {[x.get('event') or x.get('command') for x in log]}"

send("threads")
m, s = pump_until(lambda m: m.get("command") == "threads")
log += s
assert m["body"]["threads"][0]["name"] == "forgentex", m

send("stackTrace", {"threadId": 1})
m, s = pump_until(lambda m: m.get("command") == "stackTrace")
log += s
frame = m["body"]["stackFrames"][0]
assert frame["name"] == "run", frame
assert frame["line"] == 6, f"expected first stop on line 6, got {frame}"

send("scopes", {"frameId": 1})
m, s = pump_until(lambda m: m.get("command") == "scopes")
log += s
assert m["body"]["scopes"][0]["name"] == "Locals", m

# step into helper
send("stepIn", {"threadId": 1})
m, s = pump_until(lambda m: m.get("type") == "event" and m.get("event") == "stopped")
log += s
send("stepIn", {"threadId": 1})
m, s = pump_until(lambda m: m.get("type") == "event" and m.get("event") == "stopped")
log += s

send("stackTrace", {"threadId": 1})
m, s = pump_until(lambda m: m.get("command") == "stackTrace")
log += s
frame = m["body"]["stackFrames"][0]
assert frame["name"] == "helper", f"step-in should reach helper: {frame}"

send("variables", {"variablesReference": 1})
m, s = pump_until(lambda m: m.get("command") == "variables")
log += s
names = {v["name"]: v["value"] for v in m["body"]["variables"]}
assert names.get("x") == "21", names

# continue: the breakpoint on line 8 must be hit
send("continue", {"threadId": 1})
m, s = pump_until(lambda m: m.get("type") == "event" and m.get("event") == "stopped")
log += s
assert m, "breakpoint on line 8 was never hit"
send("stackTrace", {"threadId": 1})
m, s = pump_until(lambda m: m.get("command") == "stackTrace")
log += s
frame = m["body"]["stackFrames"][0]
assert frame["line"] == 8, f"expected breakpoint stop on line 8, got {frame}"

send("continue", {"threadId": 1})
m, s = pump_until(lambda m: m.get("type") == "event" and m.get("event") == "terminated")
log += s
assert m, "no terminated event"

proc.stdin.close()
proc.wait(timeout=30)
print("DAP session OK: initialize, launch, breakpoints, stepIn, variables, continue, terminate")
