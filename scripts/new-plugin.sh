#!/usr/bin/env bash
# Crea un plugin nuevo a partir de templates/plugin.
#
#   scripts/new-plugin.sh <id> [nombre] [carpeta-destino]
#
# <id>: ^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$ (docs/plugins.md). Destino por defecto: ./plugins/<id>
set -euo pipefail

usage() {
  echo "uso: $0 <id> [nombre] [carpeta-destino]" >&2
  exit 2
}

[[ $# -ge 1 && $# -le 3 ]] || usage

id="$1"
if [[ ! "$id" =~ ^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$ ]]; then
  echo "error: id inválido '$id' (usa minúsculas, números y guiones; 3-64 caracteres)" >&2
  exit 1
fi

default_name="$(printf '%s' "$id" | tr '-' ' ' | awk '{ for (i = 1; i <= NF; i++) $i = toupper(substr($i, 1, 1)) substr($i, 2); print }')"
name="${2:-$default_name}"
if [[ -z "$name" || ${#name} -gt 80 ]]; then
  echo "error: el nombre debe tener entre 1 y 80 caracteres" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template="$repo_root/templates/plugin"
target="${3:-./plugins/$id}"

if [[ ! -d "$template" ]]; then
  echo "error: no se encontró la plantilla en $template" >&2
  exit 1
fi
if [[ -e "$target" ]]; then
  echo "error: '$target' ya existe" >&2
  exit 1
fi

mkdir -p "$(dirname "$target")"
cp -R "$template" "$target"

# Reemplaza los marcadores en todos los archivos de texto de la copia, escapando el nombre
# según el tipo de archivo (JSON/JS: comillas y barras; HTML: entidades; Markdown: tal cual).
escape_sed() { printf '%s' "$1" | sed -e 's/[\/&|]/\\&/g'; }
name_json="$(printf '%s' "$name" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
name_html="$(printf '%s' "$name" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g')"
id_escaped="$(escape_sed "$id")"
while IFS= read -r -d '' file; do
  case "$file" in
    *.json | *.js) value="$name_json" ;;
    *.html) value="$name_html" ;;
    *) value="$name" ;;
  esac
  value_escaped="$(escape_sed "$value")"
  sed -i.bak -e "s|__PLUGIN_ID__|$id_escaped|g" -e "s|__PLUGIN_NAME__|$value_escaped|g" "$file"
  rm -f "$file.bak"
done < <(find "$target" -type f \( -name '*.json' -o -name '*.html' -o -name '*.js' -o -name '*.css' -o -name '*.md' \) -print0)

target_abs="$(cd "$target" && pwd)"
cat <<MSG
Plugin '$name' creado en $target_abs

Para cargarlo en Odoo Backup Desktop:
  1. Abre Plugins y activa «Modo desarrollador».
  2. Pulsa «Cargar desde carpeta…» y elige $target_abs
  3. Edita ui/index.html o ui/index.js: la app recarga el plugin al guardar.
MSG
