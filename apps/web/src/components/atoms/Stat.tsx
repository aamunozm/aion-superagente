// ÁTOMO: Stat — cifra grande + etiqueta (consolida los helpers de mind/onboarding).
export default function Stat({
  value,
  label,
  color = "var(--text-1)",
  align = "left",
}: {
  value: React.ReactNode;
  label: string;
  color?: string;
  align?: "left" | "center";
}) {
  return (
    <div style={{ textAlign: align }}>
      <div className="text-2xl font-semibold leading-tight" style={{ color }}>
        {value}
      </div>
      <div className="text-[11px] mt-0.5" style={{ color: "var(--text-3)" }}>
        {label}
      </div>
    </div>
  );
}
