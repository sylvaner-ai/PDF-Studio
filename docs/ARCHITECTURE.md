# Architecture de PDF Studio

PDF Studio se compose d'un **moteur Rust**, qui détient le document et toute la
logique PDF, et d'une **application SwiftUI**, qui affiche et pilote le moteur par
des appels typés. Aucune logique PDF n'est écrite en Swift.

```
┌──────────────────────── Application SwiftUI (app/) ──────────────────────────┐
│ Views/        DocumentWindow, DocumentView (lecteur), SidebarView,           │
│               DocumentWindowBridge (fermeture), ExportOptions, SearchField ; │
│               Canvas/ : PageCanvasView (AppKit), PageTileView ;              │
│               Grid/ : PageGrid, PageGridCoordinator, PageGridItem (AppKit)   │
│ ViewModels/   DocumentViewModel (+Saving, +Recovery, +Inserting,             │
│               +Exporting), DisplayPage, CanvasController, SearchModel,       │
│               OutlineTree, PageLayout, PageRenderer, FileSaver,              │
│               RecoveryStore, PageExporter                                    │
│ App/          PDFStudioApp, ViewerCommands et PageCommands (menus),          │
│               DocumentOpener, DocumentReference (signets de sécurité)        │
│ Bridge/       EngineBridge.swift + Generated/ (liaisons UniFFI générées)     │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │ UniFFI : PdfDocument, EditSession, PageItem,
                                │          RenderedPage, OutlineItem, SearchMatch,
                                │          PageRect, EngineError
┌───────────────────────────────▼──────────── Moteur Rust (engine/) ───────────┐
│ ffi/        interface pour Swift (seule couche contenant du code unsafe :    │
│             macros UniFFI et deux appels système de qualité de service)      │
│ edit/       liste des pages et historique d'annulation (patron Command)      │
│ render/     rastérisation RGBA : page entière ou zone, rotation d'affichage  │
│ pdf_core/   ouverture, pages, métadonnées, plan, texte et recherche ;        │
│             écriture d'un PDF réorganisé ; fil dédié par document ;          │
│             choix des polices standard                                       │
│ (MuPDF)     analyse, rendu, extraction du texte, écriture                    │
└──────────────────────────────────────────────────────────────────────────────┘
```

Les crates `ocr`, `layout` et `fonts` prévus par le cahier des charges seront
créés aux jalons qui les utilisent (4 à 7) ; `edit` l'a été au jalon 3.

## Modèle d'exécution

- **Un fil par document.** MuPDF interdit de manipuler un document depuis plusieurs
  fils. `pdf_core::DocumentHandle` lance donc un fil dédié qui possède le
  `Document` et exécute les requêtes une à une, transmises par canal. Pour chaque
  requête, ce fil adopte la qualité de service (QoS) du fil qui attend la
  réponse, ce qui évite les inversions de priorité signalées par macOS ; les
  appels système correspondants sont fournis par le crate `ffi`.
- **Rendu parallèle.** Pour chaque page, ce fil construit une *liste d'affichage*
  MuPDF (`PageContent`, mise en cache pour 48 pages). Cette liste est `Send + Sync` :
  sa rastérisation (la partie coûteuse) s'exécute sur le fil de l'appelant, donc en
  parallèle pour plusieurs pages.
- **Côté Swift**, tout appel au moteur se fait hors du fil principal
  (`Task.detached`). `PageRenderer` limite à 3 les rendus simultanés, sert les
  demandes les plus récentes en premier et abandonne celles des pages sorties de
  l'écran. Les images de pages entières sont gardées dans un `NSCache` (512 Mio au
  plus) ; les tuiles de détail appartiennent à la vue de leur page.

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

## Affichage des pages

- **Canevas AppKit.** `PageCanvasView` est une `NSScrollView` : défilement inertiel,
  pincement, barres de défilement et accessibilité sont ceux du système. Le
  grossissement vaut 1 en taille réelle (1 point PDF = 1 point à l'écran) ; la
  disposition (`PageLayout`) est calculée une fois en points, puis mise à l'échelle
  par la vue de défilement. Seules les pages proches de la zone visible ont une vue,
  recyclée au fil du défilement.
- **Deux niveaux de rendu par page.** Une image de la page entière, rendue à la
  résolution de l'écran et plafonnée à 4 096 pixels de côté, suffit jusqu'à environ
  240 % sur un écran Retina. Au-delà, des tuiles de 1 024 pixels de la seule zone
  visible (`render_region`) se posent par-dessus. Pendant un pincement, l'image
  existante est simplement agrandie ; le rendu net suit la fin du geste. Une image
  déjà en cache à une autre résolution sert de remplaçant immédiat.
- **Rotation d'affichage.** Elle est appliquée par le moteur au rendu (quart de tour
  dans la matrice de MuPDF) ; le fichier n'est pas modifié. Les rectangles de la
  recherche, exprimés sur la page droite, sont tournés côté Swift
  (`DisplayRotation`).

