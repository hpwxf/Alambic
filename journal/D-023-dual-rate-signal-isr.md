# D-023 — Double cadence : ISR signal (GPT2, 16 666 Hz) et boucle UI à 1 kHz — ou audio 48 kHz + DMA

| | |
|---|---|
| **Identifiant** | D-023 |
| **Date** | 2026-11-16 (consigne un plan du 2026-08-02 et une étude du 2026-08-23 / 2026-09-03) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (option C+), révisée |
| **Commits** | aucun pour l'implémentation ; étude : `83be36f`, `c8b3039` (`docs/supersaw/`) ; plan A : [`docs/plan/etape-3-plan-a-isr-16khz.md`](../docs/plan/etape-3-plan-a-isr-16khz.md) (suivi par git depuis le lot 3a ; auparavant `.junie/plans/isr-16khz-dual-rate.md`, exclu par `.git/info/exclude`) |

## 1. Contexte

D-008 a fait tourner tout le firmware dans une boucle coopérative à 1 kHz. Deux documents veulent en sortir, pour
deux raisons différentes :

* le plan `docs/plan/etape-3-plan-a-isr-16khz.md` (assistant Junie, 2026-08-02, longtemps hors git) vise la **cadence de contrôle** du firmware
  de référence : chemin signal en ISR à 16 666 Hz (60 µs : ADC → applet → DAC), UI dans la boucle principale à
  ≈ 1 kHz (panneau, rendu OLED ≈ 50 Hz, USB) ;
* l'étude `docs/supersaw/supersaw-sur-ornament-and-crime.md` (§ 12–13) vise l'**audio** : sortie à 48 kHz,
  suréchantillonnage interne ×4, DAC en **DMA double tampon**, ISR audio en priorité NVIC maximale, CV décimées
  (÷ 24 → 2 kHz) depuis l'ISR audio, trame OLED découpée.

Contrainte commune : le DAC et l'OLED partagent le bus SPI (D-010) ; une trame OLED complète coûte ≈ 1,05 ms, une
page ≈ 131 µs — plus que la période de 60 µs.

## 2. Options envisagées

**A** — *Plan ISR 16 666 Hz* : `Engine::tick` scindé en `signal_tick` / `ui_tick` (`tick()` = composition, pour le
simulateur et Rack) ; GPT2 en ISR périodique, GPT1 horloge libre ; paramètres UI → signal par atomiques ; OLED
poussé depuis la boucle UI **en masquant l'IRQ GPT2** pendant la transaction ; sans RTIC. Coût assumé par le plan :
≈ 16 échéances d'ISR manquées par trame OLED, ≈ 50 salves par seconde, compteur `overdue` exposé.

**B** — *Architecture audio de l'étude* : ISR à 48 kHz, DAC alimenté par DMA déclenché par timer sur double tampon
(l'ISR remplit un tampon et peut être retardée d'une demi-période sans perte), trame OLED fragmentée entre deux
périodes, priorités NVIC, tampons DMA hors D-cache.

