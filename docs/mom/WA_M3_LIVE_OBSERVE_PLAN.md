# M3 真实工单 Observe 接线方案

状态为联审候选。AgentNexus 基线 `ba9a7c7` 已含 PR #224 的生产桩浏览器验收和 PR #228 的 M3 桩路径准出。本切片服务于 AI 桌面通过 World Adapter 读取制造事实的产品目标。MOM-MCP 短期要求使用同一意图读 Odoo、OpenMES 和 Carbon。 [产品宪章](../PRODUCT.md#3-唯一主轴) · [M3 任务卡](WA_M3_TASKS.md) · [三源验收候选](https://github.com/Grant-Huang/MOM-MCP/blob/e8b9610/docs/acceptance/ADAPTER_READ.md)

## 基线复验

在 `8ed2395` 复验现有 6 项合同，并在本地正式桩绑定世界后提交单工单选择，得到 422，响应指出 `selection` 为未知字段。此结果记录新请求与已合入接口之间的差距。

## 请求与能力

建议在正式 `POST /api/world/prod/observe` 的现有 `world_id`、`request_id` 上增加可选的 `selection`。首个真实切片只接受单对象 `{"type":"work_order","id":"<不透明工单引用>"}`。在服务端登记的真实世界中，缺少选择时返回 `400 WORK_ORDER_SELECTION_REQUIRED`。桩世界继续走当前绑定、快照与续读合同，当前浏览器用例继续作为回归。

真实读成功保留 `protocol_version`、`request_id`、`world_id`、`provenance=production` 和 `consistency=per_source`，返回唯一工单对象，包含不透明引用、公开来源别名、显示号、源状态、映射版本和观测时点。此切片的 `as_of` 表示该次读取的观测时间，源更新时间按映射能力另行保留。单次读取的 `cursor` 和 `publish_seq` 为空，`events` 为空数组。真实绑定建议声明 `work_order_read: true`、`subscribe: false`、`write: false` 及 `actions: false`。UI 依据能力声明决定是否调用 Subscribe。桩合同的发布水位由现有实现负责。

## 身份、绑定与出站

建议首轮只在授权测试宿主以独立服务主体运行。正式凭证方案由宿主评审确认，至少绑定可验证的签发方、受众、主体和截止时间。按请求检查当前主体授权与撤权。现有 `X-WA-Role` 和 `X-WA-Site` 属桩夹具输入。

1. 宿主验证可信身份并解析租户、站点、真实世界及权威来源。在调用连接器前核对工单引用属于该世界且主体具备读取授权。
2. 可信站点配置登记来源别名、连接器类型、受信源地址和凭据引用。租户、站点、来源与工单 ID 由服务端从授权绑定推导。分别管理宿主凭证和源端凭证。
3. 出站按站点的固定目标和协议批准。沿用连接器的 HTTPS 或隔离回环约束、超时、响应大小和禁重定向设置。Carbon 社区版以短期 Supabase 用户会话及公开项目键读取，不同公司测试身份各自独立。
4. 每个授权请求只调用绑定的一个源。真实来源故障原样进入错误归类，其他来源和模拟调用数均为零。宿主以正式错误包络返回可公开的分类。记录请求 ID、来源别名和分类供复查，公开响应省略源端私有 ID、凭据及原始诊断。

