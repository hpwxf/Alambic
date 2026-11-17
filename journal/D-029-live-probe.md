# D-029 — Sonde live : profilage dans le cœur, second port USB CDC binaire, outil hôte `oc-probe`

| | |
|---|---|
| **Identifiant** | D-029 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | `706410a` (ADR) ; `1a7d0c6` (3b1, profilage), `5528472` (3b2, protocole), `18a6cb4` → `22fb58a` (3b3, firmware), `d5365bb` (3b4, outil hôte), `1c11495` → adoption (3b5, mesures) |
| **Relations** | **révise** D-002 (une capacité de plus sur `Clock`), D-003 (deux crates de plus), D-011 (deux ports CDC, sans `imxrt-log`) ; sert D-023 (mesure du lot 3b) ; touche D-016 (redémarrage en mode chargement sans bouton) |

## 1. Contexte

Le diagnostic hôte du module se limite à un journal texte sur USB CDC (`imxrt-log`, D-011) : des lignes à sens
unique, une par seconde. Le lot 3b doit mesurer le coût réel d'une trame OLED et la gigue de la boucle (D-023) ;
le protocole prévu (GPIO + oscilloscope, `docs/supersaw/` § 13) demande deux sondes, des broches libres et une
lecture à l'œil.

Demande de l'utilisateur (2026-11-16) : une interface plus élaborée pour capter des informations fines pendant
l'exécution, qui serve aussi au profilage, sous la forme d'un outil du projet avec des capacités dans les deux sens
(envoi de commandes, réception d'informations) avec le module en marche.

Contraintes : pas d'UART (D-011) ; pas de sonde SWD accessible sur le Teensy 4.0 ; la boucle est coopérative et
sans exécuteur (D-008) ; `oc-core` reste `no_std` et sans `unsafe`.

## 2. Options envisagées

**Transport.**

* **T1 — deux ports CDC** sur un même périphérique USB composite : le premier garde le journal texte (lisible par
  `screen`), le second porte un protocole binaire réservé à l'outil hôte.
* **T2 — un port, tout encadré** : le journal devient un type de message ; l'outil hôte devient obligatoire.
* **T3 — un port, commandes texte** : mini-shell ; simple, fragile pour des flux et des histogrammes.

**Encodage.** `postcard` (déjà utilisé, D-028) avec l'encadrement COBS qu'il fournit (`to_slice_cobs`,
`CobsAccumulator`). `postcard-rpc` est écarté : son serveur exige un exécuteur embassy ou des E/S asynchrones.

**Pile USB.** `usb-device` + `usbd-serial` sur `imxrt-usbd`, les crates qu'`imxrt-log` utilise déjà
(`Cargo.lock`) ; `imxrt-log` possède le périphérique USB et ne peut pas le partager, il est donc remplacé par un
journal `log` maison sur le premier port.

**Mesure du temps.** Le compteur de cycles du Cortex-M7 (DWT `CYCCNT`, 600 MHz, 32 bits, ≈ 7 s avant
repliement) : une lecture coûte quelques cycles, la résolution est de 1,7 ns.

**Présence dans l'image.** Toujours compilée, ou derrière une feature comme `settings-stress`.

## 3. Ce que l'assistant a proposé

T1, `postcard` + COBS, `usb-device` + `usbd-serial`, DWT ; sonde toujours présente : une image de debug à part
changerait précisément les timings que l'on veut mesurer, et le canal ne fait rien tant qu'aucun hôte ne lui
parle.

Forme dans le code :

* **profilage dans `oc-core`** (`oc_core::profile`) : des étapes nommées (tick, acquisition, applet, sorties,
  rendu, envoi de la trame) mesurées par l'`Engine`, des statistiques min / max / moyenne / nombre et un
  histogramme de la période de boucle, testés sur l'hôte. Le temps fin vient d'une capacité **par défaut** de
  `Clock` (`fine_count`, `fine_per_micro`, déduites de `now_micros` sauf surcharge) : le firmware y branche
  `CYCCNT`, le simulateur et Rack n'ont rien à faire ;
* **`oc-protocol`** (nouvelle crate, `no_std`, `forbid(unsafe_code)`) : les messages des deux sens, leur
  encadrement, et le numéro de version du protocole ; testée sur l'hôte, partagée par le firmware et l'outil ;
* **commandes dans le cœur** quand elles changent le comportement (forcer une sortie, relire le framebuffer) :
  les trois backends pourraient les servir, et elles sont testées une fois ;
* **`oc-probe`** (nouvelle crate, binaire hôte) : `oc-probe stats | stream | out | screen | reboot`, sur le port
  série (`serialport`) ; un TUI viendra plus tard s'il sert ;
* **redémarrage en mode chargement** : le geste de Teensyduino (débit 134 bauds sur le port série), à vérifier
  sur les sources du cœur Teensy 4 avant d'écrire le moindre `unsafe`, dans un module à lui.

## 4. Ce que nous avons décidé, et pourquoi

Choix de l'utilisateur (2026-11-16) : **T1**, **toujours présente**, périmètre complet pour la première version
(profilage, flux de valeurs, commandes de test, capture d'écran et redémarrage), **`oc-protocol` + `oc-probe`**
plutôt qu'une sous-commande `xtask` (qui mélangerait construction et instrumentation live).

Statut `proposée` : adoptée lot par lot, comme D-026 à D-028.

## 5. Ce qui nous ferait changer d'avis

* Si la pile USB à deux ports coûtait plus que quelques pour cent d'une période de 1 ms en sondage, ou si macOS
  ne voyait pas les deux ports, T2 sur un seul port.
* **Point de bascule vers T2 (un seul port encadré), noté le 2026-11-17 à la demande de l'utilisateur** : le second
  port ne sert qu'à lire le journal sans outil. On passe à un seul port (journal comme type de message du
  protocole, affiché par `oc-probe log` ; `screen` ne suffit plus) dès que l'une de ces raisons apparaît :
  l'arrivée de l'USB MIDI (ou d'une autre fonction USB) qui veut ses points de terminaison ; une gêne à l'usage
  (deux ports à distinguer). Coûts constatés de T1 : un descripteur de configuration de 141 octets (énumération
  impossible sans `control-buffer-256`), la recherche du bon port par `oc-probe`. Le message de panique n'est pas
  un argument pour T1 : après une panique, plus rien ne sonde l'USB, il ne part sur aucun des deux ports.
* Si le redémarrage logiciel en mode chargement exigeait plus qu'un appel documenté, il resterait hors de la
  sonde (le bouton PROGRAM suffit, D-016).
* Si l'audio (D-023 option B) passe en DMA et en interruptions, le profilage devra mesurer l'ISR elle-même ; les
  étapes nommées restent, leur point de mesure change.

## Révision (2026-11-16, lot 3b1) — un compteur fin absent par défaut, un profil qui commence après le démarrage

Écarts à la proposition, apparus en écrivant le lot :

* **`Clock::fine_count` vaut zéro par défaut** (et `fine_per_micro` aussi), au lieu d'être déduit de
  `now_micros` : la `VirtualClock` des tests avance à chaque lecture de `now_micros` (coût de lecture modélisé), si
  bien qu'un défaut qui la lirait déplacerait le temps de chaque tick et changerait les tests et les écrans dorés.
  `VirtualClock` fournit son propre compteur fin, son temps lu sans l'avancer ; une horloge sans compteur fin
  laisse le profil vide. Le simulateur et Rack, qui tournent sur la `VirtualClock`, ont donc un profil
  déterministe mais sans valeur de mesure : seul le module mesure.
* **Le profil est remis à zéro à la fin de l'écran de démarrage** : celui-ci redessine à chaque tick (D-020), ses
  durées ne décrivent pas le module en marche.
* **`Present` ne mesure que l'envoi** (`Display::present`), pas le contraste, la rotation ni le décalage : c'est le
  coût du bus que D-023 doit connaître. Le détail par page (`Stage::Page`) attend le lot 3b3, qui doit ouvrir le
  pilote OLED page par page.
* En attendant l'outil hôte, le firmware écrit le profil **sur le journal USB**, deux lignes par seconde
  (`profile_log.rs`) : les chiffres du lot 3b sont lisibles avec `screen` dès ce lot.
* La revue a trouvé, au passage, les deux arguments de la ligne `tick=… last_us=…` inversés depuis le lot 4e
  (`last_us` affichait le détail des réglages) ; corrigé.

Statut inchangé (`proposée`) : le transport et l'outil hôte restent à faire.

## Révision (2026-11-16, lot 3b3) — le serveur dans `oc-protocol`, le redémarrage vérifié sur les sources

* **Le serveur de la sonde est dans `oc-protocol`** (`server.rs`), générique sur l'`Engine` et testé contre
  `mock_engine` : session et version, profil, flux d'échantillons, sortie forcée, écran en huit pages, demande de
  redémarrage, fermeture qui relâche les sorties. Le firmware n'a gardé que le câblage USB (`usb.rs`), conformément
  à la règle « pas de tests dans `oc-firmware`, tester l'abstraction ». La file d'octets (`queue.rs`) sert aux deux
  ports.
* **Commandes dans le cœur** : `Engine::force_output` (ignorée pendant l'écran de démarrage et par l'assistant de
  calibration, qui mesure ses propres niveaux ; `reset` relâche tout) et `Engine::raw_codes`. La capture d'écran lit
  le framebuffer tel qu'envoyé (rotation et décalage déjà appliqués), sans commande dans le cœur.
* **Pile USB** : `usb-device` + `usbd-serial` sur l'`imxrt-usbd` que le BSP réexporte, `BusAdapter::new` (la variante
  sûre, avec sections critiques) ; périphérique composite à IAD, deux `CdcAcmClass` nommées « Alambic log » et
  « Alambic probe » ; mêmes VID / PID que l'ancien backend. Le journal garde le format `[LEVEL cible]: message`.
  Le `Server` (≈ 4,6 Kio) vit dans un `ConstStaticCell`, pas sur la pile de 16 Kio.
* **Redémarrage en mode chargement, vérifié** sur `teensy4/usb.c` (cœur Teensyduino) : hors mode sécurisé
  (`HW_OCOTP_CFG5 & 0x02`), `_reboot_Teensyduino_` exécute `bkpt #251`, auquel répond la puce du bootloader ; le
  déclencheur des outils Teensy est un `SET_LINE_CODING` à 134 bauds. Les deux sont repris (`reboot.rs`, second
  module `unsafe` du firmware) ; la variante sécurisée (saut dans la ROM NXP) n'est pas reprise et la sonde répond
  `Unsupported` sur une puce verrouillée. Le mécanisme (requête de contrôle sur l'endpoint 0, aucune donnée série ;
  contrôleur PJRC sur le port de debug ; ce qui est lu des sources et ce qui est vérifié) est décrit dans la
  documentation de `usb.rs` et de `reboot.rs`.
* **Pages OLED** : `Ssd130x::flush_page` ; le firmware enveloppe le pilote (`TimedPanel`) pour envoyer et chronométrer
  chaque page (`Stage::Page`). Sur SSD1306 / SSD1309, la trame passe donc en huit fenêtres d'une page au lieu d'une
  seule fenêtre (6 octets de commande en plus par page).

Statut inchangé (`proposée`) : essai sur module et outil hôte à faire.

## Révision (2026-11-16, lot 3b4) — l'outil hôte, testé contre le serveur réel

* **`oc-probe`** : `Client` générique sur `Read + Write` (le port série en vrai, une boucle en mémoire dans les
  tests), qui met de côté les échantillons rencontrés en attendant une réponse ; `port` trouve le port de la sonde
  en envoyant un hello à chaque candidat (le port du journal ne répond pas) ; `render` affiche le profil en
  tableau, l'écran en texte (demi-blocs) ou en PNG. CLI : `ports`, `info`, `stats [--fresh S] [--watch S]`,
  `stream`, `out <A-D> <mV>` (jusqu'à Entrée), `screen [--png]`, `reboot` ; alias `cargo probe`.
* **Tests** : le client contre le vrai `oc_protocol::server` sur `mock_engine`, par une boucle qui fait tourner le
  moteur pendant que l'hôte attend : toute la chaîne des deux côtés, sans matériel. Ils ont trouvé un blocage
  (attendre un échantillon pendant un flux ne rendait jamais la main), corrigé avant tout essai sur module.
* **`serialport` sans `libudev`** (feature par défaut) : elle exige les en-têtes système sous Linux, donc la CI ;
  sans elle, la liste des ports n'a pas toujours les identifiants USB, et l'outil essaie alors tous les ports.
  Licence MPL-2.0, au niveau du fichier : sans effet sur notre code.
* **La capture d'écran défait rotation et décalage** côté module (le serveur connaît les réglages) : elle se lit
  comme la façade.
* Non fait : `cargo xtask flash` qui demanderait lui-même le redémarrage ; `cargo probe reboot` puis
  `cargo xtask flash` en tient lieu.

Statut inchangé (`proposée`) : essai de l'outil sur module (lot 3b5).

## Révision (2026-11-17, lot 3b5) — adoptée après essai sur module

* **Mesures de niveau 9 dans l'outil** (`oc_probe::measure`, fonctions pures testées sur l'hôte) : `inputs`
  (extrêmes, moyenne, crête à crête et codes des quatre entrées, à chaque tick), `triggers` (fronts montants et
  intervalles, `INEXACT` si l'hôte a perdu des échantillons), `sweep` (une sortie rebouclée sur une entrée, palier
  par palier). Documentées dans `TESTING.md` 9.1b à 9.3b.
* **Les échantillons portent l'horloge du module** (`Sample.micros`, protocole **version 2**) : un tick ne dure pas
  toujours 1 ms (les ticks de rendu durent ≈ 2,1 ms, ≈ 947 ticks par seconde mesurés, valeur indicative) ; des
  durées comptées en ticks étaient fausses d'≈ 5 %.
* **Une sortie tenue par la sonde se voit sur la façade** : `TickContext` porte les sorties écrites et le fait
  qu'elles sont forcées ; l'applet diagnostic affiche `OUT*`.
* **Critères du § 5, constatés sur le module TLM** (un exemplaire, valeurs indicatives) : macOS voit les deux ports ;
  le service USB coûte ≈ 2 µs en moyenne par tick (max ≈ 7 µs), bien moins que « quelques pour cent » d'une période ;
  le redémarrage par 134 bauds remplace l'appui sur PROGRAM. T1 est gardé ; la bascule vers T2 reste celle notée au
  § 5.
* **Ce que la sonde a permis de mesurer**, au lieu de l'oscilloscope prévu : coût d'une page OLED (≈ 145 µs) et
  d'une trame (≈ 1,16 ms), du rendu (≈ 0,63 ms), de l'échantillonnage (≈ 0,29 ms) ; gigue de la boucle hors rendu
  sous ≈ 8 µs ; et, au multimètre, que l'écart du rebouclage vient de l'entrée, pas de la sortie. Détail :
  `docs/plan/etape-3b-sonde.md`, bilan : `docs/plan/avancement/etape-3.md`.
* Toujours non fait : `cargo xtask flash` qui demanderait lui-même le redémarrage (`cargo probe reboot` avant
  suffit).
