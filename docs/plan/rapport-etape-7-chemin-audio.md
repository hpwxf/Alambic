# Rapport — étape 7 : le chemin audio, du DAC à 1 kHz au DAC en DMA à 47,6 kHz

Rédigé le 2026-11-18, à la fin du lot 7e, pour servir de source de vérité sur toute l'étape : ce qui a été voulu,
essayé, raté, compris et corrigé, jusqu'à la version qui tourne (`27211a5`). Il complète, sans les remplacer, la
décision [D-033](../../journal/D-033-audio-path.md) et ses révisions, le plan
[etape-7-chemin-audio.md](etape-7-chemin-audio.md), et les révisions de D-008, D-009 et D-010. Les mesures viennent
d'**un seul module** (TLM Audio, Teensy 4.0) : ce sont des observations, jamais des constantes du code.

Conventions : *module* = l'Ornament & Crime réel ; *boucle* = la boucle coopérative à 1 kHz de
`crates/oc-firmware/src/main.rs` ; *interruption audio* = le gestionnaire qui calcule les échantillons ; *période* =
une période audio (21 µs aujourd'hui) ; *tick* = une période de la boucle (1 ms). Les essais sur le module sont
numérotés dans l'ordre où l'utilisateur les a faits.

---

## 1. Point de départ et objectif

### 1.1 Ce que faisait le firmware avant l'étape 7

* **Une seule boucle, à 1 kHz**, sans interruption ni timer dans le chemin du signal (D-008, D-023 option C+). À
  chaque tick : lire les 4 entrées CV (conversions ADC bloquantes, ≈ 290 µs en tout), les triggers, le panneau ;
  appeler `Engine::tick` (applet, calcul des 4 sorties, au plus une action d'écran) ; écrire les 4 voies du DAC ;
  attendre l'échéance suivante en servant l'USB.
* **Le DAC8565** (4 voies de 16 bits) recevait ses 4 mots de 24 bits par le bus SPI `LPSPI4`, à **8 MHz**, la vitesse
  que tolérait l'écran OLED avec lequel il partage le bus. Son *chip select* (CS) était une GPIO pilotée par le
  logiciel. Coût : **≈ 16 µs bloquantes** par tick.
* **L'écran OLED SH1106** recevait sa trame page par page, une page de 128 octets par tick après le dessin
  (≈ 145 µs par page à 8 MHz).
* **Les sorties étaient donc des CV échantillonnées à 1 kHz** : impossible d'y produire un son.

### 1.2 La demande

« Améliorer le chemin du DAC pour générer des sons en sortie du module. » Choix de l'utilisateur, posés au début
(questions à options) : **48 kHz**, **les quatre sorties à la cadence audio**, et **une applet de test `TONE`**
comme premier livrable. L'utilisateur a demandé en cours de route une estimation des chances de tenir 48 kHz sur ce
matériel, puis une vérification sur les fiches techniques (Teensy 4.0, O&C de TLM Audio) avant de lancer le code.

### 1.3 Le budget de 48 kHz

À 48 kHz, une période dure **20,8 µs**. Le chemin d'avant (16 µs de bus pour le DAC, 145 µs pour une page d'écran,
le tout bloquant dans une boucle à 1 kHz) ne tenait ni la cadence ni la régularité. Il fallait :

1. accélérer le bus pour le DAC ;
2. sortir le DAC de la boucle et le cadencer par un timer ;
3. continuer à envoyer l'écran sur le même bus, par petits morceaux, entre deux échantillons ;
4. séparer, dans `oc-core`, ce que l'applet *décide* (une fois par tick) de ce qui *produit les échantillons*
   (48 000 fois par seconde).

---

## 2. Le matériel en jeu (ce qu'il faut savoir pour suivre)

Le détail des acronymes est dans `HARDWARE.md` (non versionné). Ici, l'essentiel pour suivre le récit.

### 2.1 Les horloges du i.MX RT1062 (telles que les règle la BSP `teensy4-bsp`)

| Horloge | Fréquence | Qui l'utilise ici |
|---|---|---|
| Cœur ARM (Cortex-M7) | 600 MHz | le code ; le compteur de cycles DWT (600 cycles/µs) |
| IPG (bus des périphériques) | 150 MHz | accès aux registres de la plupart des périphériques |
| PERCLK | **1 MHz** (quartz 24 MHz ÷ 24) | **PIT** et **GPT** (les timers) |
| LPSPI | 132 MHz | horloge fonctionnelle des contrôleurs SPI ; SCK en est une division |
| Quartz | 24 MHz | source de tout le reste (PLL) |

Deux conséquences ont pesé lourd dans l'étape :

* **48 kHz ne divise pas 1 MHz** (20,83 coups) : un timer sur PERCLK ne peut faire que 47 619 Hz (21 coups) ou
  50 000 Hz (20 coups). Seul le quartz brut de 24 MHz donne 48 kHz exacts (500 coups).
* **Un périphérique cadencé par PERCLK vit dans un domaine d'horloge à 1 MHz** : chaque accès à ses registres (et
  l'effacement de ses drapeaux) doit se synchroniser avec ce domaine lent. Cela coûte **plusieurs microsecondes par
  accès** — découvert au lot 7e, et qui explique rétrospectivement des bizarreries du lot 7c (§ 7 et § 10).

### 2.2 Le bus SPI partagé : LPSPI4

* Broches : MOSI 11, MISO 12 (inutilisé), SCK 13 (aussi la LED de la carte).
* Deux périphériques sur le même MOSI / SCK : le **DAC8565** (CS sur la broche 10) et l'**OLED** (CS broche 8, D/C
  broche 6, reset broche 7).
* **Le contrôleur LPSPI** a une FIFO d'émission de **16 mots de 32 bits** (et autant en réception ; la taille se lit
  dans son registre `PARAM`, c'est ce que fait `imxrt-hal`). On y dépose soit des *commandes de
  transaction* en écrivant le registre `TCR` (taille de trame `FRAMESZ`, quel CS matériel `PCS` affirmer, masques
  de réception `RXMSK`…), soit des *données* en écrivant `TDR`. Les deux registres sont consécutifs (`TCR` à
  0x60, `TDR` à 0x64), et **les deux écritures passent par la même FIFO, dans l'ordre**.
* **Les CS matériels (PCS0..PCS3)** : le contrôleur peut affirmer lui-même un CS pendant chaque trame. La broche 10
  du DAC est muxable en `LPSPI4_PCS0` ; les broches 8 et 6 de l'OLED ne sont reliées à aucun PCS : l'OLED garde un CS
  et un D/C en GPIO.
* **La réception** : en SPI chaque bit émis fait entrer un bit. Sauf masque (`RXMSK`), le contrôleur range dans sa
  FIFO de réception **un mot reçu par mot émis, une fois le dernier bit échantillonné**. Cet « écho » est devenu
  l'outil fiable pour savoir qu'un envoi est vraiment fini (§ 7.5).

### 2.3 Le DAC8565 et l'étage de sortie

