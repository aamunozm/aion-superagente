// MOLÉCULA: Panel — tarjeta de sección con cabecera (icono + título) y nota opcional.
// Consolida el helper `Panel` que vivía suelto en mind/page.tsx.
import { Card, Icon, type IconName } from "../atoms";

export default function Panel({
  title,
  icon,
  note,
  right,
  children,
  className = "",
}: {
  title: string;
  icon?: IconName;
  note?: string;
  right?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <Card className={className}>
      <div className="flex items-center gap-2 mb-2">
        {icon && <Icon name={icon} size={16} />}
        <h2 className="t-section" style={{ color: "var(--text-2)" }}>
          {title}
        </h2>
        {right && <div className="ml-auto">{right}</div>}
      </div>
      {note && (
        <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
          {note}
        </p>
      )}
      {children}
    </Card>
  );
}
