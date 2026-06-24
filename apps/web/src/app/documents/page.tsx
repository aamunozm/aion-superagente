"use client";

import { useCallback, useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import {
  docStylesList,
  docStyleSave,
  docStyleRemove,
  docStyleExtract,
  documentsOfferta,
  type DocStyleT,
  type StyleEntry,
} from "@/lib/api";

/** Mini-previsualización de un estilo: una "hoja" con su papel, tinta y acento. */
function Swatch({ s }: { s: DocStyleT }) {
  return (
    <div
      className="w-full h-20 rounded-lg overflow-hidden flex flex-col"
      style={{ background: s.paper, border: `1px solid ${s.hair}`, fontFamily: s.font_display }}
    >
      <div style={{ background: s.ink, height: 22, display: "flex", alignItems: "center", padding: "0 8px" }}>
        <span style={{ color: s.accent, fontSize: 7, fontWeight: 800, letterSpacing: "0.1em", textTransform: "uppercase" }}>
          Aa
        </span>
      </div>
      <div className="flex-1 p-2 flex flex-col gap-1 justify-center">
        <div style={{ height: 5, width: "60%", background: s.ink, borderRadius: s.radius / 2 }} />
        <div style={{ height: 3, width: "85%", background: s.muted, opacity: 0.5, borderRadius: 2 }} />
        <div style={{ display: "flex", gap: 4, marginTop: 2 }}>
          <span style={{ width: 14, height: 8, background: s.accent, borderRadius: s.radius / 2 }} />
          <span style={{ width: 24, height: 8, background: s.soft, border: `1px solid ${s.hair}`, borderRadius: s.radius / 2 }} />
        </div>
      </div>
    </div>
  );
}

export default function DocumentsPage() {
  const [entries, setEntries] = useState<StyleEntry[]>([]);
  const [selected, setSelected] = useState<DocStyleT | null>(null);
  const [busy, setBusy] = useState("");
  const [note, setNote] = useState("");

  // Extracción
  const [extracted, setExtracted] = useState<{ style: DocStyleT; palette: string[]; fonts: string[] } | null>(null);
  const [saveName, setSaveName] = useState("");

  // Oferta
  const [client, setClient] = useState("");
  const [heroTitle, setHeroTitle] = useState("Più clienti, in automatico.");
  const [heroPitch, setHeroPitch] = useState(
    "Sito **trovato su Google**, app che **risponde subito** e dashboard dei risultati. Tutto gestito.",
  );
  const [servicesText, setServicesText] = useState(
    "Primo mese — tutto incluso | € 300,00 | \nDal secondo mese | € 200,00 | / mese",
  );
  const [recurring, setRecurring] = useState("€ 200,00 / mese");
  const [deductible, setDeductible] = useState(true);

  const load = useCallback(() => {
    docStylesList()
      .then((e) => {
        setEntries(e);
        if (!selected && e.length) setSelected(e[0].style);
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  async function onExtract(file: File) {
    setBusy("extract");
    setNote("");
    const b64 = await new Promise<string>((res, rej) => {
      const r = new FileReader();
      r.onload = () => res(String(r.result).split(",")[1] ?? "");
      r.onerror = () => rej(new Error("no pude leer"));
      r.readAsDataURL(file);
    });
    const kind = file.name.toLowerCase().endsWith(".pdf") ? "pdf" : "image";
    const r = await docStyleExtract(b64, kind, `Estilo de ${file.name}`).catch(() => null);
    setBusy("");
    if (r?.ok && r.style) {
      setExtracted({ style: r.style, palette: r.palette ?? [], fonts: r.fonts ?? [] });
      setSelected(r.style);
      setSaveName(r.style.name);
      setNote("✓ Estilo extraído del documento. Revísalo y guárdalo si te gusta.");
    } else {
      setNote(`⚠️ ${r?.error ?? "no pude extraer el estilo"}`);
    }
  }

  async function saveExtracted() {
    if (!extracted) return;
    setBusy("save");
    const style = { ...extracted.style, name: saveName.trim() || extracted.style.name };
    const r = await docStyleSave(style).catch(() => null);
    setBusy("");
    if (r?.ok) {
      setNote(`✓ Estilo «${style.name}» guardado en tu galería.`);
      setExtracted(null);
      load();
    } else {
      setNote(`⚠️ ${r?.error ?? "no pude guardar"}`);
    }
  }

  async function removeStyle(name: string) {
    setBusy(name);
    await docStyleRemove(name).catch(() => {});
    setBusy("");
    load();
  }

  function buildFacts() {
    const services = servicesText
      .split("\n")
      .map((l) => l.split("|").map((x) => x.trim()))
      .filter((p) => p[0])
      .map((p) => ({ title: p[0], desc: "", price: p[1] ?? "", price_note: p[2] ?? "" }));
    return {
      kicker: "OFFERTA SERVIZI 2026",
      client,
      hero_title: heroTitle,
      hero_pitch: heroPitch,
      services,
      recurring_label: "Dal 2° mese (IVA esclusa)",
      recurring_value: recurring,
      deductible,
      validity_days: 30,
    };
  }

  async function generate(format: "pdf" | "html") {
    setBusy("gen");
    setNote("");
    try {
      await documentsOfferta(buildFacts(), selected, format);
      setNote(`✓ Oferta generada (${format.toUpperCase()}) con el estilo «${selected?.name ?? "por defecto"}».`);
    } catch (e) {
      setNote(`⚠️ ${(e as Error).message}`);
    }
    setBusy("");
  }

  return (
    <AppShell title="Documentos">
      <div className="max-w-5xl mx-auto p-6 flex flex-col gap-6">
        <div>
          <h1 className="font-display text-2xl font-bold" style={{ color: "var(--text-1)" }}>
            Documentos
          </h1>
          <p className="text-sm mt-1" style={{ color: "var(--text-3)" }}>
            Elige un estilo, extráelo de un documento de referencia, guárdalo, y genera ofertas con esa apariencia.
          </p>
        </div>

        {note && (
          <p className="text-xs px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", color: "var(--text-2)" }}>
            {note}
          </p>
        )}

        {/* ── Galería de estilos ── */}
        <section className="card">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
            Estilos {selected && <span style={{ color: "var(--text-3)" }}>· elegido: {selected.name}</span>}
          </h2>
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-3">
            {entries.map((e) => (
              <button
                key={e.name + (e.builtin ? "-b" : "")}
                onClick={() => setSelected(e.style)}
                className="text-left rounded-xl p-2 transition-all"
                style={{
                  background: "var(--surface)",
                  border: `2px solid ${selected?.name === e.name ? "var(--accent)" : "var(--border)"}`,
                }}
              >
                <Swatch s={e.style} />
                <div className="flex items-center justify-between mt-2">
                  <span className="text-xs font-medium truncate" style={{ color: "var(--text-1)" }}>
                    {e.name}
                  </span>
                  {!e.builtin && (
                    <span
                      role="button"
                      onClick={(ev) => {
                        ev.stopPropagation();
                        removeStyle(e.name);
                      }}
                      className="text-xs opacity-50 hover:opacity-100"
                      style={{ color: "var(--danger)" }}
                    >
                      ✕
                    </span>
                  )}
                </div>
                <span className="text-[10px]" style={{ color: "var(--text-3)" }}>
                  {e.builtin ? "preset" : "guardado"}
                </span>
              </button>
            ))}
          </div>
        </section>

        {/* ── Extraer estilo ── */}
        <section className="card">
          <h2 className="t-section mb-2" style={{ color: "var(--text-2)" }}>
            Extraer estilo de un documento
          </h2>
          <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
            Sube un PDF o imagen (tu oferta, un folleto…) y AION saca su paleta y tipografías.
          </p>
          <label className="btn btn-ghost text-xs cursor-pointer inline-block">
            {busy === "extract" ? "Extrayendo…" : "⬆︎ Subir referencia"}
            <input
              type="file"
              accept=".pdf,image/*"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) onExtract(f);
                e.currentTarget.value = "";
              }}
            />
          </label>

          {extracted && (
            <div className="mt-4 rounded-xl p-3" style={{ background: "var(--surface-1)", border: "1px solid var(--border)" }}>
              <div className="flex gap-4 items-start">
                <div className="w-40 shrink-0">
                  <Swatch s={extracted.style} />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex gap-1.5 flex-wrap mb-2">
                    {extracted.palette.map((c) => (
                      <span key={c} title={c} style={{ width: 20, height: 20, background: c, borderRadius: 5, border: "1px solid var(--border)" }} />
                    ))}
                  </div>
                  <p className="text-xs" style={{ color: "var(--text-3)" }}>
                    Fuentes: {extracted.fonts.length ? extracted.fonts.join(", ") : "—"}
                  </p>
                  <div className="flex gap-2 mt-2">
                    <input
                      className="input flex-1 text-sm"
                      value={saveName}
                      onChange={(e) => setSaveName(e.target.value)}
                      placeholder="Nombre del estilo"
                    />
                    <button className="btn btn-gold text-xs shrink-0" disabled={busy === "save"} onClick={saveExtracted}>
                      Guardar estilo
                    </button>
                  </div>
                </div>
              </div>
            </div>
          )}
        </section>

        {/* ── Generar oferta ── */}
        <section className="card">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
            Generar oferta (con el estilo elegido)
          </h2>
          <div className="grid sm:grid-cols-2 gap-3">
            <label className="text-xs" style={{ color: "var(--text-2)" }}>
              Cliente
              <input className="input mt-1" value={client} onChange={(e) => setClient(e.target.value)} placeholder="Avv. Lisa Armenio" />
            </label>
            <label className="text-xs" style={{ color: "var(--text-2)" }}>
              Canone recurrente
              <input className="input mt-1" value={recurring} onChange={(e) => setRecurring(e.target.value)} />
            </label>
            <label className="text-xs sm:col-span-2" style={{ color: "var(--text-2)" }}>
              Titular (hero)
              <input className="input mt-1" value={heroTitle} onChange={(e) => setHeroTitle(e.target.value)} />
            </label>
            <label className="text-xs sm:col-span-2" style={{ color: "var(--text-2)" }}>
              Frase de valor (admite **negrita**)
              <input className="input mt-1" value={heroPitch} onChange={(e) => setHeroPitch(e.target.value)} />
            </label>
            <label className="text-xs sm:col-span-2" style={{ color: "var(--text-2)" }}>
              Servicios — una línea por servicio: <code>Título | precio | nota</code>
              <textarea className="input mt-1 font-mono text-xs" rows={3} value={servicesText} onChange={(e) => setServicesText(e.target.value)} />
            </label>
            <label className="text-xs flex items-center gap-2" style={{ color: "var(--text-2)" }}>
              <input type="checkbox" checked={deductible} onChange={(e) => setDeductible(e.target.checked)} />
              Coste deducible (franja + callout)
            </label>
          </div>
          <div className="flex gap-2 mt-4">
            <button className="btn btn-gold" disabled={busy === "gen"} onClick={() => generate("pdf")}>
              {busy === "gen" ? "Generando…" : "⬇︎ Generar PDF"}
            </button>
            <button className="btn btn-ghost text-xs" disabled={busy === "gen"} onClick={() => generate("html")}>
              HTML
            </button>
          </div>
        </section>
      </div>
    </AppShell>
  );
}
