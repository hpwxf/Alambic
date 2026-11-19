# Étape 8 — premier oscillateur : Supersaw (avancement, terminée le 2026-11-18)

Plan : [etape-8-supersaw.md](../etape-8-supersaw.md). Décision : D-037, adoptée puis révisée trois fois (entrées
assignables, ×4 tranché sur mesure, scies descendantes) ; D-036 gagne sa deuxième utilisatrice ; D-038 (budget
mémoire) ouverte en chemin. Valeurs mesurées sur un module : indicatives.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| plan | `69e54f3` | D-037 (sept scies PolyBLEP en entier, A / B stéréo, C sub-octave, ×4 si le repliement s'entend), plan en quatre lots |
| 8a | `d1aaa57` | `oc_core::audio::supersaw` : scie PolyBLEP sans division par échantillon, désaccord et mix de Szabo, passe-haut sur la fondamentale, parts `Mono` / `Left` / `Right` / `Sub`, banc partagé dans `Voice` |
| 8b | `3502881` | `SupersawApp` (« SUPERSAW ») : `NOTE`, `DETUNE`, `MIX`, `LEVEL`, `STEREO` ; menu à six applets ; écran des sept voix |
| — | `0438059` | D-038 : budget mémoire et placement des sections, constat et options (sans code) |
| 8b′ | `3247851`, `28b7112` | entrées par `oc_core::modulation` (D-036) sauf `STEREO`, CV1 sur `NOTE` au démarrage ; la ligne ne montre que la valeur du panneau, comme `RATCHET` |
| 8c | `9c827c4` | profil `audio`, écoute, `STEREO`, entrées sur le module (TESTING 9.3f) |
| 8c′ | `5fc2522`, `af901c9`, `2b01a59` | exemple `aliasing` : repliement par FFT, écrêtage par niveau, écart à une supersaw idéale, WAV ; ×4 non nécessaire |
| 8c″ | `f42372c` | scies descendantes : les pointes du passe-haut vont vers la marge de +6 V ; `LEVEL` gardé à 3 V |
| 8d | — | ×4 : non fait, la mesure ne le demande pas |
| clôture | ce commit | cet avancement ; version 0.6.0 au commit suivant |

## Mesures

* **Interruption audio** (`cargo probe stats`, `SUPERSAW`) : `audio` **3,9 µs** en moyenne, 10,2 au plus, à A2 comme à
  C8 avec 100 % de désaccord (`TONE` : 3,4 et 6,1) ; le coût ne dépend pas de la note. `audio_period` 18,4 à
  23,5 µs, `frame` 1,37 à 1,39 ms, boucle à 1 000 µs.
* **Code** (différence de `cargo xtask size`, méthode de D-038) : `.text` +10,5 Kio pour 8a–8b, +3,5 Kio pour 8b′ ;
  ITCM ≈ 142 Kio sur 192.
* **Bruit des entrées débranchées** (`cargo probe stream`) : ≈ ±5 codes, de −22 à +12 mV ; ajouté sans seuil au
  désaccord, il faisait bouger les barres à 100 % ; plus rien une fois les entrées assignables (8b′).
* **Repliement** (exemple `aliasing`, à 1 V, de C2 à C8) : sous 10 kHz, ≤ −45 dB des harmoniques (≤ −57 dB jusqu'à
  C5), 30 à 35 dB sous les rampes naïves parties des mêmes phases ; raies repliées les plus fortes entre 20 et
  23,8 kHz (−22 dBc à C8).
* **Écart à l'idéal** : −15 à −33 dB, à 82–98 % sur les harmoniques mêmes : l'aigu que PolyBLEP atténue (−0,6 dB à
  6,5 kHz, −2,2 dB à 13 kHz, −5,3 dB vers 20 kHz).
* **Écrêtage** : niveau le plus haut sans écrêtage sur tous les cas, 1,00 V avec des scies montantes, **2,01 V**
  descendantes ; à 3 V (C4, mix 50 %), 2,9 % puis 0,05 % d'échantillons écrêtés.
* **Écoute** : son, `STEREO`, polarité inversée corrects sur le module ; le pire cas d'écrêtage d'avant, inversé,
  à 3 V (`C3-mix0-3V-ab.wav`) jugé très correct.

## Reste ouvert

* Le bruit de CV1 reste sur `NOTE` tant qu'elle y est assignée (≈ ±14 cents) : un filtrage des entrées CV, à
  décider par un ADR.
* Les assignations des entrées ne sont pas sauvegardées (comme celles de `RATCHET`).
* La bande de 10 à 20 kHz n'est pas chiffrée à part ; le DAC et l'étage analogique ne sont pas mesurés (carte son,
  ou la capture par la sonde de D-038, option F).
* Au-delà d'environ 2 V de `LEVEL`, de rares crêtes touchent la borne de +6 V.
* Rack : `SUPERSAW` compilé, pas écouté.
