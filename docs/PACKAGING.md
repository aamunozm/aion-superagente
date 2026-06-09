# AION — Empaquetado autocontenido (sin Docker, sin Ollama)

AION se distribuye como **una sola app instalable** que lleva **todo dentro**: el
motor de IA (Ollama embebido), su runner nativo y el backend. El usuario final
**no instala Docker ni Ollama ni nada**: doble clic y funciona.

## Qué va dentro de la app

| Componente | Cómo se empaqueta | Notas |
|---|---|---|
| **Motor LLM (Ollama)** | Runtime portátil vendorizado en `Resources/ollama-runtime/` | Binario universal arm64+x86_64 en macOS; `ollama.exe` en Windows. Arranca en puerto privado `127.0.0.1:11919` para no chocar con un Ollama externo. |
| **Runner `llama-server` + libs** | Junto al binario ollama | Carga sus dylibs por `@loader_path` (macOS) → carpeta **portátil** (verificado ejecutando desde `/tmp`, fuera de Ollama.app). |
| **Núcleo `aion-core`** | Sidecar Tauri (`externalBin`) | chat / agente / memoria / skills / evolución. Apunta al Ollama embebido vía `AION_OLLAMA_URL`. |
| **Control-plane** | Sidecar Tauri | auth / licencias Ed25519. |
| **Modelo `gemma4-reason` (~9 GB)** | **NO** se empaqueta | Se descarga/crea en **primer arranque** (`aion-core models-ensure`) con aviso al usuario. Bundlearlo haría la app de >9 GB. |

Tamaño del `.app` macOS universal: **~524 MB** (sin el modelo).

## Cobertura de plataformas

| Plataforma | Estado | Cómo |
|---|---|---|
| **macOS Apple Silicon** | ✅ Construido e instalado | `.app` universal |
| **macOS Intel** | ✅ Cubierto por el universal | mismo `.app` (binarios arm64+x86_64) |
| **Windows x64** | ⚙️ Listo para CI | requiere runner Windows (MSVC+WebView2); no se puede compilar desde Mac |
| iPhone / Android | ⏳ F6 (Capacitor + LLM on-device) | fuera del alcance actual |

## Cómo construir

### macOS (universal, desde un Mac)
```bash
# 1) Vendorizar el runtime Ollama (requiere Ollama.app instalado como fuente)
bash apps/desktop/scripts/vendor-ollama-runtime.sh
# 2) Construir el .app universal (Silicon + Intel)
bash apps/desktop/build-universal.sh
# Resultado: apps/desktop/src-tauri/target/universal-apple-darwin/release/bundle/macos/AION.app
```

### Windows (en CI o en una máquina Windows)
```powershell
# 1) Vendorizar el runtime Ollama de Windows
powershell -ExecutionPolicy Bypass -File apps/desktop/scripts/vendor-ollama-runtime-windows.ps1
# 2) Compilar sidecars + instalador NSIS  (ver .github/workflows/release-desktop.yml)
cargo tauri build --bundles nsis
```

### CI (ambos a la vez)
`.github/workflows/release-desktop.yml` construye macOS universal (+DMG) y el
instalador Windows NSIS al hacer push de un tag `v*` o manualmente.

## Primer arranque (bootstrap de modelos)

Al abrir AION por primera vez en una máquina nueva, el shell:
1. Lanza el Ollama embebido en `127.0.0.1:11919`.
2. Ejecuta `aion-core models-ensure` en segundo plano (multiplataforma, sin bash):
   - Si faltan, descarga `nomic-embed-text` y crea `gemma4-reason` desde
     `bootstrap/Modelfile.aion` (descarga el GGUF abliterated Q6_K de Hugging Face).
   - Avisa al usuario por notificación con sonido (macOS).
   - Es **idempotente**: si ya existen, no hace nada.

## Firma y notarización (pendiente — requiere cuenta de desarrollador)

- **macOS:** para distribuir sin el aviso de Gatekeeper hay que firmar con un
  Apple Developer ID y notarizar (cada dylib del runtime + la app). Sin firmar, el
  usuario debe abrir con clic derecho → Abrir la primera vez. *Bloqueado: requiere
  cuenta Apple Developer ($99/año).*
- **Windows:** firma con certificado Authenticode para evitar SmartScreen.
  *Bloqueado: requiere certificado.*

Las rutas de datos son multiplataforma: macOS `~/Library/Application Support/AION`,
Windows `%APPDATA%\AION`, Linux `~/.local/share/AION`.
