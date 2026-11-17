# D-031 — Outils hôte dans le navigateur : sonde par Web Serial, flash par WebHID, cœur en WebAssembly

| | |
|---|---|
| **Identifiant** | D-031 |
| **Date** | 2026-11-17 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | branche `web-tools` |
| **Relations** | **complète** D-016 (un second chemin de flash, même porte de validation) et D-029 (un second client de la sonde) ; **révise** D-003 (trois crates de plus) |

## 1. Contexte

Sonder le module (`oc-probe`) ou le flasher (`cargo xtask flash`) demande aujourd'hui la chaîne Rust épinglée et
`teensy_loader_cli`. Pour un musicien qui veut seulement installer une version publiée ou regarder ce que fait son
module, c'est une marche haute. Question de l'utilisateur (2026-11-17) : peut-on faire la sonde et le flash depuis
un navigateur, idéalement en WebAssembly ?

Ce que le navigateur offre : **Web Serial** (ports CDC) et **WebHID** (périphériques HID), dans Chromium seulement
(Chrome, Edge, Opera ; ni Firefox ni Safari), en contexte sécurisé (https ou `localhost`), un port choisi par un
geste de l'utilisateur. Le bootloader HalfKay du Teensy est un périphérique HID à page d'usage propriétaire
(16C0:0478), hors de la liste de blocage de WebHID.

Ce que le dépôt offre déjà : `oc-protocol` est `no_std` et sans allocation ; le client de `oc-probe` ne connaît
qu'un flux d'octets ; `validate_image` est une fonction pure sur des octets ; la sonde sait redémarrer le module en
mode chargement (D-029), si bien qu'un flash n'a pas besoin du bouton PROGRAM.

## 2. Options envisagées

**Forme de l'outil.**

* **W1 — page statique + WebAssembly** : le protocole, la validation et le découpage HalfKay compilés en wasm, une
  page HTML et un peu de JavaScript pour les API du navigateur (asynchrones).
* **W2 — tout en JavaScript** : réécrire le protocole (postcard, COBS) et la validation ; deux implémentations à
  garder d'accord.
* **W3 — application de bureau** (Tauri, Electron) : pas de limite Chromium, mais une installation, ce qu'on veut
  éviter.

**Boucle d'E/S.** Le `Client` de `oc-probe` est bloquant (`std::io::Read + Write`), impossible dans le fil
principal d'une page. Soit un client asynchrone en Rust (`wasm-bindgen-futures`, trait de transport async), soit
un cœur **sans E/S** en wasm (encoder une requête, pousser des octets reçus, rendre des messages décodés) et la
boucle asynchrone en JavaScript.

**Ce que le flash web accepte.** L'ELF (la porte de D-016 s'applique telle quelle, l'image à écrire se déduit des
segments `PT_LOAD`) ou le HEX (ce que publie `cargo xtask hex`, mais aucune validation structurelle possible
au-delà des adresses).

**Protocole HalfKay.** Réimplémenter celui de `teensy_loader_cli` pour `TEENSY40` : rapports de 64 octets
d'en-tête (adresse sur 24 bits, petit-boutiste, relative à `0x6000_0000`) suivis de 1 024 octets de données ; blocs
vierges sautés sauf le premier, dont l'écriture efface la puce (attente longue) ; un rapport d'adresse `0xFFFFFF`
redémarre.

## 3. Ce que l'assistant a proposé

W1, cœur sans E/S en wasm, flash depuis l'**ELF** seulement, en trois crates :

* **`oc-image`** : la validation de `xtask` déplacée telle quelle (`xtask::validate` la réexporte, ses tests et
  `cargo xtask flash` ne changent pas), plus l'image flash tirée des segments `PT_LOAD` ;
* **`oc-halfkay`** : le découpage en rapports HalfKay, pur, testé sur l'hôte ; l'envoi (WebHID, ou un jour
  `hidapi` pour remplacer `teensy_loader_cli`) reste au transport ;
* **`oc-web`** : les liaisons `wasm-bindgen` ; `oc-probe` sépare son outil en ligne de commande (`serialport`,
  `clap`, `png`) derrière une feature `cli` par défaut, pour que `oc-web` réutilise le rendu du profil et des
  échantillons sans tirer de dépendance d'OS.

La page (`web/`) n'a pas d'outil de construction JavaScript : `cargo xtask web` compile `oc-web` pour
`wasm32-unknown-unknown`, appelle `wasm-bindgen` (CLI, version vérifiée contre `Cargo.lock`) et copie les
fichiers statiques dans `dist/web/`. Le flash web garde la rigueur de `xtask flash` : validation complète,
empreinte SHA-256 affichée, confirmation explicite.

Risque principal : WebHID face à HalfKay n'a jamais été essayé sur ce module. L'outil de flash est donc un
**prototype à valider sur le module** avant tout investissement d'interface.

## 4. Ce que nous avons décidé, et pourquoi

Choix de l'utilisateur (2026-11-17) : poursuivre selon la proposition, dans un worktree distinct partant de
`master`.

