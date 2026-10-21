# PDF Studio

Application macOS native d'édition de PDF, à usage strictement personnel :
locale, hors ligne, sans compte ni télémétrie.

- Interface : SwiftUI (macOS 14 minimum, Apple Silicon).
- Moteur : Rust (MuPDF), relié à Swift par UniFFI.
- Cahier des charges : [docs/CAHIER_DES_CHARGES.md](docs/CAHIER_DES_CHARGES.md).

## État d'avancement

| Jalon | Contenu | État |
|---|---|---|
| 1. Socle | moteur Rust, pont UniFFI, ouverture et affichage d'un PDF | terminé |
| 2. Lecture complète | zoom, vignettes, recherche, signets, PDF chiffrés | terminé |
| 3. Organisation des pages | §4.5 et enregistrement sécurisé | terminé (à valider) |
| 4 à 9 | édition, OCR, structure, scans, design, durcissement | à faire |

## Organiser les pages

- **Présentations :** lecture (⌘1) ou grille d'organisation (⌘2) ; ⌘+ et ⌘-
  changent la taille des vignettes de la grille.
- **Menu Pages**, sur les pages sélectionnées : faire pivoter (⌘L, ⌘R), dupliquer
  (⌘D), supprimer (⌘⌫), insérer une page vierge (⌥⌘N) ou les pages d'autres PDF
  (⌥⌘I), extraire les pages (⌥⌘E), scinder le document. Tout s'annule par ⌘Z.
- **Glisser-déposer :** les vignettes se déplacent dans la barre latérale ou la
  grille ; un PDF déposé depuis le Finder sur la grille y insère ses pages ; des
  pages glissées vers la grille d'une autre fenêtre y sont copiées.
- **Fusionner :** ouvrir le premier document, y insérer les autres, puis
  Enregistrer sous.
- **Enregistrer** (⌘S) remplace le fichier après confirmation, en gardant une copie
  de l'original ; **Enregistrer sous** (⇧⌘S) écrit une copie. Les modifications non
  enregistrées sont proposées à la prochaine ouverture du document.
- **Exporter**, menu Fichier : les pages en images (PNG ou JPEG) ou le texte en .txt.

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
