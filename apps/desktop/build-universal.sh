#!/usr/bin/env bash
# Compila AION como .app UNIVERSAL (arm64 + x86_64) → cubre Mac Silicon y Mac Intel
# en un solo bundle. Crea sidecars universales con `lipo` y construye con el target
# universal-apple-darwin. El runtime Ollama ya es universal (vendor-ollama-runtime.sh).
set -euo pipefail
cd "$(dirname "$0")/../.."   # raíz del repo
source "$HOME/.cargo/env" 2>/dev/null || true
export PATH="$HOME/.cargo/bin:$PATH"

ARM=aarch64-apple-darwin
X86=x86_64-apple-darwin
UNI=universal-apple-darwin
DEST="apps/desktop/src-tauri/binaries"

echo "==> asegurando targets Rust"
rustup target add "$ARM" "$X86" >/dev/null 2>&1 || true

# Tauri (target universal) compila cada slice por separado y necesita el sidecar
# con el sufijo de CADA triple (NO un binario lipo'd). Colocamos ambos.
for bin in aion-core aion-control-plane; do
  pkg="$bin"
  echo "==> compilando $bin para $ARM y $X86"
  cargo build --release --target "$ARM" -p "$pkg" --bin "$bin"
  cargo build --release --target "$X86" -p "$pkg" --bin "$bin"
  cp "target/$ARM/release/$bin" "$DEST/$bin-$ARM"
  cp "target/$X86/release/$bin" "$DEST/$bin-$X86"
  # El bundle final universal también requiere el sidecar lipo'd (-universal).
  lipo -create "$DEST/$bin-$ARM" "$DEST/$bin-$X86" -output "$DEST/$bin-$UNI"
  echo "   $bin: $(lipo -archs "$DEST/$bin-$UNI")"
done

echo "==> verificando runtime Ollama vendorizado (debe existir y ser universal)"
[[ -x apps/desktop/src-tauri/ollama-runtime/ollama ]] || bash apps/desktop/scripts/vendor-ollama-runtime.sh

echo "==> construyendo .app UNIVERSAL"
cd apps/desktop
cargo tauri build --target "$UNI" --bundles app

# Firma con la identidad ESTABLE propia ("AION Local Signing") si está en el
# llavero: así el hash de firma no cambia entre builds y macOS CONSERVA los
# permisos (Grabación de pantalla / Accesibilidad) tras cada actualización.
# Si no existe (CI u otra máquina), cae a ad-hoc — sigue funcionando.
APP="target/$UNI/release/bundle/macos/AION.app"
if security find-identity -p codesigning 2>/dev/null | grep -q "AION Local Signing"; then
  echo "==> firmando con identidad estable 'AION Local Signing' (permisos persistentes)"
  codesign --force --deep --sign "AION Local Signing" "$APP"
else
  echo "==> identidad estable no encontrada → firma ad-hoc (los permisos se reconceden por versión)"
  codesign --force --deep --sign - "$APP"
fi
codesign -dvv "$APP" 2>&1 | grep -E "Authority=|Signature=" | head -2 || true
echo "==> .app universal en $APP"
