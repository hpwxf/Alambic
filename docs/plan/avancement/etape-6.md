# Étape 6 — premier applet musical : Ratchet (avancement, terminée le 2026-11-24)

Décisions : D-032 (l'applet, adoptée, révisée), D-035 (le sous-menu d'assignation des entrées, adoptée, révisée),
D-036 (sa mécanique dans `oc_core::modulation`, adoptée). Plan : [etape-6-ratchet.md](../etape-6-ratchet.md).

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| plan | `3009f84` | D-032 proposée, plan de l'étape |
| 6a, 6b, 6c | `19e4f3e` (PR #2) | `oc_core::fixed` (`log2`, `exp2` en 16.16) ; `ratchet::burst` (`SHAPE` signé, placements `FIRST` / `SPAN`, entrées, bornes), `ratchet::tempo` ; `RatchetApp`, `AppId::Ratchet` : A copie TR1, B rafale, `PASSTHRU`, `PROB`, `SYNC`, `up` à la main ; deux pages, BPM, témoins, frise avec le repère du prochain trigger ; tests tick par tick, scénarios de référence |
| 6d | — | essayé sur le module, sur le chemin audio : correct ; `MODE FIRST` et `SPAN` gardés tous les deux |
| 6e | `c7257c7` (PR #5), `6b10169` (PR #6) | ascenseur à gauche des réglages, frise plus basse qui suit la rafale en cours (révision de D-032) ; sous-menu d'assignation par réglage : entrée, lecture et bornes vues, `SCALE`, `SHIFT`, `MIN` / `MAX`, effet `SET` / `ADD` / `MUL`, `CENTER` (D-035) |
| 6f | `0b5a9ee` | la mécanique du sous-menu sortie dans `oc_core::modulation` (`Assign`, `Effect`, `AssignEditor`, `Param`), `RATCHET` branchée dessus ; écrans de référence identiques sans régénération ; `tests/modulation.rs` (D-036) |
| suites | `6678f48`, `7b05514`, `2d3e2e8`, `3adb8f7` | ascenseur dans `gfx::controls` ; `SHAPE` décrit comme une courbe, sans unité (révision de D-032) ; état du sous-menu commun aux trois apps à CV et assignation par la sonde à l'étape 12 (D-045) |

## Essais sur le module

* 6d (TESTING 9.3c, première partie) : TR1 sur une horloge, rebouclage A → TR2, B → TR3, `cargo probe triggers` :
  comptes et espacements conformes ; les triggers de 5 V passent le seuil des entrées (risque 3 du plan levé).
* 6e (TESTING 9.3c en entier, 2026-11-24) : sous-menu (`SCALE`, `SHIFT`, `MIN` / `MAX`, effets), `CENTER` sous un
  LFO, ascenseur, frise de la rafale en cours, `SYNC` à `/4` : concluant.

## Mesures

Pas de relevé de coût propre à l'étape : `RATCHET` n'a pas été profilée à part (`cargo probe stats`), et son coût en
code n'a pas été isolé par différence de `cargo xtask size`.

## Réserves

* La seconde applet annoncée par le plan n'a jamais été décrite ; sortie de l'étape le 2026-11-24 (`TONE` et
  `SUPERSAW`, étapes 7 et 8, ont pris sa place), reportée dans `docs/plan/roadmap.md`.
* Quantification à 1 ms : sensible pour des intervalles de quelques millisecondes et des formes très accélérées
  (D-032 § 5).
* Assignations des entrées non sauvegardées (réserve de l'étape 8).
