# Le matériel d'Ornament & Crime sur Teensy 4.0 — guide et référence

Ce document explique le matériel sur lequel tourne Alambic, pour quelqu'un qui connaît la programmation (Rust, ici)
mais peu l'électronique embarquée, et sert ensuite de référence. Il part des principes, dit ce que les fiches
techniques garantissent, puis ce que ce module-ci est et ce qu'on y a mesuré. Il ne décrit pas le code : celui-ci
est dans `AGENTS.md` et ses annexes (`agents/firmware.md` pour la boucle, le chemin audio et l'écran), les décisions
dans `journal/`, le récit de l'étape audio dans [plan/rapport-etape-7-chemin-audio.md](plan/rapport-etape-7-chemin-audio.md).

## 0. Comment lire ce document

Chaque affirmation relève de l'un de quatre ordres. Les paragraphes et les lignes de tableaux portent l'étiquette
correspondante quand le contexte ne suffit pas à le dire :

| Étiquette | Ce que c'est | Ce qui en fait foi | Peut changer ? |
|---|---|---|---|
| **[principe]** | fonctionnement général, vrai pour toute machine de ce genre | un cours, un livre | non |
| **[spécifié]** | comportement promis par un composant | fiche technique, manuel de référence (*RM*) du fabricant, cité | seulement d'une révision de puce à l'autre |
| **[ce module]** | l'assemblage concret : quelle broche va où, quels composants, et ce qu'on a mesuré sur l'exemplaire TLM Audio O_C T4 de l'utilisateur | le schéma, `board.rs`, les mesures datées | d'un module à l'autre (autre carte, autre nappe, autre écran) |
| **[firmware]** | un choix fait dans le code d'Alambic | le code, `journal/` | à chaque décision |

Deux règles en découlent :

* **une mesure [ce module] n'est jamais une constante du code** : elle dit ce qu'un exemplaire a fait ; le code
  s'appuie sur le [spécifié] et se vérifie sur le [ce module] ;
* **ce qui est déduit sans avoir été mesuré est dit tel** (« déduit », « non vérifié »).

Les sources sont en fin de document (§ 9). « RM » désigne le *i.MX RT1060 Processor Reference Manual* de NXP ; le
dépôt n'en contient pas de copie, et plusieurs détails ci-dessous ont été lus dans le code des crates `imxrt-ral`,
`imxrt-hal` et `teensy4-bsp`, qui le transcrivent : c'est précisé quand c'est le cas.

---

## 1. Vue d'ensemble

### 1.1 Ce qu'est un module Eurorack de ce type [principe]

Un module « numérique » Eurorack reçoit des **tensions de contrôle** (CV, en volts) et des **triggers** (impulsions),
les convertit en nombres, calcule, et reconvertit des nombres en tensions sur ses sorties. Le microcontrôleur ne
voit jamais une tension : il lit des **codes** produits par un convertisseur analogique-numérique (ADC) et écrit des
codes qu'un convertisseur numérique-analogique (DAC) transforme en tension. Entre le jack et le convertisseur, un
**étage analogique** adapte la plage (le Eurorack va de −10 à +10 V, un convertisseur de 0 à 3,3 V) et protège.

### 1.2 Ce module [ce module]

```
            jacks CV1..CV4 ──► étage d'entrée (inverseur) ──► ADC1 interne du RT1062
            jacks TR1..TR4 ──► transistors inverseurs ─────► GPIO (détection de front)
     encodeurs, boutons ──────────────────────────────────► GPIO (pull-up interne)
                                                                    │
                                                 ┌──────────────────┴──────────────────┐
                                                 │   Teensy 4.0 : i.MX RT1062, 600 MHz  │
                                                 │   USB natif ◄──► hôte (journal, sonde)│
                                                 └──────────────────┬──────────────────┘
                                                      LPSPI4 (MOSI, SCK partagés)
                                         ┌──────────────────────────┴──────────────────────────┐
                                   CS = PCS0 (broche 10)                          CS, D/C en GPIO (8, 6)
                                         ▼                                                      ▼
                                 DAC8565 (4 × 16 bits)                                 OLED SH1106 128×64
                                         ▼
                        étage de sortie (inverseur) ──► jacks A..D
```

### 1.3 Index des composants [ce module]

| Composant | Liaison | À retenir | Où c'est documenté |
|---|---|---|---|
| **Teensy 4.0** (NXP i.MX RT1062, Cortex-M7 600 MHz) | cœur du module | 2 Mio de flash, dont 64 Kio du haut réservés (réglages, EEPROM émulée, programme de restauration) ; ITCM / DTCM / OCRAM | `crates/oc-firmware/MEMORY.md`, D-004, D-028 |
| **Bootloader PJRC** (HalfKay, puce séparée) | flash par USB | ineffaçable par le firmware ; bouton PROGRAM, ou `bkpt #251` sur ordre (`cargo probe reboot`) | `crates/oc-firmware/src/reboot.rs`, D-016, D-029 |
| **ADC1 interne** (12 bits, SAR) | 4 entrées CV, broches 19, 18, 20, 17 (`CV1..CV4`, pas dans l'ordre des broches) | pas d'ADC externe ; étage inverseur ; ≈ −3,33 à +6,67 V (0 V vers le code 2730) ; conversion bloquante ≈ 72 µs par voie ; justesse ≈ 20 mV | `crates/oc-firmware/src/cv_in.rs`, D-006, D-027, `plan/avancement/etape-3.md` |
| **DAC8565** (TI, 4 × 16 bits, SPI) | 4 sorties ; CS broche 10 (PCS0 de LPSPI4), RST broche 9 | 22 MHz ; écrit par DMA à 47 619 Hz ; étage inverseur ; −3 à +7 V par la calibration ; juste à ≤ 3 mV au multimètre | § 6.1, `crates/oc-drivers/src/dac8565.rs`, D-010, D-027, D-033 |
| **OLED SH1106** 128 × 64 | CS 8, D/C 6, RST 7 (GPIO) | 22 MHz sur ce module ; envoyé par tronçons de 20 octets entre deux rafales du DAC ; une trame ≈ 1,34 ms | § 6.3, `crates/oc-drivers/src/ssd130x.rs`, D-009, D-033 |
| **Bus LPSPI4** | partagé DAC + OLED ; MOSI 11, SCK 13 (aussi la LED), MISO 12 inutilisé | horloge LPSPI 132 MHz ; FIFO de 16 mots ; PCS0 pour le DAC, PCS3 (sans broche) pour l'OLED | § 4.2, D-010, D-033 |
| **4 entrées trigger** | broches 0 à 3, transistors inverseurs, pull-up | fronts capturés par le détecteur de front des GPIO ; aucune impulsion perdue dès ≈ 0,68 ms | `crates/oc-drivers/src/triggers.rs`, D-019 |
| **2 encodeurs + 4 boutons** | broches 4, 5, 14 à 16, 21 à 23 | pull-up 22 kΩ et hystérésis des pads indispensables | `crates/oc-board/src/lib.rs`, D-007 |
| **USB natif** | 2 ports série : journal texte, sonde binaire | interrogé par la boucle, pas d'interruption | `crates/oc-firmware/src/usb.rs`, D-011, D-029 |
| **LED** (broche 13) | étapes du démarrage, puis SCK du SPI | clignote SOS sur une panique | `crates/oc-firmware/src/boot_led.rs` |
| **Aucun UART** | — | toutes les LPUART des broches 0–23 entrent en conflit avec le panneau | D-011, en-tête de `main.rs` |

La table des broches fait foi dans `crates/oc-board/src/lib.rs`, **seul fichier autorisé à nommer une
broche** : configurer en sortie une broche que le module pilote en entrée est la seule façon d'abîmer le matériel.

**Réserve [ce module]** : la carte TLM Audio O_C T4 n'est pas documentée publiquement. Ce qui suit sur l'électronique
analogique vient du schéma O&C **rev 2e** (mxmxmx), la meilleure approximation disponible ; les mesures confirment
ce qu'elles touchent.

---

## 2. Le microcontrôleur i.MX RT1062

### 2.1 Un microcontrôleur, pas un ordinateur [principe]

Pas de système d'exploitation, pas de processus, pas de threads, pas d'allocation obligatoire : un seul programme,
qui démarre au reset et ne rend jamais la main (`fn main() -> !`). Le cœur parle aux **périphériques** (SPI, timers,
DMA, GPIO…) en lisant et écrivant des **registres** : des mots de 32 bits à des adresses fixes. Ces périphériques
travaillent ensuite seuls, en parallèle du cœur : un contrôleur SPI fait sortir des bits pendant que le cœur calcule.

En Rust, un accès à un registre doit être **volatile** (`read_volatile`, `write_volatile`) : sinon le compilateur,
qui y voit de la mémoire ordinaire, pourrait le fusionner, le réordonner ou le supprimer. Les crates `imxrt-ral`
(un module par périphérique, une constante par champ) et `imxrt-hal` (des types plus sûrs par-dessus) le font pour
nous.

### 2.2 Le cœur [spécifié]

* ARM **Cortex-M7**, 600 MHz sur le Teensy 4.0 (réglage de la BSP `teensy4-bsp`).
* Une seule unité d'exécution : la concurrence vient uniquement des **interruptions** (§ 5).
* **DWT** (*Data Watchpoint and Trace*) : bloc de débogage qui contient un compteur de cycles, `CYCCNT`, 32 bits, qui
  avance d'un par cycle (600 par µs) et boucle toutes les ≈ 7,2 s. **Il est arrêté au reset** : il faut activer le
  traçage (`DCB`) puis le compteur. [firmware] Alambic le démarre en tête de `main` ; le profil et l'interruption
  audio s'en servent (D-029, D-033).

### 2.3 Les mémoires [spécifié]

| Mémoire | Adresse | Ce qu'on y met | Cache |
|---|---|---|---|
| **ITCM** (*Instruction Tightly Coupled Memory*) | 0x0000_0000 | le code, copié depuis la flash au démarrage | sans objet (accès en un cycle) |
| **DTCM** (*Data TCM*) | 0x2000_0000 | pile, variables statiques (`.data`, `.bss`) | **non couverte par le cache de données** |
| **OCRAM** (*On-Chip RAM*) | 0x2020_0000 | données en lecture seule, tas | couverte par le cache |
| **Flash FlexSPI** | 0x6000_0000 | l'image du firmware, les réglages | lue au démarrage |

* [principe] Un **cache** garde une copie de la mémoire près du cœur. Si un autre maître (le DMA, § 4.5) lit la
  mémoire directement, il peut voir une valeur que le cœur n'a pas encore écrite hors du cache : il faut alors
  « nettoyer » le cache. Une mémoire non cachée évite la question.
* [ce module, constaté] Le DMA lit bien la DTCM : l'anneau du DAC y est, et les rafales sortent (lot 7e).
* [firmware] L'anneau du DMA est une `static` en DTCM ; aucune maintenance de cache n'est nécessaire.
* [ce module] Un gestionnaire d'interruption à l'adresse 0x0 dans l'ELF est normal : c'est le début de l'ITCM.

### 2.4 Les horloges et les domaines d'horloge

**[principe]** Toute logique numérique avance au rythme d'une horloge. Une puce en a plusieurs, dérivées d'un quartz
par des PLL (multiplicateurs) et des diviseurs. Un **domaine d'horloge** est l'ensemble des circuits qui suivent la
même. Quand deux domaines échangent une donnée (le cœur lit un registre d'un périphérique lent), il faut la
**synchroniser** : attendre un ou plusieurs fronts de l'horloge la plus lente. Plus celle-ci est lente, plus l'accès
coûte.

**[spécifié — réglages de la BSP, `teensy4-bsp` `clock_power.rs`]**

| Horloge | Fréquence | Ce qui en dépend |
|---|---|---|
| quartz | 24 MHz | tout, par PLL |
| cœur (ARM) | 600 MHz | le code, le DWT |
| IPG (bus des périphériques) | 150 MHz | accès aux registres de la plupart des périphériques |
| **PERCLK** | **1 MHz** (quartz ÷ 24) | **PIT et GPT** |
| LPSPI | 132 MHz | horloge fonctionnelle des contrôleurs SPI |

Conséquences :

* [principe] Le cœur et PERCLK dérivent du même quartz : un intervalle de PERCLK est un nombre **exact** de cycles du
  cœur (21 coups de PERCLK = 12 600 cycles), sans dérive.
* [ce module, mesuré] **Chaque accès aux registres d'un périphérique sur PERCLK coûte plusieurs microsecondes**, et
  l'effacement de ses drapeaux met de l'ordre de la microseconde à prendre effet (D-033, révisions du lot 7c et du
  lot 7e). [firmware] Le PIT n'est donc programmé qu'au démarrage, jamais lu en régime.

### 2.5 Les bus et l'ordre des écritures

* **[principe]** Le cœur dépose ses écritures dans un **tampon d'écriture** et continue sans attendre qu'elles
  arrivent. Deux écritures vers deux périphériques placés sur des bus différents peuvent arriver dans le désordre.
* **[spécifié, ARM]** `dsb` (*Data Synchronization Barrier*) attend que les écritures en cours soient terminées vu du
  cœur ; relire le registre attend en plus que l'écriture ait atteint le périphérique.
* **[ce module]** Le CS de l'OLED (GPIO2) et les mots du DAC (LPSPI4) passent par des chemins différents.
  **[firmware]** Après chaque changement du CS de l'OLED : `dsb` puis relecture de la broche (`audio.rs`, `settle`).

---

## 3. Les broches

### 3.1 Multiplexage [spécifié]

Chaque broche physique (un *pad*) peut porter plusieurs fonctions : GPIO, signal d'un SPI, d'un UART… L'**IOMUXC**
choisit laquelle (*alternate function*) et règle le pad (pull-up, hystérésis, vitesse). Exemple : la broche 10 du
Teensy est le pad `GPIO_B0_00`, qui peut être `LPSPI4_PCS0` ; les broches 8 et 6 ne sont reliées à aucun PCS de
LPSPI4 (`imxrt-iomuxc`, table `lpspi.rs`).

### 3.2 GPIO [principe, spécifié]

Une GPIO (*General Purpose Input/Output*) est une broche lue ou écrite par le logiciel. En entrée, un **pull-up**
interne la tire à l'état haut quand rien ne la tire vers la masse (un bouton ouvert) ; l'**hystérésis** évite qu'un
signal lent ou bruité ne fasse changer l'état plusieurs fois. Le détecteur de front d'un port GPIO peut mémoriser
qu'un front a eu lieu entre deux lectures.

[ce module] Les boutons et encodeurs mettent la broche à la masse : sans pull-up, rien ne marche (`board.rs`,
`DIGITAL_INPUT_CONFIG`). Les triggers passent par des transistors inverseurs : un gate au jack lit une broche basse.

---

## 4. Les périphériques internes utilisés

### 4.1 L'ADC interne [spécifié, ce module]

ADC à approximations successives (SAR), 12 bits. [firmware] Conversions bloquantes, une voie après l'autre, à chaque
tick de la boucle (≈ 290 µs pour quatre voies sur ce module, l'essentiel du budget de la boucle). Pistes plus rapides
(timer → ADC_ETC → DMA) : D-023, non faites.

### 4.2 LPSPI, le contrôleur SPI

**[principe] SPI.** Un bus série synchrone : le maître fournit l'horloge (SCK), émet sur MOSI, reçoit sur MISO, et
sélectionne un esclave en mettant **sa** ligne CS (*chip select*) à l'état bas. Chaque front d'horloge fait sortir un
bit *et* en fait entrer un. Un esclave dont le CS est haut ignore le bus : c'est ce qui permet d'en mettre plusieurs
sur les mêmes MOSI et SCK.

**[spécifié — RM, transcrit dans `imxrt-ral` et `imxrt-hal` `lpspi.rs`]**

* **FIFO d'émission de 16 mots de 32 bits** (et autant en réception ; la taille se lit dans `PARAM`).
* Deux registres d'écriture, **consécutifs** : `TCR` (0x60, *Transmit Command Register*) et `TDR` (0x64, *Transmit
  Data Register*). Ce qu'on écrit dans l'un ou l'autre **entre dans la même FIFO, dans l'ordre**.
* `TCR` décrit les trames suivantes : `FRAMESZ` (taille en bits − 1, jusqu'à 4096 bits), `PCS` (lequel des quatre CS
  matériels affirmer), `RXMSK` (ne pas ranger la réception), `CPOL`/`CPHA` (mode SPI), `PRESCALE`, `CONT`
  (continu)…
* **PCS0..PCS3** : CS matériels que le contrôleur affirme lui-même pendant chaque trame, avec des délais réglables
  dans `CCR` (`SCKDIV` : division de l'horloge ; `PCSSCK`, `SCKPCS` : délais CS↔SCK ; **`DBT`** : temps pendant
  lequel le CS reste relevé entre deux trames).
* **Les mots de données** : pour une trame de `8 × n` bits, les octets vont quatre par mot, le premier dans l'octet de
  poids fort, et le dernier mot porte le reste calé à droite (`imxrt-hal`, `pack_word` ; `oc_drivers::fifo`).
* **Écho** : sauf `RXMSK`, un mot reçu est rangé dans la FIFO de réception **une fois son dernier bit
  échantillonné** — donc après que le dernier bit émis est sorti.
* **Drapeaux** : `BUSY` (module occupé), `TCF` (*transfer complete* : revenu au repos, FIFO vide), `FCF` (*frame
  complete* : levé quand le PCS d'une trame retombe), erreurs d'émission et de réception.
* **[spécifié — commentaire d'`imxrt-hal`, `flush`]** `BUSY` peut ne pas être encore levé juste après avoir rempli la
  FIFO.
* [principe, déduit] `TCF` effacé pendant que le bus est au repos se relève aussitôt (sa condition est vraie).

**[ce module]** LPSPI4 : SCK 22 MHz (132 ÷ 6) pour les deux périphériques ; PCS0 sur la broche 10 (DAC) ; l'OLED
utilise **PCS3**, qu'aucune broche ne sort, pour que ses transactions n'affirment jamais le CS du DAC.

**[ce module, mesuré]** `BUSY` + FIFO d'émission vide, puis `TCF`, ont annoncé la fin d'un envoi alors que le dernier
mot sortait encore (l'OLED, sélectionné trop tôt, recevait des bits du DAC). **[firmware]** La fin d'un envoi se lit
dans l'**écho** : un mot reçu par mot envoyé.

### 4.3 Les timers : PIT et GPT

**[principe]** Un timer est un compteur qui avance au rythme d'une horloge et signale (drapeau, interruption, demande
de DMA) quand il atteint une valeur. La cadence est `horloge / nombre de coups` : avec une horloge de 1 MHz, on ne
peut faire que des périodes entières de microsecondes.

**[spécifié]**

* **PIT** (*Periodic Interrupt Timer*) : 4 canaux, chacun décompte de `LDVAL` à 0, recharge, lève `TIF` et, si
  `TIE`, l'interruption (une ligne commune aux 4 canaux). Les canaux 0 à 3 **peuvent déclencher les canaux 0 à 3 du
  DMA** (§ 4.5). Horloge : PERCLK.
* **GPT** (*General Purpose Timer*, GPT1 et GPT2) : compteur montant, trois comparaisons (`OCR1..3`), mode *Restart*
  (repart à 0 sur `OCR1`). Entrées : « horloge périphérique » (`CLKSRC = 1`), « référence haute fréquence »
  (`CLKSRC = 2`), quartz 24 MHz (`CLKSRC = 5` + `EN_24M`).

**[ce module, mesuré]** L'entrée « horloge périphérique » de GPT2 compte à **1 MHz** (PERCLK, pas l'IPG que son nom
suggère) ; la « référence haute fréquence » aussi (déduit : GPT1 y compte en µs). **L'entrée quartz de GPT2 n'a pas
démarré**, même dans l'ordre du manuel ; cause non trouvée (hypothèse : une horloge à ouvrir côté CCM).

**[firmware]** GPT1 : l'horloge en µs du moteur. PIT canal 0 : la cadence audio, 21 coups, **47 619 Hz** (48 kHz
exacts étaient impossibles sur PERCLK).

### 4.4 Interruptions : NVIC, priorités, PRIMASK

**[principe]** Une interruption suspend le programme courant pour exécuter un **gestionnaire** (*ISR*), puis le
reprend. Elle doit être courte, ne pas attendre sans borne, ne pas allouer. Les données partagées avec le programme
principal demandent une exclusion mutuelle : sur un seul cœur, la plus simple est d'**empêcher les interruptions**
le temps de l'accès (une *section critique*).

**[spécifié — ARM Cortex-M]**

* **NVIC** (*Nested Vectored Interrupt Controller*) : une ligne par source, avec masque (`unmask`/`mask`), état en
  attente (*pending*) et priorité (0 la plus haute, valeur de reset). Une interruption de priorité plus haute peut
  interrompre un gestionnaire ; à priorité égale, non.
* **Table des vecteurs** : un tableau d'adresses de gestionnaires, indexé par numéro d'interruption (+ 16).
* **PRIMASK** : un bit qui masque toutes les interruptions configurables d'un coup (`cpsid i` / `cpsie i`).
* Une interruption dont le drapeau source reste levé à la sortie du gestionnaire est rappelée aussitôt.

**[firmware]** `critical-section-single-core` (PRIMASK) pour les sections critiques ; `critical_section::Mutex<RefCell<…>>`
pour l'état partagé. Deux lignes actives, priorité 0 toutes les deux : `DMA0_DMA16` (fin de copie du DMA du DAC) et
`LPSPI4` (trame d'écran terminée). L'USB est interrogé par la boucle, sans interruption.

### 4.5 eDMA et DMAMUX

**[principe] DMA** (*Direct Memory Access*) : un circuit qui copie des données d'une adresse vers une autre sans le
cœur. On le programme une fois (d'où, vers où, combien, à quel signal) ; ensuite il copie seul, à chaque **demande**
(*request*) — d'un périphérique, d'un timer, ou du logiciel. Le cœur est libéré, et surtout l'**instant** de la copie
est fixé par le matériel.

**[spécifié — RM, transcrit dans `imxrt-dma`]**

* **eDMA** : 32 canaux. Chaque canal a un descripteur (**TCD**, *Transfer Control Descriptor*) :
  * `SADDR`, `DADDR` : adresses source et destination ;
  * `SOFF`, `DOFF` : pas après chaque élément ; `SSIZE`/`DSIZE` : taille d'un élément ;
  * `SMOD`, `DMOD` : **modulo d'adresse** — seuls les `n` bits de poids faible de l'adresse varient, ce qui fait une
    fenêtre circulaire de 2^n octets, alignée sur 2^n ;
  * `NBYTES` : octets d'une **boucle mineure** — ce que le canal copie à **chaque demande** ;
  * `CITER` / `BITER` : nombre de boucles mineures d'une **boucle majeure** ;
  * `SLAST`, `DLAST` : ajustement des adresses en fin de boucle majeure ;
  * `INTMAJOR` : lever l'interruption du canal en fin de boucle majeure ; `DREQ` : se désactiver en fin de boucle
    majeure.
* **DMAMUX** : choisit ce qui déclenche chaque canal. Registre `CHCFG[n]` : `ENBL` (bit 31), `TRIG` (bit 30 : la
  demande n'est laissée passer qu'à chaque top du **canal `n` du PIT**, `n` < 4), `A_ON` (bit 29 : demande toujours
  active), `SOURCE` (numéro de source).
* **[ce module, mesuré]** Le DMAMUX de ce chip n'a pas de source « toujours active » numérotée (l'emplacement 54 ne
  donnait aucune demande) : c'est le bit **`A_ON`**, avec `TRIG`, qui donne « une demande à chaque top du PIT ».
  `imxrt-dma` ne sait pas poser ces deux bits ensemble : [firmware] le registre est écrit directement
  (`audio_irq.rs`).

**[firmware] Le DMA du DAC**, en une ligne : à chaque top du PIT canal 0, le canal 0 du DMA copie un emplacement de
32 octets d'un anneau de 256 octets (8 emplacements, `SMOD = 8`, aligné sur 256) vers `TCR`/`TDR` de LPSPI4 en
alternance (`DMOD = 3`) : quatre fois une commande de trame de 24 bits sur PCS0 et le mot d'une voie. Une boucle
mineure par boucle majeure, `INTMAJOR` : chaque copie lève l'interruption qui calcule l'échantillon suivant.

### 4.6 USB [spécifié, firmware]

Le RT1062 a un contrôleur USB natif. [firmware] Un périphérique composite à deux ports série (CDC ACM) : le journal
en texte, la sonde en binaire (`oc-protocol`) ; interrogé par la boucle à 1 kHz.

---

## 5. Notions de signal

### 5.1 Échantillonnage, Nyquist, repliement [principe]

Un signal numérique est une suite d'**échantillons** pris à la **cadence d'échantillonnage** fs. Il ne peut
représenter que des fréquences inférieures à **fs / 2** (la limite de Nyquist). Une composante au-dessus se
**replie** : elle apparaît à une autre fréquence, `|fs − f|` pour une composante entre fs/2 et fs. Une scie ou un
carré « naïfs » (calculés sans précaution) ont des harmoniques infinies : celles qui dépassent fs/2 se replient
(`docs/supersaw/`).

Exemple vécu [ce module] : à 1 kHz (avant la cadence audio), une note de 494 Hz et son image repliée à 506 Hz
battaient à 12 Hz ; au-dessus de 500 Hz, toutes les notes sortaient à ≈ 500 Hz (la voix plafonne le pas sous
Nyquist).

### 5.2 Le bloqueur d'ordre zéro et les images [principe]

Un DAC tient chaque échantillon jusqu'au suivant : la sortie est un escalier. Son spectre contient le signal voulu et
des **images** autour des multiples de fs (la première à fs − f), atténuées dans le rapport ≈ f / (fs − f). Un filtre
analogique après le DAC (dit *de reconstruction*) les enlève normalement.

[ce module] L'étage de sortie (OPA2172, pôle ≈ 72 kHz d'après le schéma rev 2e) ne filtre pas les images de la bande
40–48 kHz. Sans effet à l'oreille ; un accordeur numérique mal filtré peut les replier dans l'audio.

### 5.3 La gigue [principe]

La **gigue** (*jitter*) est l'écart entre l'instant où un échantillon devrait sortir et celui où il sort. Sur une
sinusoïde de fréquence f, un écart Δt est une erreur de phase de 2π·f·Δt : elle grandit avec la fréquence. Une gigue
qui a un rythme module le son et crée des raies parasites.

[ce module, mesuré] Écriture du DAC par une interruption (lot 7c) : ≈ ±3 µs, perceptible à l'accordeur autour de C7.
Écriture par DMA déclenché par le PIT (lot 7e) : l'instant ne dépend plus du logiciel.

### 5.4 Volt par octave [principe]

Convention Eurorack de hauteur : +1 V = une octave plus haut, soit f = f₀ · 2^(V). Une erreur de 1 mV fait
1,2 cent.

---

## 6. Les composants externes

### 6.1 Le DAC8565

**[spécifié — TI, fiche SBAS411C]**

* Quatre voies, 16 bits, interface SPI ; mot de 24 bits : un octet de commande puis le code.
* Octet de commande `0 0 LD1 LD0 0 A1 A0 PD` : `A1 A0` choisit la voie, `LD1 LD0` ce qui arrive au registre de sortie
  (`01` : mettre à jour cette voie tout de suite).
* Horloge SCLK : **50 MHz seulement si alimenté entre 3,6 et 5,5 V, 25 MHz entre 2,7 et 3,6 V** (note 3 des
  caractéristiques temporelles) ; SYNC (le CS) haut **au moins 40 ns** entre deux mots.
* Établissement 8–10 µs ; vitesse de balayage 2,2 V/µs.

**[ce module]**

* Alimentation : 3,3 V (`3V3_A` sur le schéma rev 2e) → plafond 25 MHz.
* Commandes envoyées : `0b0001_0000 | voie << 1` (celles du firmware de référence, `OC_DAC.h`).
* Mesuré : sorties justes à ≤ 3 mV au multimètre après calibration (lot 7a).
* Non vérifié : la mise à jour simultanée des quatre voies.

**[firmware]** 22 MHz (le plus proche sous 25 MHz depuis 132 MHz) ; CS relevé ≥ 45 ns entre deux mots (`MIN_DBT`) ;
les quatre voies écrites à chaque période par le DMA.

### 6.2 L'étage de sortie [ce module, d'après le schéma rev 2e]

OPA2172 en inverseur, gain −4 (24,9 kΩ / 100 kΩ), 22 pF de contre-réaction (pôle ≈ 72 kHz), 220 Ω en série, liaison
continue ; plage ≈ −3,75 à +6,25 V autour d'un *bias* de 1,25 V. **Inverseur** : un code plus grand donne une tension
plus basse — c'est pourquoi la table de calibration des sorties décroît (`Calibration::FACTORY`, D-027).

### 6.3 L'OLED SH1106

**[spécifié — fiche SH1106]**

* 132 × 64 en mémoire, 128 × 64 visibles (décalage de 2 colonnes).
* 8 *pages* de 8 lignes ; un octet = 8 pixels verticaux.
* Ligne **D/C** : basse = commande, haute = donnée.
* Pour écrire une page : `0x10 | colonne_haute`, `0x00 | colonne_basse`, `0xB0 | page`, puis les octets.
* Autres commandes : `0xAE`/`0xAF` (éteindre / allumer), `0x81` (contraste)…

[principe, vécu] Quand l'écran reçoit des octets qui ne lui sont pas destinés, ce sont des commandes valides : des
octets du DAC (0x10..0x16) ont décalé l'image, d'autres l'ont éteinte (lot 7c).

**[ce module]** SH1106 (SSD1306/SSD1309 en option de compilation) ; tient 22 MHz.

**[firmware]** La trame est découpée en tronçons de 20 octets envoyés entre deux rafales du DAC
(`ssd130x::FrameStream`) ; une trame sort en ≈ 1,34 ms.

### 6.4 Entrées CV et triggers [ce module]

Étage d'entrée inverseur (0 V vers le code 2730 de l'ADC, environ −3,3 à +6,7 V lus) ; triggers par transistors
inverseurs et pull-up (actifs à l'état bas). Détails et mesures : `plan/avancement/etape-3.md`, D-019, D-027.

---

## 7. Comment le firmware assemble tout ça [firmware]

Hors du périmètre de ce document : les deux contextes (boucle à 1 kHz, chemin audio), l'écran par tronçons et la
mesure des coûts sont dans `agents/firmware.md` ; le déroulé d'une période dans la documentation de
`crates/oc-firmware/src/audio.rs` ; le couplage du calcul et du DMA dans D-042 ; le récit des essais dans
`plan/rapport-etape-7-chemin-audio.md`.

---

## 8. Glossaire

| Sigle | Développé | En une ligne |
|---|---|---|
| ADC | *Analog-to-Digital Converter* | tension → code |
| AIPS | *AHB-to-IP bridge* | pont entre le bus rapide et les périphériques ; vraisemblablement là que se paie la synchronisation avec un domaine lent (déduit) |
| BSP | *Board Support Package* | crate qui configure la carte (`teensy4-bsp`) |
| CCM | *Clock Controller Module* | règle les horloges et leurs portes |
| CS | *Chip Select* | ligne qui sélectionne un esclave SPI (active basse) |
| D/C | *Data / Command* | ligne de l'OLED : commande ou pixels |
| DAC | *Digital-to-Analog Converter* | code → tension |
| DMA, eDMA | *(enhanced) Direct Memory Access* | copies sans le cœur |
| DMAMUX | *DMA Multiplexer* | ce qui déclenche chaque canal DMA |
| DTCM, ITCM | *Data / Instruction Tightly Coupled Memory* | mémoires rapides du cœur, non cachées |
| DWT, CYCCNT | *Data Watchpoint and Trace*, *Cycle Count* | compteur de cycles du cœur |
| FIFO | *First In, First Out* | file matérielle |
| GPIO | *General Purpose I/O* | broche lue ou écrite par le logiciel |
| GPT | *General Purpose Timer* | timer montant à comparaisons |
| IOMUXC | *I/O Multiplexer Controller* | choix de la fonction et réglage de chaque broche |
| IPG | *IP bus* | horloge du bus des périphériques (150 MHz) |
| ISR | *Interrupt Service Routine* | gestionnaire d'interruption |
| LPSPI | *Low Power SPI* | contrôleur SPI du RT1062 |
| MOSI, MISO, SCK | *Master Out / In*, *Serial Clock* | lignes du SPI |
| NVIC | *Nested Vectored Interrupt Controller* | contrôleur d'interruptions du Cortex-M |
| OCRAM | *On-Chip RAM* | RAM générale, cachée |
| PCS | *Peripheral Chip Select* | CS matériel d'un LPSPI |
| PERCLK | *Peripheral Clock* | horloge des timers (1 MHz ici) |
| PIT | *Periodic Interrupt Timer* | timer à décompte, peut déclencher le DMA |
| PLL | *Phase-Locked Loop* | multiplicateur d'horloge |
| PRIMASK | *Priority Mask* | bit qui masque les interruptions |
| RAL, HAL | *Register / Hardware Abstraction Layer* | `imxrt-ral` (registres bruts), `imxrt-hal` (types sûrs) |
| RM | *Reference Manual* | le manuel du fabricant |
| SAR | *Successive Approximation Register* | type d'ADC |
| TCD | *Transfer Control Descriptor* | description d'un transfert DMA |
| TCR, TDR | *Transmit Command / Data Register* | registres d'émission du LPSPI |
| V/oct | volt par octave | convention de hauteur |

---

## 9. Sources

* NXP, *i.MX RT1060 Processor Reference Manual* (non inclus ; détails lus dans `imxrt-ral` 0.6, `imxrt-hal` 0.6,
  `imxrt-dma` 0.2, `imxrt-iomuxc` 0.3, `teensy4-bsp` 0.6).
* Texas Instruments, *DAC8565*, fiche SBAS411C.
* Sino Wealth, *SH1106*, fiche technique.
* mxmxmx, *O_C* rev 2e, `hardware/o_c_rev2e_schematic.pdf` (github.com/mxmxmx/O_C).
* O_C-Phazerville, `OC_config.h`, `OC_DAC.cpp`, `Main.cpp`, pilote SH1106 (github.com/djphazer/O_C-Phazerville).
* Squares-and-Circles, wiki, page Ornament-and-Crime.
* Mesures [ce module] : `plan/avancement/etape-3.md`, `TESTING.md` § 9, D-033 (révisions des lots 7a, 7c, 7e).
