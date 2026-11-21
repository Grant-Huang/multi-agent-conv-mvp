# 证据：AI Desktop C1 — 证据 ACL + E2E-ADSOURCES 真实 HTTP

- **日期**：2026-11-20（America/Vancouver）
- **决定**：DECISIONS 69（用户确认；本卡同时上 E2E-ADSOURCES 真实 HTTP）
- **范围**：`desktop_evidence` + `desktop_evidence_grants`；夹具头 `x-desktop-principal`；EvidencePanel 真实 API；不改 `orchestrator.rs`；不做 OAuth/IdP

## 命令与结果

```text
CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_acl --locked
→ 5 passed（RT-ADACL-01～05）

CARGO_TARGET_DIR=target cargo clippy --all-targets --locked
→ Finished（无本卡新增告警）

CARGO_TARGET_DIR=target cargo test -p an-server --test desktop_acl --locked
→ 见上（本卡硬门禁）
说明：`admission::rt_adm_14` 在干净 master 树亦复现「任务未结束」，与本卡无关。

cd web && npm run build
→ tsc + vite build 通过

cd web && npm run test:map
→ Feature map valid: 32 features, 63 browser cases

cd web && npx playwright test e2e/desktop.spec.ts
→ 16 passed（含 E2E-ADEVID、E2E-ADSOURCES）
```

## 断言摘要

| Case | 结果 |
|---|---|
| RT-ADACL-01 | alice 读 summary/source → 200；summary 无正文 |
| RT-ADACL-02 | bob 读 source → 403；body 无原文/摘要 |
| RT-ADACL-03 | 撤权 alice 后各视图 403；列表/搜索不含该条；撤权幂等 |
| RT-ADACL-04 | 撤权 alice 后 carol 仍可读原文 |
| RT-ADACL-05 | expired/revoked → 410 `source_unavailable`；不回正文 |
| E2E-ADSOURCES | bob 无列表/搜索；alice 撤权后拒绝；carol 仍可读原文 |
| E2E-ADEVID | 真实列表下分层预览/失败/过期/空夹具仍绿 |

## 非本卡

- 完整 IdP / OAuth
- C2 动作网关
- 删除 evidenceMock 文件（空列表夹具仍本地）