* Fiche TI SBAS411C : SCLK **50 MHz seulement entre 3,6 et 5,5 V, 25 MHz entre 2,7 et 3,6 V** ; SYNC (le CS) haut
  au moins **40 ns** entre deux mots ; établissement 8–10 µs ; vitesse de balayage 2,2 V/µs.
* Schéma O&C rev 2e (mxmxmx) : DVDD et AVDD sur `3V3_A` → **plafond 25 MHz**. LPSPI4 étant à 132 MHz, la fréquence
  permise la plus proche est **22 MHz** (132 / 6).
* Un mot = 24 bits : un octet de commande (`0b0001_0000 | voie << 1` : écrire et mettre à jour cette voie) puis le
  code sur 16 bits. ≈ 1,1 µs par mot à 22 MHz.
* Étage de sortie (rev 2e) : OPA2172 inverseur, gain −4, 22 pF de contre-réaction (pôle ≈ 72 kHz), 220 Ω en série,
  plage −3,75 à +6,25 V. **Rien ne filtre les images d'échantillonnage** au-dessus de la bande audio (§ 8).
* Réserve tenue tout du long : la carte TLM T4 n'est pas documentée publiquement ; rev 2e est la meilleure
  approximation.

### 2.4 L'OLED SH1106

* 128 × 64 pixels en 8 *pages* de 8 lignes (128 octets chacune). Pour écrire une page : trois octets de commande
  (colonne haute, colonne basse — avec un décalage de 2 colonnes propre au SH1106 —, numéro de page) avec **D/C
  bas**, puis 128 octets de pixels avec **D/C haut**.
* **Les commandes du SH1106 dans la plage 0x10–0x17 sont « colonne haute »** ; 0xAE éteint l'écran ; 0x81 règle le
  contraste. Ce détail a servi de signature : les octets de commande du DAC (0x10, 0x12, 0x14, 0x16) sont justement
  des commandes valides pour l'écran (§ 7.4).
* L'OLED accepte 22 MHz sur ce module (mesuré au lot 7a).

### 2.5 Timers, DMA, interruptions

* **PIT** (*Periodic Interrupt Timer*) : 4 canaux qui décomptent depuis une valeur de chargement (`LDVAL`) jusqu'à 0,
  rechargent, lèvent un drapeau (`TIF`) et, si demandé, une interruption (une seule ligne pour les 4 canaux). Sur
  PERCLK (1 MHz). **Les canaux 0 à 3 du PIT peuvent déclencher les canaux 0 à 3 du DMA** via le DMAMUX.
* **GPT** (*General Purpose Timer*, GPT1 et GPT2) : compteur montant, 3 registres de comparaison (`OCR1..3`), mode
  *Restart* (le compteur repart à 0 sur `OCR1`). Entrées d'horloge : « horloge périphérique » (`CLKSRC = 1`,
  **mesurée à 1 MHz** sur le module, donc PERCLK), « référence haute fréquence » (`CLKSRC = 2`, celle de GPT1, qui
  compte en µs bien que son diviseur 24 MHz soit sans effet hors quartz : **déduite** à 1 MHz elle aussi), quartz
  24 MHz (`CLKSRC = 5` + `EN_24M`, qui n'a pas démarré). GPT1 sert déjà d'horloge en µs au
  moteur ; GPT2 était libre.
* **eDMA** : 32 canaux. Chaque canal a un *descripteur* (TCD) : adresse source, adresse destination, pas après chaque
  élément (`SOFF`, `DOFF`), *modulo* d'adresse (`SMOD`, `DMOD` : l'adresse ne change que dans ses `n` bits de poids
  faible, ce qui fait une fenêtre circulaire alignée), nombre d'octets par *boucle mineure* (`NBYTES`), nombre de
  boucles mineures dans une *boucle majeure* (`CITER` / `BITER`), ajustements en fin de boucle majeure (`SLAST`,
  `DLAST`), interruption en fin de boucle majeure (`INTMAJOR`). **Une demande matérielle (« request ») = une boucle
  mineure.**
* **DMAMUX** : route une source de demande vers un canal DMA. Registre `CHCFG[n]` : `ENBL` (bit 31), `TRIG`
  (bit 30 : la demande n'est laissée passer qu'à chaque top du canal `n` du PIT), `A_ON` (bit 29 : demande toujours
  active), `SOURCE` (numéro d'emplacement). **Le DMAMUX du RT1060 n'a pas d'emplacement « toujours actif »
  numéroté** : c'est le bit `A_ON` qui joue ce rôle (§ 10.1).
* **NVIC** : contrôleur d'interruptions du Cortex-M. Chaque ligne a un masque (`unmask`/`mask`) et une priorité (0 la
  plus haute, valeur de reset). **PRIMASK** masque toutes les interruptions d'un coup : c'est ce qu'utilise
  `critical-section-single-core` (sections critiques du firmware).
* **DWT `CYCCNT`** : compteur de cycles du cœur (600 par µs), qu'il faut démarrer explicitement. Le profil s'en sert
  depuis D-029.
* **Ordre des écritures** : deux écritures vers deux périphériques sur des bus différents (ici GPIO2 et LPSPI4) ne
  sont pas garanties d'arriver dans l'ordre du programme ; une barrière `dsb` vide le tampon d'écriture du cœur, et
  relire le registre attend que l'écriture ait atteint le périphérique.

### 2.6 La mémoire

* **DTCM** (mémoire étroitement couplée aux données, à 0x2000_0000) : rapide, **non couverte par le cache de
  données**, accessible au DMA. Les `static` du firmware y sont (`.bss`, `.data`). C'est ce qui permet au DMA de lire
  l'anneau sans maintenance de cache.
* **ITCM** (code, à 0x0) : le code y est copié au démarrage — d'où des gestionnaires à l'adresse 0x0 dans l'ELF, ce
  qui est normal.

---

## 3. Les décisions de départ (D-033, 2026-11-17)

Après une recherche sur les fiches et sur deux firmwares de référence :

* **Phazerville** (fork de référence pour O&C sur Teensy 4) : une interruption à **16 666 Hz** qui termine la page
  OLED précédente, écrit les 4 voies du DAC (`LPSPI_TCR_FRAMESZ(23) | LPSPI_TCR_PCS(0)`, mots dans la FIFO), puis
  lance la page suivante par FIFO et interruption LPSPI, sans DMA ; SH1106 à 24 MHz sur le même bus.
* **Squares-and-Circles** : « DAC operation at 48kHz per channel » sur O&C Teensy 4 (wiki du projet).

Décisions (journal/D-033) :

