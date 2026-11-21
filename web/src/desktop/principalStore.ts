/** 桌面证据 ACL 夹具身份（C1）；对应请求头 x-desktop-principal。 */
const KEY = "an.desktop.principal.v1";

export type DesktopPrincipal = "alice" | "bob" | "carol" | "local";

export function loadPrincipal(): DesktopPrincipal {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === "alice" || raw === "bob" || raw === "carol" || raw === "local") return raw;
  } catch {
    /* ignore */
  }
  return "alice";
}

export function savePrincipal(p: DesktopPrincipal): void {
  try {
    localStorage.setItem(KEY, p);
  } catch {
    /* ignore */
  }
}
