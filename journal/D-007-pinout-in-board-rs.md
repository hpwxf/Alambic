# D-007 — Brochage centralisé dans `board.rs`, dérivé du firmware de référence

| | |
|---|---|
| **Identifiant** | D-007 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `ec140ed` (table et constantes), `a8f3560` (configuration des pastilles : pull-up 22 kΩ + hystérésis), lot 3a (réservation des broches dans `board.rs`, test de garde) |

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

## Révision (2026-11-16, lot 3a) — la règle devient vraie et vérifiée

La réservation des broches quitte `main.rs` pour `board.rs`. `board::split(Pins) -> BoardPins` consomme le jeu de
broches du BSP et rend des **groupes nommés par signal** (`cv`, `triggers`, `spi`, `dac`, `oled`, `panel`,
`led_sck`) dont les champs sont privés ; seule la méthode `claim` de chaque groupe en tire des périphériques
(`AnalogInput`, `Input` avec pull-up, `Output`), dans la direction que dit la table. Les broches inutilisées restent
dans `Pins` et sont détruites avec lui. `with_pullup` et `DIGITAL_INPUT_CONFIG` sont désormais privés à `board.rs` ;
`boot_led` reçoit `board::LedSckPin` au lieu de nommer `P13`. La broche 12 (`SPI MISO`, entrée, réclamée par LPSPI4
seulement parce que son jeu de broches l'exige) entre dans la table.

Vérification : `xtask/tests/pin_rule.rs` échoue si un autre fichier de `crates/oc-firmware/src/` contient, hors
commentaire, un jeton de broche du BSP (`P13`, `p13`). Le test tourne sur l'hôte, donc dans `cargo test` et en CI,
bien que `oc-firmware` n'y soit pas compilé. Il ne vérifie pas que la table en commentaire concorde avec `split` :
les deux sont maintenant à quelques lignes l'un de l'autre, dans le même fichier, ce qui suffit à la relecture.

Comportement inchangé, sauf l'ordre des réservations sur GPIO2 : les deux lignes du DAC (CS et reset) sont prises
ensemble, avant celles de l'OLED, au lieu du reset après l'OLED. Le reset du DAC reste à l'état bas (actif) jusqu'à
son relâchement explicite, comme avant ; il l'est seulement quelques microsecondes de plus.

## Points ouverts (revue du 2026-11-16)

* ~~**La règle n'est pas respectée par le code.**~~ **Traité** (révision ci-dessus). `board.rs` *documente* la table, mais c'est `main.rs` qui réserve
  les broches (27 occurrences de `pins.pN` : `pins.p19`, `pins.p22`, `pins.p10`…). Rien ne vérifie que la table et
  `main.rs` concordent. Deux sorties possibles : déplacer la réservation des broches dans `board.rs` (une fonction
  qui consomme `Pins` et rend des types de haut niveau), ou reformuler la règle (« seuls `board.rs` et la séquence
  de câblage de `main.rs` nomment des broches, et ils doivent concorder »). Le premier est préférable : c'est le
  seul qui rend la règle vérifiable.
* **Traité** (révision ci-dessus). La broche 12 (`LPSPI4 SDI`, entrée) était réservée par `main.rs` mais **absente de la table** de `board.rs`. Elle est
  configurée en entrée, donc sans danger électrique, mais c'est précisément le genre d'écart que la table doit
  rendre impossible.
* La provenance est un firmware écrit pour le Teensy 3.2 ; la carte TLM T4 n'a été vérifiée que partiellement sur
  le module (OLED, encodeurs, boutons : `3f50a42`, `a8f3560`). Entrées CV et triggers : niveau 9 à faire.
