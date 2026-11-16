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
| **3b1** — profilage dans le cœur | `oc_core::profile` : étapes nommées, statistiques (min / max / moyenne / nombre), histogramme de la période de boucle ; `Clock::fine_count` / `fine_per_micro` par défaut ; l'`Engine` mesure tick, acquisition, applet, sorties, rendu, envoi ; le firmware écrit le profil sur le journal USB (fait ; le simulateur n'a pas de compteur fin réel, révision de D-029) | tests `oc-core` sur horloge simulée ; définition de « fait » |
| **3b2** — protocole | crate `oc-protocol` (`no_std`) : messages hôte → module (hello, profil, remise à zéro, flux, sortie forcée, capture, redémarrage) et module → hôte (hello, profil, échantillons, écran, erreur) ; encadrement `postcard` COBS ; version du protocole | tests d'aller-retour et de trames corrompues |
| **3b3** — firmware | `usb-device` composite à deux ports CDC (journal `log` maison sur le premier, `imxrt-log` retiré) ; serveur du protocole sondé par la boucle ; envoi OLED mesuré page par page ; commandes servies par l'`Engine` (sortie forcée, framebuffer) ; redémarrage en mode chargement vérifié sur les sources Teensy 4 | sur module : deux ports visibles sous macOS, journal inchangé ; coût du sondage mesuré |
| **3b4** — outil hôte | crate `oc-probe` : `stats`, `stream`, `out`, `screen` (PNG ou texte), `reboot` ; `cargo xtask flash` peut demander le redémarrage | tests de l'outil sur un faux port ; essai sur module |
| **3b5** — mesure | niveau 9 de `TESTING.md` (CV, triggers) aidé de la sonde ; coût d'une page, d'une trame, gigue de la boucle consignés dans `avancement/etape-3.md` | D-029 adoptée (révisée si besoin) ; chiffres utilisables pour trancher D-023 au lot 3c |

Ordre : 3b1 → 3b2 → 3b3 → 3b4 → 3b5 ; 3b1 et 3b2 sont indépendants l'un de l'autre.

## Risques

1. Deux ports CDC : 6 points de terminaison sur 8 ; à confirmer sur l'USB du RT1062 et sous macOS (3b3).
2. Redémarrage en mode chargement : mécanisme de Teensyduino à lire avant d'écrire du code ; s'il n'est pas
   documenté, la commande est retirée (D-029 § 5).
3. Le sondage USB dans la boucle de 1 ms : son coût est mesuré par la sonde elle-même (étape « USB »).

## Premières mesures (2026-11-16, module TLM, firmware `1a7d0c6`, profil des lignes USB)

| Étape | Moyenne | Max | Lecture |
|---|---|---|---|
| `sample` (4 conversions ADC bloquantes, 4 moyennes) | 288,1 µs | 288,5 µs | ≈ 72 µs par voie ; le commentaire de `cv_in.rs` disait « quelques µs » |
| `acquire` / `update` / `usb` | 1,0 / 0,5 / 0,9 µs | 1,0 / 0,6 / 3,6 µs | négligeables |
| `outputs` (DAC8565, 4 voies) | 15,4 µs | 16,7 µs | proche de 4 × 24 bits à 8 MHz (12 µs) |
| `render` (dessin de l'écran diagnostic) | 632,4 µs | 634,3 µs | CPU seul, tous les 20 ticks |
| `present` (trame SH1106 entière) | 1 156,9 µs | 1 157,0 µs | ≈ 145 µs par page ; 1 024 octets à 8 MHz = 1 024 µs + commandes |
| `tick` | 112 µs | 1 816 µs | le max est un tick de rendu : `render` + `present` |

Période : **1 290 à 2 109 µs, jamais 1 000** ; 95 % des périodes à 256–512 µs de la cible, 5 % (un tick sur 20,
ceux qui rendent) à plus de 1 ms.

Constats :

1. **Bug de cadence, corrigé** : la boucle se calait sur le début du tick (`report.timestamp_micros`), si bien que
   l'échantillonnage, fait avant, s'ajoutait à chaque période : le module tournait à ≈ 775 Hz depuis l'étape 1.
   Désormais la période part d'avant l'échantillonnage.
2. **Un tick de rendu dépasse la période** (≈ 290 + 1 816 µs) : c'est le trou de D-023 ; une page coûte ≈ 145 µs,
   ce qui confirme l'ordre de grandeur de la proposition « page par page » (≈ 131 µs).
3. **L'ADC coûte 29 % de la période** en conversions bloquantes : à revoir avant toute cadence plus rapide (D-023),
   moins de moyennes, conversion pendant le tick ou ADC2 en parallèle.
