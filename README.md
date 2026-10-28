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
| 8. Design | finition visuelle, animations, mode sombre, icône, accessibilité | terminé (à valider) |
| 9. Durcissement | performances, récupération, documentation, jeu de tests | terminé (à valider) |

**Mode Amélioration** (redresser, nettoyer et reconnaître les scans) : en cours,
jalon 2 sur 6 (redressement des pages, ⌘4) ; voir [docs/AMELIORATION.md](docs/AMELIORATION.md).

## Onglets

- Chaque document s'ouvre dans un **onglet** de la fenêtre au premier plan, avec
  son propre état : mode, page lue, sélection, modifications et annulations.
- **Nouvel onglet** (⌘T, ou le bouton + de la barre d'onglets) affiche l'écran
  d'accueil ; **Ouvrir…** (⌘O) y place le document, ou l'ajoute en nouvel onglet.
  Les PDF déposés depuis le Finder sur une fenêtre s'y ouvrent en onglets
  (plusieurs à la fois), qu'un document y soit affiché ou non ; seule la grille
  du mode Pages garde son usage : les pages déposées y sont insérées. Un document
  déjà ouvert est ramené au premier plan.
- **Fermer l'onglet** (⌘W, ou la croix de l'onglet) demande d'abord s'il faut
  enregistrer un document modifié.
- Le reste est celui de macOS : ⌃Tab et ⌃⇧Tab passent d'un onglet à l'autre ;
  un onglet se déplace à la souris, ou se détache dans sa propre fenêtre
  (menu Fenêtre, « Placer l'onglet dans une nouvelle fenêtre ») ; « Fusionner
  toutes les fenêtres » réunit les fenêtres en onglets.

## Lire et copier le texte

- **Sélection à la souris :** en mode Lecture, le texte des pages se sélectionne
  en glissant (sur plusieurs pages si besoin), ⇧-clic étend la sélection,
  double-clic prend un mot, triple-clic une ligne. ⌘C (Édition > Copier) place le
  texte dans le presse-papiers. Cela vaut pour le texte natif et pour le texte
  reconnu des pages scannées, avant comme après l'enregistrement (D57).

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

- **Mode Édition** (⌘3, ou le crayon de la barre d'outils) : un PDF natif s'y
  modifie aussitôt ; pour un PDF scanné, le texte de toutes ses pages scannées est
  d'abord reconnu (bandeau d'avancement, « Arrêter » ou ⌘.), puis ses objets
  deviennent modifiables. Un clic sélectionne un bloc de texte, une image ou un
  tracé ; ⇧-clic en ajoute ; un rectangle tracé sur la page en sélectionne
  plusieurs.
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
- **Pages scannées :** une fois la page reconnue (à l'entrée en Édition, y compris
  un scan qui portait déjà le texte d'un autre logiciel), ses lignes, ses tableaux,
  ses logos et signatures se sélectionnent comme des objets ; les déplacer ou les supprimer retouche l'image (le fond est
  reconstitué). Réécrire une ligne la redessine dans la police la plus proche du
  scan ; le **panneau de retouche**, en haut à gauche, propose les trois polices
  retrouvées (avec leur ressemblance) et règle le corps, la graisse,
  l'interlettrage, le flou, le grain et la couleur. Une retouche n'est pas un
  caviardage : les pixels d'origine restent dans le fichier.
- **Au clavier :** Tab et ⇧Tab passent d'un objet à l'autre, dans l'ordre de
  lecture ; VoiceOver annonce chaque objet et le sélectionne par VO-Espace.
- **Comparer avec l'original** (⌥⌘K, ou le bouton de la barre d'outils) montre
  les pages sans leurs modifications, à la même place, le temps de comparer.
- Tout s'annule par ⌘Z, avec l'organisation des pages ; les limites sont décrites
  dans [docs/LIMITES.md](docs/LIMITES.md).

## Améliorer les scans

