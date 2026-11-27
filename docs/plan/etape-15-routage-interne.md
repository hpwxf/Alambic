# Étape 15 — routage interne : les sorties CV d'une app comme entrée d'assignation

> Plan de l'étape 15, suivi par git (journal/D-048).

## Contexte

Aujourd'hui une assignation (`modulation::Assign`) ne lit qu'une prise physique : `input: Option<CvChannel>`.
Pour jouer `SUPERSAW` avec les notes de `MELODY`, il faut un câble de la sortie A à CV1, donc passer par le DAC
puis l'ADC (bruit, ≈ 20 mV d'erreur, une sortie physique prise). On veut que le champ `INPUT` du sous-menu
d'assignation (clic droit sur un réglage, comme pour une CV) propose aussi les **ports CV des autres apps**,
qu'ils soient routés vers une prise ou non.

Choix arrêtés avec l'utilisateur :
- **ports CV seulement** (ceux qui portent un niveau) ; les ports audio (`SUPERSAW`, `TONE`) ne sont pas
  proposés. Le genre de port est déclaré, pour laisser la place à de futurs effets à entrée audio (hors étape).
- **jamais l'app elle-même** : le champ saute son propre emplacement.
- écran principal : `+1a` = effet, numéro d'emplacement (1–4), **lettre minuscule du rang du port** (`a`–`d`,
  distinctes des prises `A`–`D`) ; la page de routage affiche cette lettre devant chaque port de sortie ;
  le sous-menu écrit le nom complet `1:NOTE`.
- ports d'entrée de la page de routage (horloge `TR1` lue sur une sortie d'app) : **lot suivant**, pas ici.

Sémantique retenue (à écrire dans D-048) : la source lit le niveau que l'app **publie** (paramètres `Level`
de son lane, avant la calibration de sortie et sans les sorties forcées par la sonde), **du tick précédent**,
pour toutes les sources : 1 ms de retard, indépendant de l'ordre des emplacements, aucune boucle possible.
Un port audio ou absent (emplacement vidé, `NOTHING`) lit 0 V ; l'assignation est gardée telle quelle.
Une app doit calculer ses ports CV même non routés (`set_routed` ne sert qu'à l'audio).

## Conception

### `oc-core`

1. **Genre de port** — `apps/mod.rs` : `Port { name, signal: PortSignal }`, `PortSignal::{Cv, Audio}`.
   `PORTS_A_TO_D` en `Cv` (Diagnostic, Scope, Gfx) ; `TONE` reçoit ses propres ports `Audio` ;
   `SUPERSAW` `Audio` ; `MELODY`, `RATCHET` `Cv`.
2. **Source d'une assignation** — `modulation/mod.rs` : `pub enum Source { Jack(CvChannel), Lane { slot: u8, port: u8 } }`
   (serde) ; `Assign::input: Option<Source>`. `Assign::write` : `+CV1` ou `+1a` ; `field(Input)` : `CV1` ou
   `1a` (forme hôte, CLI). `Assign::reading(&cv)` devient une lecture d'une valeur brute déjà résolue.
   `DEFAULT_ASSIGNS` de `SUPERSAW` : `Some(Source::Jack(CvChannel::One))`.
3. **Le bus** — nouveau type (ex. `apps::Bus`, dans `apps/mod.rs` ou `apps/bus.rs`) porté par
   `InputSnapshot` : `levels: [[MilliVolts; CV_CHANNELS]; MAX_INSTANCES]` (tick précédent),
   `ports: [&'static [Port]; MAX_INSTANCES]`, `own: usize`. `Bus::EMPTY` par défaut (engine, tests).
   Méthodes : `level(slot, port)` (0 si audio, absent ou `own`), `sources()` (liste ordonnée des
   `Source::Lane` offertes : autres emplacements, ports `Cv`), `port_name(slot, port)`.
4. **`AppHost`** — champ `levels: [[MilliVolts; CV_CHANNELS]; MAX_INSTANCES]`. Dans `update` : chaque
   instance reçoit le snapshot avec `bus = { levels: self.levels, ports, own: index }` ; après la boucle,
   `levels[slot][lane]` = `Channel::Level(mv)` des `Params` basiques pour les ports `Cv`, 0 sinon.
   `select_at` remet à 0 les niveaux de l'emplacement remplacé.
