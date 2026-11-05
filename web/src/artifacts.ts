/** 从 unified diff 里还原任务改过的文件，方便直接打开，而不是只看到 diff 文本 */
export function filesFromDiff(diff: string): { name: string; content: string }[] {
  const out: { name: string; content: string }[] = [];
  let name = "";
  let lines: string[] = [];
  let inHunk = false;
  const flush = () => {
    if (name && lines.some((l) => l.trim())) out.push({ name, content: lines.join("\n") });
    name = "";
    lines = [];
    inHunk = false;
  };
  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git ")) {
      flush();
      name = line.match(/ b\/(.+)$/)?.[1] ?? "";
      continue;
    }
    if (line.startsWith("@@")) {
      inHunk = true;
      continue;
    }
    if (!inHunk || !name) continue;
    if (line.startsWith("+") && !line.startsWith("+++")) lines.push(line.slice(1));
    else if (line.startsWith(" ")) lines.push(line.slice(1));
  }
  flush();
  return out.slice(0, 20);
}

export type ViewFile = { name: string; content: string; label: string };

/** 对话里能打开的产物：优先真实文件，否则从 diff 还原，都没有时用执行输出 */
export function viewFiles(
  artifacts: { kind: string; name: string; content: string }[],
  summary: string,
): ViewFile[] {
  const files = artifacts
    .filter((a) => a.kind === "file")
    .map((a) => ({ name: a.name, content: a.content, label: a.name }));
  const diffs = artifacts.filter((a) => a.kind === "diff");
  const recovered = files.length
    ? []
    : diffs.flatMap((d) => filesFromDiff(d.content).map((f) => ({ name: f.name, content: f.content, label: f.name })));
  const diffRows = diffs.map((d) => ({ name: "changes.diff", content: d.content, label: "改动（git diff）" }));
  const rows = [...files, ...recovered, ...diffRows];
  if (rows.length > 0) return rows;
  const text = summary.trim();
  if (!text) return [];
  const html = /<[a-z!][^>]*>/i.test(text);
  return [{ name: html ? "output.html" : "output.txt", content: text, label: html ? "本次输出.html" : "本次输出" }];
}

/** 把任务产物下载到本地，或在新标签页打开（HTML 按页面打开，其余按文本） */
export function downloadArtifact(name: string, content: string) {
  const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name.split("/").pop() || "artifact.txt";
  a.click();
  URL.revokeObjectURL(url);
}

export function openArtifact(name: string, content: string) {
  const lower = name.toLowerCase();
  const html = lower.endsWith(".html") || lower.endsWith(".htm");
  const blob = new Blob([content], { type: `${html ? "text/html" : "text/plain"};charset=utf-8` });
  const url = URL.createObjectURL(blob);
  window.open(url, "_blank", "noopener");
}
