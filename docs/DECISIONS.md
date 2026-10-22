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

## D11 — Organisation des pages : une liste dans le moteur et un historique (jalon 3)

- **Retenu :** le crate `edit` tient la liste des pages : chacune vient d'une page
  d'un document source ou est vierge, avec un nombre de quarts de tour. Chaque
  opération (rotation, suppression, duplication, déplacement, insertion,
  restauration) remplace cette liste et s'inscrit dans un historique (patron
  Command : chaque commande retient la liste avant et après). Le gestionnaire
  d'annulation de la fenêtre (`NSUndoManager`) pilote cet historique, d'où les
  menus « Annuler la rotation », « Rétablir le déplacement »…
- **Révisions :** chaque étape de l'historique reçoit un numéro de révision qui
  n'est jamais réutilisé. L'enregistrement retient la révision de l'état écrit,
  lue en même temps que les pages exportées : une modification faite pendant
  l'écriture laisse donc le document « modifié ». Une opération sans effet (un
  déplacement à la même place) ne crée pas d'étape d'annulation.
- **Alternatives :** modifier le PDF dans MuPDF à chaque opération ; tenir
  l'historique côté Swift.
- **Raisons :** rien n'est écrit tant que l'utilisateur n'enregistre pas ;
  l'annulation est immédiate ; le cache des images, indexé par page source, sert
  encore après un déplacement ; la logique PDF reste dans le moteur.

## D12 — Écriture : réagencer le document ouvert (jalon 3)

