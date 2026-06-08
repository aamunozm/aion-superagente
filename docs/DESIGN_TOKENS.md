# AION — Design Tokens

Identidad visual propia, **inspirada** en la filosofía premium de CEO·Intelligence (no copiada).
Fuente de verdad: `packages/design-system/src/tokens/tokens.css` y `tokens.ts`.

## Filosofía
Minimalismo Apple · **un único acento focal** · escala tipográfica de 6 roles · sombras sutiles ·
dark mode invertido (pill premium) · easing premium · estados desaturados para sesiones largas.

## Identidad AION vs CEO·Intelligence
| | CEO·Intelligence | **AION** |
|---|---|---|
| Base/ink | slate-900 `#0F172A` | slate-900 `#0F172A` (misma filosofía) |
| Acento focal | dorado `#C69A24` | **plasma teal `#0FB5BA`** (inteligencia viva) |
| Tipografías | Space Grotesk / Inter / JetBrains Mono | iguales |

## Tokens principales
- **Ink:** `#0F172A` (primary/CTA) · hover `#1E293B`
- **Acento:** `#0FB5BA` (light) / `#2DD4D9` (dark) — único, reservado a valor/estado activo
- **Texto:** `#0A0A0A` / `#555` / `#999`
- **Radios:** input/btn 10px · card 12px · pill ∞
- **Sombras:** elevated `0 4px 20px rgba(0,0,0,.06)` · float `0 8px 30px rgba(0,0,0,.10)`
- **Easing:** premium `cubic-bezier(0.22,1,0.36,1)` · spring `cubic-bezier(0.34,1.56,0.64,1)`

## Estados cognitivos (telemetría del agente, exclusivo AION)
`thinking` teal · `dreaming` violeta · `evolving` azul · `curious` ámbar · `idle` gris.
Visualizan en la UI en qué "estado mental" está el agente (pensando, soñando, evolucionando…).

## Escala tipográfica (6 roles)
`t-title 15` · `t-section 13` · `t-body-strong 12/500` · `t-body 12/400` · `t-meta 11` · `t-micro 10 upper`.
