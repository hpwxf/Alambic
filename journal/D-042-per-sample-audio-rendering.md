# D-042 — Rendu audio échantillon par échantillon, déclenché par le DMA, plutôt que par blocs

| | |
|---|---|
| **Identifiant** | D-042 |
| **Date** | 2026-11-20 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | aucun code : formalise ce que font `7993063` (la voix, lot 7b), `27211a5` (le DAC en DMA, lot 7e) et `d8b0970` (D-040) |
| **Relations** | précise D-033 (qui listait le double tampon parmi les écritures du DAC et le calcul par blocs dans son § 5, sans trancher la granularité du calcul pour elle-même) ; s'appuie sur D-040 (la voix préparée dans la boucle) ; mesurée par D-029 (étapes `audio`, `voice`, `voice_params`) |

## 1. Contexte

Comment le calcul des échantillons est-il couplé au DMA qui écrit le DAC ? Le calcul peut remplir à son rythme des
tampons complets (double tampon) que le DMA vide, ou être déclenché par le DMA lui-même, au fil de ce qu'il consomme.
Aucun ADR ne le dit comme un choix : D-033 a tranché la cadence, le DMA et le partage du bus, et ce couplage en est
sorti sans être discuté. Cette entrée pose la situation, les options et ce qui ferait changer, après coup.

Ce qui tourne (`crates/oc-firmware/src/audio.rs`, `audio_irq.rs`) :

* le canal 0 du PIT (47 619 Hz) déclenche à chaque période une copie DMA d'un **emplacement** d'un anneau de 8
  (8 mots de 32 bits : quatre fois `TCR` puis le mot d'une voie ; 256 octets en DTCM, parcourus par le modulo
  d'adresse source) vers LPSPI4. L'instant d'écriture du DAC ne dépend d'aucun logiciel ;
* la fin de cette copie lève l'interruption du DMA, **une par période**. Elle compte les rafales par leur écho (ce
  qui dit où en est le DMA), prend le `Setup` laissé par la boucle (D-040), calcule **un** échantillon des quatre
  voies (`Voice::next_sample`) pendant que la rafale sort, l'écrit **deux emplacements plus loin** (latence de 2 à
  3 périodes, ≈ 60 µs), attend l'écho de la rafale, puis lance un tronçon d'écran si la période le permet ;
  l'interruption de fin de trame de LPSPI4 relève ensuite le CS de l'OLED ;
* la boucle à 1 kHz ne produit aucun échantillon : elle publie des paramètres (`Params`, puis `Setup`) ;
* `oc-core` n'offre qu'un rendu par échantillon : `Voice::next_sample`. Chaque backend l'appelle à sa cadence : le
  moteur à 1 kHz, Rack une fois par `process()` (`oc_engine_audio`), le module dans l'interruption.

Relevés du 2026-11-19 sur le module (PLAN.md, lot 9b ; Supersaw, sept scies et la sous-octave) : `voice` 1,1 à
1,2 µs par échantillon, `voice_params` 0,30 µs, `audio` 4,0 µs en moyenne et 5,0 µs au plus, sur 21 µs. `audio`
est dominé par l'attente de la rafale (≈ 5,5 µs de bus) : un calcul plus court que la rafale s'y cache.

## 2. Options envisagées

* **A — un échantillon par période, dans l'interruption de fin de copie du DMA** (ce qui tourne). Un anneau court,
  deux emplacements d'avance.
* **B — par blocs** : un anneau de 2 × N emplacements, interruption du DMA à la moitié et à la fin de l'anneau, N
  échantillons calculés d'un coup dans la moitié libre. Le DAC reste écrit à chaque période par le DMA (le DAC8565
  n'a pas de FIFO : une rafale par période, blocs ou pas).
* **C — une API par blocs dans `oc-core` seulement** (`Voice::render(&mut [Frame])`), l'ordonnancement du firmware
  inchangé ; orthogonale à A et B.

