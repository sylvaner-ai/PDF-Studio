# Banc d'essai OCR : Vision, Tesseract et ocrs

Mesures prises avec `make ocr-bench` (`engine/tools/ocr_bench`). Les trois
moteurs sont branchés sur le même trait `ocr::OcrEngine` et reçoivent la même
image, préparée par le même code que dans l'application (`ocr::recognize`).

## Méthode

- **Documents à texte de référence (10)** : les six documents de
  `tests/fixtures/ocr/` (lettre, facture, deux colonnes, tableau dense, petits
  caractères en 7 à 9 pt, texte anglais) et `texte_simple.pdf`, `tableau.pdf`,
  `edition.pdf`, `encadre.pdf`. Leur texte, extrait par MuPDF ligne par ligne
  avec sa position, sert de référence.
- **Scans simulés** : chaque page est rendue en niveaux de gris puis dégradée de
  façon déterministe, selon six profils :

  | Profil | Résolution | Dégradation |
  |---|---:|---|
  | propre | 300 ppp | encre 20, papier 245, flou léger, bruit faible, JPEG 92 |
  | dégradé | 300 ppp | encre pâle, papier gris, éclairage inégal (−22 % à droite), flou, bruit fort, JPEG 45, inclinaison 1,2° |
  | incliné | 300 ppp | page posée de travers (4°) |
  | retourné | 300 ppp | page à l'envers |
  | couché | 300 ppp | page couchée (quart de tour) |
  | 150 ppp | 150 ppp | scan rapide ou télécopie |

- **Prétraitements** : *brut* (image transmise telle quelle), *préparé* (fond
  égalisé et contraste, redressement, orientation : réglage par défaut de
  l'application), *binarisé* (préparé, plus filtre médian et binarisation de
  Sauvola).
- **Mesure** : chaque mot lu est ramené dans la page d'origine et rattaché à la
  ligne de référence qui contient son centre ; le texte de chaque ligne est
  comparé à la référence par distance d'édition (Levenshtein), après
  normalisation Unicode (NFC, ligatures, apostrophes). Le taux de caractères
  corrects est 1 − erreurs / caractères de référence ; un mot manqué compte
  toutes ses lettres en erreur.
- **Vrais scans** (sans référence) : `scan_simule.pdf` et les documents privés
  de Sylvain sans texte ; on note le nombre de mots, la confiance moyenne et le
  temps. Les documents privés n'apparaissent que sous un nom anonyme
  (« privé-NN ») ; ceux qui ont un texte natif sont mesurés comme les documents
  publics.

## Mesures du cloud (Linux) : Tesseract et ocrs

Mesures du 22 octobre 2026 dans un conteneur Linux x86-64 (Tesseract 5.3.4,
modèles `fra` et `eng` de `tesseract-ocr/tessdata` ; `ocrs` 0.13 et ses
modèles), 360 lectures. Vision ne fonctionne que sous macOS : voir la section
suivante.

### Caractères corrects par profil

| Moteur | Prétraitement | propre | dégradé | incliné | retourné | couché | 150 ppp | Mots corrects | Temps par page (300 ppp) |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Tesseract | brut | 99,66 % | 37,51 % | 57,74 % | 19,16 % | 19,44 % | 98,79 % | 47,60 % | 1,4 s |
| Tesseract | **préparé** | 99,32 % | **97,28 %** | **97,65 %** | **99,44 %** | **99,75 %** | 98,65 % | **96,65 %** | 2,0 s |
| Tesseract | binarisé | 99,69 % | 96,08 % | 92,05 % | 99,75 % | 99,71 % | 94,86 % | 93,06 % | 2,3 s |
| ocrs | brut | 90,86 % | 87,52 % | 87,74 % | 17,34 % | 1,34 % | 90,93 % | 39,63 % | 0,6 s |
| ocrs | préparé | 91,55 % | 85,87 % | 90,74 % | 16,87 % | 77,69 % | 91,30 % | 51,55 % | 1,1 s |
| ocrs | binarisé | 90,23 % | 89,03 % | 87,98 % | 15,84 % | 79,05 % | 86,58 % | 44,57 % | 1,3 s |

Le temps de Tesseract comprend le lancement de la commande et le chargement de
ses modèles à chaque page.

### Détail par document (préparé)

