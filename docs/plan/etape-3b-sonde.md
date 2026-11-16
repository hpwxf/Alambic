# Étape 3, lot 3b — Sonde live et mesures sur module (plan du 2026-11-16)

Demande de l'utilisateur : finir l'étape 3 ; au passage, une interface de diagnostic plus élaborée que le journal
USB, dans les deux sens, utile aussi au profilage (coût de l'affichage), sous la forme d'un outil du projet.
Décision : [D-029](../../journal/D-029-live-probe.md) (**proposée**, adoptée lot par lot).

Choix de l'utilisateur : deux ports CDC (journal texte inchangé + canal binaire), sonde toujours compilée,
périmètre complet en première version, crates `oc-protocol` + `oc-probe`.

La mesure du lot 3b (coût d'une trame OLED et d'une page, gigue de la boucle) se fait avec la sonde, au cycle près,
au lieu du protocole GPIO + oscilloscope ; l'oscilloscope reste un recoupement possible, plus un prérequis.

## Lots

| Lot | Contenu | Critère d'acceptation |
|---|---|---|
| **3b1** — profilage dans le cœur | `oc_core::profile` : étapes nommées, statistiques (min / max / moyenne / nombre), histogramme de la période de boucle ; `Clock::fine_count` / `fine_per_micro` par défaut ; l'`Engine` mesure tick, acquisition, applet, sorties, rendu, envoi ; le simulateur l'affiche | tests `oc-core` sur horloge simulée ; définition de « fait » |
| **3b2** — protocole | crate `oc-protocol` (`no_std`) : messages hôte → module (hello, profil, remise à zéro, flux, sortie forcée, capture, redémarrage) et module → hôte (hello, profil, échantillons, écran, erreur) ; encadrement `postcard` COBS ; version du protocole | tests d'aller-retour et de trames corrompues |
| **3b3** — firmware | `usb-device` composite à deux ports CDC (journal `log` maison sur le premier, `imxrt-log` retiré) ; serveur du protocole sondé par la boucle ; `CYCCNT` dans `Clock` ; envoi OLED mesuré page par page ; commandes servies par l'`Engine` (sortie forcée, framebuffer) ; redémarrage en mode chargement vérifié sur les sources Teensy 4 | sur module : deux ports visibles sous macOS, journal inchangé ; coût du sondage mesuré |
| **3b4** — outil hôte | crate `oc-probe` : `stats`, `stream`, `out`, `screen` (PNG ou texte), `reboot` ; `cargo xtask flash` peut demander le redémarrage | tests de l'outil sur un faux port ; essai sur module |
| **3b5** — mesure | niveau 9 de `TESTING.md` (CV, triggers) aidé de la sonde ; coût d'une page, d'une trame, gigue de la boucle consignés dans `avancement/etape-3.md` | D-029 adoptée (révisée si besoin) ; chiffres utilisables pour trancher D-023 au lot 3c |

Ordre : 3b1 → 3b2 → 3b3 → 3b4 → 3b5 ; 3b1 et 3b2 sont indépendants l'un de l'autre.

## Risques

1. Deux ports CDC : 6 points de terminaison sur 8 ; à confirmer sur l'USB du RT1062 et sous macOS (3b3).
2. Redémarrage en mode chargement : mécanisme de Teensyduino à lire avant d'écrire du code ; s'il n'est pas
   documenté, la commande est retirée (D-029 § 5).
3. Le sondage USB dans la boucle de 1 ms : son coût est mesuré par la sonde elle-même (étape « USB »).
