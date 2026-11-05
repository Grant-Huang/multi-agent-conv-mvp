"""生成 evals/routing.jsonl（路由评测集 v2）。

用脚本生成，保证每行都是合法 JSON，长文本样本也便于构造。
补充样本：修改本文件后运行  python3 evals/make_dataset.py > evals/routing.jsonl
"""
import json, sys

HEADER = """// 路由评测集 v2（由 evals/make_dataset.py 生成，请修改脚本而不是直接改这个文件）
// 字段说明见 crates/server/src/bin/eval.rs 开头；评测方法见 evals/EVAL_PROMPT.md
//
// 标注口径：
// - 精确计算、当前时间、读写工作目录文件、改代码仓库：一律交给后台（即使模型自己会）
// - 在对话里给示例代码、讲概念、给建议：直接回答
// - 文件路径、测试命令、代码风格等执行者能自行确定的细节：不追问
// - L3 = 只生成文档（保存用的 write_file 不算"需要工具"）；L4 = 需要读数据 / 计算 + 生成文档
// - 风险：只读 = low；写文件或改代码 = medium；删除、对外发送、生产环境 = high
// - 审查类样本：基础期望对应 reviewer 模式，by_review_mode 写 direct / auto 模式下的不同期望
// - coding / reviewer 样本不检查 L1-L4 类别
"""

out = []
def c(id, text, action, tags, role=None, cls=None, tools=None, risk=None, note="", **kw):
    d = {"id": id, "text": text, "tags": tags, "expect_action": action}
    if role: d["expect_role"] = role
    if cls: d["expect_class"] = cls
    if tools: d["expect_tools"] = tools
    if risk: d["expect_risk"] = risk
    d.update(kw)
    if note: d["note"] = note
    out.append(d)

def section(title):
    out.append("// ───── " + title + " ─────")

# 审查模式覆盖：短内容 → direct / auto 直接回答
SHORT_REVIEW = {"direct": {"expect_action": "respond", "expect_role": None},
                "auto": {"expect_action": "respond", "expect_role": None}}
# 长内容 → direct 直接回答；auto 仍交给审查
LONG_REVIEW = {"direct": {"expect_action": "respond", "expect_role": None}}

section("A. 闲聊、常识、建议：应直接回答（respond）")
c("chat-01", "你好", "respond", ["chat"])
c("chat-02", "早上好，今天状态不错", "respond", ["chat"])
c("chat-03", "你是谁？能帮我做什么？", "respond", ["chat"])
c("chat-04", "谢谢，辛苦了", "respond", ["chat"])
c("chat-05", "好的，明白了", "respond", ["chat"])
c("know-01", "什么是 MES 系统？和 ERP 有什么区别？", "respond", ["knowledge"])
c("know-02", "TCP 和 UDP 的区别是什么", "respond", ["knowledge"])
c("know-03", "解释一下 Rust 的所有权机制", "respond", ["knowledge"])
c("know-04", "SPC 统计过程控制是什么意思？", "respond", ["knowledge"])
c("know-05", "Python 里 list 和 tuple 有什么区别", "respond", ["knowledge"])
c("know-06", "git 怎么回滚上一次提交？", "respond", ["knowledge", "boundary"], note="问方法，不是要求在仓库里执行")
c("know-07", "什么是代码审查的最佳实践？", "respond", ["knowledge", "boundary"], note="含'审查'字样但不是审查请求")
c("adv-01", "给我一些降低 SMT 产线抛料率的建议", "respond", ["advice"])
c("adv-02", "我们的新服务该用 Postgres 还是 SQLite？说说你的看法", "respond", ["advice"])
c("adv-03", "多 agent 系统里，审查流程一般应该怎么设计？", "respond", ["advice", "boundary"], note="讨论审查流程，不是审查请求")
c("snip-01", "写一个 Python 快速排序给我看看", "respond", ["boundary", "snippet"], note="在对话里给示例代码，不写入仓库")
c("snip-02", "用 Rust 写个读取 CSV 的示例代码", "respond", ["boundary", "snippet"], note="示例代码，不写入仓库")
c("tool-know-01", "calculator 工具支持哪些运算？", "respond", ["boundary"], note="问工具本身，不需要调用工具")

section("B. 缺少关键信息：应追问（clarify）")
c("clar-01", "帮我优化一下", "clarify", ["clarify"])
c("clar-02", "把它改好", "clarify", ["clarify"])
c("clar-03", "生成一份报告", "clarify", ["clarify"], note="不知道报告的主题和内容")
c("clar-04", "帮我处理一下那个文件", "clarify", ["clarify"], note="不知道是哪个文件、做什么处理")
c("clar-05", "这个对吗？", "clarify", ["clarify"], note="没有给出要判断的内容")
c("clar-06", "按上次说的做", "clarify", ["clarify"], note="没有历史对话")
c("clar-07", "帮我翻译一下", "clarify", ["clarify"])
c("clar-08", "删掉那些没用的文件", "clarify", ["clarify", "risk"], note="不知道哪些文件算没用，且是删除操作")
c("clar-09", "帮我分析一下数据", "clarify", ["clarify"], note="不知道是什么数据")

