# D-033 — Chemin audio : DAC à 48 kHz en interruption, CS matériel, bus cédé à l'OLED par tronçons

| | |
|---|---|
| **Identifiant** | D-033 |
| **Date** | 2026-11-17 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR, plan) ; implémentation dans les lots de l'étape 7 (`docs/plan/etape-7-chemin-audio.md`) |
| **Relations** | met en œuvre l'option B de D-023 (pistes du 2026-11-17) ; révisera D-008 (une interruption à côté de la boucle, lot 7c), D-010 (CS matériel et horloge par périphérique, lot 7a ; bus possédé par l'interruption, lot 7c) et D-009 (envoi de l'écran par tronçons, lot 7c) ; garde D-005 (pas de flottant) et D-027 (conversion par la calibration) |

## 1. Contexte

L'utilisateur veut générer des sons sur les sorties du module (2026-11-17). Le chemin actuel écrit les quatre voies
du DAC8565 une fois par tick de la boucle coopérative à 1 kHz (D-008, D-023 option C+) : 4 mots de 24 bits sur LPSPI4
à 8 MHz, l'horloge que tolère l'OLED avec lequel le DAC partage le bus (D-010), CS en GPIO, ≈ 16 µs bloquantes. À
48 kHz la période est de 20,8 µs : ce chemin ne tient ni la cadence ni la régularité, et un envoi de page OLED
(≈ 145 µs) la casserait de toute façon.

Faits vérifiés pour ce choix :

* la broche 10 (`DAC_CS`) est `GPIO_B0_00`, **`LPSPI4_PCS0`** (`imxrt-iomuxc`) : le contrôleur peut encadrer lui-même
  chaque mot de 24 bits du DAC ; l'OLED (CS broche 8, D/C broche 6) reste en GPIO ;
* `imxrt-hal` 0.6 expose la FIFO de LPSPI (16 mots, `enqueue_data`, `enqueue_transaction`), `set_clock_hz` (bus
  désactivé), le DMA en émission, le PIT ; la BSP fournit `pit`, `gpt1`, `gpt2` et les canaux DMA ;
* le DAC8565 n'accepte **50 MHz qu'alimenté entre 3,6 et 5,5 V, 25 MHz entre 2,7 et 3,6 V** (fiche SBAS411C,
  note 3 des caractéristiques temporelles ; SYNC haut au moins 40 ns entre deux mots dans ce cas) ; sur le schéma
  O&C rev 2e (mxmxmx, `hardware/o_c_rev2e_schematic.pdf`), DVDD et AVDD sont sur `3V3_A` : plafond 25 MHz.
  LPSPI4 est cadencé à 132 MHz par `teensy4-bsp` (`LPSPI_FREQUENCY`) : la fréquence permise la plus proche est
  **22 MHz** (≈ 4,7 µs pour quatre voies) ;
* l'OLED fonctionne à 8 MHz sur ce module ; Phazerville demande 24 MHz au SH1106 sur LPSPI4 du Teensy 4
  (`SH1106_128x64_driver.cpp`) ;
* étage de sortie (schéma rev 2e) : OPA2172 inverseur, gain −4 (24k9 / 100k), 22 pF sur la contre-réaction (pôle
  ≈ 72 kHz), 220 Ω en série, liaison continue, −3,75 à +6,25 V autour de V_bias = 1,25 V : la bande audio passe ;
  diaphonie numérique du DAC 0,15 nV-s (SYNC haut) ;
* Squares-and-Circles sur O&C Teensy 4 : « DAC operation at 48kHz per channel » (wiki du projet) ; Phazerville sur
  Teensy 4 (`Main.cpp`, `OC_DAC.cpp`) : interruption à 16 666 Hz qui termine la page OLED précédente, écrit les quatre
  voies (`LPSPI_TCR_FRAMESZ(23) | LPSPI_TCR_PCS(0)`, mots dans la FIFO), puis lance la page suivante par FIFO et
  interruption LPSPI, sans DMA : l'architecture retenue ici, à une autre cadence.

Réserve : rien n'est publié sur la carte TLM T4 ; si elle diffère de la rev 2e (alimentation du DAC, étage de
sortie), le lot 7a le tranche par la mesure.

## 2. Options envisagées

