# Étape 3 — assainissement, sonde live, cadence (avancement, terminée le 2026-11-17)

Ouverte par la revue du 2026-11-16 ([revue-decisions-2026-11-16.md](../revue-decisions-2026-11-16.md)) ;
interrompue par l'étape 4, reprise ensuite. Plans : [etape-3b-sonde.md](../etape-3b-sonde.md) (lot 3b) et
[etape-3c-cadence.md](../etape-3c-cadence.md) (lot 3c), qui gardent aussi le relevé détaillé des mesures.
Décisions : D-029 (sonde live) adoptée puis révisée ; **D-023 adoptée en option C+** (1 kHz sans trous) ; D-013,
D-019, D-007, D-011, D-015, D-016 révisées par 3a ; D-019, D-009, D-010 révisées par 3c.

**Toutes les valeurs ci-dessous viennent d'un seul module (TLM, Teensy 4.0) et d'un seul hôte macOS** : elles
situent des ordres de grandeur pour trancher D-023, elles ne deviennent aucune constante du code. Ce qui en
dépend à l'exécution (marge d'attente sans `poll` USB) est mesuré par le firmware lui-même.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| 3a | `f862a9e` → `bc42378` | dette de revue R1–R6, V5 : CI verte avec `--all-features`, triggers sans anti-rebond, règle du brochage testée, docs de vérification corrigées, `flash --dry-run` en CI, plus de présence de câble, plan ISR suivi par git ; version 0.2.0 |
| 3b1 | `1a7d0c6` | `oc_core::profile` : étapes nommées, min / max / moyenne, histogramme de gigue ; `Clock::fine_count` ; `CYCCNT` sur le module ; deux lignes de profil par seconde sur le journal USB |
| cadence | `94133c5`, `28749a1`, `6f475d4` | trois corrections de la boucle trouvées par le profil (voir plus bas) |
| 3b2 | `5528472` | `oc-protocol` : messages, `postcard` + COBS, `FrameReader`, version |
| 3b3 | `18a6cb4` → `22fb58a` | sortie forcée et écran dans le cœur, serveur de la sonde testé sur `mock_engine` ; deux ports CDC (`usb-device`), journal `log` maison, envoi OLED page par page chronométré, redémarrage par 134 bauds (`reboot.rs`) |
| 3b4 | `07422fb` → `d5365bb` | `oc-probe` : `stats`, `stream`, `out`, `screen`, `reboot` ; client testé contre le serveur réel ; capture d'écran dans l'orientation de la façade |
| 3b5 | `aea46b5` → `6f51553` | `OUT*` sur le diagnostic ; `inputs`, `triggers`, `sweep` ; échantillons datés par l'horloge du module (protocole v2) ; condition de passage à un seul port (D-029 § 5) ; niveau 9 (CV, triggers, rebouclage) ; D-029 adoptée |
| 3b5, suite | `7389a65` → `2a713a2` | redémarrage par 134 bauds documenté ; largeur des impulsions dans `cargo probe triggers` ; série de largeurs au Pamela's Pro Workout |
| 3c | `c2c6121` | D-023 adoptée en option C+, plan du lot |
| 3c1 | `c771353` → `16fb45d` | fronts des triggers mémorisés par le détecteur de front des GPIO, lus à chaque tick sans interruption (`EdgeCapture`) ; même règle dans Rack ; vérifié au Pamela |
| 3c2 | `f4299a6` → `c21a765` | `Display::page_count` / `present_page` : dessin dans un tick, une page par tick suivant ; `PagedPanel` ; vérifié sur trois écrans |
| 3c3 | — | `signal_tick` / `ui_tick` **reporté à l'option A** (décision de l'utilisateur, 2026-11-17) : C+ n'en a pas besoin, sa forme dépend de l'état partagé de A |
| 3c4 | ce commit | clôture de l'étape |

## Corrigé en cours d'étape

* **La boucle tournait à ≈ 775 Hz depuis l'étape 1** : l'échéance partait du début du tick, après
  l'échantillonnage (≈ 0,29 ms) ; puis un retard fixe de l'attente s'ajoutait à chaque période (≈ 997,5 Hz).
  Calendrier fixe : échéance précédente + période, recalée sans dette après un dépassement.
* **Marge sans `poll` USB tirée d'une mesure** (20 µs en constante) : remplacée, sur remarque de l'utilisateur, par
  le plus long `poll` observé depuis le démarrage.
* **Aucun port USB** avec deux fonctions CDC : descripteur de configuration de 141 octets pour un tampon de
  contrôle de 128 ; feature `control-buffer-256`.
* **Outil hôte bloqué** pendant un flux (une réponse attendue avalait les échantillons) : trouvé par les tests sur
  l'hôte, avant tout essai sur module.
* **Durées comptées en ticks** : fausses d'≈ 5 % puisque les ticks de rendu durent ≈ 2,1 ms ; datées désormais par
  l'horloge du module.
