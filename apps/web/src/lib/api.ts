// Clientes de AION: control-plane (auth) y puente del núcleo (chat).

export const CONTROL_URL =
  process.env.NEXT_PUBLIC_CONTROL_URL ?? "http://127.0.0.1:8787";
export const BRIDGE_URL =
  process.env.NEXT_PUBLIC_BRIDGE_URL ?? "http://127.0.0.1:8765";

// ── Auth local del puente (P0-1 fase 2) ──────────────────────────────────────
// El backend exige un Bearer local en toda mutación de /api/*. En vez de tocar ~30
// call sites (y arriesgar olvidar uno → feature rota), se intercepta `fetch` UNA vez
// para inyectar el token SOLO en peticiones al puente. El token se obtiene de
// /api/auth/token (GET, sin auth, protegido por Origin local) y se cachea en memoria
// (NO en localStorage: así otra web local no puede leerlo). Las lecturas (GET) no lo
// necesitan, pero adjuntarlo es inocuo. Idempotente: solo parchea una vez en cliente.
const TOKEN_PATH = "/api/auth/token";
if (typeof window !== "undefined" && !(window as unknown as { __aionPatched?: boolean }).__aionPatched) {
  (window as unknown as { __aionPatched?: boolean }).__aionPatched = true;
  const rawFetch = window.fetch.bind(window);
  let tokenPromise: Promise<string> | null = null;
  const apiToken = (): Promise<string> => {
    if (!tokenPromise) {
      tokenPromise = rawFetch(`${BRIDGE_URL}${TOKEN_PATH}`)
        .then((r) => (r.ok ? r.json() : { token: "" }))
        .then((j: { token?: string }) => j.token ?? "")
        .catch(() => "");
    }
    return tokenPromise;
  };
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url =
      typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    // Solo peticiones al puente, y nunca el propio bootstrap del token (evita recursión).
    if (!url.startsWith(BRIDGE_URL) || url.includes(TOKEN_PATH)) return rawFetch(input, init);
    const token = await apiToken();
    if (!token) return rawFetch(input, init);
    const headers = new Headers(
      init?.headers ?? (input instanceof Request ? input.headers : undefined),
    );
    if (!headers.has("Authorization")) headers.set("Authorization", `Bearer ${token}`);
    return rawFetch(input, { ...init, headers });
  };
}

/**
 * Voz de AION (TTS local). Pide el audio al núcleo (que delega en el sidecar
 * Kokoro/Chatterbox) y devuelve el WAV como Blob. Lanza si el sidecar no está
 * (la capa de voz cae entonces a la voz del sistema). El Bearer se inyecta solo.
 */
export async function ttsSpeak(
  text: string,
  lang: string,
  opts?: { voice?: string; engine?: string; speed?: number },
): Promise<Blob> {
  const res = await fetch(`${BRIDGE_URL}/api/tts`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      text,
      lang,
      voice: opts?.voice ?? "",
      engine: opts?.engine ?? "",
      speed: opts?.speed ?? 1.0,
    }),
  });
  if (!res.ok) throw new Error(`tts ${res.status}`);
  return res.blob();
}

/** Voces clonadas disponibles (clips de referencia subidos por el usuario). */
export async function ttsVoices(): Promise<{ cloned: string[] }> {
  try {
    return await fetch(`${BRIDGE_URL}/api/tts/voices`).then((r) => r.json());
  } catch {
    return { cloned: [] };
  }
}

/** Sube un clip de referencia y lo registra como voz clonable (Chatterbox). */
export async function ttsCloneUpload(
  name: string,
  file: File,
): Promise<{ ok: boolean; voice?: string; error?: string }> {
  const b64 = await new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result).split(",")[1] ?? "");
    r.onerror = () => reject(new Error("no pude leer el archivo"));
    r.readAsDataURL(file);
  });
  const ext = (file.name.split(".").pop() || "wav").toLowerCase();
  return fetch(`${BRIDGE_URL}/api/tts/clone`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, ext, content_b64: b64 }),
  }).then((r) => r.json());
}

