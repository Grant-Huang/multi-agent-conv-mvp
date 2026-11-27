"""ci_path_triage.py 的测试：路径命中、L0 跳过、共享全量、fail-safe 回退。"""

import ci_path_triage as ct

ALL_JOBS = {"rust", "web", "python-websearch", "python-dlp-egress", "python-tools"}


def _run(files, event="pull_request"):
    result, _ = ct.triage(files, event)
    return result


def test_docs_only_pr_skips_all_code_jobs():
    # 纯文档 PR：全部代码作业跳过（T1 <3 分钟目标）。
    result = _run(["docs/review/2026-11-26.md", "README.md"])
    assert result == {job: False for job in ALL_JOBS}


def test_evals_data_is_l0():
    # 评测数据同属 L0。
    result = _run(["evals/fixtures/routing.jsonl"])
    assert not any(result.values())


def test_rust_change_only_runs_rust():
    result = _run(["crates/runtime/src/store.rs"])
    assert result["rust"] is True
    assert result["web"] is False


def test_cargo_lock_runs_rust():
    # Cargo.lock 变更必须触发 rust（依赖升级）。
    assert _run(["Cargo.lock"])["rust"] is True


def test_web_change_does_not_run_rust():
    # 只改 web/ 不跑 rust（#432 验收项）。
    result = _run(["web/src/desktop/WorkspaceComposer.tsx"])
    assert result["web"] is True
    assert result["rust"] is False


def test_docs_testing_triggers_web_for_test_map_gate():
    # features.json / 用例登记变化必须过 test:map（web 作业），否则地图门失效。
    assert _run(["docs/testing/features.json"])["web"] is True


def test_bindings_trigger_both_rust_and_web():
    # bindings 由 an-core 生成、web 构建消费：两侧都要跑。
    result = _run(["web/src/bindings/AppBindings.d.ts"])
    assert result["rust"] is True
    assert result["web"] is True


def test_python_tool_dirs_map_to_their_jobs():
    assert _run(["tools/websearch/x.py"])["python-websearch"] is True
    assert _run(["tools/dlp-egress/y.py"])["python-dlp-egress"] is True
    assert _run(["tools/ci/zz.py"])["python-tools"] is True
    # 改 websearch 不应带动 dlp-egress。
    result = _run(["tools/websearch/x.py"])
    assert result["python-dlp-egress"] is False


def test_shared_paths_force_full_run():
    for path in (".github/workflows/ci.yml", "config/risk-policy.toml", "fixtures/seed.json"):
        assert all(_run([path]).values()), path


def test_include_str_docs_trigger_rust():
    # R-20261127-M3：docs/mom 与 web/mom 的「文档形态」文件被 include_str! 编进 rust 二进制，
    # 改它们必须跑 rust，否则测试面失守（MockRegister 条目数断言在 tests/mom.rs）。
    # web/mom/ 同时命中 web/ 前缀 → 两作业都跑（fail-safe 方向，可接受）。
    result = _run(["docs/mom/MOCK_REGISTER.md", "web/mom/index.html"])
    assert result["rust"] is True
    assert _run(["docs/mom/MOCK_REGISTER.md"])["web"] is False


def test_experience_map_and_product_trigger_web_for_test_map():
    # R-20261127-M3：journeys.json 与 PRODUCT.md 被 test:map 读取，改动必须过 web 作业。
    result = _run(["docs/experience-map/journeys.json", "docs/PRODUCT.md"])
    assert result["web"] is True
    assert result["rust"] is False


def test_dot_prefixed_dir_hits_shared_not_unknown():
    # .github/ 必须以「共享路径」命中，而不是靠「未登记→全量」兜底：
    # 两条路结果相同，但理由不同——退化说明前缀匹配有缺陷（回归护栏）。
    _, reasons = ct.triage([".github/workflows/ci.yml"], "pull_request")
    assert any("共享路径" in r for r in reasons), reasons
    assert not any("未登记" in r for r in reasons), reasons


def test_unknown_root_file_falls_back_to_full():
    # 未登记的新目录/新文件类型不能被静默忽略。
    assert all(_run(["brand_new_dir/thing.bin"]).values())


def test_push_event_always_full():
    # master push 门禁不弱化。
    assert all(_run(["docs/any.md"], event="push").values())


def test_empty_file_list_skips_all():
    # 空 diff（workflow 已另行兜底）→ 无代码作业。
    assert _run([]) == {job: False for job in ALL_JOBS}


def test_none_files_falls_back_to_full():
    # 输入异常（None）→ 全量，绝不漏跑。
    assert all(ct.triage(None, "pull_request")[0].values())


def test_main_crash_falls_back_to_full(monkeypatch, tmp_path):
    # main() 兜底：triage 抛异常时输出仍为全量且退出码 0。
    import json

    monkeypatch.setattr(ct, "triage", lambda *a, **k: (_ for _ in ()).throw(RuntimeError("boom")))
    out = tmp_path / "triage.json"
    rc = ct.main(["--event", "pull_request", "--json-out", str(out), "--quiet"])
    assert rc == 0
    assert json.loads(out.read_text()) == {job: True for job in ALL_JOBS}
