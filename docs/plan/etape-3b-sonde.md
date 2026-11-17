# Étape 3, lot 3b — Sonde live et mesures sur module (plan du 2026-11-16)

Demande de l'utilisateur : finir l'étape 3 ; au passage, une interface de diagnostic plus élaborée que le journal
USB, dans les deux sens, utile aussi au profilage (coût de l'affichage), sous la forme d'un outil du projet.
Décision : [D-029](../../journal/D-029-live-probe.md) (adoptée, révisée, au lot 3b5).

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
| **3b4** — outil hôte (fait, `d5365bb`, essayé sur module au lot 3b5) | crate `oc-probe` : `stats`, `stream`, `out`, `screen` (PNG ou texte), `reboot` ; `cargo xtask flash` peut demander le redémarrage | tests de l'outil sur un faux port ; essai sur module |
| **3b5** — mesure (fait, `1c11495` → ce commit) | niveau 9 de `TESTING.md` (CV, triggers) aidé de la sonde ; coût d'une page, d'une trame, gigue de la boucle consignés dans `avancement/etape-3.md` | D-029 adoptée (révisée si besoin) ; chiffres utilisables pour trancher D-023 au lot 3c |

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

**9.3, `cargo probe triggers --seconds 10`** : carré du Dixie II+ sur TR1 → 79 fronts montants en 10 s
(≈ 7,9 Hz), aucun échantillon perdu, 0 sur TR2–TR4 non branchés. La fréquence de la source n'étant pas connue
exactement, la commande donne désormais aussi l'intervalle entre fronts (min, max, moyenne en ticks) : constant à
un tick près pour une horloge régulière, un rebond ou un front manqué s'y voient sans connaître la source.
Relance (`aa5bcf7`) : 79 fronts, intervalle 126 à 128 ticks (moyenne 126,91) : ni rebond ni front manqué. L'écart
de deux ticks vient de ce qu'**un tick ne dure pas toujours 1 ms** : un sur vingt (rendu) dure ≈ 2,1 ms avant que
le calendrier se recale ; « 10 s » comptées en ticks en durent ≈ 10,5. Corrigé dans l'outil : les échantillons
portent l'horloge du module (protocole version 2), durées et intervalles sont en temps réel.

Avec l'horloge du module (`9b5d74f`) : TR1 74 fronts en 10 s, 132,716 à 134,849 ms d'écart (moyenne 134,008 ms,
≈ 7,46 Hz) ; **9 469 ticks en 9 999,9 ms, soit ≈ 947 ticks/s au lieu de 1 000** : les ≈ 50 rendus par seconde
dépassent chacun leur période d'≈ 1,1 ms. Un front est daté au début du tick qui le voit, d'où une dispersion des
intervalles égale à la durée d'un tick de rendu (≈ 2,1 ms). Pour D-023 : le rendu coûte ≈ 5 % de la cadence du
signal et retarde la lecture d'un trigger jusqu'à ≈ 2 ms. Valeurs indicatives.

**9.3, LFO lent** (Dixie ≈ 1,07 Hz, 20 s) : 21 fronts, un par cycle, 929,5 à 931,6 ms d'écart : pas de rebond sur
un front lent. **Repos** (20 s) : 0 front.

**9.3b, rebouclage `cargo probe sweep`** (−3 à +6 V, pas de 1 V, erreur entrée − sortie en mV) :

| | −3 | −2 | −1 | 0 | +1 | +2 | +3 | +4 | +5 | +6 |
|---|---|---|---|---|---|---|---|---|---|---|
| A→1 | +24 | +22 | +2 | 0 | −23 | −22 | −23 | −44 | −46 | −66 |
| B→2 | +13 | +14 | −4 | −2 | −22 | −18 | −16 | −34 | −33 | −51 |
| C→3 | +21 | +21 | −1 | −2 | −23 | −23 | −22 | −42 | −43 | −63 |
| D→4 | 0 | +5 | −7 | −1 | −16 | −6 | +1 | −12 | −6 | −19 |

Chaque sortie arrive sur l'entrée attendue (ordre des canaux cohérent). L'erreur combinée a une pente (≈ −1 % sur
A–C, bien moindre sur D) et des **marches d'≈ 20 mV** : les écarts de code ADC entre paliers alternent ≈ 402 / 409
codes (A) ou 405 / 413 (D) aux mêmes tensions sur les quatre voies, signe d'une non-linéarité commune (ADC unique
ou puce DAC8565). Le rebouclage ne sépare pas sortie et entrée : il faut une référence extérieure (multimètre sur
la sortie). Rappel : l'assistant ne règle que −3 V et +5 V par sortie puis interpole, là où Phazerville règle
chaque octave (point ouvert de l'étape 4). Valeurs indicatives.

**Sortie seule au multimètre** (`cargo probe out A`, câble de rebouclage débranché) : −3,003 / −0,001 / 2,999 /
6,001 V pour −3 / 0 / +3 / +6 V demandés, soit ≤ 3 mV d'écart, de l'ordre de la précision du multimètre, y compris
à +6 V, hors des deux points réglés par l'assistant. **L'erreur du rebouclage vient donc de l'entrée** : la sortie
A est juste, CV1 lit −23 mV à +3 V et −66 mV à +6 V. Deux composantes : (1) un gain unique, mesuré sur CV1 entre
+1 V et +3 V puis appliqué aux quatre entrées (d'où l'erreur plate entre ces deux points et une pente qui croît en
s'en éloignant, et D/CV4 qui s'écarte des autres) ; (2) des marches d'≈ 20 mV aux mêmes tensions sur les quatre
voies, non-linéarité de la conversion d'entrée commune aux voies. 20 mV ≈ 24 cents en 1 V/oct : sans effet sur le
diagnostic et le scope, à reprendre si une applet suit une hauteur (quantificateur). Pistes, non engagées :
gain par entrée, table d'entrée par morceaux comme celle du DAC. Point ouvert pour l'étape 4. Valeurs indicatives,
un module.

**9.2, source connue, ordre des canaux** (`cargo probe out A 2000`, sortie vérifiée au multimètre, branchée tour à
tour sur CV1 à CV4) : à l'écran, la ligne attendue passe à `P`, `~` s'éteint après ≈ 1 s de niveau stable, revient
à `.` au débranchement, `~` s'allume sur le LFO du Dixie ; l'étiquette qui réagit est celle de la prise sérigraphiée,
sur les quatre entrées. Flux : CV1 1 973 à 1 985 mV, CV2 1 980 à 1 983, CV3 1 976 à 1 978, CV4 1 993 à 1 995 (même
erreur d'entrée que le rebouclage, CV4 à part) ; les trois autres entrées restent dans leur bruit de repos (± 7 mV)
pendant ce temps : pas de diaphonie visible. Valeurs indicatives.

**Non vérifié** : le compte exact de triggers de 1 ms (TESTING.md 9.3, étapes 4 et 7) faute de séquenceur ou de
générateur de salves. À prévoir d'après les mesures : deux échantillons d'un tick de rendu sont espacés d'≈ 2,1 ms,
une impulsion de 1 ms qui tombe entière dans cet écart est manquée, soit de l'ordre de 5 % des impulsions de 1 ms
(≈ 1,1 ms de fenêtre toutes les ≈ 21 ms), à confirmer sur module ; c'est un argument de plus pour le lot 3c ; la mise
à jour simultanée des quatre voies du DAC8565 (il faudrait un oscilloscope à deux voies).