export async function ttsCloneRemove(name: string): Promise<{ ok: boolean }> {
  return fetch(`${BRIDGE_URL}/api/tts/clone/remove`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name }),
  })
    .then((r) => r.json())
    .catch(() => ({ ok: false }));
}

export type AuthResult = {
  id: string;
  email: string;
  token: string;
  recovery_code?: string;
};

async function authCall(path: string, email: string, password: string): Promise<AuthResult> {
  const res = await fetch(`${CONTROL_URL}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error ?? "error de autenticación");
  return data as AuthResult;
}

export const register = (email: string, password: string) =>
  authCall("/auth/register", email, password);
export const login = (email: string, password: string) =>
  authCall("/auth/login", email, password);

/// Recuperación de contraseña (local-first): email + código de recuperación + nueva.
export const resetPassword = async (email: string, code: string, newPassword: string) => {
  const res = await fetch(`${CONTROL_URL}/auth/reset`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, recovery_code: code, new_password: newPassword }),
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error ?? "código o email inválido");
  return data as { ok: boolean };
};

export type ChatEvent =
  | { kind: "thinking"; text: string }
  | { kind: "answer"; text: string }
  | { kind: "done"; tokens: number; tps: number }
  | { kind: "error"; text: string };

export type AgentEvent =
  | { kind: "thought"; text: string; agent?: string }
  | { kind: "action"; text: string; agent?: string }
  | { kind: "observation"; text: string; agent?: string }
  | { kind: "answer"; text: string; steps?: number; agent?: string }
  | { kind: "confirm"; id: string; text: string }
  | { kind: "ask"; id: string; text: string }
  | { kind: "done" }
  | { kind: "error"; text: string };

/** Aprueba o rechaza una acción sensible que AION pidió confirmar (HITL). */
export async function confirmDecision(id: string, approved: boolean): Promise<void> {
  await fetch(`${BRIDGE_URL}/api/confirm`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id, approved }),
  }).catch(() => {});
}

/** Backup COMPLETO de AION (memoria + personas + skills + bandeja + biblioteca + proyectos).
 *  mode "keep" = migrar (incluye el id: mismo agente). "strip" = clon (sin id → nuevo individuo). */
export const agentExportUrl = (mode: "keep" | "strip", intent: "migrar" | "reparar" | "clonar") =>
  `${BRIDGE_URL}/api/agent/export?identity=${mode}&intent=${intent}`;
/** Descarga el .aion (vía blob, para poder encadenar acciones como el auto-borrado). */
export async function downloadAgent(
  mode: "keep" | "strip",
  intent: "migrar" | "reparar" | "clonar",
  filename: string,
): Promise<boolean> {
  try {
    const res = await fetch(agentExportUrl(mode, intent));
    if (!res.ok) return false;
    const blob = await res.blob();
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
    return true;
  } catch {
    return false;
  }
}
export async function agentImport(content_b64: string): Promise<{ ok: boolean; restored?: number; error?: string }> {
  return jpost("/api/agent/import", { content_b64 });
}
/** Borra toda la existencia local (completa una migración). Destructivo. */
export async function agentWipe(): Promise<{ ok: boolean; removed?: number }> {
  return jpost("/api/agent/wipe", {});
}
export type A2aPeer = { name: string; url: string };
export type A2aConfig = { enabled: boolean; token: string; peers: A2aPeer[] };
export async function a2aGet(): Promise<{ config: A2aConfig; identity: AionIdentity | null }> {
  try {
    return await fetch(`${BRIDGE_URL}/api/a2a`).then((x) => x.json());
  } catch {
    return { config: { enabled: false, token: "", peers: [] }, identity: null };
  }
}
export const a2aSet = (config: A2aConfig) => jpost<{ ok: boolean }>("/api/a2a", config);
export const a2aSend = (url: string, message: string) =>
  jpost<{ ok?: boolean; reply?: string; name?: string; error?: string }>("/api/a2a/send", { url, message });

// ── Claude Code (memoria compartida vía MCP) ─────────────────────────────────
export type ClaudeCodeStatus = {
  enabled: boolean;
  auto_brief: boolean;
  created_at?: string | null;
  last_seen_at?: string | null;
  registered: boolean;
  cli_found: boolean;
};
export type ClaudeCodeAuditEntry = {
  ts: string;
  tool: string;
  query: string;
  result_chars: number;
  est_tokens: number;
  ok: boolean;
};
export type ClaudeCodeStats = {
  total_calls: number;
  by_tool: Record<string, number>;
  by_tool_tokens?: Record<string, number>;
  tokens_served: number;
  writes: number;
  errors?: number;
  full_dump_tokens: number;
  memory_count?: number;
  avg_tokens_per_call?: number;
  savings_pct?: number;
  total_savings_est?: number;
  graph_concepts?: number;
  graph_communities?: number;
  last_activity?: string | null;
  // Ahorro de la traducción ES→EN del puente (distinto del ahorro del RAG de arriba).
  tokens_saved_translation?: number;
  translation_savings_pct?: number;
  by_tool_translation?: Record<string, number>;
  sessions?: {
    started_at: string;
    calls: number;
    tokens_served: number;
    tokens_saved: number;
  }[];
};
export async function claudeCodeGet(): Promise<ClaudeCodeStatus> {
  try {
    return await fetch(`${BRIDGE_URL}/api/claude-code`).then((x) => x.json());
  } catch {
    return { enabled: false, auto_brief: false, registered: false, cli_found: false };
  }
}
export const claudeCodeConnect = (auto_brief?: boolean) =>
  jpost<{ ok: boolean; error?: string }>("/api/claude-code/connect", { auto_brief });
export const claudeCodeSet = (cfg: { auto_brief?: boolean }) =>
  jpost<{ ok: boolean }>("/api/claude-code", cfg);
export const claudeCodeDisconnect = () =>
  jpost<{ ok: boolean }>("/api/claude-code/disconnect", {});
export const claudeCodeTest = () =>
  jpost<{ ok: boolean; enabled: boolean; registered: boolean; cli_found: boolean; last_seen_at?: string | null }>(
    "/api/claude-code/test",
    {},
  );
export async function claudeCodeAudit(limit = 200): Promise<ClaudeCodeAuditEntry[]> {
  try {
    const r = await fetch(`${BRIDGE_URL}/api/claude-code/audit?limit=${limit}`).then((x) => x.json());
    return (r.entries as ClaudeCodeAuditEntry[]) ?? [];
  } catch {
    return [];
  }
}
export async function claudeCodeStats(): Promise<ClaudeCodeStats | null> {
  try {
    return await fetch(`${BRIDGE_URL}/api/claude-code/stats`).then((x) => x.json());
  } catch {
    return null;
  }
}

export type AionIdentity = { id: string; name: string; born_at: string };
export async function getIdentity(): Promise<AionIdentity | null> {
  try {
    const r = await fetch(`${BRIDGE_URL}/api/identity`).then((x) => x.json());
    return (r.identity as AionIdentity) ?? null;
  } catch {
    return null;
  }
}

/** Saludo proactivo de AION al abrir (cálido, con continuidad). Vacío si no hay. */
export async function getGreeting(): Promise<string> {
  try {
    const r = await fetch(`${BRIDGE_URL}/api/greeting`, { method: "POST" }).then((x) => x.json());
    return (r.text as string) ?? "";
  } catch {
    return "";
  }
}

/** Responde (en texto) a una pregunta que el agente hizo a mitad de tarea. */
export async function answerQuestion(id: string, text: string): Promise<void> {
  await fetch(`${BRIDGE_URL}/api/ask`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id, text }),
  }).catch(() => {});
}

/** Lee un cuerpo SSE y entrega cada evento `data:` parseado a `onEvent`. */
async function readSse<T>(res: Response, onEvent: (e: T) => void): Promise<void> {
  // Si el backend respondió un error (modelo no listo, auth/CORS, 5xx…), el cuerpo
  // NO es SSE: sin esta comprobación readSse drenaría el body sin emitir nada y
  // resolvería en silencio → la UI se cuelga en una burbuja vacía sin error. Hacer
  // throw aquí propaga el fallo al try/catch de quien llama (chat/agent/crew/mind/pull).
  if (!res.ok) {
    let detail = "";
    try {
      detail = (await res.text()).slice(0, 200);
    } catch {
      /* sin cuerpo legible */
    }
    throw new Error(`backend ${res.status}${detail ? `: ${detail}` : ""}`);
  }
  if (!res.body) throw new Error("sin cuerpo de respuesta");
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    const parts = buf.split("\n\n");
    buf = parts.pop() ?? "";
    for (const part of parts) {
      const line = part.trim();
      if (!line.startsWith("data:")) continue;
      try {
        onEvent(JSON.parse(line.slice(5).trim()) as T);
      } catch {
        /* fragmento parcial */
      }
    }
  }
}

/** Chat con streaming de razonamiento + respuesta. */
/** Idioma elegido por el usuario (para que AION responda en él). */
function lang(): string {
  if (typeof window === "undefined") return "es";
  return localStorage.getItem("aion_lang") || "es";
}

export async function chatStream(
  prompt: string,
  think: boolean,
  onEvent: (e: ChatEvent) => void,
  convoId?: string,
  projectId?: string,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/chat`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      prompt,
      think,
      lang: lang(),
      convo_id: convoId ?? "default",
      project_id: projectId,
    }),
    signal,
  });
  await readSse(res, onEvent);
}

