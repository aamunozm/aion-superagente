"use client";

import { useRef, useState } from "react";
import { chatStream, type ChatEvent } from "@/lib/api";

type Turn = {
  prompt: string;
  thinking: string;
  answer: string;
  meta?: string;
};

export default function ChatPage() {
  const [input, setInput] = useState("");
  const [think, setThink] = useState(true);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [busy, setBusy] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);

  async function send(e: React.FormEvent) {
    e.preventDefault();
    const prompt = input.trim();
    if (!prompt || busy) return;
    setInput("");
    setBusy(true);
    const idx = turns.length;
    setTurns((t) => [...t, { prompt, thinking: "", answer: "" }]);

    const update = (patch: (t: Turn) => Turn) =>
      setTurns((prev) => prev.map((t, i) => (i === idx ? patch(t) : t)));

    try {
      await chatStream(prompt, think, (ev: ChatEvent) => {
        if (ev.kind === "thinking") update((t) => ({ ...t, thinking: t.thinking + ev.text }));
        else if (ev.kind === "answer") update((t) => ({ ...t, answer: t.answer + ev.text }));
        else if (ev.kind === "done")
          update((t) => ({ ...t, meta: `${ev.tokens} tokens · ${ev.tps.toFixed(1)} tok/s` }));
        else if (ev.kind === "error") update((t) => ({ ...t, answer: `⚠️ ${ev.text}` }));
        endRef.current?.scrollIntoView({ behavior: "smooth" });
      });
    } catch (err) {
      update((t) => ({ ...t, answer: `⚠️ ${err instanceof Error ? err.message : "error"}` }));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="min-h-screen flex flex-col max-w-3xl mx-auto px-4">
      <header className="flex items-center gap-2 py-4 border-b" style={{ borderColor: "var(--border)" }}>
        <span className="w-2.5 h-2.5 rounded-full" style={{ background: busy ? "var(--cog-thinking)" : "var(--accent)" }} />
        <span className="font-display font-semibold">AION</span>
        <span className="text-xs" style={{ color: "var(--text-3)" }}>
          {busy ? "pensando…" : "gemma4-reason · local"}
        </span>
      </header>

      <div className="flex-1 overflow-y-auto py-6 flex flex-col gap-6">
        {turns.length === 0 && (
          <p className="text-center text-sm mt-20" style={{ color: "var(--text-3)" }}>
            Escribe algo. AION razona localmente, sin enviar tus datos a nadie.
          </p>
        )}
        {turns.map((t, i) => (
          <div key={i} className="flex flex-col gap-2">
            <div className="self-end card max-w-[80%]" style={{ background: "var(--surface-2)" }}>
              {t.prompt}
            </div>
            {t.thinking && (
              <details className="text-sm" style={{ color: "var(--text-3)" }}>
                <summary className="cursor-pointer select-none" style={{ color: "var(--cog-thinking)" }}>
                  🧠 razonamiento
                </summary>
                <pre className="whitespace-pre-wrap font-mono text-xs mt-2">{t.thinking}</pre>
              </details>
            )}
            {t.answer && (
              <div className="card max-w-[90%]">
                <p className="whitespace-pre-wrap">{t.answer}</p>
                {t.meta && (
                  <p className="text-xs mt-2" style={{ color: "var(--text-3)" }}>
                    {t.meta}
                  </p>
                )}
              </div>
            )}
          </div>
        ))}
        <div ref={endRef} />
      </div>

      <form onSubmit={send} className="py-4 flex gap-2 items-center border-t" style={{ borderColor: "var(--border)" }}>
        <button
          type="button"
          onClick={() => setThink(!think)}
          className="text-xs px-3 py-2 rounded-full shrink-0"
          style={{
            background: think ? "var(--accent-subtle)" : "var(--surface-2)",
            color: think ? "var(--accent)" : "var(--text-3)",
          }}
          title="Modo razonamiento"
        >
          🧠 {think ? "on" : "off"}
        </button>
        <input
          className="input"
          placeholder="Pregunta a AION…"
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <button className="btn shrink-0" disabled={busy}>
          {busy ? "…" : "Enviar"}
        </button>
      </form>
    </main>
  );
}