section("C. 必须用工具：计算、时间")
c("calc-01", "帮我算一下 (1280 * 0.85 + 36) / 12 等于多少", "delegate", ["tool", "calc", "regression"], "general", "L2", ["calculator"], "low", note="上次 27B 直接心算（v1 #11）")
c("calc-02", "一批货 3600 件，良率 97.5%，合格品有多少件？", "delegate", ["tool", "calc"], "general", "L2", ["calculator"], "low")
c("calc-03", "1+1 等于几", "delegate", ["tool", "calc", "boundary"], "general", "L2", ["calculator"], note="按规则一律交给 calculator；如果认为简单算术应直接回答，需要改规则和这条标注")
c("calc-04", "设备 OEE：可用率 92%，性能率 88%，良品率 99%，OEE 是多少？", "delegate", ["tool", "calc"], "general", "L2", ["calculator"])
c("calc-05", "把 2.5 英寸换算成毫米", "delegate", ["tool", "calc"], "general", "L2", ["calculator"])
c("time-01", "现在几点了？", "delegate", ["tool", "time", "regression"], "general", "L2", ["current_time"], "low", note="上次 27B 反问时区（v1 #12）")
c("time-02", "今天星期几", "delegate", ["tool", "time"], "general", "L2", ["current_time"])
c("time-03", "今天几号？距离月底还有几天？", "delegate", ["tool", "time"], "general", "L2", ["current_time"])
c("time-04", "明天是几月几号", "delegate", ["tool", "time"], "general", "L2", ["current_time"])

section("D. 读写工作目录中的文件、生成文档")
c("file-01", "看看工作目录里有哪些文件", "delegate", ["tool", "file"], "general", "L2", ["list_files"], "low")
c("file-02", "reports 目录下有几个报告？", "delegate", ["tool", "file"], "general", "L2", ["list_files"], "low")
c("file-03", "读一下 notes/todo.md，告诉我还有哪些没完成", "delegate", ["tool", "file"], "general", "L2", ["read_file"], "low")
c("file-04", "读取 notes/meeting-1105.md，总结一下会议要点", "delegate", ["tool", "file"], "general", "L2", ["read_file"], "low", note="总结在对话中给出，不保存")
c("file-05", "看看 data/defects.csv 里哪条产线的缺陷最多", "delegate", ["tool", "file"], "general", "L2", ["read_file"], "low")
c("doc-01", "写一份关于本地小模型选型的报告，保存成 reports/llm.md", "delegate", ["doc"], "general", "L3", ["write_file"], "medium")
c("doc-02", "写一份 Rust 新人入门指南，存成 Markdown 文件", "delegate", ["doc"], "general", "L3", ["write_file"], "medium")
c("doc-03", "把下面的会议纪要整理好，保存为 notes/meeting.md：1. 下周三上线 2. 张工负责测试 3. 李工负责回滚预案", "delegate", ["doc"], "general", "L3", ["write_file"], "medium")
c("doc-04", "帮我写一份 MES 系统上线检查清单，保存到 docs/checklist.md", "delegate", ["doc"], "general", "L3", ["write_file"], "medium")
c("doc-05", "读取 data/defects.csv，统计各产线缺陷数，写成分析报告保存到 reports/defects.md", "delegate", ["tool", "file", "doc"], "general", "L4", ["read_file", "write_file"], "medium")
c("doc-06", "汇总 notes 目录下所有会议纪要，整理成一份周报文件", "delegate", ["tool", "file", "doc"], "general", "L4", ["list_files", "read_file", "write_file"], "medium")
c("doc-07", "把 reports/q-week45.md 的结论部分改得更简洁", "delegate", ["tool", "file", "doc"], "general", "L4", ["read_file", "write_file"], "medium")

