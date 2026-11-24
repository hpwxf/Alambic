# Étape 13 : plusieurs applets à la fois, ports routés vers les E/S physiques (D-046)

## Contexte

Une seule applet tourne et possède les quatre sorties : son `update` rend un `Params` pour les quatre
(`Generator::next -> [MilliVolts; 4]`, D-044), `MAILBOX` porte un seul `Setup`, `AppHost` garde un résident par
app (D-021). But : deux instances d'abord (jusqu'à `MAX_INSTANCES`), éventuellement de la même app, qui se
partagent les entrées et se répartissent les sorties sans recouvrement ; une seule à l'écran, `up` / `down` pour
passer de l'une à l'autre ou, par un appui long, ouvrir sa page de routage. Étape 3 de la trajectoire de D-043.

Chaque lot garde le comportement à une instance : écrans de référence et `tests/voice_samples.rs` identiques
jusqu'au lot qui change le panneau (13d), et celui-là seulement là où les gestes changent.

## Lots

### 13a — ADR et plan (ce commit)

* `journal/D-046-simultaneous-applets-and-routing.md` (proposée), index de `journal/README.md`, ligne de
  « Décisions actées » et étape en cours dans `PLAN.md`, ce plan.

### 13b — sorties en ports, une instance

* `App::output_ports() -> &'static [Port]` (nom, sortie par défaut ; par défaut `A` à `D`) ; `SUPERSAW` : `A`, `B`,
  `SUB` ; `RATCHET` : `A`, `B` ; les autres selon ce qu'elles écrivent.
* `Route` (voie locale → `Option<CvChannel>`). Le refus d'une sortie déjà tenue et l'app qui voit ses ports
  désactivés passent en 13c, avec les instances.
* `oc_core::audio` : la voix d'une instance et l'assemblage des voies vers les sorties physiques (nom à fixer :
  `Mix`, `Rack`…), une sortie libre à 0 V ; le maintien de la sonde (`held`) appliqué après le routage, hors de
  `Params<G>`.
* Backends : `MAILBOX` porte `[Setup; MAX_INSTANCES]`, les routes et un drapeau par emplacement ; `Player` (sim,
  Rack) de même. Une instance, route identité : échantillons identiques au bit près.
* Mesure : `.text`, `.rodata`, criterion `voice/*` et `engine/tick_*`.

### 13c — instances

* `AppHost` devient un pool de `MAX_INSTANCES` emplacements (enum des apps de l'image), un focus ; `AppId` reste
  l'espèce et la clé persistée. Changer d'app dans un emplacement y met une instance neuve.
* Toutes les instances mises à jour à chaque tick, le panneau à celle du focus, le rendu par elle seule.
* Carte d'entrées par instance (identité par défaut) appliquée à son `InputSnapshot`.
* Menu : remplacer l'app du focus, ajouter une instance à droite (sorties libres prises dans l'ordre), retirer
  celle du focus. Démarrage : une instance de `startup_app`.
* Tests dans `oc-core` : deux `TONE` sur des sorties distinctes, recouvrement refusé, instance retirée (sorties à
  0 V), deux fois la même app aux réglages indépendants.

### 13d — panneau : appui court, appui long, indicateur

* `buttons` : durée d'appui comptée en ticks ; court = instance précédente / suivante, long = page de routage,
  accord = menu (révision de D-021).
* Les actions des apps sur `up` / `down` déplacées (D-046, § 3) ; `UPDATE_SCREENS` là où le geste change, diff
  relu.
* Indicateur du moteur : 4 pixels verticaux à gauche du nom de l'applet, un par emplacement, caché avec une
  instance ; le nom de chaque app à une place commune (les apps qui le posent ailleurs s'alignent, écrans de
  référence régénérés et relus) ; marque du focus à essayer.

### 13e — page de routage

* Écran du moteur, commun : ports de sortie de l'instance au focus vers `A` à `D` ou `OFF` (sorties d'autres
  instances marquées et sautées), ports d'entrée vers `CV1` à `CV4` / `TR1` à `TR4`. Écran de référence nouveau,
  scénario `.scn`.

### 13f — sonde, protocole 9

* Les requêtes sur une app nomment une instance ; `ListInstances`, `AddInstance`, `RemoveInstance`, `Focus`,
  `SetRoute` ; `SelectApp` remplace l'app du focus. Client et CLI (`cargo probe instances`, `route`, `--instance`),
  page (un onglet par instance). Surveiller `stack-depth` (D-038, révision du 2026-11-22).

### 13g — module

* `MAX_INSTANCES = 2` (cas pratique), puis 4 (limites) : mémoire (`cargo xtask size`, `stack-depth`) et calcul.
* Deux `SUPERSAW`, `SUB` de l'une désactivée, puis `RATCHET` à côté de `SUPERSAW` : `cargo probe stats`
  (`voice`, `audio`, `update`, `tick`), écoute des deux voix ; TESTING, nouvelle section ; D-046 adoptée si les
  critères de son § 4 tiennent ; `etape-13.md`.

## Hors périmètre

* Sauvegarde du patch (instances, routes, réglages, assignations).
* Routes logiques d'une app à l'autre (contrôle, puis audio : D-042).
* `Setup` de taille variable ; l'arena (D-038), au premier grand tampon d'une app.

## Vérification

* Commandes de fin de lot d'`AGENTS.md` ; `.text` / `.rodata` relevés dans `etape-13.md`.
* `cargo test -p oc-sim --test scenarios` sans `UPDATE_SCREENS` après 13b et 13c.
