# Étape 3, lot 3c — Cadence : 1 kHz sans trous (plan du 2026-11-17)

Décision : [D-023](../../journal/D-023-dual-rate-signal-isr.md), option **C+** retenue par l'utilisateur le
2026-11-17, sur les mesures du lot 3b ([etape-3b-sonde.md](etape-3b-sonde.md), bilan
[avancement/etape-3.md](avancement/etape-3.md)). Les options A (ISR 16 666 Hz) et B (audio 48 kHz + DMA) restent des
suites possibles ; C+ en est la première marche, rien n'y est perdu.

Ce que les mesures imposent (valeurs indicatives, un module) :

* un tick de rendu dessine (≈ 0,63 ms) **et** envoie la trame entière (≈ 1,16 ms) : trou d'≈ 2 ms toutes les 20 ms,
  ≈ 947 ticks par seconde au lieu de 1 000 ;
* les triggers sont échantillonnés une fois par tick : 39 % perdus à 0,94 ms, 3,9 % à 1,56 ms, 0,7 % à 2,5 ms ; le
  critère de l'étape 3 (une impulsion de 1 ms comptée sur le module, `roadmap.md`) n'est pas tenu ;
* l'échantillonnage des CV est bloquant (≈ 0,29 ms) : il reste dans le budget à 1 kHz, pas au-delà.

## Lots

