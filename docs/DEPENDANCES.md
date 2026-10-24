# Dépendances et licences

Usage strictement personnel : aucune distribution. Les licences sont consignées
pour mémoire, et les dépendances permissives (MIT, Apache-2.0, BSD, OFL) sont
préférées à qualité équivalente.

## Intégrées à l'application

| Dépendance | Rôle | Licence |
|---|---|---|
| MuPDF (crate `mupdf` / `mupdf-sys` 0.8) | analyse, rendu, extraction PDF | AGPL-3.0 |
| FreeType (via MuPDF) | rastérisation des polices | FTL (type BSD) ou GPL-2.0 |
| HarfBuzz (via MuPDF) | mise en forme du texte | MIT |
| jbig2dec (via MuPDF) | images JBIG2 | AGPL-3.0 |
| lcms2 (via MuPDF) | gestion des couleurs | MIT |
| libjpeg (via MuPDF) | images JPEG | IJG |
| OpenJPEG (via MuPDF) | images JPEG 2000 | BSD-2-Clause |
| zlib (via MuPDF) | compression | zlib |
| `font-kit` | polices installées sur le Mac (par MuPDF, et depuis le jalon 4 pour intégrer une police au texte modifié) | MIT ou Apache-2.0 |
| `uniffi` | pont Swift ↔ Rust | MPL-2.0 |
| `thiserror` | erreurs Rust | MIT ou Apache-2.0 |
| `unicode-normalization` | repli des accents pour la recherche, comparaison des textes reconnus | MIT ou Apache-2.0 |
| Vision (framework de macOS) | reconnaissance du texte des scans (jalon 5) | fourni avec macOS |
| `rustybuzz` 0.20 | mise en forme du texte réécrit dans un scan (jalon 7) | MIT |
| `ttf-parser` 0.25 (via `rustybuzz`) | lecture des polices du catalogue (jalon 7) | MIT ou Apache-2.0 |
| `ab_glyph_rasterizer` 0.1 | rastérisation des glyphes à la résolution du scan (jalon 7) | Apache-2.0 |
| `core-text` 20.1 (déjà utilisé par `font-kit`) | fichiers des polices installées sur le Mac, pour le catalogue de l'appariement (jalon 7) | MIT ou Apache-2.0 |
| `jpeg-encoder` 0.7 | encodage JPEG de l'image d'une page améliorée (mode Amélioration, jalon 2, D63) | (MIT ou Apache-2.0) et IJG |
| `serde`, `serde_json` (déjà présents par `uniffi`) | réglages du mode Amélioration en JSON (jalon 2) | MIT ou Apache-2.0 |

**AGPL-3.0 (MuPDF, jbig2dec) :** cette licence oblige à fournir le code source à
quiconque reçoit le logiciel ou l'utilise à travers un réseau. Pour un usage
personnel, sans distribution ni service en ligne, elle n'impose aucune obligation.
Une distribution future de PDF Studio exigerait soit de publier son code sous AGPL,
soit une licence commerciale d'Artifex, soit le passage à PDFium (BSD).

**Aucune bibliothèque réseau** (jalon 9, D55) : l'arbre des dépendances du
moteur intégré à l'application (`cargo tree -p pdfstudio_ffi`) ne contient ni
client HTTP, ni environnement asynchrone, ni bibliothèque TLS ; `tokio`, présent
dans `Cargo.lock`, n'y vient que des outils de développement. Le jalon 9
n'ajoute aucune dépendance. Le jalon 2 du mode Amélioration n'ajoute que
`jpeg-encoder` (Rust pur, sans dépendance, sans accès réseau).

## Outils de développement (non intégrés)

| Outil | Rôle | Licence |
|---|---|---|
| PDFium (binaire `bblanchon/pdfium-binaries`) | banc d'essai uniquement | BSD-3-Clause |
| `pdfium-render` | banc d'essai uniquement | MIT ou Apache-2.0 |
| `image` | bancs d'essai (export PNG, scans simulés) | MIT ou Apache-2.0 |
| Tesseract 5 (commande `tesseract`, Homebrew) | banc d'essai OCR uniquement | Apache-2.0 |
| Modèles Tesseract `fra`, `eng`, `osd` (dépôt `tesseract-ocr/tessdata`) | banc d'essai OCR uniquement | Apache-2.0 |
| `ocrs` 0.13, `rten` 0.26 | banc d'essai OCR uniquement | MIT ou Apache-2.0 |
| Modèles d'`ocrs` (`text-detection.rten`, `text-recognition.rten`) | banc d'essai OCR uniquement | non vérifiée (téléchargés à la volée, jamais versionnés) |
| `serde_json` | banc d'essai OCR (réponse du programme Vision) ; tests du moteur et banc d'essai `enhance_bench` (vérité terrain du jeu du mode Amélioration) | MIT ou Apache-2.0 |
| XcodeGen | génération du projet Xcode | MIT |
| SwiftLint | analyse du code Swift | MIT |
| reportlab, pikepdf, Pillow, fontTools | génération du jeu de test (le jeu du mode Amélioration n'utilise que pikepdf et Pillow) | BSD, MPL-2.0, HPND, MIT |
| resvg-py (resvg) et Pillow | images de l'icône depuis son SVG (`make icon`, jalon 8) | MIT (resvg : MIT ou Apache-2.0), HPND |
| SF Symbols (fournis avec macOS) | icônes de l'interface | licence d'Apple, usage dans les applications pour ses plateformes |

## Polices embarquées

| Police | Rôle | Licence |
|---|---|---|
| Carlito (régulier, gras, italique, gras italique) | candidate de l'appariement des scans, équivalent métrique de Calibri (jalon 7) | SIL OFL 1.1 |
| Caladea (régulier, gras, italique, gras italique) | candidate de l'appariement des scans, équivalent métrique de Cambria (jalon 7) | SIL OFL 1.1 |

Elles sont fournies dans `app/Resources/Fonts`, avec le texte de leur licence
(`OFL.txt`). La couche de texte invisible des pages reconnues (jalon 5)
utilise une police jamais peinte, « GlyphLessFont », construite par le code du
moteur (`pdf_core/src/textlayer.rs`) : elle ne reprend aucune police existante.
Le jeu de test OCR intègre DejaVu Sans (licence Bitstream Vera, libre) dans
`tests/fixtures/ocr/petits_caracteres.pdf` ; celui de la retouche des scans
(`tests/fixtures/scan_retouche.pdf`) est dessiné avec Carlito et Caladea.
