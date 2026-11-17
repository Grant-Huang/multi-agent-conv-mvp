"""detect-secrets 封装：把面向代码仓库的扫描器改造成对任意文本做实时凭证检测。

- 关闭 Base64/Hex 高熵插件：它们在自然语言上误报极多（实测会把 "Authorization" 判为高熵串）。
  高熵兜底由 recognizers.SECRET_RULES 中的结构化规则（Authorization 头、key=value）承担。
- 保留各类“已知格式”插件（AWS、GitHub、JWT、Slack、Stripe、私钥、关键字等）。
- detect-secrets 只返回密钥值，不返回偏移；这里在该行内查找值的位置得到 span。
"""
from __future__ import annotations

import re
import threading

from detect_secrets.core.scan import scan_line
from detect_secrets.settings import default_settings, get_settings
from presidio_analyzer import EntityRecognizer, RecognizerResult

from .recognizers import LANG

# 自然语言场景中误报过高的插件
_DISABLED_PLUGINS = {"Base64HighEntropyString", "HexHighEntropyString"}
# 凭证字符串允许的字符（用于把插件返回的前缀扩展为完整 span）
_TOKEN_CHAR = re.compile(r"[A-Za-z0-9_\-./+=]")
# detect-secrets 的 settings 是进程全局状态，扫描时加锁
_LOCK = threading.Lock()


class DetectSecretsRecognizer(EntityRecognizer):
    def __init__(self):
        super().__init__(supported_entities=["SECRET"], name="DetectSecretsRecognizer", supported_language=LANG)

    def load(self) -> None:
        pass

    def analyze(self, text, entities, nlp_artifacts=None):
        if entities and "SECRET" not in entities:
            return []
        results = []
        with _LOCK, default_settings():
            settings = get_settings()
            for name in list(settings.plugins):
                if name in _DISABLED_PLUGINS:
                    settings.disable_plugins(name)
            offset = 0
            for line in text.splitlines(keepends=True):
                for secret in scan_line(line):
                    value = secret.secret_value
                    if not value:
                        continue
                    pos = line.find(value)
                    while pos >= 0:
                        # 部分插件（如 GitHub Token）只返回前缀 "ghp"，这里把 span 扩展到完整的凭证字符串，
                        # 否则会出现“检测到了却只遮住前缀”的漏放（留出集 HO-020 回归）
                        end = pos + len(value)
                        while end < len(line) and _TOKEN_CHAR.match(line[end]):
                            end += 1
                        if end - pos >= 6:
                            results.append(RecognizerResult(
                                "SECRET", offset + pos, offset + end, 0.9,
                                recognition_metadata={"recognizer_name": self.name, "source": "detect_secrets",
                                                      "plugin": secret.type}))
                        pos = line.find(value, pos + 1)
                offset += len(line)
        return results
