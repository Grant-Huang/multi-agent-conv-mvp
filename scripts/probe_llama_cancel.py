"""实测 llama-server 在客户端断开后是否停止生成（只盯自己的槽位，不受并发请求干扰）。

做法：发一个会生成很久的请求，几秒后客户端断开。断开前记下"我的槽位 id 和已生成 token 数"，
断开后持续观察这个槽位：
  - 很快空闲，或 n_decoded 回到小数值（被别的请求复用） => 已取消
  - 仍在处理且 n_decoded 继续增长 => 没有取消
"""
import http.client
import json
import socket
import sys
import time

HOST, PORT = "127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 8200
MODE = sys.argv[2] if len(sys.argv) > 2 else "both"
PROMPT = "请写一篇不少于 5000 字的长文，主题是 SMT 贴片机的维护保养，分 20 个小节，每节详细展开。"


def slots():
    c = http.client.HTTPConnection(HOST, PORT, timeout=5)
    c.request("GET", "/slots")
    data = json.loads(c.getresponse().read())
    c.close()
    return {
        s["id"]: (bool(s.get("is_processing")), s.get("next_token", [{}])[0].get("n_decoded", 0))
        for s in data
    }


def wait_all_idle(limit=120):
    t0 = time.time()
    while time.time() - t0 < limit:
        if not any(p for p, _ in slots().values()):
            return True
        time.sleep(0.5)
    return False


def run(stream: bool, hold_secs: float = 5, watch_secs: float = 20):
    label = "流式" if stream else "非流式"
    if not wait_all_idle():
        print(f"[{label}] 服务一直不空闲，放弃")
        return
    body = json.dumps({"model": "x", "messages": [{"role": "user", "content": PROMPT}],
                       "max_tokens": 6000, "stream": stream, "temperature": 0.7}).encode()
    s = socket.create_connection((HOST, PORT), timeout=10)
    s.sendall((f"POST /v1/chat/completions HTTP/1.1\r\nHost: {HOST}\r\n"
               f"Content-Type: application/json\r\n"
               f"Content-Length: {len(body)}\r\n\r\n").encode() + body)
    t0 = time.time()
    s.settimeout(0.5)
    while time.time() - t0 < hold_secs:
        try:
            s.recv(65536)
        except socket.timeout:
            pass
    now = slots()
    mine = [(i, n) for i, (p, n) in now.items() if p]
    if len(mine) != 1:
        print(f"[{label}] 断开前忙碌槽位={mine}，无法确定哪个是我的，放弃本轮")
        s.close()
        return
    slot_id, n_before = mine[0]
    s.close()
    t_close = time.time()
    verdict = None
    last = (True, n_before)
    while time.time() - t_close < watch_secs:
        p, n = slots()[slot_id]
        last = (p, n)
        if not p:
            verdict = f"已取消：槽位在断开后 {time.time() - t_close:.2f}s 回到空闲（断开时已生成 {n_before}）"
            break
        if n < n_before:
            verdict = f"已取消：槽位被新请求复用（n_decoded {n_before} -> {n}）"
            break
        time.sleep(0.2)
    if verdict is None:
        verdict = (f"没有取消：断开后 {watch_secs:.0f}s 槽位仍在生成，"
                   f"n_decoded {n_before} -> {last[1]}")
    print(f"[{label}] {verdict}")


def run_curl(stream: bool, hold_secs: int = 5, watch_secs: float = 20):
    """用真实 curl 的 -m 超时来断开，排除"裸 socket 关闭方式"的差异。"""
    import subprocess
    label = ("流式" if stream else "非流式") + "(curl)"
    if not wait_all_idle():
        print(f"[{label}] 服务一直不空闲，放弃")
        return
    body = json.dumps({"model": "x", "messages": [{"role": "user", "content": PROMPT}],
                       "max_tokens": 6000, "stream": stream, "temperature": 0.7})
    url = f"http://{HOST}:{PORT}/v1/chat/completions"
    p = subprocess.Popen(["curl", "-s", "-N", "-m", str(hold_secs), url,
                          "-H", "Content-Type: application/json", "-d", body],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(hold_secs - 1)
    mine = [(i, n) for i, (pr, n) in slots().items() if pr]
    if len(mine) != 1:
        print(f"[{label}] 断开前忙碌槽位={mine}，无法确定哪个是我的，放弃本轮")
        p.wait()
        return
    slot_id, n_before = mine[0]
    p.wait()  # curl 到 -m 超时后退出 => 断开
    t_close = time.time()
    last = n_before
    while time.time() - t_close < watch_secs:
        pr, n = slots()[slot_id]
        last = n
        if not pr:
            print(f"[{label}] 已取消：断开后 {time.time() - t_close:.2f}s 槽位空闲（断开前已生成 {n_before}）")
            return
        if n < n_before:
            print(f"[{label}] 已取消：槽位被新请求复用（{n_before} -> {n}）")
            return
        time.sleep(0.2)
    print(f"[{label}] 没有取消：断开后 {watch_secs:.0f}s 仍在生成，n_decoded {n_before} -> {last}")


if __name__ == "__main__":
    if MODE == "curl":
        run_curl(stream=False)
        sys.exit(0)
    if MODE == "curlstream":
        run_curl(stream=True)
        sys.exit(0)
    if MODE in ("both", "stream"):
        run(stream=True)
    if MODE in ("both", "nonstream"):
        run(stream=False)
