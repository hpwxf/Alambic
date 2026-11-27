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

## Mesures

`cargo xtask size`, contre `master` (`cca9e53`) compilé dans un worktree :

| | `master` | étape 15 | écart |
|---|---|---|---|
| `.text` (ITCM) | 0x21b84 (57,1 Kio libres) | 0x22954 (53,7 Kio libres) | +3 536 o |
| `.bss` | 0x6208 | 0x6208 | 0 en apparence (alignement) |
| pile estimée (main + 3 interruptions) | 10 056 o (6 328 o libres) | 10 400 o (5 984 o libres) | +344 o |
| chemin le plus profond | `run` → `Usb::serve` → `select_at` → `Instance::new` | même chemin | |

Le surcoût de pile vient de l'`InputSnapshot` agrandi du `Bus` (niveaux 64 o, ports 64 o), copié par emplacement dans
`AppHost::update`.

## Réserves

* Pas encore essayé sur le module (TESTING 9.3j), ni la page dans Chrome.
* La page liste `2a`… sans le nom du port ni de l'app (la description des autres instances n'est pas demandée).
* Ports d'entrée internes (une horloge lue sur une sortie d'app) : lot suivant (D-048).
* Assignations toujours non sauvegardées.