// ── Proyectos (workspace estilo NotebookLM) ─────────────────────────────────

export type Project = { id: string; name: string; desc: string; icon: string; created: string; updated: string };
export type ProjectSource = { id: string; title: string; kind: string; content: string; active: boolean; created: string };
export type ProjectOutput = { id: string; kind: string; title: string; content: string; created: string; audio?: string };

async function jpost<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BRIDGE_URL}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return (await res.json()) as T;
}

export async function projectsList(): Promise<Project[]> {
  const r = await fetch(`${BRIDGE_URL}/api/projects`).then((x) => x.json()).catch(() => ({ projects: [] }));
  return (r.projects ?? []) as Project[];
}
export const projectCreate = (name: string, desc: string, icon: string) =>
  jpost<{ ok: boolean; project?: Project; error?: string }>("/api/projects", { name, desc, icon });
export const projectRemove = (id: string) => jpost<{ ok: boolean }>("/api/projects/remove", { id });
export const projectUpdate = (id: string, name: string, desc: string) =>
  jpost<{ ok: boolean; project?: Project; error?: string }>("/api/project/update", { id, name, desc });
export const projectGet = (id: string) =>
  jpost<{ ok: boolean; project?: Project; sources?: ProjectSource[]; outputs?: ProjectOutput[]; error?: string }>(
    "/api/project/get",
    { id },
  );