Statut `proposée` : passe à `adoptée` quand un flash complet (redémarrage par la sonde, écriture, redémarrage sur
la nouvelle image, identifiant de build changé sur l'écran de démarrage) et une session de sonde auront été faits
depuis Chrome sur le module.

## 5. Ce qui nous ferait changer d'avis

* Si HalfKay refusait les rapports envoyés par WebHID (taille, identifiant de rapport, délais d'effacement), le
  flash resterait dans `xtask` et la page ne garderait que la sonde.
* Si le support de Web Serial / WebHID disparaissait de Chromium, ou si l'usage réel venait surtout de Firefox ou
  Safari, W3.
* Si le cœur sans E/S obligeait à dupliquer en JavaScript une logique de session non triviale (relances,
  synchronisation), un client asynchrone en Rust partagé avec `oc-probe`.
* Si les versions publiées ne portaient que le HEX, accepter le HEX avec une validation réduite aux adresses.

## 6. Révision du 2026-11-17 — essai sur le module

Essayé depuis Chrome (macOS) sur le module TLM, page servie sur `localhost`.

**Sonde.** Connexion, capture d'écran (aussi en continu), profil (lecture et remise à zéro, 20 sur 20 pendant la
capture continue, environ 21 ms par réponse), flux d'échantillons, sortie A tenue à 1 000 mV (multimètre) puis
relâchée : conformes.

**Flash.** Le redémarrage en mode chargement par la sonde marche. Le descripteur HID de HalfKay (lu par `ioreg`) n'a
pas d'identifiant de rapport et un rapport de sortie de 1 088 octets, ce que la page envoie. Trois écarts avec
`teensy_loader_cli` sont apparus et ont été corrigés :

* **délai d'effacement** : 5 s pour le premier bloc ne suffisaient pas (« Failed to write the report ») ;
  `teensy_loader_cli` laisse **45 s à chacun des cinq premiers blocs**, `oc-halfkay` fait de même ;
* **blocs vierges** : écrits jusque-là malgré la section 2 ; désormais sautés, sauf le premier ;
* **demande de démarrage** : envoyée une fois sans relance, son échec ignoré ; l'image était écrite (le module est
  passé de `11de326` à `32e0384`, démarré à la main par `teensy_loader_cli -b`) mais restait dans HalfKay.
  Elle est maintenant relancée comme un bloc, et la page attend que HalfKay quitte le bus avant de dire « done ».

Après correction, une installation complète depuis la page (redémarrage par la sonde, écriture, redémarrage seul
sur la nouvelle image) a réussi. Piège noté : Chrome sert l'ancien `app.js` depuis son cache après un
`cargo xtask web` ; recharger sans cache (Cmd+Shift+R).

La page a aussi été réécrite pour un utilisateur qui n'est pas développeur, l'équivalent en ligne de commande de
chaque fonction restant dans un encart « For developers ».

Statut : **adoptée**. `cargo xtask flash` reste le chemin de référence des développeurs.

## 7. Révision du 2026-11-17 — le HEX accepté, avec une vérification réduite

**Contexte.** La CI publie `dist/oc-firmware.hex` en artefact (D-016, D-017), et le HEX est le format de tout
l'écosystème Teensy : Teensyduino, l'application Teensy Loader, les autres firmwares O&C (Phazerville). La section 5
prévoyait ce cas. Choix de l'utilisateur : la page accepte aussi le HEX, dans la même PR.

**Ce qu'on vérifie d'un HEX.** Un HEX ne porte que des adresses et des octets : ni l'architecture, ni la cible, ni
la table des sections. La page vérifie donc :

* la syntaxe Intel HEX (enregistrements 00, 01, 02, 04, 03 et 05 tolérés), chaque somme de contrôle, et la présence
  de l'enregistrement de fin, qui distingue un fichier complet d'un téléchargement tronqué ;
* que chaque octet tombe dans la flash applicative (`FLEXSPI_BASE`..`RESERVED_FLASH_START`) et qu'aucune adresse
  n'est écrite deux fois ;
* l'**en-tête de démarrage** tel que la ROM le lit, commun à tout firmware Teensy 4.0 (relevé sur ce firmware, et
  écrit de la même façon par Teensyduino, `bootdata.c`) : le FCB (`FCFB`) à `0x6000_0000` ; l'IVT à
  `0x6000_1000` (tag `0xD1`, longueur `0x0020`, version `0x4x`, champ `self` égal à `0x6000_1000`, point d'entrée
  dans une mémoire du composant) ; les boot data qu'elle désigne, dans la flash, commençant à `0x6000_0000`, de
  taille au plus 1984 Kio.

L'empreinte affichée est le SHA-256 du fichier `.hex`, celle que `cargo xtask flash` imprime (`hex sha256`) et que
la CI journalise. La page dit clairement que la vérification est réduite. Le chemin ELF ne change pas : il garde la
porte complète de D-016, et reste le format recommandé.

**Forme.** `oc-image` gagne la lecture du HEX et le contrôle de l'en-tête de démarrage, sur l'image flash et non
sur l'ELF, et un point d'entrée qui reconnaît le format (signature ELF, `:` initial d'un HEX) ; `FlashImage` ne
porte plus les faits propres à l'ELF mais le format, l'empreinte, le point d'entrée et les avertissements.

**Essai.** Le HEX publié par la CI, installé depuis la page sur le module : accepté (« Intel HEX »), écrit, et le
module a redémarré dessus. Le HEX réel de `cargo xtask hex` donne la même image flash que son ELF, octet pour octet
(test `real_firmware_hex_reads_as_its_elf_when_present`). Un HEX produit par Teensyduino n'a pas encore été essayé.

**Ce qui nous ferait changer d'avis.** Un HEX de Teensyduino ou de Phazerville refusé à tort par le contrôle de
l'en-tête : ce contrôle serait alors à assouplir, pas à contourner.
