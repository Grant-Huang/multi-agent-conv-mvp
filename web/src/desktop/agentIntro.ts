/** AG.3 发现卡的「已看过」标记（按浏览器记录）；T-88 起卡片移到设置-连接与能力。 */
const AGENTS_INTRO_KEY = "an.agentsIntroSeen";

export function readAgentsIntroSeen(): boolean {
  try {
    return localStorage.getItem(AGENTS_INTRO_KEY) === "1";
  } catch {
    // 读不到本地存储（隐私窗口等）时不打扰用户
    return true;
  }
}

export function writeAgentsIntroSeen() {
  try {
    localStorage.setItem(AGENTS_INTRO_KEY, "1");
  } catch {
    /* 忽略：只是界面偏好 */
  }
}
