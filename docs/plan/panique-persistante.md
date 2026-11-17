# Panique persistante : garder le message d'un crash à travers un redémarrage (préparé le 2026-11-17)

**Statut : préparé, non décidé.** À reprendre si la correction du journal (envoi seulement port ouvert, révision
du 2026-11-17 de D-029) ne suffit pas. Décision à consigner alors en révisant D-018 (« panique : journal USB + SOS »)
avant tout code.

## Le problème

Une panique (ou une faute matérielle) écrit son message dans la file du journal, puis `teensy4-panic` fait
clignoter le SOS pour toujours, sans plus faire tourner l'USB : le message ne sort sur aucun port, et l'hôte finit
par perdre le périphérique. Seul le SOS est visible ; la cause (fichier, ligne, texte) est perdue.

Faire tourner l'USB dans le gestionnaire de panique est écarté : la panique peut venir du code USB lui-même
(`RefCell` empruntée, pile USB dans un état inconnu), et la boucle n'a plus d'état fiable.

## Le mécanisme proposé

1. **À la panique** : écrire un enregistrement dans une zone de RAM que le redémarrage n'efface pas — un mot
   magique, un compteur de paniques consécutives, le fichier, la ligne, le message tronqué (≈ 200 octets), une
   somme de contrôle — puis vider le cache de données sur cette zone.
2. **Quelques secondes de SOS** (le signal visuel reste), puis **redémarrage logiciel** (`SCB::sys_reset`).
3. **Au démarrage suivant**, avant tout le reste : si l'enregistrement est valide, l'écrire en tête du journal
   (`previous run panicked at …`), le garder pour la sonde et l'écran, puis l'effacer.
4. **Boucle de crash** : si le compteur de paniques consécutives dépasse un seuil (2 ou 3), démarrer en **mode
   secours minimal** : l'USB seul, ni SPI, ni OLED, ni DAC, ni applet ; le journal donne l'enregistrement, la sonde
   reste joignable (lecture du crash, redémarrage par 134 bauds pour reflasher), la LED (libre, le SPI n'a pas pris
   sa broche) fait le SOS. Un crash au démarrage ne se cache donc ni derrière des redémarrages sans fin, ni derrière
   un SOS muet. Le compteur revient à zéro après quelques secondes de fonctionnement normal.

**Indépendance vis-à-vis de l'écran** (remarque de l'utilisateur, 2026-11-17) : le message ne doit jamais dépendre
de l'OLED, dont la panne est justement un cas à couvrir. Le démarrage initialise l'USB avant le SPI et l'OLED
(`usb cdc up` précède `spi ok`, `oled init`) : l'enregistrement est relu et journalisé juste après l'USB, avant de
toucher au bus ; l'écran n'est qu'un affichage en plus, et le mode secours ne le touche pas du tout. Un échec de
l'OLED qui n'est pas une panique (`oled init failed`) est déjà journalisé, et visible depuis la correction du
journal.

Les fautes matérielles (`HardFault`) peuvent passer par le même enregistrement (registres `PC`, `LR`, `CFSR`,
`HFSR`, `BFAR`), à décider : elles ne passent pas par le gestionnaire de panique.

## À vérifier avant d'écrire du code

* **Le précédent de Teensyduino** : son `CrashReport` (`teensy4/CrashReport.cpp`, `startup.c`) garde, d'après ce
  que l'on en sait, une structure avec somme de contrôle en haut de l'OCRAM (vers `0x2027_FF80`) et la relit au
  démarrage suivant. À lire dans les sources : l'adresse, ce que la ROM d'amorçage du RT1062 écrase ou non en
  OCRAM au redémarrage, la façon de redémarrer, le vidage du cache.
* **La zone** : la section `.uninit` qu'`imxrt-rt` place en OCRAM (déjà dans la carte mémoire, contrôlée par
  `cargo xtask size`), ou une adresse fixe hors de toute section comme Teensyduino ; dans les deux cas, ajouter le
  contrôle à la liste de `xtask size` (`MEMORY.md`).
* **Le cache de données** : l'OCRAM est en cache ; sans `clean` avant le redémarrage, l'enregistrement peut ne
  jamais atteindre la RAM.
* **Le gestionnaire** : un `#[panic_handler]` à nous à la place de `teensy4-panic` (le SOS est à reprendre :
  `teensy4-panic` le fait au niveau des registres parce que la LED a été cédée au SPI, D-011), dans un module
  `unsafe` à lui (écriture brute en RAM, redémarrage), comme `flash_store.rs` et `reboot.rs` (AGENTS.md).

## Où le message serait rapporté

* **Journal** : première ligne après le démarrage ; avec la correction du 2026-11-17, elle attend que `screen`
  s'ouvre.
* **Sonde** : une requête (`LastCrash`) dans `oc-protocol`, une commande `cargo probe crash`.
* **Écran** (à discuter, jamais seul) : une ligne sur l'écran de démarrage, comme l'identifiant de build (D-025) ;
  en plus du journal, jamais à sa place.

## Où vit le code

* Le format de l'enregistrement (encodage, somme de contrôle, troncature, compteur) est de la logique pure :
  testé sur l'hôte, dans `oc-core` ou `oc-protocol` (qui le transporte vers la sonde) — à trancher.
* L'écriture en RAM, le cache, le redémarrage et le gestionnaire de panique : `oc-firmware`, module `unsafe` dédié.

## Lots esquissés

| Lot | Contenu |
|---|---|
| vérification | lire `CrashReport` de Teensyduino et la doc de la ROM (OCRAM au redémarrage) ; choisir la zone ; révision de D-018 |
| format | enregistrement, somme de contrôle, compteur ; tests sur l'hôte |
| firmware | gestionnaire de panique (SOS limité puis redémarrage), relecture au démarrage juste après l'USB, journal ; mode secours (USB seul) après N paniques consécutives |
| rapport | sonde (`cargo probe crash`), écran de démarrage si retenu |
| essai | une panique provoquée par une feature d'essai (comme `settings-stress`), jamais sur un module en usage |

## Ce qui rendrait ce travail inutile

Que les paniques restent exceptionnelles et se reproduisent sur le simulateur ou sous `cargo test` (le cœur est
testé sur l'hôte) : le SOS suffirait alors à dire « il y a eu une panique », et l'hôte en donnerait la cause.
