/** UXFIX-L（INTENT-018，CH-U4/CH-U5）：任务进度用用户语言展示——
 *  普通模式隐藏原始工具参数（“参数 {…}”），常见工具映射为动作描述；
 *  开发者模式保留原始行（诊断类 E2E 不放松）。 */
const TOOL_LABEL: Record<string, string> = {
  write_file: "写入文件",
  read_file: "读取文件",
  list_dir: "查看目录",
  web_search: "联网搜索",
  run_command: "运行命令",
  send_message: "发送消息",
};

export function userTaskLine(line: string, devMode: boolean): string {
  if (devMode) return line;
  let out = line.replace(
    /调用工具 (\S+)，参数 (\{.*\})/g,
    (_m, tool: string, args: string) => {
      let detail = "";
      const path = args.match(/"path"\s*:\s*"([^"]+)"/);
      if (path) detail = path[1];
      const label = TOOL_LABEL[tool] ?? `执行 ${tool}`;
      return detail ? `${label} ${detail}` : label;
    },
  );
  // 未匹配形态的残留参数串也一并隐藏
  out = out.replace(/，参数 \{[^}]*\}/g, "");
  return out;
}
