# Desktop 真机 mic 接入方案（T-74，仅文档）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-23（America/Vancouver） |
| 状态 | **方案卡**；待用户确认后另开实现卡 |
| 上位 | [DESIGN.md](DESIGN.md) §8 / D01；[MOCK_REGISTRY](MOCK_REGISTRY.md) M6；决定 [2026-11-21-ai-desktop-d-voice-handoff](../decisions/2026-11-21-ai-desktop-d-voice-handoff.md) |
| 本卡产出 | 接法、验收用例**草案**、风险与非目标；**无业务代码** |

## 1. 问题

D 里程碑已把语音**会话状态**接到服务权威（`desktop_voice_sessions` + `/api/desktop/v1/tasks/{id}/voice/*`；CI 用 fixture 注入权限/相位/转写）。缺口是：

1. 浏览器**真机麦克风**（`getUserMedia`）与权限 UX；
2. 音频 → 部分/最终转写的**真实媒体管道**（而非按钮注入 `partial`/`finalize`）；
3. DESIGN §9 语音质量指标（首响应、插话停止、误打断等）的**可观测证据**；
4. （可选后续）双真机跨端与 D01「手机发起 → 桌面续接」端到端。

本方案只回答：**真机 mic 如何挂在现有服务权威之上**，以及实现卡应冻结哪些用例。

## 2. 现状锚点（不可破坏）

| 层 | 现状 | 真机卡须保持 |
|---|---|---|
| 服务 | `permission` / `phase` / `partial` / `final_text` / `linked_task_id` / `playing` / `share_transcript` | 仍由 HTTP + store 权威；部分转写**不**关联 Task |
| 契约 | RT-ADVOICE-01～04、E2E-ADVOICE（fixture） | 回归仍绿；真机另开 Case，不改断言迎合实现 |
| UI | `VoiceMockPanel` 调同一套 API | 可增「真机」入口或 Media 适配层；面板标题区分 fixture / real |
| 跨端 | `desktop_handoff` + `voice_shared` | 默认不共享；真机转写同样受 `share` 门闩 |

## 3. 推荐接法（助手建议 · 待确认）

### 3.1 一句话

**媒体在浏览器，状态在服务**：客户端只负责采集/ASR；一切可验收会话态仍走现有 `/voice/*`；CI 默认仍跑 fixture，真机证据单独标记。

### 3.2 分层

```text
[Mic / TTS 播放]
      │  MediaCapture (web only)
      ▼
[ASR 适配器]  ← 可插拔：Web Speech | 远端 ASR | 试点厂商 SDK
      │  partial / final 文本
      ▼
[现有 desktopVoice* API]  ← 服务权威不变
      │
      ▼
desktop_voice_sessions（store）
```

### 3.3 客户端职责

1. **权限**：用户手势触发 `getUserMedia({ audio: true })`；成功 → `POST .../voice/permission` `granted`；拒绝/不可用 → `denied`，并引导 `as-text`（与 RT-ADVOICE-01 同语义）。
2. **会话相位**：开始监听 → `POST .../start`；收到部分假设 → `.../partial`；用户确认或端点检测 → `.../finalize`（`link_task` 仅最终）；播报中检测到语音 → `.../barge`；断线 → `.../offline` 后可 `as-text`。
3. **不本地落权威**：不以 React state 覆盖服务端 `linked_task_id` / `permission`；刷新后以 `GET .../voice` 为准。
4. **开关**：`VITE_DESKTOP_VOICE_MEDIA=fixture|real`（或等价运行时旗标）；默认 `fixture`，CI 不碰真麦。

### 3.4 ASR 适配器（首版选型）

| 选项 | 说明 | 建议 |
|---|---|---|
| **A. Web Speech API（浏览器）** | 实现快；Safari/权限差异大；质量不可控 | **首版试点推荐**：证明 mic→partial→finalize→服务链路 |
| B. 远端流式 ASR（HTTP/WS） | 质量可控；需 egress、密钥、延迟预算 | 第二刀；须登记出口与 DLP |
| C. 原生全双工语音模型 | 贴近 checklist 15.1；工程量大 | 非本方案首实现 |

**推荐顺序**：实现卡 ① A（链路通）→ 可选 ② B（质量）→ 全双工另里程碑。不在本方案选定厂商。

### 3.5 服务端是否改？

| 改动 | 本方案 |
|---|---|
| 新 voice 状态字段 | **不必须**；现有字段够承载真机 |
| 上传原始音频到服务 | **首版不做**（隐私与存储另议）；仅传转写文本 |
| `orchestrator.rs` / 任务状态机 | **禁止**（与 D 同） |
| 新表 | **禁止**（会话已有 `desktop_voice_sessions`） |

若日后需服务侧 VAD/审计录音：另开方案卡（授权、保留期、ACL）。

### 3.6 与跨端 / D01

- **本方案首实现刀**：单浏览器真机 mic → 同一 Task 服务会话（桌面或窄视口模拟手机壳均可）。
- **双真机 / 手机发起桌面续接指标**：列为 **Phase-2**（见 §6），依赖 handoff 已有权威 + 真机链路稳定后再开；不并入第一实现卡。

## 4. 验收用例草案（未冻结）

> 以下为草案。用户确认后才写入 `CASES.md` / `features.json` 并开实现卡。不得用 draft 改现有 ADVOICE 断言。

### 4.1 浏览器（真机或受控假设备）

