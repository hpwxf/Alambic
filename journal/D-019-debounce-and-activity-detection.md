# D-019 — Anti-rebond à 3 échantillons pour triggers et boutons ; activité par crête-à-crête faute de détection de jack

| | |
|---|---|
| **Identifiant** | D-019 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `555f492` (`debounce.rs`, `signal.rs`), `ec140ed` (`is_patched` toujours vrai sur le firmware), `a839a03` (`ButtonReader` centralise l'anti-rebond des boutons), lot 3a (triggers sans filtre temporel) |

## 1. Contexte

Deux familles d'entrées numériques : les **boutons et contacts d'encodeur**, mécaniques, qui rebondissent ; les
**entrées trigger**, électroniques, qui reçoivent des impulsions Eurorack (souvent 1 à 10 ms, parfois moins). Par
ailleurs, les jacks de l'O&C n'ont pas de contact de détection : le firmware ne peut pas savoir si un câble est
branché, alors que l'écran de diagnostic veut le montrer.

## 2. Options envisagées

Pour les entrées numériques :

**A** — Un même anti-rebond pour tout : un niveau n'est accepté qu'après `N` échantillons identiques consécutifs
(`N = 3`, soit 3 ms à 1 kHz).

**B** — Anti-rebond pour les contacts mécaniques seulement ; pour les triggers, détection de **front** sans
filtrage temporel, idéalement par interruption ou capture matérielle (c'est ce que fait le firmware de référence,
qui attache une interruption sur front descendant à chaque entrée trigger).

Pour la présence de signal :

**C** — `AnalogIn::is_patched` dans le trait de plateforme, que chaque backend renseigne comme il peut.

**D** — Une détection d'**activité** par excursion crête-à-crête sur des blocs de 256 échantillons (≈ 0,25 s à
1 kHz), seuil 40 mV, calculée dans `oc-core` à partir des seules valeurs lues.

## 3. Ce que l'assistant a proposé

**A** pour les deux familles (`555f492` : « trigger debouncing with edge counting ») ; **C et D** ensemble
(« peak-to-peak activity detection (the module has no cable detection) »), le firmware répondant
`is_patched = true` (« the honest answer is "always connected" »).

## 4. Ce que nous avons décidé, et pourquoi

**A** et **C + D**, de fait. Le commentaire de `debounce.rs` justifie A : « At the nominal 1 kHz tick rate the default
of three samples rejects bounce shorter than 3 ms while adding at most 3 ms of latency, which is inaudible for gate
timing. » Le scénario doré `trigger_burst.scn` **fixe ce comportement** : des impulsions de 10 ms sur TR1 sont
comptées, des impulsions d'**un tick (1 ms)** sur TR2 sont traitées comme du rebond et ignorées.

## 5. Ce qui nous ferait changer d'avis

* Un module qui manque des triggers sur un vrai patch — voir les points ouverts : c'est déjà prévisible par le calcul.
* Un applet qui se met à dépendre de `is_patched` : il se comporterait différemment dans Rack et sur le module.

## Révision (2026-11-16, lot 3a) — triggers sans filtre temporel

**B** remplace **A** pour les triggers. `TRIGGER_STABLE_SAMPLES = 1` : un niveau de trigger est accepté dès son
premier échantillon, c'est-à-dire détection de front pure ; `DEFAULT_STABLE_SAMPLES = 3` reste pour les contacts
mécaniques (boutons, encodeurs). `EdgeCounter::default()` prend la configuration trigger, `DiagnosticApp` la nomme
explicitement. Le scénario `trigger_burst` porte maintenant trois triggers d'un tick (1 ms) sur TR2, tous comptés ;
le test `a_bouncing_trigger_edge_is_still_counted_once` devient `one_millisecond_triggers_are_each_counted`.

Pourquoi : un front électronique ne rebondit pas, et 1 ms est une largeur courante en Eurorack ; le filtre ne
protégeait de rien et perdait des triggers réels. Ce qui reste : une impulsion **plus courte qu'un tick**, ou tombée
pendant le tick allongé de l'OLED (D-008), peut encore passer entre deux échantillons. Seule une capture par
interruption GPIO ou à la cadence de l'ISR signal (D-023) la rattrape. Prix accepté : un parasite d'un échantillon
compte comme un trigger, et un bouton-poussoir branché sur une entrée trigger compte ses rebonds (`TESTING.md`
niveau 9.3 mis à jour).

## Points ouverts (revue du 2026-11-16)

* ~~**Les triggers courts sont perdus par construction.**~~ **Traité** pour les impulsions d'au moins un tick
  (révision ci-dessus) ; la capture par interruption reste à trancher avec D-023. À 1 kHz avec `N = 3`, toute impulsion de moins de 3 ms est
  ignorée, et une impulsion de 3 à 4 ms dépend de la phase d'échantillonnage. S'y ajoute la gigue de D-008 : le tick
  qui pousse l'OLED dure ≈ 1 ms de plus, pendant lequel les entrées ne sont pas lues. Or 1 ms est une largeur de
  trigger courante en Eurorack, et le scénario doré la qualifie de rebond. Un front électronique propre (sortie
  d'un buffer à transistor) ne rebondit pas : appliquer aux triggers le filtre des contacts mécaniques est une erreur
  de modèle. Correctif proposé : `required = 1` (ou détection de front sur deux échantillons) pour les triggers dès
  maintenant, et, avec D-023, capture des fronts par interruption GPIO ou à la cadence de l'ISR signal. Le scénario
  `trigger_burst` est à réécrire en conséquence.
* `is_patched` est un champ du trait que le matériel **ne peut pas** honorer ; seul Rack fournit une vraie valeur, le
  simulateur une valeur scriptée. L'affichage `P` diffère donc entre backends (D-002, D-015). Options : retirer
  `is_patched` du trait et ne garder que l'activité (D), ou forcer `true` dans Rack et le simulateur pour coller au
  matériel.
