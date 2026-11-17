# D-030 — Primitives graphiques dans `oc-core` (`gfx`) et applet de démonstration

| | |
|---|---|
| **Identifiant** | D-030 |
| **Date** | 2026-11-17 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR, plan) ; implémentation à l'étape 5 (lots 5a à 5d) |
| **Relations** | prolonge D-009 (framebuffer, texte par colonnes, police en paramètre) ; ajoute une applet à D-021 ; respecte D-005 (pas de flottant dans `oc-core`) ; mesurée par D-029 |

## 1. Contexte

Les applets dessinent aujourd'hui avec `embedded-graphics` (lignes, rectangles) et `oc_core::text` (texte). Les
applets musicales à venir auront besoin d'un vocabulaire d'interface : potentiomètres, valeurs, courbes, motifs de
séquenceur. L'utilisateur a montré les écrans de Squares-and-Circles (S&C, `squares-and-circles/`, capture du
2026-11-17) comme référence de ce qu'un écran 128 × 64 peut montrer, et demande une applet de démonstration qui
essaie polices, potentiomètres et courbes simulées, en extrayant des primitives utiles.

Deux contraintes viennent de l'étape 3 : le dessin compte dans le tick (D-023, option C+) et se mesure
(`cargo probe stats`, étape `render`) ; le texte par colonnes (D-009) a montré qu'écrire des octets entiers dans
la disposition par pages bat largement la pose pixel par pixel.

## 2. Options envisagées

**Où vivent les primitives.**

* **G1 — un module `oc_core::gfx`** de fonctions sur `FrameBuffer` : bases (lignes, rectangles, zone inversée,
  pointillés, cercles) et contrôles composés (potentiomètre, valeur encadrée, barres, courbes, motifs, bandeau),
  testés sur l'hôte, partagés par toutes les applets ;
* **G2 — chaque applet dessine avec `embedded-graphics`**, sans couche commune : rien à écrire, mais chaque applet
  refait ses contrôles et paie la pose pixel par pixel.

**Trigonométrie** (aiguille d'un potentiomètre, anneau euclidien) : flottants (le Cortex-M7 a une FPU) ou table de
sinus entière.

**Applet de démonstration** : dans le menu, toujours ; ou derrière une feature de compilation.

## 3. Ce que l'assistant a proposé

G1, avec une règle : une base écrit des octets entiers quand la forme le permet (lignes horizontales et verticales,
rectangles, zone inversée), et s'appuie sur `embedded-graphics` sinon (ligne quelconque, cercle) tant que la mesure
ne justifie pas mieux ; les contrôles se construisent sur les bases et sur `text`. Trigonométrie entière (table
de sinus), pour rester sans flottant dans `oc-core` (D-005). Catalogue tiré de la capture S&C :

* bases : ligne horizontale / verticale, rectangle plein ou vide, zone en vidéo inverse, pointillés, ligne
  quelconque, cercle plein ou vide ;
* contrôles : potentiomètre (cercle et aiguille selon la valeur) et étiquette, sélecteur (⬍ et valeur), valeur
  encadrée en inverse, légende en petite police au-dessus d'une valeur, barre horizontale ou verticale, barre
  bipolaire ;
* courbes : trace de scope, forme d'onde en colonnes min / max, polyligne (LFO, enveloppe), histogramme, échelle
  graduée avec aiguille ;
* séquenceur : rangée de cases pleines ou vides, rangée de points, anneau euclidien, mini-clavier d'une octave ;
* mise en page : bandeau de titre avec indicateur de pages.

Applet de démonstration à quatre pages (polices ; contrôles ; courbes simulées et animées ; séquenceur et
accordeur), encodeur gauche pour la page, droit pour la valeur ; chaque page sert aussi de mesure du coût de dessin.
Inspiration seulement : le code de dessin de S&C (dans son loader) n'est pas dans le dépôt, son API (MIT) n'est que
consultée.

## 4. Ce que nous avons décidé, et pourquoi

Choix de l'utilisateur (2026-11-17) : **G1**, **applet dans le menu, toujours** (comme Diagnostic et Scope, sur les
trois backends : vitrine des primitives et banc de mesure), **tout le catalogue** en première version.
Trigonométrie entière (règle du dépôt). Le plan des lots est `docs/plan/etape-5-gfx.md`.

