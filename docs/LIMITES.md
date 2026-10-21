# Limites connues

Ce document recense honnêtement ce qui ne fonctionne pas, ou pas encore. Il sera
complété à chaque jalon, en particulier pour l'appariement de polices et
l'*inpainting* des scans (jalon 7).

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
  résultat. La recherche dans les scans viendra avec l'OCR (jalon 5).
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
  (jalon 3).
- **Menu Édition :** les commandes de recherche remplacent le groupe « Édition de
  texte » standard (⌘F, ⌘G, ⇧⌘G) ; d'éventuelles entrées système de ce groupe,
  comme l'orthographe, n'apparaissent pas dans le menu.
