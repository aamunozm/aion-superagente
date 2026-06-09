# Vendoriza el runtime Ollama de WINDOWS dentro del bundle de AION.
# Descarga el zip oficial de Ollama para Windows (x64) y extrae ollama.exe + su
# runner + DLLs a src-tauri/ollama-runtime/, igual que el script de macOS.
#
# Se ejecuta en el runner Windows de CI (o local con PowerShell). El usuario final
# NO instala nada: el runtime viaja dentro de la app.
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $PSScriptRoot          # apps/desktop
$dest = Join-Path $here "src-tauri\ollama-runtime"
$version = if ($env:OLLAMA_VERSION) { $env:OLLAMA_VERSION } else { "latest" }

# URL del zip oficial (x64). 'latest' resuelve a la última release.
if ($version -eq "latest") {
  $url = "https://github.com/ollama/ollama/releases/latest/download/ollama-windows-amd64.zip"
} else {
  $url = "https://github.com/ollama/ollama/releases/download/$version/ollama-windows-amd64.zip"
}

Write-Host "Descargando runtime Ollama Windows: $url"
$tmp = Join-Path $env:TEMP "ollama-windows-amd64.zip"
# Descarga con curl.exe (nativo en Windows 10+), MUCHO más rápido que
# Invoke-WebRequest para archivos grandes (el zip lleva libs GPU pesadas).
Write-Host "Descargando con curl…"
& curl.exe -L --fail --retry 3 -o $tmp $url
if ($LASTEXITCODE -ne 0) { throw "curl falló al descargar el runtime ($LASTEXITCODE)" }

if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
New-Item -ItemType Directory -Force -Path $dest | Out-Null

Write-Host "Extrayendo con tar (bsdtar nativo, mucho más rápido que Expand-Archive)…"
& tar.exe -xf $tmp -C $dest
if ($LASTEXITCODE -ne 0) {
  Write-Host "tar falló; recurriendo a Expand-Archive…"
  Expand-Archive -Path $tmp -DestinationPath $dest -Force
}
Remove-Item $tmp -Force

# Verificación mínima.
$exe = Join-Path $dest "ollama.exe"
if (-not (Test-Path $exe)) {
  # Algunos zips anidan en una subcarpeta: aplanar.
  $found = Get-ChildItem -Recurse -Filter "ollama.exe" $dest | Select-Object -First 1
  if ($found) {
    Copy-Item -Recurse -Force (Join-Path $found.Directory.FullName "*") $dest
  }
}
if (-not (Test-Path $exe)) { throw "No se encontró ollama.exe tras extraer." }

# RECORTE de aceleradores GPU (CUDA/ROCm): pesan cientos de MB y solo sirven en PCs
# con GPU NVIDIA/AMD. Para un instalador ligero y universal, AION corre en CPU. Si el
# usuario tiene GPU, puede instalar Ollama aparte. (Reduce el .exe a la mitad o más.)
Get-ChildItem -Recurse -Directory $dest |
  Where-Object { $_.Name -match 'cuda|rocm|cu1[12]|hip' } |
  ForEach-Object { Write-Host "  recortando GPU: $($_.FullName)"; Remove-Item -Recurse -Force $_.FullName }
Get-ChildItem -Recurse -File $dest |
  Where-Object { $_.Name -match 'cuda|cublas|rocm|hipblas|ggml-cuda|ggml-hip' } |
  ForEach-Object { Remove-Item -Force $_.FullName -ErrorAction SilentlyContinue }

$count = (Get-ChildItem -Recurse $dest | Measure-Object).Count
$size = "{0:N0} MB" -f ((Get-ChildItem -Recurse $dest | Measure-Object Length -Sum).Sum / 1MB)
Write-Host "OK -> $dest ($count archivos, $size)"
