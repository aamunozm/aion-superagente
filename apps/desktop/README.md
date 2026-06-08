# AION Desktop (Tauri)

Shell de escritorio que empaqueta la UI web (`@aion/web`) como app nativa de
macOS/Windows y la conecta con el núcleo Rust.

## Estado
Scaffold listo (config v2 + entrypoint). **Build pendiente** de:
1. `cargo install tauri-cli` (CLI de Tauri).
2. Iconos en `src-tauri/icons/` (`cargo tauri icon <png>` los genera).
3. Núcleo como sidecar: por ahora se ejecuta aparte con `aion-core serve`.

## Desarrollo
```bash
# 1) núcleo (puente HTTP)
cargo run --bin aion-core -- serve 127.0.0.1:8765
# 2) control-plane
cargo run -p aion-control-plane
# 3) desktop (levanta la web y la ventana nativa)
cd apps/desktop && cargo tauri dev
```

## Build de producción
```bash
cd apps/desktop && cargo tauri build   # genera .app/.dmg/.msi
```
Pendiente F1: firma (Apple Developer / Windows cert) y sidecar del núcleo.
