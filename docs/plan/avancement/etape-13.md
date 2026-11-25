# Étape 13 — plusieurs applets à la fois, ports routés vers les E/S physiques (avancement, terminée le 2026-11-25)

Décision : D-046, proposée le 2026-11-24, adoptée le 2026-11-25 ; révise D-021 (instances, `up` / `down` au
panneau), D-043 (`AppHost`, ce que nomme la sonde) et D-044 (un `Params` par instance). Étape 3 de la trajectoire
de D-043. Plan : [etape-13-applets-simultanees.md](../etape-13-applets-simultanees.md). Branche
`simultaneous-applets` (PR #22).

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| 13a | `dd8d0fd` | D-046 proposée, plan de l'étape |
| 13b | `faee549` | `audio` : `Route`, `Routed`, `Patch`, `Changes` (un `Setup` par emplacement changé), `Voices`, `Follower` par patch, `Player` ; maintien de la sonde après le routage ; `App::output_ports` (`RATCHET` A, B ; `SUPERSAW` A, B, `SUB`) ; `MAILBOX` porte des `Changes` fusionnés |
| 13c | `e99e4c6` | `AppHost` en pool d'`Instance` (emplacement et position séparés), focus ; toutes les instances mises à jour, le panneau et le rendu à celle du focus ; `select` (instance neuve), `add`, `remove`, `set_route` (`Refused::Taken`) ; `App::set_routed` ; `tests/instances.rs` |
| 13d | `527f0bc`, `f161c84` | `up` / `down` : court = instance voisine, long (1 s) = page de routage ; tap et appui long des clics ; gestes des apps déplacés (`RATCHET` et `GFX DEMO` clic gauche, `DIAGNOSTIC` tap / long gauche, sous-menu CV au clic droit) ; clic qui ferme le menu avalé ; menu `ADD APP` / `REMOVE APP`, défilement ; titre à une place commune (`TITLE_LEFT`), indicateur des instances |
| 13e | `c0fdc86` | `RoutingPage` : sorties vers `A`–`D` / `OFF` en sautant les prises tenues, entrées propres à l'app (`App::input_ports`, carte d'entrées par instance) ; scénario `routing` |
| 13f | `8586de7` | protocole 9 : `Target` (`App` / `Instance`), `ListInstances`, `AddInstance`, `RemoveInstance`, `Focus`, `SetRoute`, `SetInput` ; CLI `@N`, `instances`, `add`, `remove`, `focus`, `route` ; page : un bouton par instance, *Add beside*, table de routage |
| 13g | `8911df1`, `73739a5` | mesures à N = 2 et 4 ; `MAX_INSTANCES = 4` ; `up` / `down` en boucle et depuis la page de routage ; indicateur large de `SCROLL_WIDTH`, aussi sur la page de routage ; titre en `LIST_LEFT` ; TESTING 9.1b |
| suite | `61a15f8` | état des interruptions audio découpé : `SharedState` (`Audio`, `Screen`, `Bus`, `Timing`) ; comportement inchangé |
| clôture | ce commit | D-046 adoptée ; cet avancement |

## Mesures

* **Une instance** : écrans de référence et sommes de `tests/voice_samples.rs` identiques après 13b et 13c ; les
  écrans ne bougent ensuite que sur la ligne du titre (13d, 13g), le menu ouvert et l'écran de routage nouveau.
* **Module** (`cargo probe stats`, `NOTE` sous le bruit de CV1 ; µs, moyenne (max)) :

  | | `SUPERSAW` seule (étape 11e) | `SUPERSAW` + `RATCHET`, N = 2 | 4 × `SUPERSAW`, N = 4 |
  |---|---|---|---|
  | `audio` | ≈ 4,2 (5,0) | 4,20 (5,07) | 4,49 (6,37) |
  | `voice` | 1,0 à 1,6 | 0,86 | 2,14 |
  | `voice_params` | 0,35 | 0,50 | 1,21 |
  | `update` | 2,05 | 6,02 | 10,81 |
  | `publish` | 4,98 | 14,32 | 35,70 (42,62) |
  | `tick` | 15,0 (108) | 17,64 (111) | 25,42 (121) |

  `audio_period` au plus 23,55 µs dans les deux cas : la fusion des `Changes` ne retarde pas l'interruption. La
  boucle passe ≈ 355 µs sur 1 000 dans le pire cas. `publish` ≈ 9 µs par `SUPERSAW` modulée (≈ 5 µs pour la seule
  de l'étape 11) : à entrées fixes, aucune instance n'est repréparée (vérifié sur l'hôte) ; l'écart viendrait des
  copies de `Changes`, non instrumenté.
* **Code** (`cargo xtask size`, par lot dans D-046) : 13b +3 848 o, 13c +1 456 o (+8,9 Kio au premier jet), 13d
  +312 o, 13e +2 712 o, 13f +3 256 o, `MAX_INSTANCES` 2 → 4 +2 516 o (`.data` +888 o), découpage de l'état audio
  −316 o. Fin d'étape : `.text` 0x2a908 (21,7 Kio d'ITCM libres sur 192, ≈ +14 Kio sur l'étape), `.data` 0x24cc.
* **Pile** (`stack-depth`) : ≈ 11 648 o à N = 2, ≈ 13 008 o à N = 4, ≈ 12 744 o en fin d'étape (3 640 o libres,
  alerte sous 2 Kio) ; elle grandit surtout par les copies du `Patch` et des `Routed` dans la boucle.
* **Criterion** ne tranche pas entre N = 2 et N = 4 : l'inlining déplace les chiffres dans les deux sens.

## Essais sur le module

* Profils ci-dessus, relevés par l'utilisateur (`SUPERSAW` + `RATCHET` sur `8911df1`, quatre `SUPERSAW` à N = 4).
* L'utilisateur juge l'étape « relativement fonctionnelle » sur le module, à raffiner ; les gestes ajustés au lot
  13g (boucle, focus depuis la page de routage, indicateur élargi) viennent de ses essais.
* Page web (TESTING 9.1c) essayée dans Chrome sur le module par l'utilisateur : instances et routage fonctionnent,
  ergonomie à affiner.

## Réserves

* Écoute voix par voix de plusieurs applets et gestes de `73739a5` : pas de relevé détaillé (TESTING 9.1b, panneau).
* Ergonomie de la page web (instances, routage) à affiner.
* Pile à 3,6 Kio de l'alerte avec quatre instances : à surveiller avant d'agrandir un emplacement.
* Copies redondantes de `Params` (`Setup`, `Voice`, `bank::Setup`, `Bank`) et `Params` gardés par `Follower` ;
  champ `bank` ambigu de `SupersawParams` : à reprendre (révision de D-040), ce qui réduirait aussi `Changes` et
  `publish`.
* Hors périmètre, à venir : sauvegarde du patch (instances, routes, réglages, assignations), routes logiques d'une
  app à l'autre, `Setup` de taille variable et arena.