5. **`Modulations`** (`modulation/slots.rs`) — `read(&mut self, input: &InputSnapshot)` remplace
   `read(&cv, elapsed)` : résout la source de chaque emplacement en `values: [MilliVolts; N]` (remplace
   `cv`), et garde le nom de la source de l'éditeur ouvert (`&'static str`) pour le rendu. `reading(slot)`
   lit `values`. Appelants à adapter : `supersaw`, `melody`, `ratchet`, `tone`.
6. **Éditeur** (`modulation/editor.rs`) — `Assign::turn(Input)` parcourt `None, CV1–CV4` puis
   `input.bus.sources()` (tableau fixe ≤ 5 + 3×4) ; une source qui n'est plus offerte reste jusqu'au
   prochain tour. `observe` / `render` prennent la valeur brute résolue au lieu du tableau `cv`. Ligne
   formule : `SCALE` placé à droite du champ d'entrée quand celui-ci est long (`1:BURST`), nom tronqué
   s'il ne tient pas.
7. **Page de routage** (`routing.rs`) — lettre `a`–`d` devant chaque ligne de sortie (`a NOTE -> A`).
8. `MilliVolts` partout, aucune entrée par backend : tout reste dans `oc-core` (règle D-002) ; le simulateur
   et Rack n'ont rien à changer.

### Protocole 11 (`oc-protocol`, `oc-probe`, `oc-web`, `web/app.js`)

- `VERSION` 10 → 11 : `Assign` sérialisé avec `Source` ; `Reply::Port` reçoit `level: bool` pour une sortie.
- `oc-probe/src/apps.rs` : `parse_input_field("input", …)` accepte `none`, `cv1`–`cv4`, `1a`–`4d` ;
  `input_text` écrit `1a` ; refus d'une source sur son propre emplacement (côté `set_assign` : `Refused`).
  Vérifier `oc-probe/tests/client.rs`.
- `oc-web/src/applets.rs` : `input_choices("input")` devient une méthode de l'instance (liste des autres
  emplacements et de leurs ports `level`, `1a NOTE`…) ; `web/app.js` appelle la nouvelle forme.

## Lots

| Lot | Contenu |
|---|---|
| 15a | ADR `journal/D-048-internal-routing.md` (premier fichier du commit), index `journal/README.md`, `docs/plan/etape-15-routage-interne.md`, `PLAN.md` (étape 15, « Décisions actées ») |
| 15b | `PortSignal`, `Source`, `Bus`, `AppHost::levels`, `Modulations::read(input)` ; comportement des prises inchangé (écrans de référence identiques) ; tests unitaires |
| 15c | éditeur (parcours des sources, nom `1:NOTE`, place de `SCALE`), `+1a` à l'écran, lettre dans la page de routage ; `UPDATE_SCREENS=1`, diff relu ; scénario sim `MELODY` → `SUPERSAW` |
| 15d | protocole 11, CLI `cargo probe assign … input=1a`, page web ; tests |
| 15e | `README.md`, `docs/agents/code-map.md`, `apps/melody/README.md` (exemple), `TESTING.md` (niveau 9 : `MELODY` emplacement 1, `SUPERSAW` emplacement 2, `NOTE` ← `1a`, à l'accordeur), `cargo xtask size` (ITCM, pile), essai sur le module, `docs/plan/avancement/etape-15.md` |

## Fichiers critiques

`crates/oc-core/src/apps/mod.rs`, `crates/oc-core/src/modulation/{mod,slots,editor}.rs`,
`crates/oc-core/src/routing.rs`, `crates/oc-core/src/apps/{supersaw,melody,ratchet}/mod.rs`,
`crates/oc-core/src/apps/tone.rs`, `crates/oc-protocol/src/{lib,server}.rs`, `crates/oc-probe/src/apps.rs`,
`crates/oc-web/src/applets.rs`, `web/app.js`, `crates/oc-sim/tests/scenarios/`.

## Vérification

- Tests `oc-core` : résolution d'une source (prise, lane, audio → 0, propre emplacement → 0) ; un tick de
  retard ; port non routé (`MELODY CH1` sur `OFF`) lisible ; parcours de `INPUT` qui saute l'app elle-même,
  les ports audio et `NOTHING` ; intégration via `testing::mock_engine` : `MELODY` emplacement 0 horloge sur
  `TR1`, `SUPERSAW` emplacement 1 `NOTE` ← `1a` (`ADD`), la fréquence observée suit chaque pas (au demi-ton
  près, sans erreur d'ADC).
- Sim : scénario enregistré qui fait l'assignation au panneau ; écrans régénérés et relus.
- Protocole : aller-retour `postcard` de `Assign` avec `Source::Lane` ; parse/print CLI.
- Définition de fini (CI locale) : `cargo fmt --all -- --check`, les deux `cargo clippy … -D warnings`,
  `cargo test --all-features`, `cargo xtask build && cargo xtask size` (ITCM libre > 16 Kio, `stack-depth` OK).
- Module : `cargo xtask flash`, `MELODY` + `SUPERSAW` assigné, écoute et accordeur ; `cargo probe assign`
  et page web dans Chrome.
