# Architecture de PDF Studio

PDF Studio se compose d'un **moteur Rust**, qui détient le document et toute la
logique PDF, et d'une **application SwiftUI**, qui affiche et pilote le moteur par
des appels typés. Aucune logique PDF n'est écrite en Swift.

```
┌──────────────────────── Application SwiftUI (app/) ──────────────────────────┐
│ Views/        Design (règles visuelles communes, D43),                       │
│               DocumentWindow, DocumentView (lecteur), SidebarView,           │
│               DocumentWindowBridge (fermeture), ExportOptions, SearchField ; │
│               EditingBadge, RecognitionViews (avancement de l'OCR),          │
│               ScanRetouchPanel (réglages du texte réécrit d'un scan),        │
│               EnhancementView (mode Amélioration : aperçu avant/après) ;     │
│               Canvas/ : PageCanvasView (+Search, +Text), PageTileView,       │
│               TextSelectionView, EditingOverlayView (+Tables,                │
│               +Accessibility), InlineTextEditor ;                            │
│               Grid/ : PageGrid, PageGridCoordinator, PageGridItem (AppKit)   │
│ ViewModels/   DocumentViewModel (+Saving, +Recovery, +Inserting,             │
│               +Exporting, +Recognizing), DisplayPage, CanvasController,      │
│               SearchModel, TextSelection, PageTextLayout, OutlineTree,       │
│               PageLayout, PageRenderer,                                      │
│               FileSaver, RecoveryStore, RecoveryWriter, PageExporter,        │
│               PageEditing (+Tables, +Scans), DocumentViewModel+Comparing,    │
│               PageEnhancement (mode Amélioration)                            │
│ Recognition/  VisionTextReader (Vision), VisionTextRecognizer (pour Rust)    │
│ App/          PDFStudioApp, ViewerCommands, PageCommands et TableCommands    │
│               (menus), BundledFonts (polices fournies),                      │
│               DocumentOpener, DocumentReference (signets de sécurité)        │
│ Bridge/       EngineBridge.swift + Generated/ (liaisons UniFFI générées)     │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │ UniFFI : PdfDocument, EditSession, PageItem,
                                │          RenderedPage, OutlineItem, SearchMatch,
                                │          PageRect, PageObjectItem, ContentEdits,
                                │          ObjectEditItem, TableGridItem, TableAxis,
                                │          TextRecognizer (trait implémenté en
                                │          Swift), PageRecognition, EngineError
┌───────────────────────────────▼──────────── Moteur Rust (engine/) ───────────┐
│ ffi/        interface pour Swift (seule couche contenant du code unsafe :    │
│             macros UniFFI et deux appels système de qualité de service)      │
│ edit/       pages, modifications de leur contenu, couches de texte reconnu   │
│             et historique d'annulation (patron Command)                      │
│ ocr/        trait OcrEngine, préparation de l'image (fond, bruit,            │
│             binarisation, inclinaison, orientation), mesure de la lecture    │
│ enhance/    mode Amélioration : redressement (sens, inclinaison,             │
│             garde-fous), rotation bicubique, couleur du papier, JPEG,        │
│             réglages JSON                                                    │
│ render/     rastérisation RGBA : page entière ou zone, rotation d'affichage  │
│ layout/     géométrie de la structure : filets, quadrillages, tableaux à     │
│             filets, réorganisation des lignes et colonnes (sans dépendance)  │
│ fonts/      catalogue des polices candidates, mise en forme et rendu des     │
│             glyphes, appariement d'une police à une ligne scannée            │
│ scan/       pixels d'un scan : fond et grain, sélection de l'encre,          │
│             effacement (Telea), extraction et pose, texte assorti au scan,   │
│             filets et éléments de l'image                                    │
│ pdf_core/   ouverture, pages, métadonnées, plan, texte et recherche ;        │
│             écriture d'un PDF réorganisé ; fil dédié par document ;          │
│             choix des polices standard ; content/ : lecture du flux de       │
│             contenu, objets des pages, mise en page et réécriture ;          │
│             editing : polices, aperçu et application des modifications ;     │
│             textlayer : couche de texte invisible des pages reconnues ;      │
│             scanned/ : objets d'une page scannée, retouche, rustines ;       │
│             added : textes et images ajoutés (emprise, aperçu, écriture) ;   │
│             enhanced : pixels d'un scan, PDF des pages améliorées            │
│ (MuPDF)     analyse, rendu, extraction du texte, écriture                    │
└──────────────────────────────────────────────────────────────────────────────┘
```

