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

## D24 — Recherche dans un panneau ouvert par ⌘F (jalon 4)

- **Retenu :** la recherche n'a plus de champ dans la barre d'outils. ⌘F (ou
  Édition > Rechercher…) ouvre un petit panneau en haut à droite des pages, sous
  la barre d'outils : champ, compteur, occurrence précédente et suivante. Échap ou
  le bouton de fermeture le referment et effacent le surlignage. Elle fonctionne
  en lecture comme en édition.
- **Raisons :** demande de Sylvain du 22 octobre 2026 ; la barre d'outils reste
  épurée, comme le demande le cahier des charges.


## D25 — Blocs de texte homogènes : même style, lignes alignées (jalon 4)

- **Retenu :** un bloc ne réunit que des lignes de même style dominant (même
  police, même corps, même couleur), alignées sur la ligne précédente (même bord
  gauche, même bord droit ou même centre, les espaces ne comptant pas) et
  espacées d'un interligne régulier ; une ligne isolée accepte aussi un alinéa ou
  un retrait de liste. Parmi les blocs possibles, la ligne va à celui dont elle
  est le mieux alignée. Une ligne de texte est coupée en deux quand le style
  change après un écart plus large qu'un cadratin (une étiquette suivie de sa
  valeur) ; un mot en gras ou en couleur au fil d'une phrase reste dans son
  paragraphe. Une ligne faite seulement d'espaces ne forme pas de bloc. Un bloc
  dont moins de la moitié des lignes sont pleines, ou plus étroit que douze
  cadratins, est une liste : chaque ligne garde son retour à la ligne et, à la
  modification, sa place d'origine au lieu d'être remise en page.
- **Alternative :** le chevauchement horizontal des lignes, retenu jusque-là ;
  ou couper à chaque changement de style, ce qui morcellerait les paragraphes
  qui contiennent un mot en gras.
- **Raisons :** demande de Sylvain du 22 octobre 2026 : des blocs homogènes en
  police, couleur et taille. Dans un encadré d'étiquettes et de valeurs
  (en-tête de facture), les étiquettes forment un bloc et les valeurs un autre ;
  modifier une valeur ne bouleverse plus la mise en page. Une colonne de tableau
  n'est plus prise pour un paragraphe.

## D26 — Moteur OCR de l'application : Vision, derrière un trait (jalon 5)

- **Retenu :** le framework Vision de macOS (`VNRecognizeTextRequest`, niveau
  « précis », correction linguistique, français puis anglais). Le moteur Rust
  ne connaît que le trait `ocr::OcrEngine` ; l'application lui fournit Vision par
  le trait UniFFI `TextRecognizer`, implémenté en Swift et appelé depuis Rust.
  Le code qui interroge Vision (`VisionTextReader.swift`) est le même dans
  l'application et dans le banc d'essai.
- **Alternatives :**
  - Tesseract 5 (Apache-2.0), que MuPDF sait aussi piloter : très bon sur les
    scans propres, mais il faudrait compiler et embarquer la bibliothèque C++ et
    ses modèles (une quinzaine de Mo par langue), et il lit mal les tableaux
    (cf. `OCR_BENCHMARK.md`) ;
  - `ocrs` (Rust pur, MIT/Apache) : rapide et sans dépendance système, mais son
    alphabet n'a pas d'accents (le français plafonne vers 90 % de caractères
    corrects) et il ne donne aucune confiance par mot.
- **Raisons :** Vision est fourni par macOS (aucune dépendance, aucun modèle à
  embarquer, aucune connexion réseau), accéléré par le Neural Engine des puces
  Apple, et lit le français et l'anglais. Le trait permet de changer de moteur
  sans toucher au reste. **Confirmé par les mesures sur le Mac** (22 octobre
  2026, `OCR_BENCHMARK.md`) : 98,6 % à 99,7 % de caractères corrects sur les
  scans propres en français (documents publics) et 0,4 s par page.

## D27 — Préparation de l'image avant la lecture (jalon 5, provisoire)

- **Retenu, par défaut :** égalisation du fond et étirement du contraste,
  redressement de l'inclinaison, correction de l'orientation ; ni filtre médian
  ni binarisation, disponibles comme réglages (`RecognitionSettings`).
  - **Fond :** pour chaque carré d'un quart de pouce, le niveau que dépassent
    ses 10 % de pixels les plus clairs (et non le plus clair : le bruit le
    fausserait), au moins la médiane des carrés voisins (un carré couvert
    d'encre), interpolé ; chaque pixel est divisé par ce fond.
  - **Inclinaison :** angle (de −10° à 10°) qui rend le plus net le profil de
    projection des pixels d'encre ; corrigée au-delà de 0,1°.
  - **Orientation :** une page semble couchée quand le profil des colonnes varie
    plus que celui des lignes, en ne comptant que les taches de la taille d'un
    caractère ou d'un mot (ni filets de tableau, ni bords sombres, ni images) ;
    elle est alors lue dans les deux sens et le moteur tranche. Une lecture
    douteuse (moins de trois mots, ou confiance moyenne inférieure à 0,6) est
    reprise à l'envers, puis dans les deux autres sens ; la meilleure (somme des
    caractères pondérés par leur confiance) est retenue.
- **Alternative :** binariser systématiquement, comme le recommande Tesseract.
- **Raisons :** sur le banc, la binarisation fait perdre des caractères à
  Tesseract et à `ocrs` sur les scans à 150 ppp et n'apporte rien sur les scans
  propres ; les moteurs neuronaux lisent mieux les niveaux de gris. Le premier
  passage du banc a montré que les bords sombres d'un scan et un fond mal
  égalisé faisaient croire à une page couchée : d'où le filtre des taches et la
  confirmation par le moteur. Réglages par défaut à confirmer par les mesures de
  Vision.

## D28 — Couche de texte invisible : police Type0 jamais peinte (jalon 5)

- **Retenu :** à l'enregistrement, les mots reconnus sont ajoutés au flux de
  contenu de la page, après le contenu d'origine (entouré de `q … Q`), dans un
  bloc marqué `/PDFStudioOCR BMC … EMC`. Chaque mot est écrit en rendu invisible
  (`3 Tr`) dans une police Type0 (Identity-H) construite par le code : un
  TrueType minimal dont tous les caractères renvoient à un même glyphe de
  largeur fixe (un rectangle de la hauteur du cadratin, jamais peint), avec
  une table ToUnicode identité. Comme chez Tesseract, chaque ligne reçoit une seule matrice (position et angle de la ligne), une taille et
  une ligne de pied communes (médianes de ses mots) ; les mots suivants sont
  placés par des déplacements `Td` le long de la ligne. `Tz` étire chaque mot
  exactement sur sa largeur ; une espace étirée le relie au mot suivant, pour
  que la copie et la recherche voient des espaces. Une première version plaçait
  chaque mot par sa propre matrice et à sa propre hauteur : Aperçu ne regroupait
  pas les mots en lignes et sélectionnait le texte en colonnes. Le défaut
  tenait surtout au glyphe, d'abord vide : PDFKit mesure les caractères par
  leur dessin, et donnait à chacun une hauteur nulle (mots non regroupés en
  lignes, sélection impossible).
