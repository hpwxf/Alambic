# D-045 — L'assignation des CV par le trait `App` : un mécanisme commun, atteint par la sonde

| | |
|---|---|
| **Identifiant** | D-045 |
| **Date** | 2026-11-22 |
| **Auteur** | Pascal Havé |
| **Statut** | proposée |
| **Commits** | ce commit (ADR et plan) ; implémentation aux lots 12b à 12e |
| **Relations** | prolonge D-035 (le sous-menu d'assignation) et D-036 (sa mécanique dans `oc_core::modulation`) ; étend le modèle de D-043 (trait `App`, protocole de la sonde) ; prépare la persistance des assignations (réserve de l'étape 8, D-028) |

## 1. Contexte

La sonde règle les paramètres d'une app par le trait `App` (`param_specs`, `get`, `set`, D-043) ; `ParamSpec::assignable`
dit déjà qu'un paramètre prend une CV. Mais l'assignation elle-même (`modulation::Assign` : entrée, `SCALE`, `SHIFT`,
`MIN` / `MAX`, effet) ne se lit ni ne se règle que sur le panneau : la page web montre une marque CV sans rien
derrière.

Sur le module, le sous-menu est commun (`modulation::editor::AssignEditor`, D-036), mais l'état qui l'entoure est
recopié dans chacune des trois apps qui prennent des CV (`TONE`, `RATCHET`, `SUPERSAW`) : un tableau
`assigns: [Assign; N]`, un `assigning: Option<AssignEditor>`, les dernières CV, les accesseurs `assign()`,
`assigning()`, la lecture d'une entrée, l'ouverture, `follow`, `observe` et le dessin du sous-menu.

Constat : dans les trois apps, l'emplacement d'un `Assign` est **le rang du paramètre parmi les `assignable`** de ses
`PARAM_SPECS` (`RATCHET` : `COUNT` 0, `FIRST` 1, `SHAPE` 2, `PROB` 3, les faces `GATE` et `DIVISION` n'en prenant
pas). L'index d'un paramètre suffit donc à nommer son assignation, sans table propre à chaque app.

## 2. Options envisagées

* **A — par app, des actions de sonde.** Chaque app expose ses assignations comme des paramètres de plus
  (`NOTE CV`, `NOTE SCALE`…). Rien à changer au protocole, mais six paramètres de plus par réglage assignable, des
  noms inventés par app, et la page ne sait pas qu'ils forment un sous-panneau.
* **B — au trait, emplacement par le rang parmi les `assignable`.** Le trait gagne `assigns()` / `assigns_mut()`
  (la tranche des assignations de l'app) et `assign_sweep(param)`, la plage qui balaie le paramètre ;
  `assign(param)` / `set_assign(param, assign)` s'en déduisent, une fois pour toutes, en méthodes de `dyn App` :
  en méthodes par défaut du trait, chaque app en aurait une copie dans sa table (≈ 1,9 Kio de `.text` pour sept
  apps, mesuré en 12b). L'état et l'aiguillage du sous-menu sortent dans un
  type commun de `modulation`. Le protocole gagne `GetAssign` / `SetAssign` et une réponse `Assign`.
* **C — un registre de modulations dans le moteur**, hors des apps, que le moteur appliquerait avant `update`.
  C'est la forme d'un routage interne à venir, mais l'effet d'une CV sur une valeur (`MUL` sur un compte, `SET` sur
  une note) est propre à l'app (D-036) : le moteur devrait connaître ce que chaque paramètre veut dire.

## 3. Ce que l'assistant a proposé

B. L'app reste propriétaire de ses assignations et de ce qu'elles font à ses valeurs ; le trait n'en voit qu'une
tranche et une plage. Une valeur venue de la sonde passe les mêmes bornes que le panneau (`Assign::check`, comme
`ParamSpec::check`). `CENTER`, qui a besoin des lectures vues, se fait côté hôte : la sonde lit les CV dans `Stream`
et appelle la même fonction `Assign::center` que le module, avec la plage `assign_sweep` ; le module n'a pas à garder
de fenêtre de lectures hors du sous-menu. `DescribeApp` envoie l'assignation de chaque paramètre assignable après son
`Spec`, pour que la page n'ait qu'un échange à faire. La page bâtit un seul sous-panneau, instancié sous chaque réglage
assignable.

## 4. Ce que nous avons décidé, et pourquoi

B, à confirmer par les lots 12b à 12e : passe à `adoptée` quand les écrans de référence restent identiques après la
mise en commun (12b) et que la CLI règle une assignation sur le module (12d). Le plan est
`docs/plan/etape-12-assignation-sonde.md`.

## 5. Ce qui nous ferait changer d'avis

* Une app dont l'assignation ne suit pas le rang des `assignable` (deux paramètres partageant une entrée, un
  paramètre à deux entrées) : il faudrait une table explicite paramètre → emplacement.
* Le routage entre apps (D-043, à terme) qui demanderait qu'une modulation vienne d'ailleurs qu'une entrée CV : C
  redeviendrait d'actualité, l'`Assign` gagnant une source plus générale.
* `CENTER` côté hôte trop imprécis (la sonde échantillonne `Stream` moins souvent que le module ne lit ses CV) :
  une action `Center` sur le module, avec une fenêtre gardée par emplacement.
