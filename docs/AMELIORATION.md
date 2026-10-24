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
