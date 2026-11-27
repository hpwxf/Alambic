# D-048 — Routage interne : les sorties CV d'une app comme entrée d'une assignation

| | |
|---|---|
| **Identifiant** | D-048 |
| **Date** | 2026-11-27 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | `48f3689` (ADR, plan `docs/plan/etape-15-routage-interne.md`) ; implémentation `1d868b9` ; révision du 2026-11-27 (assignation stricte, noms) : `0d40cb2` ; ports d'entrée internes : ce commit |
| **Relations** | étend l'`Assign` de D-035 / D-036 et son assignation par le trait (D-045) ; lit les ports des instances de D-046 ; les ports d'entrée de la carte d'entrées de D-046 lisent aussi un port interne (révision du 2026-11-27, ports d'entrée) ; millivolts de D-005 ; premier pas du « routage futur » annoncé par D-043 |

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

## Révision du 2026-11-27 : un port assigne, il ne module pas ; il se nomme par son nom

**Contexte.** À l'usage, l'utilisateur juge `+1a` (emplacement et lettre du rang du port) « vraiment pas explicite » :
la lettre ne dit rien du signal, et il faut la page de routage pour la traduire. Puis il sépare deux natures : une
entrée CV reste une modulation (`SCALE`, `SHIFT`, `MIN` / `MAX`, effet `SET` / `ADD` / `MUL`) ; un port d'une autre
instance est « strictement une assignation » : pas de `+` (un `=` à la place), pas de valeur du panneau affichée, et
le nom entier dans la place ainsi libérée. Des transformations viendront peut-être sur ces assignations, pas maintenant.

**Options.** Nom : trois lettres après l'emplacement (`+1:NOT`), à l'écran seulement ou partout, la valeur reculant
pour lui faire place ; le nom entier dès que la valeur s'efface. Assignation : l'effet `SET` forcé dans l'`Assign`
quand on choisit un port (les réglages de la prise perdus) ; ou les réglages gardés mais sans effet tant qu'un port
est choisi.

**Proposé et décidé.** Un premier temps a mis `+1:NOT` à l'écran, la valeur décalée, `1:NOTE` ailleurs. La séparation
demandée ensuite le remplace :

* **assignation stricte** : un port fixe le réglage à la valeur qu'il porte, comme `SET` la lit (pour `NOTE`, 0 V =
  C4, 1 V/octave : `SUPERSAW` joue la hauteur de `MELODY` elle-même), sans `SCALE`, `SHIFT`, bornes ni effet
  (`Assign::assigns`, `Assign::shaped`). Ces réglages restent dans l'`Assign`, inertes, pour une prise reprise ensuite ;
* **écran de l'app** : `=1:NOTE` (le `=` de `SET`, demandé par l'utilisateur), la valeur du panneau masquée comme
  sous `SET` ; huit caractères, cinq lettres du nom au plus (`SCREEN_PORT_LETTERS`), `=1:--` pour un port qui n'est
  plus ;
* **sous-menu** : la seule ligne `INPUT` (`1:NOTE`), `ASSIGNED` dessous, les barres du signal et du réglage ;
  l'encodeur gauche ne choisit plus d'autre ligne ;
* **sonde et page** : `1:NOTE` seul ; la sonde lit `input=1:note` (casse ignorée) ou `1:#1` par le rang, écrit `1:#1`
  quand elle ignore les noms, refuse les autres champs d'un port. Les noms viennent de `Reply::Instance` (`names`,
  toujours le protocole 11, pas encore publié) ;
* la lettre `a`–`d` disparaît partout, page de routage comprise.

**Ce qui ferait changer d'avis.** Le besoin de transformer une source interne (échelle, décalage, lissage, S&H, comme
le popup de modulations de *squares-and-circles*) : des opérations propres à l'assignation, à décider à part. Deux
ports d'une même app aux six premières lettres égales : une abréviation déclarée par le port.

## Révision du 2026-11-27 : les ports d'entrée lisent aussi un port interne

**Contexte.** Le lot suivant annoncé au § 4 : l'utilisateur demande qu'un port d'entrée d'une app, réglé dans la page
de routage, lise une sortie d'une autre, « par exemple une horloge `TR1` lue sur une sortie de `RATCHET` ».

**Options.** Où brancher : dans la carte d'entrées de l'instance (D-046), chaque voie d'entrée lisant une prise ou un
port interne ; ou une entrée « virtuelle » de plus par app. Un port trigger lisant un niveau : un seuil fixe, ou un
seuil réglable par port, ou le front montant du niveau seul.

**Proposé et décidé.** L'assistant a retenu, sur la demande, la carte d'entrées :

* `InputRoute` branche chaque voie sur une `InputSource` : une prise de son genre (`Jack`, 0 à 3) ou un port d'une
  autre instance qui porte un niveau (`Lane { slot, port }`), comme une assignation ; ni l'app elle-même ni un port
  audio, ce que `AppHost::set_input` refuse ;
* un port d'entrée CV prend le niveau tel quel ; un port trigger est haut tant que le niveau atteint
  `INTERNAL_TRIGGER_MV` (1 V : sous les 5 V des triggers et gates des applets, près du basculement des transistors
  d'entrée) ; le niveau est celui du tick précédent, comme pour les assignations, donc une horloge interne arrive
  avec 1 ms de retard ;
* page de routage : l'encodeur droit passe de `OFF` aux quatre prises (`TR1`–`TR4`, `CV1`–`CV4`) puis aux ports
  offerts (`1:THRU`, `1:BURST`…) et s'arrête aux bouts ; la ligne écrit `CLOCK <- 1:THRU` ;
* `OFF` (`InputSource::Off`), demandé par l'utilisateur à l'essai : un port qui ne lit rien, CV à 0 V, trigger bas ;
  un `RESET` dont le patch n'a pas besoin laisse ainsi toutes les prises `TR` aux autres apps sans rien bloquer ;
* protocole 11 (non publié) : `Request::SetInput` porte une `InputSource`, `Reply::Instance::inputs` aussi ; sonde
  `cargo probe route @1 clock=1:thru` (ou `1:#1`) ; la page propose les ports dans la liste de l'entrée.

**Ce qui ferait changer d'avis.** Une source dont le niveau bas reste au-dessus de 1 V (une hauteur prise pour
horloge) : un seuil par port. Une horloge rapide mal servie par le tick de retard : la valeur du tick en cours pour les
emplacements de gauche, comme au § 5.
