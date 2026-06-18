"use client";

// ORGANISM: MessageActions — barra de acciones bajo cada respuesta del agente.
// Copiar al portapapeles + reproducir/parar con la voz de AION (TTS).
// El controlador TTS es único por página (useSpeech) y se inyecta por props,
// así dos mensajes nunca hablan a la vez.
import { useState } from "react";
import { Icon } from "@/components/atoms";
import { useT } from "@/lib/i18n";

export default function MessageActions({
  text,
  speaking,
  canSpeak,
  onSpeak,
  onStop,
}: {
  text: string;
  speaking: boolean;
  canSpeak: boolean;
  onSpeak: () => void;
  onStop: () => void;
}) {
  const { t } = useT();
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      /* sin permiso de portapapeles */
    }
  }

  const btn =
    "inline-flex items-center gap-1 text-[11px] px-1.5 py-1 rounded-md transition-colors hover:opacity-100";

  return (
    <div className="flex items-center gap-1 mt-1.5 -ml-1" style={{ color: "var(--text-3)" }}>
      <button
        type="button"
        onClick={copy}
        className={btn}
        style={{ opacity: copied ? 1 : 0.7, color: copied ? "var(--on-mint)" : "var(--text-3)" }}
        title={t("chat.copy")}
        aria-label={t("chat.copy")}
      >
        <Icon name={copied ? "check" : "copy"} size={14} />
        {copied ? t("chat.copied") : t("chat.copy")}
      </button>

      {canSpeak && (
        <button
          type="button"
          onClick={speaking ? onStop : onSpeak}
          className={btn}
          style={{ opacity: speaking ? 1 : 0.7, color: speaking ? "var(--gold-deep)" : "var(--text-3)" }}
          title={speaking ? t("chat.stopSpeak") : t("chat.speak")}
          aria-label={speaking ? t("chat.stopSpeak") : t("chat.speak")}
        >
          <Icon name={speaking ? "stop" : "play"} size={13} />
          {speaking ? t("chat.stopSpeak") : t("chat.speak")}
        </button>
      )}
    </div>
  );
}
