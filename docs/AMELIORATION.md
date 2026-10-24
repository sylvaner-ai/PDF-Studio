# Mode Amélioration

Le mode Amélioration transforme un PDF scanné en un document proche d'un PDF
natif : pages droites, fond blanc, aplats de couleur uniformes, texte reconnu
et, sur demande, texte réel à la place du texte-image. Ce document en donne le
plan d'intégration (jalon 1) et décrit son jeu de test ; il sera complété au fil
des jalons.

## Plan d'intégration

Chaîne imposée, sur toutes les pages ou une sélection : **1.** redressement
(angle, orientation), **2.** couleurs (fond de page, aplats), **3.** OCR,
**4.** remplacement du texte-image par du texte réel (optionnel, avec garde-fous).

### Réutilisé

| Besoin | Existant | Adaptation prévue |
|---|---|---|
| Angle d'inclinaison | `ocr::preprocess::estimate_skew` (profil de projection, D27) | plage portée à ±15°, affinage |
| Orientation 90°/180°/270° | `ocr::preprocess::is_sideways`, lecture reprise dans les autres sens dans `ocr::recognize` (D27) | exposée comme étape |
| Image d'une page scannée | `pdf_core::scanned` (résolution native, espace de pixels canonique, D38) | aucune |
| Fond et encre | `scan::ink` (`Paper::closing`, masque d'encre), `ocr::preprocess::flatten_background` | version couleur, page entière |
| OCR | `ocr::recognize`, trait `TextRecognizer` (Vision), `ocr::recover` (D59) | lancé sur l'image améliorée |
| Couche de texte | `pdf_core::textlayer` (D28), `apply_recognitions` (D29) | aucune |
| Police d'une ligne | `fonts::match_line`, `ScanModel::match_style` (D41, D53) | aucune |
| Effacement | `scan::inpaint` (Telea, D40) | aucune |
| Texte vectoriel | `pdf_core::editing` (D20), blocs homogènes (D25) | posé sur l'image nettoyée |
| Annulation, enregistrement, récupération | `edit::History` (D21), `export_pdf` (D12), `FileSaver` (D13), `RecoveryStore` (D51) | une modification « page améliorée » |
| Avant/après, avancement | `DocumentViewModel+Comparing` (D42), `RecognitionViews` (D58) | curseur avant/après |
| Mesure de l'OCR | `ocr::metrics`, `engine/tools/ocr_bench` | vérité terrain ci-dessous |

### Nouveau ou modifié

- `engine/enhance` (**nouveau**) : redressement (rotation bicubique ou
  Lanczos, bords remplis de la couleur du fond), fond de page, aplats
  (segmentation, regroupement en Lab, exclusion des photos et dégradés),
  réglages sérialisables, enchaînement des étapes. Aucune dépendance nouvelle
  prévue dans l'application.
- `engine/edit` : modification « page améliorée », annulable page par page ou
  en bloc.
- `engine/pdf_core` : aperçu ; à l'écriture, image de la page remplacée,
  couche de texte, texte vectoriel de l'étape 4.
- `engine/ffi` : `EnhanceSettings`, aperçu et traitement d'une page, lot
  parallèle (`std::thread::scope`, comme D53) et annulable, mémoire bornée.
- `engine/tools/enhance_bench` (**nouveau**, développement) : mesures sur le
  jeu de test.
- Application : mode Amélioration dans la barre d'outils (⌘4), panneau
  latéral (quatre interrupteurs, plage de pages, réglages avancés repliés),
  aperçu avec curseur avant/après, revue guidée, lot en arrière-plan,
  enregistrement dans un nouveau fichier par défaut.

### Jalons

| Jalon | Contenu | Critères |
|---|---|---|
| 1 | plan, jeu de test et sa vérité terrain | jeu lu par le moteur actuel, suite au vert |
| 2 | redressement, modification de page annulable et écrite, mode et aperçu ; perspective évaluée en dernier | angle résiduel < 0,2° ; pages droites inchangées |
| 3 | fond de page puis aplats, aperçu | écart de couleur du fond ; traits fins conservés ; aplats à la couleur dominante ; photos et dégradés intacts |
| 4 | enchaînement avec l'OCR, lot parallèle et annulable, banc d'essai | caractères corrects après ≥ avant |
| 5 | remplacement du texte (seuils, police, effacement, non destructif, revue guidée), note MRC | aucun mot sous le seuil remplacé ; erreurs introduites mesurées |
| 6 | interface complète, lot en arrière-plan, nouveau fichier, documentation | 50 pages sans gel, annulables ; suite au vert |

Le mode apparaît dès le jalon 2 (il faut un aperçu et un moyen d'appliquer) ;
le moteur du lot est construit au jalon 4, avec l'enchaînement.

## Jeu de test

`tests/fixtures/amelioration/` (D60) : douze scans générés par
`python3 tests/fixtures/generate.py amelioration`, chacun accompagné d'un
`.json` de vérité terrain, et un lot de 50 pages. Les pages sont dessinées
droites et propres, puis dégradées comme par un scanner ou un téléphone ;
chaque PDF ne contient qu'une image JPEG par page, comme un vrai scan.

| Fichier | Langue | Résolution | Ce qu'il met à l'épreuve |
|---|---|---|---|
| `01_lettre_inclinee` | FR | 300 ppp | inclinaison de 2,3°, papier gris clair |
| `02_rapport_incline` | EN | 200 ppp | forte inclinaison (−7,5°) |
| `03_hors_plafond` | FR | 200 ppp | 18° : au-delà du plafond de 15°, à marquer « à vérifier » ; toute la page est visible sur le fond du scanner |
| `04_page_droite` | EN | 300 ppp | déjà droite (0,05°, sous le seuil de 0,1°) et propre : aucune dégradation admise |
| `05_fond_jauni` | FR | 200 ppp | papier jauni, ombre de reliure, filets de 0,3 pt, texte en 7 pt |
| `06_photocopie_grise` | EN | 200 ppp, gris | papier gris, éclairage très inégal |
| `07_bandeaux_couleur` | FR | 200 ppp | bandeau bleu aux nuances multiples sous du texte blanc, encadré orangé bordé, pied vert |
| `08_tableau_cellules` | EN | 200 ppp | facture : en-tête foncé, lignes alternées, cellule surlignée, ligne de total, quadrillage de 0,5 pt |
| `09_photo_et_degrade` | FR | 200 ppp | photographie et dégradé volontaire (à ne pas aplatir), encadré uni (à aplatir) |
| `10_basse_qualite` | FR | 150 ppp, gris | flou, bruit fort, JPEG de qualité 35, verso visible par transparence |
| `11_orientation` | EN, FR | 200 ppp | page à l'envers ; page couchée et inclinée de 0,8° |
| `12_photo_perspective` | FR | photo 2400 × 3200 | page photographiée : perspective, courbure, lumière en coin, ombre (option facultative) |
| `13_lot_50_pages` | — | — | 50 pages tirées en boucle de 02, 07 et 10 (images partagées), pour le lot et l'annulation |

Taille du jeu : 7,9 Mio.

### Vérité terrain

Pour chaque page (`pages[]`) :

- `format_pt`, `page_pdf_pt`, `ppp`, `image_px` : format de la page droite,
  format de la page du PDF, résolution et taille de l'image ;
- `inclinaison_deg` (positive quand les lignes descendent vers la droite,
  comme `estimate_skew` ; `null` pour une photo en perspective),
  `quart_de_tour` (sens horaire), `a_verifier` (inclinaison de plus de 15°),
  `perspective`, `courbure_px` ;
- `coins_page_px` : coins de la page droite (haut gauche, haut droit, bas
  droit, bas gauche) dans l'image, en pixels. L'homographie qu'ils définissent
  ramène tout point de la page droite dans l'image (exacte pour une
  inclinaison et les quarts de tour, approchée de la courbure pour la photo) ;
- `papier`, `couvercle`, `eclairage`, `flou_px`, `bruit`, `jpeg`,
  `niveaux_de_gris`, `transparence_verso` : dégradations appliquées ;
- `zones_fond` : carrés de papier vierge (aucun contenu à moins de 8 pt) ;
- `aplats` : `id`, `rect`, `couleur` d'origine (imprimée), `nuances`
  (amplitude des taches), `dominante_scan` (couleur la plus présente dans le
  scan, hors du texte posé dessus : pic d'histogramme) ;
- `photos`, `degrades` (`de`, `a`) : zones à ne jamais aplatir ;
- `filets` : `rect`, `epaisseur_pt`, `couleur` ;
- `lignes` : `texte`, `rect`, `corps_pt`, `police`, `couleur`, `fond`
  (`papier` ou l'`id` de l'aplat sous la ligne).

Les rectangles sont en points sur la page droite, origine en haut à gauche.
Le test `engine/ffi/tests/amelioration_jeu.rs` vérifie que chaque scan s'ouvre,
est vu comme une page scannée (D58), a la taille annoncée, et que ses lignes
tombent sur de l'encre et ses zones de fond sur du papier vierge.

### Mesure de départ

Inclinaison mesurée par le code actuel (`ocr::preprocess::estimate_skew`,
recherche entre −10° et 10°) sur l'image rendue à sa résolution, le
24 octobre 2026 dans le cloud :

| Page | Vraie | Mesurée |
|---|---:|---:|
| 01 | 2,30° | 2,32° |
| 02 | −7,50° | −7,50° |
| 03 | 18,00° | −8,52° (hors de la plage) |
| 04 | 0,05° | 0,00° |
| 05 | 0,60° | 0,62° |
| 06 | −0,80° | −0,80° |
| 07 | 1,20° | 1,24° |
| 08 | −1,60° | −1,60° |
| 09 | 0,90° | 0,94° |
| 10 | 1,10° | 1,12° |
| 11, page 1 (à l'envers) | 0,00° | 0,00° |
| 11, page 2 (couchée) | 0,80° | reconnue couchée par `is_sideways` |
| 12 (perspective) | — | 9,46° (sans objet) |

L'estimateur existant est donc à moins de 0,05° de l'angle vrai dans sa
plage ; le jalon 2 doit l'étendre à ±15° et reconnaître le cas hors plafond.

### Limites du jeu

- Les scans sont synthétiques : les polices (Carlito, Caladea), la
  photographie (paysage procédural) et les nuances des aplats sont générées.
  Les documents réels de `tests/fixtures/private/` complètent les mesures,
  sans être décrits ni versionnés.
- La couleur d'origine d'un aplat suppose un modèle multiplicatif (encre ×
  papier × éclairage) ; `dominante_scan` donne la couleur réellement la plus
  présente dans le scan, sans hypothèse.
- Le JPEG produit dépend de la version de libjpeg : régénérer le jeu sur une
  autre machine peut changer les octets, pas le contenu.

## Jalon 2 : redressement des pages

### Ce qui est livré

- **Mesure** (`engine/enhance`, D61). Sur l'image du scan, égalisée,
  remise d'aplomb si elle est couchée (`is_sideways`, D27) : inclinaison par
  profil de projection de l'encre des caractères (`measure_skew`), cherchée
  entre −45° et 45° pour reconnaître une page au-delà du plafond au lieu d'y
  lire un faux angle ; sa netteté relative (saillance) dit si la mesure est
  fiable. Sens par le rapport des hampes aux jambages, mesuré sur la page et
  sur la page retournée ; s'il ne tranche pas, Vision lit la page dans les
  deux sens, et la plus sûre l'emporte ; sinon, la page garde son sens.
- **Garde-fous.** Aucune correction sous 0,1° ; au-delà de 15°, la page
  n'est pas redressée et elle est signalée « à vérifier » ; une mesure peu
  fiable (saillance inférieure à 4, ou moins de 500 points d'encre) ne
  corrige rien. Une page qui n'est pas un scan pleine page (image couvrant
  moins de 90 % de la page), ou dont le contenu a été modifié en Édition,
  reste telle quelle.
- **Correction** (D62). Rotation bicubique (noyau de Keys), de même taille
  que l'image ; les coins découverts prennent la couleur du papier à cet
  endroit (estimée par blocs, et le long de chaque bord, sans la bande du
  couvercle du scanner). Image enregistrée en JPEG de qualité 90 (D63).
- **Page améliorée** (D64). Les pages traitées remplacent les pages
  d'origine en une étape d'annulation (« l'amélioration des pages ») ; la
  rotation d'un quart ou d'un demi-tour passe par la rotation de la page,
  sans réécrire l'image. À l'enregistrement, l'objet de la page d'origine
  est gardé (signets, liens, annotations, champs) et seul son contenu est
  remplacé ; l'image d'origine n'est pas conservée dans le fichier.
  « Rétablir l'original » rend les pages d'origine (annulable aussi), et
  Comparer (⌥⌘K) les montre le temps de la comparaison.
- **Interface** (D65). Mode Amélioration (⌘4, quatrième segment de la barre
  d'outils) : aperçu de la page affichée avec curseur avant/après (les deux
  images rendues par le moteur, dans le sens final), panneau à droite
  (redressement, orientation, pages traitées : toutes, sélection ou page
  affichée ; réglages avancés repliés : seuil, plafond, qualité), bilan de
  la page, bouton Appliquer, bandeau d'avancement avec arrêt, liste des
  pages à vérifier. Les réglages sont retenus en JSON.
- **Banc d'essai** : `engine/tools/enhance_bench` (`cargo run --release -p
  enhance_bench`), qui produit le tableau ci-dessous.

### Mesures (24 octobre 2026, dans le cloud, mode release)

Réglages par défaut. Écart : angle corrigé moins angle vrai. Résidu :
inclinaison mesurée de nouveau sur le rendu de la page redressée, mise à
l'endroit. Sens : quarts de tour trouvés (attendus).

| Fichier | Vérité | Corrigée | Écart | Résidu | Sens | Conclusion | Durée |
|---|---:|---:|---:|---:|---:|---|---:|
| 01 | +2,30° | +2,32° | +0,02° | 0,00° | 0 (0) | corrigée | 1,0 s |
| 02 | −7,50° | −7,52° | −0,02° | +0,08° | 0 (0) | corrigée | 0,6 s |
| 03 | +18,00° | 0 (mesurée +17,98°) | — | — | 0 (0) | à vérifier | 0,4 s |
| 04 | +0,05° | 0 | — | — | 0 (0) | droite | 0,5 s |
| 05 | +0,60° | +0,60° | 0,00° | 0,00° | 0 (0) | corrigée | 0,5 s |
| 06 | −0,80° | −0,80° | 0,00° | 0,00° | 0 (0) | corrigée | 0,5 s |
| 07 | +1,20° | +1,22° | +0,02° | +0,02° | 0 (0) | corrigée | 0,6 s |
| 08 | −1,60° | −1,58° | +0,02° | 0,00° | 0 (0) | corrigée | 0,5 s |
| 09 | +0,90° | +0,90° | 0,00° | −0,06° | 0 (0) | corrigée | 0,6 s |
| 10 | +1,10° | +1,10° | 0,00° | −0,02° | 0 (0) | corrigée | 0,3 s |
| 11, page 1 | 0,00° | 0 | — | — | 2 (2) | droite, retournée | 0,2 s |
| 11, page 2 | +0,80° | +0,80° | 0,00° | 0,00° | 3 (3) | corrigée, remise d'aplomb | 0,4 s |
| 12 (photo) | — | +2,88° | — | +0,10° | 0 (0) | corrigée (perspective non traitée) | 1,0 s |

- **Critères :** écart maximal de 0,02° et résidu maximal de 0,10°
  (critère : moins de 0,2°) ; 04 et 11, page 1, sans correction (fichier
  inchangé pour 04) ; 03 marquée « à vérifier » ; aucune page hors critères.
  Le lot de 50 pages : 50 sur 50 conformes, traité page après page (le lot
  en parallèle vient au jalon 4).
- **Saillance de l'angle retenu** (netteté relative du profil) : de 22 à 51
  pour les pages de texte, 9,3 pour la photo 12 ; une recherche limitée à
  ±15° trouvait sur 03 un faux angle de 15,4° de saillance 1,6, d'où la
  recherche jusqu'à 45° et le seuil de 4.
- **Taille des fichiers** (Ko, avant → après) : 01 834 → 1 296 ; 02 390 →
  576 ; 05 444 → 710 ; 06 416 → 808 ; 07 517 → 769 ; 08 469 → 706 ; 09 502 →
  699 ; 10 148 → 519 ; 11 745 → 957 ; 12 1 028 → 1 415 ; 03 et 04 inchangés.
  L'image réencodée en qualité 90 pèse de 1,3 à 1,9 fois l'originale, 3,5
  fois pour 10 (scan d'origine en qualité 35, dont le bruit est gardé). Le
  nettoyage du fond (jalon 3), qui rend le papier uniforme, devrait réduire
  nettement ces tailles ; à mesurer alors.
- **Durée :** sur le MacBook Air M3 de Sylvain (même banc, même jour),
  190 ms par page en moyenne (de 92 ms pour 10 à 377 ms pour 01, à 300 ppp)
  et 6,3 s pour le lot de 50 pages ; mêmes angles et mêmes tailles qu'au
  cloud (0,25 à 1 s par page, 20 s pour le lot).

### Perspective (option facultative)

Non réalisée dans ce jalon. Sur la photo 12, le redressement corrige la
direction moyenne des lignes (2,88°, résidu de 0,10°), mais les lignes
restent convergentes et courbes. Une correction de perspective demanderait de
trouver les bords de la feuille sur le fond (bureau) puis de déformer l'image
(homographie, voire courbure) ; le jeu ne contient qu'une photo pour en
démontrer le gain. Elle reste à faire si Sylvain le souhaite.
