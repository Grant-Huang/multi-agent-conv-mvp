//! 编译 data-collection 导出的专家工作流，输出绑定卡、工作流模板和晋升统计。
//!
//! 用法：cargo run -p an-runtime --example promote_expert -- [输入 JSON] [输出目录]
//! 默认输入 fixtures/mom/expert/workflows.json，默认输出到标准输出（只打印统计）。

use an_runtime::mom::binding::{compile, promotion_report, ExpertWorkflow};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let input = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "fixtures/mom/expert/workflows.json".into());
    let workflows: Vec<ExpertWorkflow> = serde_json::from_str(&std::fs::read_to_string(&input)?)?;
    let report = promotion_report(&workflows);
    if let Some(dir) = args.get(2) {
        let dir = std::path::Path::new(dir);
        std::fs::create_dir_all(dir)?;
        let templates: Vec<_> = workflows.iter().map(|w| compile(w, false)).collect();
        std::fs::write(
            dir.join("templates.json"),
            serde_json::to_string_pretty(&templates)?,
        )?;
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_string_pretty(&report)?,
        )?;
        eprintln!("已写入 {}", dir.display());
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