- **Alternatives :** une police standard (Helvetica) en rendu invisible :
  limitée à l'encodage WinAnsi, avec des chasses variables qui ne coïncident
  pas avec l'image ; ou ajouter la couche dans un objet de formulaire séparé.
- **Raisons :** tout caractère Unicode (accents, €, œ, ligatures) est encodé
  sans police à intégrer ; une largeur fixe rend l'étirement exact, donc la
  sélection et le surlignage tombent sur les mots de l'image ; le bloc marqué
  permet de retrouver la couche. Vérifié avec MuPDF et `pdftotext` (Poppler) :
  « L'œuvre coûte 12,50 € » est extrait tel quel, l'image est inchangée au
  pixel près.

## D29 — Reconnaissance : une étape d'annulation, pages figées (jalon 5)

- **Retenu :** les pages sont lues une à une, hors du fil principal, sans
  modifier le document ; le résultat de toutes les pages lues est appliqué à la
  fin en une seule étape d'annulation (« la reconnaissance du texte »). Pendant
  la lecture, les pages sont figées comme pendant une exportation. « Arrêter »
  laisse finir la page en cours et applique les pages déjà lues. Une page lue
  couchée ou à l'envers reçoit la rotation qui la remet à l'endroit (celle de
  l'organisation, enregistrée dans le fichier) ; une rotation choisie sur une
  page droite est gardée. Les mots qui recouvrent un texte déjà présent dans
  la page sont écartés, pour ne pas doubler un texte cherchable.
- **Alternative :** appliquer chaque page dès sa lecture (une étape
  d'annulation par page), en laissant les pages modifiables pendant la lecture.
- **Raisons :** une seule annulation défait toute la reconnaissance ; une page
  modifiée ou déplacée pendant sa lecture recevrait une couche périmée.

## D30 — Mots douteux : confiance inférieure à 0,5 (jalon 5, provisoire)

- **Retenu :** un mot est douteux si sa confiance est inférieure à 0,5 ; il est
  surligné en orange en mode Édition, et le bilan de la reconnaissance en donne
  le nombre. Vision ne donne qu'une confiance par ligne : celle d'un mot est la
  confiance de sa ligne, réduite de moitié au plus quand les autres lectures
  proposées par Vision (jusqu'à trois) ne contiennent pas ce mot.
- **Raisons :** sur le banc, le seuil de 0,5 signale peu de mots justes ; le
  seuil sera ajusté d'après la calibration de Vision (`OCR_BENCHMARK.md`).
  La confiance n'est pas enregistrée dans le PDF : après réouverture, les mots
  ne sont plus signalés (cf. `LIMITES.md`).

## D31 — Bibliothèque `layout` : géométrie pure de la structure (jalon 6)

- **Retenu :** la détection des tableaux et leur réorganisation vivent dans un
  crate `layout` sans dépendance, qui ne connaît que des segments, des
  rectangles et des boîtes de glyphes. `pdf_core` lui donne les sous-chemins
  rectilignes des tracés et les glyphes, puis rattache le résultat aux objets
  de la page.
- **Alternatives :** garder la détection dans `pdf_core::content` ; un modèle
  appris (ONNX) de détection de tableaux.
- **Raisons :** testable sans PDF ni MuPDF, et réutilisable pour les scans
  (jalon 7, où les filets viendront de l'image). Le cahier des charges prévoit
  des heuristiques géométriques d'abord ; elles suffisent aux tableaux tracés
  des documents natifs.

## D32 — Tableaux détectés : quadrillages et tableaux à filets (jalon 6)

- **Retenu :** les filets sont tirés des segments droits tracés, des bords des
  rectangles et des rectangles pleins fins (2,5 pt au plus) ; un fond plein
  plus épais compte par ses quatre bords. Les fonds de page sont ignorés.
  - **Quadrillage :** les cellules sont les rectangles fermés par quatre filets
    qui se croisent (tolérance de 2,5 pt, méthode de pdfplumber) ; les
    cellules qui partagent un coin forment un tableau ; une cellule qui couvre
    plusieurs lignes ou colonnes de la grille est une cellule fusionnée.
  - **Tableau à filets horizontaux** (sans filet vertical) : au moins trois
    filets semblables (recouvrement de 90 %) ; les colonnes sont déduites des
    fragments de texte du corps (écart de plus de 0,75 cadratin), l'en-tête
    étant souvent aligné autrement ; la limite de deux colonnes est placée dans
    l'espace qu'aucun fragment ne recouvre.
  - Une bande entre deux filets qui porte plusieurs lignes de texte réparties
    en cellules est découpée en lignes virtuelles. Un tableau doit avoir au
    moins deux lignes, deux colonnes et deux cellules avec du texte.
- **Écarté :** les tableaux sans aucun filet (alignements seuls), trop proches
  d'un texte en colonnes ou d'un encadré étiquettes / valeurs : leur texte
  reste en blocs homogènes (D25).
- **Raisons :** les filets sont un signal sûr, sans faux positif sur le jeu de
  test (texte, encadré, colonnes, organisation) ; un tableau mal détecté
  casserait les blocs de texte de la page.

## D33 — Blocs de texte découpés par cellule (jalon 6)

- **Retenu :** dans un tableau, une ligne de texte est coupée à la limite de
  deux cellules et un bloc ne réunit que des lignes d'une même cellule.
  L'alignement d'un bloc d'une ligne se déduit de ses marges dans la cellule ;
  un nouveau texte est remis en page dans la largeur de la cellule (moins sa
  marge), et le débordement est signalé quand il descend sous la cellule.
- **Raisons :** chaque cellule s'édite seule (cahier des charges, §4.4). Les
  blocs d'une page qui contient un tableau changent de découpage : une
  modification retenue avant le jalon 6 sur une telle page ne retrouve plus
  son objet et est ignorée à la récupération (empreinte, D22).

## D34 — Lignes et colonnes supprimées ou déplacées : le tableau se resserre (jalon 6)

- **Retenu :** une réorganisation est décrite par les lignes et les colonnes
  d'origine conservées, dans leur nouvel ordre. Chaque élément du tableau la
  suit : un bloc ou une image se déplace d'un bloc (ou disparaît avec sa
  ligne) ; les tracés sont réécrits sous-chemin par sous-chemin, un filet
  propre à une cellule suivant sa cellule, un filet qui traverse le tableau
  étant raccourci point par point. Les limites gardent leur rang ; quand une
  ligne disparaît, la limite qui reste est la plus marquée (celle qui borde le
  plus de cellules et porte un filet), si bien que le cadre, le filet sous
  l'en-tête et les bords d'un intitulé fusionné sont conservés. Le tableau se
  resserre ; rien ne bouge autour de lui. Un déplacement doit garder chaque
  cellule fusionnée d'un seul tenant (ses sous-colonnes peuvent échanger leur
  place).
- **Alternatives :** effacer la ligne en laissant un blanc dans le tableau ;
  remonter aussi le contenu situé sous le tableau.
- **Raisons :** le résultat est celui d'un traitement de texte, sans redessiner
  le tableau (les filets, épaisseurs, couleurs et fonds d'origine sont
  gardés) ; déplacer ce qui suit le tableau supposerait de connaître le flux de
  la page, que le PDF ne décrit pas.

## D35 — Objet Tableau, sélectionné d'un bloc (jalon 6)

- **Retenu :** chaque tableau devient un objet de la page, numéroté après les
  objets dessinés (les numéros des autres ne changent pas), avec ses blocs,
  filets et fonds pour membres. En mode Édition, un clic sur le tableau le
  sélectionne en entier et désigne sa cellule courante ; il se déplace et se
  supprime comme un objet ; un double-clic (ou Retour) ouvre la saisie de la
  cellule ; le menu Tableau et le menu contextuel suppriment ou déplacent la
  ligne ou la colonne de la cellule courante. Ses modifications (`TableEdit`)
  sont rangées sur l'objet Tableau, en termes de lignes et colonnes d'origine,
  et chacune s'annule par ⌘Z.
- **Raisons :** un tableau n'existe pas dans le flux de contenu ; le décrire
  par ses bandes d'origine rend chaque modification indépendante des autres,
  et l'aperçu comme l'enregistrement sont recalculés à partir du contenu
  d'origine (D19).

## D36 — Texte saisi dans une cellule vide : le style d'une cellule voisine (jalon 6)

- **Retenu :** le texte d'une cellule vide reprend la police, le corps, la
  couleur et l'alignement d'un bloc de référence : le plus proche de la même
  colonne (hors en-tête), sinon de la même ligne, sinon du tableau. Il est
  placé comme ce bloc dans sa cellule (même bord d'alignement, même distance au
  haut de la cellule, ou centré verticalement comme lui) et dessiné après son
  objet texte, avec son état graphique. La police suit D20 (police d'origine
  si elle contient les caractères, sinon police du Mac, sinon police
  standard).
- **Raisons :** une valeur saisie dans une colonne de montants doit ressembler
  aux autres montants ; l'en-tête, souvent en gras, n'est pris qu'en dernier
  recours.

## D37 — Retouche d'un scan : rustines d'image sans perte posées sur l'original (jalon 7)

- **Retenu :** l'image scannée n'est jamais réencodée. Chaque retouche
  (texte réécrit, objet effacé ou déplacé) produit des rustines : des images
  RGB compressées sans perte (Flate) qui ne contiennent que les pixels changés,
  avec un masque `/SMask` tout-ou-rien, alignées sur la grille de pixels du
  scan. À l'enregistrement, elles sont ajoutées au flux de contenu
  (`q … cm /PSRTn Do Q`) après le contenu d'origine, enfermé dans `q … Q`, et
  avant la couche de texte invisible.
- **Alternatives :** réécrire toute l'image (JPEG : nouvelle perte sur toute la
  page ; PNG : fichier bien plus lourd) ; dessiner du texte vectoriel par-dessus
  (net, donc visible sur un scan flou et granuleux).
- **Raisons :** la résolution et la qualité du scan sont intactes hors des
  zones retouchées, le fichier grossit peu, et le rendu des rustines est celui
  de l'aperçu. Contrepartie assumée : les pixels d'origine restent dans le
  fichier sous les rustines (voir LIMITES.md, ce n'est pas un caviardage).

## D38 — Espace de pixels canonique, indépendant de la rotation (jalon 7)

- **Retenu :** le scan est analysé dans l'espace de la page sans `/Rotate`
  (origine en haut à gauche de la `MediaBox`), à la résolution native de
  l'image (plafonnée à 600 ppp et 60 millions de pixels). Les numéros des
  objets d'un scan (lignes, tableaux, éléments) sont des empreintes de leur
  position dans cet espace, marquées par le bit 30 (`SCAN_ID`).
- **Raisons :** faire pivoter la page (organisation) ne change ni les numéros
  ni les retouches, et l'annulation reste cohérente ; les rustines tombent sur
  la grille de pixels du scan quelle que soit la rotation d'affichage.

## D39 — Objets d'un scan : lignes reconnues, tableaux et éléments de l'image (jalon 7)

- **Retenu :** une page est traitée comme un scan si elle n'a aucun texte
  natif et qu'une image couvre au moins la moitié de la page, et seulement une
  fois sa couche de texte reconnue (jalon 5). Les lignes viennent de la couche
  reconnue, coupées par cellule et aux grands blancs ; les filets sont
  cherchés dans l'image, inclinaison compensée, puis passés à `layout::detect`
  (D32) pour retrouver les tableaux ; le reste de l'encre (logos, signatures,
  tampons) forme des éléments, par composantes connexes hors des mots.
- **Raisons :** réutiliser la reconnaissance et la détection des tableaux
  existantes ; un objet du scan se sélectionne, se déplace et se supprime
  comme un objet natif, avec les mêmes commandes et la même annulation.

## D40 — Effacement : fond estimé, Telea, grain réinjecté (jalon 7)

- **Retenu :** l'encre à effacer est sélectionnée par seuil local autour de
  l'objet, avec une marge ; la zone est remplie par l'algorithme de Telea
  (propagation depuis le bord), mélangé à la couleur du papier estimée loin du
  bord ; le grain du papier, mesuré autour, est ajouté ensuite.
- **Alternatives :** couleur unie (tache visible sur un papier teinté ou
  granuleux) ; synthèse de texture par patchs (plus lente, plus de risques
  d'artefacts sur une petite zone).
- **Raisons :** sur du papier uni, ce qui est la règle des documents
  administratifs, le résultat est invisible à 100 % ; sur un fond texturé ou
  tramé, une trace reste visible (voir LIMITES.md).

## D41 — Texte réécrit : police retrouvée, rendu assorti au scan (jalon 7)

- **Retenu :** à la première modification d'une ligne, les mots du scan sont
  comparés, glyphe par glyphe, au rendu des polices du catalogue (crate
  `fonts`) : polices fournies (Carlito, Caladea, équivalents métriques de
  Calibri et Cambria, OFL) et familles bureautiques installées sur le Mac. Les
  trois plus proches sont gardées avec leur ressemblance (0 à 1). Le corps,
  l'interlettrage, la graisse, le flou, le grain, la couleur de l'encre et
  l'inclinaison de la ligne sont mesurés sur le scan. Seuls les mots changés
  sont effacés et redessinés ; la fin inchangée de la ligne est découpée et
  décalée telle quelle.
- **Raisons :** les mots qui ne changent pas gardent leurs pixels d'origine ;
  le nouveau texte est rendu à la résolution du scan puis flouté et bruité
  comme lui, pour ne pas paraître plus net. Les réglages mesurés sont gardés
  (bouton « Réglages mesurés ») et chaque réglage fin est une modification
  annulable.

## D42 — Comparaison avant/après : pages d'origine dans le même canevas (jalon 7)

- **Retenu :** « Comparer avec l'original » (⌥⌘K, bouton de la barre d'outils)
  montre les pages sans les modifications de leur contenu, à la même place et
  au même grossissement ; on bascule d'un état à l'autre. Rien n'est
  modifiable pendant la comparaison ; une modification ou une annulation y met
  fin.
- **Alternative :** deux pages côte à côte (moitié de la largeur chacune, et
  deux défilements à synchroniser).
- **Raisons :** une retouche réussie se juge en basculant sur place : l'œil
  repère le moindre décalage. Le rendu d'origine est mis en cache comme les
  autres.

## D43 — Règles visuelles réunies dans `Design.swift` (jalon 8)

- **Retenu :** un seul fichier, `app/Views/Design.swift`, fixe les règles du §5
  et toutes les vues s'y réfèrent : espacements multiples de 4 pt (4, 8, 12, 16,
  32), deux rayons à arrondi continu (12 pt pour les panneaux posés sur les
  pages, 4 pt pour le cadre de sélection d'une vignette), capsules pour les
  bandeaux, une surface commune (matériau `.regularMaterial`, filet de 0,5 pt,
  ombre douce). Le texte est en SF Pro, en trois tailles : `.title3` pour les
  écrans vides, `.body` par défaut (aussi en gras pour un titre de panneau),
  `.caption` pour les précisions. Les commandes du panneau de retouche ne sont
  plus en petite taille, qui aurait ajouté une quatrième taille de texte.
- **Couleurs :** l'accent du système seul, pour ce qui est actif ou
  sélectionné. Les mises en garde gardent le triangle jaune du système et les
  mots douteux de l'OCR leur surlignage orange : ce sont des signaux d'état,
  pas des accents. Les occurrences de la recherche ont la couleur de
  surlignage de macOS.
- **Raisons :** des valeurs dispersées (10, 14, 8 et 10 pt de rayon) divergeaient
  d'une vue à l'autre ; une seule source les garde cohérentes.

## D44 — Barre d'outils : six outils, toujours visibles (jalon 8)

- **Retenu :** de gauche à droite, après le bouton de la barre latérale :
  Lecture, Pages, Édition (un contrôle segmenté, ⌘1 à ⌘3), Reconnaître le
  texte (OCR), Comparer avec l'original (⌥⌘K) et Enregistrer (⌘S). Les deux
  derniers restent visibles dans tous les modes, grisés quand ils ne servent
  pas (aucune page modifiée à comparer, rien à enregistrer, grille
  d'organisation pour la comparaison) : décision de Sylvain du 23 octobre
  2026, qui préfère les voir toujours à la même place. Comparer n'apparaissait
  d'abord qu'en mode Édition.
- **Correspondance avec le cahier des charges** (Sélection, Texte, Image, OCR,
  Pages, Enregistrer) : la sélection, la modification du texte et celle des
  images se font toutes dans le mode Édition, d'un clic ou d'un double-clic sur
  l'objet ; il n'y a pas d'outil distinct, puisque l'application ne crée pas
  de nouveau bloc de texte ni de nouvelle image (§4 ne le demande pas). Le mode
  Lecture tient la place de l'outil Sélection lorsqu'on ne modifie rien.
- **Raisons :** six outils, la limite du §5 ; des boutons toujours à la même
  place ; un bouton Enregistrer actif rappelle qu'un document est modifié.
- **Révisé par D58 (23 octobre 2026) :** le bouton « Reconnaître le texte » est
  retiré, la reconnaissance se faisant à l'entrée en mode Édition ; la barre
  compte désormais cinq outils, toujours aux mêmes places.

## D45 — Animations : ressort de 0,22 s, fondu seul si les animations sont réduites (jalon 8)

- **Retenu :** les panneaux et bandeaux (recherche, retouche, mode Édition,
  comparaison, avancement) apparaissent en fondu avec un glissement de 8 pt
  depuis leur bord, par un ressort de 0,22 s sans rebond ; le passage à la
  grille d'organisation et le retour se font en fondu. Une page ou une vignette
  rendue pour la première fois se dévoile en fondu de 0,15 s ; une image plus
  nette (zoom) remplace la précédente sans transition, et une page déjà en
  cache s'affiche aussitôt. Le défilement animé (recherche) dure 0,2 s. Si
  « Réduire les animations » est activé, les glissements disparaissent : il ne
  reste que des fondus de 0,15 s, et le défilement est immédiat.
- **Raisons :** §5 (150–250 ms, courbes naturelles, rien de gratuit) ; les
  recommandations d'Apple remplacent les mouvements par des fondus quand les
  animations sont réduites.

## D46 — Mode sombre : pages blanches, ombre adaptée au fond du canevas (jalon 8)

- **Retenu :** les pages restent blanches dans les deux modes (c'est le
  document) ; le canevas garde la couleur « sous la page » du système, distincte
  de la page dans les deux modes. L'ombre des pages, invisible sur le fond
  sombre à l'opacité du mode clair, passe de 0,14 à 0,5 en mode sombre ; les
  vignettes de la barre latérale reçoivent un filet de séparation, comme
  celles de la grille. La saisie d'un texte se fait en noir sur blanc, comme
  sur la page.
- **Alternative :** inverser les couleurs des pages en mode sombre (au risque de
  fausser le contrôle d'une retouche).

## D47 — Icône : source SVG, images générées et versionnées (jalon 8)

- **Retenu :** `app/Resources/AppIcon.svg` est l'unique source : sur la grille
  des icônes de macOS (forme de 824 pt, superellipse d'exposant 5, sur une toile
  de 1 024), une page blanche au coin replié, quatre lignes grises et une ligne
  bleue suivie d'un curseur de saisie, sur un fond bleu profond. `make icon`
  (`tools/icon/render_icon.py`, resvg et Pillow) en tire les dix images du jeu
  d'icônes (`Assets.xcassets/AppIcon.appiconset`), versionnées : la compilation
  n'a pas besoin de Python. Les lignes sont assez épaisses pour rester
  distinctes à 32 pixels.
- **Alternative :** le format `.icon` d'Icon Composer (macOS 26), à couches
  « Liquid Glass ».
- **Raisons :** le catalogue d'images classique fonctionne de macOS 14 à
  macOS 27 ; le format `.icon` n'est lisible que par les Xcode récents et ne
  s'écrit pas à la main de façon fiable.

## D48 — Accessibilité : objets de la page pour VoiceOver, Tab entre les objets (jalon 8)

- **Retenu :** en mode Édition, chaque objet sélectionnable d'une page est un
  élément d'accessibilité (`PageObjectElement`) : rôle texte, image ou groupe,
  libellé « Bloc de texte : début du texte », aide sur les touches ; l'activer
  (VO-Espace) le sélectionne, et la sélection est annoncée. La page passe du
  rôle image au rôle groupe pour que VoiceOver y entre. Tab et ⇧Tab
  sélectionnent l'objet suivant ou précédent dans l'ordre de lecture (de haut en
  bas, puis de gauche à droite), puis passent aux pages voisines ; au bout du
  document, Tab rend la main à la vue suivante de la fenêtre. Avec « Augmenter
  le contraste », les contours de sélection et le filet des panneaux
  s'épaississent. Les symboles des écrans vides suivent la taille du texte
  (`@ScaledMetric`).
- **Dynamic Type :** l'interface n'emploie que des styles de texte (`.body`,
  `.caption`, `.title3`), jamais de taille fixe ; macOS 14 n'offre pas de
  réglage global de la taille du texte pour les applications tierces, donc
  l'effet réel dépend du système (voir LIMITES).

## D49 — Captures d'écran par un test, sans autorisation système (jalon 8)

- **Retenu :** `make snapshots` exécute `DesignSnapshotTests`, ignoré par les
  tests ordinaires : il ouvre des documents du jeu de test dans des fenêtres à
  barre d'outils unifiée, en mode clair puis sombre (accueil, lecture,
  vignettes, grille, édition avec sélection, recherche), et en écrit l'image
  par `cacheDisplay`. Les images sont copiées dans
  `~/Library/Caches/PDFStudio/snapshots`.
- **Alternatives :** `screencapture` ou ScreenCaptureKit, qui demandent
  l'autorisation d'enregistrer l'écran ; une revue uniquement manuelle.
- **Raisons :** aucune autorisation macOS n'est demandée ; la revue visuelle
  du design se refait en une commande après chaque changement. Les matériaux
  translucides y paraissent sans le flou de l'arrière-plan.

## D50 — Une requête du moteur qui panique échoue seule (jalon 9)

- **Retenu :** le fil d'un document (`DocumentHandle`) exécute chaque requête
  sous `catch_unwind` : une panique du code Rust devient l'erreur « panique du
  moteur : … », renvoyée à l'appelant, et le fil continue de servir les
  requêtes suivantes ; l'analyse du fichier à l'ouverture est protégée de la
  même façon (le document est alors refusé comme invalide). Le fil d'un
  document dispose d'une pile de 16 Mio, pour les documents très imbriqués.
- **Plan d'un document abîmé :** avant de le lire, le moteur parcourt le plan
  (`/Outlines`) lui-même. Une entrée sans titre reçoit un titre vide (en
  mémoire seulement) ; un plan dont une entrée est illisible, qui boucle, qui
  dépasse 64 niveaux ou 100 000 entrées est ignoré : le document s'ouvre sans
  signets. Ces deux cas faisaient planter l'application (accès mémoire invalide
  dans la bibliothèque `mupdf`), découverts par le test de robustesse (D54).
- **Alternatives :** laisser UniFFI convertir la panique (elle fait planter
  l'application si la fonction appelée ne renvoie pas d'erreur) ; corriger la
  bibliothèque `mupdf` (dépendance externe, correction hors de notre contrôle).
- **Raisons :** une erreur sur une page ne doit jamais fermer toutes les
  fenêtres ni perdre des modifications ; le fil reste cohérent, car ses
  requêtes ne font que lire le document ouvert (les modifications vivent dans
  la session d'édition) et un résultat n'entre dans ses caches qu'une fois
  calculé.

## D51 — Récupération écrite hors du fil principal, au plus 0,5 s après la modification (jalon 9)

- **Retenu :** `RecoveryWriter` reçoit les écritures de récupération de toutes
  les fenêtres sur une file série. Les écritures
  rapprochées d'un même document sont regroupées : la dernière est faite au
  plus 0,5 s après la première. La lecture des modifications dans le moteur,
  l'encodage JSON et l'écriture du fichier s'y font ; les modifications et
  couches de texte déjà lues sont gardées par page et par état de la page, si
  bien qu'une modification ne relit que sa page. Les suppressions (document
  enregistré, modifications abandonnées) passent par la même file, après
  l'écriture en cours, et annulent celle qui attend. Quitter l'application,
  ouvrir un document ou copier la version d'ouverture écrit d'abord ce qui
  attend.
- **Priorité :** la file, comme la lecture du plan et la recherche d'une
  signature à l'ouverture, a la priorité des actions de l'utilisateur
  (`userInitiated`), celle du rendu des pages. Le fil d'un document prend la
  priorité de la requête qu'il exécute : une requête de moindre priorité en
  cours faisait attendre le rendu (inversion de priorité signalée par macOS),
  et bloquait les mesures de `make perf`.
- **Alternative :** l'écriture immédiate sur le fil principal (jalons 3 à 8).
- **Raisons :** chaque modification relisait toutes les pages modifiées du
  document sur le fil principal ; sur un document de 600 pages reconnues, cela
  devenait perceptible (§6 : 60 images par seconde). Un arrêt brutal fait
  perdre au plus la dernière demi-seconde de modifications.

## D52 — Enregistrement interrompu : un journal pour rétablir le fichier (jalon 9)

- **Retenu :** avant de réécrire un fichier en place (D13), `FileSaver` pose
  une note dans `Sauvegardes/Écritures en cours` : chemin du fichier, copie de
  sauvegarde et empreinte SHA-256 du contenu à écrire. Elle est retirée une
  fois l'écriture vérifiée, ou l'original rétabli. Si une note subsiste à
  l'ouverture du document (arrêt de l'application ou du Mac pendant
  l'écriture), le fichier est comparé à l'empreinte : identique, l'écriture
  était complète ; sinon la copie de sauvegarde est remise en place et un
  bandeau le signale. Si la copie est introuvable, la note est gardée et un
  message nomme la copie attendue. La copie d'une écriture en cours n'est
  jamais supprimée par la limite des 30 copies.
- **Alternative :** l'écriture atomique par échange de fichiers, écartée en D13.
- **Raisons :** la réécriture en place garde l'emplacement, les étiquettes et
  la synchronisation du fichier (D13) ; le journal ferme le dernier cas où un
  original pouvait rester à moitié écrit (§6 : aucun original modifié sans
  confirmation).

## D53 — Appariement des polices d'un scan en parallèle (jalon 9)

- **Retenu :** l'évaluation des polices candidates d'une ligne scannée (D41) se
  répartit sur les cœurs du Mac (fils de `std::thread::scope`, autant que de
  cœurs disponibles) ; l'affinage des trois meilleures familles aussi. Le
  classement est identique à celui du calcul en série.
- **Raisons :** c'est l'étape la plus longue de la réécriture d'une ligne
  scannée ; les candidates sont indépendantes. Aucune dépendance ajoutée.

## D54 — Jeu de tests du durcissement (jalon 9)

- **Retenu :**
  - **Robustesse du moteur** (`engine/ffi/tests/robustesse.rs`, dans
    `make test-engine`) : seize PDF du jeu de test, abîmés de façon
    déterministe (fichier tronqué, octets ou chiffres remplacés, bloc effacé,
    mots-clés de structure altérés), plus de 200 cas, parcourus comme le fait
    l'application (ouverture, rendu, texte, recherche, plan, objets, couche de
    texte, modifications, enregistrement et relecture, annulation). Une erreur
    est admise ; une panique, un blocage de plus de 60 s ou un PDF enregistré
    illisible font échouer le test. `PDFSTUDIO_ROBUSTESSE=25` multiplie les
    tirages.
  - **Arrêt imprévu** (`CrashRecoveryTests`, `InterruptedWriteTests`) :
    modifications retenues puis restaurées dans une nouvelle fenêtre, original
    intact, enregistrement interrompu rétabli, ⌘S sans confirmation ne touche
    pas au fichier.
  - **Confidentialité** (`PrivacyTests`) : l'application signée est dans le
    bac à sable sans autorisation réseau.
  - **Performances** (`make perf`, `PerformanceTests`, hors de
    `make test-app`) : ouverture, défilement, reconnaissance, réécriture d'une
    ligne scannée et écriture de la récupération, mesurés dans l'application
    compilée avec optimisations ; rapport dans
    `~/Library/Caches/PDFStudio/perf/rapport.md`.
