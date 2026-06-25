"use client";

// Página de FLUJOS DE TRABAJO — editor visual por grafo (React Flow). Reemplaza la antigua UI
// lineal; los flujos lineales ya creados se importan con «Migrar» desde el propio editor.

import { AppShell } from "@/components";
import { FlowEditor } from "@/components/flow/FlowEditor";

export default function WorkflowsPage() {
  return (
    <AppShell title="Flujos de trabajo">
      <div className="h-full min-h-0">
        <FlowEditor />
      </div>
    </AppShell>
  );
}
