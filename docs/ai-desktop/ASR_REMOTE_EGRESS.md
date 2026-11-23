# Desktop 远端 ASR 外发登记（T-76）

| 项目 | 内容 |
|---|---|
| destination_id | `desktop-asr-stream` |
| 通道 | 浏览器可选直连（HTTP/WS）；**非** Rust `[egress.tools]` 工具路径 |
| 信任 | `external_untrusted`（见 `tools/dlp-egress/config/destinations.example.yaml`） |
| 默认 | **不上送 PCM**；仅会话信令 + 文本事件；stub/回放零出站 |
| 密钥 | `VITE_DESKTOP_ASR_URL` / `VITE_DESKTOP_ASR_TOKEN`（或 window 覆盖）；缺省回退 Web Speech |
| 录音 | **禁止**落盘原始音频（本卡非目标） |

## 与 DLP 的关系

1. 当前实现是**前端可选适配器**：CI 用 `remote-stub`，不经 dlp-egress 进程。
2. 目的地表预先登记 `desktop-asr-stream`，供日后宿主代发 / enforce 校验对齐。
3. 真钥 live 验证另做：须用户可见授权、密钥不入库、证据标「真钥另验」。

## 旗标

| 变量 | 含义 |
|---|---|
| `VITE_DESKTOP_ASR_BACKEND` | `webspeech`（默认）\| `remote-stub` \| `remote` |
| `VITE_DESKTOP_ASR_URL` | HTTP(S) 或 WS(S) 端点 |
| `VITE_DESKTOP_ASR_TOKEN` | Bearer / auth 占位 |
| `VITE_DESKTOP_ASR_SEND_AUDIO` | `1` 才允许后续音频上送扩展（默认关） |
