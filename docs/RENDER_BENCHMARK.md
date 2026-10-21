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

Dix documents (5 premières pages au plus) : sept scans, dont deux portant une
couche de texte OCR invisible, et trois PDF natifs aux polices intégrées (Open
Sans, Calibri, polices d'entreprise).

| Document | Pages | Ouverture MuPDF (ms) | Ouverture PDFium (ms) | Rendu MuPDF moy./max (ms) | Rendu PDFium moy./max (ms) | Écart moyen (/255) | Pixels divergents | Échecs MuPDF / PDFium |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| doc-01 (scan + OCR) | 3 | 1,1 | 5,9 | 13,6 / 21,2 | 6,8 / 10,3 | 2,38 | 1,87 % | 0 / 0 |
| doc-02 (scan + OCR) | 2 | 0,4 | 4,9 | 20,9 / 21,5 | 12,1 / 12,5 | 3,31 | 2,47 % | 0 / 0 |
| doc-03 (scan) | 1 | 0,3 | 0,1 | 6,6 / 6,6 | 5,0 / 5,0 | 4,77 | 3,95 % | 0 / 0 |
| doc-04 (scan) | 1 | 0,3 | 0,1 | 5,6 / 5,6 | 4,2 / 4,2 | 3,33 | 2,46 % | 0 / 0 |
| doc-05 (scan) | 1 | 0,5 | 0,1 | 14,1 / 14,1 | 8,5 / 8,5 | 1,47 | 1,03 % | 0 / 0 |
| doc-06 (scan) | 1 | 0,4 | 0,1 | 5,8 / 5,8 | 4,1 / 4,1 | 4,16 | 3,30 % | 0 / 0 |
| doc-07 (natif) | 1 | 0,3 | 0,8 | 6,4 / 6,4 | 4,0 / 4,0 | 2,00 | 1,90 % | 0 / 0 |
| doc-08 (natif) | 3 | 0,5 | 16,7 | 6,0 / 9,0 | 3,0 / 4,4 | 0,83 | 0,76 % | 0 / 0 |
| doc-09 (natif) | 1 | 0,4 | 1,2 | 5,1 / 5,1 | 4,0 / 4,0 | 1,84 | 1,42 % | 0 / 0 |
| doc-10 (scan) | 3 | 0,5 | 0,1 | 32,1 / 32,2 | 5,8 / 5,9 | 1,82 | 1,15 % | 0 / 0 |

Rendu moyen : MuPDF 14,1 ms par page, PDFium 5,9 ms (17 pages).

## Comparaison visuelle

Examen des images côte à côte produites par `make bench` (dans
`~/Library/Caches/PDFStudio/bench`, hors du dépôt) :

- **PDF natifs à polices intégrées :** géométrie, chasse et coupures identiques au
  pixel près ; MuPDF anticrénèle un peu plus doucement, PDFium donne des fûts
  légèrement plus foncés. Écart de 0,8 à 2,0 sur 255 : indiscernable à 100 %.
- **Scans :** même cadrage et même contenu ; tampons, signatures manuscrites et
  filets identiques, couche de texte OCR invisible avec les deux moteurs. MuPDF garde des bords plus nets et le
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
| Rendu moyen, documents personnels | 14,1 ms | 5,9 ms |
| Fichier tronqué | réparé, 100 pages | refusé |
| Fidélité, polices intégrées | identique | identique |
| Fidélité, polices standard non intégrées | substituts mélangés | polices macOS |
| Détail des scans | conservé | lissé |
| Fonctions des jalons suivants | extraction riche, écriture, OCR intégré | plus limitées |
| Intégration | statique, un seul fichier | bibliothèque dynamique à embarquer |

Les deux points faibles de MuPDF sont traités au jalon 2 : choix explicite des
polices macOS pour les 14 polices standard, et analyse de la lenteur sur les scans
lourds (doc-10 : 32 ms par page à 150 ppp, soit environ 140 ms en plein écran
Retina, ce qui reste sous le seuil gênant puisque le rendu est asynchrone).

## Jalon 2 : polices standard et décomposition du rendu

Nouvelle mesure du 21 octobre 2026, même Mac, avec
`make bench BENCH_FLAGS="--profile --fonts"`, après le passage aux clones URW
pour les 14 polices standard (décision D8).

**Polices standard.** L'écart avec PDFium baisse de 0,79 à 0,66 sur
`texte_simple.pdf` et de 4,24 à 2,83 sur `multipage_100.pdf`. L'écart restant
vient du dessin des polices : PDFium utilise celles de macOS, MuPDF désormais les
clones URW pour toutes les graisses, avec les mêmes chasses. Le rendu moyen du jeu
public passe de 3,1 à 1,5 ms par page (PDFium : 2,2 ms), car les polices URW
intégrées au moteur se chargent plus vite que celles du système. Les documents
personnels, à polices intégrées, ne sont pas concernés : leurs écarts sont
inchangés.

| Document | Rendu MuPDF moy./max (ms) | Rendu PDFium moy./max (ms) | Écart moyen (/255) |
|---|---:|---:|---:|
| chiffre.pdf | 8,8 / 8,8 | 3,4 / 3,4 | 0,66 |
| multipage_100.pdf | 1,0 / 2,3 | 2,4 / 2,5 | 2,83 |
| pdf20.pdf | 3,3 / 3,3 | 2,1 / 2,1 | 0,66 |
| signets.pdf | 1,2 / 2,4 | 2,1 / 2,5 | 2,81 |
| texte_simple.pdf | 3,2 / 3,2 | 2,0 / 2,0 | 0,66 |
| volumineux_600.pdf | 1,0 / 2,4 | 2,0 / 2,5 | 2,83 |

**Décomposition du rendu MuPDF** (option `--profile`, document rouvert à chaque
mesure). *Préparation* : construction de la liste d'affichage ; *1er rendu* et
*2e rendu* : rastérisation de la page entière à 150 ppp, la seconde profitant des
images déjà décodées ; *tuiles* : la page à l'échelle 2 en tuiles de 512 pixels.

| Document | Images | Préparation (ms) | 1er rendu (ms) | 2e rendu (ms) | Tuiles (ms, nombre) |
|---|---|---:|---:|---:|---:|
| scan_simule.pdf | JPEG × 1 | 0,7 | 7,8 | 4,6 | 42,1 (12) |
| vectoriel.pdf | aucune | 0,3 | 9,7 | 9,6 | 9,0 (12) |
| texte_simple.pdf | aucune | 1,7 | 1,4 | 0,5 | 1,3 (12) |
| doc-01 | JPEG × 6 | 2,9 | 5,8 | 2,2 | 7,9 (12) |
| doc-02 | JPEG × 6 | 6,3 | 13,1 | 5,8 | 39,9 (12) |
| doc-03 | JPEG × 1 | 0,2 | 4,3 | 3,2 | 14,1 (15) |
| doc-04 | JPEG × 1 | 0,2 | 4,1 | 3,1 | 14,0 (15) |
| doc-05 | JPEG × 1 | 0,0 | 12,4 | 8,0 | 59,8 (18) |
| doc-06 | JPEG × 1 | 0,2 | 3,9 | 2,9 | 13,7 (15) |
| doc-07 | aucune | 2,1 | 3,2 | 2,1 | 3,2 (12) |
| doc-08 | JPEG × 4 | 0,6 | 3,5 | 2,1 | 3,6 (12) |
| doc-09 | aucune | 1,5 | 2,5 | 1,1 | 2,3 (12) |
| doc-10 | JPEG × 3 | 0,0 | 30,4 | 15,3 | 146,3 (12) |

Rendu moyen des documents personnels : MuPDF 13,0 ms par page, PDFium 5,7 ms.

**Lecture.** Sur les scans, le décodage JPEG pèse près de la moitié du premier
rendu ; MuPDF garde ensuite l'image décodée, ce qui divise par deux le rendu
suivant. Le rendu en tuiles est en revanche coûteux (146 ms pour doc-10) : chaque
tuile ne couvre qu'une partie de l'image, et MuPDF la décode à nouveau pour cette
partie. L'application en tient compte : elle affiche une image de la page entière,
mise en cache, et ne recourt aux tuiles qu'au-delà de 4 096 pixels, soit un zoom
d'environ 240 % pour une page A4 sur un écran Retina. Un décodeur JPEG plus rapide
(libjpeg-turbo) n'est donc pas retenu pour l'instant ; il reste possible si le
zoom fort sur les scans se révèle gênant à l'usage (cf. `LIMITES.md`).
