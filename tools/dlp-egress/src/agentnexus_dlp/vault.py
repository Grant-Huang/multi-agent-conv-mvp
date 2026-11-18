"""本地 Token Vault：作用域化、加密存储 Token → 原值映射（设计文档第 5 节）。

安全要点：
- 作用域 = (tenant_id, task_id)；不同租户、不同任务的 Token 空间互相独立，不能交叉恢复。
- 原值用 AES-256-GCM 加密，AAD 绑定 (tenant, task, token)，密文挪到别的作用域会解密失败。
- 幂等查找用 HMAC(tenant|task|type|规范值)，数据库里不出现任何明文。
- 主密钥来自配置（生产应由 KMS 托管并做 envelope encryption；MVP 用本地密钥，见 README 局限）。
- 凭证（SECRET）永远不写入 Vault。
"""
from __future__ import annotations

import hashlib
import hmac
import json
import os
import re
import sqlite3
import threading
import time
from pathlib import Path

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF

TOKEN_RE = re.compile(r"\[([A-Z][A-Z_]*?)_(\d{3,})\]")

_SCHEMA = """
CREATE TABLE IF NOT EXISTS mappings (
    tenant_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    value_mac TEXT NOT NULL,
    token TEXT NOT NULL,
    seq INTEGER NOT NULL,
    nonce BLOB,
    ciphertext BLOB,
    owner_actor TEXT NOT NULL,
    allowed_contexts TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    created_at REAL NOT NULL,
    expires_at REAL NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, task_id, token)
);
CREATE INDEX IF NOT EXISTS idx_lookup ON mappings (tenant_id, task_id, entity_type, value_mac);
"""


def _derive(master_key: bytes, info: bytes) -> bytes:
    return HKDF(algorithm=hashes.SHA256(), length=32, salt=None, info=info).derive(master_key)


def decode_key(raw: bytes | str) -> bytes:
    """接受 base64 编码的 32 字节密钥。"""
    import base64
    if isinstance(raw, str):
        raw = raw.encode()
    key = base64.urlsafe_b64decode(raw.strip())
    if len(key) != 32:
        raise ValueError("Vault 密钥必须是 32 字节（base64 编码后 44 个字符）")
    return key


class TokenVault:
    def __init__(self, path: str | Path, master_key: bytes, ttl_seconds: int = 86400):
        self.path = str(path)
        self.ttl = ttl_seconds
        self._enc = AESGCM(_derive(master_key, b"agentnexus-dlp/enc/v1"))
        self._mac_key = _derive(master_key, b"agentnexus-dlp/mac/v1")
        self._lock = threading.Lock()
        Path(self.path).parent.mkdir(parents=True, exist_ok=True)
        self._db = sqlite3.connect(self.path, check_same_thread=False, isolation_level=None)
        self._db.executescript(_SCHEMA)
        try:  # Vault 文件只允许当前用户读写
            os.chmod(self.path, 0o600)
        except OSError:
            pass

    def close(self) -> None:
        self._db.close()

    # ------------------------------------------------------------------
    def _mac(self, tenant: str, task: str, etype: str, canonical_value: str) -> str:
        msg = json.dumps([tenant, task, etype, canonical_value], ensure_ascii=False).encode()
        return hmac.new(self._mac_key, msg, hashlib.sha256).hexdigest()

    @staticmethod
    def _aad(tenant: str, task: str, token: str) -> bytes:
        return json.dumps([tenant, task, token]).encode()

    def get_or_create(self, tenant: str, task: str, etype: str, value: str, canonical_value: str,
                      owner_actor: str, allowed_contexts: list[str], policy_version: str) -> str:
        """返回该作用域内实体的稳定 Token；不存在（或已过期/撤销）则新建。"""
        if etype == "SECRET":
            raise ValueError("凭证不得写入 Vault")
        mac = self._mac(tenant, task, etype, canonical_value)
        now = time.time()
        with self._lock:
            row = self._db.execute(
                "SELECT token FROM mappings WHERE tenant_id=? AND task_id=? AND entity_type=? AND value_mac=?"
                " AND revoked=0 AND expires_at>? ORDER BY seq DESC LIMIT 1",
                (tenant, task, etype, mac, now)).fetchone()
            if row:
                return row[0]
            seq = (self._db.execute(
                "SELECT COALESCE(MAX(seq),0) FROM mappings WHERE tenant_id=? AND task_id=? AND entity_type=?",
                (tenant, task, etype)).fetchone()[0]) + 1
            token = f"[{etype}_{seq:03d}]"
            nonce = os.urandom(12)
            ct = self._enc.encrypt(nonce, value.encode("utf-8"), self._aad(tenant, task, token))
            self._db.execute(
                "INSERT INTO mappings (tenant_id, task_id, entity_type, value_mac, token, seq, nonce, ciphertext,"
                " owner_actor, allowed_contexts, policy_version, created_at, expires_at)"
                " VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (tenant, task, etype, mac, token, seq, nonce, ct, owner_actor, json.dumps(allowed_contexts),
                 policy_version, now, now + self.ttl))
            return token

    def lookup(self, tenant: str, task: str, token: str, actor: str, context: str) -> str | None:
        """按作用域、属主、上下文、有效期校验后解密；任何一项不满足返回 None。"""
        with self._lock:
            row = self._db.execute(
                "SELECT nonce, ciphertext, owner_actor, allowed_contexts, expires_at, revoked FROM mappings"
                " WHERE tenant_id=? AND task_id=? AND token=?", (tenant, task, token)).fetchone()
        if not row:
            return None
        nonce, ct, owner, contexts, expires_at, revoked = row
        if revoked or ct is None or expires_at <= time.time():
            return None
        if owner != actor or context not in json.loads(contexts):
            return None
        return self._enc.decrypt(nonce, ct, self._aad(tenant, task, token)).decode("utf-8")

    def revoke(self, tenant: str, task: str) -> int:
        """撤销该任务的全部映射，并立即擦除密文（不可再恢复）。"""
        with self._lock:
            cur = self._db.execute(
                "UPDATE mappings SET revoked=1, ciphertext=NULL, nonce=NULL WHERE tenant_id=? AND task_id=?"
                " AND revoked=0", (tenant, task))
            self._db.execute("VACUUM")  # 让被擦除的密文不残留在空闲页中
            return cur.rowcount
