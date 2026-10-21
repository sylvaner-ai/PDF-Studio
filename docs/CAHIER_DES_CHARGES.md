# Cahier des charges — PDF Studio

## 1. Contexte et objectif

Développer **PDF Studio**, une application macOS native d'édition de fichiers PDF.

- **Usage : strictement personnel** (un seul utilisateur, aucune distribution, aucune monétisation). Pas de comptes, pas de télémétrie, pas de serveur : tout fonctionne en local et hors ligne.
- **Priorités, par ordre d'importance :**
  1. Fidélité visuelle : un texte modifié doit être très difficile à distinguer de l'original.
  2. Qualité de l'OCR.
  3. Fluidité et sobriété de l'interface.
  4. Fiabilité : aucun fichier original ne doit jamais être corrompu ou écrasé sans confirmation.

## 2. Contraintes techniques

| Élément | Exigence |
|---|---|
| Plateforme | macOS 14 (Sonoma) minimum, Apple Silicon en priorité (Intel non requis) |
| Interface | **SwiftUI** (AppKit autorisé ponctuellement via `NSViewRepresentable` pour les besoins de performance : canvas d'édition, rendu des pages) |
| Moteur | **Rust** (bibliothèque statique ou dynamique, `cargo workspace`) |
| Pont Swift ↔ Rust | **UniFFI** (de préférence) ou FFI C via `cbindgen`. Choix à justifier dans `docs/ARCHITECTURE.md` |
| Build | Projet Xcode + `cargo`, script de build unique (`make build`) produisant l'application `.app` |
| Tests | `cargo test` (moteur), XCTest (interface), jeu de PDF de référence dans `tests/fixtures/` |
| Concurrence | Toute opération lourde (OCR, rendu, export) hors du thread principal, avec barre de progression et annulation possible |

## 3. Architecture attendue

```
PDF-Studio/
├── app/                  # Projet Xcode (SwiftUI)
│   ├── Views/            # Vues SwiftUI
│   ├── ViewModels/       # État et logique de présentation
│   ├── Bridge/           # Liaisons générées vers le moteur Rust
│   └── Resources/
├── engine/               # Workspace Rust
│   ├── pdf_core/         # Parsing, modèle de document, écriture PDF
│   ├── render/           # Rendu des pages en bitmap
│   ├── ocr/              # Reconnaissance de texte
│   ├── layout/           # Détection de structure (blocs, lignes, tableaux, images)
│   ├── fonts/            # Identification, appariement et substitution de polices
│   ├── edit/             # Opérations d'édition + historique (undo/redo)
│   └── ffi/              # Interface exposée à Swift
├── tests/fixtures/       # PDF de test (natifs, scannés, mixtes, protégés)
├── docs/
└── Makefile
```

**Principe directeur :** le moteur Rust détient le modèle de document et toute la logique ; SwiftUI n'affiche et ne pilote que via des appels typés. Aucune logique PDF dans Swift.

**Modèle de document interne :** chaque page est décrite comme un ensemble d'**objets éditables** (blocs de texte, images, tableaux, tracés vectoriels), chacun avec position, dimensions, ordre d'empilement et propriétés. Toute modification est une commande réversible (patron *Command*) alimentant l'historique d'annulation.

## 4. Fonctionnalités

### 4.1 Lecture de fichiers PDF

- Ouverture par menu, glisser-déposer sur la fenêtre ou l'icône du Dock, et association de fichier `.pdf`.
- Rendu fidèle et rapide : pagination continue et page par page, zoom (pincement, raccourcis, ajustement à la largeur ou à la page), rotation d'affichage.
- Barre latérale de vignettes, plan du document (signets) s'il existe.
- Recherche plein texte dans le document (texte natif et texte issu de l'OCR).
- Prise en charge : PDF 1.x et 2.0, PDF chiffrés (demande de mot de passe), PDF volumineux (plus de 500 pages) avec chargement paresseux des pages.
- Ouverture en lecture seule du fichier source ; l'édition travaille sur une copie en mémoire.

### 4.2 OCR performant

- Lancement sur une page, une sélection de pages ou le document entier ; détection automatique des pages scannées (absence de texte natif).
- **Exigences de résultat :**
  - Français et anglais en priorité, autres langues extensibles.
  - Gestion des accents, ligatures, chiffres, ponctuation, tableaux et mises en page multi-colonnes.
  - Redressement automatique (*deskew*) et correction d'orientation des pages.
  - Prétraitement d'image : binarisation adaptative, suppression du bruit, amélioration du contraste.
  - Conservation des coordonnées précises de chaque mot et de chaque ligne.
- **Choix du moteur OCR (à trancher par l'agent, avec benchmark documenté dans `docs/OCR_BENCHMARK.md`) :**
  - Option A : framework **Vision** d'Apple (`VNRecognizeTextRequest`, niveau « accurate »), appelé depuis Swift. Très performant sur Mac, mais rompt le principe « tout dans Rust ».
  - Option B : **Tesseract** (via `leptess` ou `tesseract-rs`) avec modèles `fra` et `eng`.
  - Option C : moteur Rust basé sur ONNX (par exemple `ocrs`, ou PaddleOCR via `ort`).
  - L'agent doit **mesurer** précision et vitesse sur au moins 10 documents de test (scans propres, scans dégradés, tableaux, factures) et retenir la meilleure option. Une architecture à moteurs interchangeables (trait Rust `OcrEngine`) est exigée.
- Résultat : couche de texte invisible superposée à l'image (PDF « cherchable »), sans altérer l'aspect visuel, et éditable via les outils du §4.3.
- Indicateur de confiance par mot ; les mots à faible confiance sont signalés visuellement dans l'éditeur.

### 4.3 Modification du texte avec fidélité typographique maximale

C'est la fonctionnalité centrale et la plus difficile. L'agent doit la traiter avec le plus grand soin.

**a) PDF natifs (texte vectoriel)**
- Extraire pour chaque bloc : police, corps, graisse, style, interligne, crénage, couleur, alignement.
- Si la police est **intégrée en sous-ensemble** (cas courant) et que les nouveaux caractères saisis n'y figurent pas : retrouver la police complète installée sur le Mac si elle existe ; sinon utiliser la police substitut la plus proche (cf. ci-dessous) et appliquer la substitution à tout le bloc concerné pour garder une cohérence visuelle.
- Reflux du texte dans le bloc : respect de la largeur, de l'alignement et de l'interligne d'origine ; avertissement visuel en cas de débordement.

**b) Documents scannés (texte sous forme d'image)**
Chaîne de traitement attendue pour modifier un mot ou une ligne :
1. **Segmentation** : isoler la zone de texte à modifier à partir de l'OCR.
2. **Estimation de la police** : analyser les glyphes de la zone (empattements ou bâton, graisse, italique, proportions, chasse) pour classer le style, puis **comparer à un catalogue de polices** (polices système macOS + polices libres embarquées, par exemple les familles libres sous licence OFL) en mesurant la similarité du rendu (comparaison de formes de glyphes rastérisés, distance structurelle). Retenir les 3 meilleurs candidats avec score ; le meilleur est appliqué par défaut, les autres sont proposés dans un sélecteur.
3. **Estimation des paramètres** : corps, interlettrage, couleur d'encre réelle (échantillonnée), angle de rotation éventuel.
4. **Effacement de l'ancien texte** par *inpainting* : reconstruction du fond (papier, texture, tampons, filigranes à proximité) pour que la zone effacée soit indiscernable.
5. **Rendu du nouveau texte** dans la police retenue, avec des **effets de correspondance au scan** : même résolution, même flou, même grain/bruit, même niveau de contraste, même léger bavage d'encre, même rotation. L'objectif est que le texte modifié ait l'apparence d'avoir été scanné avec le reste.
6. **Intégration** : composition dans l'image de la page (ou dans une couche image remplaçant la zone), en conservant la résolution d'origine. Maintien de la couche de texte invisible mise à jour pour la recherche.

**Un mode de comparaison avant/après** (curseur ou bascule) doit permettre à l'utilisateur de contrôler la discrétion de la retouche, avec réglages manuels fins : police, corps, graisse, interlettrage, flou, bruit, couleur.

**Limites à documenter honnêtement** (`docs/LIMITES.md`) : l'identification de la police d'un scan est approximative, notamment pour une police rare ou une très faible résolution ; l'application doit indiquer le score de similarité et ne jamais prétendre à un résultat parfait.

### 4.4 Déplacer, redimensionner, supprimer les éléments

- Sélection d'objets (clic, rectangle de sélection, sélection multiple, Majuscule+clic).
- Types d'objets : blocs de texte, images, tableaux, tracés vectoriels, annotations.
- Opérations : déplacement (souris et flèches), redimensionnement avec poignées (proportions verrouillables), suppression, duplication, copier-coller (y compris entre pages et entre documents), ordre d'empilement, alignement et distribution, guides magnétiques.
- **Tableaux :** détection automatique (lignes, cellules, fusions) ; édition du contenu cellule par cellule ; déplacement ou suppression du tableau entier, d'une ligne ou d'une colonne ; en cas de suppression, le fond est reconstruit (cf. §4.3 b-4).
- **Sur page scannée :** les éléments sont d'abord détectés puis découpés dans l'image ; leur déplacement ou suppression reconstruit le fond avec la même technique d'*inpainting*.
- Historique d'annulation et de rétablissement illimité sur la durée de la session (⌘Z / ⇧⌘Z).

### 4.5 Organisation des pages

- Vue en grille de vignettes avec glisser-déposer pour réordonner.
- Supprimer, dupliquer, pivoter (90°/180°/270°), extraire des pages vers un nouveau fichier.
- Insérer des pages d'un autre PDF (glisser-déposer entre fenêtres), insérer une page vierge.
- Fusion de plusieurs PDF, scission d'un PDF en plusieurs.
- Sélection multiple et actions groupées, avec annulation possible.

### 4.6 Enregistrement et export

- Enregistrer (⌘S) et Enregistrer sous (⇧⌘S) : **le fichier original n'est jamais écrasé sans confirmation explicite** ; sauvegarde automatique de récupération en cas de plantage.
- Optimisation à l'enregistrement : sous-ensemble de polices, compression d'images au choix (sans perte par défaut).
- Export des pages en PNG ou JPEG, export du texte en `.txt`.
- Préservation, dans la mesure du possible, des métadonnées, signets, liens et champs de formulaire. Une signature numérique existante est invalidée par toute modification : l'application doit avertir l'utilisateur avant d'enregistrer.

## 5. Design et expérience utilisateur

**Intention :** une interface minimaliste, précise et belle, dans l'esprit de la conception d'Apple sous la direction de Jony Ive : retrait de tout ce qui n'est pas essentiel, matériaux et proportions soignés, détails exécutés avec une grande exigence. Les principes suivants traduisent cette intention en règles concrètes.

**Principes**
1. **Le document est le héros.** La page occupe l'essentiel de l'espace ; les contrôles s'effacent quand ils ne servent pas.
2. **Peu d'éléments, tous nécessaires.** Aucun bouton décoratif, aucune bordure inutile, aucun texte superflu.
3. **Révélation progressive.** Les options avancées n'apparaissent que lorsqu'un objet est sélectionné (panneau d'inspection contextuel).
4. **Cohérence native.** Composants, comportements et raccourcis fidèles aux standards macOS.

**Spécifications visuelles**
- Fenêtre avec barre de titre unifiée et barre d'outils épurée (6 outils maximum visibles : Sélection, Texte, Image, OCR, Pages, Enregistrer), icônes **SF Symbols** d'un seul poids.
- Matériaux translucides de macOS (`.regularMaterial` / `.ultraThinMaterial`) pour barres latérales et panneaux ; ombres douces et rayons de coin cohérents (arrondis continus).
- **Typographie :** police système SF Pro uniquement, 2 ou 3 tailles au maximum.
- **Palette :** neutre (gris très doux, blanc, noir), **une seule couleur d'accent** (celle du système, ou un bleu profond unique) réservée aux éléments actifs et sélectionnés.
- **Mode clair et mode sombre** parfaitement soignés ; fond de canevas distinct de la page.
- Grille d'espacement unique (multiples de 4 pt), marges généreuses, alignements rigoureux.
- **Animations :** courtes (150–250 ms), à courbes d'accélération naturelles (ressorts SwiftUI), sans effet gratuit ; respect de « Réduire les animations ».
- Icône d'application : sobre et reconnaissable, forme standard macOS, à concevoir avec soin (un fichier SVG source dans `app/Resources/`).
- Écrans vides : un unique message d'accueil discret invitant à déposer un PDF.
- Accessibilité : VoiceOver, navigation clavier complète, contrastes conformes, Dynamic Type.
- **Raccourcis clavier** : tous ceux d'usage macOS (⌘O, ⌘S, ⌘Z, ⌘F, ⌘+/⌘−, etc.), affichés dans les menus.

## 6. Exigences non fonctionnelles

- **Performance :** ouverture d'un PDF de 100 pages en moins de 2 s ; défilement à 60 images/s ; OCR d'une page A4 à 300 dpi en moins de 5 s sur Apple Silicon (à confirmer par benchmark).
- **Fiabilité :** aucune perte de données ; sauvegarde de récupération ; gestion d'erreurs explicite en français, sans jargon technique.
- **Confidentialité :** aucune connexion réseau ; l'application ne doit déclarer aucune entitlement réseau.
- **Langue de l'interface :** français (avec chaînes localisables via `Localizable.xcstrings`).
- **Qualité du code :** Rust sans `unsafe` hors couche FFI (et documenté), `clippy` et `rustfmt` sans avertissement, Swift conforme à SwiftLint ; documentation des modules publics.
- **Licences :** usage personnel, mais l'agent doit consigner dans `docs/DEPENDANCES.md` la licence de chaque dépendance (en particulier MuPDF, sous AGPL, et les polices embarquées) et privilégier des dépendances permissives (MIT/Apache-2.0/OFL) quand elles offrent une qualité équivalente.

## 7. Choix techniques à trancher et documenter

| Sujet | Pistes |
|---|---|
| Bibliothèque PDF (Rust) | `lopdf` (lecture/écriture bas niveau), `pdfium-render` (rendu via PDFium), `mupdf` (rendu et extraction très complets, licence AGPL) |
| Rendu des pages | PDFium ou MuPDF ; comparer qualité et vitesse |
| OCR | Cf. §4.2 (Vision, Tesseract, ONNX) |
| Détection de tableaux et de mise en page | Heuristiques géométriques (lignes, alignements) puis éventuel modèle ONNX léger |
| Appariement de polices | Descripteurs de forme des glyphes + classification ; base de polices libres embarquée |
| *Inpainting* | Algorithmes classiques (Telea, PatchMatch) en Rust, puis modèle ONNX léger si nécessaire |

Pour chaque sujet, l'agent rédige une courte note de décision (option retenue, alternatives, raisons) dans `docs/DECISIONS.md`.

## 8. Plan de réalisation par jalons

Chaque jalon doit être **fonctionnel, testé et commité** avant de passer au suivant.

1. **Socle :** workspace Rust, projet Xcode, pont UniFFI, application qui ouvre et affiche un PDF (§4.1 minimal).
2. **Lecture complète :** zoom, vignettes, recherche, signets, PDF chiffrés.
3. **Organisation des pages :** §4.5 et enregistrement sécurisé (§4.6).
4. **Édition de PDF natifs :** modèle d'objets, sélection, déplacement, suppression, undo/redo, modification de texte avec police intégrée ou substituée.
5. **OCR :** benchmark, intégration du moteur retenu, couche de texte cherchable.
6. **Détection de structure :** blocs, images, tableaux ; édition de tableaux.
7. **Édition de scans :** appariement de polices, *inpainting*, rendu avec correspondance au scan, mode comparaison.
8. **Design final :** finition visuelle, animations, mode sombre, icône, accessibilité.
9. **Durcissement :** performances, récupération après plantage, documentation, jeu de tests complet.

## 9. Critères d'acceptation

- [ ] L'application se compile avec une seule commande et se lance sur macOS 14+.
- [ ] Les 5 fonctionnalités du §4 sont opérationnelles sur le jeu de PDF de test.
- [ ] Sur 10 modifications de texte dans des documents natifs, la police et la mise en forme sont visuellement identiques à l'original à 100 % de zoom.
- [ ] Sur 10 modifications de texte dans des scans, un lecteur non averti ne distingue pas le texte modifié à 100 % de zoom dans au moins 7 cas (évaluation par l'utilisateur) ; les échecs sont documentés.
- [ ] Le taux de reconnaissance de l'OCR atteint au moins 98 % de caractères corrects sur des scans propres en français (mesuré sur le jeu de test).
- [ ] L'interface respecte les principes du §5 (revue visuelle par l'utilisateur).
- [ ] Aucun fichier original n'est modifié sans confirmation ; aucune connexion réseau n'est émise.
- [ ] Documentation à jour : `README.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`, `docs/LIMITES.md`, `docs/DEPENDANCES.md`.

## 10. Consignes de travail pour l'agent

- Travailler par petits commits cohérents sur le dépôt indiqué, avec messages clairs (en français ou en anglais, de façon constante).
- Avancer jalon par jalon ; à la fin de chacun, résumer ce qui fonctionne, ce qui ne fonctionne pas et les décisions prises.
- En cas d'ambiguïté bloquante, poser **une** question précise plutôt que d'improviser ; sinon, choisir l'option la plus simple et la documenter.
- **Ne jamais prétendre qu'une fonctionnalité marche sans l'avoir testée** sur les fichiers de `tests/fixtures/`. Signaler honnêtement les limites, notamment sur l'appariement de polices et l'*inpainting* des scans.
- Ne rien ajouter qui ne figure pas dans ce cahier des charges sans l'accord de l'utilisateur.
