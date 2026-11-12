# R-FIRST-06 Git 索引隔离证据

日期：2026-11-12（America/Vancouver）。基线：2bae733。用户在本会话明确同意私有临时索引方案，授权修改 util.rs 的 Git 操作。

## Red → Green

`cargo test --locked -p an-agents --test git_evidence`

原实现：`FAILED. 0 passed; 4 failed`。RT-GIT-01/03 真实索引字节被修改，RT-GIT-02 创建真实索引，RT-GIT-04 失败未报告证据不可用。

修复后：`ok. 5 passed; 0 failed`。同一四个业务断言未改；另补 RT-GIT-05，运行真实 Git clean filter 后取消，确认进程退出、私有索引及 lock 目录消失且真实索引不变。补充取消测试沿用仓库 test_proc::alive 的进程状态口径：Z 表示已退出，容器 init 延迟回收不视为仍在执行；原四条 Red 业务断言未改。

## 生命周期与边界

每条命令30秒超时；取消信号交给持有索引 Arc 的清理任务，终止并回收自身进程组后释放目录，复用 proc_group 的有界 TERM/KILL 策略。并发采集各自私有目录，不写回用户索引。Windows 沿用仅直接子进程清理的限制。Git 命令失败不回退真实索引，明确报告不可用。

兜底保留原有相对索引的差异语义，不承诺包含仅已暂存的全部 HEAD 差异；任务隔离（R-FIRST-07）不在本修复范围。未验证真实 ACP/远端节点或浏览器；Git 操作使用受控临时仓库。

## 完整检查（本轮实际输出）

```text
cargo fmt --all -- --check：exit=0
cargo test --locked：392 passed; 0 failed; 2 原有 ignored；exit=0
cargo clippy --locked --all-targets -- -D warnings：Finished dev profile，0 warning；exit=0
web npm ci：0 vulnerabilities；exit=0
web npm test：21 passed；exit=0
web npm run build：built in 164ms；exit=0
web npm run test:map：Feature map valid: 16 features, 14 browser cases；exit=0
```

Rust设置CARGO_TARGET_DIR=target（工作区target链接）、CARGO_INCREMENTAL=0、DEV/TEST_DEBUG=0以控制磁盘，不跳过测试或关闭检查。早期两工作区共用target造成旧产物复用及rustdoc缺库；已强制重新编译、串行完成以上检查，早期失败不计为通过。本地结果不代替GitHub CI。
