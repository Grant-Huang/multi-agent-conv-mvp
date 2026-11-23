/** 首页 S1 真实委派：B1 可靠受理 + 等任务落库（不碰编排实现）。 */
import { api } from "../api";

export function newClientOpId(): string {
  return `op-home-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export type HomeAcceptResult = {
  conversationId: string;
  clientOperationId: string;
  requestId: string;
  messageId: string;
  dispatchCount: number;
};

/**
 * 丢回执后只有一次受理：POST 拿到 request_id 后只信 GET；
 * 同 client_operation_id + 同正文再 POST 必须幂等（同一 request_id、dispatch_count=1）。
 */
export async function durableHomeAccept(opts: {
  text: string;
  conversationId?: string;
  clientOperationId?: string;
}): Promise<HomeAcceptResult> {
  const conversationId =
    opts.conversationId ??
    (await api.createConversation(opts.text.trim().slice(0, 24) || "桌面委派")).id;
  const clientOperationId = opts.clientOperationId ?? newClientOpId();
  const created = await api.desktopAcceptCreate(conversationId, {
    client_operation_id: clientOperationId,
    text: opts.text,
  });
  if (!created.ok) {
    throw new Error(created.message || `受理失败 HTTP ${created.status}`);
  }
  const requestId = created.body.request_id;
  // 模拟丢回执：不以 POST body 作为 UX 终态，只靠 GET 确认
  const got = await api.desktopAcceptGet(requestId);
  if (got.status !== "accepted") {
    throw new Error(`受理回读状态异常：${got.status}`);
  }
  const replay = await api.desktopAcceptCreate(conversationId, {
    client_operation_id: clientOperationId,
    text: opts.text,
  });
  if (!replay.ok) {
    throw new Error(replay.message || "幂等复验失败");
  }
  if (replay.body.request_id !== requestId || replay.body.dispatch_count !== 1) {
    throw new Error(
      `幂等复验失败：request=${replay.body.request_id} dispatch=${replay.body.dispatch_count}`,
    );
  }
  return {
    conversationId,
    clientOperationId,
    requestId,
    messageId: got.message_id,
    dispatchCount: replay.body.dispatch_count,
  };
}

/** 等待 continue_after_accepted 分流产出任务（goal 包含片段即可）。 */
export async function waitForConversationTask(
  conversationId: string,
  goalIncludes: string,
  timeoutMs = 60_000,
): Promise<string> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const tasks = await api.tasks(conversationId);
    const hit = tasks.find((t) => t.goal.includes(goalIncludes));
    if (hit) return hit.id;
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`等待任务超时（${goalIncludes}）`);
}

/** 等待 Composer 终态交付写入发起会话（assistant 消息绑定 task_id）。 */
export async function waitForTaskDelivery(
  conversationId: string,
  taskId: string,
  timeoutMs = 60_000,
): Promise<{ id: string; content: string }> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const msgs = await api.messages(conversationId);
    const hit = msgs.find((m) => m.role === "assistant" && m.task_id === taskId);
    if (hit && hit.content.trim()) {
      return { id: hit.id, content: hit.content };
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`等待任务交付超时（${taskId}）`);
}