Le crate `edit` a été créé au jalon 3, `ocr` au jalon 5, `layout` au jalon 6
(D31), `fonts` et `scan` au jalon 7. Au jalon 4, la mise en page du texte modifié et le choix de sa police sont restés
dans `pdf_core` (`content::layout`, `editing`), car ils reposent sur
l'interprétation du flux de contenu et sur MuPDF.

## Modèle d'exécution

- **Un fil par document.** MuPDF interdit de manipuler un document depuis plusieurs
  fils. `pdf_core::DocumentHandle` lance donc un fil dédié qui possède le
  `Document` et exécute les requêtes une à une, transmises par canal. Pour chaque
  requête, ce fil adopte la qualité de service (QoS) du fil qui attend la
  réponse, ce qui évite les inversions de priorité signalées par macOS ; les
  appels système correspondants sont fournis par le crate `ffi`.
- **Panique isolée.** Chaque requête s'exécute sous `catch_unwind` : une panique
  du code Rust devient une erreur « panique du moteur » pour cette requête
  seule, et le fil continue de servir le document (D50). Le plan d'un document
  est vérifié avant d'être lu par MuPDF (`outline::prepare`), car un plan
  abîmé faisait planter la bibliothèque `mupdf`.
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
   Chaque fenêtre (une par document, avec son `DocumentViewModel`) est un onglet
   AppKit (D66) : `WindowTabbing` repère la fenêtre AppKit et `DocumentOpener`
   l'ajoute en dernier onglet de la fenêtre d'où vient la demande (dépôt, ⌘O,
   ⌘T, Finder) ; un écran d'accueil vide de cette fenêtre accueille le premier
   document.
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
- **Apparence.** Les pages restent blanches en mode sombre ; seule leur ombre
  s'adapte au fond du canevas (D46). Une page rendue pour la première fois se
  dévoile en fondu ; les panneaux posés sur les pages et le passage à la grille
  suivent les animations de `Design.Motion` (D45).
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
  Côté application, la recherche s'ouvre par ⌘F ou le menu Édition dans un
  panneau posé en haut à droite des pages (`SearchPanel`), et non dans la barre
  d'outils (décision D24).
