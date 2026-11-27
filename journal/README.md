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

D-042 est aussi rétrospective : elle formalise, sans code, le couplage du calcul audio et du DMA que D-033 avait laissé
implicite (2026-11-20).

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
| [D-001](D-001-repository-conventions.md) | conventions du dépôt : `PLAN.md` index, `docs/plan/`, journal, langue, définition de « fait » | adoptée, révisée |
| [D-002](D-002-single-behavioural-core.md) | un seul cœur comportemental `oc-core` (`no_std`, traits de plateforme, `Engine::tick`) | adoptée (rétrospective), révisée |
| [D-003](D-003-crate-split.md) | découpage en crates ; `oc-drivers` apparu en cours de route ; firmware hors `default-members` | adoptée (rétrospective), révisée |
| [D-004](D-004-cross-compilation-toolchain.md) | chaîne croisée : script de lien généré par `imxrt-rt`, ni `memory.x` ni `flip-link`, LLVM du sysroot, `RUSTFLAGS` proscrit | adoptée (rétrospective) |
| [D-005](D-005-millivolt-signal-unit.md) | unité de signal interne : millivolts `i32`, flottant à la frontière VCV et dans le calcul des blocs DSP, millivolts à leurs bords (D-039) | adoptée (rétrospective), révisée |
| [D-006](D-006-signed-slope-calibration.md) | calibration affine à pente signée ; étages inverseurs = pente négative ; constantes provisoires | adoptée (rétrospective), révisée |
| [D-007](D-007-pinout-in-board-rs.md) | brochage centralisé dans `board.rs`, dérivé du firmware de référence ; depuis le 2026-11-26 la crate `oc-board` | adoptée (rétrospective), révisée |
| [D-008](D-008-cooperative-1khz-loop.md) | boucle coopérative à 1 kHz, sans RTOS ni interruption ; rendu découplé (1 tick sur 20) | adoptée (rétrospective), révisée |
| [D-009](D-009-display-framebuffer-and-oled-driver.md) | écran : framebuffer 1 bpp dans l'ordre du contrôleur, pilote maison, SH1106 par défaut | adoptée (rétrospective), révisée |
| [D-010](D-010-shared-spi-bus.md) | bus SPI partagé DAC / OLED : `SharedBus` sur `RefCell`, CS en GPIO, 8 MHz, tronçons ≤ 512 octets | adoptée (rétrospective), révisée |
| [D-011](D-011-no-uart-usb-cdc-diagnostics.md) | aucun UART ; diagnostic par USB CDC sondé et LED de démarrage | adoptée (rétrospective), révisée |
| [D-012](D-012-native-simulation-over-renode.md) | harnais principal = simulation native ; Renode limité à un smoke test, resté non fait | adoptée (rétrospective) |
| [D-013](D-013-simulator-tui-and-scenarios.md) | simulateur : TUI ratatui, horloge virtuelle déterministe, scénarios texte et écrans de référence | adoptée (rétrospective), révisée |
| [D-014](D-014-host-only-test-strategy.md) | stratégie de test : tout sur l'hôte, firmware sans tests, bus simulés, proptest, criterion, niveau 9 manuel | adoptée (rétrospective) |
| [D-015](D-015-vcv-rack-shim-over-c-abi.md) | module VCV Rack 2 : coquille C++ sur `staticlib`, ABI C défensive, tick décimé à 1 kHz | adoptée (rétrospective), révisée |
| [D-016](D-016-safe-flash-and-xtask.md) | `cargo xtask` seul point d'entrée ; flash = validations puis `teensy_loader_cli`, sans sonde USB | adoptée (rétrospective), révisée |
| [D-017](D-017-ci-and-lint-policy.md) | CI GitHub Actions, `clippy::pedantic` hôte et ARM, toolchain épinglée (1.100.0, = MSRV), tests macOS à la demande, plugin VCV hors CI | adoptée (rétrospective), révisée |
| [D-018](D-018-release-profile-and-panic.md) | profil release : LTO, `overflow-checks` actifs ; une panique arrête le module (SOS) | adoptée (rétrospective) |
| [D-019](D-019-debounce-and-activity-detection.md) | anti-rebond 3 échantillons pour les boutons, front pur pour les triggers ; activité par crête-à-crête, aucune présence de câble | adoptée (rétrospective), révisée |
| [D-020](D-020-boot-splash.md) | écran de démarrage : 1,5 s, sorties au repos, rendu à chaque tick | adoptée (rétrospective) |
| [D-021](D-021-app-host-and-menu-chord.md) | plusieurs applets résidents, menu par accord haut + bas, haut / bas agissent au relâchement ; depuis D-046 : instances, haut / bas au panneau (court : instance, long : routage) ; menu en deux colonnes, catégories à gauche (2026-11-26) | adoptée (rétrospective), révisée |
| [D-022](D-022-license.md) | licence `MIT OR Apache-2.0`, jamais écrite comme décision | adoptée (rétrospective) |
| [D-023](D-023-dual-rate-signal-isr.md) | double cadence : ISR signal (GPT2, 16 666 Hz) et boucle UI 1 kHz — ou audio 48 kHz + DMA ; **C+ adoptée** : 1 kHz sans trous (capture des fronts, écran par pages) | adoptée, révisée |
| [D-024](D-024-derived-input-presence.md) | présence d'une entrée déduite du signal : activité récente avec maintien, réglages CV / trigger (verrou, horloge) | adoptée, révisée |
| [D-025](D-025-build-identifier.md) | identifiant de build (hash court + `-dirty`) sur l'écran de démarrage des trois backends et dans le titre de `SETUP`, crate `oc-buildinfo` sur vergen-gix | adoptée, révisée |
| [D-026](D-026-system-settings-screen.md) | réglages système : écran dans `oc-core`, gestes de démarrage et assistant calqués sur Phazerville, item `SETUP` du menu | adoptée, révisée |
| [D-027](D-027-calibration-in-core.md) | calibration dans `oc-core` (codes bruts à la frontière) ; modèle Phazerville : table DAC par octave, zéro ADC par canal, gain sur CV1 | adoptée, révisée |
| [D-028](D-028-settings-persistence.md) | persistance : `postcard` + `sequential-storage` sur `embedded-storage`, flash du Teensy 4.0 par l'API ROM, fichier (sim), JSON du patch (Rack) | adoptée, révisée |
| [D-029](D-029-live-probe.md) | sonde live : profilage dans `oc-core`, second port USB CDC binaire (`postcard` COBS), crates `oc-protocol` et `oc-probe` | adoptée, révisée |
| [D-030](D-030-graphics-primitives-and-demo.md) | primitives graphiques `oc_core::gfx` (bases en octets, contrôles, courbes, motifs) et applet de démonstration dans le menu ; `embedded-graphics` en dépendance de test | adoptée, révisée |
| [D-031](D-031-browser-host-tools.md) | outils hôte dans le navigateur : sonde par Web Serial, flash par WebHID (HalfKay), cœur sans E/S en WebAssembly, crates `oc-image`, `oc-halfkay`, `oc-web` | adoptée, révisée |
| [D-032](D-032-ratchet-applet.md) | applet Ratchet : A copie de TR1, B rafale (nombre ou gate, `SHAPE` signé, placement `FIRST` / `SPAN`, probabilité, synchronisation sur TR1), triggers de 5 ms sur le DAC, entrées CV au choix | adoptée, révisée |
| [D-033](D-033-audio-path.md) | chemin audio : DAC à 48 kHz dans une interruption de timer, CS matériel (PCS0), horloge par périphérique, bus possédé par l'interruption et cédé à l'OLED par tronçons, voix séparée du contrôle dans `oc-core`, applet `TONE` | adoptée, révisée |
| [D-034](D-034-release-and-pages.md) | publication : à chaque étiquette `v*`, une release GitHub avec les images du firmware (ELF et HEX SH1106, HEX SSD1306 et SSD1309, SHA-256) et la page web déployée sur GitHub Pages | adoptée |
| [D-035](D-035-ratchet-cv-assignment.md) | Ratchet : sous-menu d'assignation des entrées CV, par réglage : entrée, lecture en direct et bornes vues, `SCALE`, `SHIFT`, `MIN` / `MAX`, effet `SET` / `ADD` / `MUL` (5 V pour l'unité) | adoptée, révisée |
| [D-036](D-036-cv-modulation-module.md) | module partagé `oc_core::modulation` : `Assign`, `Effect`, plage vue, `CENTER`, et l'éditeur (`AssignEditor`, `Param`) réutilisable par toute applet ; l'applet garde le sens de ses paramètres | adoptée |
| [D-037](D-037-supersaw-applet.md) | applet Supersaw : sept scies PolyBLEP en entier (désaccord et mix de Szabo, passe-haut sur la fondamentale), banc d'oscillateurs partagé par les sorties (A / B stéréo, C sub-octave), ×4 seulement si le repliement s'entend ; entrées assignables par `oc_core::modulation` ; scies descendantes contre l'écrêtage | adoptée, révisée |
| [D-038](D-038-memory-budget-and-placement.md) | budget mémoire : disposition du BSP gardée (code en ITCM, FlexRAM 192 + 320 Kio), configurée par le `build.rs` d'`oc-firmware` hors de la feature `rt` ; coût d'une applet mesuré par différence de compilations ; si l'ITCM manque, bancs repartagés puis code en flash ; tampon de mesure derrière une feature, en `.uninit` (OCRAM), arena au premier grand usage d'applet ; ce que lit le DMA reste en DTCM ; `xtask size` lit le partage dans l'ELF et avertit sous 16 Kio d'ITCM libres ; profondeur de la pile estimée par `cargo xtask size`, appels `dyn App` compris, moteur démarré une fois en place (2026-11-27) ; profil `release` en `opt-level = "s"` par défaut, `3` pour `oc-dsp`, `oc-realtime`, `oc-vcv-ffi` (justifié dans `Cargo.toml`) | adoptée, révisée |
| [D-039](D-039-supersaw-float-trial.md) | le Supersaw en `f32` : essayé contre l'entier par le réglage `CALC`, adopté sur mesure (moins cher, aussi précis), banc entier retiré ; phases entières, comparaisons sans flottant ; le DSP à venir en `f32` | adoptée, révisée |
| [D-040](D-040-voice-setup-in-the-loop.md) | voix préparée dans la boucle : `Setup::new` (pas, inverses, coefficients des deux bancs), fonction pure calculée hors section critique ; l'interruption audio ne fait que copier (`Voice::apply`) ; le moteur ne rend aucune voix, chaque backend la joue (`Follower`, `Player`) | adoptée, révisée |
| [D-041](D-041-output-unit-below-the-millivolt.md) | unité des sorties sous le millivolt : micro-volts `i32` partout (B) ou volts `f32` pour le chemin audio seul (C), contre le millivolt de D-005 (A) ; à trancher par le plancher de bruit de la sortie mesuré sous `TONE` | proposée |
| [D-042](D-042-per-sample-audio-rendering.md) | rendu audio échantillon par échantillon dans l'interruption de fin de copie du DMA (anneau de 8, deux d'avance), pas par blocs ; blocs si `voice` approche la moitié de la période, API par blocs dans `oc-core` au premier backend par blocs | adoptée (rétrospective) |
| [D-043](D-043-app-trait-and-parameter-model.md) | un trait `App` : registre sans accesseurs par app, modèle de paramètres, d'observables et d'actions (un même `set` pour le panneau, la sonde USB, la flash, le routage futur), clé stable persistée, écran et navigation propres à chaque app ; premier pas vers plusieurs apps simultanées et le routage interne | adoptée, révisée |
| [D-044](D-044-audio-generators.md) | des générateurs pour la sortie audio : trait `Generator` (`Params`, `Setup`, `State`, `next`), enum composé à l'aiguillage statique, `audio` ne nomme plus le Supersaw ; le banc dans l'app `SUPERSAW`, une bibliothèque DSP au premier partage ; ce que jouent les backends dans `oc_core::playback` ; le DSP du supersaw dans `oc-dsp` (2026-11-26) | adoptée, révisée |
| [D-045](D-045-cv-assignment-by-the-trait.md) | l'assignation des CV par le trait `App` : emplacement par le rang parmi les `assignable`, état du sous-menu commun dans `modulation`, `GetAssign` / `SetAssign` à la sonde, `CENTER` côté hôte ; un seul sous-panneau web | adoptée |
| [D-046](D-046-simultaneous-applets-and-routing.md) | plusieurs applets à la fois : pool fixe d'instances (deux fois la même app possible), ports de sortie routés vers les sorties physiques sans recouvrement ou désactivés, carte d'entrées par instance, une `Voice` et un `Setup` par instance ; une seule au focus, appui court sur `up` / `down`, page de routage à l'appui long de `up` (révision du 2026-11-26), indicateur ; protocole 9 par instance ; depuis le 2026-11-25, quatre emplacements toujours occupés (`NOTHING`), protocole 10 | adoptée, révisée |
| [D-047](D-047-melody-applet.md) | applet Melody : séquence cyclique de notes en demi-tons quantifiées sur une gamme, remplacement aléatoire au passage (`PROB`, `RANGE`, `SPREAD` / `BIAS`, marche, `REROLL`), horloge et reset armé en ports, accord diatonique de deux tons sur B et C, gate sur D, chaque port routé (révision du 2026-11-26), édition des pas au panneau ; pas en position normalisée dans une fenêtre `SPREAD` / `CENTER` / `BIAS` en direct (révision du 2026-11-27) | adoptée, révisée |
| [D-048](D-048-internal-routing.md) | routage interne : une assignation lit une prise ou un port CV d'une autre instance (`Source`), ports déclarés `Cv` / `Audio`, niveau publié au tick précédent, l'app elle-même sautée ; un port assigne strictement (comme `SET`, sans mise en forme) et se nomme `1:NOTE` ; un port d'entrée (horloge, CV) lit aussi un port interne, trigger haut dès 1 V (révisions du 2026-11-27) ; protocole 11 | adoptée, révisée |
