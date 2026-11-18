"""可逆匿名化 / Token Vault 测试（对应方案第 5 节）。"""
import sqlite3
from contextlib import closing
import time

from agentnexus_dlp.models import EvaluateRequest
from agentnexus_dlp.service import EgressService, ServiceConfig
from bench.fake_secrets import STRIPE_LIVE  # noqa: E402
from tests.conftest import TEST_DESTINATIONS, TEST_MASTER_DATA, TEST_VAULT_KEY

TEXT = "将西岸三厂的CNC-07维护记录上传，关联WO-559120，客户青川重工"


def evaluate(service, task_id="task-1", tenant_id="tenant-a", text=TEXT):
    return service.evaluate(EvaluateRequest(
        tenant_id=tenant_id, actor_id="actor-1", task_id=task_id, channel="external_api",
        destination_id="vendor-api", purpose="vendor_diagnostics", text=text))


def test_restore_in_same_scope_recovers_original(service):
    r = evaluate(service)
    back = service.restore("tenant-a", "actor-1", "task-1", r.sanitized_text, context="local_ui")
    assert back.text == TEXT
    assert back.restored == 3


def test_cross_task_and_cross_tenant_restore_refused(service):
    r = evaluate(service)
    other_task = service.restore("tenant-a", "actor-1", "task-2", r.sanitized_text, context="local_ui")
    other_tenant = service.restore("tenant-b", "actor-1", "task-1", r.sanitized_text, context="local_ui")
    assert other_task.text == r.sanitized_text and other_task.restored == 0
    assert other_tenant.text == r.sanitized_text and other_tenant.restored == 0


def test_other_actor_cannot_restore(service):
    r = evaluate(service)
    back = service.restore("tenant-a", "actor-2", "task-1", r.sanitized_text, context="local_ui")
    assert back.restored == 0


def test_disallowed_context_is_refused(service):
    r = evaluate(service)
    back = service.restore("tenant-a", "actor-1", "task-1", r.sanitized_text, context="external_api")
    assert back.restored == 0


def test_injected_unknown_token_left_untouched(service):
    evaluate(service)
    text = "外部响应：[CUSTOMER_009] 与 [CUSTOMER_001]"
    back = service.restore("tenant-a", "actor-1", "task-1", text, context="local_ui")
    assert "[CUSTOMER_009]" in back.text
    assert "[CUSTOMER_009]" in back.unknown_tokens
    assert "青川重工" in back.text


def test_different_tasks_get_independent_token_spaces(service):
    a = evaluate(service, task_id="t-a")
    b = evaluate(service, task_id="t-b", text="客户Harborview Dynamics与青川重工")
    assert "[CUSTOMER_001]" in a.sanitized_text
    # 在任务 b 中青川重工是第二个客户，编号独立于任务 a
    assert "[CUSTOMER_002]" in b.sanitized_text


def test_revoke_prevents_restore(service):
    r = evaluate(service)
    assert service.revoke("tenant-a", "task-1") >= 3
    back = service.restore("tenant-a", "actor-1", "task-1", r.sanitized_text, context="local_ui")
    assert back.restored == 0


def test_ttl_expiry(tmp_path):
    svc = EgressService(ServiceConfig(
        destinations=TEST_DESTINATIONS, master_data=TEST_MASTER_DATA,
        vault_path=tmp_path / "v.sqlite3", vault_key=TEST_VAULT_KEY, token_ttl_seconds=1))
    try:
        r = evaluate(svc)
        time.sleep(1.2)
        back = svc.restore("tenant-a", "actor-1", "task-1", r.sanitized_text, context="local_ui")
        assert back.restored == 0
    finally:
        svc.close()


def test_vault_file_holds_no_plaintext(service, vault_path):
    evaluate(service)
    raw = vault_path.read_bytes()
    for value in ("青川重工", "WO-559120", "西岸三厂"):
        assert value.encode() not in raw
    # 也确认没有通过任何列以明文存储
    with closing(sqlite3.connect(vault_path)) as conn:
        dump = "\n".join(conn.iterdump())
    assert "青川重工" not in dump


def test_secrets_are_never_stored(service, vault_path):
    service.evaluate(EvaluateRequest(
        tenant_id="tenant-a", actor_id="actor-1", task_id="t-s", channel="external_api",
        destination_id="vendor-api", purpose="x", text=f"Authorization: Bearer {STRIPE_LIVE}"))
    with closing(sqlite3.connect(vault_path)) as conn:
        n = conn.execute("select count(*) from mappings where entity_type='SECRET'").fetchone()[0]
    assert n == 0


def test_mapping_survives_restart(tmp_path):
    cfg = ServiceConfig(destinations=TEST_DESTINATIONS, master_data=TEST_MASTER_DATA,
                        vault_path=tmp_path / "v.sqlite3", vault_key=TEST_VAULT_KEY)
    svc = EgressService(cfg)
    r = evaluate(svc)
    svc.close()
    svc2 = EgressService(cfg)
    try:
        assert svc2.restore("tenant-a", "actor-1", "task-1", r.sanitized_text, "local_ui").text == TEXT
        # 重启后同任务再次评估同一文本，Token 编号保持一致（幂等）
        assert evaluate(svc2).sanitized_text == r.sanitized_text
    finally:
        svc2.close()
