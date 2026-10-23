# PDF Studio

Application macOS native d'édition de PDF, à usage strictement personnel :
locale, hors ligne, sans compte ni télémétrie.

- Interface : SwiftUI (macOS 14 minimum, Apple Silicon).
- Moteur : Rust (MuPDF), relié à Swift par UniFFI.
- Cahier des charges : [docs/CAHIER_DES_CHARGES.md](docs/CAHIER_DES_CHARGES.md).

## État d'avancement

| Jalon | Contenu | État |
|---|---|---|
| 1. Socle | moteur Rust, pont UniFFI, ouverture et affichage d'un PDF | terminé |
| 2. Lecture complète | zoom, vignettes, recherche, signets, PDF chiffrés | terminé |
| 3. Organisation des pages | §4.5 et enregistrement sécurisé | terminé |
| 4. Édition des PDF natifs | sélection, déplacement, suppression, texte modifiable | terminé (à valider) |
| 5. OCR | reconnaissance du texte des scans (Vision), couche de texte invisible | terminé |
| 6. Structure | détection des tableaux, édition des tableaux | terminé (à valider) |
| 7. Scans | retouche des pages scannées : texte réécrit dans la police retrouvée, objets effacés ou déplacés, comparaison avant/après | terminé (à valider) |
| 8 et 9 | design, durcissement | à faire |

## Organiser les pages

- **Présentations :** lecture (⌘1) ou grille d'organisation (⌘2) ; ⌘+ et ⌘-
  changent la taille des vignettes de la grille.
- **Menu Pages**, sur les pages sélectionnées : faire pivoter (⌘L, ⌘R), dupliquer
  (⌘D), supprimer (⌘⌫), insérer une page vierge (⌥⌘N) ou les pages d'autres PDF
  (⌥⌘I), extraire les pages (⌥⌘E), scinder le document. Tout s'annule par ⌘Z.
- **Glisser-déposer :** les vignettes se déplacent dans la barre latérale ou la
  grille ; un PDF déposé depuis le Finder sur la grille y insère ses pages ; des
  pages glissées vers la grille d'une autre fenêtre y sont copiées.
- **Fusionner :** ouvrir le premier document, y insérer les autres, puis
  Enregistrer sous.
- **Enregistrer** (⌘S) remplace le fichier après confirmation, en gardant une copie
  de l'original ; **Enregistrer sous** (⇧⌘S) écrit une copie. Les modifications non
  enregistrées sont proposées à la prochaine ouverture du document.
- **Exporter**, menu Fichier : les pages en images (PNG ou JPEG) ou le texte en .txt.

## Modifier le contenu des pages

- **Mode Édition** (⌘3, ou le crayon de la barre d'outils) : un clic sélectionne un
  bloc de texte, une image ou un tracé ; ⇧-clic en ajoute ; un rectangle tracé sur
  la page en sélectionne plusieurs.
- **Déplacer** en glissant, ou avec les flèches (1 pt, 10 pt avec ⇧) ; **supprimer**
  avec ⌫.
- **Modifier un texte** par double-clic ou Retour : Retour valide, ⇧Retour passe à
  la ligne, Échap annule. Le texte garde sa police si elle contient tous les
  caractères ; sinon la même police installée sur le Mac est intégrée, ou une
  police standard la remplace (le bandeau du bas le signale).
- **Tableaux :** un tableau tracé (quadrillage, ou filets horizontaux) se
  sélectionne d'un clic, qui désigne aussi la cellule courante ; il se déplace et
  se supprime comme un objet. Double-clic ou Retour modifie le texte de la
  cellule (une cellule vide prend le style de ses voisines) ; le **menu Tableau**
  et le clic droit suppriment, montent ou descendent la ligne, suppriment ou
  déplacent la colonne. Le tableau se resserre, ses filets et ses fonds suivent.
- **Pages scannées :** une fois la page reconnue (bouton « Reconnaître le texte »),
  ses lignes, ses tableaux, ses logos et signatures se sélectionnent comme des
  objets ; les déplacer ou les supprimer retouche l'image (le fond est
  reconstitué). Réécrire une ligne la redessine dans la police la plus proche du
  scan ; le **panneau de retouche**, en haut à gauche, propose les trois polices
  retrouvées (avec leur ressemblance) et règle le corps, la graisse,
  l'interlettrage, le flou, le grain et la couleur. Une retouche n'est pas un
  caviardage : les pixels d'origine restent dans le fichier.
- **Comparer avec l'original** (⌥⌘K, ou le bouton de la barre d'outils) montre
  les pages sans leurs modifications, à la même place, le temps de comparer.
- Tout s'annule par ⌘Z, avec l'organisation des pages ; les limites sont décrites
  dans [docs/LIMITES.md](docs/LIMITES.md).

## Prérequis

- macOS 14 ou plus récent sur Apple Silicon, Xcode 16 ou plus récent ;
- Rust stable (`rustup`, cible `aarch64-apple-darwin`) ;
- XcodeGen et SwiftLint : `brew install xcodegen swiftlint`.

## Commandes

```sh
make build      # produit build/PDF Studio.app
make test       # tests du moteur (cargo) et de l'application (XCTest)
make lint       # clippy, rustfmt, SwiftLint
make bench      # banc d'essai MuPDF / PDFium (télécharge PDFium pour la mesure)
make fixtures   # régénère le jeu de PDF de test
```

Le premier `make build` compile MuPDF et prend plusieurs minutes ; les suivants
sont incrémentaux. Le projet Xcode (`app/PDFStudio.xcodeproj`) est généré : pour
travailler dans Xcode, lancer une fois `make project`, puis l'ouvrir.

## Documents de test personnels

Déposez vos propres PDF dans `tests/fixtures/private/` : ce dossier est exclu du
dépôt et ne quitte jamais votre Mac. `make bench` les mesure en anonymisant leurs
noms.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Décisions techniques](docs/DECISIONS.md)
- [Dépendances et licences](docs/DEPENDANCES.md)
- [Limites connues](docs/LIMITES.md)
- [Banc d'essai du rendu](docs/RENDER_BENCHMARK.md)
