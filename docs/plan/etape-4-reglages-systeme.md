# Étape 4 — Réglages système, calibration, persistance (plan du 2026-11-16)

Demande de l'utilisateur : un menu de réglages système utilisable sur le module, avec une fonction de calibration,
et une persistance pour que les réglages servent. Cible : **O&C sur Teensy 4.0 uniquement** (4.1 plus tard).
Consigne : pas de schéma exotique ; coller à **Phazerville** pour rester familier à ses utilisateurs. Aucune mesure
passée n'est reprise (les zéros du stash `stash@{0}`, base `0b0fe3b`, ne sont pas retenus).

Plan accepté globalement par l'utilisateur le 2026-11-16. Décisions :
[D-026](../../journal/D-026-system-settings-screen.md) (écran et accès),
[D-027](../../journal/D-027-calibration-in-core.md) (calibration),
[D-028](../../journal/D-028-settings-persistence.md) (persistance) — **proposées**, adoptées lot par lot.

Rapport à l'étape 3 : la partie « calibration » du lot 3b est remplacée par cette étape ; le reste de 3b (coût OLED,
gigue) et 3c restent à faire ensuite.

## Lots

| Lot | Contenu | Critère d'acceptation |
|---|---|---|
| **4a** — conversion dans le cœur | traits en codes bruts (D-027 A) ; calibration dans l'`Engine` : entrées (zéro par canal, gain commun), sorties (table de 11 points par octave et par canal) ; une calibration d'usine partagée, `Calibration::FACTORY` (zéro 2730 et table DAC dérivés de `OC_calibration.cpp`, sourcés ; le simulateur et Rack modélisent le convertisseur du module avec elle — révision de D-027) ; `cv_in.rs` et `Dac8565` ne convertissent plus | tests `oc-core` de conversion et d'interpolation ; écrans dorés relus ; définition de « fait » |
| **4b** — modèle et stockage | `oc_core::settings` (calibration, applet de démarrage, contraste, rotation, décalage d'écran, sens des encodeurs) ; encodage et stockage (D-028 S ou M) ; chargement au démarrage ; simulateur `--settings` ; ABI Rack + JSON du patch | aller-retour, données corrompues ou absentes ⇒ valeurs d'usine ; Rack : réglages restaurés à la réouverture d'un patch (vérif. manuelle) |
| **4c** — écran système | `Screen::System` ; item `SETUP` du menu ; gestes au démarrage (encodeur gauche ⇒ calibration ; `up` + `down` ⇒ secours), rappelés à l'écran ; réglages hors calibration ; sauvegarde ; rotation dans le cœur, contraste et décalage côté `Display` | scénarios `.scn` (navigation, sauvegarde, secours) ; essai sur module |
| **4d** — assistant de calibration | étapes Phazerville : « Start fresh? », DAC A–D (point bas, point haut, interpolation, affinage), zéros ADC (capture + ajustement), gain ADC sur CV1 (1 V, 3 V par rebouclage), « Save values? » | tests de la machine à états sur `mock_engine` ; scénario simulé complet |
| **4e** — flash du module | `flash_store.rs` : `NorFlash` via l'API ROM (vérifiée au manuel de référence) ; zone `0x601F_0000`–`0x601F_3FFF` ; `xtask size` nomme la zone | sur module : sauver, couper, relire ; 100 sauvegardes sans erreur ; image ≤ 1 984 Kio |
| **4f** — mesure | calibration réelle du module TLM ; écarts consignés dans `avancement/etape-4.md` ; seuil de présence D-024 revu avec le zéro corrigé | ADR passées à `adoptée` (révisées si besoin) ; `CHANGELOG.md` ; version 0.3.0 |

Ordre : 4a → 4b → 4c → 4d ; 4e peut commencer après 4b ; 4f en dernier.

## Restant à confirmer

1. D-027 : frontière en codes bruts (A, proposée) ou maintien des millivolts + `set_calibration` (B).
2. D-028 : briques standard (`postcard` + `sequential-storage` sur `embedded-storage`, proposées) ou format et
   journal maison ; API ROM (proposée) ou commandes FlexSPI façon Teensyduino.
