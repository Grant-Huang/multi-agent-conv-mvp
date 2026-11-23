//! WP4：内置三张模板（DESIGN §9.6），供 Runtime 启动时装入 TemplateRegistry。

use crate::template::{Template, TemplateRegistry};

const QUOTE: &str = r#"{
  "id": "quote",
  "deliverable": "报价单",
  "required_slots": ["序列号或型号", "交期"],
  "branches": [
    { "id": "damage", "capability": "general",
      "serial_steps": ["读测量值", "对照修复限度", "给出可修结论和工时"],
      "output_required": ["verdict", "hours"] },
    { "id": "parts", "capability": "general",
      "serial_steps": ["取原厂清单", "对库存和替代件", "算缺件金额"],
      "output_required": ["shortage_amount"] },
    { "id": "history", "capability": "general",
      "serial_steps": ["查本机维修", "查同型号返修", "给保修风险"],
      "output_required": ["warranty_risk"] }
  ],
  "join": { "capability": "general", "writes_file": true }
}"#;

const STOP_LINE: &str = r#"{
  "id": "stop_line",
  "deliverable": "停线清单",
  "required_slots": ["批次或线体"],
  "branches": [
    { "id": "unit_a", "capability": "general",
      "serial_steps": ["铭牌", "检测", "出厂参数", "超差", "处置"],
      "output_required": ["disposition"] },
    { "id": "unit_b", "capability": "general",
      "serial_steps": ["铭牌", "检测", "出厂参数", "超差", "处置"],
      "output_required": ["disposition"] }
  ],
  "join": { "capability": "general", "writes_file": true }
}"#;

const RELEASE: &str = r#"{
  "id": "release",
  "deliverable": "放行单",
  "required_slots": ["批次号"],
  "branches": [
    { "id": "test", "capability": "general",
      "serial_steps": ["循环结束", "读曲线"],
      "output_required": ["curve_ok"] },
    { "id": "compliance", "capability": "general",
      "serial_steps": ["批次号", "材质证明"],
      "output_required": ["material_cert"] },
    { "id": "customer", "capability": "general",
      "serial_steps": ["条款列表", "对现场记录"],
      "output_required": ["clause_ok"] }
  ],
  "join": { "capability": "general", "writes_file": true }
}"#;

/// 加载内置模板；`max_branches` 通常取后台槽位数（至少 4）。
pub fn builtin_registry(max_branches: usize) -> Result<TemplateRegistry, Vec<String>> {
    let cap = max_branches.max(4);
    let templates = [QUOTE, STOP_LINE, RELEASE]
        .into_iter()
        .map(|s| serde_json::from_str::<Template>(s).expect("内置模板 JSON 非法"))
        .collect();
    TemplateRegistry::new(templates, cap)
}
