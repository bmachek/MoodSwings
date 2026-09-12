#!/usr/bin/env python3
"""Run native server + graphical client + a moving protocol peer; capture the result.

Build first: cargo build --workspace
Run: python3 tools/check-multiplayer.py
Requires a working GPU, like the normal capture harness. Uses only loopback TCP.
"""
import pathlib
import re
import socket
import subprocess
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT = ROOT / "shots" / "multiplayer-qa"
OUT.mkdir(parents=True, exist_ok=True)
SERVER = ROOT / "target" / "debug" / "mood-server"
CLIENT = ROOT / "target" / "debug" / "mood_swings"
SUFFIX = ".exe" if __import__("os").name == "nt" else ""
stop = threading.Event()
errors = []
positions_seen = []


def peer(address):
    try:
        with socket.create_connection(address, timeout=5) as conn:
            conn.settimeout(0.2)
            conn.sendall(b'Hello(version:1,name:"Netztest")\n')
            pending = b""
            anchor = None
            while not stop.is_set():
                try:
                    chunk = conn.recv(32768)
                    if not chunk:
                        return
                    pending += chunk
                except socket.timeout:
                    pass
                while b"\n" in pending:
                    line, pending = pending.split(b"\n", 1)
                    # Ignore our own replica to avoid feeding its offset back
                    # into itself. IDs also count health-check connections.
                    match = re.search(rb'name:"Grafiktest",pose:\(position:\(([^)]+)\)', line)
                    if match:
                        anchor = [float(x) for x in match[1].split(b",")]
                        positions_seen.append(anchor)
                if anchor:
                    x, y, z = anchor
                    x += 2.5
                    message = f'Update(Some((position:({x},{y},{z}),rotation:(0.0,0.0,0.0,1.0),character:"Punk",mood:0.75)))\n'
                    conn.sendall(message.encode())
                else:
                    conn.sendall(b'Update(None)\n')
                time.sleep(0.05)
    except Exception as error:
        if not stop.is_set():
            errors.append(str(error))


server = subprocess.Popen(
    [str(SERVER) + SUFFIX, "--bind", "127.0.0.1:0", "--day-seconds", "0"],
    cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
)
worker = None
try:
    line = server.stdout.readline()
    (OUT / "server.log").write_text(line)
    match = re.search(r"127\.0\.0\.1:(\d+)", line)
    if not match:
        raise RuntimeError(f"Server failed to start: {line}")
    address = ("127.0.0.1", int(match[1]))
    subprocess.run([str(SERVER) + SUFFIX, "--check", f"127.0.0.1:{address[1]}"], check=True, timeout=5)
    worker = threading.Thread(target=peer, args=(address,), daemon=True)
    worker.start()
    time.sleep(0.2)
    with (OUT / "client.log").open("w") as log:
        subprocess.run([
            str(CLIENT) + SUFFIX, "--connect", f"127.0.0.1:{address[1]}", "--name", "Grafiktest",
            "--screenshot", str(OUT / "client.png"), "--frames", "200", "--follow",
        ], cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=240, check=True)
    if errors or not positions_seen:
        raise RuntimeError(f"No client positions replicated: {errors}")
    if not (OUT / "client.png").exists():
        raise RuntimeError("Capture did not produce a PNG")
    print(f"Two clients exchanged positions; capture: {OUT / 'client.png'}")
finally:
    stop.set()
    if worker:
        worker.join(timeout=2)
    server.terminate()
    try:
        server.wait(timeout=5)
    except subprocess.TimeoutExpired:
        server.kill()
        server.wait()
