# D-004 — Chaîne croisée : script de lien généré par `imxrt-rt`, ni `memory.x` ni `flip-link`

| | |
|---|---|
| **Identifiant** | D-004 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `1524760` (toolchain, `.cargo/config.toml`, `MEMORY.md`), `6f81467` (vérifications de disposition) |

## 1. Contexte

Le plan de fondation (FR1) demandait une toolchain épinglée, un `memory.x` écrit à la main décrivant
ITCM / DTCM / OCRAM / FLASH, `flip-link` pour la protection de pile et une sortie `.hex`. L'i.MX RT1062 démarre sur
des sections spécifiques (FlexSPI Configuration Block, Image Vector Table) qu'il faut placer exactement.

## 2. Options envisagées

**A** — Le plan : `memory.x` maison + `flip-link`. Habituel sur Cortex-M, mais `teensy4-bsp` (feature `rt`)
génère déjà, via `imxrt-rt::RuntimeBuilder`, un script complet `t4link.x` (régions **et** sections de boot) : un
`memory.x` serait ignoré ou entrerait en conflit. `flip-link` exige une région `MEMORY { RAM }` et échoue
(« MEMORY.RAM not found ») contre les régions générées.

**B** — Utiliser le script généré (`-Tt4link.x` dans `.cargo/config.toml`), documenter la disposition dans
`MEMORY.md`, et obtenir la protection de pile autrement : `imxrt-rt` place `.stack` tout en bas de la DTCM
(`0x2000_0000`), sous `.vector_table` / `.data` / `.bss` ; un débordement sort de la DTCM et faute immédiatement.

Et, pour les outils binaires : `objcopy` / `size` du système, ou ceux du sysroot Rust (`llvm-tools`).

## 3. Ce que l'assistant a proposé

**B**, en écart assumé au plan : le message de `1524760` dit « Documented deviations from the plan in
crates/oc-firmware/MEMORY.md: no hand-written memory.x (the BSP generates the full layout) and no flip-link
(incompatible with the generated regions, and redundant since imxrt-rt already places the stack at the bottom of
DTCM) ». L'assistant a aussi choisi les outils LLVM localisés dans `rustc --print sysroot`.

## 4. Ce que nous avons décidé, et pourquoi

**B**. Précisions :

* l'invariant « pile sous la table des vecteurs, à la base de la DTCM » n'est pas du folklore : `cargo xtask size`
  le vérifie (élément `stack-below-vector-table` de la liste de contrôle, `6f81467`) et échoue s'il est rompu ;
* **`RUSTFLAGS` ne doit jamais être posé en variable d'environnement** : il *remplace*
  `target.thumbv7em-none-eabihf.rustflags` et fait sauter `-Tt4link.x` sans erreur claire. La CI refuse les
  avertissements par `-D warnings` en ligne de commande ;
* outils LLVM du sysroot (`xtask/src/llvm.rs`) : la build ne dépend pas des binutils de la machine ;
* `rust-toolchain.toml` épingle le **canal** `stable` et les composants (`rust-src`, `llvm-tools`, la cible), pas
  une version (voir D-017).

## 5. Ce qui nous ferait changer d'avis

* Si `imxrt-rt` changeait sa disposition (pile ailleurs qu'en bas de DTCM) : la liste de contrôle de
  `cargo xtask size` le dirait, et il faudrait soit configurer `RuntimeBuilder`, soit une garde de pile explicite
  (MPU).
* Si l'ajout d'un DMA (D-023) demandait des tampons non cachés en OCRAM, le script généré devrait être paramétré
  (sections dédiées) — c'est le moment où un contrôle fin du lien pourrait justifier de reprendre la main.