export const projectSourceAdd = (project_id: string, title: string, kind: string, content: string) =>
  jpost<{ ok: boolean; source?: ProjectSource; error?: string }>("/api/project/source/add", {
    project_id,
    title,
    kind,
    content,
  });
export const projectSourceUpload = (project_id: string, filename: string, content_b64: string) =>
  jpost<{ ok: boolean; source?: ProjectSource; error?: string }>("/api/project/source/upload", {
    project_id,
    filename,
    content_b64,
  });
export const projectSourceToggle = (project_id: string, id: string, active: boolean) =>
  jpost<{ ok: boolean }>("/api/project/source/toggle", { project_id, id, active });
export const projectSourceRemove = (project_id: string, id: string) =>
  jpost<{ ok: boolean }>("/api/project/source/remove", { project_id, id });
export type DiscoverResult = { title: string; url: string; snippet: string };
export const projectDiscover = (project_id: string, query: string) =>
  jpost<{ ok: boolean; results?: DiscoverResult[]; error?: string }>("/api/project/discover", {
    project_id,
    query,
  });
/** Genera un Audio Overview (guion hablado + síntesis con el TTS del sistema). */
export const projectStudioAudio = (project_id: string) =>
  jpost<{ ok: boolean; output?: ProjectOutput; error?: string }>("/api/project/studio/audio", {
    project_id,
    lang: lang(),
  });
