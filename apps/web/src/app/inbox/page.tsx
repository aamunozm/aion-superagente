"use client";

import { useEffect, useState } from "react";
import { AppShell, Icon, IconChip, Badge, Button, Markdown, type IconName, type Tint } from "@/components";
import { inboxList, inboxRead, type InboxMessage } from "@/lib/api";
import { LightboxProvider } from "@/lib/lightbox";
import { soundEnabled, setSoundEnabled, chime } from "@/lib/chime";

// Tipo de aviso → icono + tinte. AION etiqueta cada mensaje al escribírtelo.
const KIND: Record<string, { icon: IconName; tint: Tint; label: string }> = {
  insight: { icon: "bulb", tint: "gold", label: "Insight" },
  pregunta: { icon: "help", tint: "sky", label: "Pregunta" },
  idea: { icon: "sparkle", tint: "lavender", label: "Idea" },
  saludo: { icon: "wave", tint: "mint", label: "Saludo" },
  alerta: { icon: "warn", tint: "peach", label: "Alerta" },
};
const kindOf = (k: string) => KIND[k] ?? { icon: "bell" as IconName, tint: "gold" as Tint, label: k };

// Tiempo relativo legible ("hace 5 min", "hace 2 h", "ayer"…).
function ago(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "";
  const s = Math.max(0, (Date.now() - then) / 1000);
  if (s < 60) return "ahora";
  if (s < 3600) return `hace ${Math.floor(s / 60)} min`;
  if (s < 86400) return `hace ${Math.floor(s / 3600)} h`;
  if (s < 172800) return "ayer";
  return new Date(iso).toLocaleDateString();
}

export default function InboxPage() {
  const [msgs, setMsgs] = useState<InboxMessage[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [sound, setSound] = useState(true);

  async function refresh() {
    try {
      const r = await inboxList();
      // Más recientes primero.
      setMsgs([...(r.all ?? [])].reverse());
    } catch {
      /* núcleo no disponible */
    } finally {
      setLoaded(true);
    }
  }

  useEffect(() => {
    setSound(soundEnabled());
    refresh();
    const id = setInterval(refresh, 25000);
    return () => clearInterval(id);
  }, []);

  async function markAll() {
    await inboxRead().catch(() => {});
    refresh();
  }

  function toggleSound() {
    const next = !sound;
    setSound(next);
    setSoundEnabled(next);
    if (next) chime(); // confirmación audible al activar
  }

  const unread = msgs.filter((m) => !m.read).length;

  return (
    <LightboxProvider>
      <AppShell title="Bandeja">
        <div className="max-w-6xl mx-auto px-4 py-6">
          <div className="flex items-start justify-between gap-4 mb-5">
            <p className="text-[15px] max-w-xl" style={{ color: "var(--text-2)" }}>
              Lo que AION te ha escrito por su cuenta —ideas, preguntas, hallazgos, avisos—.
              Aquí ves de qué trata cada notificación, aunque ya la hayas oído en el chat.
            </p>
            <button
              onClick={toggleSound}
              className="shrink-0 inline-flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-full"
              style={{
                background: sound ? "var(--accent-subtle)" : "var(--surface-2)",
                color: sound ? "var(--gold-deep)" : "var(--text-3)",
              }}
              title={sound ? "Sonido activado" : "Sonido silenciado"}
            >
              <Icon name={sound ? "bell" : "bellOff"} size={14} />
              {sound ? "Sonido" : "Silencio"}
            </button>
          </div>

          <div className="flex items-center gap-2 mb-4">
            <Badge tone={unread > 0 ? "accent" : "muted"}>
              {unread > 0 ? `${unread} sin leer` : "todo al día"}
            </Badge>
            {unread > 0 && (
              <Button size="sm" variant="subtle" onClick={markAll}>
                <span className="inline-flex items-center gap-1.5">
                  <Icon name="check" size={14} /> Marcar todo leído
                </span>
              </Button>
            )}
          </div>

          {loaded && msgs.length === 0 && (
            <div className="card text-sm text-center" style={{ color: "var(--text-3)" }}>
              <div className="flex justify-center mb-2">
                <IconChip icon="bell" tint="gold" />
              </div>
              AION aún no te ha escrito nada. Cuando descubra, aprenda o quiera algo, aparecerá aquí
              (y sonará, si tienes el sonido activado).
            </div>
          )}

          <div className="flex flex-col gap-3">
            {msgs.map((m) => {
              const k = kindOf(m.kind);
              return (
                <div
                  key={m.id}
                  className="card flex gap-3"
                  style={{
                    padding: 16,
                    borderColor: m.read ? "var(--border)" : "var(--accent)",
                    background: m.read ? "var(--surface)" : "var(--accent-subtle)",
                  }}
                >
                  <IconChip icon={k.icon} tint={k.tint} size={18} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2 mb-1">
                      <span className="text-xs font-semibold" style={{ color: "var(--text-2)" }}>
                        {k.label}
                      </span>
                      {!m.read && <span className="w-1.5 h-1.5 rounded-full" style={{ background: "var(--accent)" }} />}
                      <span className="text-[11px] ml-auto" style={{ color: "var(--text-3)" }}>
                        {ago(m.at)}
                      </span>
                    </div>
                    <div className="text-sm" style={{ color: "var(--text-1)" }}>
                      <Markdown>{m.text}</Markdown>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </AppShell>
    </LightboxProvider>
  );
}