* **Diagnostic à `0.0`** pendant qu'une sortie était tenue par la sonde ; ligne de profil tronquée ; arguments de la
  ligne de pulsation USB inversés depuis 4e ; capture d'écran pivotée.

## Mesures sur le module TLM (indicatives)

### Coûts de la boucle (`cargo probe stats`, `d5365bb`)

| Étape | Moyenne | Lecture |
|---|---|---|
| `sample` (4 conversions ADC bloquantes, moyennes) | ≈ 288 µs | ≈ 29 % d'une période de 1 ms |
| `outputs` (DAC8565, 4 voies) | ≈ 15 µs | |
| `render` (dessin de l'écran diagnostic) | ≈ 630–640 µs | CPU seul, un tick sur 20 |
| `present` (trame SH1106) | ≈ 1 157 µs | huit pages à ≈ 145 µs |
| `usb` (deux ports, sonde) | ≈ 2 µs (max ≈ 7) | critère de D-029 § 5 tenu |
| `tick` | ≈ 112 µs (max ≈ 1,8 ms) | le max est un tick de rendu |

Gigue hors rendu : sous ≈ 4 à 8 µs. Ticks de rendu (≈ 5 %) : dépassent leur période d'≈ 1,1 ms ; **≈ 947 ticks
par seconde au lieu de 1 000**. Conséquence observée : un trigger est daté à ≈ 2 ms près (intervalles d'un carré
régulier dispersés de 132,7 à 134,8 ms). Conséquence mesurée (Pamela's Pro Workout,
32 Hz) : triggers de 0,94 ms perdus à 39 %, de 1,56 ms à 3,9 %, de 2,5 ms à 0,7 %, de 5 ms et plus à 0 % ; la
prévision de ≈ 5 % pour 1 ms était fausse.

### Après le lot 3c (`c771353`, `f4299a6`)

| | Avant | Après |
|---|---|---|
| ticks par seconde | ≈ 947 | **1 000** (29 999 en 29 999 ms) |
| période min – max | ≈ 997,6 – 2 110 µs | **997,5 – 1 002,4 µs** |
| triggers de 0,94 ms comptés | 588 / 960 | **960 / 960** |
| dispersion des intervalles d'une horloge | ≈ −1,1 / +2,0 ms | **± 1 tick** |
| pire tick de dessin, échantillonnage compris | ≈ 2,1 ms | **≈ 0,96 ms** (Diagnostic ; Scope ≈ 0,46, SETUP ≈ 0,71) |

Compte exact au Pamela jusqu'à 192 Hz ; sous 0,625 ms de largeur réglée, plus aucun front, à toute cadence (source
ou étage d'entrée, non tranché ; voir le plan 3c).

### Niveau 9

* **9.2 entrées** : débranchées, moyenne −1 à 0 mV, crête à crête 20 à 24 mV sur 20 s (sous le seuil de 50 mV de
  D-024) ; plage de lecture ≈ −3,33 à +6,67 V, fixée par l'étage analogique (une sinusoïde de ± 4,8 V est écrêtée
  en bas) ; `P`, `~` et ordre des canaux conformes sur les quatre entrées ; pas de diaphonie visible.
* **9.3 triggers** : carré, LFO lent (un front par cycle), repos (zéro front) conformes ; compte exact au Pamela
  (960 sur 960) pour 5, 10 et 15 ms ; pertes sous ≈ 3 ms avant le lot 3c, **aucune après** jusqu'à 0,68 ms.
* **9.3b rebouclage et multimètre** : sortie A juste à ≤ 3 mV de −3 à +6 V ; l'erreur du rebouclage (jusqu'à
  −66 mV à +6 V sur A → CV1) vient de l'entrée : gain unique mesuré sur CV1 entre 1 et 3 V, et marches d'≈ 20 mV
  communes aux quatre voies.

## Reste ouvert

* ~~Marge du tick de dessin (≈ 40 µs sur le Diagnostic)~~ : réglé après la clôture (`53ee310`, D-009) : texte posé
  par colonnes, `render` 645,5 → 87,8 µs sur le module, pire tick ≈ 0,46 ms échantillonnage compris.
* **Option A ou B de D-023** : échantillonnage non bloquant, envoi OLED en DMA, puis `signal_tick` / `ui_tick`
  (3c3 reporté), le jour où un applet en a besoin.
* **Triggers de moins de 0,625 ms** : rien n'est vu ; à départager à l'oscilloscope ou avec une seconde source.
* **Précision des entrées** (≈ 20 mV, ≈ 24 cents) : gain par entrée ou table par morceaux ; à reprendre avec la
  première applet qui suit une hauteur.
* Non vérifié sur module : mise à jour simultanée des quatre voies du DAC8565 ; d'où vient le raccourcissement
  apparent des impulsions (source, étage d'entrée ou écarts entre lectures).
* `cargo xtask flash` ne demande pas lui-même le redémarrage (`cargo probe reboot` avant).
