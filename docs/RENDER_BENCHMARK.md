# Banc d'essai du rendu : MuPDF contre PDFium

Mesures du 21 octobre 2026 sur le Mac de développement (MacBook Air M3, 16 Go,
macOS 27.0.1), avec `make bench` : rendu à 150 ppp, compilation Release, un seul
fil. PDFium : binaire `bblanchon/pdfium-binaries` (dernière version), piloté par
`pdfium-render` 0.9. L'« écart moyen » est la différence moyenne par canal entre
les deux rendus (sur 255) ; les « pixels divergents » ont au moins un canal qui
diffère de plus de 40.

## Résultat

**MuPDF est conservé** (décision D2). Les deux moteurs produisent la même
géométrie sur tous les documents et sont très au-dessous des objectifs de
performance. PDFium rend environ 2,5 fois plus vite les pages riches en images et
utilise des polices système plus cohérentes pour les polices standard non
intégrées ; MuPDF répare un fichier endommagé que PDFium refuse, conserve mieux le
détail des scans et offre les fonctions dont les jalons 3 à 7 ont besoin.

## Jeu de test public (`tests/fixtures`)

| Document | Pages | Ouverture MuPDF (ms) | Ouverture PDFium (ms) | Rendu MuPDF moy./max (ms) | Rendu PDFium moy./max (ms) | Écart moyen (/255) | Pixels divergents | Échecs MuPDF / PDFium |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| chiffre.pdf | 1 | 38,0 | 205,9 | 31,9 / 31,9 | 3,1 / 3,1 | 0,79 | 0,78 % | 0 / 0 |
| corrompu.pdf | 100 | 0,8 | échec | 2,6 / 5,6 | — | — | — | 0 / 1 |
| formats_mixtes.pdf | 5 | 0,2 | 0,7 | 2,4 / 4,5 | 0,9 / 1,1 | 0,15 | 0,06 % | 0 / 0 |
| multipage_100.pdf | 100 | 0,5 | 7,5 | 2,2 / 5,3 | 2,5 / 2,7 | 4,24 | 3,64 % | 0 / 0 |
| pas_un_pdf.pdf | 0 | échec | échec | — | — | — | — | 1 / 1 |
| pdf20.pdf | 1 | 0,2 | 1,6 | 7,8 / 7,8 | 2,2 / 2,2 | 0,79 | 0,78 % | 0 / 0 |
| scan_simule.pdf | 1 | 0,7 | 0,3 | 10,8 / 10,8 | 5,0 / 5,0 | 2,15 | 1,21 % | 0 / 0 |
| signets.pdf | 6 | 0,2 | 1,4 | 2,5 / 5,1 | 2,2 / 2,6 | 4,22 | 3,63 % | 0 / 0 |
| tableau.pdf | 1 | 0,3 | 0,6 | 4,2 / 4,2 | 1,3 / 1,3 | 0,61 | 0,48 % | 0 / 0 |
| texte_simple.pdf | 1 | 0,1 | 1,6 | 6,9 / 6,9 | 2,2 / 2,2 | 0,79 | 0,78 % | 0 / 0 |
| vectoriel.pdf | 1 | 0,1 | 0,4 | 11,7 / 11,7 | 6,8 / 6,8 | 0,65 | 0,24 % | 0 / 0 |
| volumineux_600.pdf | 600 | 2,2 | 39,6 | 2,1 / 5,0 | 2,1 / 2,6 | 4,24 | 3,64 % | 0 / 0 |

Rendu moyen : MuPDF 3,1 ms par page, PDFium 2,3 ms (20 premières pages de chaque
document). Le premier document ouvert par MuPDF (`chiffre.pdf`) inclut
l'initialisation du moteur et le chargement des polices.

## Documents personnels (`tests/fixtures/private`, anonymisés)