- **Mode Amélioration** (⌘4, ou la baguette de la barre d'outils) : la page
  affichée est montrée avant et après redressement ; faire glisser le curseur
  pour comparer. Le panneau de droite règle le redressement (inclinaison) et
  l'orientation (pages couchées ou à l'envers), et les pages à traiter :
  toutes, la sélection de la barre latérale ou la page affichée.
- **Appliquer** traite les pages (bandeau d'avancement, « Arrêter ») ; ⌘Z
  annule. Une page inclinée de plus de 15° n'est pas redressée : elle est
  listée « à vérifier ». **Rétablir l'original** rend les pages d'origine ;
  ⌥⌘K les montre le temps de comparer.
- À l'enregistrement, la page redressée remplace l'originale ; signets, liens
  et annotations sont gardés.

## Sécurité des documents

- **Aucun original modifié sans confirmation :** le fichier ouvert est lu une fois
  en mémoire ; seul ⌘S le réécrit, après confirmation et copie de l'original
  dans les sauvegardes (30 copies au plus). L'application est dans le bac à
  sable de macOS, sans aucune autorisation réseau : elle ne peut émettre
  aucune connexion.
- **Plantage ou coupure :** les modifications non enregistrées sont retenues au
  fil de l'eau (au plus une demi-seconde de retard) et proposées à la
  réouverture du document (Restaurer ou Ignorer). Un enregistrement interrompu
  est détecté à la réouverture, et le fichier rétabli dans sa version
  précédente.
- **Fichiers abîmés :** une erreur du moteur sur une page n'interrompt que
  l'opération en cours ; un PDF illisible est refusé avec un message en
  français.
- Les sauvegardes et les modifications retenues sont dans
  `~/Library/Containers/fr.sylvainrobert.pdfstudio/Data/Library/Application Support/PDF Studio`.

## Critères d'acceptation (§9 du cahier des charges)

| Critère | État |
|---|---|
| Compilation en une commande, lancement sur macOS 14+ | `make build` ; vérifié sur macOS 27 (Apple Silicon), macOS 14 non essayé |
| Fonctionnalités du §4 sur le jeu de test | testées automatiquement (`make test`) ; à valider à l'usage |
| 10 textes modifiés dans des PDF natifs, identiques à 100 % de zoom | évaluation de Sylvain à faire |
| 10 textes modifiés dans des scans, indiscernables dans au moins 7 cas | évaluation de Sylvain à faire ; échecs à consigner dans LIMITES |
| Performances du §6 (100 pages en moins de 2 s, 60 images/s, OCR en moins de 5 s) | 19 ms ; 59,5 images/s sur 100 pages, 56 à 59 sur 600 ; 0,3 à 0,5 s ([mesures](docs/PERFORMANCES.md)) |
| OCR ≥ 98 % sur des scans propres en français | atteint : 98,63 à 99,68 % avec Vision ([banc d'essai](docs/OCR_BENCHMARK.md)) |
| Interface conforme au §5 | revue visuelle de Sylvain (`make snapshots` aide à la refaire) |
| Aucun original modifié sans confirmation, aucune connexion réseau | vérifié par des tests (`CrashRecoveryTests`, `PrivacyTests`) et par le bac à sable |
| Documentation à jour | README, ARCHITECTURE, DECISIONS, LIMITES, DEPENDANCES |

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
make icon       # régénère les images de l'icône depuis app/Resources/AppIcon.svg
make snapshots  # captures d'écran de l'interface, en mode clair et sombre
make perf       # mesures de performance (§6), rapport dans ~/Library/Caches/PDFStudio/perf
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
- [Mode Amélioration](docs/AMELIORATION.md)
- [Banc d'essai du rendu](docs/RENDER_BENCHMARK.md)
- [Banc d'essai de l'OCR](docs/OCR_BENCHMARK.md)
- [Mesures de performance](docs/PERFORMANCES.md)
