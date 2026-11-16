# D-005 — Unité de signal interne : millivolts `i32`, flottant seulement à la frontière VCV

| | |
|---|---|
| **Identifiant** | D-005 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `555f492` (`platform::MilliVolts`, calibration), `40e8c3f` (conversion en `float` dans le module Rack) |

## 1. Contexte

Les signaux traversent un ADC 12 bits en entrée et un DAC 16 bits en sortie, sur des plages bipolaires
(entrées −5 V…+5 V, sorties −3 V…+6 V dans `calibration.rs`). Il faut une unité commune aux conversions, aux
applets, aux tests et aux trois backends. Le Cortex-M7 a un FPU simple et double précision ; VCV Rack parle en
volts `float`.

## 2. Options envisagées

**A** — `f32` en volts partout. Naturel pour du DSP, aligné sur Rack ; mais les tests d'égalité deviennent des tests
de tolérance, et la reproductibilité bit à bit (exigée du simulateur) dépend de l'ordre des opérations.

**B** — Entier signé en **millivolts** (`i32`), conversions affines exactes vers les codes ADC / DAC (D-006), `f32`
uniquement dans le C++ du module Rack.

**C** — Codes bruts (ADC / DAC) dans les applets. Aucune conversion, mais chaque applet doit connaître le matériel.

## 3. Ce que l'assistant a proposé

**B**. Plan de fondation, Key Decision 6 (validée) : « Unité de signal interne : millivolts `i32`, pas de flottants
dans le chemin critique du firmware ; conversion en `f32` uniquement à la frontière VCV. »

## 4. Ce que nous avons décidé, et pourquoi

**B**. Le millivolt est plus fin que le pas de l'ADC (≈ 2,4 mV sur 10 V / 4096) et plus grossier que celui du DAC
(≈ 0,14 mV sur 9 V / 65536) ; la calibration garde une résolution sous le code (D-006) pour que la troncature reste
sous un pas de convertisseur. L'ABI C reste en `int32_t` millivolts ; seul `Alambic::process()` convertit vers et
depuis les volts de Rack. Les tests comparent des entiers, et `proptest` vérifie les allers-retours sur toute la
plage.

## 5. Ce qui nous ferait changer d'avis

* Un applet audio (le supersaw de `docs/supersaw/`) : un oscillateur à PolyBLEP, un filtre ou un suréchantillonnage
  s'écrivent naturellement en `f32` et le M7 les exécute en matériel. Le signal **de contrôle** pourra rester en mV,
  mais il faudra décider où vit la frontière entier / flottant dans `oc-core` au lieu de l'improviser applet par
  applet. Signal : le premier applet qui convertit en `f32` en interne.
* Si un besoin de précision sous le millivolt apparaissait côté sortie (1 V/oct : 1 mV ≈ 1,2 cent), il faudrait
  passer en dixièmes de millivolt ou en codes DAC pour l'accord fin. À mesurer au niveau 9 de `TESTING.md`.