/** URL para reproducir el audio de una salida de Studio. */
export const projectAudioUrl = (project_id: string, file: string) =>
  `${BRIDGE_URL}/api/project/audio?project_id=${encodeURIComponent(project_id)}&file=${encodeURIComponent(file)}`;
export const projectStudioGenerate = (project_id: string, kind: string) =>
  jpost<{ ok: boolean; output?: ProjectOutput; error?: string }>("/api/project/studio/generate", {
    project_id,
    kind,
    lang: lang(),
  });
export const projectStudioRemove = (project_id: string, id: string) =>
  jpost<{ ok: boolean }>("/api/project/studio/remove", { project_id, id });

/** Reinicia el hilo de una conversación en el backend (nuevo chat). */
export async function chatReset(convoId: string): Promise<void> {
  await fetch(`${BRIDGE_URL}/api/chat/new`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ convo_id: convoId }),
  }).catch(() => {});
}

/** Agente ReAct con herramientas: emite pasos (thought/action/observation) + answer.
 * `context`: últimos turnos de la conversación — sin él, una tarea referencial
 * («puedes buscarlo tú») llega huérfana al backend y el modelo alucina el antecedente. */
export async function agentStream(
  task: string,
  onEvent: (e: AgentEvent) => void,
  context?: string,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/agent`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ task, lang: lang(), context }),
    signal,
  });
  await readSse(res, onEvent);
}

/** Equipo multiagente: orquestador + especialistas. Emite la actividad por rol. */
export async function crewStream(
  task: string,
  onEvent: (e: AgentEvent) => void,
  context?: string,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/crew`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ task, lang: lang(), context }),
    signal,
  });
  await readSse(res, onEvent);
}

// ── Adjuntos: documentos (biblioteca) y fotos (visión) ──────────────────

/** Sube un documento (.pdf/.txt/.md) a la biblioteca, bajo un dominio. */
export async function libraryUpload(
  domain: string,
  filename: string,
  contentB64: string,
): Promise<{ ok: boolean; passages: number; source: string; total_chunks: number }> {
  const res = await fetch(`${BRIDGE_URL}/api/library/upload`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ domain, filename, content_b64: contentB64 }),
  });
  const data = await res.json();
  if (data.error) throw new Error(data.error);
  return data;
}

export type LibraryDoc = { domain: string; source: string; chunks: number };

/** Lista los documentos de la biblioteca (agrupados por dominio/fuente). */
export async function libraryList(): Promise<{ total_chunks: number; documents: LibraryDoc[] }> {
  return jsonCall(`/api/library`);
}

/** Encola un libro para ingesta en segundo plano (no bloquea). */
export async function libraryEnqueue(
  domain: string,
  filename: string,
  contentB64: string,
): Promise<{ ok: boolean; id: string; queued: string }> {
  return jsonCall(`/api/library/enqueue`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ domain, filename, content_b64: contentB64 }),
  });
}

export type QueueStatus = {
  pending: number;
  processing: number;
  done: number;
  error: number;
  current: string | null;
};

