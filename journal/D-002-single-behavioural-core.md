# D-002 — Un seul cœur comportemental : `oc-core`, `no_std`, piloté par des traits de plateforme

| | |
|---|---|
| **Identifiant** | D-002 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `1524760` (workspace), `555f492` (traits, `Engine::tick`, applet diagnostic) |

## 1. Contexte

Le même comportement doit tourner à trois endroits : le Teensy 4.0, un simulateur natif et un module VCV Rack 2.
Le risque classique est d'écrire trois fois la logique (une par cible) et de découvrir sur le module ce que le
simulateur ne montrait pas. Il faut aussi que l'essentiel soit testable sans matériel, puisque le firmware lui-même
ne l'est pas (D-014).

## 2. Options envisagées

**A** — Logique dans le firmware, simulateur et VCV écrits à part en imitant le comportement. Le plus direct pour
le premier jalon ; trois implémentations divergent dès le deuxième applet.

**B** — Un crate `no_std` qui contient **tout** le comportement (conversions, anti-rebond, quadrature, applets,
rendu dans un framebuffer) et ne voit le monde qu'à travers six traits : `AnalogIn`, `AnalogOut`, `DigitalIn`,
`Controls`, `Clock`, `Display`. Chaque backend implémente les traits ; `Engine::tick` est le seul point d'appel.

**C** — Un RTOS ou framework (RTIC, Embassy) comme socle commun, avec un portage hôte. Plus d'outillage, et le
simulateur et VCV auraient dû émuler l'exécuteur.

## 3. Ce que l'assistant a proposé

**B**. Le plan de fondation (Junie) le liste en premier de ses « Key Decisions », marqué « validé » :
« `oc-core` ne connaît ni registre ni OS ; firmware, simulateur et VCV fournissent chacun une implémentation des
traits de plateforme. Une seule source de vérité comportementale. » Il fixe aussi `Engine::tick` comme « la
**seule** fonction appelée par les trois backends : c'est le point de convergence testable ».

## 4. Ce que nous avons décidé, et pourquoi

**B**, avec `#![no_std]` et `#![forbid(unsafe_code)]` sur `oc-core`. Le moteur est générique sur les six traits
(`Engine<A, O, D, C, K, S>`), pas sur des objets dynamiques : aucune allocation, monomorphisation sur la cible.

La frontière a tenu à l'usage :

* le menu d'applets (`a839a03`) n'a demandé **aucune** modification du firmware ni de l'ABI C — le commit le dit
  explicitement ;
* le simulateur réutilise le backend hôte `oc_core::testing` au lieu d'en écrire un (`84f9d86`), et l'ABI VCV aussi :
  ce que montrent la TUI et Rack est ce que les tests vérifient.

## 5. Ce qui nous ferait changer d'avis

* Si un comportement dépendait d'un contexte d'exécution que `tick()` ne sait pas exprimer — une ISR à cadence
  différente de l'UI, un DMA. C'est exactement la question de D-023, qui propose de couper `tick()` en
  `signal_tick` / `ui_tick` plutôt que de sortir de B.
* Si les backends se mettaient à fournir des informations que le matériel ne peut pas avoir : la promesse
  « le simulateur est un proxy du matériel » tomberait. C'est déjà le cas pour `is_patched` (D-019).

## Points ouverts (revue du 2026-11-16)

* `oc_core::testing` est un module **public et non conditionnel** qui sert de backend de production au simulateur et
  au module VCV. Le nom ment sur l'usage, et le module est compilé (puis éliminé) dans le firmware. À trancher :
  renommer (`host`) et/ou le placer derrière une feature `std`/`host`.
