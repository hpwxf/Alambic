# D-036 — Module partagé de modulation par une entrée CV (`oc_core::modulation`)

| | |
|---|---|
| **Identifiant** | D-036 |
| **Date** | 2026-11-18 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ce commit (ADR, extraction, `Ratchet` branchée dessus) |
| **Relations** | sort de `RATCHET` la mécanique de D-035 pour d'autres applets (D-021) ; `oc-core` seul porteur du comportement (D-002) ; écran par `gfx` / `text` (D-030) ; deuxième utilisatrice : `SUPERSAW` (révision de D-037) |

## 1. Contexte

D-035 a donné à `RATCHET` un sous-menu d'assignation des entrées : chaîne `borne(cv × SCALE + SHIFT, MIN, MAX)`,
effet `SET` / `ADD` / `MUL`, plage vue glissante, `CENTER`, cadre par-dessus l'écran, deux barres. Tout vit dans
`apps/ratchet/mod.rs`, mêlé aux réglages de la rafale. L'utilisateur demande (2026-11-18) d'en extraire la mécanique
pour la réutiliser dans d'autres applets.

## 2. Options envisagées

* **Un module `oc_core::modulation`** : les types et la logique sans écran (`Assign`, `Effect`, la plage vue), plus
  un éditeur (`AssignEditor`) qui suit le panneau et dessine le cadre ; l'applet ne fournit que ce qui dépend de son
  paramètre.
* **Un trait que chaque paramètre implémente** (nom, valeur, plage, application de l'effet) et un éditeur générique
  sur ce trait.
* **Ne rien extraire** avant une deuxième applet qui en ait besoin.

## 3. Ce que l'assistant a proposé

Le module, sans trait : l'applet passe à l'éditeur une description de son paramètre par valeur (`Param` : nom,
valeur affichée, plage de signal qui le balaie, positions sur sa barre) à chaque tick ou rendu. Un trait obligerait
chaque paramètre à être un objet, alors que dans `RATCHET` ce sont des champs d'une même structure (`Burst`) ; une
description par valeur se calcule à la demande et ne prête rien.

## 4. Ce que nous avons décidé, et pourquoi

La proposition.

* **`oc_core::modulation`** (`modulation/mod.rs`) : `Effect` et `UNIT_MV` (sortis de `ratchet::burst`), `Assign`
  (entrée, `SCALE`, `SHIFT`, `MIN` / `MAX`, effet ; `shape`, `reading`, `center`, `turn`), `AssignLine`, `multiplied`
  (l'effet `MUL` sur une valeur entière), `usable_range(sweep, effect)` qui dérive de la plage `SET` d'un paramètre
  (`sweep`) celles d'`ADD` (même largeur centrée sur 0 V) et de `MUL` (0 à 10 V) ; la plage vue (`Window`).
* **`modulation::editor`** : `AssignEditor`, l'état du sous-menu ouvert (champ choisi, plage vue) ;
  `follow(&mut Assign, input, sweep)` suit le panneau et dit s'il reste ouvert ; `observe` ajoute la lecture ;
  `render(frame, &Assign, cv, &Param)` dessine le cadre, les champs et les deux barres.
* **Ce qui reste à l'applet** : quels paramètres prennent une entrée et où ranger leurs `Assign` ; la plage `SET` de
  chacun (sa correspondance volts → valeur) ; l'application de l'effet à sa valeur (`count_from`, `time_from`, … dans
  `RATCHET`) ; le texte de la valeur et sa position sur la barre ; l'ouverture (clic droit sur l'écran principal) et
  le dessin de l'écran sous le cadre.
* **Comportement inchangé** : écrans dorés et tests de `RATCHET` identiques ; les tests de la mécanique passent dans
  `tests/modulation.rs`.

## 5. Ce qui nous ferait changer d'avis

* Une deuxième applet dont les paramètres ne tiennent pas dans `Param` (plusieurs barres, une valeur non scalaire) :
  élargir `Param`, ou passer au trait.
* Le besoin de persister les assignations (D-028) : `Assign` entre alors dans les réglages sérialisés.