**C** — Garder D-008 et ne monter la cadence que pour un applet qui le demande (aucun aujourd'hui).

## 3. Ce que l'assistant a proposé

**A**, dans le plan local (« Key Decisions » 1 à 6 : 16 666 Hz exacts, GPT2, pas de RTIC, scission dans `oc-core`,
OLED sous masque d'IRQ, simulateur et Rack inchangés). L'étude supersaw, rédigée ensuite, recommande **B** pour tout
chemin audio et écrit explicitement que masquer ou baisser le rafraîchissement n'est « pas une architecture ».

## 4. Ce que nous avons décidé, et pourquoi

Rien encore. Reste à trancher, dans cet ordre :

1. **L'objectif** du prochain applet : contrôle (CV, enveloppes, LFO, triggers fins) ou audio (supersaw). A répond au
   premier, B au second ; A n'est pas une étape vers B si l'OLED reste sous masque d'IRQ.
2. **Le partage du bus** : mesurer d'abord (protocole GPIO + oscilloscope de l'étude, § 13, étape 1) ce que coûte
   réellement une trame et ce que fait Squares-and-Circles sur le même bus.
3. **La forme dans `oc-core`** : `signal_tick` / `ui_tick` est utile à A comme à B ; c'est la partie à adopter en
   premier, testable sur l'hôte, sans engager le firmware.

## 5. Ce qui nous ferait changer d'avis

* Si la mesure montrait qu'une trame OLED fragmentée par pages tient entre deux échantillons à 16 666 Hz, A sans
  masque d'IRQ deviendrait acceptable.
* Si aucun applet audio n'était prévu à moyen terme, C (rester à 1 kHz) serait le plus économique, à condition de
  corriger la capture des triggers (D-019) indépendamment.

## Points ouverts (revue du 2026-11-16)

* **Traité au lot 3a (2026-11-16)** : le plan A est suivi par git sous `docs/plan/etape-3-plan-a-isr-16khz.md`,
  avec un bandeau qui liste ce que les révisions postérieures rendent caduc (anti-rebond des triggers, `patched`)
  et le conflit avec l'étude audio. Constat d'origine : le plan vivait dans un fichier exclu de git.
* A et B ne sont pas compatibles en l'état ; adopter A tel qu'écrit, c'est accepter des trous de ≈ 1 ms toutes les
  20 ms dans le chemin signal, ce que l'étude supersaw qualifie d'inacceptable pour l'audio.
  **Proposition de l'assistant (2026-11-16, lot 3a), à trancher au lot 3c** : quelle que soit l'option, aucune
  transaction OLED ne masque le chemin signal plus d'une période ; la trame est poussée **page par page** (≈ 131 µs,
  voire demi-page) entre deux échéances, comme le fait le firmware de référence (« Flush OLED DMA → DAC → start
  OLED DMA » dans son ISR). C'est la seule variante de A qui reste une étape vers B. L'essai Squares-and-Circles
  (`docs/supersaw/` § 13 : audio sans tic sur le même bus) indique que ce découpage tient sur ce matériel ; la mesure
  du lot 3b (coût réel d'une page et d'une trame, GPIO + oscilloscope) doit le confirmer avant l'adoption.
* Les effets de bord de A sur D-010 (`RefCell` à deux contextes), D-018 (panique en ISR), D-019 (anti-rebond des
  triggers à 60 µs ≈ 180 µs) et D-020 (splash rendu à cadence signal) sont listés dans le plan local ; ils devront
  être révisés dans les entrées concernées le jour où A ou B est adoptée.

## Révision (2026-11-17, lot 3c) — option C+ adoptée sur les mesures du lot 3b

Mesures de la sonde (D-029) sur le module TLM, indicatives (un module) : une page OLED ≈ 145 µs, une trame
≈ 1,16 ms, le dessin de l'écran diagnostic ≈ 0,63 ms, l'échantillonnage bloquant des quatre CV ≈ 0,29 ms ; un tick
de rendu dure ≈ 2,1 ms, d'où ≈ 947 ticks par seconde ; au Pamela's Pro Workout, 39 % des triggers de 0,94 ms
perdus, 3,9 % à 1,56 ms, 0,7 % à 2,5 ms, aucun à 5 ms et plus. Le protocole GPIO + oscilloscope prévu au § 4.2
n'a pas été nécessaire.

Ce que ces chiffres changent aux options :

* A et B demandent d'abord un **échantillonnage non bloquant** (≈ 72 µs par voie en conversion bloquante, plus
  qu'une période de 60 µs) et un **envoi OLED en DMA ou en morceaux** sous la période ; ce sont deux chantiers à
  part entière, pas des détails de A ;
* aucun applet audio n'est prévu avant l'étape « chemin audio » de la feuille de route ; le prochain (quantizer,
  enveloppes, LFO) vit à 1 kHz ;
* le défaut que l'utilisateur voit aujourd'hui (triggers courts perdus, trou de 2 ms) se corrige à 1 kHz.