/** Estado de la cola de ingesta. */
export async function libraryQueue(): Promise<QueueStatus> {
  return jsonCall(`/api/library/queue`);
}

/** Limpia de la cola los trabajos terminados. */
export async function libraryQueueClear(): Promise<{ cleared: number }> {
  return jsonCall(`/api/library/queue/clear`, { method: "POST" });
}

/** Elimina un documento de la biblioteca. */
export async function libraryRemove(domain: string, source: string): Promise<{ removed: number }> {
  return jsonCall(`/api/library/remove`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ domain, source }),
  });
}

/** Pregunta a la biblioteca: respuesta fundamentada con fuentes. */
export async function libraryAsk(
  query: string,
  domain?: string,
): Promise<{ answer: string; sources: { n: number; domain: string; source: string; idx: number; score: number }[] }> {
  return jsonCall(`/api/library/ask`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ query, domain }),
  });
}

/** Analiza una imagen adjunta con visión (gemma multimodal, local). */
export async function visionAsk(prompt: string, imageB64: string): Promise<string> {
  const res = await fetch(`${BRIDGE_URL}/api/vision`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt, image_b64: imageB64 }),
  });
  const data = await res.json();
  if (data.error) throw new Error(data.error);
  return data.answer as string;
}

// ── Bóveda de credenciales (Llavero; la contraseña nunca se devuelve) ────

export type CredMeta = { host: string; user: string };

export async function credentialsList(): Promise<CredMeta[]> {
  const r = await jsonCall<{ credentials: CredMeta[] }>(`/api/credentials`);
  return r.credentials;
}

export async function credentialSet(host: string, user: string, pass: string): Promise<void> {
  await jsonCall(`/api/credentials`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ host, user, pass }),
  });
}

export async function credentialRemove(host: string): Promise<void> {
  await jsonCall(`/api/credentials/remove`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ host }),
  });
}

// ── APIs externas opcionales (gratis) que el usuario añade en Ajustes ────
// La clave NUNCA se devuelve al cliente: el backend solo informa si está puesta (`set`).

export type ApiKeyMeta = { provider: string; label: string; help: string; set: boolean };

export async function apiKeysList(): Promise<ApiKeyMeta[]> {
  const r = await jsonCall<{ keys: ApiKeyMeta[] }>(`/api/apikeys`);
  return r.keys;
}

export async function apiKeySet(provider: string, key: string): Promise<void> {
  await jsonCall(`/api/apikeys`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ provider, key }),
  });
}

// ── Memoria de largo plazo ──────────────────────────────────────────────

export type MemoryStats = { count: number; path: string };
export type SleepReport = { before: number; merged: number; pruned: number; after: number };

