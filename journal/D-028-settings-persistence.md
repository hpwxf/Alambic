# D-028 — Persistance des réglages : briques standard de l'écosystème Rust embarqué, flash du Teensy 4.0

| | |
|---|---|
| **Identifiant** | D-028 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR) ; implémentation aux lots 4b (encodage, stockage) et 4e (flash) de l'étape 4 |
| **Relations** | **révise** D-002 (une capacité de plateforme de plus), D-003 (`oc-firmware` « sans `unsafe` »), D-006 (« non persistées ») ; sert D-026 et D-027 ; touche D-004 / D-016 (zone flash réservée, contrôlée par `xtask`) |

## 1. Contexte

Les réglages de D-026 et D-027 ne servent que s'ils survivent à une coupure. Le Teensy 4.0 n'a pas d'EEPROM : sa
flash FlexSPI de 2 Mio porte l'image (1 984 Kio au plus, D-016) ; les 64 Kio du haut contiennent, chez Teensyduino,
l'EEPROM émulée (60 Kio) puis le **programme de restauration** (4 Kio, à ne jamais toucher). Cible : Teensy 4.0
seul.

Ce que font les références : **Phazerville** range la calibration dans l'EEPROM émulée de Teensyduino au travers de
`PageStorage` (pages tournantes, numéro de génération, marque, somme de contrôle, écriture seulement si le contenu
change), et ses réglages globaux dans **LittleFS** sur 512 Kio de flash programme. **Squares-and-Circles** : support
non documenté.

Choix de l'utilisateur : même mécanisme sur les trois backends (fichier dans le simulateur, JSON du patch dans Rack).
Question de l'utilisateur : existe-t-il un standard, au-delà de ces deux firmwares ?

## 2. Options envisagées

Il n'existe pas de standard unique, mais des **briques de fait** dans l'écosystème Rust embarqué :

* **interface** : traits `NorFlash` / `ReadNorFlash` du crate `embedded-storage` (groupe de travail Rust embarqué) ;
* **stockage** : `sequential-storage` (Tweede golf) — carte clé-valeur ou file sur ces traits, pages en rotation
  (usure répartie), en-têtes et données sous CRC, résistant aux coupures ; API asynchrone d'abord, utilisable en
  bloquant ; alternatives : `ekv` (Embassy, pour beaucoup de clés), `littlefs2` (système de fichiers, ce que fait
  Phazerville, liaison C) ;
* **encodage** : `postcard` (+ `serde`), format compact `no_std` ;
* **écriture de la flash i.MX RT** : l'**API ROM** de NXP (pilote NOR FlexSPI en ROM, exemple `fsl_romapi` du SDK
  MCUXpresso) est la voie documentée par le constructeur ; Teensyduino envoie lui-même des commandes FlexSPI depuis
  la RAM. Aucun crate Rust trouvé n'implémente `NorFlash` pour le Teensy 4.

**S** — Briques standard : `postcard` pour encoder les réglages, `sequential-storage` sur `NorFlash`, une
implémentation `NorFlash` par backend (flash via API ROM ; fichier ; tampon mémoire sérialisé dans le JSON Rack).

**M** — Format maison (en-tête, champs à plat, CRC-32) et journal circulaire maison façon `PageStorage`, derrière un
trait `Storage` d'`oc-core`. Moins de dépendances, plus de code à tester soi-même.

## 3. Ce que l'assistant a proposé

**S**, parce que l'usure, les coupures pendant l'écriture et le CRC sont déjà traités et testés ailleurs :

* `oc-core` ne dépend que de `embedded-storage` et `sequential-storage` ; il encode les réglages (`postcard`, une
  clé par section : calibration, système) et appelle la carte en bloquant ;
* firmware : `oc-firmware/src/flash_store.rs` implémente `NorFlash` sur l'**API ROM** (à vérifier dans le manuel de
  référence du RT1060 avant d'écrire du code : adresse de l'arbre d'API, signatures, adresses relatives à la base
  FlexSPI) ; chaque bloc `unsafe` avec sa section `# Safety`, interruptions masquées et caches invalidés autour de
  l'opération ; zone : quatre secteurs de 4 Kio en bas de la zone EEPROM de Teensyduino (`0x601F_0000`–
  `0x601F_3FFF`), jamais le dernier secteur ; données laissées par Phazerville ignorées (CRC / format) ;
* simulateur : fichier image de flash (option `--settings`, absente ⇒ mémoire seule, scénarios déterministes) ;
  Rack : tampon mémoire exposé par l'ABI (`oc_engine_storage_get/set`), en base64 dans le JSON du patch ;
* sauvegarde bloquante (effacement : dizaines de ms) acceptée, uniquement depuis l'écran système, sorties au repos ;
* `cargo xtask size` / `flash` nomment la zone réservée et vérifient que l'image n'y déborde pas.

## 4. Ce que nous avons décidé, et pourquoi

**S** et l'**API ROM**, choisis par l'utilisateur le 2026-11-16, avec le fichier dans le simulateur et le JSON du
patch dans Rack. Des briques maintenues et largement utilisées plutôt qu'un journal maison : l'usure, la coupure en
pleine écriture et le contrôle d'intégrité sont les parties difficiles, et elles y sont déjà testées. L'API ROM
parce que c'est la voie documentée par le constructeur, et que son code s'exécute depuis la ROM pendant que la flash
est occupée. La vérification au manuel de référence reste le premier pas du lot 4e.

## 5. Ce qui nous ferait changer d'avis

* API ROM absente, différente ou défaillante sur la puce du Teensy : commandes FlexSPI façon Teensyduino.
* `sequential-storage` trop lourd en taille de code ou contraignant à cause de son API asynchrone : M.
* Sauvegarde perceptible comme une coupure gênante : écriture découpée sur plusieurs ticks.

