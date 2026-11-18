# 外发脱敏测试集：样本要求、标注规范与交付格式（v0.1）

给负责准备测试集的同事。目标是做出一份**独立于开发者**的正式评测集，用来判断 dlp-egress 能否上线（设计文档 9.3 节）。

相关文件：

- 模板：`tools/dlp-egress/bench/templates/annotation_template.csv`（10 条示例，可直接在 Excel 中打开）
- 校验与转换：`uv run python -m bench.convert_annotations <你的文件> --out <输出.jsonl>`
- 评测：见 `docs/LOCAL_TEST_GUIDE.md` 第 6 节

## 1. 三条硬规则

1. **不使用真实数据。** 客户名、人名、工单号、金额、电话、邮箱全部虚构，但要像真的一样。
   邮箱请用 `example.com` / `example.net` 或明显虚构的域名。
2. **不写真实或真实格式的凭证。** 凭证样本统一写成合成占位串，例如 `DEMO-NOT-A-REAL-TOKEN-0001`、`Demo#Pass-2026`。
   不要写 `ghp_…`、`AKIA…`、`sk_live_…`、`eyJ…` 这类格式：会被仓库的密钥扫描拦截，也存在误用风险。
   需要覆盖特定凭证格式时，在 `notes` 里写清楚格式名称，由开发者用运行时拼接的方式补充。
3. **测试集与开发隔离。**
   - 编写人不要参考 `bench/data/agentnexus_dlp_700.jsonl`、`bench/holdout.py` 和识别规则源码，避免写成同一套模板。
   - **测试集不提交到仓库**，由测试负责人保管。开发者只看评测报告（报告中凭证已掩码），不看样本原文，避免“对着考卷调参”。
   - 确实需要向开发者展示失败样本时，每轮最多展示 10 条，并在交付记录中注明。展示过的样本在下一版测试集中替换掉。

## 2. 数量与分布要求

最少 **300 条**（设计文档要求的底线是 100 条，300 条才能按类型和通道分桶看结果）。建议分布：

| 维度 | 要求 |
|---|---|
| 语言 | 中文（含中英混排）≥ 65%，英文 ≥ 20% |
| 通道 `channel` | `email` / `slack` / `external_api` / `file_upload` / `mcp` 每类 ≥ 12% |
| 期望决策 | ALLOW ≥ 15%，ANONYMIZE ≥ 35%，REVIEW ≥ 15%，BLOCK ≥ 10% |
| 目的地 `destination_trust` | `external_untrusted` ≥ 75%，`internal` ≥ 8%，`unknown` ≥ 5% |
| 难度 `difficulty` | `hard` ≥ 30%（定义见第 3 节） |
| 每类实体出现次数 | CUSTOMER、PERSON、ORDER_ID ≥ 60；PROJECT、PLANT、DEVICE_ID、METRIC ≥ 40；EMAIL、CONTACT、AMOUNT、INTERNAL_INFO ≥ 30；SECRET ≥ 25 |
| 实体名的多样性 | 同一个客户名、人名在整个集合里最多出现 5 次；客户名不少于 80 个，人名不少于 80 个 |
| 句式多样性 | 不得批量套用同一句式。同一句式（只换实体）最多 3 条 |

## 3. 样本类型清单（每类都要有）

| 场景 `scenario` | 说明 | 举例方向 |
|---|---|---|
| `normal` | 常规外发，含各种标识符 | 维修记录给供应商、交期变更给物流商、质量报告给客户 |
| `confidential` | 含机密信息 → REVIEW | 报价底线、折扣协议、工艺配方、成本结构、未公开路线图 |
| `secret` | 含凭证 → BLOCK | Authorization 头、`password=`、API key、连接串里的密码、私钥片段 |
| `amount` | 金额，含“经批准用于计算”（`preserve:AMOUNT`）和未批准两种 | 审计核对、报价比较、预算外泄 |
| `toolcall` | Agent 工具调用文本或 JSON 参数 | `send_email(to=…)`、`calendar.invite(attendees=[…])`、`{"customer": …}` |
| `internal` | 发往内部系统 → 除凭证外原样 | 内部知识库、内部工单系统 |
| `unknown_destination` | 未登记目的地 → BLOCK | 粘贴到外部网站、发给陌生地址 |
| `clean` | 干净负例 → ALLOW | 通用知识、公开标准；**要包含易误报词**：“项目管理”“工厂安全”“周工作计划”“OEE 定义” |
| `adversarial` | 对抗与混淆 | 全角字符、零宽字符、空格插字、同一实体多次出现、中英混排、简称与全称混用 |
| `hard` | 规则难以识别的情况（`difficulty=hard`） | 无称谓的人名（王晓峰）、无行业后缀的公司名（蓝鲸）、口语化简称、拼音或英文缩写的客户名 |