section("E. 改代码仓库（coding）：不应为路径、测试命令等可自行确定的细节追问")
c("code-01", "在代码仓库里写一个 Python 斐波那契函数，并加上单元测试", "delegate", ["coding"], "coding", risk="medium")
c("code-02", "修复 src/main.rs 里的编译错误", "delegate", ["coding"], "coding", risk="medium")
c("code-03", "给项目加一个 README，并把 Cargo.toml 的版本号改成 0.2.0", "delegate", ["coding"], "coding", risk="medium")
c("code-04", "重构 utils.py，把重复代码抽成函数，确保测试通过", "delegate", ["coding", "regression"], "coding", risk="medium", note="上次 27B 追问仓库路径和测试命令（v1 #22）")
c("code-05", "跑一下项目的测试，失败的话修好", "delegate", ["coding"], "coding", risk="medium")
c("code-06", "修复登录接口偶尔返回 500 的问题", "delegate", ["coding"], "coding", risk="medium")
c("code-07", "给 order 模块补充单元测试", "delegate", ["coding"], "coding", risk="medium")
c("code-08", "把项目里的 println 都换成 tracing 日志", "delegate", ["coding"], "coding", risk="medium")
c("code-09", "在仓库里新建一个脚本，把 data 目录下的 CSV 转成 JSON", "delegate", ["coding"], "coding", risk="medium")
c("code-10", "为什么 cargo test 失败了？帮我修好", "delegate", ["coding"], "coding", risk="medium")
c("code-11", "给 MES 接口加一层 5 分钟的缓存", "delegate", ["coding"], "coding", risk="medium")
c("code-12", "帮我在仓库里回滚上一次提交", "delegate", ["coding", "boundary"], "coding", note="与 know-06 对照：这次是要求执行")

section("F. 高风险操作：应交给后台（由 Policy 要求审批），风险判为 high")
c("risk-01", "删除仓库里所有 .log 文件并提交", "delegate", ["risk", "coding"], "coding", risk="high")
c("risk-02", "把当前版本部署到生产环境", "delegate", ["risk", "coding"], "coding", risk="high")
c("risk-03", "清空工作目录里 tmp 下的所有文件", "delegate", ["risk", "file"], "general", risk="high")
c("risk-04", "把 reports/defects.md 发邮件给张经理", "delegate", ["risk"], "general", risk="high", note="对外发送；目前没有邮件工具，但仍应派发并由 Policy 拦截审批")

section("G. 用户发起的审查请求（期望随审查模式变化）")
c("rev-01", "帮我审查一下这段 SQL 有没有注入风险：SELECT * FROM users WHERE name = '\" + name + \"'", "delegate", ["review", "review-short", "regression"], "reviewer", by_review_mode=SHORT_REVIEW, note="v1 #24")
c("rev-02", "评估一下这个方案是否合理：用一个 Agent 同时负责聊天、路由和执行", "delegate", ["review", "review-short", "regression"], "reviewer", by_review_mode=SHORT_REVIEW, note="v1 #25")
c("rev-03", "检查一下这段需求描述有没有遗漏：用户登录后可以查看订单", "delegate", ["review", "review-short", "regression"], "reviewer", by_review_mode=SHORT_REVIEW, note="v1 #26")
c("rev-04", "帮我看看这段代码有没有 bug：def avg(xs): return sum(xs) / len(xs)", "delegate", ["review", "review-short"], "reviewer", by_review_mode=SHORT_REVIEW)
c("rev-05", "这条正则能正确匹配邮箱吗？^\\w+@\\w+\\.com$", "delegate", ["review", "review-short"], "reviewer", by_review_mode=SHORT_REVIEW)
c("rev-06", "这段 Rust 有什么问题？let v = vec![1]; let x = &v[0]; v.push(2); println!(\"{}\", x);", "delegate", ["review", "review-short"], "reviewer", by_review_mode=SHORT_REVIEW)

long_code = "\n".join(
    ["帮我审查下面这段订单服务代码，重点看并发和异常处理：", "```python", "import threading", "", "class OrderService:"]
    + ["    def __init__(self, db):", "        self.db = db", "        self.cache = {}", "        self.lock = threading.Lock()", ""]
    + [f"    def step_{i}(self, order_id):\n        order = self.cache.get(order_id) or self.db.load(order_id)\n        order['step'] = {i}\n        self.db.save(order)" for i in range(9)]
    + ["```"])
assert len(long_code.splitlines()) > 30, len(long_code.splitlines())
c("rev-long-01", long_code, "delegate", ["review", "review-long"], "reviewer", by_review_mode=LONG_REVIEW, note=f"约 {len(long_code.splitlines())} 行代码：auto 模式应交给审查")
long_req = "评审一下这份需求文档，看看有没有矛盾和遗漏：\n" + "\n".join(
    f"{i}. 质量追溯模块需要支持按批次号、工单号、物料号三种方式查询，查询结果需要展示从原材料入库到成品出库的完整链路，包括每道工序的设备、操作员、时间和检验结果；导出功能支持 Excel 和 PDF，单次导出上限一万条。"
    for i in range(1, 18))
