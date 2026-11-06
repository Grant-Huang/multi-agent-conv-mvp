//! 粗略估算 token 数：中日韩字符约 1 个字 1 个 token，其他字符约 3.5 个字符 1 个 token（偏保守，宁多勿少）。
//! 只用于限流和上下文预算，不追求精确。

use crate::ChatMessage;

pub fn estimate_tokens(text: &str) -> u32 {
    let (mut cjk, mut other) = (0u32, 0u32);
    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + (other as f64 / 3.5).ceil() as u32
}

/// 一组消息的估算 token 数（每条消息额外算 4 个 token 的角色 / 分隔开销）
pub fn estimate_messages(messages: &[ChatMessage]) -> u32 {
    messages
        .iter()
        .map(|m| estimate_tokens(&m.content) + 4)
        .sum()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3000..=0x303F | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xFF00..=0xFFEF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_counts_one_per_char_and_ascii_about_a_third() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("你好世界"), 4);
        assert_eq!(estimate_tokens("abcdefg"), 2); // 7 / 3.5
        assert_eq!(estimate_tokens("abcdefgh"), 3); // 向上取整
        assert_eq!(estimate_tokens("你好 hello"), 2 + 2); // 空格也算在"其他"里：6 / 3.5 → 2
    }

    #[test]
    fn messages_add_per_message_overhead() {
        let m = vec![ChatMessage::system("你好"), ChatMessage::user("世界")];
        assert_eq!(estimate_messages(&m), 2 + 4 + 2 + 4);
    }
}
