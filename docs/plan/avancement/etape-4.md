# Étape 4 — réglages système, calibration, persistance (avancement, terminée le 2026-11-16)

Plan : [etape-4-reglages-systeme.md](../etape-4-reglages-systeme.md). Décisions : D-026 (écran et accès), D-027
(calibration), D-028 (persistance), toutes adoptées puis révisées en cours d'implémentation ; D-024 (seuil de
présence) révisée par la mesure ; D-002, D-003, D-006 révisées.

## Fait

| Lot | Commit | Contenu |
|---|---|---|
| plan | `92ab189` | plan de l'étape, ADR D-026 à D-028, révisions de D-002, D-003, D-006 |
| 4a | `e73d873` | conversion code ↔ mV dans `oc-core` (traits en codes bruts), `Calibration::FACTORY` de Phazerville partagée par les trois backends |
| 4b | `3ac9e65` | `oc_core::settings` : `postcard` + `sequential-storage` sur `NorFlash` ; simulateur `run --settings` ; ABI Rack et JSON du patch |
| 4c | `1248b2f` | écran système `SETUP`, gestes au démarrage (encodeur gauche, `up` + `down` = secours), aperçu en direct ; rotation, décalage, contraste, sens des encodeurs |
| 4d | `1dc5261` | assistant de calibration aux étapes de Phazerville ; test de bout en bout par rebouclage |
| 4e | `fb6f3a1` | réglages dans la flash du Teensy par l'API ROM du RT1062 (seul `unsafe` du firmware) ; `xtask` refuse une image qui atteint `0x601F_0000` |
| retour module | `f7c56ba` | confirmation de sauvegarde à l'écran (`SETTINGS SAVED`) ; état des réglages répété sur chaque ligne de pulsation USB |
| 4f | `dc20316`, `7cf1cf1`, ce commit | firmware d'essai `settings-stress` (TESTING.md 9.5c) ; seuil de présence 50 mV ; version 0.3.0 |

## Corrigé en cours d'étape

* **Valeurs d'usine fausses** : zéro d'entrée à mi-échelle (2048) au lieu des 2/3 (2730), table de sortie sur 9 V au
  lieu de 10 V ; mesure du 2026-11-16 ci-dessous.
* **Assistant** (trouvés par les tests avant tout essai sur module) : traverser les étapes réécrivait des tables non
  réglées ; la moyenne ADC mélangeait deux étapes ; le premier bloc après un changement d'étape mêlait deux niveaux
  de sortie (816 codes au lieu de 819 entre 1 V et 3 V) : 32 ticks de stabilisation.
* **Retour du module** : aucune confirmation visible après `SAVE` ; lignes du journal USB émises avant qu'un terminal
  puisse s'y connecter.

## Reste ouvert

* Module VCV Rack : le C++ de `dataToJson` / `dataFromJson` n'a pas été compilé (pas de Rack SDK sur la machine de
  développement) ; restauration des réglages à la réouverture d'un patch non vérifiée.
* Affinage de la table DAC octave par octave (Phazerville hors « fresh ») : non fait, la table le permet.
* Revenir à Phazerville après une sauvegarde ici lui fait perdre une partie de son EEPROM émulée (D-028).

## Mesures sur le module TLM

### 2026-11-16 — sorties en mode ZERO, calibration d'usine (`e73d873`)

Multimètre sur chaque sortie, mode **ZERO** (0 V demandé partout). Cordons inversés à la mesure (COM sur la pointe du
jack, rouge sur le corps) : valeurs affichées positives, tension réelle de signe opposé.

| Sortie | Affiché | Tension réelle |
|---|---|---|
| OUT1 | 0,037 V | ≈ −37 mV |
| OUT2 | 0,033 V | ≈ −33 mV |
| OUT3 | 0,033 V | ≈ −33 mV |
| OUT4 | 0,044 V | ≈ −44 mV |

Lecture :

* la table d'usine de Phazerville (0 V au code 40 984) tombe à ≈ 40 mV du 0 V sur ce module ; l'ancienne table
  (0 V au code 43 690) aurait donné ≈ 0,4 V d'écart : le changement de 4a est confirmé ;
* l'écart restant est propre au module et de même ordre sur les quatre canaux (11 mV d'étalement) ; il relève de
  l'assistant (4d), pas d'une correction des valeurs d'usine.

### 2026-11-16 — après calibration (`f7c56ba`), entrées débranchées

Journal USB, une seconde (lignes `cvN`), CV1 rebouclée pendant l'assistant ignorée :

| Entrée | mV min..max | crête à crête | codes |
|---|---|---|---|
| CV2 | −7..7 | 14 mV | 2727..2733 |
| CV3 | −10..7 | 17 mV | 2731..2738 |
| CV4 | −7..7 | 14 mV | 2729..2735 |

Pas d'indicateur `~` au repos. Pire écart à 0 V : 10 mV ; pire crête à crête : 17 mV. Décision : seuil de présence
abaissé de 100 à 50 mV (révision de D-024). Calibration et réglage d'écran (rotation) relus après redémarrage.

### 2026-11-16 — flash sous contrainte (firmware `settings-stress`, `7cf1cf1`)

* 300 sauvegardes relues : `STRESS OK 300/300` ;
* plusieurs coupures d'alimentation pendant la boucle : aucun échec au redémarrage, réglages retrouvés ;
* firmware normal reflashé : réglages chargés.

