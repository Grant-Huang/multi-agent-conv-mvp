# 分层记忆细化 片 5：写入面扫描与撤回影响追溯（RT-UMEM-06/07，#346 收尾）

| 项目 | 内容 |
|---|---|
| 日期 | 2026-11-26 |
| 特性 | conversation-memory；需求 REQ-conversation-memory-07（2026-11-26 用户确认冻结；片 5 范围与做法会话经用户批准） |
| 用例 | RT-UMEM-06（写入面扫描 flags）、RT-UMEM-07（撤回影响追溯） |
| 运行模式 | 无头 local；进程内服务夹具 + respond 假模型；`cargo test -p an-server --test personal_memory` |

## 实现

- **写入面扫描（REQ-07 §7.3「写入时扫描可疑指令与敏感信息」）**：`memory_json` 读时派生 `flags`（不改库、不阻断保存、无迁移）——可疑指令样式（词对匹配：忽略/无视/忘记/不要遵守 × 指令/规则/提示、你现在是 × 管理员）→ `injection_risk`；敏感信息样式（-----BEGIN / AKIA / api_key / password / token= / 密码，小写比较）→ `sensitive_info`；保存响应、列表、详情均带。命中供界面标注与审查（展示归第 1 线 #347）。
- **撤回影响追溯（REQ-07 §7.6「可查出被污染记忆影响过哪些回答」/ CH-G3）**：新表 `materials_injections`（来源 id、会话、段别、时间；不含正文）在每轮注入发生时留痕（记录失败仅告警、不影响当前轮）；`GET /api/desktop/v1/memories/{id}/impact` 查询影响过的会话轮次——含已撤回记忆（追溯正是为撤回服务）；未知记忆 404。
- **外部候选流的架构事实（如实登记）**：当前唯一写入面是本人 `POST /memories`（本就是本人确认动作），外部文档/工具输出/他人消息无自动抽取路径——「不能直接写入」由结构保证；若后续引入自动抽取，须先落「待确认候选」闸门再接入（档案 §2 REQ-07 行已注明）。

## Red 说明（如实）

flags 字段、impact 端点、留痕表均为本片新增：master 无对应字段/路由/表，Red 以「缺失」形式存在（编译级 / 端点 404 级）；行为边界（只标注不阻断、flags 三态、留痕一条、撤回后可追溯、空影响、404）由测试断言直接验证。

## Green

```
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test personal_memory
test result: ok. 12 passed; 0 failed; 0 ignored; ... finished in 0.10s
```

（RT-UMEM-01～05、RT-USTY-01～05 回归同绿。）

## 门禁

- fmt / clippy（-D warnings）/ check-map / doc_lint：pass（全量见 PR 评论）
