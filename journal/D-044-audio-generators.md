# D-044 — Des générateurs pour la sortie audio : la voix ne nomme plus le Supersaw

| | |
|---|---|
| **Identifiant** | D-044 |
| **Date** | 2026-11-21 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ce commit (ADR seule) ; implémentation au premier lot qui l'adoptera |
| **Relations** | étape 2 de la trajectoire de D-043 ; révisera D-033 (forme de `Params` et de `Voice`) et D-037 (où vit le banc du Supersaw) ; garde D-040 (le `Setup` préparé dans la boucle, copié par l'interruption) et D-042 (un échantillon par période) ; mesurée par D-029 (`voice`, `voice_params`, `audio`) |

## 1. Contexte

D-043 a sorti les apps du cœur côté contrôle : le moteur ne les atteint plus que par le trait `App`. Le côté signal,
lui, nomme encore une app (`crates/oc-core/src/audio/mod.rs`) :

* `Channel` est un enum fermé : `Level`, `Tone`, `Supersaw(Part)` ;
* `Params` porte un champ `supersaw: Supersaw` pour toutes les apps ;
* `Setup` porte un `supersaw::Setup` et le drapeau `bank_used` ;
* `Voice` contient le banc du Supersaw, et `next_sample` aiguille sur `Channel` à chaque échantillon ;
* le banc lui-même vit dans `audio/supersaw/` (`fixed.rs`, `float.rs`), à côté des briques génériques
  (`wave`, la table de sinus).

Une nouvelle app sonore modifie donc `Channel`, `Params`, `Setup` et `Voice` ; deux apps sonores simultanées sont
impossibles (un seul banc). Le `Setup` copié par l'interruption est la **somme** de tout ce que toutes les sources
peuvent demander (les pas des tonalités et tout le banc), alors qu'une seule sert à la fois.

Contraintes qui ne changent pas : l'interruption tourne à 47 619 Hz, sans allocation ; tout ce qui ne dépend que des
paramètres se calcule dans la boucle (`Setup::new`, D-040) et l'interruption ne fait que copier et rendre ; une
tonalité garde sa phase quand ses paramètres changent (pas de clic, D-033).

Tranché par l'utilisateur le 2026-11-21 : **le banc du Supersaw vit dans l'app `SUPERSAW`**. S'il doit un jour être
partagé, ce sera par une bibliothèque DSP commune (un module ou une crate `dsp`), pas en le laissant dans `audio`.

## 2. Options envisagées

* **A — l'enum fermé actuel.** Rien à faire ; le couplage reste.
* **B — un trait `Generator` et un enum de générateurs composé.** Un générateur rend un échantillon des quatre sorties
  à partir de son `Setup` (un banc alimente plusieurs sorties à la fois : l'unité n'est pas une sortie) :

  ```rust
  pub trait Generator {
      type Params: Copy + PartialEq;   // published by the applet once a tick
      type Setup: Copy;                // computed in the loop (D-040), copied by the interrupt
      type State;                      // phases, filters: lives in the interrupt
      fn setup(params: &Self::Params, rate_hz: u32) -> Self::Setup;
      fn apply(state: &mut Self::State, setup: &Self::Setup); // keeps phases
      fn next(state: &mut Self::State, setup: &Self::Setup) -> [MilliVolts; CV_CHANNELS];
  }
  ```

  L'endroit qui compose l'image (là où vit `AppHost`) déclare la liste des générateurs ; une macro d'`audio` en tire
  trois enums (`Params`, `Setup`, `State`) et leur aiguillage, statique. `audio` garde le mécanisme (la voix, la
  macro, le trait) et les briques génériques, dont le générateur `Basic` (niveaux tenus et tonalités) que partagent
  les apps CV et `TONE`.
* **C — des générateurs derrière `dyn` dans l'interruption.** Ouvert sans macro, mais un appel indirect par
  échantillon, un état à loger dans une mémoire statique sans allocateur, et un `Setup` qui n'est plus une valeur
  `Copy` de taille fixe à déposer dans `MAILBOX` : D-040 serait à refaire.
* **D — l'app rend elle-même ses échantillons** (une méthode audio sur `App`). Mélange les deux contextes : l'app
  vit dans la boucle, ses échantillons dans l'interruption ; contraire à D-033 et D-040.

Où vit le banc : dans `audio` (brique réutilisable que la voix ne nomme plus), dans l'app, ou dans une bibliothèque
DSP partagée. Tranché : dans l'app (§ 1).

## 3. Ce que l'assistant a proposé

**B.** C'est l'option C de D-043 (l'aiguillage statique réservé au chemin où le coût par échantillon compte) : pas
d'appel indirect, un `Setup` toujours `Copy` et de taille fixe, la règle de D-040 intacte.

* **Ce qui bouge** : `audio/supersaw/` devient `apps/supersaw/bank/` (le banc entier et le banc `f32`, leurs `Setup`
  et `Bank`) ; `audio` ne garde que `Voice`, le trait, la macro, `wave`, la table de sinus, `phase_step`, et le
  générateur `Basic` (le `Channel::Level` / `Channel::Tone` d'aujourd'hui, sur les quatre sorties).
* **La composition** : `apps/mod.rs` déclare `Basic` et le banc du Supersaw ; `App::update` rend les paramètres de
  son générateur (`Params::Basic(..)`, `Params::Supersaw(..)`). Les parts `Mono` / `Left` / `Right` / `Sub` et leur
  affectation aux sorties A à D deviennent des paramètres du générateur Supersaw, plus de `Channel`.
* **Changer de générateur** (passer de `TONE` à `SUPERSAW`) repart d'un état neuf ; garder le même générateur garde
  ses phases, comme aujourd'hui. Le maintien d'une sortie par la sonde (`Params::hold`, D-029) passe par une
  surcouche de la voix, pas par chaque générateur.
* **La taille copiée** devient le **maximum** des `Setup`, plus leur somme : la copie de `voice_params` (0,47 µs
  mesurée, seuil d'une microseconde dans D-040) ne peut que baisser.
* **Pour plus tard**, sans le faire ici : une voix à plusieurs emplacements de générateurs et un mélange ou un routage
  vers les sorties, quand plusieurs apps sonores tourneront ensemble (étapes 3 et 5 de D-043, et D-042 pour le
  calcul par blocs) ; la bibliothèque DSP au premier partage d'un bloc.

## 4. Ce que nous avons décidé, et pourquoi

**Proposée.** Tranché par l'utilisateur : le banc dans l'app `SUPERSAW`, une bibliothèque DSP seulement le jour où un
bloc est partagé. À trancher au premier lot : la forme exacte de la macro de composition, et où se place le maintien
d'une sortie par la sonde.

**Critères d'acceptation du premier lot** (ce qui le fait passer à `adoptée`), comme pour D-043 :

* **échantillons identiques au bit près** : une somme de contrôle des premiers échantillons de chaque app, relevée
  avant le lot, inchangée après (`TONE`, `SUPERSAW` en `INT` et en `F32`, les apps CV) ; écrans de référence intacts ;
* criterion `voice/*` inchangé au bruit près ;
* sur le module, `cargo probe stats` : `voice`, `audio` et `voice_params` inchangés au bruit près (`voice_params`
  plutôt en baisse) ;
* coût en `.text` relevé par différence de `cargo xtask size` (méthode de D-038), l'ITCM étant la limite à
  surveiller (163 Kio sur 192 après D-043).

## 5. Ce qui nous ferait changer d'avis

* `voice` qui monte au-delà du bruit : l'aiguillage par enum coûte plus que l'actuel ; revoir sa forme (une seule
  correspondance par échantillon, pas par sortie).
* Une macro de composition qui rend le code illisible ou les erreurs incompréhensibles : écrire les trois enums à la
  main, la liste étant courte.
* Un générateur dont l'état ne tient pas en mémoire statique raisonnable (une réverbération, plusieurs kilo-octets) :
  l'emplacement de l'état devient une question à part (arena de D-038).
* Deux apps qui veulent le même bloc DSP : la bibliothèque `dsp` partagée arrive à ce moment-là.

## Lot 11a (2026-11-21) — les générateurs, sur l'hôte

Le statut reste `proposée` jusqu'à la mesure sur le module (lot 11b). Ce que l'implémentation a fixé :

* **`audio`** : le trait `Generator` (`Params`, `Setup`, `State`, `REST`, `REST_SETUP`, `setup`, `start`,
  `apply`, `next` ; supertraits `Copy + PartialEq + Debug`, le générateur étant nommé par un type marqueur),
  `Params<G>`, `Setup<G>`, `Voice<G>`, et `basic` (le générateur `Basic`, niveaux et tonalités, et ses
  `Channel` / `Tone`). `Setup::rest` reste une `const fn` : `MAILBOX`, dans le firmware, est une `static`.
* **Le banc** : `audio/supersaw/` est devenu `apps/supersaw/bank/` (déplacé par `git mv`), avec le générateur
  `SupersawBank` (`SupersawParams` : le banc et la part de chaque sortie, `None` pour 0 V).
* **La composition** : `apps/sound.rs`, **écrite à la main** (le point laissé ouvert) : `Sound`, ses trois
  enums, et les alias `Params`, `Setup`, `Voice` que les backends emploient. Une macro viendra si la liste
  s'allonge.
* **Le maintien d'une sortie par la sonde** (le second point ouvert) : une surcouche de la voix,
  `Params::held`, appliquée après le générateur, et seulement quand une sortie est maintenue (`any_held`,
  décidé à chaque changement de paramètres). Le son continue dessous : une tonalité relâchée reprend à la phase
  où elle en est, et non plus là où le maintien l'avait figée.
* **Mesuré sur l'hôte** :
  * échantillons identiques au bit près : `tests/voice_samples.rs`, relevé avant le lot (`26fa243`), passe
    inchangé (`TONE`, `SUPERSAW` en `INT` et en `F32`, la rampe du diagnostic, les triggers de `RATCHET`) ;
    écrans de référence intacts ;
  * criterion, contre `master` : `voice/tone` 19,65 → 20,18 ns (dans la dispersion de `master`, 19,65 à
    20,04), `voice/supersaw` 36,5 → 30,8 ns, `square` 47,4 → 37,3 ns, `f32` 33,8 → 28,9 ns. Une première
    version perdait jusqu'à 26 % (`f32`) : la voix générique s'instancie dans la crate de l'appelant, et
    sans `#[inline]` le chemin par échantillon du banc ne s'y inlinait pas en LTO *thin* (le profil des
    bancs) ; `#[inline]` sur ce chemin, et la surcouche sautée quand rien n'est maintenu, ont tout rendu.
    Le gain est propre aux bancs : le firmware est en LTO *fat*, une seule unité ;
  * firmware contre `master` : `.text` +1 380 o (164 Kio sur 192 d'ITCM), `.rodata` +400 o, `.data` −16 o
    (`MAILBOX`, qui porte le `Setup`).

## Lot 11b (2026-11-21) — mesure sur le module, adoptée

Relevé par l'utilisateur, `cargo probe stats` sur le module TLM, dans les réglages de D-043 (lot 10c) : `SUPERSAW` en
`F32`, `NOTE` et `DETUNE` modulés par du bruit, `MIX` 50 %, `SHAPE` 25 %, `STEREO ON`, environ 10 s. Avant :
`b5c444a` (le relevé du lot 10c ; chemin audio identique à `master`) ; après : `bbc05ae`.

| Étape | Avant | Après |
|---|---|---|
| `voice` moyenne / max | 1,68 / 1,75 µs | **1,56 / 1,65 µs** |
| `audio` moyenne / max | 4,22 / 5,66 µs | 4,21 / 5,70 µs |
| `voice_params` moyenne / max | 0,47 / 0,47 µs | 0,50 / 0,52 µs |
| `update` moyenne / max | 14,87 / 18,02 µs | 13,65 / 17,21 µs |
| `outputs` moyenne / max | 1,47 / 5,63 µs | 2,27 / 6,04 µs |
| `render` moyenne | 53,84 µs | 54,13 µs |
| `tick` moyenne / max | 26,65 / 83,17 µs | 25,71 / 82,13 µs |

Trame (1 344,7 µs), période de la boucle et gigue inchangées.

* **`voice` baisse de 7 %** : l'aiguillage par générateur, une correspondance par échantillon et non plus par sortie,
  coûte moins que l'enum `Channel` ; le premier critère de § 5 ne joue pas.
* **`voice_params` monte de 0,03 µs** (une vingtaine de cycles) au lieu de baisser : le `Setup` du Supersaw
  dominait déjà la somme, `MAILBOX` n'a perdu que 16 octets, et la copie d'un enum n'est pas plus courte.
  Écart dans le bruit, loin du seuil d'une microseconde de D-040 ; ce relevé est le pire cas (le bruit change le
  `Setup` à chaque tick, une copie par milliseconde).
* **`update` et `outputs`** : −1,2 et +0,8 µs, −0,4 µs à eux deux ; tous deux préparent un `Setup` (la voix du moteur
  dans `update`, `audio::publish` dans `outputs`), le partage entre eux suit l'inlining du LTO, sans conséquence.
  `usb` (2,89 → 1,90 µs) dépend de l'hôte, pas du lot.

Critères de § 4 tenus (le léger écart de `voice_params` mis à part) : **D-044 adoptée**. D-033 (forme de `Params` et de
`Voice`) et D-037 (où vit le banc) sont révisées en conséquence.