## 5. Ce qui nous ferait changer d'avis

* Une primitive qui coûte trop au tick sur le module (mesure `render` par page) : chemin rapide dédié, comme pour
  le texte, ou dessin réparti sur plusieurs ticks.
* Un contrôle que deux applets veulent différemment : il devient paramétrable ou retourne dans l'applet ; `gfx` ne
  garde que ce qui sert à plusieurs.
* Le besoin d'un rendu anticrénelé ou en niveaux de gris : hors de portée d'un écran monochrome, à ne pas imiter.

## Révision (2026-11-17, lot 5a) — lignes et cercles écrits ici, pour pouvoir inverser

Le § 3 prévoyait la ligne quelconque et le cercle par `embedded-graphics`. Ils sont écrits dans `gfx` (Bresenham,
cercle du point milieu, disque en lignes horizontales), parce que toute forme doit pouvoir **inverser** ce qu'elle
couvre (`Ink::Invert`, l'aiguille d'un potentiomètre sur une zone allumée), ce qu'`embedded-graphics` ne fait pas ;
chaque point est encré une seule fois, si bien qu'inverser deux fois rend l'image d'origine (testé). Les formes
droites (`fill_rect`, `hline`, `vline`, `rect`) écrivent des octets par masque et allument les mêmes pixels
qu'`embedded-graphics` (testé, bords et coupures compris) ; banc `gfx/` : un rectangle de 100 × 40, 4,36 → 0,43 µs
sur l'hôte (indicatif). Sinus entier au 1/256 de tour, à une unité près du sinus réel (`tests/trig.rs`).

## Révision (2026-11-17, lots 5b et 5c) — cinq pages, sorties simulées

* **Cinq pages** au lieu de quatre : séquenceur et accordeur ne tenaient pas ensemble sous le bandeau (128 × 53
  pixels utiles). Polices, potentiomètres, courbes, séquenceur, mesures.
* **Les sorties portent les signaux simulés** (A le LFO, B la porte euclidienne, C le premier potentiomètre, D
  l'aiguille de l'accordeur) : un scope sur le module les suit.
* **`AppId::Gfx` en dernier** : un réglage de démarrage enregistré avant elle se relit (énumération sérialisée).
* Contrôles : `gfx::controls`, `gfx::plots` (`Area`), `gfx::patterns`, valeurs sur une plage par `Ratio` ;
  `polyline` encre une fois chaque point partagé (inversible). Écrans dorés : `gfx_*.scn`, un par page. Coût de
  dessin par page sur l'hôte (criterion, indicatif) : 1,5 à 3,1 µs, l'ordre du Diagnostic (2,6 µs).

## Révision (2026-11-17) — `embedded-graphics` devient une dépendance de test

Proposition de l'utilisateur, une fois `gfx` et `text` en place : nos primitives partout, `embedded-graphics`
gardé comme référence, donc seulement pour les tests.

* `oc-core` n'en dépend plus à l'exécution : `gfx::Point` (mêmes conventions), `text::Alignment` et
  `text::Baseline` (mêmes variantes), et `Ink` comme couleur du texte (allumé, éteint, et désormais inversé) ; les
  bandes de surbrillance du menu et du SETUP et le cadre du message de sauvegarde passent par `gfx::fill_rect` /
  `gfx::rect` ; le firmware d'essai `settings-stress` écrit par `text::draw`.
* `FrameBuffer` n'implémente plus `DrawTarget`. Les tests et le banc dessinent la référence par un adaptateur,
  `tests/common/mod.rs` (`Reference`), partagé avec `benches/engine.rs` ; les comparaisons de formes sont devenues
  des tests d'intégration (`tests/gfx.rs`), les tests de texte l'étaient déjà (`tests/fonts.rs`,
  `tests/diagnostic.rs`).
* Effet mesuré sur le firmware (`cargo xtask size`) : `.text` 114 932 → 113 980 octets (−952), `.rodata` 25 872 →
  25 536 (−336). `embedded-graphics` n'est plus compilé pour le firmware, le simulateur ni le module Rack.
* Une applet qui voudrait `embedded-graphics` le rajouterait derrière une feature, avec un `DrawTarget` pour
  `FrameBuffer` ; rien ne le demande aujourd'hui.
