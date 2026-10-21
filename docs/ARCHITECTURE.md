# Architecture de PDF Studio

PDF Studio se compose d'un **moteur Rust**, qui détient le document et toute la
logique PDF, et d'une **application SwiftUI**, qui affiche et pilote le moteur par
des appels typés. Aucune logique PDF n'est écrite en Swift.

```
┌──────────────────────── Application SwiftUI (app/) ─────────────────────────┐
│ Views/        DocumentWindow, WelcomeView, DocumentView, PageScrollView      │
│ ViewModels/   DocumentViewModel (état d'une fenêtre), PageRenderer (cache)   │
│ App/          PDFStudioApp, DocumentOpener, DocumentReference (signets)      │
│ Bridge/       EngineBridge.swift + Generated/ (liaisons UniFFI générées)     │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │ UniFFI : PdfDocument, RenderedPage, EngineError
┌───────────────────────────────▼──────────── Moteur Rust (engine/) ───────────┐
│ ffi/        interface exposée à Swift (seule couche contenant du code unsafe, │
│             entièrement généré par les macros UniFFI)                         │
│ render/     rastérisation d'une page en image RGBA                            │
│ pdf_core/   ouverture, pages, métadonnées ; fil dédié par document            │
│ (MuPDF)     analyse, rendu et, aux jalons suivants, extraction et écriture    │
└──────────────────────────────────────────────────────────────────────────────┘
```

Les crates `ocr`, `layout`, `fonts` et `edit` prévus par le cahier des charges
seront créés aux jalons qui les utilisent (3 à 7).

## Modèle d'exécution

- **Un fil par document.** MuPDF interdit de manipuler un document depuis plusieurs
  fils. `pdf_core::DocumentHandle` lance donc un fil dédié qui possède le
  `Document` et exécute les requêtes une à une, transmises par canal.
- **Rendu parallèle.** Pour chaque page, ce fil construit une *liste d'affichage*
  MuPDF (`PageContent`, mise en cache pour 48 pages). Cette liste est `Send + Sync` :
  sa rastérisation (la partie coûteuse) s'exécute sur le fil de l'appelant, donc en
  parallèle pour plusieurs pages.
- **Côté Swift**, tout appel au moteur se fait hors du fil principal
  (`Task.detached`). `PageRenderer` limite à 3 les rendus simultanés, sert les
  demandes les plus récentes en premier et abandonne celles des pages sorties de
  l'écran. Les images rendues sont gardées dans un `NSCache` (512 Mio au plus).

## Flux d'ouverture d'un document

1. Le fichier arrive par le menu (⌘O), un glisser-déposer sur une fenêtre, le
   Finder ou l'icône du Dock ; `DocumentOpener` centralise ces quatre voies.
2. Une `DocumentReference` est créée : chemin + signet de sécurité en lecture seule
   (l'application est dans le bac à sable de macOS). Elle sert d'identité à la
   fenêtre : un même fichier n'ouvre jamais deux fenêtres, et les fenêtres sont
   restaurées au relancement.
3. Le moteur lit le fichier **une seule fois** en mémoire puis ne le touche plus :
   toute la suite travaille sur cette copie. Le fichier original ne peut donc pas
   être altéré par la lecture.
4. Les dimensions de toutes les pages sont mesurées à l'ouverture (4 ms pour
   600 pages), ce qui permet d'afficher immédiatement la mise en page complète ;
   les pages ne sont rendues qu'à leur apparition à l'écran.

## Pont Swift ↔ Rust : UniFFI

Retenu de préférence à une FFI C écrite via `cbindgen` (cf. `DECISIONS.md`, D1) :

- les types (structures, énumérations d'erreurs avec message, objets à compteur de
  références) sont générés des deux côtés, sans pointeur manipulé à la main ;
- les erreurs Rust deviennent des `Error` Swift, les objets Rust des classes Swift
  libérées automatiquement ;
- le code `unsafe` reste confiné aux macros UniFFI du crate `ffi`.

La génération se fait par `make bindings` : `uniffi-bindgen-swift` lit la
bibliothèque compilée et produit `app/Bridge/Generated/` (source Swift, en-tête C,
modulemap), non versionné.

## Build

`make build` enchaîne :

1. `make engine` : `cargo build --release --target aarch64-apple-darwin`
   → `libpdfstudio_ffi.a` (MuPDF compris, lié statiquement) ;
2. `make bindings` : liaisons Swift + copie du `.a` dans `app/Bridge/Generated/` ;
3. `make project` : `xcodegen generate` depuis `app/project.yml` (le `.xcodeproj`
   n'est pas versionné) ;
4. `xcodebuild` en Release, puis copie de `PDF Studio.app` dans `build/`.

Le projet Xcode relance les étapes 1 et 2 avant chaque compilation : un build
lancé depuis Xcode reste donc à jour.

## Tests

- `make test-engine` : tests Rust (`cargo test`) sur `tests/fixtures/`.
- `make test-app` : tests XCTest du pont et du rendu, hébergés dans l'application.
- `tests/fixtures/generate.py` régénère le jeu de PDF publics ;
  `tests/fixtures/private/` reçoit les documents personnels, jamais versionnés.
