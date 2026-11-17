# Journal de décisions — `Alambic`

Journal d'ADR (*Architecture Decision Records*) : un fichier par arbitrage d'architecture, avec les options
écartées et ce qui ferait changer d'avis. Format repris de `~/haveneer/make_sound/journal` et
`~/haveneer/slidekit/journal`.

## Ce qui entre ici

Ce que **le matériel et l'objectif ne fixent pas**. Le module Ornament & Crime (Teensy 4.0, DAC8565, OLED,
4 CV in / 4 trigger in / 4 CV out, 2 encodeurs, 2 boutons) impose ses broches, ses convertisseurs et ses étages
analogiques ; l'objectif (une fondation d'ingénierie vérifiable : firmware, simulateur, module VCV Rack, flash
sûr) dit ce qui doit marcher. Ni l'un ni l'autre ne disent le découpage en crates, l'unité de signal, le modèle
d'exécution (boucle, interruptions, DMA), la stratégie de test, la forme de l'ABI C ni la politique d'outillage.
C'est le périmètre de ce journal.

Une propriété du matériel n'est pas une décision : « les étages d'entrée et de sortie inversent » est un fait lu
dans le firmware de référence ; le choix de l'encoder comme **pente négative** dans la calibration, plutôt que comme
une inversion dans le code, en est une (D-006). Une mesure (durée d'un tick, coût d'une trame OLED) va dans le
fichier d'avancement de l'étape, pas ici.

## Format

Un fichier par décision, `D-NNN-slug-en-anglais.md`, contenu en français. En-tête : identifiant, date, auteur, statut
(`proposée` / `adoptée` / `révisée` / `abandonnée`), commits concernés. Puis cinq champs, tous obligatoires :

1. **Contexte** — la question qui se posait ;
2. **Options envisagées** — au moins deux, avec ce qui les distingue ;
3. **Ce que l'assistant a proposé** — fidèlement, y compris quand c'était la bonne réponse ;
4. **Ce que nous avons décidé, et pourquoi** ;
5. **Ce qui nous ferait changer d'avis** — le signal qui dirait que c'était une erreur.

Une décision `proposée` a ses champs 1 à 3 remplis ; le champ 4 dit ce qui reste à trancher et comment (mesure,
essai sur le module, question à l'utilisateur). Elle passe à `adoptée` dans le commit qui la tranche.

Une entrée peut finir par une section **« Points ouverts (revue du AAAA-MM-JJ) »** : doute identifié à la relecture,
pas encore tranché. Elle ne change pas le statut ; la décision qui en sortira, si elle diffère, se consigne comme une
révision (section datée, `Statut: adoptée, révisée`).

## Entrées rétrospectives : D-002 à D-023

Le journal a été ouvert le 2026-11-16, après 24 commits (du 2026-08-01 au 2026-09-05). Les entrées **D-002 à D-023
sont reconstruites** depuis `git log`, le plan de fondation `.junie/plans/oc-rust-firmware-foundation.md`
(§ « Key Decisions », écarts consignés), `crates/oc-firmware/MEMORY.md`, `AGENTS.md`, `TESTING.md` et le code. Elles
portent `Statut: adoptée (rétrospective)` (D-023 : `proposée`). Deux conséquences, assumées plutôt que masquées :

* le champ 3, « ce que l'assistant a proposé », est **partiellement inconnaissable**. Les six premiers commits sont
  co-signés par l'assistant Junie, qui a rédigé le plan de fondation (ses « Key Decisions » portent la mention
  « validé ») ; plusieurs commits de septembre sont co-signés par Claude et argumentés dans leur message. Là où la
  trace ne dit rien, l'entrée écrit « non consigné à l'époque » au lieu de reconstituer un dialogue plausible ;
* `git log --diff-filter=A -- journal/` montre ces entrées ajoutées **d'un seul commit**. C'est la vérité : le
  journal commence là. La progression étalée que cette commande vérifie ne vaut qu'à partir de D-024.

