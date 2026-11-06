//! 运行时集成用例 RT-TPL-10（定义见 docs/testing/CASES.md）：后台容量由后端配置换算。

use an_llm::{BackendConfig, BackendKind};

#[test]
fn rt_tpl_10_capacity_follows_backend_config() {
    // llama_cpp：后台槽位个数
    let local = BackendConfig {
        kind: BackendKind::LlamaCpp,
        slots: 6,
        interactive_slots: vec![0, 1],
        background_slots: vec![2, 3, 4, 5],
        ..Default::default()
    };
    assert_eq!(local.background_capacity(), Some(4));

    // openai：并发上限减去前台预留
    let cloud = BackendConfig {
        kind: BackendKind::Openai,
        max_inflight: 8,
        reserved_interactive: 1,
        ..Default::default()
    };
    assert_eq!(cloud.background_capacity(), Some(7));

    // openai 不限并发：None
    let unlimited = BackendConfig {
        kind: BackendKind::Openai,
        max_inflight: 0,
        ..Default::default()
    };
    assert_eq!(unlimited.background_capacity(), None);
}
