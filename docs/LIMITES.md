# Limites connues

Ce document recense honnêtement ce qui ne fonctionne pas, ou pas encore. Il est
complété à chaque jalon.

## Jalon 1

- **Affichage :** ajustement à la largeur uniquement ; zoom, vignettes, recherche,
  signets et rotation d'affichage arrivent au jalon 2. *Résolu au jalon 2.*
- **Rendu non interruptible :** une page dont le rendu a commencé va jusqu'au bout,
  même si elle sort de l'écran (le résultat est alors ignoré). Sans conséquence
  visible à 150–300 ppp (10 à 50 ms par page) ; à revoir pour les zooms extrêmes.
- **Zoom extrême :** une page n'est pas rendue au-delà de 16 384 pixels de côté ; un
  rendu par tuiles sera nécessaire pour les très forts grossissements (jalon 2).
  *Résolu au jalon 2 : tuiles de détail au-delà de 4 096 pixels.*
- **Polices standard non intégrées** (Helvetica, Times, Courier et leurs variantes,
  quand le PDF ne contient pas le fichier de police) : MuPDF prend la police de
  macOS pour les graisses normales mais ses substituts URW pour le gras et
  l'italique, et le tiret cadratin « — » est dessiné trop court. Correction prévue
  au jalon 2. Les PDF à polices intégrées, de loin les plus courants, ne sont pas
  concernés. *Résolu au jalon 2 : les quatorze polices standard sont toujours les
  clones URW (décision D8).*
- **Scans lourds :** le rendu d'un scan riche en image peut prendre 32 ms à
  150 ppp (environ 130 ms en plein écran Retina), contre 6 ms avec PDFium. La page
  apparaît alors avec un court délai ; analyse prévue au jalon 2.
- **Fichiers endommagés :** MuPDF répare ce qu'il peut ; un fichier tronqué peut
  s'ouvrir avec des pages incomplètes plutôt que d'être refusé.

## Jalon 2

- **Polices standard :** les clones URW reproduisent les chasses d'Adobe, donc la
  mise en page est exacte, mais leur dessin n'est pas celui des polices de macOS.
  Nimbus Mono (Courier) en particulier paraît plus gras que le Courier du Mac.
- **Recherche dans les scans :** une page sans couche de texte ne donne aucun
  résultat. La recherche dans les scans viendra avec l'OCR (jalon 5). *Résolu au
  jalon 5 : le texte des pages reconnues est cherché, avant comme après
  l'enregistrement.*
- **Mots coupés en fin de ligne :** « effica-⏎cité » est retrouvé par
  « efficacité », mais un mot composé coupé sur son trait d'union
  (« porte-⏎monnaie ») devient « portemonnaie » : il faut alors chercher
  « portemonnaie » ou « monnaie ».
- **Recherche littérale :** pas d'expressions régulières, d'option « mot entier »
  ni de respect de la casse. Une recherche qui inclut des guillemets français doit
  reproduire les espaces du document ; le plus simple est de les omettre.
- **Scans lourds :** à fort grossissement, chaque tuile de détail redécode la
  partie de l'image qu'elle couvre ; le premier affichage net d'une zone peut
  prendre plusieurs dixièmes de seconde sur un scan JPEG de grande taille.
  L'image de la page entière, elle, est mise en cache. Un décodeur JPEG plus
  rapide (libjpeg-turbo) reste une piste si la gêne est réelle.
- **Navigation :** pas encore de boîte « Aller à la page… » ; on passe par les
  vignettes, le menu Aller ou le clavier. Le mode page par page est simple : pas de
  pages en vis-à-vis ni de couverture séparée.
- **Rotation d'affichage :** elle s'applique à tout le document et n'est pas
  enregistrée ; la rotation enregistrée page par page relève de l'organiseur
  (jalon 3). *Résolu au jalon 3 : ⌘L et ⌘R font pivoter les pages sélectionnées.*
- **Barre d'outils :** constaté sur un document d'une page dans une fenêtre de
  920 pt, un chevron de débordement (») apparaît à droite de la barre d'outils
  et le bouton de la barre latérale n'est pas visible. Cause à confirmer ; la barre
  latérale reste accessible par le menu Présentation (⌃⌘S).
- **Menu Édition :** les commandes de recherche remplacent le groupe « Édition de
  texte » standard (⌘F, ⌘G, ⇧⌘G) ; d'éventuelles entrées système de ce groupe,
  comme l'orthographe, n'apparaissent pas dans le menu.

## Jalon 3

- **Pages insérées depuis un autre document :** leurs annotations visibles sont
  reprises, mais pas leurs champs de formulaire, leurs liens internes ni les
  signets de leur document d'origine.
