# D-028 — Persistance des réglages : briques standard de l'écosystème Rust embarqué, flash du Teensy 4.0

| | |
|---|---|
| **Identifiant** | D-028 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
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
