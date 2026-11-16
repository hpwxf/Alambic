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
| **3b3** — firmware (fait, `fcd683d` → `22fb58a`, vérifié sur module) | `usb-device` composite à deux ports CDC (journal `log` maison sur le premier, `imxrt-log` retiré) ; serveur du protocole sondé par la boucle ; envoi OLED mesuré page par page ; commandes servies par l'`Engine` (sortie forcée, framebuffer) ; redémarrage en mode chargement vérifié sur les sources Teensy 4 | sur module : deux ports visibles sous macOS, journal inchangé ; coût du sondage mesuré |
| **3b4** — outil hôte (écrit, essai sur module à faire) | crate `oc-probe` : `stats`, `stream`, `out`, `screen` (PNG ou texte), `reboot` ; `cargo xtask flash` peut demander le redémarrage | tests de l'outil sur un faux port ; essai sur module |
| **3b5** — mesure | niveau 9 de `TESTING.md` (CV, triggers) aidé de la sonde ; coût d'une page, d'une trame, gigue de la boucle consignés dans `avancement/etape-3.md` | D-029 adoptée (révisée si besoin) ; chiffres utilisables pour trancher D-023 au lot 3c |

Ordre : 3b1 → 3b2 → 3b3 → 3b4 → 3b5 ; 3b1 et 3b2 sont indépendants l'un de l'autre.

## Risques

1. Deux ports CDC : 6 points de terminaison sur 8 ; à confirmer sur l'USB du RT1062 et sous macOS (3b3).
2. Redémarrage en mode chargement : mécanisme de Teensyduino à lire avant d'écrire du code ; s'il n'est pas
   documenté, la commande est retirée (D-029 § 5).
3. Le sondage USB dans la boucle de 1 ms : son coût est mesuré par la sonde elle-même (étape « USB »).

## Premières mesures (2026-11-16, module TLM, firmware `1a7d0c6`, profil des lignes USB)

Un seul exemplaire, un seul hôte : ces chiffres situent des ordres de grandeur, ils ne fixent aucune constante du
code. Ce qui en dépend (marge d'attente sans `poll` USB) est mesuré par le firmware lui-même à l'exécution.

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

Après correction (`94133c5`) : période **1 002,5 à 2 110,7 µs** ; 88 % à 2–4 µs de la cible, 7 % à 4–8 µs, 5 % à
plus de 1 ms (les ticks de rendu). Reste un retard fixe : l'échéance repartait de l'instant où l'attente l'avait
vue passer (pas de 1 µs du GPT, plus un `poll` USB jusqu'à 10,6 µs), et ce retard s'ajoutait à chaque période
(≈ 997,5 Hz). Corrigé à la suite : calendrier fixe (échéance précédente + période, recalé seulement après un
dépassement, donc toujours sans dette), et plus de `poll` USB quand le plus long `poll` observé depuis le démarrage ne tient plus avant l'échéance.

Après le calendrier fixe (`6f475d4`) : `last_us=1000` ; hors ticks de rendu, toutes les périodes à moins de 4 µs de
la cible (≈ 48 % sous 1 µs, 40 % à 1–2 µs, 7 % à 2–4 µs) ; minimum 997,6 µs, la compensation attendue d'une
période sortie tard ; les ticks de rendu (≈ 5 %) restent au-delà de 1 ms, sujet du lot 3c. Valeurs indicatives.

Après le lot 3b3 (`bd0db5e`, deux ports USB, `screen` ouvert sur le premier) : deux ports visibles sous macOS,
journal inchangé ; `page` ≈ 145 µs par page (huit par trame, cohérent avec `present`). Gigue hors rendu toujours
sous 8 µs, mais plus étalée (≈ 14 % sous 1 µs, 47 % à 1–2 µs, 31 % à 2–4 µs, 2 % à 4–8 µs) : à examiner avec la
sonde (service du second port dans l'attente, marge de `poll` en µs entières). Valeurs indicatives. La ligne
`profile` était tronquée (tampon de 160 caractères) : corrigé.

Essai du redémarrage (`22fb58a`) : `stty -f <port> 134` (le `stty` de macOS ; GNU : `-F`) fait passer le module en
mode chargement, et `cargo xtask flash` passe ensuite sans appui sur PROGRAM.

Premier `cargo probe stats` (`d5365bb`, ≈ 94 s après le démarrage) : tick 111,6 µs en moyenne (max 1 822,5),
`render` ≈ 639 µs, `present` ≈ 1 157 µs, `page` ≈ 144,5 µs (8 par trame), `sample` ≈ 287 µs, `usb` ≈ 1,8 µs
(max 7,3) ; période moyenne 1 055,6 µs, tirée par les 4,9 % de ticks de rendu au-delà de 1 ms ; hors rendu,
≈ 93 % des périodes à moins de 4 µs de la cible. Valeurs indicatives.

Essai de `cargo probe out A 1000` : 1 V au multimètre, mais l'écran Diagnostic affichait encore `0.0` (sa propre
sortie, pas celle du jack) ; corrigé : la ligne `OUT` affiche ce qui est écrit, `OUT*` quand la sonde tient une
sortie.

## Niveau 9 avec la sonde (lot 3b5, module TLM, firmware `1c11495`)

**9.2, `cargo probe inputs --seconds 20`** (20 000 échantillons, aucun perdu) : CV2–CV4 non branchées, moyenne
−1 à 0 mV, crête-à-crête 20 à 24 mV (fenêtre de 20 s, contre ≈ 17 mV par seconde au journal), bien sous le seuil
de présence de 50 mV (D-024). CV1 reçoit une sinusoïde (Dixie II+, ≈ ± 4,8 V) : +4 793 mV en haut, −3 280 mV en
bas au code 4 086 — le creux est écrêté par la plage de l'entrée (≈ −3,33 à +6,67 V, calibration d'usine de
Phazerville), pas un défaut. Valeurs indicatives.