## 4. 标注格式

### 4.1 文件格式

CSV（UTF-8；Excel 中用“另存为 → CSV UTF-8”）或 JSONL，每行一条，字段如下：

| 字段 | 必填 | 取值 / 说明 |
|---|---|---|
| `id` | 是 | 唯一编号，建议 `<编写人缩写>-<序号>`，如 `ZS-0001` |
| `language` | 是 | `zh` / `en`。中英混排标 `zh` |
| `channel` | 是 | `email` / `slack` / `external_api` / `file_upload` / `mcp` |
| `scenario` | 否 | 第 3 节中的场景名 |
| `destination_trust` | 是 | `external_untrusted`（已登记的外部接收方）/ `internal`（企业内部系统）/ `unknown`（未登记） |
| `purpose` | 否 | 外发用途，自由填写，如 `vendor_diagnostics` |
| `constraints` | 否 | 目前只有 `preserve:AMOUNT`（金额经批准用于计算）。多个用 `;` 分隔 |
| `annotated_text` | 是 | 带实体标记的文本，见 4.2 |
| `expected_action` | 是 | 你认为**正确**的决策：`ALLOW` / `ANONYMIZE` / `REVIEW` / `BLOCK` |
| `difficulty` | 否 | `normal` / `hard` |
| `annotator` / `reviewer` | 建议 | 标注人 / 复核人 |
| `notes` | 否 | 判断理由、疑问 |

不需要填写期望的脱敏文本：转换脚本会按策略自动生成。

### 4.2 实体标记

- 写法：`{类型:原文}`，例如 `请把{CUSTOMER:松岩液压}的{ORDER_ID:PO-771203}发出`。
- 不允许嵌套；花括号内的原文首尾不要有空格。
- 正文里本来就有的花括号写成双括号：`{{` 表示 `{`，`}}` 表示 `}`。
- 一个实体只标一次。同一实体出现多次，每次出现都要标。

### 4.3 实体类型与边界

| 类型 | 标什么 | 边界规则 | 正例 | 不要标 |
|---|---|---|---|---|
| `CUSTOMER` | 外部组织：客户、供应商、合作方 | 完整名称，含“有限公司”等后缀；不含“的”“客户”等前后文 | `{CUSTOMER:松岩液压}`、`{CUSTOMER:Granite Peak Machinery}` | 本公司名称；泛指的“客户”“供应商” |
| `PROJECT` | 项目代号 | 含“项目”/“Project”字样 | `{PROJECT:项目长庚}`、`{PROJECT:Project Osprey}` | “项目管理”“项目进展”等普通词 |
| `PLANT` | 工厂、车间、产线基地名称 | 完整名称 | `{PLANT:滨江四厂}`、`{PLANT:Assembly Hall B}` | “工厂”“车间”等泛指 |
| `PERSON` | 人名 | 中文**连同称谓**一起标；英文标姓名全称 | `{PERSON:郑厂长}`、`{PERSON:王晓峰}`、`{PERSON:Dana Brooks}` | 职务泛称（“经理”“工程师”） |
| `ORDER_ID` | 工单、订单、批次、质检单、RMA 等单据号 | 整个编号 | `{ORDER_ID:WO-118820}`、`{ORDER_ID:LOT-7Q}` | 版本号（V1.0）、标准号（ISO 9001） |
| `DEVICE_ID` | 设备编号 | 整个编号 | `{DEVICE_ID:CNC-07}`、`{DEVICE_ID:AGV-11}` | 设备型号的通用名称 |
| `EMAIL` | 邮箱地址 | 整个地址。**收件人参数里的邮箱也要标**，是否保留由策略判断 | `{EMAIL:qa@partner-demo.com}` | — |
| `CONTACT` | 电话号码 | 只标号码 | `{CONTACT:13612345678}`、`{CONTACT:+44 20 7946 0958}` | — |
| `AMOUNT` | 金额 | 币种 + 数值（+ 单位） | `{AMOUNT:96.5万元}`、`{AMOUNT:USD 72,500}` | 数量、百分比 |
| `METRIC` | 生产指标 | 指标名 + 数值 + 单位 | `{METRIC:良率97.5%}`、`{METRIC:cycle time 42 s}` | 只有数字、没有指标名 |
| `INTERNAL_INFO` | 商业机密内容 | 连同修饰语一起标 | `{INTERNAL_INFO:内部报价底线}`、`{INTERNAL_INFO:proprietary pricing strategy}` | “请勿外传”等动作说明本身 |
| `SECRET` | 凭证的值 | **只标值**，不含 `Bearer`、`password=` 等前缀 | `Authorization: Bearer {SECRET:DEMO-NOT-A-REAL-TOKEN-0001}` | — |

