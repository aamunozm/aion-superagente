// Clientes de AION: control-plane (auth) y puente del núcleo (chat).

export const CONTROL_URL =
  process.env.NEXT_PUBLIC_CONTROL_URL ?? "http://127.0.0.1:8787";
export const BRIDGE_URL =
  process.env.NEXT_PUBLIC_BRIDGE_URL ?? "http://127.0.0.1:8765";

export type AuthResult = { id: string; email: string; token: string };

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

export type ChatEvent =
  | { kind: "thinking"; text: string }
  | { kind: "answer"; text: string }
  | { kind: "done"; tokens: number; tps: number }
  | { kind: "error"; text: string };

/** Llama al puente de chat y entrega cada evento SSE a `onEvent`. */
export async function chatStream(
  prompt: string,
  think: boolean,
  onEvent: (e: ChatEvent) => void,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/chat`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt, think }),
  });
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
        onEvent(JSON.parse(line.slice(5).trim()) as ChatEvent);
      } catch {
        /* fragmento parcial */
      }
    }
  }
}