1. **48 kHz**, les 4 sorties, applet `TONE` (choix de l'utilisateur).
2. **CS du DAC en PCS0 matériel**, horloge du DAC à **22 MHz** (sous le plafond de 25 MHz en 3,3 V). Le premier jet
   visait 30 MHz : la fiche l'a corrigé.
3. **Une interruption de timer, priorité la plus haute, qui possède le bus** : DAC chaque période, puis un tronçon
   d'écran qui finit avant la période suivante. La boucle à 1 kHz reste le contexte de contrôle.
4. **Dans `oc-core`, contrôle et signal séparés** : l'applet publie des paramètres une fois par tick ; une *voix*
   calcule un échantillon par période.
5. **DMA différé**, seulement si la mesure le demande (elle l'a demandé, § 8).
6. Les sorties passent à 0 V pendant une sauvegarde en flash (les appels à la ROM masquent les interruptions).

Plan en cinq lots : 7a DAC rapide (encore à 1 kHz) ; 7b cœur audio et `TONE` (hôte) ; 7c interruption ; 7d essai
sur le module ; 7e DMA si nécessaire.

---

## 4. Lot 7a — le DAC rapide, encore à 1 kHz (`b293d6a`)

### 4.1 Ce qui a été fait

* **`DAC_CS` (broche 10) confiée à LPSPI4 en PCS0** (`iomuxc::lpspi::prepare`) : le contrôleur encadre lui-même
  chaque mot de 24 bits. Le pilote `Dac8565` reçoit `ControllerChipSelect`, un CS qui ne fait rien.
* **Le danger identifié avant d'écrire le code** : le contrôleur affirme le PCS *courant* à chaque transaction. Si
  les transactions de l'OLED gardaient PCS0, **le DAC recevrait les octets de l'écran**. D'où : chaque poignée du bus
  partagé choisit son PCS et son horloge avant chacune de ses transactions (`SharedBus::with_select`) ; l'OLED
  utilise **PCS3**, qu'aucune broche ne porte.
* **Horloge par périphérique** possible (`board::select_dac` / `select_oled`), avec une feature `oled-spi-8mhz` pour
  remettre l'OLED à 8 MHz si 22 MHz ne passait pas.
* **Une voie n'est écrite que si son code a changé** (`Dac8565::pending`, un bit par voie), y compris en mode
  simultané.

### 4.2 Mesures sur le module

| Contrôle | Résultat |
|---|---|
| Écran à 22 MHz, toutes applets, une minute d'encodeurs | propre |
| Sorties au multimètre, `cargo probe out` | A et D : −3,000 / +4,997 V ; B et C : −2,999 V (à −3 V) |
| Étape `outputs` du profil (SCOPE + LFO sur CV1, 4 voies changent) | 3,0 µs en moyenne, 8,0 µs au plus (≈ 16 µs avant) |
| Une page d'OLED | 49 µs (≈ 145 µs à 8 MHz) |

Un balayage `cargo probe sweep` B→CV2 et C→CV3 a d'abord montré une entrée immobile (code 2730 = 0 V d'usine) :
**pas de câble**, pas un défaut. Une lecture « −2,999 pour B et C avec out à 3000 » était une faute de frappe
(c'était −3000). L'écart de 3 mV à +5 V relève de la calibration (D-027), pas du bus : un octet de l'écran reçu par
le DAC donnerait des volts d'erreur.

**Décision** : une seule horloge, 22 MHz, pour les deux ; `oled-spi-8mhz` reste comme repli pour un autre module.

---

## 5. Lot 7b — le cœur audio et l'applet `TONE` (`7993063`)

### 5.1 Paramètres de sortie et voix (`oc_core::audio`)

* Une applet ne rend plus des tensions mais des **paramètres de sortie** : `Params { channels: [Channel; 4] }`, avec
  `Channel::Level(mV)` (niveau tenu) ou `Channel::Tone(Tone { wave, frequency: MilliHertz, amplitude: mV })`. Les
  applets de CV existantes rendent `Params::levels(...)` ; le moteur applique par-dessus l'écran des réglages,
  l'assistant de calibration et les niveaux tenus par la sonde.
* **`Voice`** : une cadence fixée à la construction, une **phase de 32 bits par voie** (un tour = 2^32, elle
  déborde toute seule), un *pas de phase* par voie calculé quand les paramètres changent :
  `pas = fréquence_mHz × 2^32 / (cadence × 1000)`, en `u64`. Le pas est **plafonné juste sous un demi-tour** (la
  limite de Nyquist) : au-delà, une tonalité se replierait vers une autre hauteur. `set_params` ne recalcule que si
  les paramètres changent et **garde les phases** : changer de note ne fait pas de clic.
* **Les formes** (`audio::wave`), toutes en entiers dans `-ONE..=ONE` (`ONE = 2^15`) :
  * **sinus** : table d'un quart de période, 257 points, **calculée à la compilation** par une série de Taylor en
    virgule fixe 2.30 (jusqu'à x^13, Horner, `const fn` : pas de flottant dans `oc-core`, D-005), interpolée
    linéairement ; erreur ≤ 1 pas de 2^15 contre `f64` (test) ;
  * triangle, scie, carré déduits de la phase, **sans limitation de bande** (la scie et le carré replient : attendu
    pour un son de test).
* **Le moteur garde sa propre voix à 1 kHz** (une fois par tick) et écrit son échantillon comme avant : rien ne
  change pour les applets de CV ni pour les écrans dorés. `Engine::params()` donne les derniers paramètres à un
  backend qui fait tourner sa voix à sa propre cadence : Rack (`oc_engine_audio`, appelé à chaque `process()`, à la
  fréquence d'échantillonnage de Rack), le firmware (lot 7c).

### 5.2 `TONE`

A sinus, B triangle, C scie, D carré, à la même hauteur ; encodeur gauche = note (C1 à C8, A4 = 440 Hz au départ) ;
encodeur droit = amplitude crête (0 à 3 V, pas de 0,1 V, 1 V au départ) ; CV1 en V/oct s'ajoute à la note.

* Fréquence : `440 Hz × 2^(octaves)` avec `octaves = (note − 69)/12 + cv/1000`, en virgule fixe 16.16 avec
  `fixed::exp2`. **Premier jet faux dans le grave** (−2,5 cents à C1 − 3 V) : l'exponentielle 16.16 perd des bits
  quand le résultat est petit. Correction : exponentielle de la seule fraction d'octave, puis décalage par les
  octaves entières. Reste ≤ 0,1 cent, ou 1 mHz sous 20 Hz (le millihertz devient la dernière décimale).
* Le **menu des applets** a perdu la ligne vide sous son titre (une assertion à la compilation interdisait une
  cinquième applet).

### 5.3 Essai sur le module, encore échantillonné à 1 kHz

L'utilisateur a essayé `TONE` alors que le firmware n'échantillonnait qu'une fois par tick : **A#4 « un peu moche »,
B4 « scintille », C5 et au-dessus « toutes la même note »**. Explication, conforme au code :

* à 1 kHz, la limite de Nyquist est 500 Hz ;
* A#4 = 466 Hz : son image repliée à 1000 − 466 = 534 Hz se mélange au son ;
* B4 = 494 Hz : son et image à 12 Hz l'un de l'autre → battement, le « scintillement » ;
* C5 = 523 Hz et plus : le pas plafonné juste sous un demi-tour → toutes les notes à ≈ 500 Hz.

Rien à corriger dans `oc-core` : il fallait la cadence audio du lot 7c.

---

## 6. Lot 7c — la conception de l'interruption (`21cdbb6`)

### 6.1 Ce qui a été écrit

* **Timer : GPT2 sur le quartz de 24 MHz**, 500 coups, mode *Restart* sur `OCR1` → 48 000 Hz exacts. Le PIT avait
  été écarté parce qu'il est sur PERCLK (1 MHz).
* **Déroulé d'une période** (une seule interruption, qui attend le bus) :
  1. attendre que le tronçon d'écran précédent soit sorti, relever le CS de l'OLED ;
  2. déposer dans la FIFO les mots des voies du DAC qui ont changé (`TCR` 24 bits sur PCS0 + mot) ;
  3. pendant leur envoi : relever la boîte aux lettres, calculer l'échantillon suivant (`Voice::next_sample` puis
     calibration en codes du DAC) ;
  4. attendre la fin des mots du DAC, puis placer D/C, baisser le CS de l'OLED, déposer un tronçon (`TCR` de
     `8 × n` bits sur PCS3).

  L'échantillon calculé en période *n* sort en période *n + 1* : un échantillon de latence, sans gigue de calcul.
* **Côté boucle** : `audio::MutedOutputs` (l'`AnalogOut` du moteur ne fait plus rien), `audio::MailboxDisplay` (le
  `present` du moteur copie la trame de 1 Kio dans une boîte aux lettres, la dernière gagne), `audio::publish`
  après chaque tick (paramètres + calibration des sorties). Le partage passe par des
  `critical_section::Mutex<RefCell<…>>`.
* **Dans `oc-drivers`, testés sur l'hôte** :
  * `ssd130x::FrameStream` : une trame découpée en morceaux, soit commandes soit pixels, jamais les deux, **les mêmes
    octets que le pilote bloquant** (test qui compare les deux) ; un changement de contraste attend la frontière
    entre deux pages ;
  * `fifo::words` : des octets en mots de FIFO, quatre par mot, premier octet en haut, le dernier mot calé à droite
    — la règle du LPSPI, et ce que fait `imxrt-hal` (`pack_word`) ;
  * `dac8565::channel_word` (le mot de 24 bits calé à droite) et `ChangedCodes` (les voies qui changent).
* **Écart entre deux mots du DAC porté à 45 ns** : à 22 MHz, `imxrt-hal` laisse le PCS relevé 4 cycles de 132 MHz
  (≈ 30 ns), sous les 40 ns de la fiche. Les écritures bloquantes du lot 7a laissaient sans le vouloir assez
  d'écart ; mises à la suite par l'interruption, non. `board::MIN_DBT` élargit le délai entre transferts (`DBT`).
* **Profil** : trois étapes nouvelles, remplies par l'interruption et versées dans le profil de la boucle à chaque
  tick (`Profile::merge`) : `audio` (durée de l'interruption), `audio_period` (temps entre deux), `frame` (envoi
  d'une trame entière).
* **`unsafe`** : seulement l'inscription du gestionnaire et le démasquage NVIC, dans un module à part
  (`audio_irq.rs`).
* Le moteur ne pagine plus l'écran sur le module (`PagedPanel` supprimé).

---

## 7. Lot 7c sur le module — six essais (`21cdbb6` → `69e9099`)

### 7.1 Essai 1 — écran noir, et l'interruption ne tourne pas

**Symptôme** : écran figé noir au démarrage ; le journal USB va jusqu'au bout ; la sonde répond.

**Raisonnement** :

* La sonde répond, donc la boucle tourne : l'interruption n'est **pas** bloquée dans une attente (une interruption
  de priorité maximale qui ne rend jamais la main affamerait aussi l'USB).
* `cargo probe stats` n'affiche **aucune ligne `audio`** : l'interruption ne s'exécute jamais.
* Vérifications hors module : l'entrée 101 (GPT2) de la table des vecteurs, extraite de l'ELF avec
  `llvm-objcopy`, pointe bien sur le gestionnaire (adresse 0x0 + bit Thumb, normal en ITCM) ; le mode SPI des
  transactions est le même que celui de la BSP (mode 0) ; aucun masquage global (PRIMASK) dans le firmware.
* Suspect : la configuration de GPT2 sur le quartz. `imxrt-hal` écrit `CLKSRC = 5` et `EN_24M = 1` dans la **même**
  écriture ; le manuel décrit une séquence (source, puis remise à zéro logicielle).

**Correctif d'essai** : GPT2 sur l'entrée « horloge périphérique » (`ClockSource::PeripheralClock`), supposée être
l'IPG à 150 MHz (3 125 coups = 48 kHz), et **un auto-test de 2 ms au démarrage**, écrit dans le journal : nombre
d'exécutions de l'interruption, compteur du timer au début et à la fin, drapeau de comparaison, état de la ligne
NVIC (active, en attente). Cet auto-test a ensuite servi à chaque essai.

### 7.2 Essai 2 — l'interruption tourne, mais à 320 Hz

**Journal** : 0 exécution en 2 ms, compteur 1 → 2007, drapeau baissé, ligne active. **Stats** : `audio_period`
= 3 125 µs, `frame` = 175 ms. **Écran** : cinq petites barres en bas.

**Raisonnement** : 2 006 coups en 2 ms = **1 MHz**. Sur ce chip, l'entrée « horloge périphérique » de GPT2 est
PERCLK, pas l'IPG : 3 125 coups à 1 MHz = 3,125 ms = **320 Hz**. Une trame de ≈ 56 tronçons met donc 175 ms : les
« barres » étaient le début d'une trame, l'écran se remplissant très lentement. L'auto-test, fait juste après le
démarrage, ne voyait aucune exécution parce que la première comparaison tombait après lui.

**Correctif** : refaire l'essai du quartz **dans l'ordre du manuel** (désactiver, couper les interruptions, choisir
la source, remise à zéro logicielle `SWR`, rechoisir la source au cas où la remise à zéro l'efface, puis le reste),
et **vérifier au démarrage** qu'une comparaison tombe dans les 25 µs ; sinon **repli sur PERCLK ÷ 21 = 47 619 Hz**,
la voix construite à cette cadence (les hauteurs restent justes : l'erreur d'arrondi de la cadence entière est de
l'ordre du ppm).

### 7.3 Essai 3 — repli sur PERCLK, l'interruption s'exécute deux fois, l'écran est décalé

**Journal** : `on Perclk at 47619 Hz`, **153 exécutions en 2 ms** (≈ 95 attendues), compteur 1 → 20, drapeau levé.
**Écran** : « décalage de page et périodicité verticale », ne correspond pas à `cargo probe screen` ; dans `TONE`,
l'écran « saute » puis devient noir, **mais le son sort**. La sonde répond lentement.

Deux défauts distincts :

1. **Double déclenchement.** Le drapeau de comparaison de GPT2 vit dans le domaine PERCLK (1 MHz) : son effacement
   met quelques microsecondes à prendre effet. Une exécution courte (pas de mot du DAC, pas de tronçon) rendait la
   main avant : le drapeau encore levé relançait aussitôt l'interruption. D'où 153 au lieu de 95, et une boucle
   ralentie (sonde lente). L'exemple GPT de la BSP fait d'ailleurs `clear_elapsed` puis `while is_elapsed {}`.
   *Correctif* : attendre que l'effacement ait pris, dans la limite d'une demi-période.
2. **L'OLED reçoit des bits du DAC.** Indices : le défaut s'aggrave quand le DAC écrit à chaque période (`TONE`) ;
   les octets de commande du DAC (0x10..0x16) sont des commandes « colonne haute » du SH1106 (décalage de l'image) ;
   les codes du DAC, eux, peuvent valoir 0xAE (« éteindre »), d'où l'écran noir. Hypothèse retenue : le relâchement
   du CS de l'OLED (une écriture sur GPIO2) et le premier mot du DAC (une écriture sur LPSPI4) **passent par deux bus
   différents qui ne garantissent pas l'ordre** ; le DAC commence à recevoir alors que le CS de l'OLED est encore bas.
   *Correctif* : après chaque changement du CS, `dsb` puis relecture de la broche jusqu'à voir le bon niveau.
   L'essai 4 a montré que ce n'était pas la cause principale ; on ne sait pas si ce correctif était nécessaire. Il
   est resté, par prudence : il coûte quelques dizaines de nanosecondes.

### 7.4 Essai 4 — `RATCHET` correct, `TONE` saute encore

**Journal** : 92 exécutions en 2 ms (le double déclenchement a disparu). **Écran** : bon dans `RATCHET`, qui écrit
rarement le DAC ; dans `TONE`, il saute encore puis s'éteint.

**Raisonnement** : la corruption suit le trafic du DAC, donc il reste un moment où l'OLED est sélectionné pendant
que des bits du DAC passent. Le seul autre point de bascule est **l'attente « bus libre » après les mots du DAC**,
avant de baisser le CS de l'OLED. Elle reposait sur `BUSY` faux et une FIFO d'émission vide — exactement ce que
`imxrt-hal` met en garde : « *the busy flag may not set once the TX FIFO is filled* ». Quand le dernier mot est
passé de la FIFO au registre à décalage, la FIFO est vide alors que ses bits sortent encore.

*Correctif essayé* : effacer le drapeau « transfert terminé » (`TCF`) avant chaque envoi, attendre qu'il se relève.

### 7.5 Essai 5 — le drapeau « transfert terminé » ne change rien

**Résultat** : identique, `RATCHET` correct, `TONE` qui saute. Statistiques de ce firmware (dans `TONE`) : `audio`
12,3 µs en moyenne, `audio_period` 17,9 – 24 µs, `frame` 1,18 ms.

**Raisonnement** : `TCF` se lève « quand le contrôleur revient au repos et que la FIFO est vide ». Or on l'efface
*quand le bus est déjà au repos* : la condition étant vraie, il se relève aussitôt, et l'attente retombait sur
l'ancien test. C'est une déduction (on n'a pas lu le drapeau sur le module), cohérente avec le symptôme inchangé.

### 7.6 Essai 6 — l'écho en réception règle le problème

**Raisonnement** : la seule preuve sûre qu'un mot est entièrement sorti est **son écho** : le contrôleur ne range un
mot reçu qu'après en avoir échantillonné le dernier bit. C'est ainsi que se terminent les écritures bloquantes
d'`imxrt-hal` (qui lisent ce qu'elles ont reçu), et c'est pour cela que le lot 7a marchait.

*Correctif* : ne plus masquer la réception ; compter les mots envoyés ; attendre autant de mots reçus ; vider la
FIFO de réception. **Écran stable dans toutes les applets, son présent.** Commit `69e9099`.

**Mesures (`TONE`)** : `audio` 12,5 µs en moyenne, 14,9 µs au plus (≈ 60 % du processeur) ; `audio_period` 17,9 à
24 µs, soit **≈ ±3 µs de gigue** sur l'instant d'écriture du DAC (l'interruption est retardée par les sections
critiques de la boucle, et sa durée varie) ; `frame` 1,18 ms ; boucle à 1 000 µs, gigue 8 à 16 µs (1 à 4 avant).

---

## 8. Lot 7d — à l'oreille et à l'accordeur

**Observations de l'utilisateur** : de C1 à C6, accordé et stable ; à C7, petites perturbations (« ±03 » sur
l'accordeur, a priori des cents) ;
au-dessus, l'accordage ne tient pas ; précision ensuite : C8 est lu comme C7.

**Analyse** (sinus sur A ; la scie et le carré replient de toute façon) :

* **Gigue d'échantillonnage** : une erreur de Δt sur l'instant d'écriture décale la phase de 2π·f·Δt. Avec ±3 µs :
  C6 (1 047 Hz) 1,1°, raies à ≈ −34 dB ; C7 (2 093 Hz) 2,3°, ≈ −28 dB ; C8 (4 186 Hz) 4,5°, ≈ −22 dB. La gigue vient
  surtout de la boucle à 1 kHz : modulation à 1 kHz, vue comme une hauteur instable.
* **Images d'échantillonnage** : un DAC tient chaque échantillon jusqu'au suivant (« bloqueur d'ordre zéro ») ;
  l'image de f est à fs − f, atténuée dans le rapport f / (fs − f). C6 : 46,6 kHz à −33 dB ; C7 : 45,5 kHz à
  −27 dB ; C8 : 43,4 kHz à −20 dB. L'étage de sortie (pôle à 72 kHz) ne les filtre pas ; un accordeur numérique mal
  filtré les replie dans l'audio.
* Les deux effets sont du même ordre ; un filtre RC entre la sortie et l'accordeur les départagerait (proposé, pas
  fait).
* **C8 lu C7** : le nom de note juste, l'octave fausse — erreur d'octave typique d'un accordeur en limite de gamme
  (autocorrélation), pas du module (`frequency(108, 0)` = 4 186,0 Hz, testé). Vérification proposée : la période à
  l'oscilloscope (239 µs), ou C7 + 1 V sur CV1.

**Décision de l'utilisateur** : lancer le lot 7e (DMA), qui supprime la gigue et la charge de l'interruption.

---

## 9. Lot 7e — la conception du DMA

### 9.1 L'idée

Faire écrire le DAC **par le matériel**, à l'instant d'un timer, sans logiciel entre les deux :

```
PIT canal 0 (PERCLK, 21 coups)  ──top──►  DMAMUX canal 0 (TRIG)  ──►  DMA canal 0
                                                                          │ une boucle mineure = 32 octets
                                                                          ▼
                          anneau en DTCM : [TCR, mot A, TCR, mot B, TCR, mot C, TCR, mot D] × 8 emplacements
                                                                          │ destination : TCR, TDR, TCR, TDR…
                                                                          ▼
                                                 FIFO de LPSPI4 ──► 4 trames de 24 bits sur PCS0 ──► DAC8565
```

* **La cadence devient 47 619 Hz** (PERCLK ÷ 21) : seul un canal 0–3 du PIT peut déclencher le DMA, et le PIT est
  sur PERCLK. Le quartz de GPT2 n'avait de toute façon pas démarré.
* **Alterner `TCR` et `TDR`** : chaque mot du DAC doit être précédé de sa commande de transaction (24 bits, PCS0),
  parce que les tronçons d'écran en ont mis une autre entre-temps. `TCR` et `TDR` étant consécutifs et la fenêtre
  0x60–0x67 alignée sur 8 octets, un modulo de destination `DMOD = 3` fait alterner la destination entre les deux.
* **Les quatre voies à chaque période** : un DMA de taille fixe ne sait pas sauter une voie inchangée. Le gain du lot
  7a (« n'écrire que ce qui change ») est perdu pour le DAC : ≈ 5,5 µs de bus par période en permanence.
* **Écho gardé pour le DAC** (`RXMSK = 0`) : il compte les rafales ; **masqué pour l'écran** (`RXMSK = 1`).
* `DAC_TCR` est composé à la main depuis les champs du RAL (`FRAMESZ = 23`, `PCS = 0`) : le `Transaction`
  d'`imxrt-hal` ne livre pas sa valeur brute pour la mettre dans un tampon.

### 9.2 Première conception de l'interruption (abandonnée)

Un second canal du PIT, démarré ≈ 13 µs avant le premier, devait tomber ≈ 8 µs après chaque rafale ; l'écran devait
attendre la fin de la rafale ; le CS de l'OLED devait être relâché par l'interruption « trame terminée » de LPSPI4
(drapeau `FCF`, levé quand le PCS de la trame retombe, après son dernier bit). Le placement du tronçon se décidait
en lisant le compteur du PIT. Cette partie a été corrigée trois fois (§ 10).

---

## 10. Lot 7e sur le module — quatre essais (→ `27211a5`)

### 10.1 Essai 1 — aucune rafale, écran noir

**Journal** : 254 interruptions et **0 rafale DAC** en 2 ms, aucune erreur DMA. **Stats** : `audio` 12,9 µs
constants. **Écran** : noir.

**Raisonnement** :

* 0 rafale sans erreur DMA : le canal n'est jamais *demandé*. L'emplacement 54 (« `DMAMUX_SOURCE_ALWAYS0` », repris
  de mémoire de Teensyduino, sans pouvoir le vérifier) ne donne aucune demande sur ce chip. Le RAL montre le bit
  `A_ON` dans `CHCFG` : la demande « toujours active » est un bit, pas un emplacement. Or `imxrt-dma` ne pose `TRIG`
  qu'avec une source (`Configuration::Enable { source, periodic }`) et `A_ON` que sans `TRIG`
  (`Configuration::AlwaysOn`).
* `audio` constant à 12,9 µs : la boucle d'attente de l'effacement du drapeau (reprise du 7c) tournait jusqu'à la
  demi-période.
* Écran noir : hypothèse du moment, la fenêtre pour un tronçon trop étroite (décision prise après le calcul de
  l'échantillon). La vraie cause, établie à l'essai 3 (§ 10.3), était la phase de l'interruption : elle tombait trop
  tôt après la rafale pour qu'un tronçon y ait jamais sa place.

**Correctifs** : `CHCFG[0] = ENBL | TRIG | A_ON` écrit directement par le RAL ; décision du tronçon au début de
l'interruption, tronçons de 12 octets ; plus d'attente active, mais un **filtre** : une exécution moins d'une
demi-période après la précédente n'est qu'un drapeau revu, elle l'efface et rend la main ; compteurs de tronçons et
de relâchements dans l'auto-test.

### 10.2 Essai 2 — « 1 interruption », `audio` à 10,3 µs

**Journal** : 1 interruption, 0 rafale, 0 tronçon. **Stats en régime** : ≈ 47 600 exécutions par seconde (donc
l'interruption tourne), `audio` 10,3 µs alors qu'elle n'attend plus rien.

**Raisonnement** :

* **Le compteur de cycles DWT était arrêté pendant l'auto-test** : il n'était démarré que par `SystemClock::new`,
  créée *après* l'audio. L'interruption lisait donc toujours la même valeur : chaque exécution semblait arriver
  0 cycle après la précédente, et le filtre les rejetait toutes, sauf la première. Les rafales n'étant comptées que
  sur les exécutions acceptées, **« 0 rafale » ne prouvait rien** : le DMA marchait peut-être déjà.
* **10,3 µs** : l'interruption lisait le compteur du PIT à chaque période (`current_timer_value`, qui lit aussi
  `TCTRL`). **Chaque accès au PIT attend son domaine de 1 MHz.** C'est probablement aussi l'explication, après
  coup, de l'« effacement lent du drapeau » du lot 7c (GPT2, lui aussi sur PERCLK) : chaque lecture du drapeau
  coûtait déjà ce temps. Non vérifié par une mesure isolée.

**Correctifs** : démarrer le DWT en tête de `main` ; ne lire le PIT qu'une fois (à la première exécution), puis
suivre la phase au compteur de cycles. PERCLK et le cœur viennent du même quartz par PLL : une période fait
**exactement 12 600 cycles** (600 × 21), sans dérive.

### 10.3 Essai 3 — le DMA marche, l'écran reste noir

**Journal** : 62 interruptions, **61 rafales**, 0 tronçon, aucune erreur. **Stats** : `audio` 4,8 µs.

**Raisonnement** : le DMA est validé. Plus aucun tronçon ne part parce que **les écritures dans le PIT sont lentes
elles aussi** : l'avance donnée au canal de l'interruption (activer le canal 1, attendre 14 µs, activer le canal 0)
s'est allongée de la durée de ces écritures. L'interruption tombait ≈ 5 µs après la rafale, sous le seuil de 6 µs
exigé avant un tronçon : jamais de place.

**Correctif, sur le principe plutôt que sur le réglage** : **supprimer le second canal du PIT et faire déclencher
l'interruption par le DMA lui-même.**

* Une itération par boucle majeure (`BITER = CITER = 1`) : chaque top est une boucle majeure complète, dont la fin
  lève l'interruption du canal (`INTMAJOR`, ligne `DMA0_DMA16`).
* L'anneau n'est plus parcouru par `SLAST` mais par le **modulo d'adresse source** (`SMOD = 8`) : l'adresse avance de
  32 octets par top et revient au début tous les 256 octets. Il faut pour cela que l'anneau soit **aligné sur sa
  taille** (`#[repr(align(256))]`, vérifié dans l'ELF : `RING` à 0x2000_B100).
* L'interruption tombe donc juste après chaque copie, **par construction**. Elle calcule l'échantillon pendant que la
  rafale sort, **attend son écho** (≈ 2–3 µs, borné à 12 µs pour ne jamais rester bloquée), puis lance un tronçon de
  20 octets si la place jusqu'au top suivant le permet, mesurée au compteur de cycles depuis l'exécution la plus
  précoce observée.

### 10.4 Essai 4 — écran et son corrects

**Journal** : 60 interruptions, 59 rafales, 0 tronçon (normal : le moteur n'a pas encore présenté de trame à ce
moment du démarrage), aucune erreur. **Stats (`TONE`)** : `audio` **3,4 µs** en moyenne, 6,1 au plus ;
`audio_period` 18,4 – 23,5 µs (ne mesure plus que l'entrée de l'interruption) ; `frame` 1,34 ms ; boucle à
1 000 µs. **Écran et son corrects.** Commit `27211a5`.

Remarque : l'auto-test compte ≈ 60 événements en « 2 ms » au lieu de ≈ 95. En régime, le profil compte bien
47 600 exécutions par seconde. Explication probable, non vérifiée : `CycleDelay` repose sur
`cortex_m::asm::delay`, qui compte des itérations de boucle supposées durer un cycle ; sur un M7 (double émission,
prédiction de branchement), elles vont plus vite, et « 2 ms » durent moins.

---

## 11. L'architecture qui tourne (`27211a5`)

### 11.1 Une période de 21 µs

```
t = 0       PIT canal 0 : top.  DMAMUX (ENBL|TRIG|A_ON) → DMA canal 0 copie l'emplacement k de l'anneau :
            8 écritures dans TCR/TDR de LPSPI4 (< 1 µs). Fin de boucle majeure → interruption DMA0_DMA16.
t ≈ 0,5     Interruption audio (priorité 0) :
              - efface le drapeau d'interruption du DMA ; filtre les doubles ;
              - compte les rafales terminées par leur écho (4 mots reçus chacune) → position du DMA dans l'anneau ;
              - prend la boîte aux lettres (paramètres, calibration, trame, contraste) ;
              - calcule l'échantillon suivant (Voice) → 4 codes (calibration) → l'écrit à l'emplacement k + 2 ;
              - attend l'écho de la rafale en cours (≈ jusqu'à t ≈ 5,5) ;
              - si la place restante ≥ tronçon (9 µs) + relâchement (3 µs) : D/C, CS de l'OLED bas (dsb +
                relecture), un TCR d'une trame de 160 bits (20 octets) sur PCS3, réception masquée, puis
                5 mots de pixels ; active FCF ;
              - rend la main (≈ 3,4 µs en moyenne).
t ≈ 5,5–13  le tronçon d'écran sort sur le bus (20 octets à 22 MHz, ≈ 7,3 µs).
t ≈ 13      LPSPI4 « trame terminée » (FCF) → interruption : relâche le CS de l'OLED (dsb + relecture), coupe FCF,
            remet l'horloge du DAC si elle diffère (feature oled-spi-8mhz).
t = 21      top suivant.
```

### 11.2 Le flux de données

```
boucle à 1 kHz : Engine::tick → Engine::params() ─► audio::publish ─┐
                 Engine (dessin) → MailboxDisplay::present ─────────┤  MAILBOX (critical_section::Mutex)
                 set_contrast ──────────────────────────────────────┘
                                                                     │
interruption DMA : Voice::next_sample → calibration.to_code ─► RING (audio_irq::write_slot, k + 2)
                   FrameStream::next_chunk ─► FIFO de LPSPI4 (PCS3) ─► OLED
DMA (matériel)   : RING ─► TCR/TDR de LPSPI4 (PCS0) ─► DAC8565
```

La latence entre un échantillon calculé et sa sortie est de 2 à 3 périodes (≈ 60 µs). Les paramètres publiés par la
boucle prennent effet au plus 1 ms plus tard (un tick).

### 11.3 Sauvegarde et redémarrage

* **SAVE** : `audio::rest_outputs` publie le repos, attend 5 périodes (pris, écrit deux emplacements plus loin, sorti
  deux plus tard), puis **met le DMA en pause**. Sinon, interruptions masquées par la ROM pendant des dizaines de
  millisecondes, personne ne viderait la FIFO de réception : pleine après quatre rafales, elle arrête le
  contrôleur, et les écritures suivantes du DMA déborderaient la FIFO d'émission ; après l'écriture,
  `audio::resume_outputs` le relance là où il s'était arrêté dans l'anneau.
* **Passage au bootloader** : `audio::stop` masque les deux lignes et arrête le DMA.

### 11.4 Inventaire de l'`unsafe` (`crates/oc-firmware/src/audio_irq.rs`)

| Bloc | Pourquoi c'est `unsafe` | Ce qui le rend sûr |
|---|---|---|
| `Ring` + `unsafe impl Sync` | un tampon partagé avec le DMA | un seul écrivain CPU (l'interruption), le DMA lit d'autres emplacements |
| `write_slot` (`write_volatile`) | écriture brute dans l'anneau | index borné, emplacement que le DMA ne lira pas avant deux tops |
| `start_dma` (TCD) | le DMA lira et écrira ces adresses | source = l'anneau statique, bornée par le modulo ; destination = `TCR`/`TDR`, fenêtre de 8 octets |
| `DMAMUX::instance()` + `CHCFG` | accès brut au registre | le registre de ce seul canal, écrit nulle part ailleurs |
| `enable` / `resume_dma` | le DMA démarre | transfert entièrement décrit, sur des mémoires statiques |
| `NVIC::unmask` | peut casser une section critique par masquage de ligne | les sections critiques du firmware masquent tout (PRIMASK) |
| `#[interrupt]` (`DMA0_DMA16`, `LPSPI4`) | symboles exportés | gestionnaires qui ne passent que par les `Mutex` |

### 11.5 Constantes (`crates/oc-firmware/src/board.rs`)

| Constante | Valeur | Rôle |
|---|---|---|
| `AUDIO_TIMER_HZ` | PERCLK, 1 MHz | horloge du PIT |
| `AUDIO_PERIOD_COUNTS` | 21 | une période |
| `AUDIO_RATE_HZ` | 47 619 | cadence de la voix |
| `AUDIO_DMA_CHANNEL` | 0 | canal DMA = canal du PIT qui le déclenche |
| `DAC_SPI_HZ` / `OLED_SPI_HZ` | 22 MHz / 22 MHz (8 avec la feature) | horloges du bus |
| `MIN_DBT` | 4 (6 cycles, 45 ns) | CS relevé entre deux mots du DAC |
| `DAC_BURST_MICROS` | 6 | durée d'une rafale, arrondie |
| `OLED_RELEASE_MICROS` | 3 | marge pour relâcher le CS avant le top |
| `OLED_CHUNK_BYTES` | 20 à 22 MHz (8 à 8 MHz) | taille d'un tronçon (budget 8 µs) |
| `OLED_CHUNK_MICROS` | 9 à 22 MHz (9 à 8 MHz) | durée d'un tronçon, arrondie, plus 1 µs |

Une assertion à la compilation vérifie que rafale + tronçon + relâchement tiennent dans une période.

### 11.6 Ce que le profil et le journal disent

* `cargo probe stats` : `audio` (durée de l'interruption), `audio_period` (temps entre deux entrées : la gigue
  *d'entrée*, plus celle du DAC), `frame` (une trame entière).
* Journal de démarrage : `audio at 47619 Hz, DAC by DMA, N screen bytes a period: … interrupts, … DAC bursts, …
  screen chunks and … releases in 2 ms, line enabled … pending …, DMA error … (status …)`.

---

## 12. Ce que l'on retient (pièges de ce matériel)

1. **Lire la fiche du composant avant de choisir une horloge** : le DAC8565 en 3,3 V plafonne à 25 MHz, pas 50.
2. **Un CS matériel s'affirme à chaque transaction** : tout ce qui partage le bus doit choisir son PCS (ici PCS3,
   sans broche, pour l'OLED).
3. **Enchaîner les mots dans la FIFO change les timings** que des écritures bloquantes masquaient (écart de 40 ns
   du DAC).
4. **Les noms d'horloge mentent selon le chip** : sur le RT1062, l'entrée « périphérique » de GPT est PERCLK
   (mesuré), la « haute fréquence » aussi (déduit) ; seul le quartz de GPT2 aurait donné 24 MHz, et il n'a pas
   démarré.
5. **Un périphérique à 1 MHz coûte des microsecondes à chaque accès** (lecture, écriture, effacement de drapeau) :
   ne jamais lire le PIT ou un GPT sur PERCLK à chaque période.
6. **Effacer un drapeau ne prend pas effet tout de suite** dans un domaine lent : une interruption courte peut être
   rappelée aussitôt ; filtrer les doubles plutôt qu'attendre.
7. **`BUSY` et `TCF` ne disent pas qu'un envoi est fini** ; l'écho en réception, si.
8. **Deux bus, deux ordres** : une écriture GPIO et une écriture SPI ne sont pas ordonnées sans `dsb` + relecture.
9. **Le DMAMUX du RT1060 n'a pas de source « toujours active » numérotée** : `ENBL | TRIG | A_ON`.
10. **Un DMA cyclique sans `SLAST` passe par un modulo**, donc par un tampon aligné sur sa taille.
11. **Un instant matériel vaut mieux qu'une phase réglée à la main** : déclencher l'interruption par la fin de copie du
    DMA a supprimé toute dépendance au décalage de deux timers.
12. **Démarrer les compteurs dont dépend la logique avant cette logique** (le DWT).
13. **Ajouter un auto-test au journal à chaque inconnue matérielle** : chaque essai a tranché une hypothèse en un
    flash.

---

## 13. Ce qui reste ouvert

* **Justesse autour de C7 et C8** à repasser à l'accordeur, sur le sinus (A), maintenant que la gigue d'écriture a
  disparu ; s'il reste un tremblement, ce sont les images (essai du filtre RC proposé).
* **Le quartz de GPT2** ne démarre pas ; hypothèse non vérifiée : l'horloge `ipg_clk_24M` doit être ouverte côté CCM.
  Sans enjeu tant que le DMA est déclenché par le PIT (qui ne peut pas utiliser le quartz).
* **Les quatre voies sont écrites à chaque période** (≈ 5,5 µs de bus en permanence) ; `ChangedCodes` reste dans
  `oc-drivers`, testé, mais le firmware ne s'en sert plus.
* **L'auto-test compte « 2 ms » trop courtes** (`CycleDelay`) : à rendre exact (DWT) si l'on veut des comptes
  absolus.
* **Gigue d'entrée de l'interruption** (18,4 – 23,5 µs) : sans effet sur le DAC, mais elle réduit la marge du tronçon
  d'écran ; un tronçon qui ne trouve pas sa place est simplement reporté.
* **Rack** : `oc_engine_audio` n'a été ni compilé (pas de SDK sur la machine) ni écouté.
* **`HARDWARE.md`** n'est pas versionné (choix de l'utilisateur en attente).
* Le compte rendu court de l'étape (`docs/plan/avancement/etape-7.md`) et la PR restent à faire quand l'étape sera
  close.

---

## 14. Index

### 14.1 Commits

| Commit | Lot | Contenu |
|---|---|---|
| `336817a` | plan | D-033, plan de l'étape 7 |
| `b293d6a` | 7a | PCS0 pour le DAC, PCS3 pour l'OLED, 22 MHz, voies écrites au changement |
| `7993063` | 7b | `oc_core::audio` (paramètres, voix, formes), `TONE`, Rack `oc_engine_audio` |
| `21cdbb6` | 7c | interruption GPT2, `FrameStream`, `fifo`, boîtes aux lettres, étapes du profil |
| `69e9099` | 7c | correctifs du module (essais 2 à 6) : repli PERCLK, double déclenchement, écho en réception, ordre GPIO/SPI |
| `27211a5` | 7e | DAC en DMA déclenché par le PIT, interruption du DMA, DMAMUX `A_ON`, DWT en tête |

### 14.2 Fichiers

| Fichier | Ce qu'on y trouve |
|---|---|
| `crates/oc-core/src/audio/mod.rs`, `wave.rs` | paramètres, voix, formes, table du sinus |
| `crates/oc-core/src/apps/tone.rs` | l'applet `TONE` |
| `crates/oc-core/src/profile.rs` | étapes `audio`, `audio_period`, `frame`, `Profile::merge` |
| `crates/oc-drivers/src/ssd130x.rs` | `FrameStream`, `page_commands`, `release` |
| `crates/oc-drivers/src/dac8565.rs` | `channel_word`, `ChangedCodes` |
| `crates/oc-drivers/src/fifo.rs` | octets → mots de FIFO |
| `crates/oc-firmware/src/audio.rs` | le chemin audio, sûr : boîtes aux lettres, interruption, écran |
| `crates/oc-firmware/src/audio_irq.rs` | l'`unsafe` : anneau, DMA, DMAMUX, gestionnaires, NVIC |
| `crates/oc-firmware/src/board.rs` | constantes de cadence, d'horloge, de tronçon ; `bus_clocks` |
| `journal/D-033-audio-path.md` | décision et ses six révisions |
| `TESTING.md` § 9.3d, 9.3e | protocoles de mesure sur le module |

### 14.3 Mesures, de bout en bout

| État | DAC | `audio` (moy.) | Écriture du DAC | Écran |
|---|---|---|---|---|
| avant | 8 MHz, bloquant, 1 kHz | — | ≈ 16 µs par tick | page 145 µs par tick |
| 7a | 22 MHz, 1 kHz | — | 3,0 µs par tick | page 49 µs |
| 7c (`69e9099`) | 22 MHz, interruption, 47,6 kHz | 12,5 µs | ±3 µs de gigue | trame 1,18 ms |
| 7e (`27211a5`) | 22 MHz, DMA, 47,6 kHz | 3,4 µs | instant fixé par le PIT | trame 1,34 ms |