async function jsonCall<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BRIDGE_URL}${path}`, init);
  const data = await res.json();
  if (data.error) throw new Error(data.error);
  return data as T;
}

export const memoryStats = () => jsonCall<MemoryStats>("/api/memory");

export type GraphStats = {
  nodes: number;
  edges: number;
  concepts: number;
  sources: number;
  communities: number;
  community_edges: number;
};
// Estadísticas del grafo de conocimiento (conceptos, comunidades).
export const graphStats = () => jsonCall<GraphStats>("/api/graph/stats");

export const memoryRemember = (text: string) =>
  jsonCall<{ ok: boolean; count: number }>("/api/memory/remember", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ text }),
  });

export const memorySleep = () =>
  jsonCall<SleepReport>("/api/memory/sleep", { method: "POST" });

/// Descarga la memoria como archivo JSONL (para llevarla a otro PC/Mac).
export const memoryExport = () => fetch(`${BRIDGE_URL}/api/memory/export`);

/// Importa memoria desde un archivo JSONL (fusiona, omite duplicados).
export const memoryImport = (jsonl: string) =>
  jsonCall<{ ok: boolean; added: number; count: number }>("/api/memory/import", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonl }),
  });

// ── Bandeja de AION (mensajes proactivos del agente hacia ti) ───────────────

export type InboxMessage = {
  id: string;
  at: string;
  kind: string;
  text: string;
  read: boolean;
};

// ── Onboarding: escaneo de hardware, modelos y proveedor ────────────────────

export type SystemScan = {
  os: string;
  arch: string;
  cpu_cores: number;
  ram_gb: number;
  disk_free_gb: number;
  gpu: string;
  tier: string;
  tier_reason: string;
};
export type ModelOption = {
  id: string;
  name: string;
  ollama_name: string;
  size_gb: number;
  tier: string;
  note: string;
  recommended: boolean;
};

export const systemScan = () =>
  jsonCall<{ scan: SystemScan; catalog: ModelOption[] }>("/api/system/scan");

export const providerSet = (cfg: {
  kind: string;
  model: string;
  base_url?: string;
  api_key?: string;
}) =>
  jsonCall<{ ok?: boolean; error?: string }>("/api/provider", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ base_url: "", api_key: "", ...cfg }),
  });

export type ProviderState = {
  kind: string;
  model: string;
  base_url: string;
  has_key: boolean;
  local_model: string;
  ext_model: string;
  can_toggle: boolean;
};
// Lee el proveedor activo. NUNCA devuelve la API key (solo `has_key`).
export const providerGet = () => jsonCall<ProviderState>("/api/provider");

// Alterna en un clic el motor activo local↔API (solo si ambos están configurados).
export const providerToggle = () =>
  jsonCall<{ ok?: boolean; kind?: string; model?: string; has_key?: boolean; error?: string }>(
    "/api/provider/toggle",
    { method: "POST" },
  );

export const governanceSetup = (posture: string) =>
  jsonCall<{ ok: boolean }>("/api/governance/setup", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ posture }),
  });

// ── Catálogo REAL de herramientas del agente (dashboard sincronizado) ──
export type ToolInfo = { name: string; description: string; sensitive: boolean };
export type ToolGroup = { category: string; tools: ToolInfo[] };
export const toolsList = () =>
  jsonCall<{ count: number; groups: ToolGroup[] }>("/api/tools");

// ── Comunicaciones: gobernanza por contacto y canal ──
export type CommContact = {
  id: string;
  name: string;
  handle: string;
  channels: string[];
  allow_read: boolean;
  allow_send: boolean;
  note: string;
};
export type CommsPolicy = {
  enabled: boolean;
  default_allow: boolean;
  channels: string[];
  contacts: CommContact[];
};
export const commsGet = () => jsonCall<CommsPolicy>("/api/comms");
export const commsSet = (p: {
  enabled: boolean;
  default_allow: boolean;
  contacts: CommContact[];
}) =>
  jsonCall<{ ok?: boolean; error?: string }>("/api/comms", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(p),
  });

// ── Flujos de trabajo (tipo n8n) ──
export type WorkflowTrigger =
  | { type: "manual" }
  | { type: "interval"; minutes: number }
  | { type: "event"; kind: string };
export type WorkflowStep = { tool: string; input: string };
export type Workflow = {
  id: string;
  name: string;
  description: string;
  trigger: WorkflowTrigger;
  steps: WorkflowStep[];
  enabled: boolean;
  last_run_ms?: number | null;
};
export type StepResult = {
  tool: string;
  input: string;
  output: string;
  ok: boolean;
  needs_approval: boolean;
};
export type WorkflowRun = {
  workflow_id: string;
  steps: StepResult[];
  ok: boolean;
  stopped_for_approval: boolean;
};

export const workflowsList = () => jsonCall<{ workflows: Workflow[] }>("/api/workflows");
export const workflowsSet = (wf: Workflow) =>
  jsonCall<{ ok?: boolean; count?: number; error?: string }>("/api/workflows", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(wf),
  });
export const workflowsRemove = (id: string) =>
  jsonCall<{ ok?: boolean; error?: string }>("/api/workflows/remove", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id }),
  });
export const workflowsRun = (id: string) =>
  jsonCall<WorkflowRun & { error?: string }>("/api/workflows/run", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id }),
  });

/// Descarga un modelo local con progreso (SSE).
export type InstalledModel = { name: string; size_gb: number };

/** Modelos locales ya instalados en Ollama. */
export async function modelsInstalled(): Promise<InstalledModel[]> {
  const r = await jsonCall<{ installed: InstalledModel[] }>(`/api/models/installed`);
  return r.installed;
}

/** Elimina un modelo local (no permite borrar el que está en uso). */
export async function modelsRemove(model: string): Promise<void> {
  await jsonCall(`/api/models/remove`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model }),
  });
}

export async function modelsPull(
  model: string,
  onEvent: (e: { kind: string; status?: string; percent?: number; text?: string }) => void,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/models/pull`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model }),
  });
  await readSse(res, onEvent);
}

