#!/usr/bin/env python3
"""W4 soak metrics sampler (#426). Writes CSV rows every --interval seconds.

Columns: ts, rss_kb, fds, listen_ports, db_bytes, log_bytes, child_procs, note
"""
from __future__ import annotations

import argparse
import csv
import os
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path


def rss_kb(pid: int) -> int | None:
    try:
        out = subprocess.check_output(["ps", "-o", "rss=", "-p", str(pid)], text=True)
        return int(out.strip() or "0")
    except Exception:
        return None


def fd_count(pid: int) -> int | None:
    path = Path(f"/proc/{pid}/fd")
    if path.exists():
        try:
            return len(list(path.iterdir()))
        except Exception:
            return None
    # macOS
    try:
        out = subprocess.check_output(["lsof", "-nP", "-p", str(pid)], text=True, stderr=subprocess.DEVNULL)
        # header + rows
        return max(0, len(out.splitlines()) - 1)
    except Exception:
        return None


def listen_ports(pid: int) -> str:
    try:
        out = subprocess.check_output(
            ["lsof", "-nP", "-a", "-p", str(pid), "-iTCP", "-sTCP:LISTEN"],
            text=True,
            stderr=subprocess.DEVNULL,
        )
        ports = []
        for line in out.splitlines()[1:]:
            parts = line.split()
            if len(parts) >= 9 and ":" in parts[8]:
                ports.append(parts[8].rsplit(":", 1)[-1])
        return ",".join(sorted(set(ports)))
    except Exception:
        return ""


def child_agent_count() -> int:
    needles = ("codex-acp", "claude-agent-acp", "pi-acp", "opencode", "codex exec")
    try:
        out = subprocess.check_output(["ps", "-ax", "-o", "command="], text=True)
    except Exception:
        return 0
    n = 0
    for line in out.splitlines():
        if any(x in line for x in needles):
            n += 1
    return n


def file_size(path: Path) -> int:
    try:
        return path.stat().st_size if path.exists() else 0
    except Exception:
        return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pid", type=int, required=True, help="agentnexus PID")
    ap.add_argument("--db", type=Path, required=True, help="SQLite db path")
    ap.add_argument("--log-dir", type=Path, required=True, help="server logs dir")
    ap.add_argument("--out", type=Path, required=True, help="CSV output path")
    ap.add_argument("--interval", type=int, default=300, help="seconds between samples")
    ap.add_argument("--duration-hours", type=float, default=8.0)
    args = ap.parse_args()

    args.out.parent.mkdir(parents=True, exist_ok=True)
    new_file = not args.out.exists()
    deadline = time.time() + args.duration_hours * 3600
    fields = [
        "ts_utc",
        "rss_kb",
        "fds",
        "listen_ports",
        "db_bytes",
        "log_bytes",
        "child_procs",
        "note",
    ]
    with args.out.open("a", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        if new_file:
            w.writeheader()
        while time.time() < deadline:
            log_bytes = 0
            if args.log_dir.exists():
                for p in args.log_dir.rglob("*"):
                    if p.is_file():
                        log_bytes += file_size(p)
            row = {
                "ts_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                "rss_kb": rss_kb(args.pid),
                "fds": fd_count(args.pid),
                "listen_ports": listen_ports(args.pid),
                "db_bytes": file_size(args.db),
                "log_bytes": log_bytes,
                "child_procs": child_agent_count(),
                "note": "",
            }
            # alive?
            if row["rss_kb"] is None:
                row["note"] = "process_gone"
                w.writerow(row)
                f.flush()
                return 2
            w.writerow(row)
            f.flush()
            time.sleep(max(1, args.interval))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
