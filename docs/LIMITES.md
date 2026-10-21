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
- **Polices non intégrées :** une police absente du PDF et du Mac est remplacée par
  une police de base de MuPDF ; l'aspect peut différer de l'original.
- **Fichiers endommagés :** MuPDF répare ce qu'il peut ; un fichier tronqué peut
  s'ouvrir avec des pages incomplètes plutôt que d'être refusé.
