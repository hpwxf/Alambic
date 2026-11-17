# D-009 — Écran : framebuffer 1 bpp dans l'ordre du contrôleur, pilote maison, SH1106 par défaut

| | |
|---|---|
| **Identifiant** | D-009 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01, révisée le 2026-08-02) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `555f492` (`framebuffer.rs`), `ec140ed` (`ssd130x.rs`, défaut SSD1306), `3f50a42` (SH1106 par défaut), `2deca8a` (features relayées par `xtask`) |

## 1. Contexte

L'OLED 128×64 monochrome est l'unique retour visuel du module. Le même écran doit s'afficher dans le simulateur
(braille / octants) et dans Rack (NanoVG). Le contrôleur exact de la dalle n'était pas connu : SSD1306, SSD1309 ou
SH1106 selon les fabrications.

## 2. Options envisagées

Pour la mémoire d'image :

**A** — `embedded-graphics` avec un `DrawTarget` propre à chaque backend (`SimulatorDisplay` côté hôte, crate
`ssd1306` côté cible), comme le suggérait le plan.

**B** — Un seul `FrameBuffer` de 1 024 octets **dans l'ordre mémoire du contrôleur** (pages de 8 lignes, un octet =
une colonne de 8 pixels), écrit par `embedded-graphics` dans `oc-core`, poussé tel quel par le firmware et lu octet
pour octet par le simulateur et l'ABI C.

Pour le contrôleur :

**C** — Crates tiers `ssd1306` / `sh1106`. **D** — Un pilote maison `ssd130x` paramétré par `Controller`, testé
contre un bus simulé.

## 3. Ce que l'assistant a proposé

**B** et **D**. `555f492` : « 128x64 1bpp in the OLED controller order, so the firmware can push it unrepacked and
every backend reads the same bytes ». `ec140ed` : `ssd130x` « supports both OLED controllers the panel ships with;
a test asserts that only the charge-pump and pre-charge bytes differ ». Défaut initial : SSD1306, SSD1309 en
feature.

## 4. Ce que nous avons décidé, et pourquoi

**B** et **D**. Un seul format d'image partagé est ce qui rend les écrans dorés du simulateur (D-013) représentatifs
du module, et le pilote maison laisse `oc-drivers` testable sans dépendre des choix d'API d'un crate tiers.

## 5. Ce qui nous ferait changer d'avis

* Un rendu partiel (ne pousser que les pages modifiées, ou une page par période d'ISR — piste de D-023) : le format
  page-major s'y prête, mais il faudra un suivi de pages sales dans `FrameBuffer`.
* Une dalle en niveaux de gris ou plus grande : le format « ordre du contrôleur » cesserait d'être universel.

## Révision (2026-08-02, `3f50a42`)

Premier essai sur le module : seules ≈ 8 lignes de pixels semblaient vivantes. Le module porte une dalle **SH1106**
(132 colonnes, décalage de 2, rafraîchissement par page), pas un SSD1306 en adressage horizontal. Le défaut devient
SH1106 ; `ssd1306` et `ssd1309` deviennent des features. Le même commit découpe les écritures SPI sous le plafond de
trame LPSPI de 512 octets (D-010). `2deca8a` relaie `--features` à travers `cargo xtask build|size|hex|flash`.

## Révision (2026-11-17, lot 3c2) — la trame part une page par tick

D-023 (option C+) : dessiner et envoyer la trame entière dans le même tick coûtait ≈ 0,63 + 1,16 ms, au-delà de la
période (mesure du lot 3b, indicative). Le framebuffer et le pilote ne changent pas ; ce qui change est **qui
découpe l'envoi** :

* `Display` gagne `page_count` (1 par défaut) et `present_page` (par défaut, la trame entière pour la page 0) ;
* l'`Engine` dessine dans un tick (toast, rotation, décalage compris), puis envoie **une page par tick suivant** ;
  il ne redessine qu'après la dernière page, si bien qu'**aucune page n'est envoyée à moitié redessinée**, même
  avec un intervalle de rendu plus court que le nombre de pages. Ce n'est pas une image sans transition : pendant
  l'envoi (≈ 8 ms, une page par tick), l'OLED montre en haut les pages déjà reçues de la nouvelle trame et en bas
  celles de l'ancienne. La transition durait ≈ 1,2 ms quand la trame partait d'un bloc ; elle occupe maintenant
  8 des 20 ms de chaque image. Rien de visible sur le Diagnostic, le Scope et le SETUP (2026-11-17) ; un contenu
  qui défile vite pourrait montrer un décalage entre haut et bas. Parades si c'était le cas : deux pages par tick
  sur les ticks sans dessin, ou le SPI en DMA (D-023, option A) ;
* un écran d'une page (simulateur, Rack, tests) reçoit la trame dans le tick qui la dessine, comme avant : les
  écrans dorés ne bougent pas ;
* l'écran de démarrage envoie toujours la trame entière à chaque tick (sorties au repos, rien à protéger) ;
* le firmware n'enveloppe plus le pilote pour chronométrer (`TimedPanel` devient `PagedPanel`) : l'`Engine` mesure
  chaque page (`Stage::Page`), `Stage::Present` ne couvre plus que les envois entiers.

## Points ouverts (revue du 2026-11-16)

* Les features `ssd1306` et `ssd1309` sont mutuellement exclusives, mais rien ne l'impose : avec les deux,
  `ssd1309` gagne silencieusement. Un `compile_error!` sur la combinaison coûterait deux lignes.

## Pistes (2026-11-17, après le lot 3c2) — non engagées

Deux gains distincts, à ne pas confondre :

* **N'envoyer que ce qui a changé.** Le SH1106 comme le SSD130x acceptent un envoi qui commence à n'importe quelle
  colonne d'une page (commandes de colonne et de page, puis N octets ; fenêtre colonnes / pages sur SSD130x), dans
  n'importe quel ordre. Les applets ne changent pas : elles dessinent toujours leur image entière ; le cœur la
  compare à la dernière image envoyée (copie de 1 Kio) et n'en extrait que les morceaux modifiés. Limites : l'unité
  est l'octet (8 pixels verticaux) et l'OLED ne se relit pas (pas de MISO) ; chaque morceau coûte ≈ 3 octets de
  commande et deux fenêtres CS (≈ 14 µs de surcoût par envoi d'après une page mesurée, indicatif), d'où des morceaux
  voisins fusionnés et une page entière au-delà d'un seuil ; le pire cas (Scope qui défile, changement d'écran)
  reste une trame entière, donc un budget de bus par tick reste nécessaire. Gain : le **bus** (et la transition de
  ≈ 8 ms) ; utile surtout à l'option A de D-023. Ne réduit pas le dessin.
* **Accélérer le dessin du texte.** Le dessin, pas l'envoi, fixe la marge du tick le plus chargé (≈ 646 µs pour le
  Diagnostic contre ≈ 73 µs pour le Scope, ≈ 40 µs de marge ; indicatif) ; le texte en est le suspect. Le texte est
  aligné sur les pages (police de 8 pixels, `ROW_HEIGHT`) : une colonne de glyphe tient dans un octet et pourrait
  être copiée telle quelle au lieu d'être posée pixel par pixel. Transparent pour ce que les applets affichent ;
  hypothèse à mesurer avant d'écrire quoi que ce soit (exploration ouverte le 2026-11-17).

