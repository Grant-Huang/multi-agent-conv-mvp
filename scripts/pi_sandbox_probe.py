"""试跑：动态起 Docker 容器，同时最多 6 个。每个任务一个 git worktree。

容器用本机已有的 alpine。里面只做两件事：写一个标记文件，并访问宿主机 8200 的 /slots。
不启动 pi，也不改 AgentNexus 的派发。
"""
import subprocess
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT / "workspace" / "code"
OUT = ROOT / ".tmp" / "pi-sandboxes"
CAP = 6
JOBS = 8
slots = threading.Semaphore(CAP)
running = 0
peak = 0
lock = threading.Lock()
results = []


def sh(args, cwd=None):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True)


def job(i):
    global running, peak
    wt = OUT / f"job-{i}"
    t_wait = time.time()
    with slots:
        waited = time.time() - t_wait
        with lock:
            global running, peak
            running += 1
            peak = max(peak, running)
            now = running
        t0 = time.time()
        add = sh(["git", "worktree", "add", "--detach", str(wt), "HEAD"], cwd=REPO)
        net = "fail"
        marker = False
        if add.returncode == 0:
            run = subprocess.run(
                [
                    "docker", "run", "--rm",
                    "--add-host=host.docker.internal:host-gateway",
                    "-v", f"{wt}:/work",
                    "-w", "/work",
                    "alpine",
                    "sh", "-c",
                    "echo ok > SANDBOX_OK; wget -qO- http://host.docker.internal:8200/slots | head -c 60",
                ],
                text=True, capture_output=True,
            )
            marker = (wt / "SANDBOX_OK").read_text().strip() == "ok" if (wt / "SANDBOX_OK").exists() else False
            net = "ok" if '"id"' in run.stdout or "id" in run.stdout else (run.stderr.strip()[:120] or "empty")
            code = run.returncode
        else:
            code = add.returncode
            net = add.stderr.strip()[:160]
        elapsed = round(time.time() - t0, 2)
        with lock:
            running -= 1
            results.append({"i": i, "wait_s": round(waited, 2), "run_s": elapsed, "code": code, "marker": marker, "net": net, "seen_running": now})
        sh(["git", "worktree", "remove", "--force", str(wt)], cwd=REPO)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    threads = [threading.Thread(target=job, args=(i,)) for i in range(JOBS)]
    t0 = time.time()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    print(f"jobs={JOBS} cap={CAP} peak={peak} wall_s={round(time.time()-t0, 2)}")
    for row in sorted(results, key=lambda r: r["i"]):
        print(row)


if __name__ == "__main__":
    main()
