#!/usr/bin/env python3
"""Drive the viewer through its --automation channel.

    scripts/drive.py /tmp/big.binlog --source /path/Sdk.props --line 50 -- \
        'keys cmd-f' 'type import' 'keys enter' dump 'screenshot /tmp/find.png'

Each positional step after `--` is either a bare word (dump, probes, quit),
`<cmd> <arg>` for keys/type/action/bounds/move/click/screenshot/sleep, or a
raw JSON object. Replies are printed one per line. With no steps, reads
steps from stdin. Exits non-zero if any step fails.
"""
import json
import os
import subprocess
import sys

CRATE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(CRATE, "target", "debug", "structured-log-viewer-gpui")


def to_command(step: str) -> dict:
    step = step.strip()
    if step.startswith("{"):
        return json.loads(step)
    head, _, rest = step.partition(" ")
    rest = rest.strip()
    if head in ("dump", "probes", "quit"):
        return {"cmd": head}
    if head == "keys":
        return {"cmd": "keys", "keys": rest}
    if head == "type":
        return {"cmd": "type", "text": rest}
    if head == "action":
        return {"cmd": "action", "name": rest}
    if head == "bounds":
        return {"cmd": "bounds", "id": rest}
    if head == "screenshot":
        return {"cmd": "screenshot", "path": rest}
    if head == "sleep":
        return {"cmd": "sleep", "ms": int(rest)}
    if head in ("click", "move", "scroll"):
        parts = rest.split()
        cmd = {"cmd": head}
        if parts and parts[0].replace(".", "", 1).replace("-", "", 1).isdigit():
            cmd["x"], cmd["y"] = float(parts[0]), float(parts[1])
            parts = parts[2:]
        else:
            cmd["id"] = parts[0]
            parts = parts[1:]
        if head == "scroll" and parts:
            cmd["dx"], cmd["dy"] = float(parts[0]), float(parts[1])
            # Optional trackpad phase: started / moved / ended.
            if len(parts) > 2:
                cmd["phase"] = parts[2]
        return cmd
    raise SystemExit(f"unknown step: {step!r}")


def main() -> int:
    argv = sys.argv[1:]
    if "--" in argv:
        split = argv.index("--")
        app_args, steps = argv[:split], argv[split + 1 :]
    else:
        app_args, steps = argv, []
    if not steps:
        steps = [line for line in sys.stdin.read().splitlines() if line.strip()]

    proc = subprocess.Popen(
        [BIN, *app_args, "--automation"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    # The first reply arrives once the window is up; give the log a moment
    # to load by making the first step wait for the workspace to be ready.
    failed = False
    try:
        for step in ["sleep 100", *steps]:
            command = to_command(step)
            proc.stdin.write(json.dumps(command) + "\n")
            proc.stdin.flush()
            if command["cmd"] == "quit":
                break
            reply = proc.stdout.readline()
            if not reply:
                print("viewer exited", file=sys.stderr)
                return 1
            if step != "sleep 100":
                print(reply.rstrip())
            if not json.loads(reply).get("ok", False):
                failed = True
    finally:
        try:
            proc.stdin.write(json.dumps({"cmd": "quit"}) + "\n")
            proc.stdin.flush()
        except (BrokenPipeError, ValueError):
            pass
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