- **Raisons :** §8, jalon 9 ; les mesures de performance dépendent de la
  machine et ne doivent pas faire échouer les tests ordinaires.

## D55 — Aucune connexion réseau : garantie par le bac à sable (jalon 9)

- **Retenu :** l'application est dans le bac à sable de macOS sans les
  autorisations `network.client` ni `network.server` : le système refuse toute
  connexion, quel que soit le code. Un test le vérifie sur l'application
  signée (D54). Le moteur ne dépend d'aucune bibliothèque réseau (vérifié par
  `cargo tree`), et le code Swift n'emploie aucune API réseau.
- **Raisons :** §6 (aucune connexion réseau) ; une garantie du système vaut
  mieux qu'une relecture du code.

## D56 — Page lue publiée sans recalculer la fenêtre (jalon 9)

- **Retenu :** la page lue change à chaque page qui défile. Seuls en dépendent
  désormais le sous-titre de la fenêtre et le suivi de la page par les
  vignettes, calculés chacun dans un modificateur SwiftUI à part
  (`DocumentSubtitle`, `FollowCurrentPage`). Les menus ne lisent plus la page
  lue : « page précédente/suivante possible » et le nombre de pages
  sélectionnées sont publiés seulement quand ils changent (`canGoBack`,
  `canGoForward`, `selectionCount`, `targetCount`).
