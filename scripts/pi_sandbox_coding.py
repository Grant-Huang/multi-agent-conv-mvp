"""动态起 pi 容器写代码。同时最多 6 个，多出来的要等名额。

每个任务一个 git worktree。容器是 pi-local:1.5.0，模型走宿主机 8200。
"""
import subprocess
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT / "workspace" / "code"
OUT = ROOT / ".tmp" / "pi-sandboxes"
CAP = 6
JOBS = 7
TIMEOUT = 600
slots = threading.Semaphore(CAP)
running = 0
peak = 0
lock = threading.Lock()
results = []


def sh(args, cwd=None):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True)


def job(i):
    global running, peak
    wt = OUT / f"code-{i}"
    name = f"add{i}"
    prompt = (
        f"在 /work 新建 {name}.js，导出 function add(a, b) {{ return a + b }}。"
        f"再新建 {name}.test.js，用 node:test 断言 add(1, 2) === 3。"
        f"然后运行 node --test {name}.test.js。不要改别的文件。"
    )
    t_wait = time.time()
    with slots:
        waited = time.time() - t_wait
        with lock:
            running += 1
            peak = max(peak, running)
        t0 = time.time()
        add = sh(["git", "worktree", "add", "--detach", str(wt), "HEAD"], cwd=REPO)
        code = add.returncode
        tail = add.stderr.strip()[-200:]
        wrote = False
        if add.returncode == 0:
            try:
                run = subprocess.run(
                    [
                        "docker", "run", "--rm",
                        "--name", f"pi-code-{i}",
                        "--add-host=host.docker.internal:host-gateway",
                        "-v", f"{wt}:/work",
                        "-w", "/work",
                        "pi-local:1.5.0",
                        "-p", "--no-session",
                        "--model", "local-llama/qwen3.8-35b-a3b",
                        prompt,
                    ],
                    text=True, capture_output=True, timeout=TIMEOUT,
                )
                code = run.returncode
                tail = (run.stdout or run.stderr)[-300:].replace("\n", " ")
            except subprocess.TimeoutExpired:
                code = 124
                tail = f"超过 {TIMEOUT}s"
                subprocess.run(["docker", "rm", "-f", f"pi-code-{i}"], capture_output=True)
            wrote = (wt / f"{name}.js").exists() and (wt / f"{name}.test.js").exists()
        elapsed = round(time.time() - t0, 1)
        with lock:
            running -= 1
            results.append({"i": i, "wait_s": round(waited, 1), "run_s": elapsed, "code": code, "files": wrote, "tail": tail})


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    threads = [threading.Thread(target=job, args=(i,)) for i in range(JOBS)]
    t0 = time.time()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    print(f"jobs={JOBS} cap={CAP} peak={peak} wall_s={round(time.time()-t0, 1)}")
    for row in sorted(results, key=lambda r: r["i"]):
        print(row)


if __name__ == "__main__":
    main()
