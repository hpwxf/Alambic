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
| `docs/hardware.md` | le matériel du module : principes, ce que disent les fiches, ce module et ses mesures ; glossaire |
| `docs/plan/rapport-etape-7-chemin-audio.md` | rapport détaillé de l'étape 7 : essais, échecs, analyses et correctifs, du DAC à 1 kHz au DAC en DMA |
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
| **7** — chemin audio (2026-11-17 → 11-18) | ✅ `oc_core::audio` (paramètres de sortie, voix), applet `TONE` ; DAC en PCS0 à 22 MHz ; PIT → DMA → LPSPI4 à **47 619 Hz** (instant fixé par le matériel), interruption du DMA pour l'échantillon et l'écran par tronçons ; `audio` 3,4 µs sur 21 ; juste à l'accordeur jusqu'à B7 (D-033). Réserves : 48 kHz impossible (quartz de GPT2 muet), quatre voies écrites à chaque période, Rack non écouté. Version **0.5.0** (avec l'étape 6 ; plugin Rack 2.5.0) | [etape-7.md](docs/plan/avancement/etape-7.md), [rapport](docs/plan/rapport-etape-7-chemin-audio.md) |
| **8** — premier oscillateur : Supersaw (2026-11-18) | ✅ `oc_core::audio::supersaw` : sept scies PolyBLEP en entier (désaccord et mix de Szabo, passe-haut sur la fondamentale), A / B mono ou stéréo, C sub-octave ; applet `SUPERSAW`, entrées par `oc_core::modulation` ; `audio` 3,9 µs sur 21 ; repliement mesuré par FFT (exemple `aliasing`) : sous 10 kHz ≤ −45 dB, ×4 non nécessaire ; scies descendantes, sans écrêtage jusqu'à 2 V (D-037) ; budget mémoire (D-038). Réserves : bruit de CV1 sur `NOTE`, assignations non sauvegardées, Rack non écouté. Version **0.6.0** (plugin Rack 2.6.0) | [etape-8.md](docs/plan/avancement/etape-8.md) |
| **5** — primitives graphiques et applet de démo (2026-11-17) | ✅ texte par colonnes (Diagnostic 646 → 88 µs), six polices en paramètre ; `oc_core::gfx` (bases en octets, contrôles, courbes, motifs) ; applet `GFX DEMO` ; `embedded-graphics` en dépendance de test (D-030). Version **0.4.0** (avec l'étape 3 ; plugin Rack 2.4.0) | [etape-5.md](docs/plan/avancement/etape-5.md) |

### Étape 9 — essai du Supersaw en `f32` (en cours)

Demandée le 2026-11-19 (D-039) : le Cortex-M7 a un FPU, le Supersaw est en virgule fixe sans que le choix ait été
mesuré. Choix de l'utilisateur : les deux versions embarquées dans l'applet, choisies par un réglage.

| Lot | Contenu | État |
|---|---|---|
| **9a** — banc `f32` et réglage | `audio/supersaw/float.rs` : phases dans `[0, 1)`, PolyBLEP par `1/dt` préparé, passe-haut et sommes en `f32`, millivolts arrondis en sortie ; préparation reprise du banc entier (courbe de désaccord, gains, normaliseurs), coefficient du passe-haut en `f32` ; `Supersaw::arithmetic` (`Fixed` / `Float`), les deux bancs suivent chaque changement, seul le choisi tourne ; ligne `CALC INT` / `F32` dans l'applet (sans entrée), éventail des voix sur une rangée ; tests : écart au banc entier < 2 mV RMS (mesuré < 1), hauteur, passe-haut, silence, bascule ; bench criterion `supersaw_f32_sample_to_codes` ; `SUPERSAW_CALC=f32` pour l'exemple `aliasing` ; `.text` +2,4 Kio ; sur l'hôte : criterion 31,6 ns (`INT`) contre 35,5 ns (`F32`) par échantillon ; repliement identique à quelques dixièmes de dB ; écart à la supersaw idéale plus grand en `F32` aux notes basses (C2 : −39,2 → −28,5 dB sous 10 kHz, identique dès C5), la phase en `f32` qui dérive (F1 de D-039, piste F2) | ✅ |
| **9c** — deux pages | `STEREO` et `CALC` en page 2, ascenseur à gauche des réglages ; l'ascenseur de `RATCHET` sorti dans `gfx::controls::scroll_bar` (commit à part, écrans de `RATCHET` inchangés) ; éventail de nouveau sur deux rangées ; scénario doré `supersaw_page2` (révisions de D-030 et D-037) | ✅ |
| **9d** — voix préparée dans la boucle | `oc_core::audio::Setup` (pas des tons, `supersaw::Setup` des deux bancs) : fonction pure `Setup::new`, `Voice::apply` ne fait que copier, `set_params` = `apply(Setup::new(..))` (écrans dorés inchangés) ; le firmware calcule le `Setup` dans `audio::publish`, hors section critique, l'interruption l'applique ; tests : mêmes échantillons par `apply` et par `set_params` à travers des changements de note et de banc ; `.data` +312 o (la boîte aux lettres) ; D-040, révision de D-033. Reste : `voice_params` sur le module, cas C (attendu : une copie, bien sous la microseconde) | ✅ |
| **9e** — une arithmétique par sous-module | `audio/supersaw.rs` ne garde que le commun (paramètres, parts, courbes de Szabo, préparation partagée en entier : pas, gains, normaliseurs ; aiguillage `Setup` / `Bank`) ; le banc entier passe dans `audio/supersaw/fixed.rs`, symétrique de `float.rs` (`Setup` et `Bank` à lui) ; échantillons identiques au bit près (écrans dorés et tests inchangés) ; `.text` −212 o | ✅ |
| **9f** — gains par voix, référence naïve | `supersaw::voice_gains(mix)` : le gain de chaque voix (loi du centre pour `CENTRE`, des côtés pour les six autres), seule source de la répartition pour les deux bancs (pondération voix par voix, normaliseurs tirés des mêmes gains), l'écran de l'applet et les deux références de l'exemple `aliasing` ; banc entier identique au bit près, banc `f32` inchangé à l'arrondi près (colonnes `blep` d'`aliasing` identiques). Les rampes naïves d'`aliasing` partent des phases du banc au lieu de 0 : en phase entre elles, elles faussaient la référence ; sous 10 kHz, PolyBLEP est maintenant 30 à 35 dB sous elles (18 à 38 dB avec l'ancienne référence ; les « 13 à 35 dB » du lot 8c′ venaient d'elle) | ✅ |
| **9b** — module | relevés du 2026-11-19 (TESTING 9.3g), `voice` en moyenne / au plus : `INT` 1,16–1,19 / 1,22–1,35 µs, `F32` 1,06–1,07 / 1,11–1,14 µs, de A2 à C8 / 100 %, CV sur `NOTE` ou non : **`F32` ≈ 8 % moins cher**, et plus régulier ; `voice_params` 0,30 µs (la copie du `Setup`, D-040) ; `audio` 4,0 / 5,0 µs dans tous les cas, le pic de 11 µs disparu, trame revenue à 1 345 µs. `audio` mesure la rafale du DAC, d'où les étapes `voice` et `voice_params` (protocole de sonde v3). Reste : trancher D-039 | en cours |

### Étape 8 — premier oscillateur : Supersaw (terminée)

Demandée le 2026-11-18 (D-037) : l'applet de l'étude `docs/supersaw/` sur le chemin audio. Choix de l'utilisateur : A
et B stéréo, C sub-octave ; note, désaccord et mix ; PolyBLEP d'abord, ×4 si le repliement s'entend. Plan :
[etape-8-supersaw.md](docs/plan/etape-8-supersaw.md).

| Lot | Contenu | État |
|---|---|---|
| **8a** — calcul | `oc_core::audio::supersaw` : scie PolyBLEP entière, désaccord et mix de Szabo, passe-haut sur la fondamentale, parts `Mono` / `Left` / `Right` / `Sub` ; banc partagé dans `Voice` ; alias sous 10 kHz ≥ 25 dB sous la scie naïve (≈ 35 dB estimés), banc criterion 36 ns / échantillon sur l'hôte (`TONE` 20 ns) | ✅ |
| **8b** — applet | `SupersawApp` (« SUPERSAW ») : `NOTE`, `DETUNE`, `MIX`, `LEVEL`, `STEREO` ; CV1 V/oct, CV2, CV3 (+20 %/V, valeur jouée affichée `60>80%`) ; menu à six applets, `SETUP` sans ligne vide ; écran : réglages et les sept voix (écart, gain) ; scénario doré `supersaw` | ✅ |
| **8b′** — entrées assignables | `NOTE`, `DETUNE`, `MIX`, `LEVEL` prennent leur entrée par `oc_core::modulation` (clic de l'encodeur droit, comme `RATCHET`) ; `NOTE` prend CV1 en `ADD` au démarrage, les autres rien : le bruit d'une entrée débranchée (≈ ±5 codes, mesuré) ne bouge plus l'écran ; `SET` de la note depuis C4, `MUL` du niveau en VCA ; révision de D-037 ; `tests/supersaw_inputs.rs`, scénario doré `supersaw_inputs` ; `.text` +3,5 Kio ; la ligne ne montre plus que la valeur du panneau (plus de `60>80%`), comme `RATCHET` : la valeur jouée est dans le sous-menu | ✅ |
| **8c** — module | profil `audio` relevé (3,9 µs en moyenne, 10,2 au plus, A2 comme C8 à 100 % : marge large) ; son et `STEREO` corrects ; écran stable à 100 % de désaccord une fois les entrées assignables (le gigotement venait bien du bruit des CV) ; entrées assignables essayées sur le module (TESTING 9.3f, étape 4) ; ×4 tranché par 8c′ | ✅ |
| **8c′** — repliement mesuré | `cargo run -p oc-core --example aliasing --release` : le banc réel à 47 619 Hz, en mV, FFT de 65 536 points (Blackman-Harris), harmoniques de chaque voix tirées de son pas exact ; à 1 V, de C2 à C8 et 0 / 50 / 100 % de désaccord : sous 10 kHz, repli ≤ −45 dB partout (≤ −57 dB jusqu'à C5), 30 à 35 dB sous les rampes naïves parties des mêmes phases ; raies repliées les plus fortes entre 20 et 23,8 kHz (−22 dBc à C8), donc hors de l'audible ; **×4 non nécessaire**. Constat à part : au-delà d'environ 1,1 V de `LEVEL`, la somme des sept voix touche la borne de −3 V et est écrêtée (3 % des échantillons à 3 V, mix 50 %) | ✅ |
| **8c″** — polarité inversée | les parts sont des scies descendantes : le passe-haut fait de chaque saut une pointe, qui va maintenant vers +6 V (6 V de marge) au lieu de −3 V (3 V) ; niveau sans écrêtage sur tous les cas de 1,00 à **2,01 V** (+6 dB), 0,05 % d'échantillons écrêtés à 3 V (2,9 % avant) ; spectre inchangé ; `LEVEL` garde 3 V au plus, l'utilisateur ayant jugé à l'écoute le pire cas d'avant très correct (révision de D-037) ; flashé et écouté sur le module : correct | ✅ |
| **8d** — ×4 | suréchantillonnage et décimation, si 8c le demande | non nécessaire (8c′) |

### Étape 7 — chemin audio : DAC à 47,6 kHz par DMA (terminée)

Demandée le 2026-11-17 (D-033) : générer des sons sur les sorties. Choix de l'utilisateur : 48 kHz, les quatre
sorties, une applet `TONE` de test d'abord. Plan : [etape-7-chemin-audio.md](docs/plan/etape-7-chemin-audio.md).

| Lot | Contenu | État |
|---|---|---|
| **7a** — DAC rapide, encore à 1 kHz | `DAC_CS` en PCS0, OLED sur PCS3, DAC à 22 MHz, OLED à 22 ou 8 MHz (feature `oled-spi-8mhz`), voies écrites au changement (révision de D-010) ; mesuré sur le module : écran propre à 22 MHz, sorties justes, `outputs` 3,0 µs en moyenne (≈ 16 µs avant), page d'OLED 49 µs (≈ 145 µs) | ✅ |
| **7b** — cœur audio | `oc_core::audio` : paramètres de sortie (niveau tenu ou tonalité), `Voice` à phases de 32 bits, sinus en table calculée à la compilation ; le moteur rend sa voix à 1 kHz ; applet `TONE` (A sinus, B triangle, C scie, D carré, note C1–C8, niveau 0–3 V, CV1 en V/oct) ; Rack à sa fréquence d'échantillonnage (`oc_engine_audio`, compilé, pas encore écouté) ; menu sans ligne vide sous le titre ; révision de D-033 | ✅ |
| **7c** — interruption à 48 kHz | GPT2 sur le quartz (le PIT est à 1 MHz) ; l'interruption possède le bus : mots du DAC qui changent, échantillon suivant, tronçon d'OLED (`FrameStream`, 24 octets à 22 MHz) ; boîtes aux lettres sous section critique ; profil `audio`, `audio_period`, `frame` ; sorties au repos pendant SAVE ; révisions de D-008, D-009, D-010, D-033 ; sur le module : le quartz ne démarre pas, repli PERCLK / 21 = 47 619 Hz, fin d'envoi lue dans l'écho reçu (l'OLED prenait des bits du DAC dans `TONE`) ; `audio` 12,5 µs sur 21, gigue ±3 µs de l'écriture du DAC, écran stable, son présent | ✅ |
| **7d** — module | `TONE` juste et stable de C1 à C6 à l'accordeur, perturbé autour de C7 (gigue ±3 µs, images), C8 lu une octave trop bas (limite de l'accordeur probable) : décide le DMA | ✅ |
| **7e** — DMA | le PIT déclenche une copie DMA de quatre mots du DAC (DMAMUX `ENBL\|TRIG\|A_ON`) ; l'interruption du DMA calcule l'échantillon deux emplacements plus loin et envoie l'écran ; plus aucune lecture du PIT par période ; sur le module : `audio` 3,4 µs (12,5 avant), écran et son corrects ; accordeur juste jusqu'à B7, C8 en limite de l'accordeur ; auto-test de 50 ms et rapport à 3 s dans le journal | ✅ |

### Étape 6 — premier applet musical : Ratchet (en cours)

Demandée le 2026-11-17 (D-032) : rafales de triggers sur B après une copie sur A, à chaque front de TR1. Plan :
[etape-6-ratchet.md](docs/plan/etape-6-ratchet.md). Une seconde applet reste à décrire.

| Lot | Contenu | État |
|---|---|---|
| **6a** — calcul | ✅ `oc_core::fixed` (`log2`, `exp2` en 16.16) ; `ratchet::burst` (`SHAPE` signé, placements `FIRST` / `SPAN`, entrées, bornes) ; `ratchet::tempo` | fait |
| **6b** — applet | ✅ `RatchetApp`, `AppId::Ratchet` (« RATCHET ») : A copie, B rafale, `PASSTHRU`, `PROB`, `SYNC`, entrée au choix par réglage (`CVn=` / `CVn+`), `up` à la main ; tests tick par tick | fait |
| **6c** — écran | ✅ deux pages et leur curseur, entrées, BPM, témoins, frise avec repère du prochain trigger ; scénarios dorés | fait |
| **6d** — module | essayé par l'utilisateur, sur le chemin audio : correct ; `MODE FIRST` et `SPAN` gardés tous les deux pour l'instant | ✅ |
| **6e** — écran et entrées | ascenseur à gauche des réglages, frise plus basse qui suit la rafale en cours (révision de D-032) ; sous-menu d'assignation des entrées par réglage : entrée, lecture et bornes vues, `SCALE`, `SHIFT`, `MIN` / `MAX`, effet `SET` / `ADD` / `MUL` (D-035) ; tests et écrans dorés. Reste : essai sur le module (TESTING 9.3c) | en cours |
| **6f** — modulation partagée | la mécanique du sous-menu sortie dans `oc_core::modulation` (`Assign`, `Effect`, `AssignEditor`, `Param`), `RATCHET` branchée dessus sans changement de comportement (écrans dorés identiques) ; `tests/modulation.rs` (D-036) | ✅ |

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
| signal | millivolts `i32` ; `f32` seulement dans le C++ du module Rack ; à l'essai : un banc Supersaw en `f32`, millivolts aux bords (D-039, proposée) | D-005, D-039 |
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
| applet Ratchet | A copie de TR1, B rafale ; nombre ou gate, `SHAPE` signé (accélère / ralentit), placement `FIRST` ou `SPAN`, `PROB`, `PASSTHRU`, `SYNC` sur le tempo de TR1 ; triggers 5 V / 5 ms ; entrée par réglage dans un sous-menu : `SCALE`, `SHIFT`, `MIN` / `MAX`, effet `SET` / `ADD` / `MUL` | D-032, D-035 |
| modulation par une entrée CV | `oc_core::modulation` partagé entre applets : `Assign` (entrée, `SCALE`, `SHIFT`, `MIN` / `MAX`, effet), `AssignEditor` + `Param` ; l'applet fournit la plage `SET` de chaque paramètre et applique l'effet | D-036 |
| publication | à chaque étiquette `v*` : release GitHub avec les images du firmware (ELF et HEX SH1106, HEX SSD1306 / SSD1309, SHA-256) ; page web sur GitHub Pages | D-034 |
| chemin audio | interruption PIT à 48 kHz, priorité la plus haute ; les quatre sorties ; `DAC_CS` en PCS0 matériel, DAC à 22 MHz (plafond 25 MHz en 3,3 V) ; l'interruption possède le bus et cède des tronçons à l'OLED ; voix séparée du contrôle, préparée dans la boucle (`Setup`), copiée par l'interruption ; applet `TONE` ; DMA seulement si la mesure le demande | D-033, D-040 |
| applet Supersaw | sept scies PolyBLEP en entier, désaccord et mix de Szabo, passe-haut sur la fondamentale ; banc partagé dans la voix : A / B mono ou stéréo, C sub-octave ; entrées assignables par `oc_core::modulation` sauf `STEREO`, CV1 en V/oct sur `NOTE` au démarrage ; ×4 non nécessaire (repliement mesuré) ; scies descendantes, `LEVEL` à 3 V au plus | D-037 |
| budget mémoire | disposition de `teensy4-bsp` : code en ITCM, bancs 6 / 10 (192 + 320 Kio) ; coût d'une applet par différence de `xtask size` ; ITCM à court : bancs repartagés, puis code en flash ; tampon de mesure : feature de test, `.uninit` en OCRAM ; arena dans `oc-core` au premier grand usage d'applet ; tampons du DMA en DTCM | D-038 |