| Document | Tesseract propre | Tesseract dégradé | Tesseract incliné | ocrs propre | ocrs dégradé |
|---|---:|---:|---:|---:|---:|
| anglais.pdf | 100,00 % | 100,00 % | 100,00 % | 99,66 % | 90,53 % |
| colonnes.pdf | 99,39 % | 99,39 % | 99,42 % | 91,98 % | 91,49 % |
| facture.pdf | 99,51 % | 89,16 % | 89,32 % | 88,35 % | 64,56 % |
| lettre.pdf | 100,00 % | 100,00 % | 99,75 % | 89,86 % | 87,02 % |
| petits_caracteres.pdf | 99,90 % | 99,90 % | 99,90 % | 95,24 % | 92,65 % |
| tableau_dense.pdf | 96,71 % | 89,88 % | 97,04 % | 86,17 % | 82,39 % |
| texte_simple.pdf | 100,00 % | 100,00 % | 100,00 % | 86,97 % | 79,20 % |
| tableau.pdf | 100,00 % | 83,80 % | 67,91 % | 83,49 % | 73,52 % |
| edition.pdf | 100,00 % | 100,00 % | 100,00 % | 93,49 % | 78,08 % |
| encadre.pdf | 99,51 % | 99,51 % | 99,51 % | 95,12 % | 44,39 % |

### Confiance et mots faux (Tesseract, préparé)

2,77 % des mots sont faux. Pour chaque seuil : part des mots signalés, part de
mots faux parmi eux, part des mots faux signalés.

| Seuil | Mots signalés | Faux parmi les signalés | Faux signalés |
|---:|---:|---:|---:|
| 0,3 | 0,45 % | 67,39 % | 10,92 % |
| 0,5 | 0,85 % | 64,37 % | 19,72 % |
| 0,7 | 1,56 % | 60,62 % | 34,15 % |
| 0,9 | 5,13 % | 32,51 % | 60,21 % |

`ocrs` ne donne aucune confiance (toujours 1).

### Orientation et inclinaison (préparé)

- Pages retournées remises à l'endroit : Tesseract 10 sur 10 ; `ocrs` 0 sur 10
  (sans confiance, une lecture à l'envers n'est jamais jugée douteuse).
- Pages couchées remises à l'endroit : 8 sur 10 pour les deux moteurs. Les deux
  pages manquées sont les tableaux : Tesseract y lit le texte vertical sans
  rotation (texte juste, page laissée couchée) ; `ocrs` n'y lit rien.
- Inclinaison de 4° mesurée à 0,03° près en moyenne.

### Enseignements

- **La préparation est indispensable** sur les scans réels : sans elle,
  Tesseract tombe à 37 % sur un scan dégradé et à 19 % sur une page retournée.
- **La binarisation n'est pas retenue par défaut** : elle fait perdre 4 points à
  150 ppp et 5 points sur une page inclinée, pour un gain nul sur les scans
  propres (D27).
- **Le premier passage du banc a révélé deux défauts**, corrigés : les bords
  sombres d'une page faisaient croire à une page couchée, et le grain du
  papier, laissé gris sur toute la page, égarait Tesseract sur une page presque
  vide (`encadre.pdf` dégradé : 0 % avant, 99,5 % après). Sur les scans
  dégradés, Tesseract préparé est passé de 76 % à 97 %.
- **ocrs est écarté** : sans accents dans son alphabet, il plafonne à 91 % sur
  le français, et sans confiance il ne sait ni signaler un mot douteux ni
  reconnaître une page à l'envers.
- **Tesseract atteint le critère** (99,3 % à 99,7 % sur les scans propres en
  français, 2 s par page) mais lit mal les tableaux dès que le scan se dégrade.

## Mesures du Mac : Vision

*À compléter par la vérification sur le Mac de Sylvain (`make ocr-bench`) :
caractères corrects par profil, critère des 98 % sur les documents publics et
privés, temps par page, calibration de la confiance, vrais scans.*

## Choix

Vision est retenu à titre provisoire (D26) : intégré à macOS, sans dépendance
ni modèle à embarquer. Le choix sera confirmé si Vision atteint au moins 98 %
de caractères corrects sur les scans propres en français et moins de 5 s par
page ; sinon, Tesseract, qui atteint le critère dans le cloud, serait intégré
derrière le même trait.
