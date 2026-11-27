# Étape 15 — routage interne : les sorties CV d'une app comme entrée d'assignation (avancement, en cours)

Décision : D-048, adoptée le 2026-11-27 (choix tranchés avec l'utilisateur). Plan :
[etape-15-routage-interne.md](../etape-15-routage-interne.md).

## Fait

| Lot | Contenu |
|---|---|
| 15a | D-048, plan de l'étape, `PLAN.md` |
| 15b | `apps::PortSignal` (`Cv` / `Audio`) et `Port::cv` / `Port::audio` (`SUPERSAW`, `TONE` en audio) ; `modulation::Source` (`Jack`, `Lane { slot, port }`), `Assign::input: Option<Source>` ; `apps/bus.rs` : `Bus` (niveaux du tick précédent, ports de chaque emplacement, lecteur), dans `InputSnapshot` ; `AppHost::levels` rempli par `update` depuis les `Channel::Level` des ports `Cv`, remis à 0 par `select_at` ; `AppHost::bus`, `position_of` ; `Modulations::read(input)` résout chaque source (`raw`) ; écrans de référence inchangés |
| 15c | sous-menu : `INPUT` parcourt `--`, `CV1`–`CV4`, puis les ports offerts emplacement par emplacement, l'app elle-même et l'audio sautés ; écrit `1:NOTE` (nom tronqué à 5 lettres), `SCALE` poussé à droite d'une entrée longue ; écran de l'app `+1a` ; lettre `a`–`d` devant chaque sortie de la page de routage (`routing.screen` régénéré) ; `tests/internal_routing.rs` (3 tests) ; scénario `internal_routing` (MELODY au 1, SUPERSAW au 2 par le menu, `NOTE` ← `1:NOTE`, quatre horloges) et son écran |
| 15d | protocole 11 : `Assign` avec `Source`, `Reply::Port::level`, `Reply::Instance::levels` ; `SetAssign` refuse (`NoSuch`) une source non offerte ; sonde : `input=1a`…`4d` (casse ignorée), `outputs` et routage avec la lettre, `--center` refusé pour un port (l'hôte n'échantillonne que les prises) ; page : liste `input` complétée par `Instances::sources`, lettre dans le routage ; tests (`oc-probe`, `oc-protocol`) |
| 15e | README, `docs/agents/code-map.md`, guide de `MELODY`, TESTING 9.3j ; mesures ci-dessous. **Reste** : essai sur le module (9.3j) et la page dans Chrome |
| 15f | révision de D-048 : un port assigne strictement (`Assign::assigns`, `Assign::shaped` : la valeur comme `SET` la lit, sans `SCALE`, `SHIFT`, bornes ni effet, gardés inertes) ; écran `=1:NOTE` sans valeur (cinq lettres au plus, `=1:--` pour un port disparu), `Modulations` gardant le nom de chaque source ; sous-menu réduit à `INPUT` et `ASSIGNED` ; lettre retirée (page de routage, sonde, page web : `routing.screen` revient à l'original) ; protocole 11 : `Reply::Instance::names` ; sonde : `oc_probe::apps::PortNames`, `input=1:note` ou `1:#1`, `1:NOTE` seul affiché, autres champs d'un port refusés ; page : `AppView::take_ports`, liste `1:NOTE`… ; scénario `internal_routing_closed` ; un premier temps à `+1:NOT` (trois lettres, valeur décalée) remplacé dans la journée |
| 15g | révision de D-048 : `apps::InputSource` (`Jack`, `Lane`) dans `InputRoute`, appliquée après le bus de l'instance ; un port CV prend le niveau, un trigger est haut dès `INTERNAL_TRIGGER_MV` (1 V) ; `AppHost::set_input` refuse l'app elle-même et l'audio ; page de routage : prises puis ports offerts (`CLOCK <- 1:THRU`) ; protocole 11 : `SetInput::source`, `Instance::inputs` ; sonde : `parse_input_source`, `route @1 clock=1:thru`, `routing_text` avec les noms ; page : codes 0–3 prises, 4 + 4 × emplacement + port, `lane_codes`, `source_label` ; `Simulator::apps` ; tests (`internal_routing` : MELODY sur `THRU` de RATCHET, puis plus d'horloge quand RATCHET part ; refus du protocole ; analyse de la sonde) ; scénario `internal_clock` |
| 15h | `InputSource::Off` (demande de l'utilisateur : un `RESET` qui ne bloque aucune prise) : CV à 0 V, trigger bas ; premier choix de la page de routage, avant `TR1` ; sonde `off` / `none`, `OFF` affiché ; page : code 255, `Instances::off_code` ; test : `RESET` sur `OFF`, `TR2` ne réarme plus |
| 15i | `RATCHET` (demande de l'utilisateur) : les voyants `1 A B` du titre retirés ; l'état de l'horloge en rond (plein haut, vide bas) à droite du tempo, sans nom de prise, l'horloge pouvant venir d'ailleurs ; écrans `ratchet`, `ratchet_tempo` régénérés |
| 15j | emplacements numérotés de 1 à 4 partout (demande de l'utilisateur), comme `1:NOTE` : page web (`1 MELODY`, `Run in slot 2`) et sonde (`@1`–`@4`, `apps::instance_position`, `apps::slot_label` ; `@0` refusé) ; `scripts/profile-supersaw.sh` suivi ; le protocole compte toujours depuis 0 |

## Mesures

`cargo xtask size`, contre `master` (`cca9e53`) compilé dans un worktree :

| | `master` | étape 15 | écart |
|---|---|---|---|
| `.text` (ITCM) | 0x21b84 (57,1 Kio libres) | 0x23460 (50,9 Kio libres ; 15e 0x22954, 15f 0x22cb4, 15g 0x233ec) | +6 364 o (15e +3 536, 15f +864, 15g +1 848, 15h et 15i +116) |
| `.bss` | 0x6208 | 0x6208 | 0 en apparence (alignement) |
| pile estimée (main + 3 interruptions) | 10 056 o (6 328 o libres) | 10 720 o (5 664 o libres ; 15e 10 400, 15f 10 648) | +664 o (15e +344, 15f +248, 15g +72) |
| chemin le plus profond | `run` → `Usb::serve` → `select_at` → `Instance::new` | même chemin | |

Le surcoût de pile vient de l'`InputSnapshot` agrandi du `Bus` (niveaux 64 o, ports 64 o), copié par emplacement dans
`AppHost::update`, puis (15f) du nom de la source de chaque réglage gardé par `Modulations` (8 o par emplacement), qui
agrandit chaque applet construite par `Instance::new`.

## Essai sur le module (2026-11-27)

Firmware de l'arbre de travail (lots 15a à 15i) flashé par `cargo xtask flash --yes`, essais par la sonde
(`c439365-dirty`), une horloge de 105 BPM sur `TR1` ; les `@N` ci-dessous comptent depuis 0, comme la sonde le
faisait alors (15j les décale de 1) :

* `MELODY` @0, `RATCHET` @1, `SUPERSAW` @2 ; `route @0 clock=2:burst reset=off` : `CLOCK <- 2:BURST`,
  `RESET <- OFF` ; `clock=1:note` (son propre port) et `clock=3:l/mono` (audio) refusés (`NoSuch`) ;
* 6 s de flux à chaque milliseconde : `COUNT 1`, 10 fronts sur `TR1`, 21 gates de `MELODY` ; `COUNT 3`, 11 fronts,
  41 gates. Avec `PASSTHRU ON`, la rafale porte le trigger entrant plus `COUNT` (2 et 4 par horloge) : aucun trigger
  perdu par le chemin interne, seuil de 1 V compris ;
* `assign @2 note input=1:note` : `NOTE CV 1:NOTE` ; `scale=0.5` refusé par la sonde, `input=3:l/mono` par le module ;
  horloge de `MELODY` sur `OFF`, note tenue F4 : sortie A 417 mV, `SUPERSAW` 349,309 Hz (F4 exact 349,228 Hz :
  +0,4 cent, l'arrondi au millivolt de `MELODY`) ; `OCTAVE` +1 / −1 : 698,618 / 174,656 Hz, exactement ×2 / ÷2.
  `TRANSPOSE` n'agit qu'à l'horloge suivante, comportement de `MELODY` antérieur à l'étape ;
* écrans capturés (`cargo probe screen`) : `SUPERSAW` écrit `NOTE  =1:NOTE` sans valeur ; `RATCHET` écrit
  `106 BPM ○`, la rafale sur la frise.

Après 15j (`@1`–`@4`), sur le même firmware, la page web fermée : `instances` liste `@1 MELODY` à `@4 NOTHING`,
`get @3 freq` lit `SUPERSAW`, `route @1` et `focus @3` visent le bon emplacement ; `@0` et `@5` répondent
« an instance is @1 to @4 » (avant ce correctif, la recherche par nom les prenait pour des applets).

Pas faits : les gestes au panneau (sous-menu, page de routage) sur le module, l'écoute, la page web dans Chrome.

## Réserves

* Sur le module, essayé par la sonde seulement : gestes au panneau, écoute et page dans Chrome restent (TESTING 9.3j).
* La page liste `1:NOTE`… sans le nom de l'app.
* Un trigger interne a un seuil fixe (1 V) : une hauteur prise pour horloge resterait haute.
* Assignations toujours non sauvegardées.
