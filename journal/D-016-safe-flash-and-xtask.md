# D-016 — `cargo xtask` seul point d'entrée ; flash = validations puis `teensy_loader_cli`, sans sonde USB

| | |
|---|---|
| **Identifiant** | D-016 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01, complétée le 2026-08-02) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `1524760` (`xtask build|size|hex`), `ec140ed` (`xtask flash`, `validate.rs`), `6f81467` (vérifications de disposition), `2deca8a` (relais des features) |

## 1. Contexte

Construire, mesurer, empaqueter et flasher demande une suite d'étapes faciles à rater (cible, script de lien,
`objcopy`, HEX, chargeur). Le plan voulait que `cargo xtask flash` refuse tout binaire suspect avant d'écrire sur le
module, alors même que HalfKay rend le « brick » impossible (D-007).

## 2. Options envisagées

**A** — Scripts shell / `Makefile` + `teensy_loader_cli` à la main.

**B** — Un crate `xtask` (clap) comme **seul** point d'entrée : `build`, `size`, `hex`, `flash`, `vcv`. Le flash
valide l'ELF, imprime ses faits et un SHA-256, demande confirmation (`--yes` pour l'automatisation, `--dry-run` pour
valider seulement) et **délègue** l'écriture à `teensy_loader_cli` sans réimplémenter le protocole HID HalfKay.

**C** — Réimplémenter le protocole HalfKay en Rust (`hidapi`) : détection du Teensy, pas de dépendance externe.

## 3. Ce que l'assistant a proposé

**B**. Plan de fondation, Key Decision 4 (validée) : « `cargo xtask flash` en garde-fou, `teensy_loader_cli` en
exécutant ». Le plan listait sept vérifications, dont la détection du périphérique (VID `16C0`).

## 4. Ce que nous avons décidé, et pourquoi

**B**. Vérifications réalisées (`validate.rs`, toutes rapportées d'un coup) : classe ELF, endianness, `EM_ARM`, ABI
ARM, taille chargeable sous 1 984 Kio, segment FlexSPI Configuration Block, couverture des segments, table des
vecteurs et vecteur de reset, cible. `cargo xtask size` ajoute une liste de contrôle de disposition (D-004). Le
chargeur n'est **jamais** invoqué sur une image rejetée.

Écart au plan : **pas de détection USB**. `warn_about_device()` l'assume : « We deliberately avoid a USB/HID
dependency; without one we cannot probe for a Teensy, so the operator is always steered toward the button rather
than given a false "device found" signal. »

`2deca8a` relaie `--features` (variantes d'OLED) à toutes les sous-commandes, pour que le flash d'une variante
passe par la même validation.

## 5. Ce qui nous ferait changer d'avis

* Si l'on voulait flasher une image construite ailleurs (artefact de CI) : aujourd'hui `flash` reconstruit
  toujours depuis les sources, donc l'image flashée n'est pas celle que la CI a produite.
* Si `teensy_loader_cli` disparaissait des gestionnaires de paquets, C redeviendrait pertinent.

## Révision (2026-08-01, `ec140ed`)

Vérification n° 6 du plan (« Périphérique Teensy détecté (VID `16C0`) ») abandonnée, remplacée par un avertissement
permanent qui renvoie au bouton PROGRAM.

## Points ouverts (revue du 2026-11-16)

* La validation de l'**image réelle** (`validate_image` sur l'ELF produit) ne tourne qu'au flash : la CI construit,
  mesure et empaquette, mais ne lance pas `cargo xtask flash --dry-run`, qui ne demande pourtant ni module ni
  chargeur. Une régression du FCB ou de la table des vecteurs ne serait vue qu'au moment de flasher.
