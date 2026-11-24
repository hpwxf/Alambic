# Plan : `Alambic`, firmware Rust pour Ornament & Crime (Teensy 4.0)

> Plan destiné à être exécuté étape par étape (une session par lot, relecture entre les étapes).
> **Règle pour l'exécutant** : ne jamais passer à l'étape suivante tant que tous les critères d'acceptation de
> l'étape en cours ne sont pas satisfaits ET rapportés avec la sortie des commandes.

## Carte des documents

`PLAN.md` est le point d'entrée : carte, bilan par étape, étape en cours. Le détail vit à côté, pour qu'une session
ne recharge que ce dont elle a besoin (journal/D-001).

| Document | Contenu |
|---|---|
| **ce fichier** | carte, contexte, bilan d'une ligne par étape passée, étape **en cours** avec sa checklist, décisions actées |
| `journal/` | **ADR** — un fichier par décision d'architecture ; `README.md` donne le périmètre, le format et l'index |
| `docs/plan/roadmap.md` | étapes passées et à venir, questions ouvertes |
| `docs/plan/revue-decisions-2026-11-16.md` | revue des décisions reconstruites : à vérifier / à réviser |
| `docs/plan/etape-4-reglages-systeme.md` | plan de l'étape 4 : réglages système, calibration, persistance (lots 4a–4f) |
| `docs/plan/etape-3b-sonde.md` | plan du lot 3b (sonde live) et relevé détaillé des mesures sur module |
| `docs/plan/etape-6-ratchet.md` | plan de l'étape 6 : applet Ratchet (D-032) |
| `docs/plan/etape-7-chemin-audio.md` | plan de l'étape 7 : chemin audio, DAC à 48 kHz (D-033) |
| `docs/plan/etape-8-supersaw.md` | plan de l'étape 8 : applet Supersaw, premier oscillateur (D-037) |
| `docs/plan/etape-12-assignation-sonde.md` | plan de l'étape 12 : assignation des CV par le trait `App` et la sonde, sous-panneau commun (D-045) |
| `docs/plan/etape-13-applets-simultanees.md` | plan de l'étape 13 : plusieurs applets à la fois, ports routés (D-046) |
| `docs/hardware.md` | le matériel du module : principes, ce que disent les fiches, ce module et ses mesures ; glossaire |
| `docs/plan/rapport-etape-7-chemin-audio.md` | rapport détaillé de l'étape 7 : essais, échecs, analyses et correctifs, du DAC à 1 kHz au DAC en DMA |
| `docs/plan/panique-persistante.md` | préparé, non décidé : garder le message d'une panique à travers un redémarrage (D-018) |
| `docs/plan/etape-5-gfx.md` | plan de l'étape 5 : primitives graphiques et applet de démo (D-030) |
| `docs/plan/etape-3c-cadence.md` | plan du lot 3c (D-023 option C+) |
| `docs/plan/etape-3-plan-a-isr-16khz.md` | plan A de D-023 (ISR 16 666 Hz), proposé par Junie, non adopté ; bandeau des points caducs |
| `docs/plan/avancement/etape-*.md` | journal d'une étape terminée : fait, mesuré, corrigé |
| `docs/plan/avancement/budget-memoire.md` | hors étape : dépasser l'ITCM, partage des bancs repris en main, niveaux d'optimisation mesurés (code froid en `s`), écran noir en `z` |
| `.junie/plans/oc-rust-firmware-foundation.md` | plan détaillé de l'étape 0 (fondation), rédigé par l'assistant Junie, terminé |
| `docs/supersaw/` | étude : supersaw, repliement, PolyBLEP, architecture audio et partage du bus SPI (base de D-023) |
| `AGENTS.md` | règles et pièges du dépôt (anglais), seule source de vérité des agents ; route vers ses annexes |
| `docs/agents/` | annexes d'`AGENTS.md` (anglais) : `firmware.md` (chaîne croisée, contextes, écran, mesure des coûts), `code-map.md` (crates, où vit quoi), `tools.md` (outils hôte) |
| `TESTING.md` | protocole de test par niveau, dont le niveau 9 (validation manuelle sur le module) |
| `crates/oc-firmware/MEMORY.md` | carte mémoire du Teensy 4.0 et absence voulue de `memory.x` / `flip-link` (D-004) |
| `README.md` | doc d'usage (anglais) |

