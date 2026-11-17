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
| **3c2** — écran page par page (écrit, essai sur module à faire) | `Display` : nombre de pages et envoi d'une page, par défaut une seule page qui envoie tout (simulateur, Rack et tests inchangés) ; l'`Engine` dessine dans un tick, puis envoie une page par tick suivant ; l'écran de démarrage garde la trame entière (révision de D-009, D-010) | tests `oc-core` (ordre des pages, aucun dessin pendant un envoi, intervalle de rendu plus court que le nombre de pages) ; sur module : plus aucun tick au-delà de la période hors écran de démarrage, ≈ 1 000 ticks par seconde, `cargo probe stats` |
| **3c3** — `signal_tick` / `ui_tick` | séparer dans `oc-core` ce qui doit suivre le signal (CV, triggers, applet, sorties) de l'interface (panneau, menus, dessin, pages) ; `tick()` reste la composition pour les trois backends. La forme exacte du contrat des applets se décide au début du lot, dans D-023 | tests et écrans dorés inchangés ; définition de « fait » |
| **3c4** — mesure et clôture | niveau 9.3 au Pamela, profil, `TESTING.md` ; D-023 et D-019 à jour ; `avancement/etape-3.md` ; étape 3 close | critères de l'étape 3 (`roadmap.md`) tenus et consignés |

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