拿不准时：在 `notes` 里写下疑问，先按“更保守”的方式标（标上实体），由复核人决定。

## 5. 怎么判断 `expected_action`

先按下表推出“按策略的决策”，再结合业务常识判断它是否合理。
如果你认为按策略的结果在业务上不对（例如应该审批却会被放行），**照你的判断填**，并在 `notes` 写明理由。转换脚本会把这类样本列为“分歧”，这正是我们最需要的反馈。

| 目的地 | 文本中含 | 决策 |
|---|---|---|
| `unknown` | 任何内容 | BLOCK |
| 任意 | `SECRET` | BLOCK |
| `internal` | 不含 SECRET | ALLOW |
| `external_untrusted` | `INTERNAL_INFO` | REVIEW |
| `external_untrusted` | 客户、项目、厂区、人员、单据号、邮箱（非收件人参数）、电话、金额（无 `preserve:AMOUNT`） | ANONYMIZE |
| `external_untrusted` | 只有指标、设备号、收件人参数中的邮箱，或没有实体 | ALLOW |

多条同时满足时取最严：BLOCK > REVIEW > ANONYMIZE > ALLOW。

## 6. 流程

1. **试标**：每位标注人先按模板写 20 条，用转换脚本校验；复核人检查边界规则是否一致，统一口径后再批量编写。
2. **双人一致性**：随机抽 10% 的样本由第二人独立标注。统计实体（类型 + 起止位置）一致率和决策一致率；
   决策一致率低于 90% 时，先开会统一规则再继续。
3. **复核**：每条都要有 `reviewer`。复核人重点看边界、漏标和 `expected_action`。
4. **校验**：

   ```bash
   cd tools/dlp-egress && uv run python -m bench.convert_annotations path/to/annotations.csv --out /tmp/dlp-new.jsonl
   ```

   - 退出码 0：通过，并输出 JSONL。
   - 退出码 1：有格式错误，按 `[错误]` 提示逐条修改。不会输出半成品。
   - 退出码 3：有 `[分歧]`。逐条确认：是标注错误就改正；是策略问题，就在 `notes` 写明并汇总给我，再加 `--allow-disagreement` 转换。
   - 最后一行会打印分布统计，可以对照第 2 节检查是否达标。
5. **交付**：
   - 原始标注文件和转换后的 JSONL：**由测试负责人保管，不提交到仓库**。
   - 发给开发者：分布统计、分歧清单（只写编号和理由，不带原文），以及 `LOCAL_TEST_GUIDE.md` 第 6 节的评测报告。

## 7. 版本管理

- 测试集按版本命名，如 `dlp-gold-v1.csv`，并记录 sha256。评测报告中会记录所用文件的哈希。
- 一个版本发布后不再修改。需要更正时出新版本，并附变更说明。
- 已向开发者展示过原文的样本，在下一版本中替换（见第 1 节）。
