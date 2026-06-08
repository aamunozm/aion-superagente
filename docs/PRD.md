# AION — PRD técnico (vivo)

## Visión
Un super-agente de IA local-first que razona, recuerda (memoria darwiniana), aprende, crea y
evoluciona — propiedad y privacidad del usuario por diseño.

## Usuarios y valor
- **Usuario pro/founder** (perfil Ariel): agente autónomo privado para trabajo de conocimiento,
  arquitectura, investigación, automatización — sin enviar datos a terceros.
- **Diferenciadores:** local-first real, memoria darwiniana (IP), autonomía con autoobjetivos,
  sin censura (modelo abliterated local), estética premium.

## MVP (F0 + F1) — criterios de éxito
1. App de escritorio firmada (Tauri) que arranca y valida licencia **offline**.
2. Registro + login + **pago Stripe** (test) → licencia firmada Ed25519.
3. **Chat** con Gemma 4 local: streaming de razonamiento (`<think>`) + respuesta.
4. **RAG** mínimo sobre documentos locales (LanceDB + nomic-embed).
5. Privacidad: la nube solo ve metadatos; `sync_blobs` cifrados E2E.

## Fuera de alcance del MVP
Browser agéntico, auto-mejora, memoria darwiniana, móvil (F3–F6).

## Métricas
- Activación: % usuarios que completan registro→pago→primer chat.
- Rendimiento: tok/s local (objetivo ≥ ~18 en M2 Max, paridad con prototipo).
- Privacidad: 0 contenido cognitivo en el control-plane (auditable).

## Roadmap
F0 cimientos · F1 MVP cobrable · F2 cerebro · F3 skills · F4 evolución · F5 autonomía · F6 escala/móvil.
Detalle: plan maestro.