## Révision (2026-11-16, lot 4b) — stockage hors de l'`Engine`, blob encodé pour Rack

Écarts à l'implémentation :

* **pas de paramètre de plateforme de plus dans l'`Engine`** : `oc_core::settings::SettingsStore<F: NorFlash>`
  porte le chargement et la sauvegarde (bloquants, `embassy_futures::block_on` sur l'API asynchrone de
  `sequential-storage` 8.0.2) ; le backend le construit sur sa flash, charge au démarrage
  (`load_or_default`, qui retombe toujours sur `Settings::DEFAULT`) et passe le résultat à
  `Engine::apply_settings`. La logique (format, repli, usure) reste dans `oc-core` et testée une fois ; l'`Engine`
  garde ses six traits. La demande de sauvegarde viendra de l'écran système (4c) ;
* **Rack ne garde pas une image de flash** mais l'enregistrement encodé (`settings::encode` / `decode`, octet de
  format puis `postcard`), via `oc_engine_settings_max_len` / `_save` / `_load` (et non `oc_engine_storage_get/set`) :
  quelques centaines d'octets en base64 dans le JSON du patch au lieu de 16 Kio ;
* **format** : octet `FORMAT = 1`, qui est aussi la clé de la carte ; un changement de `Settings` que `postcard` ne
  relit pas l'incrémente, et l'ancien enregistrement est ignoré (valeurs par défaut) ;
* **zone** : `FLASH_BYTES` = 4 secteurs de 4 Kio, adresses `0..16 Kio` de la `NorFlash` fournie ; le firmware la
  placera à `0x601F_0000` (4e) ;
* **flash simulée** : `testing::MemoryFlash`, NOR stricte (effacement à `0xFF`, écriture qui ne fait que baisser des
  bits) ; le simulateur l'écrit dans un fichier (`run --settings`). Testé : flash vierge, aller-retour après
  « coupure », 456 sauvegardes successives (secteurs recyclés), données étrangères ignorées puis écrasées par une
  sauvegarde.

## Révision (2026-11-16, lot 4e) — la flash du Teensy par l'API ROM, vérifiée

**Vérification préalable** (sources lues le 2026-11-16 : `fsl_romapi.{h,c}` et `MIMXRT1062_features.h` du SDK
MCUXpresso de NXP, `teensy4/eeprom.c` de Teensyduino, `teensy4-fcb` 0.5.0, `imxrt-boot-gen` 0.4.0) :

* le pointeur de l'arbre d'API est à `0x0020_001C` ; son cinquième mot pointe le pilote NOR `FlexSPI`
  (`version`, `init`, `program`, `erase_all`, `erase`, `read`, `clear_cache`, …) ; le RT1062 a toutes ces
  fonctions (`FSL_FEATURE_BOOT_ROM_HAS_ROMAPI`, `FSL_ROM_FLEXSPINOR_API_HAS_FEATURE_*`) ; adresses relatives à la
  base `FlexSPI`, instance 0 ;
* le bloc de configuration de 512 octets que la ROM attend est celui de l'image (`teensy4_fcb`,
  `SERIAL_NOR_CONFIGURATION_BLOCK`, W25Q16JV) et contient les séquences d'écriture (write enable, effacement de
  secteur `0x20`, programmation de page `0x02`, lecture d'état), pages de 256 octets, secteurs de 4 Kio ;
* Teensyduino place l'EEPROM émulée du Teensy 4.0 à `0x601F_0000` sur 15 secteurs, le dernier secteur restant au
  programme de restauration ;
* `imxrt-rt` exécute `.text` depuis l'ITCM et n'active pas les caches.

**Implémentation** (`oc-firmware/src/flash_store.rs`, `SettingsFlash`, seul `unsafe` du firmware) :

* `init` une fois au démarrage avec une copie alignée du bloc de configuration ; chaque appel ROM sous
  `cortex_m::interrupt::free`, suivi de `clear_cache` ;
* **lectures par la fonction ROM `read`**, jamais par la fenêtre mémoire : aucune question de cohérence de cache ou
  de tampon AHB après une écriture ;
* `WRITE_SIZE` 4, `READ_SIZE` 4, `ERASE_SIZE` 4 Kio ; une écriture programme la ou les pages concernées, le reste de
  la page à `0xFF` (sans effet sur une cellule NOR) ;
* zone : `board::SETTINGS_FLASH_OFFSET = 0x1F_0000`, assertions de compilation : au-dessus du budget de 1 984 Kio,
  avant le secteur de restauration ; `xtask::validate` refuse désormais tout segment chargé qui atteint
  `0x601F_0000` (`RESERVED_FLASH_START`) ;
* au démarrage : `SettingsStore::load_or_default`, résultat dans le journal USB (`settings: Loaded|Empty|Failed`) ;
  si la ROM ne fournit pas de pilote, le module démarre sur les valeurs par défaut sans rien sauver ; sur demande de
  l'écran système : `save`, résultat dans le journal.

**Conséquence assumée** : la zone recouvre les quatre premiers secteurs de l'EEPROM émulée de Teensyduino. Un module
venant de Phazerville démarre ici sur les valeurs par défaut (données étrangères ignorées) ; revenir à Phazerville
après une sauvegarde lui fait perdre une partie de son EEPROM (ses sommes de contrôle le ramèneront à ses propres
valeurs par défaut). Rester dans la zone EEPROM est ce qui fait survivre les réglages aux mises à jour du firmware ;
une zone sous `0x601F_0000` serait dans l'espace programme.

**Reste à mesurer sur le module** (`TESTING.md` 9.4, 9.5b) : sauvegarde relue après coupure, cent sauvegardes sans
erreur, coupure juste après `SAVE`.
