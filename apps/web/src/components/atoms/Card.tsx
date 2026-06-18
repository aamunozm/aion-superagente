// ÁTOMO: Card — superficie elevada. Envuelve .card / .module + hover opcional.
import type { HTMLAttributes } from "react";

export default function Card({
  variant = "card",
  hover = false,
  className = "",
  children,
  ...rest
}: HTMLAttributes<HTMLDivElement> & {
  variant?: "card" | "module";
  hover?: boolean;
}) {
  const cls = [variant, hover ? "card-hover" : "", className].filter(Boolean).join(" ");
  return (
    <div className={cls} {...rest}>
      {children}
    </div>
  );
}