Cinq fichiers, huit documents traités : cinq scans d'une page (reçus), trois PDF
natifs aux polices intégrées (Open Sans, Calibri, polices d'entreprise).

| Document | Pages | Ouverture MuPDF (ms) | Ouverture PDFium (ms) | Rendu MuPDF moy./max (ms) | Rendu PDFium moy./max (ms) | Écart moyen (/255) | Pixels divergents | Échecs MuPDF / PDFium |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| doc-01 (scan) | 1 | 1,0 | 0,1 | 7,1 / 7,1 | 5,6 / 5,6 | 4,77 | 3,95 % | 0 / 0 |
| doc-02 (scan) | 1 | 0,3 | 0,1 | 5,7 / 5,7 | 4,6 / 4,6 | 3,33 | 2,46 % | 0 / 0 |
| doc-03 (scan) | 1 | 0,4 | 0,1 | 14,4 / 14,4 | 8,9 / 8,9 | 1,47 | 1,03 % | 0 / 0 |
| doc-04 (scan) | 1 | 0,3 | 0,1 | 5,6 / 5,6 | 4,4 / 4,4 | 4,16 | 3,30 % | 0 / 0 |
| doc-05 (natif) | 1 | 0,1 | 0,9 | 8,9 / 8,9 | 7,0 / 7,0 | 2,00 | 1,90 % | 0 / 0 |
| doc-06 (natif) | 3 | 0,3 | 14,4 | 5,8 / 8,8 | 2,8 / 4,2 | 0,83 | 0,76 % | 0 / 0 |
| doc-07 (natif) | 1 | 0,2 | 1,7 | 5,3 / 5,3 | 4,2 / 4,2 | 1,84 | 1,42 % | 0 / 0 |
| doc-08 (scan) | 3 | 0,5 | 0,1 | 33,9 / 35,0 | 6,0 / 6,1 | 1,82 | 1,15 % | 0 / 0 |

Rendu moyen : MuPDF 13,8 ms par page, PDFium 5,1 ms.

## Comparaison visuelle

Examen des images côte à côte produites dans `build/bench*` :

- **PDF natifs à polices intégrées :** géométrie, chasse et coupures identiques au
  pixel près ; MuPDF anticrénèle un peu plus doucement, PDFium donne des fûts
  légèrement plus foncés. Écart de 0,8 à 2,0 sur 255 : indiscernable à 100 %.
- **Scans :** même cadrage et même contenu. MuPDF garde des bords plus nets et le
  grain du scan (plus proche des pixels d'origine) ; PDFium lisse davantage (plus
  doux, un peu flou). Le comportement de MuPDF est préférable pour l'OCR et pour la
  retouche des scans, qui doit reproduire flou et grain (§4.3 b).
- **Polices standard non intégrées** (Helvetica, Times, Courier sans fichier de
  police dans le PDF) : c'est l'origine des écarts de 4,2 sur 255 du jeu public.
  PDFium utilise les polices de macOS ; MuPDF utilise celles de macOS pour les
  graisses normales, mais ses substituts URW pour les variantes grasses et
  italiques, et dessine un tiret cadratin trop court. Aucun des documents
  personnels n'est concerné. Correction prévue au jalon 2 (cf. `LIMITES.md`).
- **Dégradés :** léger effet de bandes chez MuPDF, dégradé plus lisse chez PDFium.

## Conclusion et suites

| Critère | MuPDF | PDFium |
|---|---|---|
| Ouverture de 100 / 600 pages | 0,5 / 2,2 ms | 7,5 / 39,6 ms |
| Rendu moyen, documents personnels | 13,8 ms | 5,1 ms |
| Fichier tronqué | réparé, 100 pages | refusé |
| Fidélité, polices intégrées | identique | identique |
| Fidélité, polices standard non intégrées | substituts mélangés | polices macOS |
| Détail des scans | conservé | lissé |
| Fonctions des jalons suivants | extraction riche, écriture, OCR intégré | plus limitées |
| Intégration | statique, un seul fichier | bibliothèque dynamique à embarquer |

Les deux points faibles de MuPDF sont traités au jalon 2 : choix explicite des
polices macOS pour les 14 polices standard, et analyse de la lenteur sur les scans
lourds (doc-08 : 34 ms par page à 150 ppp, soit environ 140 ms en plein écran
Retina, ce qui reste sous le seuil gênant puisque le rendu est asynchrone).
