#!/usr/bin/env bash
# Cross-platform generation; needs Python 3 and Pillow (see assets/app-icon/README.md).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/packaging/icons.py"
