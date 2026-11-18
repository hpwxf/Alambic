# Étape 7 — chemin audio (avancement, terminée le 2026-11-18)

Plan : [etape-7-chemin-audio.md](../etape-7-chemin-audio.md). Décision : D-033, adoptée puis révisée six fois ;
D-008, D-009, D-010 révisées. Récit détaillé de chaque essai, de ses échecs et de leurs corrections :
[rapport-etape-7-chemin-audio.md](../rapport-etape-7-chemin-audio.md). Valeurs mesurées sur un module : indicatives.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| plan | `336817a` | D-033 (48 kHz, quatre sorties, `TONE`), plan en cinq lots |
| 7a | `b293d6a` | `DAC_CS` en PCS0, OLED sur PCS3, bus à 22 MHz, voies écrites au changement |
| 7b | `7993063` | `oc_core::audio` (paramètres de sortie, `Voice`, formes, sinus calculé à la compilation), `TONE`, `oc_engine_audio` pour Rack |
| 7c | `21cdbb6`, `69e9099` | interruption audio propriétaire du bus, écran par tronçons (`FrameStream`), boîtes aux lettres ; puis six essais sur le module : repli PERCLK ÷ 21 = 47 619 Hz, double déclenchement, fin d'envoi lue dans l'écho |
| 7d | — | à l'accordeur : juste jusqu'à C6, perturbé autour de C7 (gigue ±3 µs) : décide le DMA |
| 7e | `27211a5` | PIT → DMAMUX (`ENBL \| TRIG \| A_ON`) → DMA → LPSPI4 : DAC écrit à l'instant du timer ; interruption de fin de copie pour l'échantillon et l'écran |
| clôture | ce commit | auto-test de 50 ms et rapport à 3 s dans le journal ; rapport détaillé, `docs/hardware.md` |

## Mesures

* **Interruption audio** (`TONE`, `cargo probe stats`) : 12,5 µs sur 21 au lot 7c (≈ 60 % du processeur), **3,4 µs**
  au lot 7e (6,1 au plus).
* **Écriture du DAC** : ≈ 16 µs par tick à 8 MHz avant l'étape ; 3,0 µs à 22 MHz (7a) ; ±3 µs de gigue par
  interruption (7c) ; instant fixé par le PIT (7e).
* **Écran** : une page 145 → 49 µs (7a) ; une trame en tronçons, 1,18 ms (7c), 1,34 ms (7e).
* **Justesse** (Korg NTS-2, sinus) : juste et stable jusqu'à B7 ; C8 en limite de l'accordeur (lu C7 ou C8) ;
  affichage qui bouge de ±1 cent plus souvent qu'avant le DMA, cause non établie.
* **Boucle** : toujours 1 000 µs de période.

## Reste ouvert

* 48 kHz exacts : l'entrée quartz de GPT2 n'a pas démarré, et seul le PIT (sur PERCLK) déclenche le DMA.
* Les quatre voies du DAC sont écrites à chaque période (≈ 5,5 µs de bus), changées ou non.
* Rack : `oc_engine_audio` compilé, pas encore écouté.
* L'ADC reste bloquant dans la boucle (≈ 290 µs par tick).
