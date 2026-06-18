// ÁTOMO: Avatar — inicial en círculo (cuenta, contactos). Tamaño configurable.
export default function Avatar({
  label,
  size = 32,
  bg = "var(--accent-subtle)",
  fg = "var(--gold-deep)",
}: {
  label: string;
  size?: number;
  bg?: string;
  fg?: string;
}) {
  const initial = (label.trim()[0] || "?").toUpperCase();
  return (
    <span
      className="inline-flex items-center justify-center rounded-full font-semibold shrink-0"
      style={{ width: size, height: size, background: bg, color: fg, fontSize: size * 0.42 }}
      aria-hidden="true"
    >
      {initial}
    </span>
  );
}
