"use client";

import AppShell from "@/components/AppShell";
import DocStudio from "@/components/documents/DocStudio";

/**
 * Estudio de documentos como pantalla (deep-link). Ya NO vive en el menú lateral: el acceso
 * natural es DENTRO de un Proyecto (panel Studio) y como acción rápida del Chat. Esta ruta se
 * conserva para enlaces directos y comparte exactamente el mismo componente.
 */
export default function DocumentsPage() {
  return (
    <AppShell title="Documentos">
      <div className="max-w-5xl mx-auto p-6 flex flex-col gap-6">
        <div>
          <h1 className="font-display text-2xl font-bold" style={{ color: "var(--text-1)" }}>Documentos</h1>
          <p className="text-sm mt-1" style={{ color: "var(--text-3)" }}>
            Elige o extrae un estilo, ajústalo, duplícalo o guárdalo, y genera ofertas con esa apariencia.
          </p>
        </div>
        <DocStudio />
      </div>
    </AppShell>
  );
}
