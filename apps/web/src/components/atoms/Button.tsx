// ÁTOMO: Button — envuelve las clases CSS .btn / .btn-gold del design system
// (globals.css) sin reemplazarlas. Variantes semánticas + tamaños.
import type { ButtonHTMLAttributes } from "react";

type Variant = "primary" | "gold" | "ghost" | "subtle";
type Size = "sm" | "md";

const VARIANT_STYLE: Record<Variant, React.CSSProperties> = {
  primary: {},
  gold: {},
  ghost: { background: "transparent", color: "var(--text-2)", boxShadow: "none" },
  subtle: { background: "var(--surface-2)", color: "var(--text-2)", boxShadow: "none" },
};

const SIZE_STYLE: Record<Size, React.CSSProperties> = {
  sm: { padding: "8px 13px", fontSize: 13 },
  md: {},
};

export default function Button({
  variant = "primary",
  size = "md",
  className = "",
  style,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: Size }) {
  const cls = variant === "gold" ? "btn btn-gold" : "btn";
  return (
    <button
      className={`${cls} ${className}`.trim()}
      style={{ ...VARIANT_STYLE[variant], ...SIZE_STYLE[size], ...style }}
      {...rest}
    >
      {children}
    </button>
  );
}