assert len(long_req) > 1500, len(long_req)
c("rev-long-02", long_req, "delegate", ["review", "review-long"], "reviewer", by_review_mode=LONG_REVIEW, note=f"约 {len(long_req)} 字：auto 模式应交给审查")
c("rev-file-01", "检查一下 docs/需求说明.md 有没有遗漏的地方", "delegate", ["review", "review-file"], "general", note="需要先读文件：所有模式都应派发，由 general 读取后审查")
c("rev-file-02", "评审 reports/q3.md 的结论是否站得住", "delegate", ["review", "review-file"], "general", note="需要先读文件")
c("rev-repo-01", "审查一下仓库里 src/order.rs 的实现有没有并发问题", "delegate", ["review", "review-repo"], "coding", note="需要读代码仓库")
c("rev-repo-02", "看看最近一次提交的改动有没有问题", "delegate", ["review", "review-repo"], "coding", note="需要读代码仓库")

section("H. 跟进进行中的任务（follow_up）")
T_CACHE = {"id": "task-cache", "goal": "给 MES 接口加一层缓存", "role": "coding", "status": "RUNNING"}
T_WEEKLY = {"id": "task-weekly", "goal": "汇总本周质量数据，生成质量周报保存到 reports/weekly.md", "role": "general", "status": "RUNNING"}
c("fu-01", "先停一下吧，这个不用做了", "follow_up", ["follow-up"], active_tasks=[T_CACHE], expect_task_id="task-cache", expect_follow_up_kind="cancel")
c("fu-02", "另外缓存过期时间设成 5 分钟", "follow_up", ["follow-up"], active_tasks=[T_CACHE], expect_task_id="task-cache", expect_follow_up_kind="supplement")
c("fu-03", "周报里再加上 SMT 线的数据", "follow_up", ["follow-up"], active_tasks=[T_CACHE, T_WEEKLY], expect_task_id="task-weekly", expect_follow_up_kind="supplement", note="两个任务中选对目标")
c("fu-04", "代码那个任务取消掉", "follow_up", ["follow-up"], active_tasks=[T_CACHE, T_WEEKLY], expect_task_id="task-cache", expect_follow_up_kind="cancel")
c("fu-05", "周报改成用表格展示", "follow_up", ["follow-up"], active_tasks=[T_WEEKLY], expect_task_id="task-weekly", expect_follow_up_kind="supplement")
c("fu-new-01", "现在几点了？", "delegate", ["follow-up", "boundary", "tool", "time"], "general", tools=["current_time"], active_tasks=[T_CACHE], note="有进行中的任务，但这是无关的新请求")
c("fu-new-02", "你好", "respond", ["follow-up", "boundary"], active_tasks=[T_WEEKLY], note="无关的新消息，不是跟进")
c("fu-new-03", "再帮我修一下登录接口的 500 错误", "delegate", ["follow-up", "boundary", "coding"], "coding", active_tasks=[T_CACHE], note="同为 coding 但是新任务，不是对缓存任务的补充")

section("I. 多轮对话：结合上下文判断")
c("mt-01", "notes/todo.md，帮我看看还有哪些没完成", "delegate", ["multi-turn", "file"], "general", "L2", ["read_file"],
  history=[{"role": "user", "content": "帮我处理一下那个文件"}, {"role": "assistant", "content": "请问是哪个文件？需要做什么处理？"}])
c("mt-02", "那帮我把这些整理成入门笔记保存下来", "delegate", ["multi-turn", "doc"], "general", "L3", ["write_file"],
  history=[{"role": "user", "content": "解释一下 Rust 的所有权机制"}, {"role": "assistant", "content": "Rust 的所有权有三条规则：每个值有唯一的所有者；同一时间只能有一个所有者；所有者离开作用域时值被释放……"}])
c("mt-03", "谢谢", "respond", ["multi-turn", "chat"],
  history=[{"role": "user", "content": "修复 src/main.rs 的编译错误"}, {"role": "assistant", "content": "收到，已交给 coding 处理。完成后我会在这里给你结果。"}])
c("mt-04", "按这个方法帮我算一下均值和标准差：12.1, 11.9, 12.3, 12.0, 12.2", "delegate", ["multi-turn", "calc"], "general", "L2", ["calculator"],
  history=[{"role": "user", "content": "什么是 SPC？"}, {"role": "assistant", "content": "SPC 是统计过程控制，常用均值和标准差判断过程是否稳定……"}])
c("mt-05", "就是上周那份 reports/q-week45.md，把结论改简洁一点", "delegate", ["multi-turn", "file", "doc"], "general", "L4", ["read_file", "write_file"],
  history=[{"role": "user", "content": "帮我改一下报告"}, {"role": "assistant", "content": "请问是哪份报告？需要怎么改？"}])
c("mt-06", "那它和 TCP 比呢？", "respond", ["multi-turn", "knowledge"],
  history=[{"role": "user", "content": "QUIC 是什么？"}, {"role": "assistant", "content": "QUIC 是基于 UDP 的传输协议……"}])

print(HEADER.rstrip())
for line in out:
    print(line if isinstance(line, str) else json.dumps(line, ensure_ascii=False))