| Lot | Contenu | Critère d'acceptation |
|---|---|---|
| **3c1** — capture des fronts (fait, `c771353`, vérifié sur module) | le détecteur de front des GPIO du RT1062 (`imxrt-hal` : `set_interrupt`, `is_triggered`, `clear_triggered`) mémorise le front actif de chaque trigger ; la ligne du NVIC reste fermée, rien ne s'exécute en interruption. `oc-drivers` : trait `EdgeCapture`, `Triggers` lit « haut maintenant **ou** monté depuis la lecture précédente » ; Rack : même règle entre deux ticks (`oc-vcv-ffi`) ; le simulateur fournit déjà des niveaux par tick. Le contrat de `DigitalIn::trigger_state` le dit (révision de D-019) | tests `oc-drivers` (impulsion plus courte qu'un tick comptée une fois, impulsion longue non doublée, polarité) ; sur module, au Pamela : 960 / 960 à 3 % (0,94 ms), aucun front en trop à 50 % |
| **3c2** — écran page par page (fait, `f4299a6`, vérifié sur module) | `Display` : nombre de pages et envoi d'une page, par défaut une seule page qui envoie tout (simulateur, Rack et tests inchangés) ; l'`Engine` dessine dans un tick, puis envoie une page par tick suivant ; l'écran de démarrage garde la trame entière (révision de D-009, D-010) | tests `oc-core` (ordre des pages, aucun dessin pendant un envoi, intervalle de rendu plus court que le nombre de pages) ; sur module : plus aucun tick au-delà de la période hors écran de démarrage, ≈ 1 000 ticks par seconde, `cargo probe stats` |
| **3c3** — `signal_tick` / `ui_tick` (**reporté à l'option A**, décision de l'utilisateur du 2026-11-17 : C+ n'en a pas besoin, sa forme dépend de l'état partagé entre l'ISR et la boucle) | séparer dans `oc-core` ce qui doit suivre le signal (CV, triggers, applet, sorties) de l'interface (panneau, menus, dessin, pages) ; `tick()` reste la composition pour les trois backends. La forme exacte du contrat des applets se décide au début du lot, dans D-023 | tests et écrans dorés inchangés ; définition de « fait » |
| **3c4** — mesure et clôture (fait) | niveau 9.3 au Pamela, profil, `TESTING.md` ; D-023 et D-019 à jour ; `avancement/etape-3.md` ; étape 3 close | critères de l'étape 3 (`roadmap.md`) tenus et consignés |

Ordre : 3c1 et 3c2 sont indépendants ; 3c3 après 3c2 (l'envoi par pages fait partie de l'interface) ; 3c4 en dernier.

## Risques

1. **Le tick de dessin reste lourd** : ≈ 0,29 ms d'échantillonnage + ≈ 0,63 ms de dessin du diagnostic ≈ 0,92 ms
   sur 1 ms. Un écran plus coûteux (scope, menu) peut dépasser ; le profil le dira par écran. Parades, dans
   l'ordre : dessiner en deux ticks, échantillonnage non bloquant (qui sert aussi A).
2. **Deux fronts dans le même tick n'en font qu'un** avec un bit mémorisé : sans usage musical à moins d'1 ms
   d'écart, accepté ; une interruption les compterait (option A).
3. **Un parasite de quelques µs est désormais compté** : le détecteur est plus sensible que l'échantillonnage.
   Repos au Pamela et câble débranché à vérifier en 3c1 ; l'hystérésis des pads est déjà active (D-007).
4. Une page envoyée par tick allonge le délai entre le dessin et l'écran complet à ≈ 9 ms : invisible à 50 images
   par seconde.

## Mesures (module TLM, valeurs indicatives)

**3c1** (`c771353`, Pamela's Pro Workout, 120 BPM ×16, 31,25 ms, 30 s, `cargo probe triggers --seconds 30`) :

| Width | Avant (`2a713a2`) | Après | Intervalle min – max | Largeur vue (moyenne) |
|---|---|---|---|---|
| 3 % (0,94 ms) | 588 / 960 | **960 / 960** | 30,1 – 33,2 ms | 1,06 ms (un tick) |
| 50 % (15,6 ms) | 960 / 960 | **960 / 960** | 30,1 – 33,2 ms | 14,96 ms |
| repos, câble branché | 0 | **0** | — | — |

Aucune perte (une perte doublerait l'intervalle), aucun front compté deux fois, aucun parasite au repos. Une
impulsion courte est vue pendant un tick, comme prévu. La datation reste au tick (dispersion ≈ ± 2 ms, toujours
≈ 947 ticks par seconde : c'est le lot 3c2).

**3c1, limite de largeur** (même firmware `c771353`, Pamela à 120 BPM) :

| Réglage | Période | Largeur envoyée | Fronts en 30 s |
|---|---|---|---|
| ×32, 5 % | 15,625 ms | 0,78 ms | **1 920 / 1 920** |
| ×32, 4 % | 15,625 ms | 0,625 ms | **0** |
| ×96, 13 % | 5,208 ms | 0,68 ms | **5 760 / 5 760** |
| ×96, 12 % | 5,208 ms | 0,625 ms | **0** |

Au-dessus du seuil, compte exact jusqu'à 192 Hz ; en dessous, **plus aucun front**, quelle que soit la cadence. Ce
n'est donc pas l'échantillonnage (le détecteur de front ne dépend pas de la largeur) : l'impulsion n'atteint jamais
le seuil logique de la broche, ou n'est pas émise. Les deux limites tombent exactement sur 0,625 ms, ce qui
évoque un pas de la source plutôt qu'un filtre analogique (qui raccourcirait progressivement) ; non tranché sans
oscilloscope ou seconde source. Valeurs indicatives.

**3c2** (`f4299a6`, `cargo probe stats --fresh 10`, Pamela 192x / 26 % sur TR1) :

| Écran | `render` | `tick` max | Pire tick + `sample` | Période min – max | Ticks en 10 s |
|---|---|---|---|---|---|
| Diagnostic | 645,5 µs | 671 µs | ≈ 0,96 ms | 997,5 – 1 002,4 µs | 10 005 |
| Scope | 73,2 µs | 169 µs | ≈ 0,46 ms | 997,5 – 1 002,4 µs | 10 005 |
| SETUP | 417,5 µs | 426 µs | ≈ 0,71 ms | 997,5 – 1 002,3 µs | 10 002 |

Pages : 4 000 en 10 s (8 × 50 images par seconde), 144,6 µs chacune. **Plus aucun tick ne dépasse la période** : la
boucle tient 1 000 ticks par seconde (≈ 947 avant), toutes les périodes à moins de 4 µs de la cible, sur les trois
écrans. Le Diagnostic est le pire cas, ≈ 40 µs de marge : son dessin coûte près de neuf fois celui du Scope, sans
doute le texte ; si la marge manque un jour, regarder d'abord le dessin des caractères. Valeurs indicatives.

Affichage normal sur les trois écrans (pas de déchirure). `cargo probe triggers --seconds 30` à 192x / 26 %
(0,68 ms toutes les 2,604 ms) : **11 521 fronts** pour 11 520 attendus (± 1 de fenêtre), **29 999 ticks en
29 999,0 ms**, intervalles de 1,997 à 3,003 ms (moyenne 2,603) : la dispersion n'est plus que d'un tick, la
quantification attendue d'une période qui n'est pas un multiple de 1 ms (≈ −1,1 / +2,0 ms avant 3c2).

**Après l'étape 3** (`53ee310`, texte posé par colonnes, D-009) : Diagnostic `render` 645,5 → **87,8 µs**, `tick`
max 671 → 169 µs, périodes 997,6 – 1 002,3 µs ; la marge du tick de dessin (≈ 40 µs) n'est plus un risque.
