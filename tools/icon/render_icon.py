#!/usr/bin/env python3
"""Images de l'icône de PDF Studio, à partir de app/Resources/AppIcon.svg.

Produit les dix images du jeu d'icônes de macOS (16 à 512 points, en 1x et
2x) dans app/Resources/Assets.xcassets/AppIcon.appiconset, avec son
Contents.json. Le rendu SVG est fait par resvg (`pip install resvg-py`), qui
prend en charge les ombres portées ; la réduction aux petites tailles par
Pillow (filtre de Lanczos), plus net qu'un rendu SVG direct à 16 pixels.

    python3 tools/icon/render_icon.py      (ou : make icon)
"""

import io
import json
import pathlib

import resvg_py
from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parents[2]
SOURCE = ROOT / "app/Resources/AppIcon.svg"
OUTPUT = ROOT / "app/Resources/Assets.xcassets/AppIcon.appiconset"
POINTS = [16, 32, 128, 256, 512]


def main() -> None:
    master = Image.open(io.BytesIO(bytes(resvg_py.svg_to_bytes(
        svg_path=str(SOURCE), width=1024, height=1024
    )))).convert("RGBA")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    images = []
    for points in POINTS:
        for scale in (1, 2):
            pixels = points * scale
            name = f"icon_{points}x{points}{'@2x' if scale == 2 else ''}.png"
            image = master if pixels == 1024 else master.resize((pixels, pixels), Image.LANCZOS)
            image.save(OUTPUT / name, optimize=True)
            images.append({
                "filename": name,
                "idiom": "mac",
                "scale": f"{scale}x",
                "size": f"{points}x{points}",
            })
    contents = {"images": images, "info": {"author": "xcode", "version": 1}}
    (OUTPUT / "Contents.json").write_text(json.dumps(contents, indent=2) + "\n")
    catalog = OUTPUT.parent / "Contents.json"
    catalog.write_text(json.dumps({"info": {"author": "xcode", "version": 1}}, indent=2) + "\n")
    print(f"{len(images)} images écrites dans {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