- **Sélection du texte.** En mode Lecture, le texte des pages se sélectionne à la
  souris et se copie (⌘C), qu'il soit natif ou reconnu par l'OCR, enregistré ou non
  (D57). Le moteur donne, page par page (`selectable_text`), les lignes et
  l'emprise de chaque caractère, sur la page droite, depuis le même texte que la
  recherche (`PageText::glyph_lines`) : texte natif, invisible compris, suivi de la
  couche reconnue en session. `PageTextLayout` met ces caractères bout à bout et
  retrouve le caractère visé (ligne dont la bande contient le pointeur, puis
  caractère le plus proche le long d'elle), les mots, les lignes, les rectangles à
  surligner et le texte à copier. `TextSelection` tient la sélection de tout le
  document (positions suivant l'identifiant de la page), lit le texte des pages
  quand elles apparaissent et l'abandonne si la page est modifiée ou reconnue.
  `TextSelectionView`, posée sur chaque page, surligne, change le pointeur en barre
  de texte et transmet la souris au canevas. La copie est la commande Copier de
  l'application (`PagesDocumentView.copy`).
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

## Édition du contenu

- **Objets.** `pdf_core::content` analyse le flux de contenu d'une page
  (`lexer`), l'interprète en suivant l'état graphique et les polices (`interp`,
  `resources`, `font`, `cmap`, `encodings`) et en tire les objets de la page
  (`objects`) : blocs de texte, images, tracés, formulaires, dégradés. `editing`
  les présente à l'application (`PageObjectItem`, emprise sur la page droite).
- **Modifications.** Supprimer, déplacer ou remplacer le texte d'un objet passe
  par l'`EditSession`, qui les range dans l'état du crate `edit`, avec les pages
  (décision D21). Chaque page modifiée porte un numéro d'état (`edit_revision`),
  qui change à chaque modification ou annulation.
- **Aperçu et écriture.** `editing::edit_page` met en page le nouveau texte
  (`layout`), choisit sa police (D20) et réécrit le flux (`rewrite`, D19). Pour
  l'aperçu, le fil du document substitue un instant le contenu modifié à celui de
  la page, en tire la liste d'affichage puis rétablit l'original ; à
  l'enregistrement, le contenu modifié remplace celui de la page dans la copie
  écrite. Le cache d'images de l'application est indexé aussi par l'état des
  modifications.
- **Interface.** `PageEditing` garde la sélection et les objets connus de chaque
  page ; `EditingOverlayView` dessine la sélection sur chaque page du canevas et
  traduit souris et clavier en modifications (D23), toujours exprimées sur la page
  droite, rotation d'affichage et quarts de tour de l'organisation retirés.

## Tableaux

- **Détection.** À la construction des objets d'une page, `objects::tables`
  donne au crate `layout` les sous-chemins rectilignes des tracés (relevés par
  `interp`, dans l'espace de la page) et les glyphes ; `layout::detect` en tire
  les filets, puis les quadrillages et les tableaux à filets horizontaux (D32).
  Les lignes de texte sont coupées aux limites des cellules et les blocs
  regroupés par cellule (D33). Chaque tableau devient un objet `Table`, numéroté
  après les autres, dont les blocs, filets et fonds sont les membres (D35).
- **Modifications.** `TableEdit` (crate `edit`) range, sur l'objet Tableau, les
  lignes et colonnes d'origine conservées dans leur ordre et les textes saisis
  dans des cellules vides. `editing` les traduit objet par objet
  (`PageModel::derive`) : déplacement ou suppression des blocs et des images
  (`layout::Restructure`, D34), déformation des tracés sous-chemin par
  sous-chemin (`rewrite::Reshape`), texte des cellules vides mis en page dans
  le style d'un bloc voisin (`rewrite::Insertion`, D36). La grille modifiée est
  rendue à l'application (`TableGridItem`).
- **Interface.** L'`EditSession` supprime ou déplace des lignes et des colonnes
  (`delete_table_bands`, `move_table_band`, numéros actuels) et saisit le
  texte d'une cellule (`set_cell_text`). `PageEditing+Tables` garde la cellule
  courante ; `EditingOverlayView+Tables` dessine la grille, désigne la cellule
  et propose le menu contextuel ; `TableCommands` donne le menu Tableau.

## Retouche des pages scannées

- **Objets d'un scan.** Une fois la page reconnue (couche de texte), et si
  elle n'a pas de texte natif mais une image qui en couvre au moins la moitié,
  `pdf_core::scanned` rend l'image dans un espace de pixels sans rotation, à sa
  résolution native (D38), et construit un `ScanModel` : blocs de texte
  (lignes puis paragraphes ou listes, regroupés par `scanned::blocks` sur
  l'encre de la page redressée, D59), tableaux (filets de l'image redressés, `layout::detect`) et
  éléments (encre hors des mots et des filets) (D39). Ils remplacent l'image
  parmi les objets de la page, avec des numéros marqués `SCAN_ID`.
- **Modifications.** Elles sont rangées comme les autres (`ObjectEdit` :
  suppression, déplacement), plus `ScanText` pour un bloc réécrit : texte,
  réglages actuels et mesurés, polices candidates. `scanned::retouch` calcule
  les rustines en deux temps : réécriture des blocs (`scanned::reflow` : mots
  gardés déplacés avec leur encre, nouveaux mots dessinés, paragraphe remis à
  la ligne, D59), puis suppressions et
  déplacements (sélection de l'encre, extraction, effacement par `scan::inpaint`,
  pose décalée). La couche de texte est mise à jour en conséquence.
- **Police et rendu.** `ScanModel::match_style` mesure la ligne
  (`scan::line::observe` : ligne de base, corps, couleur, grain) et demande à
  `fonts::match_line` les trois polices les plus proches (D41) ; `scan::draw`
  rend le texte avec `rustybuzz` et `ab_glyph_rasterizer`, puis le floute et le
  bruite comme le scan.
- **Aperçu et enregistrement.** L'aperçu ajoute les rustines à la liste
  d'affichage de la page ; l'enregistrement les écrit en images sans perte
  (`/PSRTn`) entre le contenu d'origine et la couche de texte (D37).
- **Interface.** L'`EditSession` réécrit une ligne (`replace_text`), change ses
  réglages (`set_scan_style`, `choose_scan_candidate`, `reset_scan_style`).
  `PageEditing+Scans` et `ScanRetouchPanel` donnent le panneau de retouche ;
  `DocumentViewModel+Comparing` montre les pages d'origine (D42).
  `BundledFonts` désigne au moteur les polices fournies (`set_font_directories`)
  et les rend disponibles à la saisie.

## Textes et images ajoutés (D67)

- **Modèle.** Un objet ajouté (`edit::AddedObject` : texte avec police, corps
  et couleur, ou image avec ses octets et son cadre) est rangé en entier dans
  les modifications de la page (`PageEdits::added`), numéroté à partir de
  `ADDED_ID` (`0x2000_0000`), à part des objets d'origine et de ceux d'un
  scan. Il se déplace, se réécrit et se supprime par les mêmes commandes que
  les autres objets (`Editor::edit_objects`) ; `add_object` et `edit_added`
  (style, cadre) passent par le même historique d'annulation. Sa position est
  dans l'espace de la page PDF ; il retient les quarts de tour de la page à
  l'écran quand il a été ajouté, pour se lire droit sur la page redressée.
- **Aperçu et enregistrement.** `pdf_core::added` met le texte en page (une
  ligne par retour voulu, interligne de 1,2 corps), choisit sa police (police
  standard non intégrée si WinAnsi suffit, sinon son clone URW ; police du Mac
  intégrée si sa licence le permet, sinon la police standard la plus proche)
  et écrit le dessin des objets dans un flux de contenu de plus, après tout le
  contenu de la page entouré de `q … Q`. L'aperçu rend ce flux à part et pose
  sa liste d'affichage sur celle de la page (rustines d'un scan comprises) ;
  l'enregistrement l'ajoute à la page écrite (`/PSAFn`, `/PSAIn`).
- **Page vierge.** Une page vierge n'a pas de document : le premier objet
  ajouté lui en donne un (`blank_page_document`, une page vierge de son format,
  ajoutée aux sources comme un document inséré), dans la même étape
  d'annulation.
- **Interface.** L'`EditSession` ajoute (`add_text`, `add_image`), restyle
  (`set_added_text_style`) et redimensionne (`set_object_bounds`).
  `PageEditing+Adding` porte l'outil « Texte », le style et l'ajout des images
  (`AddedImageFile` lit le fichier, `AddedTextFont` les polices) ;
  `EditingOverlayView+Adding` pose le texte au clic et dessine les poignées des
  images ; `AddingPalette` et `AddedTextPanel` (`AddingViews.swift`) et le menu
  Insertion (`InsertionCommands`) les commandent. La récupération range chaque
  image à part, une seule fois (`RecoveryStore.keepImage`).

## Reconnaissance du texte (OCR)

- **Moteurs interchangeables.** Le crate `ocr` définit le trait `OcrEngine`
  (une image en niveaux de gris, des langues → des mots avec leur quadrilatère,
  leur confiance et leur ligne). L'application fournit le sien : Vision, par le
  trait UniFFI `TextRecognizer`, implémenté en Swift (`VisionTextRecognizer`) et
  appelé par Rust (décision D26). Le banc d'essai (`engine/tools/ocr_bench`)
  branche sur le même trait Tesseract, `ocrs` et Vision.
- **Préparation.** `ocr::recognize` égalise le fond et étire le contraste,
  filtre éventuellement le bruit, détecte une page couchée, mesure et corrige
  l'inclinaison, binarise éventuellement, puis lit ; une lecture douteuse est
  reprise dans les autres sens (D27). Les zones d'encre d'une ligne laissées
  sans mot sont relues en un appel (`ocr::recover`, D59). Les mots reviennent dans le repère de
  l'image reçue.
- **Une page.** `EditSession.recognize_page` rend la page (contenu modifié
  compris) à 300 ppp, la fait lire, écarte les mots qui recouvrent un texte déjà
  présent dans la page, puis convertit les quadrilatères en points dans l'espace
  de la page PDF (inverse de la matrice d'affichage). Rien ne change dans le
  document : `apply_recognitions` range ensuite les couches de toutes les pages
  lues dans l'état du crate `edit`, en une étape d'annulation, et remet à
  l'endroit les pages couchées ou retournées.
- **Couche de texte.** Avant l'enregistrement, la recherche porte sur le texte
  de la page et sur sa couche (`search_page_with_layer`). À l'écriture,
  `textlayer` ajoute au flux de contenu, après le contenu d'origine entouré de
  `q … Q`, un bloc marqué `/PDFStudioOCR` : chaque mot en rendu invisible
  (`3 Tr`), placé et étiré sur son quadrilatère, dans une police Type0 sans
  dessin construite par le code (D28). Les lecteurs PDF y trouvent le texte
  (sélection, recherche, copie) sans que l'image change.
- **Interface.** L'entrée en mode Édition lance la lecture des pages
  scannées (D58), détectées par le moteur (`pages_to_recognize`) : aucun texte
  mais un contenu, ou seulement le texte invisible d'un OCR précédent posé sur
  un scan. Un PDF natif passe aussitôt en Édition. Les pages sont lues une à
  une hors du fil principal ; un bandeau montre l'avancement et permet
  d'arrêter après la page en cours. En mode Édition, les mots douteux
  (confiance inférieure à 0,5) sont surlignés.
- **Couche remplacée.** Sur une page dont le seul texte était invisible, la
  nouvelle couche remplace l'ancienne : la recherche et la sélection l'ignorent
  (`service::Worker::page_text_with_layer`), et l'écriture retire du contenu les
  opérations d'affichage du texte avant d'ajouter la nouvelle couche
  (`writer::remove_replaced_text`).

## Mode Amélioration

Plan et jalons : `docs/AMELIORATION.md`. Au jalon 2 :

- **Analyse d'une page.** `EditSession.enhance_page` lit les pixels du scan
  de la page (`Document::scan_pixels` : image pleine page à sa résolution,
  cadres de la page et place de l'image), puis `enhance::analyze` en mesure
  le sens et l'inclinaison (D61), avec Vision en repli pour le sens. Si
  l'inclinaison est corrigée, l'image est tournée (`enhance::rotate`, coins
  à la couleur du papier, D62) et encodée en JPEG (D63). Le résultat
  (`EnhancedPage`) se rend avant et après pour l'aperçu, sans rien changer au
  document.
- **Application.** `enhanced_document` écrit les images retenues dans un PDF
  d'une page par image (`pdf_core::write_enhanced_pages`), que l'application
  ajoute aux sources de la session ; `apply_enhancements` fait pointer les
  pages vers ce document et retient leur page d'origine
  (`edit::Editor::enhance`), en une étape d'annulation ;
  `restore_original_pages` les y ramène.
- **Écriture.** Une page améliorée s'écrit `OutputPage::Replaced` : l'objet
  de la page d'origine reste (annotations, liens, signets), son contenu et
  ses ressources sont greffés depuis la page améliorée (D64).
- **Interface.** `PageEnhancement` (modèle) et `EnhancementView` (aperçu à
  curseur, panneau) ; le traitement suit le modèle de la reconnaissance du
  texte (pages figées, bandeau d'avancement, arrêt) (D65).

## Enregistrement, récupération et exportations

- **Écriture.** `EditSession.export_pdf` produit les octets du nouveau PDF à partir
  des octets lus à l'ouverture (décision D12) ; `FileSaver` les écrit après
  confirmation et copie de sauvegarde, puis relit le résultat (D13). Pendant la
  réécriture, une note dans `Sauvegardes/Écritures en cours` permet de rétablir
  le fichier à sa réouverture si l'écriture a été interrompue (D52).
- **Récupération.** `RecoveryStore` retient, à chaque modification non enregistrée,
  l'organisation des pages, les modifications de leur contenu (D22), le texte
  reconnu des pages scannées et des copies des documents dont elles viennent
  (D14).
  Les copies orphelines sont supprimées au lancement. Depuis le jalon 9, ces
  écritures passent par `RecoveryWriter`, une file série hors du fil principal
  qui regroupe les demandes rapprochées (au plus 0,5 s d'attente) et relit
  seulement les pages modifiées depuis l'écriture précédente (D51) ; quitter
  l'application écrit aussitôt ce qui attend.
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

- `make test-engine` : tests Rust (`cargo test`) sur `tests/fixtures/`, dont
  `engine/ffi/tests/robustesse.rs` : les PDF du jeu de test, abîmés de façon
  déterministe, parcourus par l'interface du moteur sans panique ni blocage
  (D54) ; `PDFSTUDIO_ROBUSTESSE=25` multiplie les tirages.
- `make ocr-bench` : banc d'essai des moteurs OCR (`docs/OCR_BENCHMARK.md`).
- `make test-app` : tests XCTest du pont, du rendu, de la recherche, de la
  géométrie d'affichage, de l'organisation, de l'enregistrement, de la
  récupération, des exportations, de l'édition du contenu et de la
  reconnaissance du texte par Vision, hébergés dans l'application ; depuis le
  jalon 9, aussi l'arrêt imprévu et l'enregistrement interrompu
  (`CrashRecoveryTests`) et l'absence d'autorisation réseau (`PrivacyTests`).
- `make perf` : mesures de performance du §6 dans l'application compilée avec
  optimisations (`PerformanceTests`, ignoré par `make test-app`) ; rapport dans
  `~/Library/Caches/PDFStudio/perf/rapport.md` (D54) ; résultats dans
  `docs/PERFORMANCES.md`.
- `tests/fixtures/generate.py` régénère le jeu de PDF publics, dont
  `organisation.pdf` (signets, liens, champs, note, numérotation et métadonnées à
  préserver) et `signature_factice.pdf` (champ de signature) pour le jalon 3,
  `edition.pdf` (titre en police intégrée, paragraphe justifié, ligne en
  Helvetica, image, tracés, page tournée) pour le jalon 4, `ocr/` (lettre,
  facture, colonnes, tableau dense, petits caractères, texte anglais) pour le
  jalon 5, `tableaux.pdf` (cellules fusionnées, cellules vides, fonds de
  cellule, tableau à filets horizontaux) pour le jalon 6, `amelioration/`
  (douze scans dégradés et leur vérité terrain en `.json`, lot de 50 pages) pour
  le mode Amélioration (D60, `docs/AMELIORATION.md`), vérifié par
  `engine/ffi/tests/amelioration_jeu.rs` ;
  `tests/fixtures/private/` reçoit les documents personnels, jamais versionnés.
- `make snapshots` : captures d'écran de l'interface en mode clair et sombre
  (`DesignSnapshotTests`, ignoré par `make test-app`), copiées dans
  `~/Library/Caches/PDFStudio/snapshots` (D49).

## Icône

`app/Resources/AppIcon.svg` est la source ; `make icon` (`tools/icon/render_icon.py`)
produit le jeu d'icônes `app/Resources/Assets.xcassets/AppIcon.appiconset`, versionné.
Le SVG n'est pas copié dans l'application (`project.yml`).
