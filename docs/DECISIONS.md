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
