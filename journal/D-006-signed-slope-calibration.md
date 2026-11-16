# D-006 — Calibration affine à pente signée ; étages inverseurs = pente négative ; constantes provisoires

| | |
|---|---|
| **Identifiant** | D-006 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `555f492` (`calibration.rs`), `ec140ed` (constantes et assertions de `board.rs`) |

## 1. Contexte

Les deux étages analogiques du module inversent : le firmware de référence calcule `offset - raw` pour les entrées
et `MAX_VALUE - value` pour les sorties. Il faut aussi, tôt ou tard, une calibration par exemplaire. La question :
où et comment exprimer l'inversion, et avec quelles valeurs démarrer sans avoir mesuré le module.

## 2. Options envisagées

**A** — Inverser dans le code (`DAC_MAX - code`) au moment de l'écriture, comme la référence. Simple, mais
l'inversion devient un détail caché du pilote, et une calibration mesurée devra la reproduire.

**B** — Une conversion affine `(zéro, pente)` dont la **pente est signée** : l'inversion devient une donnée
(pente négative), pas du code ; une calibration par exemplaire ne change que les deux nombres.

## 3. Ce que l'assistant a proposé

**B**. Message de `555f492` : « affine ADC and DAC conversions with a signed slope, so an inverting front end or
output stage is a data change, not a code change; sub-code slope resolution keeps the truncation error below one
converter step ». `ec140ed` ajoute les assertions de compilation sur le signe des pentes.

## 4. Ce que nous avons décidé, et pourquoi

**B**. `CvInputCalibration { zero_code, nanovolts_per_code }` et `CvOutputCalibration { zero_code,
millicodes_per_volt }` (résolution sous le code). Dans `board.rs`, les constantes sont les valeurs nominales avec
la pente niée, et un bloc `const _: () = { assert!(…) }` refuse à la compilation une pente positive, un zéro hors
plage ou une entrée qui ne lirait pas le code 0 comme la tension la plus haute. Le commentaire de provenance de
`board.rs` interdit de « corriger » le signe sans le relire.

Les constantes sont **provisoires** : dérivées du code source de référence, pas mesurées. Elles ne sont ni
persistées ni réglables (le plan de fondation exclut explicitement la calibration persistante).

## 5. Ce qui nous ferait changer d'avis

* Si la mesure au niveau 9 de `TESTING.md` montrait une non-linéarité notable (étage d'entrée saturant avant les
  bornes), une affine ne suffirait plus : table par segments.
* Si les quatre canaux d'un même module divergeaient au-delà de la tolérance, la calibration par canal (déjà prévue
  côté sortie : `CV_OUTPUT_CALIBRATIONS` par canal) devrait s'étendre aux entrées, et il faudrait une persistance
  (EEPROM émulée en flash).

## Points ouverts (revue du 2026-11-16)

* La plage d'entrée retenue (−5 V…+5 V, zéro à mi-échelle) n'est pas sourcée dans le dépôt ; sur l'O&C d'origine,
  l'étage d'entrée n'est pas forcément symétrique. À vérifier au niveau 9 avant d'écrire un applet qui dépend de la
  valeur absolue d'une entrée (quantizer).
* Aucune procédure de calibration ni de stockage n'existe ; tant que c'est le cas, toute précision 1 V/oct annoncée
  est théorique.

## Révision (2026-11-16, D-027) — calibration mesurée, persistée, modèle de Phazerville

Les deux signaux de changement d'avis du § 5 et les deux points ouverts sont pris en compte d'un coup :

* **sorties** : la conversion affine devient une **table de 11 points par octave et par canal** (−3 V…+7 V,
  interpolation linéaire), le modèle de Phazerville ; la pente signée reste l'expression de l'inversion entre deux
  points ;
* **entrées** : affine conservée, **zéro par canal**, pente commune. Le zéro nominal est à 2/3 de l'échelle 12 bits
  (≈ 2730, `OC_calibration.h` de Phazerville), pas à mi-échelle : la plage −5 V…+5 V centrée était fausse ;
* les valeurs sont **mesurées** par un assistant sur le module et **persistées** (D-028) ; les constantes de
  `board.rs` ne sont plus que les valeurs d'usine ;
* la conversion quitte les backends pour `oc-core` (D-027, option A).