- **Retenu :** l'enregistrement repart des octets lus à l'ouverture et garde le
  catalogue du document (métadonnées, formulaire, préférences d'affichage, plan).
  Toutes les pages sont retirées de l'arbre, puis les pages conservées y sont
  remises dans le nouvel ordre ; une page répétée est copiée en profondeur, les
  pages d'autres documents sont greffées avec une table de greffe par document
  source (une ressource commune n'est copiée qu'une fois), les pages vierges sont
  créées. Les signets, liens et champs qui visaient une page supprimée sont
  retirés, et toute autre référence à une page supprimée ou à ses annotations est
  effacée : leur contenu ne reste pas dans le fichier. La structure balisée
  (`/StructTreeRoot`), qui renvoie au contenu des pages, est alors retirée. Une
  simple rotation, sans changement d'ordre, ne touche que l'attribut `/Rotate`. Le
  PDF produit est relu par MuPDF avant d'être écrit.
- **Alternatives :** `pdf_rearrange_pages` de MuPDF, qui « recrée l'arbre des pages
  avec les pages listées, dans l'ordre » : il n'est pas exposé par le crate `mupdf`
  (il faudrait du code `unsafe` hors du crate `ffi`) et ne traite que les pages du
  document lui-même. Copier les pages dans un document neuf ferait perdre le
  catalogue.
- **Raisons :** une page conservée reste le même objet PDF, si bien que les liens et
  signets qui la visent fonctionnent encore après un déplacement.
- **Règle :** un objet direct ne doit jamais appartenir à deux conteneurs ; MuPDF
  renumérote alors deux fois ses références à l'enregistrement, et elles deviennent
  fausses ou nulles. Les attributs hérités de l'arbre des pages et les pages
  dupliquées sont donc copiés en profondeur.

## D13 — Enregistrer sans jamais écraser en silence (jalon 3)

- **Retenu :** ⌘S demande confirmation, en prévenant si une signature numérique
  sera invalidée. L'original est ensuite copié dans les sauvegardes
  (`Application Support/PDF Studio/Sauvegardes` du conteneur, 30 copies au plus),
  puis le fichier est réécrit en place. Le contenu écrit est relu, et l'original
  est restauré en cas d'échec. ⇧⌘S écrit une copie, qui devient le fichier de la
  fenêtre. Un fichier remplacé par une exportation est, lui aussi, d'abord
  sauvegardé, dans un dossier propre à cette exportation
  (`Sauvegardes/Exportations`, 10 exportations au plus) pour ne pas évincer les
  copies des documents. La confirmation signale aussi un fichier modifié par une
  autre application depuis son ouverture. Quitter pendant un enregistrement
  attend la fin de l'écriture.
- **Alternative :** l'écriture atomique, par un fichier temporaire échangé avec
  l'original.
- **Raisons :** la réécriture en place conserve l'emplacement du fichier, ses
  étiquettes et sa synchronisation. La copie préalable et la relecture couvrent le
  risque d'une écriture interrompue.

## D14 — Récupération : retenir l'organisation des pages, pas leur contenu (jalon 3)

- **Retenu :** à chaque modification non enregistrée, un fichier JSON de quelques
  kilo-octets décrit les pages. Il est rangé dans
  `Application Support/PDF Studio/Récupération`, avec le chemin du document et
  l'empreinte de son fichier (taille, date). Avant le premier enregistrement, qui
  remplace le fichier, la version d'ouverture est copiée à côté, de même que chaque
  document dont des pages sont insérées. À la réouverture du fichier, PDF Studio
  propose Restaurer ou Ignorer.
- **Alternative :** enregistrer un PDF complet à intervalles réguliers.
- **Raisons :** l'écriture est instantanée à chaque changement, et rien n'est perdu
  entre deux sauvegardes automatiques. Le mot de passe d'un document protégé n'est
  jamais écrit sur le disque.
- *Complétée au jalon 4 : les modifications du contenu sont retenues aussi (D22).*

## D15 — ⌘L et ⌘R font pivoter les pages, ⌥⌘L et ⌥⌘R l'affichage (jalon 3)

- **Retenu :** ⌘L et ⌘R font pivoter les pages sélectionnées, et la rotation est
  enregistrée dans le fichier. La rotation d'affichage du jalon 2 passe à ⌥⌘L et
  ⌥⌘R.
- **Raisons :** décision de Sylvain du 21 octobre 2026 (rotation par page,
  enregistrée, dès le jalon 3) ; ce sont aussi les raccourcis d'Aperçu.

## D16 — Grille d'organisation en AppKit (jalon 3)

- **Retenu :** une `NSCollectionView` avec une source de données différentielle,
  intégrée à SwiftUI par `NSViewRepresentable`.
- **Alternative :** `LazyVGrid` de SwiftUI, avec `draggable` et `dropDestination`.
- **Raisons :** la sélection multiple native (⇧-clic, ⌘-clic, rectangle, flèches),
  l'indicateur d'insertion entre deux vignettes, le glisser-déposer entre fenêtres
  et depuis le Finder, les déplacements animés et le recyclage des cases sont
  fournis par AppKit.

## D17 — Confirmation de fermeture par un délégué intercalé (jalon 3)

- **Retenu :** `WindowCloseGuard` se place devant le délégué que SwiftUI donne à la
  fenêtre et lui transmet tout ce qu'il ne traite pas. Il demande « Enregistrer… /
  Annuler / Ne pas enregistrer » avant de fermer un document modifié.
- **Raisons :** sous macOS 14, SwiftUI n'offre aucun moyen d'empêcher la fermeture
  d'une fenêtre.

## D18 — Exportations : des pages désignées par leur identifiant (jalon 3)

- **Retenu :** l'extraction, la scission et l'export en images désignent les pages
  par leur identifiant tant que le panneau d'enregistrement est ouvert, puis les
  figent à sa confirmation (`PageSnapshot` du moteur). Toutes les parties d'une
  scission viennent du même instantané. Les pages glissées vers une autre fenêtre
  et les insertions de documents sont repérées de la même façon : une insertion
  se fait après la page qui la précédait au moment de la demande, et les
  insertions se font une à une, dans l'ordre demandé.
- **Alternative :** des panneaux modaux, qui bloqueraient la fenêtre du document.
- **Raisons :** les panneaux de macOS ne bloquent pas la fenêtre ; une page
  supprimée ou déplacée entre-temps ne doit pas faire exporter une autre page. Une
  page extraite qui a disparu fait échouer l'extraction avec un message, plutôt que
  de produire un fichier faux ; une page sélectionnée qui a disparu est simplement
  laissée de côté par l'export en images et par la scission.

## D19 — Édition : réécriture ciblée du flux de contenu (jalon 4)

- **Retenu :** le moteur lit lui-même le flux de contenu de la page (analyseur et
  interpréteur en Rust sûr, `pdf_core::content`). Il en tire des objets : blocs de
  texte (un paragraphe forme un bloc), images, groupes de tracés qui se touchent,
  formulaires et dégradés, dans l'ordre d'empilement. Une modification (supprimé,
  déplacé de dx, dy, nouveau texte) est notée par objet ; à l'enregistrement comme
  pour l'aperçu, seules les opérations des objets modifiés sont réécrites, le
  reste du flux est recopié octet pour octet. Un glyphe effacé devient un décalage
  dans son tableau `TJ`, un texte déplacé ou remplacé est redessiné juste après son
  objet texte avec tout son état graphique, un autre objet est retiré ou entouré
  de `q cm … Q`.
- **Alternative :** reconstruire toute la page à partir de la liste d'affichage de
  MuPDF.
- **Raisons :** ce qui n'est pas touché reste identique (couleurs, détourages,
  calques, ordre d'empilement, texte balisé) ; le fichier grossit à peine ; le même
  code produit l'aperçu et le fichier, qui ne peuvent pas différer.

## D20 — Police d'un texte modifié : celle d'origine, sinon installée, sinon standard (jalon 4)

- **Retenu :** le nouveau texte garde la police du bloc si elle contient tous ses
  caractères : le moteur le vérifie par un rendu d'essai, chaque glyphe devant
  exister et avoir un contour (une police réduite aux caractères utilisés n'a pas
  les autres). Sinon, la même police installée sur le Mac est cherchée par son nom
  PostScript (`font-kit`) et intégrée au document si sa licence le permet
  (OS/2 `fsType`) et si elle pèse 16 Mo au plus. À défaut, la police standard la
  plus proche (Helvetica, Times, Courier, en gras ou italique) est utilisée, sans
  être intégrée. Le bandeau du mode Édition signale la substitution, le texte qui
  déborde du bloc et les caractères absents.
- **Alternative :** toujours intégrer une police libre embarquée.
- **Raisons :** le texte modifié ressemble le plus possible au reste ; une police
  dont la licence interdit l'intégration n'est jamais copiée dans le fichier.

## D21 — Un seul historique pour les pages et leur contenu (jalon 4)

- **Retenu :** l'état du crate `edit` réunit la liste des pages et les
  modifications du contenu, rangées par identifiant de page. ⌘Z annule
  indifféremment une rotation, une suppression de page ou un texte modifié, dans
  l'ordre où ils ont été faits. Une page dupliquée ou déplacée garde ses
  modifications.
- **Alternative :** un historique par page.
- **Raisons :** un seul ⌘Z, comme dans les autres applications du Mac ; une page
  supprimée puis rétablie retrouve son contenu modifié.

## D22 — Récupération : le contenu modifié aussi, avec une empreinte par objet (jalon 4)

- **Retenu :** le fichier de récupération (D14) retient aussi, pour chaque page,
  les modifications de son contenu, avec l'empreinte de l'objet visé (nature et
  emprise). À la restauration, une modification dont l'objet ne correspond plus est
  ignorée. Les enregistrements du jalon 3 restent lisibles.
- **Raisons :** une modification ne doit jamais s'appliquer à un autre objet que
  celui qui a été modifié.

## D23 — Mode Édition dans le canevas de lecture (jalon 4)

- **Retenu :** un troisième mode (⌘3) réutilise le canevas de lecture ; une vue
  AppKit posée sur chaque page dessine la sélection et reçoit la souris. Un clic
  sélectionne le plus petit objet sous le pointeur (un fond qui couvre presque
  toute la page n'est pas sélectionné par un clic), ⇧-clic ajoute ou retire, un
  rectangle tracé sur la page sélectionne plusieurs objets ; on déplace en
  glissant ou avec les flèches (1 pt, 10 pt avec ⇧), on supprime avec ⌫, on
  modifie un texte par double-clic ou Retour.
- **Alternative :** une vue d'édition séparée, page par page.
- **Raisons :** zoom, défilement et rendu par tuiles sont ceux de la lecture ;
  passer d'un mode à l'autre ne change pas la page lue.
