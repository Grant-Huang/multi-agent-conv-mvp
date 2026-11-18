"""R-20261118-M3：常见凭证写法、同形字与原文偏移的端到端回归。"""
import json

import pytest

from agentnexus_dlp.models import EvaluateRequest
from bench.fake_secrets import AWS_DOC_EXAMPLE, GITHUB_A

SYNTH_PASSWORD = "Rivulet" + "7!Cedar5"
OPENAI_PROJECT = "sk" + "-proj-" + "AbCd0123_" * 5
GOOGLE_KEY = "AI" + "za" + "Sy" + ("AbCd9E" * 6)[:33]

CASES = [
    ("密码：" + SYNTH_PASSWORD, SYNTH_PASSWORD),
    ("数据库口令 " + SYNTH_PASSWORD, SYNTH_PASSWORD),
    ("token=" + SYNTH_PASSWORD, SYNTH_PASSWORD),
    ("DB_PASS=" + SYNTH_PASSWORD, SYNTH_PASSWORD),
    (OPENAI_PROJECT, OPENAI_PROJECT),
    (GOOGLE_KEY, GOOGLE_KEY),
    (AWS_DOC_EXAMPLE.replace("K", "\u041a", 1), AWS_DOC_EXAMPLE.replace("K", "\u041a", 1)),
    (GITHUB_A.replace("a", "\u0430", 1), GITHUB_A.replace("a", "\u0430", 1)),
]


def request(text):
    return EvaluateRequest("tenant-a", "actor-1", "review-m3", "email", "vendor-api",
                           "vendor_diagnostics", text=text)


@pytest.mark.parametrize("fragment,secret", CASES, ids=[
    "chinese-password", "chinese-passphrase", "token-assignment", "environment-password",
    "openai-project", "google-api", "cyrillic-aws-prefix", "cyrillic-github-body"])
def test_dlp_m3_common_and_confusable_credentials_are_blocked(service, fragment, secret):
    result = service.evaluate(request("请联系供应商：" + fragment + "；温度37℃"))
    assert result.decision == "BLOCK"
    assert result.ready_to_send is False
    assert secret not in json.dumps(result.to_dict(), ensure_ascii=False)
    assert result.sanitized_text.startswith("请联系供应商：")
    assert result.sanitized_text.endswith("；温度37℃")


def test_dlp_m3_noncredential_unicode_and_terminology_stay_unchanged(service):
    text = "Кирилл；token count = 12；密码策略说明；温度37℃"
    result = service.evaluate(request(text))
    assert result.decision == "ALLOW"
    assert result.sanitized_text == text