**Décision (utilisateur, 2026-11-17) : C+**, c'est-à-dire C (rester à 1 kHz, D-008) complétée de ce que le § 5
demandait déjà (« corriger la capture des triggers indépendamment ») et de la proposition du lot 3a (aucune
transaction OLED ne masque le chemin signal plus d'une période) :

1. **capture des fronts des triggers** par le détecteur de front des GPIO, lu et effacé à chaque tick, sans
   interruption (révision de D-019) ;
2. **écran page par page** : dessin dans un tick, une page envoyée par tick suivant ; l'écran de démarrage garde la
   trame entière (révision de D-009, D-010) ;
3. **`signal_tick` / `ui_tick`** dans `oc-core` (§ 4.3), `tick()` restant la composition ;
4. le plan A reste consigné tel quel (`docs/plan/etape-3-plan-a-isr-16khz.md`), non adopté.

Plan : [`docs/plan/etape-3c-cadence.md`](../docs/plan/etape-3c-cadence.md) (lots 3c1 à 3c4).

Ce qui ferait passer à A ou B (remplace le § 5) : un applet qui demande une cadence de contrôle au-delà de 1 kHz
(enveloppes de l'ordre de la milliseconde, triggers datés à mieux qu'un tick) ⇒ A, précédée de l'échantillonnage
non bloquant ; un chemin audio ⇒ B. Dans les deux cas, C+ en est la première marche : capture des
fronts, envoi par pages et scission du tick restent.

**Vérifié sur le module TLM (2026-11-17, lots 3c1 et 3c2)**, valeurs indicatives : triggers de 0,94 ms comptés
960 / 960 (588 avant) ; 1 000 ticks par seconde (≈ 947 avant), toutes les périodes à moins de 4 µs de la cible sur
les écrans Diagnostic, Scope et SETUP ; pire tick de dessin ≈ 0,96 ms échantillonnage compris (Diagnostic). Détail :
`docs/plan/etape-3c-cadence.md`.

**Lot 3c3 reporté (utilisateur, 2026-11-17)** : la scission `signal_tick` / `ui_tick` (point 3 de la décision C+,
§ 4.3) n'est pas faite. C+ tourne sans elle, et sa bonne forme dépend de ce que A partagera entre l'ISR et la boucle
(atomiques, paramètres, état des applets), qui n'est pas conçu. Elle devient le premier lot de A, après
l'échantillonnage non bloquant.

Note (2026-11-17, clôture de l'étape 3) : la feuille de route a été renumérotée, l'étape 4 ayant servi aux réglages
système ; « premier applet musical » devient l'étape 5, « chemin audio » l'étape 6.

## Pistes pour aller au-delà de C+ (2026-11-17) — ADC en attente, DAC prioritaire

Après l'étape 5, l'échantillonnage bloquant est le premier poste d'un tick (≈ 288 µs, ≈ 29 %) ; l'utilisateur met
l'ADC **en attente** : le Teensy 4.0 en limite les ambitions, et c'est le **DAC** qui compte pour générer du son.

**ADC** (mesuré et lu, non engagé). `teensy4-bsp` crée l'ADC1 sur l'horloge ADACK divisée par 8 ; avec la vitesse
« Medium » et 4 moyennes matérielles de `cv_in.rs`, une voie coûte ≈ 72 µs. Trois voies, de la moins à la plus
coûteuse :

1. horloge plus rapide (ADACK sans diviseur ou IPG), lecture encore bloquante mais de l'ordre de 20 à 40 µs pour
   les quatre voies ; à vérifier : bruit (`cargo probe inputs`), justesse (`sweep`), recalibration ;
2. conversions lancées par la boucle et terminées pendant l'attente de fin de tick (où l'USB est déjà servi) :
   les 288 µs sortent du tick, au prix d'un tick de latence sur les CV ;
3. chaîne matérielle timer → ADC_ETC → DMA : conversions sans le processeur, préalable de l'option A ; pas de
   pilote ADC_ETC dans `imxrt-hal`, registres et `unsafe` confinés à écrire.

**DAC pour l'audio** (option B). À 48 kHz la période est de 20,8 µs ; écrire les quatre voies prend ≈ 16 µs
mesurées (12 octets à 8 MHz plus les CS), parce que le bus partagé tourne à la vitesse de l'OLED alors que le
DAC8565 accepte 50 MHz. Pistes, dans l'ordre où elles se cumulent :

1. **horloge SPI par périphérique** : le DAC à 30–50 MHz (≈ 2–3 µs pour quatre voies), l'OLED à 8 MHz ; révise
   D-010 (une seule horloge pour le bus) ;
2. **DAC en DMA double tampon déclenché par timer** : l'audio ne dépend plus de la ponctualité du code ;
3. priorité d'interruption de l'audio au plus haut, tampons DMA hors cache (ou maintenance de cache) ;
4. l'envoi OLED intercalé entre deux écritures du DAC (page, demi-page ou DMA), D-009.

Étude de fond : `docs/supersaw/supersaw-sur-ornament-and-crime.md` § 12–14. Index matériel (non versionné) :
`HARDWARE.md`.
