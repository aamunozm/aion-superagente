// ÁTOMO: ProgressBar — unifica las dos `Bar` duplicadas (mind/ y claude-code/).
// Acepta value/max o un porcentaje directo; color configurable.
export default function ProgressBar({
  value,
  max = 1,
  color = "var(--accent)",
  height = 6,
  label,
  className = "",
}: {
  value: number;
  max?: number;
  color?: string;
  height?: number;
  label?: string;
  className?: string;
}) {
  const pct = Math.max(0, Math.min(100, max > 0 ? (value / max) * 100 : 0));
  return (
    <div className={className}>
      {label && (
        <div className="flex justify-between text-[11px] mb-1" style={{ color: "var(--text-3)" }}>
          <span>{label}</span>
          <span>{Math.round(pct)}%</span>
        </div>
      )}
      <div className="w-full rounded-full overflow-hidden" style={{ height, background: "var(--surface-2)" }}>
        <div className="h-full rounded-full transition-all" style={{ width: `${pct}%`, background: color }} />
      </div>
    </div>
  );
}
