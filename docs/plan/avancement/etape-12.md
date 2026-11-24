# Étape 12 — assignation des CV par la sonde, sous-panneau commun (avancement, terminée le 2026-11-24)

Décision : D-045, proposée puis adoptée le 2026-11-22 ; prolonge D-035 et D-036, étend D-043. Plan :
[etape-12-assignation-sonde.md](../etape-12-assignation-sonde.md). Branche `probe-cv-assign` (PR #17), le lot de la
pile sur `stack-depth` (PR #16) ; `CENTER` de la page dans le commit qui clôt l'étape.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| 12a | `cab6b5d` | D-045 proposée, plan de l'étape |
| 12b | `fc4c04c` | `modulation::slots` : `Modulations<N>` (assignations, CV, sous-menu ouvert), `slot_of` ; `Assign::check` ; trait `assigns` / `assigns_mut` / `assign_sweep`, `assign` / `set_assign` sur `dyn App` ; `TONE`, `RATCHET`, `SUPERSAW` branchées ; `tests/params.rs` |
| 12c | `06c97b8` | protocole 8 : `GetAssign`, `SetAssign`, `Reply::Assign` (l'entrée et la plage qui balaie le paramètre), un `Reply::Assign` après le `Spec` de chaque paramètre assignable dans `DescribeApp` ; client, `oc-web` |
| 12d | `0cc4889`, `681cd0d` | `cargo probe assign <app> <param> [champ=valeur…] [--center S]`, `CENTER` côté hôte (`apps::centered`) ; `app` montre l'entrée de chaque paramètre, alignée |
| 12e | `8b44f2b`, `ded4d88` | un seul sous-panneau CV dans la page, ouvert par le bouton de chaque réglage assignable de chaque app ; colonnes immobiles à l'ouverture |
| 12f | `e25b47d`, `e511640` (PR #16 pour la surveillance) | `DescribeApp` dans la pile principale : `send_input` et `send_screen` hors ligne, une copie du framebuffer en moins ; profondeur de la pile surveillée par `cargo xtask size` (`stack-depth`) |
| adoption | `1a6d35b`, `862161d` | D-045 adoptée ; `scripts/profile-supersaw.sh` assigne ses entrées par la sonde |
| 12g | ce commit | `CENTER` dans le sous-panneau de la page : flux à chaque tick pendant N s (5 par défaut), `oc_web::Readings` (les échantillons sur N s de l'horloge du module, comme `collect` de la CLI), `AppView::center_request` par le même `apps::centered` ; test contre le vrai serveur (triangle sur CV2, même ajustement que la CLI sur les mêmes échantillons, entrée immobile refusée) ; README, TESTING 9.1c, CHANGELOG |

## Mesures

* **Écrans de référence** identiques après 12b (`cargo test -p oc-sim --test scenarios` sans `UPDATE_SCREENS`).
* **Code** (différence de `cargo xtask size`) :
  * 12b : `.text` +696 o (`assign` / `set_assign` en méthodes par défaut du trait : +2 940 o, une copie par table
    d'app), `.rodata` +240 o ;
  * 12c : `.text` +6 336 o (le sérialiseur de `Reply` sorti de `Server::send`, −2 432 o, en une fonction de
    6 036 o ; `SetAssign` décodé dans `Usb::serve`, +1 458 o), `.rodata` +72 o : 167 Kio d'ITCM sur 192 ;
  * 12d, 12e, 12g : firmware inchangé ;
  * 12f : `.text` +236 o.
* **Pile** (12f) : chemin le plus profond de `DescribeApp` (`main`, `Usb::serve`, `describe`, `send_spec`, `send`)
  ≈ 16 260 o, plus ≈ 580 o pour l'interruption audio : au-delà des 16 Kio de `.stack`, le module plantait ;
  14 628 o après. Sur `master`
  au 2026-11-24 (avec le budget mémoire, D-038 révisée) : ≈ 11 824 o sur 16 384 (`main` 10 952 + trois
  interruptions 872), `.text` 156,3 Kio (35,7 Kio d'ITCM libres), `.rodata` 32,4 Kio.

## Essais sur le module

* CLI (TESTING 9.1b) : `cargo probe assign supersaw detune input=cv2 effect=add` appliquée, relue, `+CV2` à
  l'écran ; `cargo probe app` décrit les six applets ; `--center` sous un LFO : concluant.
* Page (TESTING 9.1c) : sous-panneau CV de `SUPERSAW`, `RATCHET` et `TONE`, champ par champ, refus hors bornes,
  mêmes champs que le sous-menu du module : concluant.

## Réserves

* `CENTER` de la page (12g) : essayé contre le serveur sur un moteur simulé, pas encore sur le module (TESTING 9.1c,
  dernier paragraphe). Il prend le flux à chaque tick (≈ 1 000 échantillons par seconde sur l'USB) et l'arrête
  ensuite, comme `--center` ; un flux lancé depuis la page s'arrête avec lui.
* `CENTER` côté hôte : la sonde lit les CV au rythme du flux, pas à celui du module ; une action `Center` sur le
  module si l'écart se voit (D-045, §5).
* Assignations non sauvegardées (réserve de l'étape 8) : `assigns()` est ce que les réglages sauveraient.
* Le sous-menu du module ne s'ouvre pas depuis la sonde.
