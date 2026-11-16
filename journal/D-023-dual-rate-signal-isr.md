# D-023 — Double cadence : ISR signal (GPT2, 16 666 Hz) et boucle UI à 1 kHz — ou audio 48 kHz + DMA

| | |
|---|---|
| **Identifiant** | D-023 |
| **Date** | 2026-11-16 (consigne un plan du 2026-08-02 et une étude du 2026-08-23 / 2026-09-03) |
| **Auteur** | Pascal Havé |
| **Statut** | proposée |
| **Commits** | aucun pour l'implémentation ; étude : `83be36f`, `c8b3039` (`docs/supersaw/`) ; plan : `.junie/plans/isr-16khz-dual-rate.md` (**non suivi par git**, exclu par `.git/info/exclude`) |

## 1. Contexte

D-008 a fait tourner tout le firmware dans une boucle coopérative à 1 kHz. Deux documents veulent en sortir, pour
deux raisons différentes :

* le plan local `isr-16khz-dual-rate.md` (assistant Junie, 2026-08-02) vise la **cadence de contrôle** du firmware
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

* Le plan A vit dans un fichier **exclu de git** : il doit être suivi (déplacé sous `docs/plan/`) avant de servir
  de base à une décision.
* A et B ne sont pas compatibles en l'état ; adopter A tel qu'écrit, c'est accepter des trous de ≈ 1 ms toutes les
  20 ms dans le chemin signal, ce que l'étude supersaw qualifie d'inacceptable pour l'audio.
* Les effets de bord de A sur D-010 (`RefCell` à deux contextes), D-018 (panique en ISR), D-019 (anti-rebond des
  triggers à 60 µs ≈ 180 µs) et D-020 (splash rendu à cadence signal) sont listés dans le plan local ; ils devront
  être révisés dans les entrées concernées le jour où A ou B est adoptée.
