import { downloadArtifact, openArtifact, viewFiles } from "../artifacts";

export function ArtifactList(props: { artifacts: { kind: string; name: string; content: string }[]; summary?: string }) {
  const files = viewFiles(props.artifacts, props.summary ?? "");
  if (files.length === 0) return null;
  return (
    <div className="artifacts">
      {files.map((a, i) => (
        <div className="artifact-row" key={`${a.label}-${i}`}>
          <span className="artifact-name" title={a.label}>{a.label}</span>
          <button type="button" className="btn small" onClick={() => openArtifact(a.name, a.content)}>查看</button>
          <button type="button" className="btn small" onClick={() => downloadArtifact(a.name, a.content)}>下载</button>
        </div>
      ))}
    </div>
  );
}
