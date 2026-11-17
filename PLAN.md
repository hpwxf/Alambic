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
| `docs/plan/panique-persistante.md` | préparé, non décidé : garder le message d'une panique à travers un redémarrage (D-018) |
| `docs/plan/etape-5-gfx.md` | plan de l'étape 5 : primitives graphiques et applet de démo (D-030) |
| `docs/plan/etape-3c-cadence.md` | plan du lot 3c (D-023 option C+) |
| `docs/plan/etape-3-plan-a-isr-16khz.md` | plan A de D-023 (ISR 16 666 Hz), proposé par Junie, non adopté ; bandeau des points caducs |
| `docs/plan/avancement/etape-*.md` | journal d'une étape terminée : fait, mesuré, corrigé |
| `.junie/plans/oc-rust-firmware-foundation.md` | plan détaillé de l'étape 0 (fondation), rédigé par l'assistant Junie, terminé |
| `docs/supersaw/` | étude : supersaw, repliement, PolyBLEP, architecture audio et partage du bus SPI (base de D-023) |
| `AGENTS.md` | architecture et pièges du dépôt (anglais) — référence technique, lue avant tout changement structurel |
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

### Étape 5 — primitives graphiques et applet de démonstration (terminée)

Demandée le 2026-11-17 (D-030) : primitives tirées des écrans de Squares-and-Circles, applet de démo à quatre
pages. Plan : [etape-5-gfx.md](docs/plan/etape-5-gfx.md).

