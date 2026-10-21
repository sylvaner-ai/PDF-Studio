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
| `font-kit` | accès aux polices système de macOS | MIT ou Apache-2.0 |
| `uniffi` | pont Swift ↔ Rust | MPL-2.0 |
| `thiserror` | erreurs Rust | MIT ou Apache-2.0 |
| `unicode-normalization` | repli des accents pour la recherche | MIT ou Apache-2.0 |

**AGPL-3.0 (MuPDF, jbig2dec) :** cette licence oblige à fournir le code source à
quiconque reçoit le logiciel ou l'utilise à travers un réseau. Pour un usage
personnel, sans distribution ni service en ligne, elle n'impose aucune obligation.
Une distribution future de PDF Studio exigerait soit de publier son code sous AGPL,
soit une licence commerciale d'Artifex, soit le passage à PDFium (BSD).

## Outils de développement (non intégrés)

| Outil | Rôle | Licence |
|---|---|---|
| PDFium (binaire `bblanchon/pdfium-binaries`) | banc d'essai uniquement | BSD-3-Clause |
| `pdfium-render` | banc d'essai uniquement | MIT ou Apache-2.0 |
| `image` | banc d'essai (export PNG) | MIT ou Apache-2.0 |
| XcodeGen | génération du projet Xcode | MIT |
| SwiftLint | analyse du code Swift | MIT |
| reportlab, pikepdf, Pillow | génération du jeu de test | BSD, MPL-2.0, HPND |

## Polices embarquées

Aucune à ce jour. Les polices libres (OFL) prévues pour l'appariement de polices
des scans seront listées ici au jalon 7.
