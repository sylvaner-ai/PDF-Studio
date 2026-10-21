# Limites connues

Ce document recense honnêtement ce qui ne fonctionne pas, ou pas encore. Il sera
complété à chaque jalon, en particulier pour l'appariement de polices et
l'*inpainting* des scans (jalon 7).

## Jalon 1

- **Affichage :** ajustement à la largeur uniquement ; zoom, vignettes, recherche,
  signets et rotation d'affichage arrivent au jalon 2.
- **Rendu non interruptible :** une page dont le rendu a commencé va jusqu'au bout,
  même si elle sort de l'écran (le résultat est alors ignoré). Sans conséquence
  visible à 150–300 ppp (10 à 50 ms par page) ; à revoir pour les zooms extrêmes.
- **Zoom extrême :** une page n'est pas rendue au-delà de 16 384 pixels de côté ; un
  rendu par tuiles sera nécessaire pour les très forts grossissements (jalon 2).
- **Polices standard non intégrées** (Helvetica, Times, Courier et leurs variantes,
  quand le PDF ne contient pas le fichier de police) : MuPDF prend la police de
  macOS pour les graisses normales mais ses substituts URW pour le gras et
  l'italique, et le tiret cadratin « — » est dessiné trop court. Correction prévue
  au jalon 2. Les PDF à polices intégrées, de loin les plus courants, ne sont pas
  concernés.
- **Scans lourds :** le rendu d'un scan riche en image peut prendre 35 ms à
  150 ppp (environ 140 ms en plein écran Retina), contre 6 ms avec PDFium. La page
  apparaît alors avec un court délai ; analyse prévue au jalon 2.
- **Fichiers endommagés :** MuPDF répare ce qu'il peut ; un fichier tronqué peut
  s'ouvrir avec des pages incomplètes plutôt que d'être refusé.
