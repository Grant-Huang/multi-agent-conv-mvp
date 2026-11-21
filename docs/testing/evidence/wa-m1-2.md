# World Adapter M1-2 — 正式投递水位

分支：`cursor/wa-m1-2-delivery-watermark-f2df`。方案：`docs/mom/WA_M1_2_PLAN.md`。未改 `engine` / `DesConnector`。

## 实现

- ingest → `commit_publish`；测试控件 `abort-before-publish` / `commit-publish`
- Observe：`publish_seq`、`as_of`、`objects` 与已发布事件同边界
- 空 pending commit 不抬水位；`event_id` 去重
- 合同：`RT-WADES-05`、`RT-WADES-06`

## 验证

```text
$ CARGO_TARGET_DIR=target cargo test --locked -p an-server --test mom_world_des -- --nocapture
test result: ok. 6 passed; 0 failed
$ cd web && npm run test:map
Feature map valid: 32 features, 62 browser cases
```
