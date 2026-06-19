// ÁTOMO: Textarea — multilínea con el mismo lenguaje visual que .input.
import type { TextareaHTMLAttributes } from "react";

export default function Textarea({
  className = "",
  ...rest
}: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea className={`input ${className}`.trim()} {...rest} />;
}
