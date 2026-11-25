# D-046 — Plusieurs applets à la fois : instances, ports routés vers les E/S physiques, focus au panneau

| | |
|---|---|
| **Identifiant** | D-046 |
| **Date** | 2026-11-24 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | `dd8d0fd` (ADR et plan), `faee549` (13b), `e99e4c6` (13c), `527f0bc`, `f161c84` (13d), `c0fdc86` (13e), `8586de7` (13f), `8911df1`, `73739a5` (13g), adoption avec `etape-13.md` |
| **Relations** | étape 3 de la trajectoire de D-043 ; révisera D-021 (un résident par app, `up` / `down` au relâchement sans minuterie), D-043 (`AppHost`, ce que nomme la sonde), D-044 (un `Params` pour les quatre sorties) et D-040 (un `Setup` par changement, désormais par instance) ; déplace le maintien par la sonde de D-029 après le routage ; prépare l'arena de D-038 ; rouvre D-042 si `audio` approche la moitié de la période ; la persistance du patch (D-028, D-045) vient plus tard |

## 1. Contexte

Une seule applet tourne. `AppHost` garde une instance de chaque app (D-021) et un curseur ; seule l'app en cours est
mise à jour, et son `update` rend un `Params` qui décrit **les quatre sorties** : un générateur rend
`[MilliVolts; 4]` d'un coup (`Generator::next`, D-044), et `MAILBOX` porte un seul `Setup` de la composition
(`apps::sound`). Le firmware ne nomme aucune app, mais tout le chemin de sortie suppose qu'une app les possède
toutes.

Les boutons `up` et `down` agissent au relâchement, sans minuterie, et servent aux apps : `RATCHET` déclenche une
rafale sur `up`, `DIAGNOSTIC` et `GFX DEMO` changent de mode sur `up` / `down`, le sous-menu d'assignation se ferme
sur `down` (`modulation/editor.rs`). L'accord `up` + `down` ouvre le menu.

Tailles mesurées sur l'hôte (`size_of`, 2026-11-24) : apps de 72 o (`GFX DEMO`) à 280 o (`RATCHET`), `SUPERSAW`
208 o, `AppHost` 1 088 o ; `sound::Setup` 220 o, `SoundState` 192 o, `Voice` 280 o.

Ce que l'utilisateur a fixé le 2026-11-24 :

1. **Plusieurs applets en même temps**, deux d'abord, peut-être plus, éventuellement **plusieurs fois la même**.
   Elles se partagent les entrées et **se répartissent les sorties, sans recouvrement**.
2. **Une page de routage par applet**, ouverte par `up` / `down`, la durée de l'appui la distinguant du changement
   d'applet. Elle route les E/S de l'applet depuis et vers les E/S physiques ; plus tard vers des E/S logiques,
   d'une applet à l'autre sans passer par le physique. Une sortie peut y être **désactivée** (la voix `SUB` de
   `SUPERSAW`), ce qui libère la place pour une autre applet.
3. **Une seule applet à l'écran.** `up` / `down` passent de l'une à l'autre, de gauche à droite. Un mini
   indicateur de quelques pixels, dans un coin en haut, montre la position de l'applet courante et le nombre total.
4. La **sauvegarde** de cet état viendra plus tard. L'arena est une piste pour les instances. Un `Setup` dynamique
   n'est pas urgent tant que la taille fixe tient : essayer d'abord avec deux instances.
5. La sonde suivra : réglages et assignations par instance.

## 2. Options envisagées

* **Instances** :
  * **A** — un résident par `AppId` (D-021) et une liste des actives : simple, mais pas deux fois la même app ;
  * **B** — un pool fixe de `MAX_INSTANCES` emplacements, chacun un enum des apps de l'image
    (`Instance::Ratchet(RatchetApp)`…), de la taille de la plus grande ;
  * **C** — des instances placées dans l'arena (D-038, I), en OCRAM, de taille quelconque.
