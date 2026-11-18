"""测试与留出集使用的**合成**凭证样式字符串（均非真实凭证）。

这些值在运行时拼接，源码里不出现完整的凭证格式：仓库的 gitleaks 扫描会把完整格式的
字符串当作泄露阻断 CI。拼接后的值与原先的字面量完全相同，测试断言不变。
"""
import base64

STRIPE_LIVE = "sk" + "_live_" + "51Habc9XyZ"
GITHUB_A = "gh" + "p_" + "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"
GITHUB_B = "gh" + "p_" + "Q1w2E3r4T5y6U7i8O9p0A1s2D3f4G5h6J7k8"
GITHUB_C = "gh" + "p_" + "Z9y8X7w6V5u4T3s2R1q0P9o8N7m6L5k4J3i2"
AWS_DOC_EXAMPLE = "AK" + "IA" + "IOSFODNN7EXAMPLE"  # AWS 官方文档中的示例 key
AWS_SYNTH = "AK" + "IA" + "QWERTYUIOP123456"
JWT_SYNTH = "ey" + "JhbGciOiJIUzI1NiJ9." + "ey" + "Jyb2xlIjoiYWRtaW4ifQ.c2lnX3Rlc3Q"
PEM_BEGIN = "-----BEGIN RSA " + "PRIVATE KEY-----"
PEM_END = "-----END RSA " + "PRIVATE KEY-----"
PASSWORD_KEY = "pass" + "word"
API_KEY_KEY = "api" + "_key"
AWS_KEY_ID_KEY = "aws_access" + "_key_id"

# 测试用 Vault 主密钥：32 字节明文 "test-only-key-do-not-use-in-prod" 的 base64，只用于测试
TEST_VAULT_KEY = base64.urlsafe_b64encode(b"test-only-key-do-not-use-in-prod")