| Lot | Contenu | État |
|---|---|---|
| **5a** — bases | ✅ `oc_core::gfx` : rectangles, lignes droites, zone inversée en octets (mêmes pixels qu'`embedded-graphics`) ; ligne, cercle, disque, pointillés ; `Ink::{Lit, Dark, Invert}` ; sinus entier | fait |
| **5b** — contrôles et motifs | ✅ `gfx::controls` (potentiomètre, sélecteur, valeur encadrée, légende, barres, bandeau), `gfx::plots` (`Area`, trace, min / max, histogramme, échelle à aiguille), `gfx::patterns` (cases, points, anneau euclidien, mini-clavier) ; `Ratio` ; tests par propriétés | fait |
| **5c** — applet de démo | ✅ `AppId::Gfx` (« GFX DEMO »), cinq pages (polices, potentiomètres, courbes, séquenceur, mesures), sorties simulées, écrans dorés par page, banc par page | fait |
| **5d** — mesure | ✅ courbes 105 µs de dessin, périodes inchangées ; pages validées à l'œil ; banc `graphics.rs` et rapports HTML | fait |

### Étape 3 — assainissement, puis choix de la cadence (terminée)

Ouverte par la revue du 2026-11-16 ([revue-decisions-2026-11-16.md](docs/plan/revue-decisions-2026-11-16.md)).
D-023 est **adoptée en option C+** (2026-11-17) : 1 kHz sans trous ; A et B restent des suites.

| Lot | Contenu | État |
|---|---|---|
| **3a** — dette de revue | ✅ CI verte avec `--all-features` (R1, D-013) ; ✅ triggers sans anti-rebond mécanique (R2, D-019) ; ✅ règle du brochage rendue vraie et testée (R3, D-007) ; ✅ docs « jamais sur matériel » / « jamais dans Rack » corrigées (R4, D-011, D-015) ; ✅ `flash --dry-run` en CI (V5, D-016) ; ✅ plus de présence de câble, backends alignés (R5, D-019) ; ✅ plan de cadence suivi par git, conflit avec l'étude audio consigné, OLED par pages proposé (R6, D-023) | fait — version **0.2.0** (plugin Rack 2.2.0, `CHANGELOG.md`) |
| **3b** — sonde et mesure sur module | ✅ sonde live (D-029 adoptée, plan : [etape-3b-sonde.md](docs/plan/etape-3b-sonde.md)) : profilage dans le cœur (`CYCCNT`), protocole `oc-protocol`, deux ports CDC et redémarrage par 134 bauds, outil `oc-probe` (`stats`, `stream`, `out`, `inputs`, `triggers`, `sweep`, `screen`, `reboot`) ; boucle recalée (≈ 775 Hz depuis l'étape 1) ; niveau 9 CV, triggers, rebouclage. Mesures indicatives : trame OLED ≈ 1,16 ms (page ≈ 145 µs), rendu ≈ 0,63 ms, ADC ≈ 0,29 ms, ≈ 947 ticks/s ([etape-3.md](docs/plan/avancement/etape-3.md)) | fait |
| **3c** — cadence | ✅ D-023 option C+ (plan : [etape-3c-cadence.md](docs/plan/etape-3c-cadence.md)) : 3c1 fronts des triggers capturés par le détecteur des GPIO ; 3c2 écran envoyé une page par tick ; 3c3 `signal_tick` / `ui_tick` **reporté à l'option A** ; 3c4 mesures et clôture | fait |

### Étape 4 — réglages système, calibration, persistance (terminée)

Demandée le 2026-11-16 : menu de réglages utilisable sur le module, assistant de calibration, persistance ; Teensy 4.0
seul ; interaction et modèle de calibration repris de Phazerville. Plan détaillé :
[etape-4-reglages-systeme.md](docs/plan/etape-4-reglages-systeme.md). Les lots 3b et 3c reprennent après l'étape 4.

| Lot | Contenu | État |
|---|---|---|
| **4a** — conversion dans le cœur | ✅ traits en codes bruts (`read_code` / `write_code`) ; `Engine` convertit avec sa `Calibration` (entrées affines par canal, sorties en table de 11 points) ; `Calibration::FACTORY` de Phazerville partagée par les trois backends (révision de D-027) ; `board.rs` sans calibration | fait |
| **4b** — modèle et stockage | ✅ `oc_core::settings` (`Settings`, `SettingsStore` sur `NorFlash`, `encode`/`decode`), `Engine::apply_settings` ; simulateur `run --settings` ; ABI Rack + JSON du patch (C++ non compilé ici : pas de Rack SDK) ; firmware sans persistance jusqu'à 4e | fait |
| **4c** — écran système | ✅ `oc_core::system` (`Screen::System`), item `SETUP`, gestes au démarrage (encodeur gauche, `up` + `down` = secours), brouillon appliqué en direct, `SAVE` / `EXIT` ; rotation et décalage dans le cœur, `Display::set_contrast` ; demande de sauvegarde consommée par les backends (révision de D-026) | fait |
| **4d** — assistant de calibration | ✅ `oc_core::wizard` : 13 étapes Phazerville (fresh, DAC A–D à −3 V / +5 V, zéros ADC, gain CV1 à 1 V / 3 V, save), moyennes après stabilisation, fenêtres d'acceptation ; test de bout en bout par rebouclage (révision de D-027) | fait |
| **4e** — flash du module | ✅ `flash_store.rs` : `NorFlash` par l'API ROM du RT1062 (vérifiée sur les sources NXP), lectures par la ROM, zone `0x601F_0000`–`0x601F_3FFF` ; `xtask` refuse une image qui l'atteint ; chargement au démarrage, sauvegarde sur demande (révision de D-028) ; vérifié sur module | fait |
| **4f** — mesure | ✅ calibration du module TLM, réglages relus après coupure ; firmware d'essai `settings-stress` : 300/300, coupures sans échec ; seuil D-024 à 50 mV ; version 0.3.0 | fait |

## Décisions actées

Valeurs seules ; le raisonnement est dans `journal/`.

| Sujet | Valeur | ADR |
|---|---|---|
| conventions | `PLAN.md` index, `docs/plan/`, journal d'ADR, docs de travail en français, code et commits en anglais | D-001 |
| cœur | `oc-core` `no_std`, `forbid(unsafe_code)`, six traits de plateforme, `Engine::tick` seul point d'appel | D-002 |
| crates | `oc-core`, `oc-drivers`, `oc-firmware` (hors `default-members`), `oc-sim`, `oc-vcv-ffi`, `oc-buildinfo` (dépendance de build), `xtask` ; `oc-image`, `oc-halfkay`, `oc-web` (outils web) | D-003, D-025, D-031 |
| chaîne croisée | `thumbv7em-none-eabihf`, `t4link.x` généré par `imxrt-rt`, pas de `memory.x` ni `flip-link`, outils LLVM du sysroot, jamais `RUSTFLAGS` | D-004 |
| signal | millivolts `i32` ; `f32` seulement dans le C++ du module Rack | D-005 |
| calibration | pente signée (négative : étages inverseurs) ; dans `oc-core`, codes bruts à la frontière ; entrées : zéro par canal (≈ 2730), gain commun ; sorties : table de 11 points par octave ; mesurée par assistant, persistée | D-006, D-027 |
| brochage | table unique et réservation des broches dans `board.rs` (`split` + groupes par signal, test `xtask/tests/pin_rule.rs`), dérivée de `OC_gpio.cpp` ; pull-up 22 kΩ + hystérésis sur les entrées numériques | D-007 |
| exécution | boucle coopérative 1 kHz, sans interruption ; rendu tous les 20 ticks (≈ 50 Hz) ; option C+ : fronts des triggers capturés, écran envoyé une page par tick ; A (ISR 16 kHz) et B (audio) en suites | D-008, D-023 |
| écran | framebuffer 1 024 octets dans l'ordre du contrôleur ; pilote `ssd130x` maison ; SH1106 par défaut, `ssd1306` / `ssd1309` en features | D-009 |
| bus SPI | DAC8565 + OLED partagés, `SharedBus` sur `RefCell`, CS en GPIO, 8 MHz, tronçons ≤ 512 octets ; une page OLED par tick | D-010, D-009 |
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
| applets | `AppHost` (tous résidents), menu par accord haut + bas, haut / bas au relâchement ; `DiagnosticApp`, `ScopeApp` | D-021 |
| licence | `MIT OR Apache-2.0` (workspace) | D-022 |
| présence d'entrée | déduite dans `oc-core` (activité récente + maintien) : CV 50 mV / 300 ms, trigger verrou ou horloge 2 s ; jamais fournie par un backend | D-024 |
| identifiant de build | hash court + `-dirty` sous la version, écran de démarrage des trois backends ; calculé par `oc-buildinfo` (vergen-gix, sans binaire `git`), jamais dans `oc-core` | D-025 |
| réglages système | écran `oc-core` hors applets ; gestes au démarrage (encodeur gauche ⇒ calibration, `up` + `down` ⇒ secours) et item `SETUP` ; interaction de Phazerville | D-026 |
| persistance | `postcard` + `sequential-storage` sur `embedded-storage` ; flash Teensy 4.0 par l'API ROM (`unsafe` confiné) ; fichier (sim) ; JSON du patch (Rack) | D-028 |
| sonde live | profilage dans `oc-core` (DWT sur le module), second port USB CDC binaire `postcard` COBS, crates `oc-protocol` et `oc-probe`, toujours compilée ; un seul port encadré si l'USB MIDI arrive | D-029 |
| graphisme | `oc_core::gfx` : bases en octets entiers, contrôles, courbes, motifs ; trigonométrie entière ; applet de démo dans le menu | D-030 |
| outils web | page statique Chromium : sonde par Web Serial, flash par WebHID depuis l'ELF validé ou un HEX (vérification réduite) ; cœur wasm sans E/S (`oc-image`, `oc-halfkay`, `oc-web`), `cargo xtask web` ; essayé sur le module (sonde et flash) | D-031 |