## Révision (2026-11-17) — le texte posé colonne par colonne (`oc_core::text`)

La seconde piste ci-dessus, mesurée puis faite. Mesure d'abord (hôte, criterion, indicatif) : le texte prenait
≈ 96 % du dessin de l'écran Diagnostic ; `embedded-graphics` pose chaque pixel de glyphe à part (≈ 40 itérations
par caractère, police rangée par lignes, framebuffer par colonnes).

* **`oc_core::text`** : `FONT_5X8` en table de colonnes (`GLYPHS`, 96 glyphes × 5 octets, extraits de
  `embedded-graphics`) ; `text::draw` pose chaque colonne d'un coup (`FrameBuffer::paint_column` : un octet sur la
  grille des pages, deux moitiés décalées sinon). Couleur (allumé, éteint), alignement, ligne de base, lignes
  multiples et glyphe de remplacement comme `embedded-graphics`.
* **Mêmes pixels** : les tests comparent les deux tracés pour chaque glyphe, dans les deux couleurs, sur et hors
  de la grille des pages et aux bords, et pour chaque alignement et ligne de base ; écrans dorés inchangés.
  `embedded-graphics` reste la référence des tests et sert toujours aux lignes et rectangles.
* **Tous les textes** passent par `text::draw` (Diagnostic, Scope, menu, SETUP, assistant, écran de démarrage,
  message de sauvegarde) ; les applets ne changent que l'appel.
* **Gain** (hôte, criterion, indicatif) : `applet/render` 15,7 → 2,8 µs (−82 %), `menu/render` 4,6 → 2,0 µs,
  un écran de texte 13,4 → 1,06 µs (banc `benches/graphics.rs`, qui garde les deux tracés).
* **Sur le module TLM** (`53ee310`, `cargo probe stats --fresh 10`, Diagnostic, indicatif) : `render`
  645,5 → **87,8 µs** (÷ 7,4) ; pire tick 671 → 169 µs, ≈ 0,46 ms échantillonnage compris : la marge du tick le
  plus chargé passe d'≈ 40 à ≈ 540 µs, et ce sont désormais les ticks d'envoi d'une page, plus le dessin, qui
  sont les plus longs ; périodes 997,6 – 1 002,3 µs.

## Révision (2026-11-17) — la police devient un paramètre

Remarque de l'utilisateur : l'alignement sur les pages (`ROW_HEIGHT`) et la police 5×8 sont des choix d'affichage du
Diagnostic, pas des contraintes ; une applet doit pouvoir choisir son affichage. L'alignement n'était déjà pas
imposé (hors grille, une colonne est posée en deux moitiés décalées) ; la police l'était (table `GLYPHS` 5×8, un
octet par colonne). Corrigé :

* **`ColumnFont`** : largeur, hauteur, espacement, ligne de base, et colonnes de ⌈hauteur / 8⌉ octets ; la police
  fait partie du `TextStyle` (`TextStyle::lit(&fonts::FONT_6X10)`), chaque applet choisit la sienne ;
* **`text::fonts`**, généré : `FONT_4X6`, `FONT_5X8`, `FONT_6X10`, `FONT_7X13`, `FONT_9X15`, `FONT_10X20` (un à trois
  octets par colonne), extraits des polices ASCII d'`embedded-graphics` par `tests/fonts.rs`
  (`UPDATE_FONTS=1 cargo test -p oc-core --test fonts`, sur le modèle des écrans dorés) ; le même test compare les
  pixels pour chaque police, glyphe, couleur, alignement, ligne de base et position, et échoue si le fichier n'est
  plus à jour. Une police qu'aucun code n'utilise n'entre pas dans l'image ;
* `embedded-graphics` reste la référence des tests ; à l'exécution, il a été retiré ensuite (D-030, révision du
  2026-11-17).

Coût de la généralité (hôte, criterion, indicatif) : un écran de texte 1,06 → 1,72 µs ; `applet/render` ≈ 2,6 µs,
inchangé à la mesure près.