- **Numérotation des pages** (« i, ii, 1, 2… », `/PageLabels`) : elle est abandonnée
  dès que l'ordre des pages change, plutôt que de devenir fausse.
- **Destinations nommées :** une destination nommée qui visait une page supprimée
  reste dans le fichier, sans la page ; elle ne mène plus nulle part.
- **Balisage d'accessibilité :** supprimer des pages retire la structure balisée
  du document (`/StructTreeRoot`), qui renvoie au contenu des pages ; les lecteurs
  d'écran lisent alors le document sans ses balises.
- **Signature numérique :** tout enregistrement la rend invalide (PDF Studio prévient
  avant). Signer de nouveau relève d'un autre logiciel.
- **Récupération :** seule l'organisation des pages est retenue (*le contenu
  modifié l'est aussi depuis le jalon 4*). Si le fichier est
  modifié par une autre application avant tout enregistrement dans PDF Studio, les
  modifications retenues sont abandonnées à la réouverture. Insérer des pages d'un
  PDF protégé par un mot de passe rend la récupération impossible pour ce document,
  car le mot de passe n'est jamais écrit ; l'alerte de fin le signale. L'empreinte
  d'un fichier se limite à sa taille et à sa date de modification.
- **Glisser-déposer :** les pages se copient d'une fenêtre à l'autre depuis la grille
  d'organisation seulement, pas depuis la barre latérale ; on ne peut pas encore
  les déposer dans le Finder pour créer un fichier.
- **Retour de la grille :** la lecture reprend sur la page lue, mais avec
  l'ajustement automatique et en haut de la page ; le zoom choisi et la position
  dans la page ne sont pas conservés.
- **Fusion :** il n'y a pas de commande dédiée. On ouvre le premier document, on y
  insère les autres (⌥⌘I, plusieurs à la fois, ou par dépôt sur la grille), puis on
  choisit Enregistrer sous.
- **Export du texte :** seul le texte natif est exporté, sans OCR, dans l'ordre de
  lecture établi par MuPDF.
- **Export en images :** 10 000 pixels de côté au plus, pour une affiche exportée à
  300 ppp.
- **⌘Z dans le champ de recherche :** quand ce champ a le focus, ⌘Z annule la saisie
  et non la dernière modification des pages.
- **Développement :** dans le profil de débogage de Cargo, MuPDF est compilé avec ses
  assertions. L'une d'elles (`pdf_load_hail_mary_font`) peut se déclencher quand des
  tests parallèles chargent un formulaire sans police par défaut. La version Release
  n'est pas concernée, et le jeu de test fournit cette police.

## Jalon 4

- **Objets verrouillés :** le texte vertical, le texte dans une police que le moteur
  ne sait pas mesurer (codages CJK prédéfinis, par exemple) et le texte qui sert de
  détourage ne peuvent être ni déplacés ni modifiés ; le bandeau l'indique. Un
  formulaire PDF (contenu réutilisable) se déplace ou se supprime d'un bloc, sans
  accès à ses éléments.
- **Texte modifié :** il reprend la police, le corps, la couleur et l'alignement du
  début du bloc ; les mélanges de styles dans un même bloc (un mot en gras, un
  lien coloré) sont perdus, de même que le crénage d'origine. Un texte plus long
  que l'original déborde du bloc vers le bas sans repousser ce qui suit ; le
  bandeau le signale.
