# D-013 — Simulateur : TUI ratatui, horloge virtuelle déterministe, scénarios texte et écrans dorés

| | |
|---|---|
| **Identifiant** | D-013 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01, complétée jusqu'au 2026-09-03) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `84f9d86` (simulateur), `384cb2b` (glyphes octants), `baf70c7` (écrans régénérés), `d97db13` (AZERTY / QWERTY, compteurs de boutons), `a839a03` (touche `m`, `SETTLE_TICKS`), lot 3a (écrans dorés en braille quelle que soit la feature) |

## 1. Contexte

D-012 fait du simulateur le harnais quotidien. Il doit permettre de jouer avec le module sans matériel, et de
transformer un défaut trouvé à la main en test commité. Il doit aussi être **déterministe bit à bit** (exigence
non fonctionnelle du plan).

## 2. Options envisagées

Pour l'interface : **A** — GUI (egui, fenêtre) ; **B** — TUI dans le terminal (ratatui), écran 128×64 rendu en
caractères braille (64×16 cellules, chaque pixel visible).

Pour le temps : horloge murale, ou horloge **virtuelle** à période fixe avec des politiques de pas (pause, temps
réel, ×50) qui ne changent jamais ce que voit le moteur.

Pour les tests : assertions dans le code Rust, ou **scénarios texte** éditables à la main (`*.scn`) avec un
instantané d'écran doré (`*.screen`) régénéré par `UPDATE_SCREENS=1`.

## 3. Ce que l'assistant a proposé

**B**, horloge virtuelle et scénarios texte (`84f9d86`), en réutilisant le backend hôte de `oc_core::testing`
plutôt qu'en en réécrivant un : « what the UI shows is exactly what the core test suite asserts on ». `ratatui`
limité au backend `crossterm` pour garder un arbre de dépendances raisonnable.

## 4. Ce que nous avons décidé, et pourquoi

**B**, horloge virtuelle, scénarios texte + écrans dorés, avec un garde-fou « aucun scénario non testé ». Ajouts
ultérieurs : glyphes **octants** Unicode derrière la feature `octant` (`384cb2b`), bascule AZERTY / QWERTY et
panneau d'aide (`d97db13`), touche `m` pour l'accord du menu (`a839a03`). Le format de scénario n'a pas changé :
l'accord est deux événements `button` au même tick.

## 5. Ce qui nous ferait changer d'avis

* Si D-023 coupe `tick()` en deux cadences, le simulateur devra choisir entre continuer d'appeler `tick()`
  (comportement composé) et simuler les deux cadences — décision à prendre avec D-023, pas après.
* Si les écrans dorés devenaient trop fragiles (chaque changement de police casse tout), il faudrait des
  assertions sur des régions plutôt que sur l'écran entier.

## Révision (2026-11-16, lot 3a)

Les écrans dorés sont désormais **toujours rendus en braille**, quelle que soit la feature `octant`.
`oc_sim::braille` expose le jeu de glyphes en valeur (`Glyphs::Braille`, `Glyphs::Octant`, `Glyphs::DEFAULT` choisi
par la feature) et `render_with` ; la TUI suit la feature, `tests/scenarios.rs` force `Glyphs::Braille`. Les
`*.screen` existants ne changent pas. Pourquoi plutôt que retirer `--all-features` de la CI : un test doré ne doit pas
dépendre d'une feature de présentation, et la CI doit continuer à compiler toutes les features (D-017).

## Points ouverts (revue du 2026-11-16)

* ~~**La CI est rouge sur `master`.**~~ **Traité** (révision ci-dessus). La CI lance `cargo test --all-features` ; avec la feature `octant`, le rendu du
  simulateur passe en octants alors que les `*.screen` sont en braille, et **5 des 9 scénarios échouent**
  (`cv_passthrough`, `trigger_burst`, `encoder_offset`, `mode_cycle`, `app_menu`). Sans features, les 9 passent.
  Constaté le 2026-11-16. Correctif : soit la comparaison dorée force le braille quelle que soit la feature, soit la
  CI teste sans `--all-features` (et la définition de « fait » de D-001 s'aligne). Le premier est préférable : un
  test doré ne doit pas dépendre d'une feature de présentation.
* Le commit `baf70c7` (« Update simulation screen tests for consistent rendering ») porte une date d'auteur
  (2026-08-19) antérieure à son parent `83be36f` (2026-08-23) : historique réécrit. Sans conséquence technique, mais
  la date d'un écran doré n'est pas fiable pour dater un changement de rendu.
