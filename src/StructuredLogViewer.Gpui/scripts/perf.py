#!/usr/bin/env python3
"""Render-cost scenarios for the viewer, over its --automation channel.

    scripts/perf.py <file.binlog> [app args...]
    scripts/perf.py big.binlog --source /path/Microsoft.Common.CurrentVersion.targets --line 3000

Builds nothing: runs target/release/structured-log-viewer-gpui (or
$VIEWER_BIN). Each scenario prints the process CPU it took and, per view,
how many times it rendered and how long that took (`perf` command; list
processors are `View.rows`/`.lines`, with the rows they built). The point
is the counts: hovering the tree should re-render the tree, not the search
pane and the source well beside it.

Element lookups (probes/bounds) force a full-window refresh, which
re-renders every view, so each scenario resolves its targets first, resets
the counters, and then drives the mouse by window coordinates only, one
input per frame. Keep the display awake (`caffeinate -d`): a sleeping or
occluded window draws no frames and the lookups find nothing. (The script
brings the viewer to the front itself.)
"""
import json
import os
import subprocess
import sys

CRATE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.environ.get("VIEWER_BIN", os.path.join(CRATE, "target", "release", "structured-log-viewer-gpui"))


class App:
    def __init__(self, bin, args):
        self.p = subprocess.Popen([bin, *args, "--automation"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1)

    def send(self, cmd):
        self.p.stdin.write(json.dumps(cmd) + "\n")
        self.p.stdin.flush()
        r = json.loads(self.p.stdout.readline())
        if not r.get("ok"):
            raise RuntimeError(f"{cmd}: {r}")
        return r.get("result")

    def sleep(self, ms):
        self.send({"cmd": "sleep", "ms": ms})

    def bring_to_front(self):
        # An occluded window draws no frames: nothing would be measured, and
        # nothing laid out for the lookups to find.
        script = f'tell application "System Events" to set frontmost of (first process whose unix id is {self.p.pid}) to true'
        subprocess.run(["osascript", "-e", script], capture_output=True)

    def wait_loaded(self):
        for _ in range(600):
            d = self.send({"cmd": "dump"})
            if d and d.get("phase") == "loaded" and d["tree"]["rowCount"] > 1:
                return d
            self.sleep(200)
        raise RuntimeError("never loaded")

    def centers(self, prefix, limit):
        for _ in range(30):
            ids = [p for p in self.send({"cmd": "probes"}) if p.startswith(prefix)]
            if ids:
                break
            self.sleep(200)
        ids.sort(key=lambda p: int(p.rsplit("-", 1)[-1]))
        out = []
        for i in ids[:limit]:
            b = self.send({"cmd": "bounds", "id": i})
            out.append((b["x"] + min(b["width"] / 2, 120), b["y"] + b["height"] / 2))
        return out

    def move(self, x, y):
        self.send({"cmd": "move", "x": x, "y": y})
        self.sleep(20)  # let each input land in its own frame

    def scroll(self, x, y, dy):
        self.send({"cmd": "scroll", "x": x, "y": y, "dy": dy})
        self.sleep(20)

    def cpu(self):
        t = subprocess.run(["ps", "-o", "time=", "-p", str(self.p.pid)], capture_output=True, text=True).stdout.strip()
        m, s = t.split(":")
        return float(m) * 60 + float(s)

    def start(self):
        self.sleep(300)
        self.send({"cmd": "perf"})
        self._cpu = self.cpu()

    def report(self, label):
        self.sleep(250)
        perf = self.send({"cmd": "perf"})
        used = (self.cpu() - self._cpu) * 1000
        parts = [f"{k}={v['calls']}x/{v['ms']}ms" + (f"/{v['items']}it" if "items" in v else "") for k, v in sorted(perf.items())]
        print(f"  {label:<34} cpu={used:4.0f}ms  " + "  ".join(parts), flush=True)

    def quit(self):
        try:
            self.p.stdin.write(json.dumps({"cmd": "quit"}) + "\n")
            self.p.stdin.flush()
            self.p.wait(5)
        except Exception:
            self.p.kill()



def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    binlog, *extra = sys.argv[1:]
    app = App(BIN, [binlog, *extra])
    has_source = "--source" in extra
    app.wait_loaded()
    app.bring_to_front()
    app.sleep(3000)
    # Expand rows so there is a tree to work in.
    for attempt in range(20):
        try:
            app.send({"cmd": "click", "id": "tree-row-0"})
            break
        except RuntimeError:
            app.sleep(300)
    for _ in range(25):
        app.send({"cmd": "keys", "keys": "right down"})
    app.sleep(800)

    # S1: hover down 25 tree rows.
    rows = app.centers("tree-row-", 25)
    app.start()
    for _ in range(4):
        for x, y in rows:
            app.move(x, y)
    app.report("hover tree rows x100")

    # S2: arrow down 25 rows (selection + inspector fetch).
    app.start()
    for _ in range(25):
        app.send({"cmd": "keys", "keys": "down"})
        app.sleep(20)
    app.report("arrow down 25 rows")

    # S3: wheel-scroll the tree.
    x, y = rows[3]
    app.start()
    for i in range(40):
        app.scroll(x, y, -40 if i < 20 else 40)
    app.report("scroll tree x40")

    if has_source:
        editor = app.centers("line-", 40)
        ex, ey = editor[len(editor) // 2]
        app.start()
        for i in range(40):
            app.scroll(ex, ey, -54 if i < 20 else 54)
        app.report("scroll source x40")

        lines = app.centers("line-", 40)
        app.start()
        for _ in range(3):
            for lx, ly in lines:
                app.move(lx + 40, ly)
        app.report("hover source lines x120")

        rows = app.centers("tree-row-", 25)
        app.start()
        for _ in range(4):
            for rx, ry in rows:
                app.move(rx, ry)
        app.report("hover tree rows x100 (source open)")

    # S4: type a search, one character at a time.
    app.send({"cmd": "keys", "keys": "cmd-f"})
    app.start()
    for ch in "Copying file":
        app.send({"cmd": "type", "text": ch})
        app.sleep(20)
    app.sleep(2500)
    app.report("type search 'Copying file'")

    # S5: hover search results.
    results = app.centers("search-row-", 15)
    app.start()
    for _ in range(4):
        for sx, sy in results:
            app.move(sx, sy)
    app.report(f"hover search rows x{4 * len(results)}")

    # S6: timeline pan.
    app.send({"cmd": "keys", "keys": "cmd-2"})
    app.sleep(3000)
    tl = app.send({"cmd": "bounds", "id": "divider-sidebar"})
    tx, ty = tl["x"] + 300, tl["y"] + 300
    app.start()
    for i in range(40):
        app.scroll(tx, ty, -30 if i < 20 else 30)
    app.report("scroll timeline x40")
    app.start()
    for i in range(60):
        app.move(tx + i * 7, ty + (i % 5) * 9)
    app.report("hover timeline x60")
    app.send({"cmd": "keys", "keys": "cmd-1"})

    # S7: idle.
    app.sleep(500)
    app.start()
    app.sleep(2000)
    app.report("idle 2s")
    app.quit()

    return 0


if __name__ == "__main__":
    sys.exit(main())
