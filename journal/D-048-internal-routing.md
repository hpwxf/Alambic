# D-048 — Routage interne : les sorties CV d'une app comme entrée d'une assignation

| | |
|---|---|
| **Identifiant** | D-048 |
| **Date** | 2026-11-27 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ADR, plan `docs/plan/etape-15-routage-interne.md` et implémentation : ce commit et les suivants de l'étape 15 |
| **Relations** | étend l'`Assign` de D-035 / D-036 et son assignation par le trait (D-045) ; lit les ports des instances de D-046 ; les ports d'entrée de la page de routage restent ceux de D-046 (lot suivant) ; millivolts de D-005 ; premier pas du « routage futur » annoncé par D-043 |

## 1. Contexte

L'utilisateur demande (2026-11-27) un routage interne : par exemple jouer `SUPERSAW` avec les notes de `MELODY`.
Aujourd'hui une assignation (`modulation::Assign`) ne lit qu'une prise, `CV1` à `CV4` : il faut un câble de la
sortie A à `CV1`, donc passer par le DAC puis l'ADC (≈ 20 mV d'erreur à l'entrée, un demi-ton près de 83 mV), et
une sortie physique est prise. L'assignation doit se faire au même endroit que celle d'une CV : le sous-menu ouvert
par le clic droit sur un réglage. Toutes les sorties d'une app doivent pouvoir servir, même celles qu'aucune prise
ne reçoit.

## 2. Options envisagées

* **Ce qu'une source lit** : le niveau que l'app publie (`Channel::Level` de son lane, avant la calibration et les
  sorties forcées par la sonde) ; ou l'échantillon rendu par la voix, ce qui suppose un rendu dans le moteur, que
  D-040 a retiré, et n'a pas de sens à 1 kHz pour un port audio.
* **Quels ports** : tous, un port audio (`SUPERSAW`, `TONE`) lisant 0 V ; ou les seuls ports qui portent un
  niveau, chaque port déclarant son genre.
* **Quand** : la valeur du tick en cours pour les emplacements déjà mis à jour, celle du précédent pour les autres
  (dépend de l'ordre des emplacements) ; ou toujours celle du tick précédent (1 ms de retard, indépendant de l'ordre,
  aucune boucle à résoudre).
* **L'app elle-même** : proposée (rétroaction d'un tick) ou sautée.
* **Où vit le choix de la source** : dans l'`Assign` (une source parmi les prises et les ports des autres
  instances) ; ou dans la carte d'entrées de l'instance (D-046), la page de routage branchant une voie CV sur un
  port interne, l'assignation lisant cette voie : deux pages pour une seule idée.
* **Nom à l'écran** : dans 4 caractères à droite d'un réglage (`+CV1`), l'emplacement et le rang du port.

## 3. Ce que l'assistant a proposé

Une `Source` dans l'`Assign` : `Jack(CvChannel)` ou `Lane { slot, port }`. Les seuls ports de niveau, déclarés par
un `PortSignal` (`Cv` / `Audio`) ; la valeur du tick précédent pour tous ; l'app elle-même sautée ; le niveau
publié, avant calibration. Le bus des niveaux est tenu par `AppHost` et passé à chaque instance dans son
`InputSnapshot`, avec les ports de chaque emplacement, pour que le sous-menu sache quoi proposer. Les ports d'entrée
de la page de routage (une horloge lue sur une sortie d'app) dans un lot suivant.

## 4. Ce que nous avons décidé, et pourquoi

Les propositions (réponses de l'utilisateur, 2026-11-27) :

* **Ports CV seulement.** « Il faut imaginer que sinon on le ferait avec des câbles et qu'on serait soumis à
  l'ADC » : le routage interne remplace un câble de CV. Un port déclare son genre, `Cv` ou `Audio` ; seuls les
  premiers sont proposés. Le genre laisse la place à de futurs effets à entrée audio, hors de cette étape.
* **Jamais l'app elle-même** : le champ `INPUT` saute son propre emplacement.
* **Niveau publié, du tick précédent**, pour toutes les sources : déterministe, indépendant de l'ordre des
  emplacements, sans boucle. Un port audio, absent ou d'un emplacement vidé (`NOTHING`) lit 0 V ; l'assignation
  reste telle quelle. Une app calcule ses ports CV même non routés : `set_routed` ne sert qu'à épargner de l'audio.
* **Noms** : sur l'écran de l'app, l'effet, le numéro d'emplacement (1 à 4) et une **lettre minuscule pour le rang
  du port** (`+1a`), distincte des prises `A` à `D` ; la page de routage écrit cette lettre devant chaque port de
  sortie ; le sous-menu écrit le nom complet (`1:NOTE`). La sonde lit et écrit `1a`.
* **Ports d'entrée de la page de routage** : lot suivant.
* **Protocole 11** : l'`Assign` sérialisé porte la `Source` ; la description d'un port de sortie dit s'il porte un
  niveau.

Pourquoi : un seul endroit pour choisir ce qui module un réglage, celui qu'on connaît déjà ; aucune erreur de
conversion ni prise occupée ; tout reste dans `oc-core` (D-002), simulateur et Rack n'ont rien à faire.

## 5. Ce qui nous ferait changer d'avis

* Une milliseconde de retard audible ou gênante (une note qui part avant sa hauteur sur un générateur à enveloppe) :
  la valeur du tick en cours pour les emplacements de gauche.
* Le besoin d'un trigger interne (horloge de `MELODY` par `RATCHET`) : ports d'entrée internes, prévus au lot
  suivant.
* Un effet à entrée audio : router des ports `Audio` dans le chemin des voix, ce qui révisera D-044 et D-046.
* Le coût en ITCM ou en pile (`cargo xtask size`) du bus passé à chaque instance.
