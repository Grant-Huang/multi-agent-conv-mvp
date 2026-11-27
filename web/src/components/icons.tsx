/** UXFIX-R（#344）：统一简洁 SVG 线性图标——单色、currentColor 描边、适配深浅色，
 *  内联渲染，不引入 emoji 或图标字体。清单与使用规则见 docs/ai-desktop/DESIGN.md 第 11 节。 */
import type { JSX } from "react";

type IconProps = { size?: number; className?: string };

function base(path: JSX.Element, size = 16, className?: string, viewBox = "0 0 16 16"): JSX.Element {
  return (
    <svg
      width={size}
      height={size}
      viewBox={viewBox}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
    >
      {path}
    </svg>
  );
}

export function IconMenu(p: IconProps) { return base(<><path d="M2 4h12M2 8h12M2 12h12" /></>, p.size, p.className); }
export function IconWarning(p: IconProps) { return base(<><path d="M8 2 14.5 13.5h-13L8 2z" /><path d="M8 7v3" /><path d="M8 11.8v.2" /></>, p.size, p.className); }
export function IconCheck(p: IconProps) { return base(<path d="M3 8.5 6.5 12 13 4.5" />, p.size, p.className); }
export function IconCross(p: IconProps) { return base(<><path d="M4 4l8 8" /><path d="M12 4l-8 8" /></>, p.size, p.className); }
export function IconCaretDown(p: IconProps) { return base(<path d="M4 6l4 4 4-4" />, p.size, p.className); }
export function IconCaretUp(p: IconProps) { return base(<path d="M4 10l4-4 4 4" />, p.size, p.className); }
export function IconArrowLeft(p: IconProps) { return base(<><path d="M13 8H3" /><path d="M7 4 3 8l4 4" /></>, p.size, p.className); }
export function IconSettings(p: IconProps) {
  return base(
    <>
      <circle cx="8" cy="8" r="2.2" />
      <path d="M8 1.8v2M8 12.2v2M1.8 8h2M12.2 8h2M3.6 3.6l1.4 1.4M11 11l1.4 1.4M12.4 3.6 11 5M5 11l-1.4 1.4" />
    </>,
    p.size,
    p.className,
  );
}
export function IconClose(p: IconProps) { return IconCross(p); }
export function IconEvidence(p: IconProps) { return base(<><path d="M4 1.5h5.5L13 5v9.5H4z" /><path d="M9.5 1.5V5H13" /><path d="M6 8h4M6 10.5h4" /></>, p.size, p.className); }
export function IconLayoutSplit(p: IconProps) { return base(<><rect x="2" y="2.5" width="12" height="11" rx="1" /><path d="M8 2.5v11" /></>, p.size, p.className); }
export function IconFocus(p: IconProps) { return base(<><rect x="2" y="2.5" width="12" height="11" rx="1" /><rect x="5" y="5.5" width="6" height="5" rx="0.5" /></>, p.size, p.className); }
export function IconAssistant(p: IconProps) { return base(<><circle cx="8" cy="8" r="5.5" /><path d="M5.5 7v.2M10.5 7v.2" /><path d="M5.5 10c1.5 1 3.5 1 5 0" /></>, p.size, p.className); }
export function IconLogo(p: IconProps) { return base(<><rect x="2.5" y="2.5" width="11" height="11" rx="2" transform="rotate(45 8 8)" /></>, p.size, p.className); }