- **Blocs de texte :** ils sont devinés d'après la position et le style des
  lignes. Un mot en gras ou en couleur au milieu d'une phrase reste dans son
  paragraphe (le bloc n'est donc pas strictement homogène) et prend le style du
  bloc s'il est retapé. Un paragraphe de deux lignes courtes est traité comme
  deux lignes distinctes.
- **Polices :** une police installée est intégrée entière, sans réduction aux
  caractères utilisés (quelques centaines de kilo-octets à quelques
  méga-octets). Une police qui interdit l'intégration, ou de plus de 16 Mo, est
  remplacée par une police standard non intégrée. Selon les règles du bac à sable
  de macOS, les polices installées dans la bibliothèque personnelle
  (`~/Library/Fonts`) peuvent être illisibles : la police standard prend alors le
  relais.
- **Recherche :** elle porte sur le texte d'origine des pages tant que le document
  n'est pas enregistré puis rouvert.
- **Scans :** le texte invisible issu d'une reconnaissance de caractères n'est pas
  un objet modifiable ; supprimer ou déplacer l'image d'un scan laisse ce texte en
  place.
- **Saisie du texte :** elle se fait à l'horizontale, même sur une page tournée, et
  dans la police du bloc seulement si elle est installée sur le Mac (sinon la police
  du système) ; le rendu exact apparaît une fois la saisie validée. Pendant un
  glisser, seul le contour des objets suit le pointeur.
- **Historique :** chaque appui sur une flèche compte pour une modification à
  annuler.

## Jalon 5

- **Pages retournées ou couchées :** Vision lit le texte dans tous les sens
  avec la même confiance, donc la page n'est pas relue ni remise à l'endroit
  (0 page retournée sur 15, 6 couchées sur 15) ; le texte reconnu est juste et
  cherchable, mais la page reste à l'envers à l'écran. Tesseract les remet à
  l'endroit (15 sur 15 et 13 sur 15).
- **Petit texte sur une page presque vide :** Vision peut ne pas lire un pied de
  page minuscule sur une page presque blanche (`encadre.pdf` : 53 à 59 %).
- **Mesures :** deux des cinq documents privés de test ont une référence
  inexploitable (texte natif sans Unicode, ou texte invisible) : leur taux n'est
  pas significatif.
- **Sélection du texte dans l'application :** le mode Lecture affiche les pages
  en images et ne permet pas de sélectionner ni de copier du texte, natif ou
  reconnu. Le texte reconnu se sélectionne dans les autres lecteurs (Aperçu,
  Acrobat) une fois le document enregistré.
- **Mots douteux :** sur un scan propre, Vision donne une confiance d'au moins
  0,5 à presque tous les mots : aucun n'est alors signalé. Le surlignage
  n'apparaît que sur les scans de moindre qualité.
- **Confiance perdue à l'enregistrement :** la couche de texte enregistrée ne
  garde pas la confiance des mots. Après réouverture, les mots douteux ne sont
  plus signalés, et la couche n'est pas modifiable en mode Édition (le texte
  invisible n'est pas un objet de la page).
- **Nouvelle reconnaissance d'une page déjà enregistrée avec sa couche :** le
  texte invisible compte comme un texte de la page ; les mots relus qui le
  recouvrent sont écartés, la page n'est donc pas relue. Il n'existe pas encore
  de commande pour retirer une couche enregistrée.
- **Arrêt :** la page en cours de lecture va jusqu'au bout (quelques secondes au
  plus) ; les pages restent figées jusque-là.
- **Image non redressée :** l'inclinaison est corrigée pour la lecture
  seulement ; l'image de la page reste inclinée, et les mots invisibles suivent
  ses lignes. Une page couchée ou retournée est remise à l'endroit par sa
  rotation, pas par une nouvelle image.
- **Langues :** français et anglais seulement ; une autre langue en alphabet
  latin est lue, mais sans correction linguistique adaptée.
- **Confiance de Vision :** Vision ne donne qu'une confiance par ligne ; celle
  des mots en est déduite (D30). Un mot faux sur une ligne sûre peut ne pas être
  signalé.
- **Tableaux et colonnes :** l'ordre de lecture est celui que donne le moteur ;
  la recherche d'une expression à cheval sur deux cellules ou deux colonnes peut
  échouer.
- **Écriture manuscrite :** non mesurée ; Vision en lit une partie, sans
  garantie.
- **Banc d'essai :** les mesures à texte de référence portent sur des scans
  simulés (pages rendues puis dégradées comme par un scanner) ; les vrais scans
  sont mesurés sans référence (nombre de mots, confiance, temps).

## Jalon 6

- **Tableaux sans filet :** un tableau fait seulement d'alignements (aucun
  filet tracé) n'est pas détecté ; son texte reste en blocs (D32).