* **Sorties** :
  * **A** — chaque app garde ses quatre sorties, un masque choisit ce qui passe ;
  * **B** — l'app déclare ses **ports de sortie** (nom, sortie physique par défaut) ; le générateur rend toujours
    `[MilliVolts; 4]`, lu comme des voies locales ; une **route** par instance envoie chaque voie vers une sortie
    physique ou nulle part ;
  * **C** — un générateur à `k` voies (paramètre constant ou tranche) : plus juste, plus invasif.
* **Voix** : une `Voice` et un `Setup` par instance, `MAILBOX` portant `[Setup; MAX_INSTANCES]` ; ou un `Setup` de
  taille variable (arena), plus tard.
* **Entrées** : chaque instance lit tout le physique, comme aujourd'hui ; ou une carte d'entrées par instance (port
  de l'app vers entrée physique), appliquée à son `InputSnapshot`.
* **Mise à jour** : l'instance au focus seule (D-021) ; toutes à chaque tick.
* **Gestes** :
  * **a** — appui court sur `up` / `down` : instance précédente / suivante ; appui long : page de routage ;
  * **b** — l'appui court reste à l'app, l'appui long change d'instance, le routage passe par le menu.

## 3. Ce que l'assistant a proposé

* **Instances : B.** Un pool fixe dans `oc-core`, `MAX_INSTANCES = 4` (pas plus d'instances que de sorties),
  essayé à deux d'abord. Avec les tailles mesurées, quatre emplacements tiennent en ≈ 1,2 Kio dans le moteur
  (`.bss`, DTCM) : **l'arena n'est pas nécessaire aux instances elles-mêmes**. Elle reste la bonne place, en OCRAM,
  pour leurs **grands tampons** (ligne à retard, enregistrement de CV), avec une part par instance, le jour où une
  app en aura un (D-038, révision alors). L'OCRAM convient : lue et écrite par le processeur seul, jamais par un
  DMA. `AppId` devient l'**espèce** (le catalogue de l'image, la clé persistée) ; l'instance est un emplacement.
  Changer l'app d'un emplacement y met une **instance neuve** : la résidence de D-021 tombe, la sauvegarde du patch
  la remplacera.
* **Sorties : B.** `App::output_ports() -> &'static [Port]`, l'indice d'un port étant sa voie dans le
  `[MilliVolts; 4]` du générateur. `Route` par instance : pour chaque voie, une sortie physique (`A` à `D`) ou
  aucune. **Sans recouvrement** : le moteur refuse (`Refused`) une sortie déjà tenue par une autre instance, et une
  sortie que personne ne tient reste à 0 V. L'app connaît ses ports désactivés (`SUPERSAW` ne prépare pas `SUB`),
  pour ne pas payer une voix que rien n'écoute. Le **maintien par la sonde** (`held`) sort de `Params<G>` et
  s'applique aux sorties physiques, après le routage : il garde son sens (« cette prise est à tel niveau »).
* **Voix** : une `Voice` par instance, chacune avec son `Setup` et son `Follower` ; `MAILBOX` porte les
  `[Setup; MAX_INSTANCES]`, les routes, et un drapeau par emplacement, pour ne copier que celui qui a changé.
  L'interruption appelle `next` pour chaque instance active, puis place les voies. Taille fixe (≈ 0,9 Kio de
  `Setup` à quatre) ; pas de `Setup` dynamique tant qu'elle tient. Estimation pour deux `SUPERSAW` : `voice`
  ≈ 3,1 µs, `audio` ≈ 5,5 µs sur 21, loin de la moitié de la période (D-042).
* **Entrées** : une carte par instance, l'identité par défaut, appliquée à l'`InputSnapshot` qu'elle reçoit. Les
  entrées se partagent librement, sans exclusivité. Elle vaut pour ce que l'app lit directement (`TR1` de
  `RATCHET`, les entrées de `SCOPE`) ; les assignations de CV (`Assign::input`, D-045) continuent de nommer
  l'entrée physique, qu'elles choisissent déjà.
