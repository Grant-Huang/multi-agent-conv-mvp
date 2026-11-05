"""实测 llama-server 的槽位指定行为（id_slot / slot_id）。

每个实验都用流式请求，观察完就断开（流式断开会取消生成，不留僵尸）。
"""
import http.client
import json
import sys
import threading
import time

HOST, PORT = "127.0.0.1", 8200
LONG = "请写一篇不少于 3000 字的长文，主题是 SMT 贴片机维护，分 15 个小节。"


def get_slots():
    c = http.client.HTTPConnection(HOST, PORT, timeout=5)
    c.request("GET", "/slots")
    d = json.loads(c.getresponse().read())
    c.close()
    return {s["id"]: bool(s.get("is_processing")) for s in d}


def wait_idle(limit=30):
    t = time.time()
    while time.time() - t < limit:
        if not any(get_slots().values()):
            return True
        time.sleep(0.3)
    return False


class Req(threading.Thread):
    """发一个流式请求；记录首个 token 到达时间；stop() 断开。"""

    def __init__(self, extra, prompt=LONG, max_tokens=1500):
        super().__init__(daemon=True)
        self.extra, self.prompt, self.max_tokens = extra, prompt, max_tokens
        self.t_send = self.t_first = None
        self._stop = False
        self.conn = None

    def run(self):
        body = {"model": "x", "messages": [{"role": "user", "content": self.prompt}],
                "max_tokens": self.max_tokens, "stream": True, "temperature": 0.7, **self.extra}
        self.conn = http.client.HTTPConnection(HOST, PORT, timeout=120)
        self.t_send = time.time()
        self.conn.request("POST", "/v1/chat/completions", json.dumps(body),
                          {"Content-Type": "application/json"})
        r = self.conn.getresponse()
        while not self._stop:
            line = r.readline()
            if not line:
                break
            if line.startswith(b"data:") and self.t_first is None and b"content" in line:
                self.t_first = time.time()
        self.conn.close()

    def stop(self):
        self._stop = True
        try:
            self.conn.sock.close()
        except Exception:  # noqa: BLE001
            pass


def busy_ids():
    return sorted(i for i, p in get_slots().items() if p)


def exp_param(name, value):
    assert wait_idle()
    r = Req({name: value})
    r.start()
    time.sleep(2.0)
    ids = busy_ids()
    r.stop()
    time.sleep(0.8)
    print(f"  {name}={value!r:>5} -> 忙碌槽位 {ids}")


def exp_default(n=6):
    seen = []
    for _ in range(n):
        assert wait_idle()
        r = Req({}, prompt=LONG + f"（第{_}次）")
        r.start()
        time.sleep(1.5)
        seen.append(busy_ids())
        r.stop()
        time.sleep(0.8)
    print(f"  不指定槽位，{n} 次分别落到：{seen}")


def exp_busy_target():
    """slot 0 被后台长请求占着，再发一个 id_slot=0 的请求：排队多久？对比 id_slot=1 的请求。"""
    assert wait_idle()
    hog = Req({"id_slot": 0}, max_tokens=3000)
    hog.start()
    time.sleep(2.0)
    short = "只回答：1+1=?"
    a = Req({"id_slot": 0}, prompt=short, max_tokens=20)
    a.start()
    t0 = time.time()
    while a.t_first is None and time.time() - t0 < 15:
        time.sleep(0.1)
    wa = (a.t_first - a.t_send) if a.t_first else None
    b = Req({"id_slot": 1}, prompt=short, max_tokens=20)
    b.start()
    t0 = time.time()
    while b.t_first is None and time.time() - t0 < 15:
        time.sleep(0.1)
    wb = (b.t_first - b.t_send) if b.t_first else None
    c = Req({}, prompt=short, max_tokens=20)  # 不指定
    c.start()
    t0 = time.time()
    while c.t_first is None and time.time() - t0 < 15:
        time.sleep(0.1)
    wc = (c.t_first - c.t_send) if c.t_first else None
    hog.stop()
    for r in (a, b, c):
        r.stop()
    f = lambda x: "15s 内没有首 token（在排队）" if x is None else f"{x:.2f}s"  # noqa: E731
    print(f"  slot0 被占时：id_slot=0 的短请求首 token {f(wa)}；id_slot=1 的 {f(wb)}；不指定的 {f(wc)}")


if __name__ == "__main__":
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    if which in ("all", "param"):
        print("[1] 参数名与指定是否生效")
        for name, value in (("id_slot", 0), ("id_slot", 3), ("id_slot", 4), ("slot_id", 2)):
            exp_param(name, value)
    if which in ("all", "default"):
        print("[2] 不指定槽位时会落到哪")
        exp_default()
    if which in ("all", "busy"):
        print("[3] 目标槽位被占时")
        exp_busy_target()
    wait_idle()