/// Estado de preparación del motor/modelo (para mostrar "descargando…" en 1er arranque).
export const status = () =>
  jsonCall<{ engine_up: boolean; model_ready: boolean; engine: string }>("/api/status");

export const inboxList = () =>
  jsonCall<{ unread: InboxMessage[]; unread_count: number; all: InboxMessage[] }>("/api/inbox");

export const inboxRead = (id?: string) =>
  jsonCall<{ ok: boolean }>("/api/inbox/read", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(id ? { id } : {}),
  });

/* ── Mente: corriente de conciencia (GWT), estado interno e índice Φ ──────── */

export type MindEvent = { at: number; source: string; kind: string; text: string };

export type InnerStateInfo = {
  focus: string;
  focus_since: number;
  curiosity: string;
  certainty: number;
  mood: string;
  recent_outcomes: boolean[];
  last_task_steps: number;
  competence: number;
  observations: number;
  updated_at: number;
};

export type ConsciousnessInfo = {
  index: number;
  components: {
    integration: number;
    recurrence: number;
    metacognition: number;
    coherence: number;
  };
  measurements: number;
  history: { at: number; score: number }[];
};

/** Corriente de conciencia en vivo (SSE). Cancelable vía `signal`. */
export async function mindStream(
  onEvent: (e: MindEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/stream`, { signal });
  await readSse(res, onEvent);
}

export const innerState = () => jsonCall<InnerStateInfo>("/api/inner");
export const consciousness = () => jsonCall<ConsciousnessInfo>("/api/consciousness");

// Existencia: dimensiones de autonomía/presencia/curiosidad (datos reales).
export type ExistenceInfo = {
  debts_open: number;
  seconds_since_user: number | null;
  curiosity: { goals: number; learning: number; top: string };
  journal: {
    entries: number;
    last: { at: number; text: string; dominant: string } | null;
  };
  capabilities: { tool_families: number; skills: number };
  host: {
    battery_pct: number | null;
    power: string | null;
    thermal: string | null;
    uptime: string | null;
    ram_gb: number;
    cpu_cores: number;
    gpu: string;
  };
};
export const existence = () => jsonCall<ExistenceInfo>("/api/existence");

// Diario de existencia: las jornadas que AION cierra por su cuenta, en primera persona.
export type JournalEntry = {
  id: string;
  at: number;
  text: string;
  dominant: string;
  debts_resolved: number;
};
export const journal = () =>
  jsonCall<{ count: number; entries: JournalEntry[] }>("/api/journal");

export type SensorConfig = {
  enabled: boolean;
  lat: number | null;
  lon: number | null;
  place: string;
};

export const sensorsGet = () => jsonCall<SensorConfig>("/api/sensors");

export const sensorsSet = (cfg: SensorConfig) =>
  jsonCall<{ ok: boolean }>("/api/sensors", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(cfg),
  });
