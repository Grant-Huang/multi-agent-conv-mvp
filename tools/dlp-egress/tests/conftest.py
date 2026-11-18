"""共享测试夹具：构造一个使用临时 Vault 的外发脱敏服务。

所有测试都只用合成数据，不访问网络，不加载 GLiNER 模型。
"""
from pathlib import Path

import pytest

from agentnexus_dlp.service import EgressService, ServiceConfig
from bench import fake_secrets

# 测试用租户主数据：模拟企业 CRM / MES 中维护的客户、项目、厂区名单。
# 这些名字是专门为单元测试编的，与 700 条评测集不同，避免单元测试依赖评测集词汇。
TEST_MASTER_DATA = {
    "CUSTOMER": ["青川重工", "Harborview Dynamics"],
    "PROJECT": ["项目白鹭", "Project Kestrel"],
    "PLANT": ["西岸三厂", "Plant North"],
    "PERSON": ["吴主管", "Riley Morgan"],
}

# 已登记的目的地。external_untrusted = 已登记但按不可信对待（可脱敏后外发）；
# 未出现在此表里的目的地一律视为 unknown，直接 BLOCK。
TEST_DESTINATIONS = {
    "vendor-api": {"trust": "external_untrusted"},
    "vendor-mail": {"trust": "external_untrusted"},
    "internal-wiki": {"trust": "internal"},
}

# 测试用 Vault 主密钥（运行时生成，见 bench/fake_secrets.py）。只用于测试，绝不能用于生产。
TEST_VAULT_KEY = fake_secrets.TEST_VAULT_KEY


@pytest.fixture
def vault_path(tmp_path: Path) -> Path:
    return tmp_path / "vault.sqlite3"


@pytest.fixture
def service(vault_path: Path) -> EgressService:
    svc = EgressService(ServiceConfig(
        destinations=TEST_DESTINATIONS,
        master_data=TEST_MASTER_DATA,
        vault_path=vault_path,
        vault_key=TEST_VAULT_KEY,
    ))
    yield svc
    svc.close()
