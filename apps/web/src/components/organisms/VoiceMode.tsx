"use client";

// ORGANISM: VoiceMode — conversación por voz inmersiva (estilo asistente).
//   Un overlay a pantalla completa que VISUALIZA el bucle manos-libres que ya
//   vive en la página de chat: Escuchando → Pensando → Hablando → (reabre micro).
//   Presentacional: no contiene hooks de voz; recibe el estado derivado y
//   dispara callbacks. 100% local (Web Speech API), cero backend.
import { Icon } from "@/components/atoms";
import { useT } from "@/lib/i18n";

export type VoiceState = "listening" | "thinking" | "speaking" | "idle";

export default function VoiceMode({
  open,
  state,
  muted,
  interim,
  caption,
  onToggleMic,
  onClose,
}: {
  open: boolean;
  state: VoiceState;
  /** Micrófono en pausa (silenciado) — no escucha aunque AION calle. */
  muted?: boolean;
  /** Transcripción provisional mientras hablas (se muestra al escuchar). */
  interim?: string;
  /** Texto/Resumen de lo que AION dice (se muestra al hablar). */
  caption?: string;
  /** Pausar/reanudar la escucha sin salir del modo. */
  onToggleMic: () => void;
  onClose: () => void;
}) {
  const { t } = useT();
  if (!open) return null;

  const active = state === "listening" || state === "speaking";
  const label = muted
    ? t("chat.voiceMuted")
    : state === "listening"
      ? t("chat.listening")
      : state === "thinking"
        ? t("chat.voiceThinking")
        : state === "speaking"
          ? t("chat.voiceSpeaking")
          : t("chat.voiceTap");

  // Subtexto: lo que dices (al escuchar) o lo que AION dice (al hablar).
  const sub =
    state === "listening" && interim
      ? interim
      : state === "speaking" && caption
        ? caption
        : "";

  return (
    <div
      className="fixed inset-0 z-50 flex flex-col items-center justify-center voice-fade"
      style={{
        background: "color-mix(in srgb, var(--surface) 88%, transparent)",
        backdropFilter: "blur(16px)",
        WebkitBackdropFilter: "blur(16px)",
      }}
      role="dialog"
      aria-modal="true"
      aria-label={t("chat.voiceMode")}
    >
      {/* Salir (esquina) */}
      <button
        onClick={onClose}
        className="absolute top-6 right-6 rounded-full p-2.5 transition-colors"
        style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
        title={t("chat.voiceExit")}
        aria-label={t("chat.voiceExit")}
      >
        <Icon name="x" size={20} />
      </button>

      {/* Orbe + onda */}
      <div
        className="rounded-full flex items-center justify-center mb-8"
        style={{
          width: 168,
          height: 168,
          background:
            state === "speaking"
              ? "linear-gradient(135deg, var(--gold), var(--gold-deep))"
              : "var(--accent-subtle)",
          boxShadow: active ? "0 0 0 12px var(--accent-subtle)" : "none",
          transition: "box-shadow .4s ease, background .4s ease",
          animation: state === "thinking" ? "voicePulse 1.4s ease-in-out infinite" : undefined,
        }}
      >
        <div className="flex items-end gap-1.5" style={{ height: 56 }}>
          {[0, 1, 2, 3, 4].map((i) => (
            <span
              key={i}
              className="rounded-full"
              style={{
                width: 7,
                height: active ? "100%" : 12,
                background: state === "speaking" ? "#fff" : "var(--gold-deep)",
                transformOrigin: "center",
                animation: active ? `voiceBar 0.9s ease-in-out ${i * 0.12}s infinite` : undefined,
                transition: "height .3s ease",
              }}
            />
          ))}
        </div>
      </div>

      {/* Estado */}
      <div className="font-display text-2xl font-bold mb-2" style={{ color: "var(--text-1)" }}>
        {label}
      </div>
      <p
        className="text-sm text-center px-8 max-w-lg min-h-[2.5rem] line-clamp-3"
        style={{ color: "var(--text-2)" }}
      >
        {sub}
      </p>

      {/* Controles */}
      <div className="flex items-center gap-4 mt-6">
        <button
          onClick={onToggleMic}
          className="rounded-full p-4 transition-colors"
          style={{
            background: muted ? "var(--surface-2)" : state === "listening" ? "#ef4444" : "var(--accent-subtle)",
            color: muted ? "var(--text-3)" : state === "listening" ? "#fff" : "var(--gold-deep)",
          }}
          title={muted ? t("chat.voiceMuted") : t("chat.listening")}
          aria-label={muted ? t("chat.voiceMuted") : t("chat.listening")}
        >
          <Icon name={muted ? "bellOff" : "mic"} size={24} className={state === "listening" ? "animate-pulse" : ""} />
        </button>
        <button
          onClick={onClose}
          className="rounded-full p-4 transition-colors"
          style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
          title={t("chat.voiceExit")}
          aria-label={t("chat.voiceExit")}
        >
          <Icon name="x" size={24} />
        </button>
      </div>
    </div>
  );
}
