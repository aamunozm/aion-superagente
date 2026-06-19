// ÁTOMO: Badge — píldora de estado/etiqueta. Tonos semánticos + tinte libre.
type Tone = "neutral" | "accent" | "success" | "warn" | "danger" | "muted";

const TONE: Record<Tone, { bg: string; fg: string }> = {
  neutral: { bg: "var(--surface-2)", fg: "var(--text-2)" },
  accent: { bg: "var(--accent-subtle)", fg: "var(--gold-deep)" },
  success: { bg: "var(--pastel-mint)", fg: "var(--on-mint)" },
  warn: { bg: "var(--pastel-peach)", fg: "var(--on-peach)" },
  danger: { bg: "rgba(239,68,68,0.12)", fg: "#ef4444" },
  muted: { bg: "transparent", fg: "var(--text-3)" },
};

export default function Badge({
  tone = "neutral",
  children,
  className = "",
  style,
}: {
  tone?: Tone;
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}) {
  const t = TONE[tone];
  return (
    <span
      className={`inline-flex items-center gap-1 text-[11px] font-medium px-2 py-0.5 rounded-full ${className}`.trim()}
      style={{ background: t.bg, color: t.fg, ...style }}
    >
      {children}
    </span>
  );
}