- **Raisons :** le sous-titre était calculé dans le corps de `DocumentView`,
  si bien que toute la fenêtre, la liste des vignettes et les menus étaient
  recalculés deux fois par seconde pendant un défilement rapide, avec des
  images manquées (mesures de `make perf`, `docs/PERFORMANCES.md`).

## D57 — Sélection et copie du texte en mode Lecture (suite du jalon 5)

- **Retenu :** en mode Lecture, le texte des pages se sélectionne à la souris
  (glisser, ⇧-clic, double-clic pour un mot, triple-clic pour une ligne, défilement
  automatique au bord) et se copie par ⌘C, sans AppKit `PDFView` ni nouvelle
  dépendance. Le moteur fournit les lignes et l'emprise de chaque caractère
  (`selectable_text`), tirées du texte déjà employé par la recherche : le texte
  reconnu se sélectionne donc dès la fin de la lecture, avant l'enregistrement,
  comme après la réouverture du fichier (texte invisible). L'application garde la
  sélection par identifiant de page, sur plusieurs pages, et l'abandonne si le
  texte de la page change.
- **Texte natif aussi :** le même mécanisme sert à tout le texte de la page ; le
  limiter à la couche OCR aurait demandé de distinguer les deux sans bénéfice.
- **Pages modifiées :** une page dont le contenu natif a été modifié en mode
  Édition, sans couche reconnue, n'a pas de texte sélectionnable : le texte
  d'origine ne correspondrait plus à ce qui est affiché. Une page scannée
  retouchée, elle, se sélectionne avec son texte retouché (couche de texte à jour).
