# Étape 4 — réglages système, calibration, persistance (avancement, en cours)

Plan : [etape-4-reglages-systeme.md](../etape-4-reglages-systeme.md). Ce fichier se remplit au fil des lots et des
mesures ; il sera complété à la fin de l'étape.

## Fait

* **4a** (`e73d873`) : conversion code ↔ mV dans `oc-core`, `Calibration::FACTORY` de Phazerville partagée par les
  trois backends (révision de D-027).
* **4b** : réglages (`oc_core::settings`), stockage `sequential-storage` sur `NorFlash`, simulateur `--settings`,
  ABI et JSON du patch Rack (révision de D-028). Reste à vérifier dans Rack : réglages restaurés à la réouverture
  d'un patch (le C++ n'a pas été compilé, faute de Rack SDK sur la machine de développement).

* **4c** : écran système (`SETUP`, gestes au démarrage, mode secours), réglages d'écran et d'encodeurs appliqués
  (révision de D-026). À vérifier sur le module : contraste visible, rotation et décalage, gestes au démarrage, sens
  des encodeurs ; le firmware ne sauvegarde pas encore (4e).

* **4d** : assistant de calibration (révision de D-027). À faire sur le module : calibrer les 4 sorties et les
  entrées (TESTING.md 9.4) ; la calibration ne survit pas encore à une coupure (4e).

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
