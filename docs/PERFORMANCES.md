# Mesures de performance

Exigences du cahier des charges (§6) : ouverture d'un PDF de 100 pages en moins
de 2 s, défilement à 60 images par seconde, OCR d'une page A4 à 300 ppp en moins
de 5 s sur Apple Silicon.

Mesures du 23 octobre 2026 sur le Mac de développement (MacBook Air M3, 16 Go,
macOS 27.0.1), commit e714c2e, par `make perf` (`app/Tests/PerformanceTests.swift`) :
l'application hébergeant les tests, compilée avec optimisations, et le moteur
Rust en Release. Deux passages consécutifs ; les documents viennent du jeu de
test public (`tests/fixtures`), aucun document personnel n'est mesuré.

## Résumé

| Exigence | Mesure | État |
|---|---|---|
| 100 pages ouvertes en moins de 2 s | 19 ms jusqu'à la première page affichée (`multipage_100.pdf`) ; 16 ms pour 600 pages | atteint |
| Défilement à 60 images/s | 59,5 images/s sur 100 pages (0,6 à 0,8 % d'images manquées) ; 56,2 à 58,9 images/s sur 600 pages (1,6 à 6,6 %) | presque atteint (voir ci-dessous) |
| OCR d'une page A4 à 300 ppp en moins de 5 s | 0,29 à 0,44 s (Vision, rendu et couche de texte compris) | atteint |

## Ouverture

De la demande d'ouverture à la première page rendue à la largeur d'une fenêtre
Retina (1 600 pixels). L'initialisation du moteur, au premier document ouvert,
prend 4 ms.

| Document | Pages | Document prêt (ms) | Première page (ms) | Total (ms) |
|---|---:|---:|---:|---:|
| multipage_100.pdf | 100 | 2 | 17 | 19 |
| volumineux_600.pdf | 600 | 4 | 12 | 16 |
| edition.pdf | 2 | 1 | 11 | 12 |
| tableaux.pdf | 1 | 1 | 11 | 12 |
| scan_simule.pdf | 1 | 1 | 21 | 23 |

Les dimensions de toutes les pages sont lues à l'ouverture ; seules les pages
visibles sont rendues.

## Défilement

Défilement continu à 2 000 points par seconde (environ deux pages A4 et demie
par seconde), sur 25 pages, fenêtre de 1 080 × 760 points avec la barre des
vignettes, ajustement à la largeur, écran à 60 images par seconde. Le
défilement est commandé image par image par l'horloge de l'écran ; une image
manquée est un intervalle de plus d'une fois et demie la période. Deux
passages :

| Document | Images/s | Images manquées | Travail moyen (ms) | Travail max (ms) | Pages blanches |
|---|---:|---:|---:|---:|---:|
| multipage_100.pdf | 59,5 / 59,6 | 0,8 % / 0,6 % | 2,6 / 2,6 | 32,6 / 32,4 | 0 % |
| volumineux_600.pdf | 56,2 / 58,9 | 6,6 % / 1,6 % | 3,0 / 2,7 | 40,9 / 37,7 | 0 % |
| tableaux.pdf (1 page) | 60,0 / 60,0 | 0 % | 1,0 / 0,9 | 2,0 / 1,1 | 0 % |
| volumineux_600.pdf, page lue non publiée | 59,8 / 60,0 | 0,2 % / 0 % | 1,7 / 1,9 | 18,4 / 20,1 | 0 % |

*Travail* : temps passé sur le fil principal pour une image (mise en page,
dessin, validation Core Animation). *Pages blanches* : images où une page
visible attend encore son rendu — aucune.

**Analyse.** La dernière ligne est un diagnostic : la page lue n'y est plus
publiée à l'interface (sous-titre « Page n sur N », vignette sélectionnée de la
barre latérale, menus). Le défilement atteint alors 60 images par seconde. Le
reste du coût vient donc de la mise à jour de l'interface quand la page lue
change, environ deux fois par seconde à cette vitesse : jusqu'à 28–37 ms de
dessin sur l'image concernée. Avant le jalon 9, ce changement recalculait toute
la fenêtre ; il ne recalcule plus que le sous-titre, la sélection des
vignettes et leur défilement (D56), ce qui a réduit de moitié les images
manquées sur 100 pages. Sur 600 pages, le résultat varie d'un passage à
l'autre (1,6 à 6,6 % d'images manquées). Les pointes de validation Core
Animation (18–20 ms), présentes même sans publication, restent rares.

Historique sur 600 pages : 55,3 à 56,1 images/s avant cette correction
(commit e0080ee).

## Reconnaissance du texte (Vision)

Page rendue à 300 ppp, préparée (fond, inclinaison, orientation) puis lue par
Vision ; le temps comprend le rendu et la conversion des mots en couche de
texte.

| Document | Mots lus | Temps (s) |
|---|---:|---:|
| scan_simule.pdf | 314 | 0,44 |
| scan_retouche.pdf | 31 | 0,29 |

La précision de la lecture est mesurée à part (`docs/OCR_BENCHMARK.md`).

## Retouche d'une ligne scannée

Réécriture d'une ligne de `scan_retouche.pdf` : mesure de la ligne, recherche de
la police parmi le catalogue (polices fournies et polices bureautiques
installées), rendu de l'aperçu. La première réécriture établit aussi le
catalogue.

| Ligne | Temps (ms) |
|---:|---:|
| 1 (catalogue compris) | 170 à 315 |
| 2 | 36 |
| 3 | 37 à 38 |

## Récupération d'un gros document

`volumineux_600.pdf`, 600 pages munies chacune d'une couche de texte de
300 mots, puis 10 rotations de page.

| Mesure | Valeur |
|---|---:|
| Modification, fil principal (moyenne et maximum) | 1 ms |
| Écriture de la récupération, hors du fil principal | 355 ms |
| Taille du fichier de récupération | 14,6 Mo |

## Refaire les mesures

`make perf` : des fenêtres apparaissent quelques secondes ; le rapport est écrit
dans `~/Library/Caches/PDFStudio/perf/rapport.md`. Les mesures dépendent de la
charge du Mac au moment de l'essai.