* **Mise à jour : toutes les instances à chaque tick**, puisqu'elles jouent toutes ; seule l'instance au focus
  reçoit le panneau (les autres, `silence_controls`) et dessine. Coût : un `update` de `SUPERSAW` fait ≈ 15 µs, deux
  ≈ 30 µs sur les 1 000 d'un tick.
* **Gestes : a**, comme l'utilisateur l'a décrit. Appui court (sous un seuil de l'ordre de 400 ms) : instance
  précédente ou suivante, sans boucler ; appui long : page de routage de l'instance au focus ; l'accord ouvre le
  menu, comme avant. La distinction demande une durée : la règle « sans minuterie » de D-021 tombe, remplacée par
  un compte de ticks, déterministe sur les trois backends. Les apps perdent `up` / `down` : le déclenchement à la
  main de `RATCHET` passe au clic de l'encodeur gauche (et reste une action de la sonde), les modes de
  `DIAGNOSTIC` et `GFX DEMO` aux encodeurs, le sous-menu se ferme au clic droit, qu'il accepte déjà.
* **Indicateur** : dessiné par le moteur après le rendu de l'app, en haut à droite, une marque par instance et
  celle du focus pleine, sur un fond effacé de quelques pixels. **Caché avec une seule instance** : les écrans de
  référence ne changent pas tant qu'on n'en ajoute pas. (L'utilisateur a choisi une autre place, § 4.)
* **Menu** : il agit sur l'instance au focus ; choisir une app la remplace. Deux entrées de plus : ajouter une
  instance à droite du focus (ses ports prennent les sorties libres dans l'ordre, désactivés s'il n'y en a plus) et
  retirer l'instance au focus (s'il en reste une autre). `SETUP` reste.
* **Page de routage** : un écran du moteur, commun à toutes les apps, comme le menu. Il liste les ports de
  l'instance au focus : chaque sortie vers `A` à `D` ou `OFF` (les sorties prises par une autre instance sont
  marquées et sautées), chaque entrée vers `CV1` à `CV4` ou `TR1` à `TR4`. Les routes logiques d'une app à l'autre
  viendront dans une étape à elles, au rythme de contrôle d'abord (D-043, étape 3), audio ensuite (D-042).
* **Sonde : protocole 9.** Les requêtes sur une app (`DescribeApp`, `GetParam`, `SetParam`, `Observe`, `Act`,
  `GetAssign`, `SetAssign`) nomment une **instance** ; `ListApps` reste le catalogue ; nouvelles requêtes
  `ListInstances` (emplacement, app, focus, routes), `AddInstance`, `RemoveInstance`, `Focus`, `SetRoute` ;
  `SelectApp` remplace l'app de l'instance au focus. La CLI et la page suivent (un onglet par instance).
* **Démarrage** : une instance de `startup_app`, routes identité, comme aujourd'hui. La sauvegarde du patch
  (instances, routes, réglages, assignations) est hors de cette étape.

## 4. Ce que nous avons décidé, et pourquoi

**Adoptée le 2026-11-25** (section « Adoption » en fin de fichier). Le périmètre et l'interface sont ceux fixés
par l'utilisateur le 2026-11-24 (§ 1) ; le reste était la proposition, confirmée lot par lot.

Tranché par l'utilisateur le 2026-11-24, après la proposition :

* **appui court : changement d'instance ; appui long : page de routage**, le geste le moins fréquent ;
* **les actions des apps sur `up` / `down` sont déplacées** comme proposé (le déclenchement de `RATCHET` au clic de
  l'encodeur gauche) ; des nuances court / long pour la navigation dans une app pourront venir plus tard ;
* **une instance neuve au changement d'app**, pour l'instant ;
* **l'indicateur : 4 pixels verticaux à gauche du nom de l'applet**, un par emplacement.

* **le seuil de l'appui long : 1 s pour commencer**, à régler à l'usage ;
* **les 4 pixels** : un pixel allumé par instance, celui du focus marqué autrement (éteint au milieu de la
  colonne, par exemple), à essayer à l'écran ;
* **`MAX_INSTANCES` : 2 pour mesurer le cas pratique** (calcul et mémoire), **puis 4 pour chercher les limites**.

* **le nom de l'app à une place commune de l'écran**, que toutes les apps respectent : le moteur sait où poser
  l'indicateur sans le demander à chacune.

* **`MAX_INSTANCES` = 4** (2026-11-24, après les deux mesures du lot 13g) : le calcul tient (`audio` 6,4 µs au plus
  avec quatre `SUPERSAW`), et quatre sorties bornent de toute façon à quatre les instances qui jouent ; le prix est
  fixe (`.text` +2,5 Kio, `.data` +888 o, pile +1,4 Kio), la pile à surveiller ;
* **`up` / `down` tournent en boucle** (après la dernière, la première ; avant la première, la dernière) et
  **changent aussi d'instance depuis la page de routage**, qui s'ouvre alors sur celle de la nouvelle instance ;
* **l'indicateur large de `SCROLL_WIDTH`**, dans l'alignement de la barre de défilement des listes, **et dessiné
  aussi sur la page de routage**.

**Critères d'acceptation** (ce qui la fait passer à `adoptée`) :

* **une instance** : écrans de référence et échantillons de `tests/voice_samples.rs` identiques ; sur le module,
  `voice` et `audio` inchangés au bruit près ;
* **deux `SUPERSAW`** sur le module, `SUB` de l'une désactivée : `audio` sous la moitié de la période (≈ 10,5 µs),
  `tick` dans son budget, `stack-depth` sans alerte ; l'ITCM au-dessus de 16 Kio libres (D-038) ; les deux voix
  écoutées sur leurs sorties ;
* recouvrement refusé et sortie libre à 0 V, testés dans `oc-core` ;
* coût en `.text` et `.rodata` relevé par différence de `cargo xtask size`.

## 5. Ce qui nous ferait changer d'avis

* `audio` près de la moitié de la période avec deux instances : rendu par blocs (D-042), ou des voix plus légères.
* L'ITCM sous 16 Kio libres : D-038, B (bancs repartagés) avant de réduire le périmètre.
* Une app à l'état volumineux, qui gonflerait chaque emplacement du pool : son tampon dans l'arena (C pour cette part).
* L'appui long confus à l'usage, ou trop lent : gestes **b**.
* L'indicateur qui masque ce qu'une app dessine : une colonne réservée, que les apps évitent.
* Des assignations qui doivent suivre la carte d'entrées (une instance qu'on veut rebrancher d'un bloc) : `Assign`
  nomme alors une entrée de l'instance, plus une entrée physique.
* Un besoin de routage d'une app vers une autre avant la fin de l'étape : en faire une étape à part, pas un ajout
  à celle-ci.

## Lot 13b (2026-11-24) — les sorties en ports, une instance

Le statut reste `proposée`. Ce que l'implémentation a fixé :

* **`oc_core::audio`** : `Params<G>` ne porte plus que le son ; `Route` (voie → `Option<CvChannel>`, `IDENTITY`,
  `OFF`, `first(n)`) ; `Routed` (paramètres et route d'un emplacement) ; `Patch<G, N>` (un `Routed` par emplacement
  et les niveaux maintenus par la sonde, appliqués après le routage) ; `Voices<G, N>` (une `Voice` par emplacement,
  un emplacement dont la route n'atteint rien ne tourne pas, une sortie qu'aucune route n'atteint à 0 V) ;
  `Changes<G, N>` (le `Setup` des seuls emplacements dont les paramètres ont changé, toutes les routes, les niveaux
  maintenus), `merge` dans `MAILBOX`, `applied` après la copie par l'interruption.
* **Un seul `Follower` par patch**, pas un par instance : quand la révision avance, il compare les paramètres de
  chaque emplacement à ceux qu'il a préparés et ne refait que les `Setup` qui ont changé. Une révision unique
  (`Engine::params_revision`) reste ce que compare la boucle à chaque tick.
* **`App::output_ports`** : `A` à `D` par défaut, `A`, `B` pour `RATCHET`, `A`, `B`, `SUB` pour `SUPERSAW` ;
  `AppHost::route` donne la route de l'app en cours (`Route::first` du nombre de ports). `MAX_INSTANCES = 2`
  (`apps/mod.rs`), seul l'emplacement 0 sert. `Engine::params` rend les paramètres de l'app en cours,
  `Engine::patch` ce que jouent les backends.
* **Reportés en 13c**, avec les instances qui les rendent utiles : le refus d'une sortie déjà tenue et le fait que
  l'app sache ses ports désactivés.

Mesures :

* `tests/voice_samples.rs` passe désormais par le chemin entier (patch, route des ports, `Player`) : sommes de
  contrôle inchangées pour `TONE`, `SUPERSAW`, la rampe de `DIAGNOSTIC` et les triggers de `RATCHET` ; écrans de
  référence inchangés.
* Code (`cargo xtask size` contre `master`) : `.text` +3 848 o (31,9 Kio d'ITCM libres), `.rodata` +416 o, `.data`
  +440 o (`MAILBOX` porte un `Changes`) ; pile ≈ 11 800 o (inchangée).
* Hôte (criterion) : `engine/tick_*` inchangés (+0,4 %, bruit) ; `voice/supersaw_routed_sample_to_codes` 36,6 ns
  contre 27,7 ns pour la voix seule : l'aiguillage des voies coûte ≈ 9 ns par échantillon ;
  `two_supersaws_routed_sample_to_codes` 46,8 ns, le second banc ≈ 10 ns. À mesurer sur le module (13g).

## Lot 13c (2026-11-24) — les instances

* **`AppHost`** : un pool de `MAX_INSTANCES` emplacements, chacun une `Instance` (enum des six apps) et sa route ;
  un ordre de gauche à droite (la *position*) séparé de l'emplacement (le *slot*), qui ne bouge pas tant que
  l'instance tourne : sa voix garde son état quand une autre s'ajoute ou part à côté. `select` remplace l'app au
  focus par une neuve (idempotent pour la même), `add` en démarre une à droite du focus et lui donne le focus,
  `remove` retire celle du focus (jamais la dernière), `set_focus`, `set_route` (`Refused::Taken` pour une sortie
  déjà atteinte, nouvelle variante). Une nouvelle instance prend les sorties libres dans l'ordre ; avec une seule
  app, la route est celle de ses ports, comme en 13b.
* **Toutes les instances tournent à chaque tick**, la seule au focus reçoit le panneau et dessine ; `TickContext::cv_out`
  donne à l'app ses voies telles qu'elles ont atteint les prises, 0 V pour une voie routée nulle part.
* **`App::set_routed`** : l'app apprend quels ports atteignent une prise ; `SUPERSAW` ne prépare plus une partie
  routée nulle part.
* **La sonde, jusqu'au protocole 9** : une requête sur une app vise sa première instance ; une app qui ne tourne
  pas se décrit et se lit sur une instance neuve (ses valeurs de départ), mais ne se règle pas (`NoSuchApp`) ;
  `ListApps` la marque `running` si une instance tourne.
* **Le menu reste celui d'une app** : ajouter et retirer passent au lot 13d, avec le défilement que le menu n'a pas
  encore (il remplit l'écran).
* **Écart à D-021 assumé** : choisir une autre app la démarre neuve ; tests de résidence réécrits
  (`tests/menu.rs`, `apps/mod.rs`).

Mesures : `.text` +1 456 o contre 13b (au premier jet +8,9 Kio : `free_route` déroulé, 2,5 Kio, et `Instance::new`
recopié dans chaque appelant ; ramenés par une boucle à un passage et `#[inline(never)]` sur `Instance::new`, `app`,
`app_mut`), `.rodata` −356 o ; pile ≈ 11 552 o (−248 o). Écrans de référence inchangés ; `tests/instances.rs` :
`RATCHET` et `SCOPE` sur des prises distinctes, la rafale de `RATCHET` hors focus, une instance retirée (prises à
0 V), deux `RATCHET` aux réglages indépendants.

## Lot 13d, première partie (2026-11-24) — les gestes

* `ButtonReader` compte la durée d'un appui : `ButtonEvents::long_pressed` (une fois, au seuil, bouton encore
  enfoncé), `tapped` (relâché avant) ; `up` / `down` n'agissent au relâchement que pour un appui court.
  `LONG_PRESS_TICKS = 1 000` (1 s).
* Le moteur : appui court sur `up` → instance de gauche, `down` → de droite, sans boucler ; l'appui long est réservé
  à la page de routage (lot 13e). Le clic qui ferme le menu n'atteint pas l'app (`ButtonReader::swallow`).
* Les gestes des apps déplacés comme décidé (révision de D-021) ; tests et scénario `mode_cycle` repris ; écrans de
  référence inchangés ; README et TESTING à jour.
* Coût : `.text` +332 o contre 13c ; pile ≈ 11 584 o.

## Lot 13d, seconde partie (2026-11-24) — le menu et l'indicateur

* **Menu** : `ADD APP` (tant qu'un emplacement est libre) et `REMOVE APP` (tant que deux tournent) entre les apps et
  `SETUP` ; `ADD APP` affiche de nouveau la liste des apps, titrée `ADD APP`, et celle choisie démarre à droite du
  focus (`MenuOutcome::Add`) ; `REMOVE APP` arrête celle du focus. La liste défile (sept lignes visibles), comme
  `SETUP`.
* **Le titre à une place commune** : toutes les apps l'écrivent en `gfx::controls::TITLE_LEFT` (3 px) sur la ligne
  du haut, `GFX DEMO` par son `header` ; écrans de référence régénérés, seule la ligne du titre bouge (diff relu).
* **L'indicateur** (`gfx::controls::instance_marks`), dessiné par le moteur après l'app quand deux instances au
  moins tournent : un pixel par instance dans la colonne x = 0, un pixel sur deux en descendant (jusqu'à quatre),
  celui du focus large de deux pixels, sur un fond effacé ; caché avec une seule instance.
* Coût : `.text` −20 o contre la première partie ; pile ≈ 11 600 o.

## Lot 13e (2026-11-24) — la page de routage

* **Écran du moteur** (`oc_core::routing::RoutingPage`, `Screen::Routing`), ouvert et fermé par l'appui long de
  `up` ou `down`, fermé aussi par l'accord ; les apps tournent derrière, comme derrière le menu, et l'appui qui la
  ferme ne leur parvient pas. Titre `ROUTE <APP>` à la place commune, une ligne par port : les sorties, puis les
  entrées ; le gauche choisit, le droit tourne. Une sortie passe par `A` à `D` et `OFF` en sautant les prises tenues
  ailleurs ; une entrée par les quatre prises de son genre.
* **Carte d'entrées** (`InputRoute`, une par instance, identité par défaut) appliquée à l'`InputSnapshot` de
  l'instance ; `App::input_ports` déclare ce que l'app lit elle-même (`InputPort` : nom, genre, voie) :
  `RATCHET` `CLOCK` (`TR1`), `SCOPE` `IN` (`CV1`). **Les assignations de CV restent sur les prises physiques** :
  une app qui lit ses CV par des assignations (`TONE`, `SUPERSAW`, les CV de `RATCHET`) ne déclare pas de port CV,
  donc sa carte ne déplace pas ce qu'elles lisent (§ 5 : à revoir si une app doit rebrancher ses CV d'un bloc).
* Tests : `routing.rs` (lignes, sorties qui sautent les prises tenues, entrées), `tests/instances.rs` (appui long,
  `SUB` coupée et plus préparée ; deux `RATCHET`, la seconde cadencée par `TR2`) ; scénario et écran de référence
  `routing`.
* Coût : `.text` +2 712 o, `.rodata` +272 o ; pile ≈ 11 648 o.

## Lot 13f (2026-11-24) — la sonde, protocole 9

* **Une cible au lieu d'un indice d'app** : `Target::App(n)` (la première instance du genre `n`, une neuve pour
  décrire ou lire un genre qui ne tourne pas, jamais pour écrire) ou `Target::Instance(p)` (la position `p`), dans
  `DescribeApp`, `GetParam`, `SetParam`, `Observe`, `Act`, `GetAssign`, `SetAssign`. `ListApps` et `SelectApp`
  restent par genre (`SelectApp` remplace l'app au focus).
* **Nouvelles requêtes** : `ListInstances`, `AddInstance`, `RemoveInstance`, `Focus`, `SetRoute`, `SetInput` ;
  réponses `Reply::Instance` (position, app, focus, la prise de chaque port de sortie, celle de chaque port
  d'entrée) et `Reply::Port` à la fin d'une description (le nom de chaque port, le genre d'une entrée).
  `Refused::Taken` passe sur le fil.
* **CLI** : `@N` partout où une app est nommée ; `instances`, `add`, `remove`, `focus`, `route @N port=jack…` ;
  `app` imprime les ports. Lecture et écriture des prises dans `oc_probe::apps` (`parse_output`, `parse_jack`,
  `find_port`, `routing_text`), testées sur l'hôte.
* **Page** : `oc_web::Instances` et `AppView::of_instance` (testés contre le vrai serveur) ; un bouton par instance
  (la montrer sur le module et dans la page, ✕ pour l'arrêter), *Add beside*, la table de routage de l'instance
  affichée, des listes qui envoient à chaque changement. Pas encore essayée dans Chrome.
* Coût : `.text` +3 256 o (≈ 24,4 Kio d'ITCM libres), pile inchangée (≈ 11 648 o).

## Lot 13g, côté hôte (2026-11-24) — `MAX_INSTANCES` à 4

Mesuré sur `8586de7` en passant la constante de 2 à 4, puis revenu à 2 (les tests passent avec les deux) :

| | 2 | 4 |
|---|---|---|
| `.text` | 0x29e70 | 0x2a844 (+2 516 o) |
| `.data` (`MAILBOX` : un `Setup` par emplacement) | 0x2154 | 0x24cc (+888 o) |
| pile, chemin le plus profond (`main` + interruptions) | ≈ 11 648 o | ≈ 13 008 o (+1 360 o ; 3 376 o libres) |

La pile grandit surtout par les copies du `Patch` et des `Routed` dans la boucle (`AppHost::update`,
`Engine::update_params`) ; à 4, elle reste au-dessus du seuil d'alerte (2 Kio) mais s'en approche : à surveiller
avant d'aller plus loin. Criterion ne tranche pas : selon `N`, l'inlining déplace les chiffres dans les deux sens
(`voice/two_supersaws_routed_sample_to_codes` 46,8 ns à 2, 36,6 ns à 4), le coût réel est celui du module.

## Lot 13g, module (2026-11-24) — mesures, `MAX_INSTANCES` à 4, gestes ajustés

Relevés de l'utilisateur, `cargo probe stats`, `NOTE` sous le bruit de CV1 ; µs, moyenne (max) :

| | `SUPERSAW` seule (étape 11e) | `SUPERSAW` + `RATCHET`, N = 2 (`8911df1`) | 4 × `SUPERSAW`, N = 4 |
|---|---|---|---|
| `audio` | ≈ 4,2 (5,0) | 4,20 (5,07) | 4,49 (6,37) |
| `audio_period` max | — | 23,55 | 23,55 |
| `voice` | 1,0 à 1,6 | 0,86 | 2,14 |
| `voice_params` | 0,35 | 0,50 | 1,21 |
| `update` | 2,05 | 6,02 | 10,81 |
| `publish` | 4,98 | 14,32 | 35,70 (42,62) |
| `tick` | 15,0 (108) | 17,64 (111) | 25,42 (121) |

* `audio` reste sous le tiers de la période avec quatre instances ; `audio_period` ne bouge pas : la fusion des
  `Changes` dans la section critique de `publish` ne retarde pas l'interruption. La boucle passe ≈ 355 µs sur 1 000
  dans le pire cas (`sample` 291 µs compris).
* `publish` croît d'≈ 9 µs par `SUPERSAW` modulée, contre ≈ 5 µs pour la seule de l'étape 11 : à entrées fixes,
  aucune instance n'est repréparée sur l'hôte (vérifié par un test jetable sur les quatre configurations), l'écart
  vient donc du bruit de CV1, que chaque `SUPERSAW` suit, et probablement des copies de `Changes` (≈ 330 o par
  emplacement), qui grossissent avec N ; non instrumenté. Sans conséquence au budget actuel, suite possible.
* Mémoire à N = 4 (`cargo xtask size`) : `.text` 0x2aa44 (21,4 Kio d'ITCM libres), `.data` 0x24cc, pile ≈ 13 008 o
  (3 376 o libres, alerte sous 2 Kio).

Retenu : `MAX_INSTANCES = 4`. Ajustés à l'essai, à la demande de l'utilisateur :

* `up` / `down` courts tournent en boucle (`AppHost::step_focus`), sur l'écran de l'app comme sur la page de
  routage, qui reste ouverte et se rouvre sur la première ligne de la nouvelle instance ;
* l'indicateur prend la largeur de la barre de défilement (`SCROLL_WIDTH`, 3 px) : le focus sur toute la largeur,
  les autres d'un pixel au milieu, sur la piste pointillée de la barre ; il est dessiné aussi sur la page de
  routage. Le titre passe à `LIST_LEFT` (4 px), un pixel à droite de l'indicateur et aligné sur les lignes des
  listes ; écrans de référence régénérés, seule la ligne du titre bouge (diff relu).

## Adoption (2026-11-25)

Critères du § 4 :

* **une instance** : écrans de référence et sommes de `tests/voice_samples.rs` identiques après 13b et 13c ;
* **plusieurs `SUPERSAW`** sur le module : quatre à N = 4, `audio` 6,37 µs au plus (sous les 10,5 µs), `tick`
  25,4 µs en moyenne, `stack-depth` sans alerte (3 640 o libres), 21,7 Kio d'ITCM libres ; `SUPERSAW` + `RATCHET`
  à N = 2 de même ;
* recouvrement refusé (`Refused::Taken`) et sortie libre à 0 V : `tests/instances.rs`, `routing.rs` ;
* coût relevé lot par lot (sections ci-dessus) et pour l'étape (`etape-13.md`).

L'utilisateur clôt l'étape sur ses essais du module, « relativement fonctionnelle », à raffiner. L'écoute voix par
voix et les gestes de `73739a5` (boucle, focus depuis la page de routage, indicateur élargi) n'ont pas de relevé
détaillé : réserve de `etape-13.md`. La page (instances, routage) est essayée dans Chrome sur le module,
son ergonomie à affiner.

Suites notées, hors de cette décision :

* les copies de `Params` dans `Setup`, `Voice`, `bank::Setup` et `Bank`, restes du moteur qui rendait sa voix
  (D-040) : à retirer, avec une révision par emplacement à la place des `Params` gardés par `Follower`, et le
  champ `bank` de `SupersawParams` renommé ; révision de D-040 à venir ;
* `publish` à ≈ 9 µs par `SUPERSAW` modulée ; les `Changes` plus petits en découleraient.
