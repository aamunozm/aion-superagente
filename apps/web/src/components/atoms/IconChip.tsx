// ÁTOMO: IconChip — chip pastel con icono (reemplaza emojis 3D). Tinte por módulo.
import Icon, { type IconName } from "./Icon";

export type Tint = "mint" | "sky" | "lavender" | "peach" | "gold";

const TINT: Record<Tint, { bg: string; fg: string }> = {
  mint: { bg: "var(--pastel-mint)", fg: "var(--on-mint)" },
  sky: { bg: "var(--pastel-sky)", fg: "var(--on-sky)" },
  lavender: { bg: "var(--pastel-lavender)", fg: "var(--on-lavender)" },
  peach: { bg: "var(--pastel-peach)", fg: "var(--on-peach)" },
  gold: { bg: "var(--pastel-gold)", fg: "var(--on-gold)" },
};

export default function IconChip({
  icon,
  tint = "gold",
  size = 18,
}: {
  icon: IconName;
  tint?: Tint;
  size?: number;
}) {
  const t = TINT[tint];
  return (
    <span className="icon-chip" style={{ background: t.bg, color: t.fg }}>
      <Icon name={icon} size={size} />
    </span>
  );
}