| Case（草案 ID） | Given | When | Then |
|---|---|---|---|
| E2E-ADMIC-01 | `media=real`；干净上下文 | 点「开启麦克风」；浏览器拒绝权限 | UI `permission=denied`；可见改文字入口；`POST permission denied` 落库；无 Task 关联 |
| E2E-ADMIC-02 | 权限可授予（真机或 Playwright fake device） | 授权 → 开始听 → 注入/说出短句 | 出现非空 `partial`；`linked_task_id` 仍空；确认后 `final_text` 非空且可关联当前 Task |
| E2E-ADMIC-03 | 已在播放播报（fixture TTS 或短音频） | 用户开麦插话（或模拟 barge 事件） | `playing=false`；`phase` 回 listening；`final_text` 保留 |
| E2E-ADMIC-04 | 听写中断网或 `offline` | 切「改用文字」提交同文 | 会话可继续；`final_text` 与服务一致 |

**CI 策略（草案）**：E2E-ADMIC-* 默认用 Chromium `--use-fake-ui-for-media-stream` / `--use-fake-device-for-media-stream`；无假设备的环境标 `manual` / 夜间机，**不得**因此 skip 现有 ADVOICE。

### 4.2 HTTP / 运行时（回归既有即可）

真机卡**不新增**改变 RT-ADVOICE 语义的断言。可选补充：

| Case（草案） | Then |
|---|---|
| RT-ADMIC-01 | 文档化：客户端在 `media=real` 时权限迁移路径与 fixture 同契约（granted/denied 枚举不变） |

### 4.3 试点指标（证据，非硬门槛初值）

DESIGN §9：首响应、插话停止、误打断、任务识别准确率。首实现卡只要求：

- 证据文件记录采样方法与 raw 样本数；
- 目标数值「依据试点数据确定」——**本方案不定 KPI 数字**。

## 5. 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| CI 无真麦 / 权限弹窗不稳定 | 假绿或 flake | 默认 fixture；真机 Case 用 fake device 或 manual 标签；证据标明 VERIFIED 级别 |
| Web Speech 浏览器差异 | Safari/企业策略失败 | 适配器失败 → 强制 as-text；不阻塞桌面主路径 |
| 音频出域（远端 ASR） | 隐私 / DLP | 首版不上传音频；若选 B 须登记 egress + 用户可见授权 |
| 部分转写误关联 Task | 业务错派 | 保持 D 不变量：仅 finalize(+link) 写 `linked_task_id` |
| 顺手改 orchestrator / 新表 | 越权高风险 | 实现卡非目标写死；PR 扫描 store 语音表以外变更 |
| 与「持久化记忆表」混淆 | 范围膨胀 | 记忆另卡；语音转写≠个人记忆写入 |

## 6. 非目标（本方案及建议的第一实现卡）

- 改 `orchestrator.rs`、任务状态机、公开受理 API。
- 新建语音/记忆持久化表；个人记忆抽取。
- 强制双真机、WebRTC 网格、原生 App 麦。
- 上传并长期存储原始录音（除非另方案批准）。
- 改写或弱化既有 RT-ADVOICE / E2E-ADVOICE。
- 整壳 UI/UE 大改；以最小 Media 适配 + 现有 Voice 面板扩展为准。
- 自动开实现卡或选定付费 ASR 厂商合同。

## 7. 建议实现切片（确认方案后）

| 刀 | 内容 | 出口 |
|---|---|---|
| **T-75（建议）** | MediaCapture + Web Speech → 现有 `/voice/*`；旗标；E2E-ADMIC-01～04（fake device）；证据 `ai-desktop-t75-mic-real.md` | 真机链路可辨；ADVOICE 回归绿 |
| T-76（建议） | 可选远端 ASR 适配器 + egress 登记 | 质量试点 |
| T-77（建议） | 双设备 D01 指标采集 | MOCK M6「两设备」收口（**已实现**：双 context；第二物理机另验） |

编号待排期确认；**本卡不开实现**。

## 8. Mock → 真 差距（相对 Desktop 主线）

| 能力 | Mock / fixture | 真 | 本方案后 |
|---|---|---|---|
| 语音会话状态 | ✅ 服务权威（D） | ✅ 同 API | 不变 |
| 麦权限 / 采集 | ❌ 按钮注入 | 待 T-75 | 方案已定接法 |
| ASR 质量 / 全双工 | ❌ | 待 T-76+ | 仅选型顺序 |
| 插话停播报 | ✅ fixture barge | 待真麦触发 | 契约已有 |
| 跨端 handoff | ✅ 服务权威 | 双真机待 T-77 | 不并首刀 |
| M3 记忆持久表 | 夹具 | 另卡 | 非本方案 |
| 阅读锚点跨端 | localStorage | 另卡 | 非本方案 |

**M3 夹具主线**：已收口（preferences / 记忆夹具读写 / 已读跨端 / Attention）。**M6 真机媒体**：本方案为入口文档；实现未开始。

## 9. 待用户确认

1. 是否采纳 §3「媒体在浏览器、状态在服务」+ 首版 Web Speech？
2. 第一实现卡是否按 §7 T-75 范围（单端 + fake device E2E），双真机另卡？
3. 用例草案 ID（E2E-ADMIC-*）是否可在确认后冻结进 CASES？

未确认前：**不写业务代码、不登记 features.json 真机 Case 为必跑。**