* **Cadence** : 48 kHz ; 32 kHz ; 16,7 kHz (firmware de référence).
* **Sorties** : les quatre à la cadence audio ; A et B audio, C et D à 1 kHz.
* **Écriture du DAC** :
  * dans une interruption de timer, mots déposés dans la FIFO de LPSPI, CS matériel (PCS0) ;
  * DMA déclenché par timer (DMAMUX cadencé par le PIT) sur double tampon, l'interruption ne remplissant que le
    tampon ;
  * rester dans la boucle à 1 kHz (pas d'audio).
* **Partage du bus avec l'OLED** :
  * l'interruption audio possède le bus : chaque période, le DAC puis un tronçon d'OLED qui finit avant la période
    suivante (ce que fait le firmware de référence) ;
  * la boucle envoie l'OLED et l'interruption attend le bus (contention : un échantillon retardé d'une page) ;
  * OLED en DMA, intercalé entre deux échantillons.
* **Horloge** : une seule pour le bus (8 MHz) ; une par périphérique (DAC rapide, OLED lent), changée entre deux
  transactions (`set_clock_hz`, bus vide) ou par le préscaler de la commande de transaction (TCR).
* **Calcul audio** : un moteur unique appelé par les deux contextes ; ou un découpage contrôle / signal, la boucle
  passant des paramètres à une voix que l'interruption fait tourner.

## 3. Ce que l'assistant a proposé

48 kHz, les quatre sorties, une applet de test `TONE` comme premier livrable ; interruption de timer et FIFO avec CS
matériel d'abord, le DMA seulement si la mesure le demande ; l'interruption possède le bus et cède des tronçons à
l'OLED ; une horloge par périphérique ; un découpage contrôle / signal dans `oc-core`.

## 4. Ce que nous avons décidé, et pourquoi

Choix de l'utilisateur (2026-11-17) : **48 kHz**, **les quatre sorties** à la cadence audio, **applet `TONE`** comme
premier livrable. Le reste, proposé par l'assistant :

* **Cadence et contextes** : un timer PIT lève une interruption à 48 kHz, priorité NVIC la plus haute. La boucle à
  1 kHz reste le contexte de contrôle (panneau, CV, triggers, applets, dessin) ; l'interruption ne fait que le signal
  et le bus. Une seule interruption : l'USB reste servi par la boucle.
* **DAC** : `DAC_CS` passe en PCS0 matériel ; chaque voie est une transaction de 24 bits que le contrôleur encadre.
  Les quatre mots d'une période vont dans la FIFO sans attendre ; mise à jour par voie (le mode simultané reste à
  vérifier, D-010). Horloge du DAC : **22 MHz**, sous le plafond de 25 MHz du DAC alimenté en 3,3 V. Une voie n'est
  écrite que si son code change : une applet CV laisse le bus et le DAC aussi calmes qu'aujourd'hui.
* **Bus** : l'interruption possède LPSPI4. Chaque période : constater la fin du tronçon OLED précédent et relever
  son CS ; déposer les quatre mots du DAC ; calculer l'échantillon suivant pendant leur envoi ; puis, bus libre,
  baisser le CS de l'OLED, placer D/C et déposer un tronçon d'OLED à 8 MHz, dimensionné pour finir avant la
  période suivante. Un tronçon est soit des commandes, soit des données, jamais les deux. La boucle ne touche plus
  le bus : elle dessine, puis remet la trame à l'interruption (double tampon, échange sous section critique quand la
  précédente est partie). L'écran se rafraîchit plus lentement qu'aujourd'hui (une trame en ≈ 1 024 / 12 ≈ 85
  périodes d'une douzaine d'octets à 8 MHz : ≈ 2 ms, ≈ 0,6 ms à 22 MHz ; encore très au-dessus de 50 Hz).
* **Horloge** : d'abord **une seule, 22 MHz**, pour le DAC et l'OLED, comme Phazerville ; si l'OLED ne la tient
  pas, une horloge par périphérique (OLED à 8 MHz), changée entre deux transactions, bus vide. Les deux se
  comparent au lot 7a (feature de firmware), sans changer le code.
* **Transactions de l'OLED sur un PCS sans broche** : le contrôleur affirme le PCS courant à chaque transaction ;
  celles de l'OLED doivent donc en choisir un qu'aucune broche ne porte (PCS3), sinon le DAC recevrait les octets
  de l'écran. Le bus partagé choisit le PCS (et l'horloge) de chaque périphérique avant chacune de ses
  transactions.
* **Sauvegarde** : les appels à la ROM de la flash masquent les interruptions (`flash_store.rs`) et un effacement
  dure des dizaines de ms ; l'audio s'y interrompt. Les sorties passent à 0 V avant, plutôt que de rester figées
  sur un échantillon (lot 7c).
* **`oc-core`, contrôle et signal séparés** : une applet audio a deux parties. Sa partie de contrôle vit dans
  `Engine::tick` comme aujourd'hui et publie des paramètres une fois par tick ; sa **voix** calcule un échantillon
  par période à partir des derniers paramètres, sans allocation ni verrou, en entier (phase sur 32 bits, tables).
  Les échantillons sont en millivolts et passent par la calibration (D-027) en codes du DAC, comme le reste. Le
  firmware relie les deux contextes (section critique) ; le simulateur et Rack appellent la voix à leur propre
  cadence (Rack : à chaque `process()`). Une applet sans voix garde ses niveaux tenus, réécrits à chaque période.
