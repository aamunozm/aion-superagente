import Link from "next/link";

export default function Home() {
  return (
    <main className="min-h-screen flex flex-col items-center justify-center px-6 text-center">
      <div className="max-w-xl">
        <div
          className="inline-flex items-center gap-2 mb-6 px-3 py-1 rounded-full text-[11px] font-semibold uppercase tracking-wide"
          style={{ background: "var(--accent-subtle)", color: "var(--accent)" }}
        >
          <span className="w-2 h-2 rounded-full" style={{ background: "var(--accent)" }} />
          local-first · privado · auto-evolutivo
        </div>
        <h1 className="font-display text-5xl font-bold mb-4" style={{ color: "var(--text-1)" }}>
          AION
        </h1>
        <p className="text-lg mb-8" style={{ color: "var(--text-2)" }}>
          Tu super-agente de IA que razona, recuerda y evoluciona — toda la
          cognición en tu dispositivo.
        </p>
        <div className="flex gap-3 justify-center">
          <Link href="/login" className="btn">
            Empezar
          </Link>
          <Link
            href="/chat"
            className="btn"
            style={{ background: "transparent", color: "var(--text-1)", border: "1px solid var(--border-2)" }}
          >
            Abrir chat
          </Link>
        </div>
      </div>
    </main>
  );
}
