// ÁTOMO: IconButton — botón cuadrado/redondo solo-icono. Base de las acciones
// compactas (copiar, reproducir, micrófono, nuevo chat…). Reutiliza el icono atómico.
import type { ButtonHTMLAttributes } from "react";
import Icon, { type IconName } from "./Icon";

type Tone = "neutral" | "accent" | "danger";
type Shape = "chip" | "round";

const TONE: Record<Tone, { bg: string; fg: string }> = {
  neutral: { bg: "var(--surface-2)", fg: "var(--text-2)" },
  accent: { bg: "var(--accent-subtle)", fg: "var(--gold-deep)" },
  danger: { bg: "rgba(239,68,68,0.10)", fg: "#ef4444" },
};

export default function IconButton({
  icon,
  iconSize = 16,
  tone = "neutral",
  shape = "chip",
  active = false,
  className = "",
  style,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  icon: IconName;
  iconSize?: number;
  tone?: Tone;
  shape?: Shape;
  active?: boolean;
}) {
  const t = active ? TONE.accent : TONE[tone];
  const base: React.CSSProperties =
    shape === "chip"
      ? { background: t.bg, color: t.fg }
      : { background: t.bg, color: t.fg, borderRadius: 999, padding: 8, display: "inline-flex" };
  return (
    <button
      className={`${shape === "chip" ? "icon-chip" : ""} transition-all hover:opacity-80 ${className}`.trim()}
      style={{ ...base, ...style }}
      {...rest}
    >
      <Icon name={icon} size={iconSize} />
    </button>
  );
}
