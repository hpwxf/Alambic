# D-014 — Stratégie de test : tout sur l'hôte, firmware sans tests, bus simulés, niveau 9 manuel

| | |
|---|---|
| **Identifiant** | D-014 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `555f492` (84 tests `oc-core`, proptest, criterion), `ec140ed` (bus simulés de `oc-drivers`, `xtask/tests`), `40e8c3f` (`TESTING.md`, tests d'ABI) |

## 1. Contexte

Trois choses à tester : le comportement (D-002), les protocoles de périphériques (D-003) et l'outillage qui touche
au module (validation avant flash, D-016). Le câblage de la carte, lui, ne s'exécute que sur la cible.

## 2. Options envisagées

**A** — Tests embarqués (`defmt-test`, `embedded-test`) sur la cible, pour le firmware aussi. Teste le vrai
binaire, mais demande un module branché et une sonde, donc hors CI.

**B** — Tout le fardeau sur l'hôte : `oc-core` via `oc_core::testing::mock_engine` à travers `Engine::tick` ;
`oc-drivers` contre des bus `embedded-hal` qui **enregistrent** les octets écrits ; `xtask::validate` comme
fonction pure sur des octets, testée sur des ELF synthétiques ; **aucun** test dans `oc-firmware` ; une checklist
manuelle (« niveau 9 ») pour le module réel.

## 3. Ce que l'assistant a proposé

**B**. Plan de fondation, § Testing : quatre niveaux du moins coûteux au plus coûteux, et « Pas de test unitaire
dans `oc-firmware` : sa couverture vient de `oc-core` et du smoke test Renode ». `TESTING.md` (`40e8c3f`)
détaille neuf niveaux.

## 4. Ce que nous avons décidé, et pourquoi

**B**. Règles qui en découlent (`AGENTS.md`) : un nouveau comportement se teste **une fois** dans `oc-core`, jamais
par backend ; ne pas ajouter de test à `oc-firmware`, mais câbler le firmware sur une abstraction testée ;
`xtask::validate` reste pure (`validate_image(&[u8])`) et exposée par `xtask/src/lib.rs`. `proptest` couvre les
allers-retours de conversion sur toute la plage ; `criterion` est un garde-fou de régression (compilé en CI,
chronométré en local), pas une mesure matérielle. Le DAC8565 est vérifié octet pour octet contre la référence
(`0x10 | channel << 1`).

État au 2026-11-16 : ≈ 290 tests hôte, tous verts **sans** features ; voir D-013 pour l'échec avec
`--all-features`.

## 5. Ce qui nous ferait changer d'avis

* Un défaut qui ne vit que dans `oc-firmware` et se répète (ordre d'initialisation, configuration de pastille comme
  dans `a8f3560`) : il faudrait un test sur cible minimal, ou sortir davantage du câblage dans des types testables.
* D-023 : le temps réel (durée d'ISR, dépassements, contention SPI) ne se teste pas sur l'hôte. Il faudra un
  niveau de test « mesuré sur module » avec des critères chiffrés, pas seulement une checklist.

## Points ouverts (revue du 2026-11-16)

* Le mode de mise à jour simultanée du DAC8565 est « inféré de la datasheet » et marqué comme à vérifier depuis
  `ec140ed`. Aucun niveau de `TESTING.md` ne l'exerce explicitement.
