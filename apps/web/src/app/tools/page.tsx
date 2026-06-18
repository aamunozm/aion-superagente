"use client";

import { useEffect, useState } from "react";
import { AppShell, Icon, IconChip, Badge, type IconName, type Tint } from "@/components";
import { toolsList, type ToolGroup } from "@/lib/api";

// Mapa categoría → icono + tinte pastel. La lista de herramientas viene del
// backend (/api/tools), fuente única que refleja lo que el agente registra de verdad.
const CAT_STYLE: Record<string, { icon: IconName; tint: Tint }> = {
  "Cálculo": { icon: "calculator", tint: "gold" },
  "Memoria": { icon: "memory", tint: "mint" },
  "Conocimiento": { icon: "graph", tint: "lavender" },
  "Web e investigación": { icon: "globe", tint: "sky" },
  "Navegador": { icon: "globe", tint: "sky" },
  "Archivos y sistema": { icon: "file", tint: "peach" },
  "Red": { icon: "wifi", tint: "sky" },
  "Pantalla y control": { icon: "eye", tint: "peach" },
  "Reconocimiento facial": { icon: "user", tint: "lavender" },
  "Comunicaciones": { icon: "message", tint: "mint" },
  "Skills": { icon: "code", tint: "lavender" },
  "Confirmación": { icon: "shield", tint: "gold" },
};
const catStyle = (cat: string) => CAT_STYLE[cat] ?? { icon: "tools" as IconName, tint: "gold" as Tint };

export default function ToolsPage() {
  const [groups, setGroups] = useState<ToolGroup[]>([]);
  const [count, setCount] = useState(0);
  const [err, setErr] = useState(false);

  useEffect(() => {
    toolsList()
      .then((r) => {
        setGroups(r.groups);
        setCount(r.count);
      })
      .catch(() => setErr(true));
  }, []);

  return (
    <AppShell title="Herramientas">
      <div className="max-w-6xl mx-auto px-3 py-6">
        <p className="text-[15px] mb-2 max-w-2xl" style={{ color: "var(--text-2)" }}>
          Las capacidades REALES del agente, leídas del núcleo. Las marcadas{" "}
          <Badge tone="accent">
            <Icon name="shield" size={11} /> confirma
          </Badge>{" "}
          piden tu aprobación antes de actuar (acciones sensibles o que escriben en el mundo).
        </p>
        {count > 0 && (
          <p className="text-xs mb-7" style={{ color: "var(--text-3)" }}>
            {count} herramientas activas en {groups.length} categorías.
          </p>
        )}

        {err && (
          <div className="card text-sm" style={{ color: "var(--text-2)" }}>
            No pude leer el catálogo del núcleo. ¿Está AION en marcha (puerto 8765)? Reintenta al
            abrir esta página con el backend activo.
          </div>
        )}

        <div className="flex flex-col gap-8">
          {groups.map((g) => {
            const cs = catStyle(g.category);
            return (
              <section key={g.category}>
                <div className="flex items-center gap-2 mb-3">
                  <IconChip icon={cs.icon} tint={cs.tint} size={16} />
                  <h2 className="font-display font-semibold text-[15px]">{g.category}</h2>
                  <span className="text-xs" style={{ color: "var(--text-3)" }}>
                    · {g.tools.length}
                  </span>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
                  {g.tools.map((t) => (
                    <div key={t.name} className="module card-hover flex flex-col" style={{ padding: 16 }}>
                      <div className="flex items-center gap-2 mb-1.5">
                        <code className="text-[13px] font-semibold" style={{ color: "var(--text-1)" }}>
                          {t.name}
                        </code>
                        {t.sensitive && (
                          <Badge tone="accent">
                            <Icon name="shield" size={10} /> confirma
                          </Badge>
                        )}
                      </div>
                      <p className="text-[13px]" style={{ color: "var(--text-2)" }}>
                        {t.description}
                      </p>
                    </div>
                  ))}
                </div>
              </section>
            );
          })}
        </div>
      </div>
    </AppShell>
  );
}
