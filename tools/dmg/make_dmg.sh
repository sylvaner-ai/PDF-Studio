#!/bin/bash
# Image disque (.dmg) d'installation de PDF Studio.
#
#   tools/dmg/make_dmg.sh "build/PDF Studio.app" build/PDF-Studio-0.1.0-arm64.dmg
#
# L'image contient l'application, un raccourci vers /Applications (pour la
# glisser-déposer) et la notice du premier lancement. Une somme SHA-256 est
# écrite à côté, dans <image>.sha256. Aucune mise en page de la fenêtre n'est
# faite : elle demanderait un Finder piloté par script, absent d'une
# construction automatique.

set -euo pipefail

if [ "$#" -ne 2 ]; then
	echo "usage : $0 <PDF Studio.app> <image.dmg>" >&2
	exit 2
fi

app="$1"
dmg="$2"
here="$(cd "$(dirname "$0")" && pwd)"

[ -d "$app" ] || { echo "Application introuvable : $app" >&2; exit 1; }

# L'application doit porter une signature valide, même locale : sans elle,
# macOS refuse de la lancer sur un Mac à processeur Apple.
codesign --verify --deep --strict "$app"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

ditto "$app" "$stage/PDF Studio.app"
ln -s /Applications "$stage/Applications"
cp "$here/Premier lancement.txt" "$stage/Premier lancement.txt"

mkdir -p "$(dirname "$dmg")"
rm -f "$dmg"
hdiutil create -volname "PDF Studio" -srcfolder "$stage" -fs HFS+ \
	-format UDZO -imagekey zlib-level=9 -ov "$dmg" >/dev/null

(cd "$(dirname "$dmg")" && shasum -a 256 "$(basename "$dmg")" > "$(basename "$dmg").sha256")

echo "Image prête : $dmg"
