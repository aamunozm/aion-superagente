"use client";

// ORGANISM: VoiceBar — controles de voz del composer del chat.
//   · Micrófono (push-to-talk / dictado): inicia o corta la escucha.
//   · Manos libres: AION lee sus respuestas y reabre el micro al terminar,
//     para una conversación hablada continua en tiempo real.
// Presentacional: los hooks de voz viven en la página (controlador único).
import { Icon } from "@/components/atoms";
import { useT } from "@/lib/i18n";

export default function VoiceBar({
  micSupported,
  listening,
  handsFree,
  ttsSupported,
  disabled,
  onMic,
  onToggleHandsFree,
}: {
  micSupported: boolean;
  listening: boolean;
  handsFree: boolean;
  ttsSupported: boolean;
  disabled?: boolean;
  onMic: () => void;
  onToggleHandsFree: () => void;
}) {
  const { t } = useT();
  if (!micSupported && !ttsSupported) return null;

  return (
    <>
      {micSupported && (
        <button
          type="button"
          onClick={onMic}
          disabled={disabled && !listening}
          className="shrink-0 rounded-full p-2 transition-colors"
          style={{
            color: listening ? "#fff" : "var(--text-3)",
            background: listening ? "#ef4444" : "var(--surface-2)",
          }}
          title={listening ? t("chat.listening") : t("chat.listen")}
          aria-label={listening ? t("chat.listening") : t("chat.listen")}
        >
          <Icon name="mic" size={18} className={listening ? "animate-pulse" : ""} />
        </button>
      )}

      {ttsSupported && (
        <button
          type="button"
          onClick={onToggleHandsFree}
          className="shrink-0 rounded-full p-2 transition-colors"
          style={{
            color: handsFree ? "var(--gold-deep)" : "var(--text-3)",
            background: handsFree ? "var(--accent-subtle)" : "var(--surface-2)",
          }}
          title={handsFree ? t("chat.handsFreeOn") : t("chat.handsFree")}
          aria-label={handsFree ? t("chat.handsFreeOn") : t("chat.handsFree")}
        >
          <Icon name="volume" size={18} />
        </button>
      )}
    </>
  );
}
