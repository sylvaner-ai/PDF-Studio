# Décisions techniques

Chaque note : option retenue, alternatives, raisons. Les décisions marquées
« provisoire » seront confirmées ou révisées par une mesure.

## D1 — Pont Swift ↔ Rust : UniFFI (jalon 1)

- **Retenu :** UniFFI 0.32, macros procédurales (`#[uniffi::export]`), liaisons
  générées depuis la bibliothèque compilée.
- **Alternative :** FFI C exposée à la main, en-tête produit par `cbindgen`.
- **Raisons :** UniFFI génère des types Swift idiomatiques (classes, structures,
  erreurs `throws`) et gère la mémoire de part et d'autre ; avec `cbindgen`, chaque
  chaîne, tableau et objet demanderait du code `unsafe` écrit et maintenu à la main,
  ce que le cahier des charges veut limiter. Le coût est une copie des pixels à la
  frontière (~18 Mo par page plein écran Retina, quelques millisecondes), acceptable.

## D2 — Bibliothèque PDF et rendu : MuPDF (jalon 1, confirmée par mesure)

- **Retenu :** MuPDF via le crate `mupdf` 0.8 (liaison sûre, MuPDF compilé et lié
  statiquement). Formats non PDF et JavaScript désactivés.
- **Alternatives :**
  - PDFium via `pdfium-render` : rendu de référence de Chrome, licence BSD, mais
    distribué en bibliothèque dynamique à embarquer et charger à l'exécution ;
  - `lopdf` : lecture et écriture bas niveau, sans rendu (sera utile pour l'écriture) ;
  - `hayro` (rendu pur Rust, MIT/Apache) : écarté, car il ne lit pas les PDF chiffrés
    (exigés au §4.1) et ses auteurs indiquent qu'aucune optimisation de performance
    n'a encore été faite.
- **Raisons :** MuPDF couvre dans une seule bibliothèque tout ce dont les jalons
  suivants ont besoin (extraction du texte avec positions et polices, accès aux
  objets, écriture, annotations, OCR Tesseract intégré) ; il se lie statiquement,
  sans bibliothèque à embarquer. Sa licence AGPL n'impose aucune obligation pour un
  usage strictement personnel, sans distribution ni service en réseau.
- **Mesure :** banc d'essai sur 12 fichiers publics et 10 documents personnels,
  cf. `RENDER_BENCHMARK.md`. Fidélité identique sur les polices intégrées ; PDFium
  rend plus vite les scans lourds (6 ms contre 14 ms en moyenne, très en deçà des
  besoins) et gère mieux les polices standard non intégrées ; MuPDF répare les
  fichiers endommagés, conserve le détail des scans et offre les fonctions des
  jalons suivants. MuPDF est conservé ; ses deux faiblesses sont traitées au
  jalon 2.

## D3 — Un fil dédié par document (jalon 1)

- **Retenu :** le `Document` MuPDF vit sur un fil qui lui est propre ; les autres
  fils lui envoient des requêtes. Les listes d'affichage, partageables, sont
  rastérisées en parallèle.
- **Alternative :** un `Mutex` autour du document, ce qui exige de déclarer le type
  MuPDF `Send` avec du code `unsafe` hors de la couche FFI, interdit par le §6.
- **Raisons :** aucune ligne `unsafe` dans `pdf_core` et `render`, et la partie
  coûteuse du rendu reste parallèle.

## D4 — Projet Xcode généré par XcodeGen (jalon 1)

- **Retenu :** `app/project.yml`, transformé en `.xcodeproj` par XcodeGen.
- **Alternatives :** `.xcodeproj` versionné tel quel ; Tuist.
- **Raisons :** un fichier YAML lisible et révisable dans les pull requests, sans
  les conflits du format `pbxproj` ; XcodeGen est un outil simple, installé par
  Homebrew. Le projet généré reste un projet Xcode standard.

## D5 — Bac à sable macOS sans réseau (jalon 1)

- **Retenu :** App Sandbox activé, accès en lecture seule aux fichiers choisis par
  l'utilisateur, signets de sécurité, **aucune autorisation réseau**.
- **Raisons :** le système lui-même garantit qu'aucune connexion ne peut être émise
  (§6, confidentialité) et que l'application ne touche que les fichiers ouverts
  par l'utilisateur. L'accès en écriture sera ajouté au jalon 3 (enregistrement).

## D6 — Swift 5 avec vérification stricte de la concurrence (jalon 1)

- **Retenu :** mode de langage Swift 5, `SWIFT_STRICT_CONCURRENCY = complete`.
- **Alternative :** mode Swift 6 (erreurs de concurrence bloquantes).
- **Raisons :** mêmes vérifications, signalées en avertissements, ce qui évite d'être
  bloqué par le code généré par UniFFI. Passage au mode Swift 6 à envisager au
  jalon 9 (durcissement).

## D7 — Jeu de test généré (jalon 1)

- **Retenu :** `tests/fixtures/generate.py` (reportlab, pikepdf, Pillow) produit des
  PDF déterministes : texte et polices, 100 et 600 pages, signets, formats et
  rotations, vectoriel et transparence, tableau, scan simulé, chiffré, PDF 2.0,
  fichier tronqué, faux PDF. Les documents personnels restent dans
  `tests/fixtures/private/`, exclu du dépôt.
- **Raisons :** reproductible, sans contenu personnel dans un dépôt public.

## D8 — Polices standard non intégrées : clones URW de MuPDF (jalon 2)

- **Retenu :** les 14 polices standard du PDF (Helvetica, Times, Courier et leurs
  variantes, Symbol, ZapfDingbats), quand le fichier ne les contient pas, sont
  toujours dessinées avec les clones URW intégrés à MuPDF (Nimbus Sans, Nimbus
  Roman, Nimbus Mono, etc.). Les autres polices non intégrées restent cherchées
  parmi les polices de macOS.
- **Alternative :** les polices de macOS du même nom, comme Aperçu et PDFium.
- **Raisons :** la recherche par nom de MuPDF n'aboutissait sur macOS que pour
  certaines variantes : une même page mélangeait polices système et clones, et le
  tiret cadratin « — » était dessiné en demi-cadratin avec les polices système.
  Les clones URW ont les mêmes chasses que les originaux, donnent un rendu
  identique d'une machine et d'une version de macOS à l'autre, et peuvent être
  intégrés librement dans un PDF, ce dont l'édition de texte (jalon 4) aura besoin.
  Seul écart visible avec Aperçu : Nimbus Mono est un peu plus gras que le Courier
  de macOS. Un test vérifie que le tiret cadratin occupe toute sa chasse.

## D9 — Canevas des pages en AppKit (jalon 2)

- **Retenu :** une `NSScrollView` (pincement, inertie, grossissement natifs),
  intégrée à SwiftUI par `NSViewRepresentable` ; par page, une image entière à la
  résolution de l'écran (4 096 pixels au plus) et, au-delà, des tuiles de la seule
  zone visible.
- **Alternatives :** `ScrollView` SwiftUI (pincement à réimplémenter, sans ancrage
  sous le curseur ni comportement natif) ; `PDFView` de PDFKit (lié au moteur
  d'Apple, incompatible avec MuPDF) ; `CATiledLayer` pour toutes les pages.
- **Raisons :** comportement identique aux applications de macOS. Pour les scans,
  le rendu par tuiles décode l'image de la page tuile par tuile (mesuré par
  `render_bench --profile`) : une image unique tant qu'elle suffit évite ce surcoût
  dans les cas courants, et les tuiles ne servent qu'aux forts grossissements.

## D10 — Recherche repliée dans le moteur, page par page (jalon 2)

- **Retenu :** extraction du texte par MuPDF, repli Unicode (minuscules,
  décomposition NFD sans diacritiques, ligatures, signes typographiques) et
  recherche de sous-chaîne dans le moteur ; l'application parcourt les pages une à
  une.
- **Alternative :** la recherche intégrée de MuPDF (`fz_search_stext_page`),
  insensible à la casse mais pas aux accents.
- **Raisons :** « efficacite » doit trouver « efficacité » et « oeuvre » trouver
  « œuvre » ; la correspondance octet par octet avec les glyphes donne des
  rectangles exacts. La recherche page par page permet d'afficher les premiers
  résultats tout de suite et de l'interrompre à la frappe suivante.