## Règle d'antériorité

À partir de D-024, l'entrée est commitée **avant ou avec** le code qu'elle décrit, en premier fichier du commit
concerné.

```bash
git log --diff-filter=A -- journal/
```

Si l'implémentation fait dévier une décision déjà consignée, on **révise** l'entrée (`Statut: adoptée, révisée`, plus
une section datée en fin de fichier) au lieu d'en ouvrir une nouvelle. Les numéros ne sont jamais réattribués.

## Index

| ID | Décision | Statut |
|---|---|---|
| [D-001](D-001-repository-conventions.md) | conventions du dépôt : `PLAN.md` index, `docs/plan/`, journal, langue, définition de « fait » | adoptée |
| [D-002](D-002-single-behavioural-core.md) | un seul cœur comportemental `oc-core` (`no_std`, traits de plateforme, `Engine::tick`) | adoptée (rétrospective), révisée |
| [D-003](D-003-crate-split.md) | découpage en crates ; `oc-drivers` apparu en cours de route ; firmware hors `default-members` | adoptée (rétrospective), révisée |
| [D-004](D-004-cross-compilation-toolchain.md) | chaîne croisée : script de lien généré par `imxrt-rt`, ni `memory.x` ni `flip-link`, LLVM du sysroot, `RUSTFLAGS` proscrit | adoptée (rétrospective) |
| [D-005](D-005-millivolt-signal-unit.md) | unité de signal interne : millivolts `i32`, flottant seulement à la frontière VCV | adoptée (rétrospective) |
| [D-006](D-006-signed-slope-calibration.md) | calibration affine à pente signée ; étages inverseurs = pente négative ; constantes provisoires | adoptée (rétrospective), révisée |
| [D-007](D-007-pinout-in-board-rs.md) | brochage centralisé dans `board.rs`, dérivé du firmware de référence | adoptée (rétrospective), révisée |
| [D-008](D-008-cooperative-1khz-loop.md) | boucle coopérative à 1 kHz, sans RTOS ni interruption ; rendu découplé (1 tick sur 20) | adoptée (rétrospective), révisée |
| [D-009](D-009-display-framebuffer-and-oled-driver.md) | écran : framebuffer 1 bpp dans l'ordre du contrôleur, pilote maison, SH1106 par défaut | adoptée (rétrospective), révisée |
| [D-010](D-010-shared-spi-bus.md) | bus SPI partagé DAC / OLED : `SharedBus` sur `RefCell`, CS en GPIO, 8 MHz, tronçons ≤ 512 octets | adoptée (rétrospective), révisée |
| [D-011](D-011-no-uart-usb-cdc-diagnostics.md) | aucun UART ; diagnostic par USB CDC sondé et LED de démarrage | adoptée (rétrospective), révisée |
| [D-012](D-012-native-simulation-over-renode.md) | harnais principal = simulation native ; Renode limité à un smoke test, resté non fait | adoptée (rétrospective) |
| [D-013](D-013-simulator-tui-and-scenarios.md) | simulateur : TUI ratatui, horloge virtuelle déterministe, scénarios texte et écrans dorés | adoptée (rétrospective), révisée |
| [D-014](D-014-host-only-test-strategy.md) | stratégie de test : tout sur l'hôte, firmware sans tests, bus simulés, proptest, criterion, niveau 9 manuel | adoptée (rétrospective) |
| [D-015](D-015-vcv-rack-shim-over-c-abi.md) | module VCV Rack 2 : coquille C++ sur `staticlib`, ABI C défensive, tick décimé à 1 kHz | adoptée (rétrospective), révisée |
| [D-016](D-016-safe-flash-and-xtask.md) | `cargo xtask` seul point d'entrée ; flash = validations puis `teensy_loader_cli`, sans sonde USB | adoptée (rétrospective), révisée |
| [D-017](D-017-ci-and-lint-policy.md) | CI GitHub Actions, `clippy::pedantic` hôte et ARM, toolchain épinglée (1.100.0, = MSRV), tests macOS à la demande, plugin VCV hors CI | adoptée (rétrospective), révisée |
| [D-018](D-018-release-profile-and-panic.md) | profil release : LTO, `overflow-checks` actifs ; une panique arrête le module (SOS) | adoptée (rétrospective) |
| [D-019](D-019-debounce-and-activity-detection.md) | anti-rebond 3 échantillons pour les boutons, front pur pour les triggers ; activité par crête-à-crête, aucune présence de câble | adoptée (rétrospective), révisée |
| [D-020](D-020-boot-splash.md) | écran de démarrage : 1,5 s, sorties au repos, rendu à chaque tick | adoptée (rétrospective) |
| [D-021](D-021-app-host-and-menu-chord.md) | plusieurs applets résidents, menu par accord haut + bas, haut / bas agissent au relâchement | adoptée (rétrospective), révisée |
| [D-022](D-022-license.md) | licence `MIT OR Apache-2.0`, jamais écrite comme décision | adoptée (rétrospective) |
| [D-023](D-023-dual-rate-signal-isr.md) | double cadence : ISR signal (GPT2, 16 666 Hz) et boucle UI 1 kHz — ou audio 48 kHz + DMA ; **C+ adoptée** : 1 kHz sans trous (capture des fronts, écran par pages) | adoptée, révisée |
| [D-024](D-024-derived-input-presence.md) | présence d'une entrée déduite du signal : activité récente avec maintien, réglages CV / trigger (verrou, horloge) | adoptée, révisée |
| [D-025](D-025-build-identifier.md) | identifiant de build (hash court + `-dirty`) sur l'écran de démarrage des trois backends, crate `oc-buildinfo` sur vergen-gix | adoptée, révisée |
| [D-026](D-026-system-settings-screen.md) | réglages système : écran dans `oc-core`, gestes de démarrage et assistant calqués sur Phazerville, item `SETUP` du menu | adoptée, révisée |
| [D-027](D-027-calibration-in-core.md) | calibration dans `oc-core` (codes bruts à la frontière) ; modèle Phazerville : table DAC par octave, zéro ADC par canal, gain sur CV1 | adoptée, révisée |
| [D-028](D-028-settings-persistence.md) | persistance : `postcard` + `sequential-storage` sur `embedded-storage`, flash du Teensy 4.0 par l'API ROM, fichier (sim), JSON du patch (Rack) | adoptée, révisée |
| [D-029](D-029-live-probe.md) | sonde live : profilage dans `oc-core`, second port USB CDC binaire (`postcard` COBS), crates `oc-protocol` et `oc-probe` | adoptée, révisée |
| [D-030](D-030-graphics-primitives-and-demo.md) | primitives graphiques `oc_core::gfx` (bases en octets, contrôles, courbes, motifs) et applet de démonstration dans le menu ; `embedded-graphics` en dépendance de test | adoptée, révisée |
| [D-031](D-031-browser-host-tools.md) | outils hôte dans le navigateur : sonde par Web Serial, flash par WebHID (HalfKay), cœur sans E/S en WebAssembly, crates `oc-image`, `oc-halfkay`, `oc-web` | adoptée, révisée |
| [D-032](D-032-ratchet-applet.md) | applet Ratchet : A copie de TR1, B rafale (nombre ou gate, `SHAPE` signé, placement `FIRST` / `SPAN`, probabilité, synchronisation sur TR1), triggers de 5 ms sur le DAC, entrées CV au choix | adoptée |
| [D-033](D-033-audio-path.md) | chemin audio : DAC à 48 kHz dans une interruption de timer, CS matériel (PCS0), horloge par périphérique, bus possédé par l'interruption et cédé à l'OLED par tronçons, voix séparée du contrôle dans `oc-core`, applet `TONE` | adoptée, révisée |