## Recherche et plan

- **Recherche.** Le moteur extrait le texte d'une page (avec la position de chaque
  glyphe) et le « replie » : minuscules, accents retirés, ligatures développées,
  guillemets, apostrophes et tirets unifiés, espaces fusionnées, mots coupés en fin
  de ligne recollés. La requête est repliée de la même façon. Chaque caractère
  replié garde la trace de son glyphe d'origine, d'où les rectangles à surligner,
  un par ligne. L'application interroge les pages une à une (`search_page`) :
  les résultats s'affichent au fil de l'eau et une nouvelle saisie interrompt la
  recherche en cours. Le texte de 256 pages reste en cache dans le moteur.
- **Plan.** Le plan MuPDF est aplati en parcours en profondeur (titre, niveau, page,
  position verticale), forme simple à faire traverser le pont ; l'application
  reconstruit l'arbre (`OutlineTree`).

## Organisation des pages

- **Liste des pages.** Une `EditSession` (crate `ffi`) enveloppe l'`Organizer` du
  crate `edit` : la liste des pages, chacune venue d'un document source (0 : le
  document ouvert ; 1, 2… : les documents insérés) ou vierge, avec ses quarts de
  tour. L'application en reçoit une copie (`PageItem`) après chaque opération et
  la convertit en `DisplayPage`, avec le format de chaque page.
- **Historique.** Chaque opération s'inscrit dans le gestionnaire d'annulation de
  la fenêtre, qui appelle `undo` et `redo` du moteur (décision D11).
- **Deux présentations.** Le canevas de lecture (⌘1) et la grille d'organisation
  (⌘2, `NSCollectionView`) partagent la sélection, par identifiant de page, et le
  cache des images, indexé par page source : déplacer une page ne demande aucun
  nouveau rendu.
- **Insertion.** Un document inséré est ouvert hors du fil principal et ajouté aux
  sources de la session. Les pages glissées depuis une autre fenêtre sont exportées
  par celle-ci en PDF, puis rouvertes en mémoire (`PdfDocument.fromData`).

## Enregistrement, récupération et exportations

- **Écriture.** `EditSession.export_pdf` produit les octets du nouveau PDF à partir
  des octets lus à l'ouverture (décision D12) ; `FileSaver` les écrit après
  confirmation et copie de sauvegarde, puis relit le résultat (D13).
- **Récupération.** `RecoveryStore` retient, à chaque modification non enregistrée,
  l'organisation des pages et des copies des documents dont elles viennent (D14).
  Les copies orphelines sont supprimées au lancement.
- **Exportations.** Extraction et scission passent par `export_pages` ; les images
  sont rendues par le moteur et encodées par ImageIO ; le texte vient de
  `plain_text`, page par page. Tout se fait hors du fil principal, pages figées.

## Pont Swift ↔ Rust : UniFFI

Retenu de préférence à une FFI C écrite via `cbindgen` (cf. `DECISIONS.md`, D1) :

- les types (structures, énumérations d'erreurs avec message, objets à compteur de
  références) sont générés des deux côtés, sans pointeur manipulé à la main ;
- les erreurs Rust deviennent des `Error` Swift, les objets Rust des classes Swift
  libérées automatiquement ;
- le code `unsafe` reste confiné au crate `ffi` : les macros UniFFI et le module
  `qos`, deux appels système documentés (`qos_class_self`,
  `pthread_set_qos_class_self_np`).

La génération se fait par `make bindings` : `uniffi-bindgen-swift` lit la
bibliothèque compilée et produit `app/Bridge/Generated/` (source Swift, en-tête C,
modulemap), non versionné.

## Build

Les produits intermédiaires (cible Cargo, DerivedData) sont placés dans
`~/Library/Caches/PDFStudio` (modifiable par `PDFSTUDIO_BUILD_ROOT`) : la
compilation de MuPDF échoue si son dossier de sortie contient un espace, et la
signature refuse les attributs étendus qu'iCloud Drive ajoute aux fichiers d'un
dossier synchronisé. Seule l'application finale est copiée dans `build/`.

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
- `make test-app` : tests XCTest du pont, du rendu, de la recherche, de la
  géométrie d'affichage, de l'organisation, de l'enregistrement, de la
  récupération et des exportations, hébergés dans l'application.
- `tests/fixtures/generate.py` régénère le jeu de PDF publics, dont
  `organisation.pdf` (signets, liens, champs, note, numérotation et métadonnées à
  préserver) et `signature_factice.pdf` (champ de signature) pour le jalon 3 ;
  `tests/fixtures/private/` reçoit les documents personnels, jamais versionnés.