## Contexte

Firmware Rust pour le module Eurorack **Ornament & Crime** (fabrication TLM Audio, Teensy 4.0 / i.MX RT1062,
Cortex-M7 à 600 MHz). L'objectif posé à l'étape 0 n'est pas la richesse musicale mais une **fondation d'ingénierie
vérifiable** : firmware croisé, simulateur natif, module VCV Rack 2 et flash sûr, tous sur un même cœur
comportemental (`oc-core`, D-002). Les applets musicaux sont la suite.

## État d'avancement

| Étape | Bilan | Détail |
|---|---|---|
| **0** — fondation (2026-08-01) | ✅ workspace 6 crates, chaîne croisée, `oc-core` + applet diagnostic, simulateur TUI, firmware Teensy, `xtask flash` validé, module VCV (compilé et lié). Écarts : pas de `memory.x` ni `flip-link` (D-004), `oc-drivers` ajouté (D-003), aucun UART (D-011), smoke test Renode non fait (D-012) | [etape-0.md](docs/plan/avancement/etape-0.md), [plan](.junie/plans/oc-rust-firmware-foundation.md) |
| **1** — mise en route sur le module (2026-08-02) | ✅ OLED SH1106 affiché (D-009), encodeurs et boutons (pull-ups, D-007), journal USB CDC + LED de démarrage (D-011), écran de démarrage (D-020), contrôles de disposition au flash (D-016), façade VCV alignée sur le module. Réserves : calibration, entrées CV et triggers non vérifiés au niveau 9 | [etape-1.md](docs/plan/avancement/etape-1.md) |
| **2** — plusieurs applets (2026-08-19 → 09-05) | ✅ menu par accord haut + bas, applets résidents, `ScopeApp`, haut / bas au relâchement (D-021) ; module Rack renommé `Alambic` (D-015) ; étude supersaw (base de D-023). Réserve : CI rouge avec `--all-features` depuis l'ajout de la feature `octant` (D-013) | [etape-2.md](docs/plan/avancement/etape-2.md) |
| **3** — assainissement, sonde live, cadence (2026-11-16 → 11-17) | ✅ dette de revue (0.2.0) ; sonde live `oc-probe` (D-029) : profil au cycle, flux, sorties forcées, capture, redémarrage ; niveau 9 (CV, triggers, rebouclage, multimètre) ; D-023 adoptée en **C+** : fronts des triggers capturés (0,94 ms : 960 / 960), écran une page par tick (1 000 ticks/s au lieu de ≈ 947). Réserves : `signal_tick` / `ui_tick` reporté à l'option A ; DAC8565 simultané non vérifié ; entrées à ≈ 20 mV près | [etape-3.md](docs/plan/avancement/etape-3.md) |
| **4** — réglages système, calibration, persistance (2026-11-16) | ✅ écran `SETUP` et gestes de démarrage façon Phazerville (D-026) ; conversion dans `oc-core`, valeurs d'usine de Phazerville, assistant de calibration (D-027) ; réglages dans la flash du Teensy par l'API ROM, fichier (sim), patch (Rack) (D-028) ; seuil de présence 50 mV mesuré (D-024). Version **0.3.0**. Réserve : C++ Rack non compilé | [etape-4.md](docs/plan/avancement/etape-4.md) |
| **5** — primitives graphiques et applet de démo (2026-11-17) | ✅ texte par colonnes (Diagnostic 646 → 88 µs), six polices en paramètre ; `oc_core::gfx` (bases en octets, contrôles, courbes, motifs) ; applet `GFX DEMO` ; `embedded-graphics` en dépendance de test (D-030). Version **0.4.0** (avec l'étape 3 ; plugin Rack 2.4.0) | [etape-5.md](docs/plan/avancement/etape-5.md) |
| **6** — premier applet musical : Ratchet (2026-11-17 → 11-24) | ✅ `oc_core::fixed` (`log2`, `exp2` en 16.16), `RATCHET` : A copie TR1, B rafale (nombre ou gate, `SHAPE` signé, `FIRST` / `SPAN`, `PROB`, `PASSTHRU`, `SYNC`) (D-032) ; ascenseur et frise de la rafale en cours ; sous-menu d'assignation des entrées (`SCALE`, `SHIFT`, `MIN` / `MAX`, `SET` / `ADD` / `MUL`, `CENTER`, D-035), sorti dans `oc_core::modulation` (D-036) ; TESTING 9.3c concluant sur le module. Réserves : la seconde applet prévue sortie de l'étape (roadmap), quantification à 1 ms, assignations non sauvegardées | [etape-6.md](docs/plan/avancement/etape-6.md), [plan](docs/plan/etape-6-ratchet.md) |
| **7** — chemin audio (2026-11-17 → 11-18) | ✅ `oc_core::audio` (paramètres de sortie, voix), applet `TONE` ; DAC en PCS0 à 22 MHz ; PIT → DMA → LPSPI4 à **47 619 Hz** (instant fixé par le matériel), interruption du DMA pour l'échantillon et l'écran par tronçons ; `audio` 3,4 µs sur 21 ; juste à l'accordeur jusqu'à B7 (D-033). Réserves : 48 kHz impossible (quartz de GPT2 muet), quatre voies écrites à chaque période, Rack non écouté. Version **0.5.0** (avec l'étape 6 ; plugin Rack 2.5.0) | [etape-7.md](docs/plan/avancement/etape-7.md), [rapport](docs/plan/rapport-etape-7-chemin-audio.md) |
| **8** — premier oscillateur : Supersaw (2026-11-18) | ✅ `oc_core::audio::supersaw` : sept scies PolyBLEP en entier (désaccord et mix de Szabo, passe-haut sur la fondamentale), A / B mono ou stéréo, C sub-octave ; applet `SUPERSAW`, entrées par `oc_core::modulation` ; `audio` 3,9 µs sur 21 ; repliement mesuré par FFT (exemple `aliasing`) : sous 10 kHz ≤ −45 dB, ×4 non nécessaire ; scies descendantes, sans écrêtage jusqu'à 2 V (D-037) ; budget mémoire (D-038). Réserves : bruit de CV1 sur `NOTE`, assignations non sauvegardées, Rack non écouté. Version **0.6.0** (plugin Rack 2.6.0) | [etape-8.md](docs/plan/avancement/etape-8.md) |
| **9** — le Supersaw en `f32` (2026-11-19 → 11-21) | ✅ banc `f32` essayé contre l'entier par `CALC`, puis adopté : phases entières (F2), comparaisons sans flottant ; `voice` 1,07 / 0,78 µs (`SHAPE` 25 % / 0) contre 1,37 / 1,25 en entier, aussi précis (`aliasing`) ; `SHAPE` et `WIDTH`, deux pages ; voix préparée dans la boucle (D-040) ; banc entier retiré (`.text` −4,9 Kio) ; D-039 adoptée ; banc `f32` écouté. Réserve : écoute comparée des deux bancs non faite | [etape-9.md](docs/plan/avancement/etape-9.md) |
| **10** — apps modulaires : trait `App` et modèle de paramètres (2026-11-20 → 11-24) | ✅ trait `App` et registre (`&dyn App`, `find::<T>()`), accesseurs par app retirés ; modèle de paramètres, d'observables et d'actions (`ParamSpec`, `ParamValue`, `Refused`) pour les six apps ; protocole v4 à v6 (`ListApps`, `DescribeApp`, `GetParam`, `SetParam`, `Observe`, `Act`, `SelectApp`, libellé, faces inactives), CLI et panneau *Apps* de la page ; `.text` +9,2 Kio ; `voice` et `audio` inchangés sur le module ; page essayée dans Chrome sur le module (D-043). Réserves : observables hors `Stream`, pas d'`AppKey`, une seule app à la fois (étape 13) | [etape-10.md](docs/plan/avancement/etape-10.md) |
| **11** — générateurs audio (2026-11-21) | ✅ trait `Generator`, `Params` / `Setup` / `Voice` génériques, générateurs `Basic` et `SupersawBank` composés dans `apps/sound.rs` ; le banc dans l'app `SUPERSAW`, `oc_core::audio` ne nomme plus d'applet ; maintien par la sonde en surcouche ; échantillons identiques au bit près ; sur le module `voice` 1,68 → 1,56 µs, `voice_params` 0,47 → 0,50 µs ; `.text` +1,4 Kio (D-044) ; le moteur ne rend plus de voix, chaque backend joue la sienne, un seul `Setup` par changement : `tick` 21,1 → 15,0 µs sous CV, voix de `SUPERSAW` écoutées sur le module (D-040). Réserves : composition à la main, un générateur à la fois, pas de bibliothèque DSP | [etape-11.md](docs/plan/avancement/etape-11.md) |
| **12** — assignation des CV par la sonde (2026-11-22 → 11-24) | ✅ état du sous-menu commun (`modulation::slots::Modulations`), assignation au trait `App` (`assigns` / `assigns_mut` / `assign_sweep`, `assign` / `set_assign` sur `dyn App`) ; protocole 8 (`GetAssign`, `SetAssign`) ; `cargo probe assign`, `CENTER` côté hôte ; un seul sous-panneau CV dans la page, `CENTER` compris ; pile de `DescribeApp` ramenée sous `.stack` (14 628 o), surveillée par `cargo xtask size` ; écrans de référence identiques ; essayé sur le module (CLI et page) (D-045). Réserves : `CENTER` de la page pas essayé sur le module, assignations non sauvegardées | [etape-12.md](docs/plan/avancement/etape-12.md), [plan](docs/plan/etape-12-assignation-sonde.md) |

### Étape 13 — plusieurs applets à la fois, ports routés vers les E/S physiques (en cours)

Demandée le 2026-11-24 (D-046, proposée ; [plan](docs/plan/etape-13-applets-simultanees.md)) : deux instances
d'abord, éventuellement de la même app, entrées partagées, sorties réparties sans recouvrement ou désactivées ; une
seule à l'écran, `up` / `down` court pour changer d'instance, long pour sa page de routage, indicateur en haut ;
la sonde par instance. Étape 3 de la trajectoire de D-043.

| Lot | Contenu | État |
|---|---|---|
| **13a** — ADR et plan | D-046 proposée, plan de l'étape | ✅ |
| **13b** — sorties en ports | `audio` : `Route`, `Patch`, `Voices`, `Changes` (un `Setup` par emplacement changé), `Follower` par patch ; maintien de la sonde après le routage ; `App::output_ports` (`RATCHET` A, B ; `SUPERSAW` A, B, `SUB`) ; `MAX_INSTANCES = 2`, un emplacement servi ; sommes de `voice_samples` par le chemin entier et écrans inchangés ; `.text` +3 848 o, `.data` +440 o ; criterion : `tick` inchangé, aiguillage ≈ 9 ns par échantillon, second banc ≈ 10 ns | ✅ |
| **13c** — instances | pool d'`Instance` (position et slot séparés), focus, toutes mises à jour, panneau et rendu au focus ; `select` neuve, `add`, `remove`, `set_route` (`Refused::Taken`) ; `App::set_routed` (`SUPERSAW` sans `SUB` routée nulle part) ; sonde par la première instance, lecture d'une app arrêtée sur une neuve ; `.text` +1 456 o, pile −248 o ; `tests/instances.rs`. Menu et carte d'entrées reportés (13d, 13e) | ✅ |
| **13d** — panneau | appui court (instance) / long (routage, 1 s), actions `up` / `down` des apps déplacées, indicateur de 4 pixels à gauche du nom ; menu : ajouter, retirer, défilement (révision de D-021) | |
| **13e** — page de routage | écran du moteur commun aux apps | |
| **13f** — sonde | protocole 9 par instance, CLI, page | |
| **13g** — module | deux `SUPERSAW`, `RATCHET` + `SUPERSAW` : profil, écoute, TESTING ; adoption de D-046 | |

## Décisions actées

Valeurs seules ; le raisonnement est dans `journal/`.

| Sujet | Valeur | ADR |
|---|---|---|
| conventions | `PLAN.md` index, `docs/plan/`, journal d'ADR, docs de travail en français, code et commits en anglais | D-001 |
| cœur | `oc-core` `no_std`, `forbid(unsafe_code)`, six traits de plateforme, `Engine::tick` seul point d'appel | D-002 |
| crates | `oc-core`, `oc-drivers`, `oc-firmware` (hors `default-members`), `oc-sim`, `oc-vcv-ffi`, `oc-buildinfo` (dépendance de build), `xtask` ; `oc-image`, `oc-halfkay`, `oc-web` (outils web) | D-003, D-025, D-031 |
| chaîne croisée | `thumbv7em-none-eabihf`, `t4link.x` généré par `imxrt-rt`, pas de `memory.x` ni `flip-link`, outils LLVM du sysroot, jamais `RUSTFLAGS` | D-004 |
| signal | millivolts `i32` ; `f32` dans le calcul des blocs DSP (accumulateurs entiers, millivolts aux bords) et dans le C++ du module Rack | D-005, D-039 |
| calibration | pente signée (négative : étages inverseurs) ; dans `oc-core`, codes bruts à la frontière ; entrées : zéro par canal (≈ 2730), gain commun ; sorties : table de 11 points par octave ; mesurée par assistant, persistée | D-006, D-027 |
| brochage | table unique et réservation des broches dans `board.rs` (`split` + groupes par signal, test `xtask/tests/pin_rule.rs`), dérivée de `OC_gpio.cpp` ; pull-up 22 kΩ + hystérésis sur les entrées numériques | D-007 |
| exécution | boucle coopérative 1 kHz pour le contrôle (entrées, applets, dessin, USB) ; à côté, le chemin audio en interruption (B de D-023, D-033) ; rendu tous les 20 ticks (≈ 50 Hz) ; option C+ : fronts des triggers capturés par les GPIO ; A (ISR 16 kHz) en suite | D-008, D-023, D-033 |
| écran | framebuffer 1 024 octets dans l'ordre du contrôleur ; pilote `ssd130x` maison ; SH1106 par défaut, `ssd1306` / `ssd1309` en features ; trame remise à l'interruption audio, envoyée par tronçons (`FrameStream`) | D-009, D-033 |
| bus SPI | DAC8565 + OLED sur LPSPI4, possédé par le chemin audio et cédé à l'OLED par tronçons ; `DAC_CS` en PCS0 matériel, OLED sur PCS3 (sans broche) avec CS en GPIO ; 22 MHz pour les deux, OLED à 8 MHz en feature `oled-spi-8mhz` | D-010, D-033 |
| diagnostic | aucun UART ; USB CDC sondé, deux ports (journal texte, sonde binaire) ; LED de démarrage avant la prise du SPI | D-011, D-029 |
| harnais | simulation native ; Renode non fait | D-012 |
| simulateur | TUI ratatui (crossterm), horloge virtuelle, scénarios `.scn` + écrans `.screen` (`UPDATE_SCREENS=1`) | D-013 |
| tests | tout sur l'hôte, `oc-firmware` sans tests, bus simulés, proptest, criterion compilé en CI, niveau 9 manuel | D-014 |
| VCV Rack | `staticlib` + ABI C défensive (`catch_unwind`, `cbindgen`), coquille C++, tick décimé à 1 kHz, slug `Alambic` | D-015 |
| flash | `cargo xtask` seul point d'entrée ; validations ELF puis `teensy_loader_cli`, sans sonde USB | D-016 |
| CI | GitHub Actions ; fmt, clippy pedantic hôte + ARM, tests Linux (macOS à la demande), image firmware, bancs compilés, `flash --dry-run` ; toolchain épinglée **1.100.0** = MSRV | D-017 |
| profil | release LTO fat, `overflow-checks = true` ; panique → journal USB + SOS | D-018 |
| entrées | anti-rebond 3 échantillons (boutons), front sur 1 échantillon (triggers) ; activité CV par crête-à-crête (256 éch., 40 mV), aucune présence de câble sur aucun backend | D-019 |
| démarrage | écran 1,5 s, sorties à 0 V, rendu à chaque tick | D-020 |
| applets | `AppHost` (tous résidents), menu par accord haut + bas, haut / bas au relâchement ; `DIAGNOSTIC`, `SCOPE`, `GFX DEMO`, `RATCHET`, `TONE`, `SUPERSAW` ; trait `App` et registre (`&dyn App`), modèle de paramètres, d'observables et d'actions partagé par le panneau, la sonde (protocole v8, assignations des CV comprises, D-045) et le simulateur ; écran et navigation propres à chaque app | D-021, D-030, D-032, D-033, D-037, D-043 |
| licence | `MIT OR Apache-2.0` (workspace) | D-022 |
| présence d'entrée | déduite dans `oc-core` (activité récente + maintien) : CV 50 mV / 300 ms, trigger verrou ou horloge 2 s ; jamais fournie par un backend | D-024 |
| identifiant de build | hash court + `-dirty` sous la version, écran de démarrage des trois backends ; version et hash dans le titre de `SETUP` ; calculé par `oc-buildinfo` (vergen-gix, sans binaire `git`), jamais dans `oc-core` | D-025 |
| réglages système | écran `oc-core` hors applets ; gestes au démarrage (encodeur gauche ⇒ calibration, `up` + `down` ⇒ secours) et item `SETUP` ; interaction de Phazerville | D-026 |
| persistance | `postcard` + `sequential-storage` sur `embedded-storage` ; flash Teensy 4.0 par l'API ROM (`unsafe` confiné) ; fichier (sim) ; JSON du patch (Rack) | D-028 |
| sonde live | profilage dans `oc-core` (DWT sur le module), second port USB CDC binaire `postcard` COBS, crates `oc-protocol` et `oc-probe`, toujours compilée ; un seul port encadré si l'USB MIDI arrive | D-029 |
| graphisme | `oc_core::gfx` : bases en octets entiers, contrôles, courbes, motifs ; trigonométrie entière ; applet de démo dans le menu | D-030 |
| outils web | page statique Chromium : sonde par Web Serial, flash par WebHID depuis l'ELF validé ou un HEX (vérification réduite) ; cœur wasm sans E/S (`oc-image`, `oc-halfkay`, `oc-web`), `cargo xtask web` ; essayé sur le module (sonde et flash) | D-031 |
| applet Ratchet | A copie de TR1, B rafale ; nombre ou gate, `SHAPE` signé (accélère / ralentit), placement `FIRST` ou `SPAN`, `PROB`, `PASSTHRU`, `SYNC` sur le tempo de TR1 ; triggers 5 V / 5 ms ; entrée par réglage dans un sous-menu : `SCALE`, `SHIFT`, `MIN` / `MAX`, effet `SET` / `ADD` / `MUL` | D-032, D-035 |
| modulation par une entrée CV | `oc_core::modulation` partagé entre applets : `Assign` (entrée, `SCALE`, `SHIFT`, `MIN` / `MAX`, effet), `AssignEditor` + `Param` ; l'applet fournit la plage `SET` de chaque paramètre et applique l'effet | D-036 |
| publication | à chaque étiquette `v*` : release GitHub avec les images du firmware (ELF et HEX SH1106, HEX SSD1306 / SSD1309, SHA-256) ; page web sur GitHub Pages | D-034 |
| chemin audio | **47 619 Hz** (PERCLK / 21 ; 48 kHz impossible, quartz de GPT2 muet) ; le canal 0 du PIT déclenche le DMA qui écrit les quatre voies du DAC à chaque période (instant fixé par le matériel) ; `DAC_CS` en PCS0 matériel, DAC à 22 MHz (plafond 25 MHz en 3,3 V) ; l'interruption de fin de copie du DMA calcule l'échantillon et cède des tronçons à l'OLED, celle de LPSPI4 relève son CS ; voix séparée du contrôle, préparée dans la boucle (`Setup`), copiée par l'interruption ; générateurs (`Generator`) composés dans `apps/sound.rs`, aiguillage statique, `audio` ne nomme aucune applet ; applet `TONE` | D-033, D-040, D-044 |
| rendu audio | un échantillon par période, calculé dans l'interruption de fin de copie du DMA, deux emplacements d'avance (≈ 60 µs) ; pas de blocs tant que `voice` reste loin de la moitié de la période ; `Voice::next_sample` seule API de `oc-core` | D-042 |
| assignation des CV | une entrée par paramètre assignable, nommée par l'index du paramètre (son rang parmi les `assignable`) ; état du sous-menu commun (`modulation::slots::Modulations`) ; trait `assigns` / `assigns_mut` / `assign_sweep`, `assign` / `set_assign` sur `dyn App` ; sonde : `GetAssign`, `SetAssign` (protocole 8), CLI `cargo probe assign`, `CENTER` côté hôte (CLI et page, même `Assign::center`) ; un seul sous-panneau web | D-045 |
| applets simultanées (proposée) | pool fixe de `MAX_INSTANCES` instances (deux essayées d'abord), `AppId` = espèce ; ports de sortie routés vers `A` à `D` sans recouvrement, ou désactivés ; carte d'entrées par instance ; une `Voice` et un `Setup` par instance ; focus au panneau (`up` / `down` court), page de routage (appui long), indicateur ; protocole 9 par instance | D-046 |
| applet Supersaw | sept scies PolyBLEP en `f32` (phases entières), désaccord et mix de Szabo, passe-haut sur la fondamentale ; banc dans l'app (générateur `SupersawBank`) partagé par les sorties : A / B mono ou stéréo, C sub-octave ; entrées assignables par `oc_core::modulation` sauf `STEREO`, CV1 en V/oct sur `NOTE` au démarrage ; ×4 non nécessaire (repliement mesuré) ; forme de la scie au carré (`SHAPE`) ; scies descendantes, `LEVEL` à 3 V au plus | D-037, D-044 |
| budget mémoire | disposition de `teensy4-bsp`, reprise dans le `build.rs` d'`oc-firmware` (sans la feature `rt`) : code en ITCM, bancs 6 / 10 (192 + 320 Kio) ; coût d'une applet par différence de `xtask size` ; budgets lus dans l'ELF (`__flexram_config`), alerte sous 16 Kio d'ITCM libres ; code froid (USB, sonde, réglages) en `opt-level = "s"` ; ITCM à court : bancs repartagés, puis code en flash ; tampon de mesure : feature de test, `.uninit` en OCRAM ; arena dans `oc-core` au premier grand usage d'applet ; tampons du DMA en DTCM ; profondeur de la pile estimée par `cargo xtask size` (`stack-depth` : `FAIL` au-delà de `.stack`, `warn` sous 2 Kio) | D-038 |