- **Mode Édition :** inchangé ; la sélection y est abandonnée et la souris revient
  aux objets de la page.
- **Copie :** une ligne par ligne, ligatures développées (« ﬁ » → « fi »), sans
  recoller les mots coupés en fin de ligne ; l'ordre des lignes est celui du moteur
  (ordre de lecture de MuPDF pour le texte natif, ordre de l'OCR pour le reste).
- **Alternatives écartées :** `PDFView` de PDFKit pour la lecture (remplacerait le
  rendu par MuPDF, D2) ; un champ de texte invisible par mot (lourd, et
  inutilisable pour une sélection continue sur plusieurs lignes) ; la sélection
  dans le moteur (la géométrie du pointeur est un souci d'interface).
- **Raisons :** demande de Sylvain (2026-10-23) : copier le texte reconnu depuis
  le volet Lecture.

## D58 — Reconnaissance du texte intégrée au mode Édition (après le jalon 9)

- **Retenu :** le mode Édition (bouton crayon, ⌘3) commence par chercher les
  pages scannées qui n'ont pas encore été reconnues. Un PDF natif passe
  aussitôt en Édition. Pour un PDF scanné, le texte de toutes ses pages
  scannées est d'abord lu par Vision, page après page, avec un bandeau
  d'avancement et le bouton « Arrêter » (⌘.) ; les pages restent figées et
  montrées comme en lecture, puis la couche de texte est appliquée en une étape
  d'annulation (D29) et les objets des pages deviennent modifiables. Le
  document s'ouvre toujours en Lecture : c'est l'entrée en Édition qui
  déclenche la lecture. La recherche des pages scannées ne s'affiche que si
  elle dure plus de 0,4 s.
- **Pages concernées :** une page sans aucun texte mais qui dessine quelque
  chose (règle du jalon 5), et désormais une page dont le seul texte est
  invisible (mode de rendu 3), posé sur une image qui couvre au moins la moitié
  de la page : la couche d'un scanner, d'un autre logiciel ou d'un
  enregistrement précédent de PDF Studio. Sur un document mixte, seules les
  pages scannées sont lues. Le résultat de l'examen de chaque page source est
  retenu par le moteur ; sans opérateur `Tr` dans son contenu, une page qui a
  du texte est écartée sans être interprétée. Mesure dans le cloud (Linux,
  compilation optimisée) : 0,4 s pour les 600 pages de `volumineux_600.pdf`.
- **Couche existante remplacée :** sur une telle page, la nouvelle lecture
  n'écarte aucun mot (le texte invisible ne compte pas comme un texte de la
  page) ; avant l'enregistrement, la recherche et la sélection ne voient plus
  que la nouvelle couche ; à l'enregistrement, les opérations d'affichage du
  texte de l'ancien contenu sont retirées (`rewrite::without_text`), le reste
  du flux est recopié octet pour octet. La page n'ayant aucun texte visible,
  rien d'autre ne change. Elle devient retouchable comme un scan reconnu dans
  la session (D39), ce qui lève la limite du jalon 7 pour ces pages. Par
  prudence, un texte en mode détourage (7), un formulaire (dont le texte n'est
  pas interprété) ou un glyphe de police illisible font traiter la page comme
  ayant du texte visible.
- **Arrêt et sortie :** « Arrêter » applique les pages déjà lues et laisse en
  Édition ; les pages restantes sont lues à la prochaine entrée en Édition.
  Quitter l'Édition pendant la recherche n'en lit aucune ; pendant la lecture,
  celle-ci continue (on peut l'arrêter). Une page lue sans y trouver de texte
  (photo, dessin) n'est pas relue dans la même fenêtre. Des pages insérées
  pendant l'Édition sont lues aussitôt.
- **Commandes retirées :** le bouton « Reconnaître le texte » de la barre
  d'outils (D44) et les commandes du menu Pages « Reconnaître le texte des
  pages scannées » (⌥⌘O) et « … des pages sélectionnées » ; « Arrêter la
  reconnaissance du texte » (⌘.) reste. Il n'est plus possible de faire relire
  une page qui a du texte visible.
- **Alternatives :**
  - garder un bouton OCR à côté du mode Édition : deux chemins pour le même
    résultat, ce que Sylvain veut simplifier ;
  - lire les scans dès l'ouverture du document : du temps de calcul pour un
    document qu'on ne fait que lire, en concurrence avec le rendu des pages ;
  - reprendre la couche existante sans la relire : instantané, mais sans la
    confiance des mots (mots douteux non signalés) et avec la qualité, variable,
    de l'OCR d'un autre logiciel. Reste une évolution possible pour la couche
    écrite par PDF Studio lui-même.
- **Raisons :** demande de Sylvain du 23 octobre 2026 : fusionner la
  reconnaissance du texte et le mode Édition pour simplifier l'expérience ; un
  PDF natif passe directement en Édition, un PDF scanné est d'abord reconnu
  en entier.

## D59 — Lignes et paragraphes d'un scan reconnu (après le jalon 9)

- **Constat :** sur un courrier tapé en chasse fixe et justifié, Vision coupe
  chaque ligne en plusieurs morceaux dès que les espaces s'élargissent, saute
  des mots courts isolés entre deux grands blancs, et donne parfois à un mot
  une emprise qui couvre aussi la ligne du dessus. Chaque morceau devenait un
  bloc : modifier une phrase obligeait à retoucher et replacer plusieurs blocs.
- **Retenu :**
  - *Mots oubliés :* après la lecture, les zones d'encre d'une ligne qu'aucun
    mot ne couvre sont relues en un seul appel à Vision (image composite des
    morceaux), et les mots trouvés sont insérés dans leur ligne. Les emprises
    deux fois trop hautes sont ramenées à leur ligne, au blanc qui les sépare
    de la précédente (`ocr::recover`).
  - *Regroupement :* les mots sont rangés par la hauteur de leur centre sur la
    page redressée, sans se fier aux lignes de Vision ; une rangée n'est coupée
    qu'aux grands blancs (1,5 hauteur de mot, 2,5 si la ligne est justifiée,
    c'est-à-dire si ses espaces élargis ont une largeur voisine), ou au
    changement de cellule d'un tableau. Les lignes sont mesurées sur l'encre
    (ligne de base, hauteur, graisse, couleur, bande d'encre des accents aux
    jambages) puis regroupées en paragraphes et listes comme le texte natif
    (D25 : même corps, même graisse, même couleur, interligne régulier, bords
    alignés) ; un morceau resté seul sur la rangée d'un paragraphe rejoint sa
    ligne (`pdf_core::scanned::blocks`).
  - *Réécriture :* un bloc se modifie d'un seul tenant, comme un bloc natif.
    Les mots inchangés gardent leurs pixels (appariement par plus longue
    sous-suite commune) ; les nouveaux sont dessinés dans la police retrouvée.
    Un paragraphe est remis à la ligne dans sa largeur à partir de la première
    ligne modifiée, en gardant l'espace d'origine entre deux mots restés
    voisins ; dès que la suite du texte redevient celle d'origine, les lignes
    sont reprises telles quelles, décalées au besoin. Une liste garde ses
    retours à la ligne. L'alignement (gauche, droite, centré, justifié) est
    conservé (`pdf_core::scanned::reflow`).
  - *Encre des voisines :* une composante d'encre appartient à la ligne dont
    le corps (des lettres basses à la ligne de base) est le plus proche de son
    centre ; elle n'est jamais effacée ni déplacée avec une autre ligne. La
    police d'une ligne est mesurée dans sa seule bande d'encre.
- **Alternatives :** se fier aux lignes de Vision (le défaut constaté) ;
  regrouper par proximité des emprises (trompé par les hauteurs variables des
  emprises de Vision) ; relire toute la page avec un autre moteur (cf. l'étude
  PaddleOCR-VL : trop lent, sans confiance par mot) ; réécrire tout le
  paragraphe dans la police retrouvée (le texte inchangé perdrait son aspect
  d'origine).
- **Raisons :** demande de Sylvain (2026-10-24) : sur le courrier d'assurance
  de test, la reconnaissance se faisait par blocs de quelques mots, au lieu de
  lignes et de paragraphes modifiables d'un seul tenant.


## D60 — Jeu de test du mode Amélioration : scans générés et vérité terrain (Amélioration, jalon 1)

- **Retenu :** douze scans générés par `tests/fixtures/generate.py`
  (`amelioration`) et un lot de 50 pages, dans `tests/fixtures/amelioration/`.
  Chaque page est dessinée droite et propre à la résolution du scan, en
  relevant tout ce qu'elle contient, puis dégradée : papier teinté, éclairage
  inégal, inclinaison, quarts de tour, perspective et courbure, flou, bruit,
  JPEG, verso transparent. Un `.json` par fichier donne la vérité terrain :
  angle, orientation, coins de la page dans l'image, zones de papier vierge,
  aplats (couleur imprimée et couleur dominante dans le scan), photos et
  dégradés à ne pas aplatir, filets fins, texte et emprise de chaque ligne.
  Le PDF ne contient que l'image JPEG de chaque page, écrite telle quelle
  (pikepdf), comme un scanner. Le lot de 50 pages partage les images de trois
  fichiers (1 Mio).
- **Alternatives :** de vrais scans, annotés à la main (pas de vérité exacte
  sur l'angle ni les couleurs, et pas de documents publiables) ; une
  dégradation à la volée dans les tests, comme `ocr_bench` (le même jeu doit
  servir aux tests Rust, au banc d'essai et aux essais dans l'application).
- **Raisons :** chaque critère du mode (angle résiduel, écart de couleur du
  fond, couleur des aplats, photos intactes, taux de caractères corrects) se
  chiffre sur une référence connue. Le test `amelioration_jeu.rs` vérifie que
  la vérité terrain tombe au bon endroit de l'image. Le jeu reste synthétique ;
  les documents réels de Sylvain servent de contrôle (`docs/AMELIORATION.md`).


## D61 — Mesure de l'inclinaison et du sens d'une page (Amélioration, jalon 2)

- **Retenu :** sur l'image du scan égalisée (`flatten_background`), remise
  d'aplomb si ses lignes sont verticales (`is_sideways`, D27) :
  - *inclinaison* par profil de projection (`ocr::preprocess::measure_skew`),
    sur l'encre de forme textuelle seule (les ombres, bandeaux et photos sont
    écartés), cherchée de −45° à 45° puis affinée au centième de degré ; les
    points échantillonnés sont décalés au hasard (graine fixe) dans leur
    pixel pour qu'aucune direction de la grille (45°, 26,6°) ne ressorte ;
  - *fiabilité* par la saillance : score de l'angle retenu rapporté à la
    médiane des scores ; en dessous de 4, ou avec moins de 500 points
    d'encre, rien n'est corrigé ;
  - *sens* par le rapport des hampes aux jambages (`ascender_ratio`), mesuré
    sur la page et sur la page retournée (moyenne géométrique des deux) : au
    moins 1,5, la page est à l'endroit ; au plus 1/1,5, à l'envers ; entre
    les deux, Vision lit la page dans les deux sens et la lecture la plus
    sûre décide ; sans réponse nette, la page garde son sens ;
  - *garde-fous* réglables : seuil de 0,1°, plafond de 15° (au-delà, page
    « à vérifier », non redressée).
- **Alternatives :** transformée de Hough sur les lignes de base (sensible
  aux filets et aux bords de photos, plus lente) ; recherche limitée à ±15°
  (faux angle de 15,4° trouvé sur la page inclinée de 18°) ; sens par la
  seule lecture OCR dans les quatre sens (quatre lectures par page, quand la
  forme des lignes suffit presque toujours).
- **Raisons :** sur le jeu de test, écart à la vérité de 0,02° au plus, et
  sens juste pour toutes les pages, sans lecture ; la mesure de départ
  (`estimate_skew`, D27) était déjà de cette précision dans sa plage. Le
  chemin de l'OCR (`estimate_skew`) n'est pas modifié.

## D62 — Rotation bicubique et coins à la couleur du papier (Amélioration, jalon 2)

- **Retenu :** rotation autour du centre, image de même taille (la page
  garde son format et sa place), interpolation bicubique (noyau de Keys,
  a = −0,5), calculée en parallèle par bandes de rangées. Les pixels venus
  d'en dehors de l'image prennent la couleur du papier au bord le plus
  proche : couleur estimée par blocs (24 sur le grand côté, médiane des
  pixels clairs, blocs couverts remplacés par leurs voisins) et, pour les
  bords, sur une bande étroite juste à l'intérieur ; la bande que le
  couvercle du scanner occupe autour d'une page inclinée (largeur × sinus
  de l'angle) est écartée de l'estimation.
- **Alternatives :** Lanczos (plus net, avec un léger rebond autour des
  traits, plus lent) ; une couleur unique pour toute la page (visible comme
  une pièce rapportée sur un éclairage inégal : 19 niveaux d'écart sur la
  photocopie 06) ; blanc (le fond n'est blanchi qu'à l'étape 2, jalon 3).
- **Raisons :** aller-retour de 3° quasi exact loin des bords ; coins à
  moins de 16 niveaux du papier voisin sur tout le jeu. Le prompt demande
  la couleur du fond « estimée à l'étape 2 » : au jalon 3, le fond devenant
  blanc, les coins le seront aussi.

## D63 — Image améliorée en JPEG de qualité 90, par `jpeg-encoder` (Amélioration, jalon 2)

- **Retenu :** l'image redressée est encodée en JPEG de qualité 90 (réglable
  de 50 à 100), chrominance sous-échantillonnée (4:2:0) comme les scans,
  tables de Huffman optimisées, en niveaux de gris si tous les pixels sont
  gris ; encodeur `jpeg-encoder` 0.7 (Rust pur, MIT ou Apache-2.0, avec une
  part sous licence IJG, celle de libjpeg déjà présente par MuPDF).
- **Alternatives :** sans perte (Flate : dix fois plus lourd) ; l'encodeur
  JPEG de MuPDF (non exposé par le crate `mupdf` 0.8) ; le crate `image`
  (bien plus de dépendances pour le seul encodage) ; garder la qualité de
  l'original (inconnue sans lire ses tables de quantification).
- **Raisons :** le scan d'origine est déjà un JPEG ; la qualité 90 n'ajoute
  pas de dégradation visible. Coût mesuré : fichier 1,3 à 1,9 fois plus
  lourd, 3,5 fois pour un scan d'origine très compressé (consigné dans
  `docs/AMELIORATION.md`).

## D64 — La page améliorée remplace la page d'origine (Amélioration, jalon 2)

- **Retenu :** les images améliorées forment un document de plus, ajouté
  aux sources de la session comme un document inséré (une page par page
  améliorée, au format, aux cadres et à la rotation de la page d'origine,
  l'image posée à la place exacte du scan). Dans la session, la page vise
  ce document et retient sa page d'origine (`PageEntry.replaces`) ; les
  modifications de contenu et la couche de texte de la page sont retirées
  quand l'image change. Un quart ou un demi-tour passe par la rotation de la
  page, sans réécrire l'image. À l'écriture (`OutputPage::Replaced`), l'objet
  de la page d'origine est gardé, avec ses annotations, liens, champs et les
  signets qui le visent ; seuls `Contents`, `Resources` et les cadres sont
  remplacés. Une page modifiée en Édition, ou qui n'est pas un scan pleine
  page (image couvrant moins de 90 % de la page), n'est pas améliorée. Les
  deux opérations (« l'amélioration des pages », « le retour aux pages
  d'origine ») s'annulent ; la récupération retient la page d'origine.
- **Alternatives :** remplacer la page entière par la page améliorée (les
  signets, liens et annotations seraient perdus) ; garder aussi l'image
  d'origine dans le fichier (point ouvert du jalon 1 : non retenu par défaut,
  le fichier doublerait de taille) ; réécrire l'image pour les quarts de
  tour (perte JPEG sans raison).
- **Raisons :** une page améliorée reste la même page pour le reste du
  document ; l'annulation, la comparaison (⌥⌘K), l'enregistrement et la
  récupération reposent sur les mécanismes existants (sources de la session,
  D12, D51).

## D65 — Interface du mode Amélioration (Amélioration, jalon 2)

- **Retenu :** quatrième mode de la barre d'outils (⌘4). La page affichée
  est montrée avant et après, superposées, avec un curseur à faire glisser ;
  les deux images sont rendues par le moteur dans le sens final, pour être
  comparables. Panneau à droite : redressement et orientation, pages
  traitées (toutes, sélection, page affichée), réglages avancés repliés
  (seuil, plafond, qualité), bilan de la page, Appliquer, Rétablir
  l'original, pages à vérifier. Le traitement se fait page après page hors
  du fil principal, pages figées, avec un bandeau d'avancement et d'arrêt ;
  les pages déjà traitées sont appliquées à l'arrêt. Réglages retenus en
  JSON dans les préférences.
- **Alternatives :** l'aperçu dans le canevas de lecture (le curseur
  avant/après demande deux images de même format) ; appliquer page par page
  au fil du traitement (une étape d'annulation par page).
- **Raisons :** reprise des usages des modes Édition et OCR (D58) ; le lot
  parallèle et l'enregistrement dans un nouveau fichier viennent aux jalons
  4 et 6, comme prévu au plan.

## D66 — Onglets : les onglets de fenêtre de macOS, un document par fenêtre

- **Retenu :** chaque document garde sa fenêtre SwiftUI (`WindowGroup`), avec
  son `DocumentViewModel`, son gestionnaire d'annulation et son délégué de
  fermeture ; ces fenêtres sont réunies en onglets par AppKit
  (`tabbingMode = .preferred`, identifiant d'onglets commun). `DocumentOpener`
  place chaque fenêtre qu'il ouvre en dernier onglet de la fenêtre d'où vient
  la demande : celle qui a reçu le dépôt, sinon la fenêtre au premier plan.
  ⌘T (Fichier > Nouvel onglet, ou le bouton + de la barre d'onglets) ouvre un
  écran d'accueil en onglet ; un écran d'accueil vide de la fenêtre accueille
  le prochain document. ⌘W ferme l'onglet, après la demande d'enregistrement
  existante. Un écran d'accueil seul dans sa fenêtre est encore fermé quand un
  document est affiché (fenêtre de lancement en surplus), mais pas un onglet
  d'accueil.
- **Alternatives :** une seule fenêtre SwiftUI avec sa propre barre d'onglets
  et plusieurs documents (il aurait fallu refaire les commandes de menu, qui
  visent le document de la fenêtre active, l'annulation, la fermeture, la
  restauration et le glisser-déposer entre fenêtres, sans gagner ce que macOS
  donne aux onglets : réordonner, détacher, fusionner, ⌃Tab, « Afficher tous
  les onglets ») ; imposer les onglets par la préférence `AppleWindowTabbingMode`
  de l'application (elle s'appliquerait aussi aux fenêtres des tests).
- **Raisons :** l'état propre à chaque document (mode, page, modifications)
  existait déjà par fenêtre ; les onglets du système sont ceux d'Aperçu et du
  Finder, attendus sur macOS (§5).

