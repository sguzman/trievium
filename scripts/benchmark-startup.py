#!/usr/bin/env python3
"""Measure cold-process invocation to compositor window mapping, on one Wayland session.

This is a CI/local comparative proxy, not the user's physical first-pixel time.
Uses identical fonts/compositor/config for both builds and alternates order.
"""
import json
import os
import pathlib
import signal
import statistics
import subprocess
import sys
import tempfile
import time

def run(command, **kwargs):
    return subprocess.run(command, check=True, **kwargs)

def main():
    if len(sys.argv) != 3:
        sys.exit("usage: benchmark-startup.py /old/trievium /new/trievium")
    previous, updated = [str(pathlib.Path(x).resolve()) for x in sys.argv[1:]]
    with tempfile.TemporaryDirectory(prefix="trievium-launch-bench-") as temp:
        root = pathlib.Path(temp)
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700)
        (runtime / "sway.conf").write_text("output * resolution 1280x720\ndefault_border none\nfocus_follows_mouse no\n")
        cfg = root / "items.json"
        cfg.write_text(json.dumps({
            "version": 1,
            "entries": [
                {"label": "🚀 Project init", "key": "D", "text": "project init"},
                {"label": "🔁 Chat broke", "key": "L", "text": "chat broke"},
                {"label": "🧩 Restore context", "key": "S", "text": "restore"},
                {"label": "📊 Status", "key": "K", "text": "status"},
                {"label": "🔧 Fix issue", "key": "F", "text": "fix"},
            ],
        }))
        env = os.environ.copy()
        env.update({
            "XDG_RUNTIME_DIR": str(runtime),
            "WLR_BACKENDS": "headless",
            "WLR_HEADLESS_OUTPUTS": "1",
            "WLR_LIBINPUT_NO_DEVICES": "1",
            "WLR_RENDERER": "pixman",
            "WLR_RENDERER_ALLOW_SOFTWARE": "1",
            "LIBGL_ALWAYS_SOFTWARE": "1",
            "XDG_CURRENT_DESKTOP": "sway",
            "XDG_STATE_HOME": str(root / "state"),
        })
        env.pop("DISPLAY", None)
        swaylog = (root / "sway.log").open("w")
        sway = subprocess.Popen(
            ["sway", "--unsupported-gpu", "-c", str(runtime / "sway.conf")],
            env=env, stdout=swaylog, stderr=subprocess.STDOUT,
        )
        keyboard = None
        try:
            socket = None
            ipc = None
            for _ in range(200):
                sockets = list(runtime.glob("wayland-*"))
                ipcs = list(runtime.glob("sway-ipc.*.sock"))
                if sockets and ipcs:
                    socket = sockets[0].name
                    ipc = str(ipcs[0])
                    break
                time.sleep(0.05)
            if not socket:
                raise RuntimeError("nested Sway did not start")
            env["WAYLAND_DISPLAY"] = socket
            keyboard = subprocess.Popen(
                ["wtype", "-s", "999999999", "-k", "F15", "-k", "F15"],
                env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            time.sleep(0.3)

            def mapped():
                result = run(
                    ["swaymsg", "-s", ipc, "-t", "get_tree"],
                    capture_output=True, text=True,
                )
                return any(node.get("app_id") == "trievium" for node in walk(json.loads(result.stdout)))

            def once(path):
                proc = None
                started = time.perf_counter()
                try:
                    proc = subprocess.Popen(
                        [path, "--config", str(cfg)],
                        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                    )
                    for _ in range(400):
                        if mapped():
                            return (time.perf_counter() - started) * 1000
                        if proc.poll() is not None:
                            raise RuntimeError(f"{path} exited early: {proc.stderr.read().decode(errors='replace')}")
                        time.sleep(0.005)
                    raise RuntimeError(f"{path} did not map in 400 polls")
                finally:
                    if proc:
                        proc.terminate()
                        try:
                            proc.communicate(timeout=3)
                        except subprocess.TimeoutExpired:
                            proc.kill()
                            proc.communicate()
                    # Give Sway a moment to remove the old tree entry.
                    for _ in range(100):
                        if not mapped():
                            break
                        time.sleep(0.01)

            series = {"before": [], "after": []}
            for round_no in range(8):
                order = [("before", previous), ("after", updated)]
                if round_no % 2:
                    order.reverse()
                for label, path in order:
                    result = once(path)
                    if round_no:
                        series[label].append(result)
                    print(f"{label} round={round_no}: {result:.1f} ms", flush=True)

            old = statistics.median(series["before"])
            new = statistics.median(series["after"])
            print(f"Mapped-window median (7 warm rounds): before {old:.1f} ms, after {new:.1f} ms, delta {new-old:+.1f} ms", flush=True)
            if new >= old:
                print("WARNING: no positive improvement on this runner. Actual desktop may differ.", flush=True)
        finally:
            if keyboard:
                keyboard.terminate()
                keyboard.wait(timeout=3)
            sway.terminate()
            sway.wait(timeout=3)
            swaylog.close()

def walk(node):
    yield node
    for sub in node.get("nodes", []) + node.get("floating_nodes", []):
        yield from walk(sub)

if __name__ == "__main__":
    main()
