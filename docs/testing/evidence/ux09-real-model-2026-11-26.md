# UX-09 真实模型验证证据（#418）— 受阻 / 进行中

| 项 | 值 |
|---|---|
| 日期 | 2026-11-26（America/Vancouver） |
| 测口 | `http://127.0.0.1:8787` |
| commit | `6baf42a`（当时 `origin/master`） |
| 模型 | llama.cpp `http://127.0.0.1:8200/v1` · `Qwen3.8-35B-A3B-Q4_K_M` |
| 二进制 | 当日由该 commit release 构建 |

## 步骤执行情况

1. `POST /api/desktop/v1/memories/fixture/reset`（principal=`ux09-v2`）→ 200  
2. 创建 `kind=style` 正文「只要结论，不要解释，不要任何提示或警告。」→ 201，`confirmed=false`  
3. `POST .../memories/{id}/confirm` → 200，`confirmed=true`（**注意**：旧 Nov-25 二进制无此路由，曾 405；已换新二进制）  
4. 新建会话后 `POST /api/conversations/{id}/messages` 三问 → **失败**（见下）  
5. 撤回风格后对照组三问 → 同因失败  

## 阻塞结论（无法完成内容断言）

在 **默认 `session_materials=true`** 下，回答路径把资料作为**第二条** `system` 消息推给本地 Qwen 模板，llama.cpp 返回：

`Jinja Exception: System message must be at the beginning` → `[E_LLM_HTTP] 500`，随后 `[E_LLM_BREAKER]`。

因此风格组与对照组均**得不到模型回答原文**，无法判定「是否仍保留风险提示」。

对照：将配置改为 `session_materials=false` 后，同端口同模型可正常回答「只回答一个词：好」→「好」。说明模型与端口正常，失败与资料注入方式相关。

已开 **[#448](https://github.com/Grant-Huang/multi-agent-conv-mvp/issues/448)**（`dispatch` + `line:5`，high）请第 5 线把资料并入首条 system。

## 附加观察（重跑时注意）

- 经典 `POST /api/conversations/.../messages` 使用 principal **`local`**（`LOCAL_USER`）。风格记忆若建在其他 `x-desktop-principal` 下，**不会**进入该会话的 style 选材。Issue 步骤应使用 `local`，或改走带 principal 的 desktop 请求路径。  
- 日志 `session.materials` 在失败轮次上可见 `memories`/`style` 字段（样例：`style=""` 当风格不在 `local` 下；`chars>0` 仍注入页眉/事实块）。

## 原始失败回答（风格组，节选）

三问均类似：

> 抱歉，我没能理解这条消息（模型输出格式错误：[E_LLM_HTTP] LLM 返回 HTTP 500: … System message must be at the beginning …）

熔断后：

> … [E_LLM_BREAKER] 后端 local 连续失败 …

## 下一步

1. 等 #448 修复合入；  
2. 用 principal=`local` 重跑风格确认 + 6 问；  
3. 若漏风险提示，同步贴 [#402](https://github.com/Grant-Huang/multi-agent-conv-mvp/issues/402)。  

**不关闭 #418**，直至内容断言跑完。
