// ÁTOMO: Input — envuelve la clase .input del design system (globals.css).
import type { InputHTMLAttributes } from "react";

export default function Input({
  className = "",
  ...rest
}: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={`input ${className}`.trim()} {...rest} />;
}