- **Tableaux à filets horizontaux seuls :** les limites des colonnes sont
  déduites du texte ; la grille montrée (et l'alignement deviné d'un intitulé
  d'en-tête) peut différer de celle qu'avait l'auteur, sans effet sur le rendu.
- **Scans :** depuis le jalon 7, les tableaux d'une page scannée sont retrouvés,
  mais leurs lignes et colonnes ne se réorganisent pas (voir ci-dessous).
- **Lignes qui ne grandissent pas :** un texte plus long que sa cellule est
  remis à la ligne dans la cellule et déborde vers le bas sans repousser les
  lignes suivantes ; le bandeau le signale. Une ligne supprimée resserre le
  tableau, mais ce qui suit le tableau ne remonte pas.
- **Opérations absentes :** ni ajout de ligne ou de colonne, ni fusion ou
  scission de cellules, ni redimensionnement des lignes et colonnes (non
  prévus au jalon 6). Une cellule fusionnée doit rester d'un seul tenant : un
  déplacement qui la couperait est refusé, avec un message.
- **Éléments à cheval :** une image ou un tracé qui couvre plusieurs cellules
  d'une ligne supprimée suit la réorganisation point par point ou disparaît
  avec la ligne ; un texte posé sur un filet (hors cellule) n'appartient pas au
  tableau et ne bouge pas.
- **Saisie d'une cellule vide :** elle reprend le style d'une cellule voisine
  (D36) ; le texte d'une cellule déjà remplie se modifie comme un bloc, avec les
  limites du jalon 4. Une cellule dont le texte a été supprimé ne peut pas être
  remplie de nouveau (annuler la suppression).
- **Récupération :** sur une page qui contient un tableau, les blocs de texte
  sont découpés par cellule depuis ce jalon ; des modifications retenues avant
  lui sur une telle page sont ignorées à la restauration (D33).
- **Recherche :** elle porte sur le texte d'origine tant que le document n'est
  pas enregistré puis rouvert ; l'ordre de lecture d'un tableau réorganisé reste
  celui du flux de contenu (copie de texte dans un autre lecteur).


## Jalon 7

- **Ce n'est pas un caviardage :** une retouche est une rustine posée sur
  l'image d'origine (D37). Les pixels effacés restent dans le fichier et
  peuvent être extraits par un outil PDF. Ne pas retoucher un scan pour cacher
  une information confidentielle.
- **Retouche limitée à la session de reconnaissance :** une page n'est
  retouchable que si sa couche de texte a été reconnue dans la session. Une
  fois le document enregistré puis rouvert, ou pour un scan qui porte déjà le
  texte d'un autre logiciel (scanner, Acrobat), la page n'est plus reconnue
  (limite du jalon 5) et ne se retouche donc plus comme un scan. Reprendre la
  couche de texte existante est une évolution possible.
- **Pages concernées :** une page est traitée comme un scan si elle n'a aucun
  texte natif et qu'une image en couvre au moins la moitié. Une page mixte
  (scan avec un en-tête vectoriel, par exemple) n'est pas retouchable comme un
  scan ; les tracés vectoriels posés sur le scan sont recouverts par les
  rustines qui les touchent.
- **Police retrouvée :** le choix se fait parmi les polices fournies (Carlito,
  Caladea) et une liste de familles bureautiques installées sur le Mac
  (`fonts::catalog::DOCUMENT_FAMILIES`). Une police absente de cette liste
  (police d'entreprise, Calibri ou Cambria non installées) est remplacée par la
  plus proche, avec un score de ressemblance plus bas. Sur la facture de test
  (200 ppp, tests du moteur), Caladea est retrouvée avec 84 % de ressemblance,
  Carlito gras avec 91 %, Carlito avec 81 %. Sur un Mac (169 polices
  candidates, 36 familles), la ligne « Échéance » en Caladea donne Caladea
  (80 %), Charter (78 %) et American Typewriter (73 %) : la première est juste,
  mais les suivantes peuvent être des suggestions peu plausibles, les polices
  d'allure particulière n'étant pas pénalisées.
- **Graisse :** le nouveau texte peut paraître très légèrement plus gras que
  l'original ; le curseur « Graisse » corrige l'écart.
- **Fond texturé :** sur papier uni, l'effacement est invisible ; sur un fond
  tramé, texturé, ou sous un tampon, une trace légère peut rester visible là
  où un logo ou une signature a été effacé.
- **Texte réécrit plus long :** la fin de la ligne est décalée, sans retour à
  la ligne ; un texte qui dépasse le bord de la page ou de la cellule n'est pas
  remis à la ligne.
- **Couleurs :** les couleurs sont lues en RGB, sans profil ICC ; un scan en
  CMJN ou à profil particulier peut donner une encre légèrement différente
  (curseur « Couleur »).
- **Tableaux scannés :** ils se déplacent et se suppriment d'un bloc, et le
  texte de leurs cellules se réécrit, mais leurs lignes et colonnes ne se
  suppriment ni ne se déplacent (menu Tableau désactivé).
- **Annotations :** les annotations (commentaires, surlignages) ne suivent pas
  les objets déplacés dans le scan.
- **Écriture manuscrite :** une signature ou une écriture manuscrite se
  déplace ou s'efface comme un élément ; réécrite, elle prend une police
  d'imprimerie (l'écriture n'est pas imitée).
- **Temps de réponse :** la première modification d'une ligne cherche sa police
  parmi tout le catalogue, et la fenêtre attend la fin de la recherche :
  environ une seconde pour 169 polices, mesurée sur le test du moteur (profil de
  développement, MacBook Air) ; davantage avec plus de polices installées.
