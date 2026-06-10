#!/bin/sh
# Activa los git hooks versionados de AION (formateo automático pre-commit).
# Ejecútalo una vez tras clonar: sh scripts/setup-hooks.sh
git config core.hooksPath .githooks
echo "✅ hooks activados (.githooks). El pre-commit formatea Rust automáticamente."
