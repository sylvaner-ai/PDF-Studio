# PDF Studio

Application macOS native d'édition de PDF, à usage strictement personnel :
locale, hors ligne, sans compte ni télémétrie.

- Interface : SwiftUI (macOS 14 minimum, Apple Silicon).
- Moteur : Rust (MuPDF), relié à Swift par UniFFI.
- Cahier des charges : [docs/CAHIER_DES_CHARGES.md](docs/CAHIER_DES_CHARGES.md).

## État d'avancement

| Jalon | Contenu | État |
|---|---|---|
| 1. Socle | moteur Rust, pont UniFFI, ouverture et affichage d'un PDF | terminé (à valider) |
| 2. Lecture complète | zoom, vignettes, recherche, signets, PDF chiffrés | à faire |
| 3. Organisation des pages | §4.5 et enregistrement sécurisé | à faire |
| 4 à 9 | édition, OCR, structure, scans, design, durcissement | à faire |

## Prérequis

- macOS 14 ou plus récent sur Apple Silicon, Xcode 16 ou plus récent ;
- Rust stable (`rustup`, cible `aarch64-apple-darwin`) ;
- XcodeGen et SwiftLint : `brew install xcodegen swiftlint`.

## Commandes

```sh
make build      # produit build/PDF Studio.app
make test       # tests du moteur (cargo) et de l'application (XCTest)
make lint       # clippy, rustfmt, SwiftLint
make bench      # banc d'essai MuPDF / PDFium (télécharge PDFium pour la mesure)
make fixtures   # régénère le jeu de PDF de test
```

Le premier `make build` compile MuPDF et prend plusieurs minutes ; les suivants
sont incrémentaux. Le projet Xcode (`app/PDFStudio.xcodeproj`) est généré : pour
travailler dans Xcode, lancer une fois `make project`, puis l'ouvrir.

## Documents de test personnels

Déposez vos propres PDF dans `tests/fixtures/private/` : ce dossier est exclu du
dépôt et ne quitte jamais votre Mac. `make bench` les mesure en anonymisant leurs
noms.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Décisions techniques](docs/DECISIONS.md)
- [Dépendances et licences](docs/DEPENDANCES.md)
- [Limites connues](docs/LIMITES.md)
- [Banc d'essai du rendu](docs/RENDER_BENCHMARK.md)