前置评审项为可信身份映射、凭据存取、站点出站和公开能力字段。建议依赖可由两仓干净检出的独立 MOM-MCP 适配器 crate，固定 Git revision。工作区候选适配器 `ea1fcd8`、宿主 `7a680bf` 及 Carbon 双会话证据 `b5e7dee` 是回归材料。依赖评审使用 MOM-MCP 已发布候选分支 `stage/adapter-agentnexus-compat` 的固定提交 `dc504693a44b2ed64afbab0dd345fe48dade23a3`。已用该远端 SHA 复验干净的 AgentNexus `ba9a7c7` 检出。临时锁文件只新增 adapter，宿主桩合同 6 项与 adapter 12 项均通过。生产接线前确认 adapter 与宿主方案的合并及审批范围。本卡先交接口方案与测试断言。 [现有真站点升级边界](WA_M3_1_PLAN.md#3-明确不改除非升级方案再批)

## 候选验收

复用正式错误包络 `code`、`message`、`request_id`、`protocol_version`。下表错误码随本卡联审冻结。

| 情形 | 候选结果与断言 |
|---|---|
| 缺少、失效或已撤销的服务凭证 | `401 AUTHENTICATION_REQUIRED`，来源调用为零 |
| 已认证但缺少世界或引用授权 | `403 ACCESS_DENIED`，来源调用为零 |
| 已授权的范围内工单 | `200`，精确匹配工单引用、来源及保留的源状态 |
| 明确绑定的源端范围外样本 | `404 ORDER_NOT_VISIBLE`，通过第二来源身份独立确认样本存在 |
| 上游故障或当前无法分类的工具拒绝 | `503 SOURCE_UNAVAILABLE`，零回退，保留受信诊断 |
| 记录错配、响应无效 | `502 INVALID_SOURCE_RESPONSE`，结果省略源私有字段 |
| 新旧模式兼容 | 本卡 Green 后重复 M3 桩合同与浏览器回归，验证桩续读、生产写关闭和隔离 Mock 写路径 |

执行 `node --test scripts/accept-m3-live-observe.test.mjs` 验证候选断言和脱敏输出。在已批准的联调宿主上，使用 `AN_M3_OBSERVE_ORIGIN` 和 `AN_M3_OBSERVE_TOKEN`，运行 `node scripts/accept-m3-live-observe.mjs <bindings.json>`。输入数组中的各项包括 `vendor`（`odoo`／`openmes`／`carbon`）、`world`、`visible`、`hidden`、`source`、`sourceStatus`。两个不透明引用都需向测试主体授权，以通过同一身份验证源端可见范围。该探针验证三源的匿名拒绝、成功和源端不可见。撤权、跨站、故障注入和零回退另由受信服务层测试记录调用数。测试索引和正式 ID 随联审冻结。

建议按服务层合同、单站点联调、三源复跑顺序验收。停用真实绑定并撤销测试凭证即可回退联调。桩数据与真实来源配置分别保留验证证据。

## 正式路径夹具验收

AgentNexus `c69c9e5` 已合入本方案。夹具切片仅在测试构造宿主时显式注入工单读取器。服务入口默认加载的 M3 桩无工单读取器，有选择的请求返回 `TEST_CONTROLS_DISABLED`。认证使用测试角色和绑定站点头，缺少任一项或站点不匹配时拒绝。成功响应标明 `provenance=fixture`，其工单、来源、状态与空游标字段供正式接线复用。生产身份和凭据验证仍按上节方案验收。

运行 `cargo test --locked -p an-server --test mom_world_prod`。正式路径测试按 Git SHA 引入 MOM-MCP `5aa3129` 的 adapter，使用 Odoo JSON-2、OpenMES HTTP 和 Carbon 社区 REST 回环响应。RT-WAPROD-07 验证默认关闭工单夹具。RT-WAPROD-08 验证一世界一来源读取、权限／引用拒绝零调用、范围外不可见，以及 OpenMES 故障只访问已绑定连接器。原有 6 项桩合同随同回归。

## 单站点联调探针

首个站点获批后，在联调宿主设置上文的两个环境变量，并运行 `node scripts/accept-m3-live-observe.mjs --single-site <bindings.json>`。文件沿用上文的数组格式，只填该站点的一项绑定。探针对同一世界验证匿名拒绝、授权工单读取和源端范围外不可见。省略 `--single-site` 后，联合验收仍要求三个不同世界及来源，且三个系统均须通过。两种模式均在发请求前校验完整绑定，输出只包含已检查的系统及 HTTP 状态。

AgentNexus PR #248 已合入正式路由夹具。单站点探针的自动化测试覆盖三个系统的独立调用及两种模式的输入边界。真实联调需要批准服务主体、工单绑定、源端凭据和出站范围，仍按服务层、单站点、三源的顺序记录结果。
