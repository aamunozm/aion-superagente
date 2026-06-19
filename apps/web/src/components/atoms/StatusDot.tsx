// ÁTOMO: StatusDot — punto de estado (activo/externo/inactivo). Indicador vivo.
export default function StatusDot({
  color = "var(--accent)",
  size = 8,
  pulse = false,
  className = "",
}: {
  color?: string;
  size?: number;
  pulse?: boolean;
  className?: string;
}) {
  return (
    <span
      className={`inline-block rounded-full ${pulse ? "animate-pulse" : ""} ${className}`.trim()}
      style={{ width: size, height: size, background: color }}
    />
  );
}
