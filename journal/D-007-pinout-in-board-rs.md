# D-007 — Brochage centralisé dans `board.rs`, dérivé du firmware de référence

| | |
|---|---|
| **Identifiant** | D-007 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `ec140ed` (table et constantes), `a8f3560` (configuration des pastilles : pull-up 22 kΩ + hystérésis) |

## 1. Contexte

Un mauvais firmware ne peut pas « briquer » un Teensy 4.0 : le chargeur HalfKay vit sur une puce séparée et le
bouton PROGRAM le restaure toujours. Le seul risque matériel réel est **électrique** : configurer en sortie une
broche que la carte pilote en entrée (une entrée analogique CV, par exemple). Le brochage exact de la variante TLM
Audio n'est pas publié ; il faut le dériver.

## 2. Options envisagées

**A** — Nommer les broches là où on en a besoin (dans chaque module du firmware). Habituel, mais une revue de
sécurité doit alors lire tout le firmware.

**B** — Une **table unique**, documentée et revue, dans `crates/oc-firmware/src/board.rs` ; partout ailleurs, on
raisonne en types `oc-core` (`CvChannel`, `TriggerChannel`, `Button`). Brochage dérivé de `OC_gpio.cpp` /
`OC_gpio.h` du firmware de référence (branche « old hardware », orientation par défaut), le Teensy 4.0 étant
compatible broche à broche avec le 3.2 sur 0–23.

## 3. Ce que l'assistant a proposé

**B**. Le plan de fondation en fait la mitigation du premier risque (« Brochage exact du variant TLM Audio
inconnu ») et la note de sécurité matérielle conclut : risques « traités par une table de pinout unique et
centralisée dans `oc-firmware/src/board.rs`, revue avant le premier flash, et non par l'outil d'upload ».

## 4. Ce que nous avons décidé, et pourquoi

**B**, érigé en règle dure dans `AGENTS.md` et `CLAUDE.md` : « `board.rs` is the **only** file allowed to name a
Teensy pin ». Garde-fous : canaux ADC résolus depuis les types de pastille (`imxrt-iomuxc`) et recopiés dans
`CV_ADC_CHANNELS` uniquement pour une assertion de cohérence ; l'ordre `CV1..CV4` (broches 19, 18, 20, 17) n'est
pas l'ordre des broches, et la table le signale.

`a8f3560` a ajouté la configuration électrique des entrées numériques (`DIGITAL_INPUT_CONFIG` : pull-up 22 kΩ,
hystérésis) : sans elle, `Port::input` laissait les lignes flotter et ni les encodeurs ni les boutons ne
répondaient sur le module.

## 5. Ce qui nous ferait changer d'avis

* Une seconde carte (Teensy 4.1) : la table deviendrait un trait ou un module par carte.
* Une erreur de brochage constatée au niveau 9 : il faudrait une vérification croisée automatique (table ↔ appels
  de `main.rs`) plutôt qu'une relecture.

## Points ouverts (revue du 2026-11-16)

* **La règle n'est pas respectée par le code.** `board.rs` *documente* la table, mais c'est `main.rs` qui réserve
  les broches (27 occurrences de `pins.pN` : `pins.p19`, `pins.p22`, `pins.p10`…). Rien ne vérifie que la table et
  `main.rs` concordent. Deux sorties possibles : déplacer la réservation des broches dans `board.rs` (une fonction
  qui consomme `Pins` et rend des types de haut niveau), ou reformuler la règle (« seuls `board.rs` et la séquence
  de câblage de `main.rs` nomment des broches, et ils doivent concorder »). Le premier est préférable : c'est le
  seul qui rend la règle vérifiable.
* La broche 12 (`LPSPI4 SDI`, entrée) est réservée par `main.rs` mais **absente de la table** de `board.rs`. Elle est
  configurée en entrée, donc sans danger électrique, mais c'est précisément le genre d'écart que la table doit
  rendre impossible.
* La provenance est un firmware écrit pour le Teensy 3.2 ; la carte TLM T4 n'a été vérifiée que partiellement sur
  le module (OLED, encodeurs, boutons : `3f50a42`, `a8f3560`). Entrées CV et triggers : niveau 9 à faire.