Ce qui les distingue :

| | A | B |
|---|---|---|
| calcul de la voix | appel, dispatch (`Bank`, forme) et chargement de l'état à chaque échantillon | état en registres, dispatch hors de la boucle, instructions DSP du M7 exploitables ; gain non mesuré, estimé à 1,5 à 2 fois sur `voice` |
| entrée / sortie d'interruption | à chaque période | une fois par bloc ; quelques dizaines de cycles par période gagnés, négligeable |
| écran | le tronçon d'OLED part dans la même interruption, après la rafale | il faut un autre moyen d'intercaler l'OLED à chaque période (octets et D/C par DMA, ou une interruption par période qui demeure) : c'est l'essentiel du travail |
| latence | 2 à 3 périodes (≈ 60 µs) | ≈ 2 N périodes : 0,7 ms pour N = 16, 2 ms pour N = 48 |
| niveaux tenus, triggers (`RATCHET`) | un changement prend effet à la période suivante | à la frontière d'un bloc, sauf à horodater les changements dans le `Setup` |
| portabilité | `next_sample` sert tous les backends ; un hôte par blocs boucle dessus | idem côté firmware ; l'API par blocs reste celle de C |

## 3. Ce que l'assistant a proposé

À l'époque (D-033) : l'interruption et la FIFO d'abord, le DMA seulement si la mesure le demandait, le calcul par blocs
en recours si l'interruption ne tenait pas sa période avec une vraie voix (§ 5). Au lot 7e, le DMA est venu pour fixer
l'instant d'écriture du DAC, pas pour calculer par blocs : l'anneau a juste la profondeur qu'il faut pour deux
emplacements d'avance. Le 2026-11-20, relu sur les mesures : **garder A**, faire **C** quand un second consommateur par
blocs apparaîtra, envisager **B** seulement si `voice` approche la moitié de la période, et avant cela s'attaquer à
l'attente active de l'écho de la rafale, qui pèse plus lourd qu'un gain sur le calcul.

## 4. Ce que nous avons décidé, et pourquoi

**A**, constaté et gardé. Raisons :

* il faut de toute façon du processeur à chaque période pour intercaler l'OLED (D/C en GPIO, CS, tronçon dimensionné
  à la place qui reste) : l'interruption par période existe pour l'écran, et y calculer l'échantillon ne coûte rien
  de plus tant que le calcul tient dans la rafale qu'elle attend ;
* le budget est large : 1,2 µs de voix sur 21, environ 16 µs libres par période ;
* la latence est d'environ 60 µs, et un changement de niveau ou de trigger sort à la période près ;
* `oc-core` reste simple : une voix sans tampon, appelée par chaque backend à sa propre cadence. Une API par blocs
  s'en déduit sans rien changer.

**C** n'est pas fait : aucun backend ne rend par blocs aujourd'hui (Rack appelle une fois par échantillon).

## 5. Ce qui nous ferait changer d'avis

* **`voice` au-delà d'environ 10 µs** (la moitié de la période), ou `audio` au plus près de la période : polyphonie,
  filtres, réverbération. Alors B, en petits blocs (8 à 16 échantillons, latence sous la milliseconde), l'écriture
  du DAC toujours cadencée par le matériel.
* **Un traitement qui veut des blocs** par nature (convolution, FFT, suréchantillonnage par blocs, SIMD sur plusieurs
  échantillons) : B pour ce traitement.
* **Un backend hôte par blocs** (plugin CLAP ou VST, cpal, JACK) : C, d'abord une boucle sur `next_sample`, puis une
  version spécialisée si le profil le justifie.
* **L'OLED sorti du processeur à chaque période** (octets et D/C par DMA, ou l'écran sur un autre bus) : l'argument
  principal de A tombe, B devient bien moins cher.
* **L'attente de l'écho de la rafale devenue le goulot** : d'abord une interruption sur la fin du transfert (piste
  notée au lot 7c de D-033), pas les blocs.