* **Mesure** : le profil (D-029) compte la durée de l'interruption (max, moyenne), les dépassements de période et
  le débit de l'écran ; `cargo probe stats` les montre. Pas de broche d'oscilloscope dédiée pour commencer.
* **`TONE`** : formes sinus, triangle, scie naïve, carré sur A–D ; fréquence à l'encodeur et en V/oct sur CV1 ;
  amplitude réglable, centrée sur 0 V, au plus ±3 V (la sortie descend à −3 V). Elle sert à écouter les clics, à
  vérifier la justesse et le partage du bus, pas à jouer : pas d'anti-repliement (la scie et le carré naïfs
  replient, c'est attendu).
* **DMA** : différé. Il n'entre que si la mesure (lot 7d) montre des échantillons perdus ou retardés que la
  priorité et la FIFO ne suffisent pas à éviter.

## 5. Ce qui nous ferait changer d'avis

* Des clics ou une gigue audibles malgré la priorité et la FIFO : DAC en DMA déclenché par le PIT (DMAMUX), tampons
  hors cache ou maintenance du cache.
* Un DAC qui ne tiendrait pas 22 MHz sur la nappe d’un autre module : horloge plus basse, au plus 16 µs de bus par période en comptant
  l'OLED.
* Une interruption qui ne tient pas dans sa période avec une voix réelle (supersaw) : calcul par blocs (quelques
  échantillons d'avance, FIFO plus profonde ou DMA).
* Un écran visiblement dégradé : tronçons plus longs, ou trame partielle (D-009, pistes notées).
* Le besoin de lire les CV à la cadence audio : ADC en DMA (D-023, pistes de l'ADC).

## Révision du 2026-11-17 — lot 7a mesuré, forme de la voix (lot 7b)

**Lot 7a, sur le module** (firmware `b293d6a`, une seule horloge de 22 MHz) : écran propre sur toutes les applets ;
sorties à −3 V et +5 V justes au multimètre sur les quatre voies (−3,000 / +4,997 V sur A et D, −2,999 V sur B et C
à −3 V) ; étape `outputs` 3,0 µs en moyenne, 8,0 µs au plus (≈ 16 µs avant), une page d'OLED 49 µs (≈ 145 µs à
8 MHz), tick le plus long 130 µs. Une horloge unique est donc gardée par défaut ; `oled-spi-8mhz` reste le repli
d'un module dont la nappe ou l'écran ne tiendrait pas 22 MHz. Ce sont des observations sur un module, pas des
constantes du code.

**Forme de la voix**, tranchée par l'assistant pour le lot 7b :

* une applet ne rend plus des niveaux mais des **paramètres de sortie** (`oc_core::audio::Params`) : pour chaque
  voie, un niveau tenu ou une **tonalité** (forme, fréquence en millihertz, amplitude crête en millivolts, centrée
  sur 0 V). Les applets existantes rendent leurs niveaux tenus ; le moteur applique par-dessus l'écran des réglages,
  l'assistant de calibration et les niveaux tenus par la sonde, comme avant ;
* la **voix** (`oc_core::audio::Voice`) est l'objet qui tournera dans l'interruption : une cadence fixée à la
  construction, une phase de 32 bits par voie gardée quand les paramètres changent (pas de clic au changement de
  note), un échantillon en millivolts par appel, sans division ni allocation par échantillon ;
* le **moteur** garde sa propre voix, à la cadence du tick (1 kHz sur tous les backends), et en écrit
  l'échantillon comme aujourd'hui : rien ne change pour le firmware avant le lot 7c, ni pour les applets de CV et
  les écrans dorés. `Engine::params` donne les derniers paramètres à un backend qui fait tourner sa voix à sa
  cadence : Rack au lot 7b (`oc_engine_audio`, une voix à la fréquence d'échantillonnage de Rack, appelée à
  chaque `process()`), le firmware au lot 7c ;
* le **sinus** vient d'une table d'un quart de période (257 points, 15 bits) calculée à la compilation en
  entiers (série de Taylor en virgule fixe), interpolée linéairement : erreur sous le millivolt, vérifiée contre
  `f64` dans les tests ; triangle, scie et carré se déduisent de la phase ;
* **`TONE`** : A sinus, B triangle, C scie, D carré, à la même fréquence ; encodeur gauche, la note (C1 à C8,
  A4 = 440 Hz par défaut), encodeur droit, l'amplitude (0 à 3 V crête, défaut 1 V) ; CV1 en V/oct s'ajoute à la
  note (`fixed::exp2`). Le menu des applets perd la ligne vide sous son titre pour accueillir une cinquième
  applet ; au-delà, il faudra le faire défiler.
