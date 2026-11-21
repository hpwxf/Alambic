# D-039 — Essai du Supersaw en `f32` : les deux calculs embarqués, choisis par un réglage

| | |
|---|---|
| **Identifiant** | D-039 |
| **Date** | 2026-11-19 |
| **Auteur** | Pascal Havé |
| **Statut** | proposée |
| **Commits** | ce commit (ADR et code) |
| **Relations** | révise D-005 (première exception au « pas de flottant dans `oc-core` », prévue par son § 5) ; étend D-037 (le banc entier reste, un banc `f32` s'ajoute) ; mesuré par D-029 (profil `audio` de l'interruption, D-033) ; budget de code D-038 |

## 1. Contexte

D-005 a fixé les millivolts `i32` et « pas de flottant dans le chemin critique du firmware », et annonçait en § 5 qu'un
oscillateur audio rouvrirait la question. D-037 a écrit le Supersaw en virgule fixe (rampe en 1.15, correction
PolyBLEP en 16.16 par un inverse `2^48 / dt`, passe-haut en 2.30 avec 8 bits de garde) sans rouvrir D-005 : le choix
entier y est tranché par l'assistant, sans mesure. Le 2026-11-19, en relisant ce choix, on a constaté que les raisons
de D-005 tiennent pour le contrôle (conversions exactes, phase en `u32`) mais mal pour le DSP : les opérations de base
`f32` sont déterministes en IEEE 754 (Rust ne contracte pas en FMA de lui-même), et le Cortex-M7 a un FPU simple et
double précision (FPv5-D16) dont `cortex-m-rt` active l'accès au démarrage pour une cible `eabihf`. Ce qui reste
inconnu : le coût réel dans l'interruption à 47 619 Hz (empilement paresseux des registres flottants à l'entrée,
`audio` mesuré à 3,9 µs sur 21 en entier) et la lisibilité gagnée.

L'utilisateur demande d'essayer une version `f32` du Supersaw, **les deux versions embarquées dans l'applet,
sélectionnables par un réglage**, pour les comparer sur le module.

## 2. Options envisagées

**Où vit le choix**

* **A — feature de compilation** (`supersaw-f32`) : une seule version dans l'image, rien de plus en ITCM ; mais deux
  firmwares à flasher pour comparer, et pas de comparaison à l'oreille d'un instant à l'autre.
* **B — un réglage de l'applet** (`CALC INT` / `F32`) qui choisit le banc : les deux dans l'image, bascule en direct,
  le profil `audio` de `cargo probe stats` compare sur le même module, la même note, le même patch.
* **C — deux applets** (`SUPERSAW`, `SUPERSAW F`) : rien à changer dans l'applet, mais deux entrées de menu et deux
  états de réglages à garder accordés.

**Ce que calcule le banc `f32`**

* **F1 — tout en `f32`, phase comprise** : phase dans `[0, 1)` incrémentée et repliée en flottant, PolyBLEP par
  `x = t · (1/dt)` (inverse calculé au changement de paramètres), sommes, normalisation et passe-haut en `f32`,
  conversion en millivolts arrondis à la sortie du banc. Ce qu'on écrirait naturellement en flottant ; la phase en
  `f32` perd de la précision près de 1 (pas de 2^-24, soit un écart de hauteur de l'ordre du centième de cent).
* **F2 — phase en `u32`, le reste en `f32`** : la phase garde son repli gratuit et sa précision uniforme, convertie en
  `f32` à chaque échantillon. Mélange des deux arithmétiques, moins représentatif de « la version flottante ».

## 3. Ce que l'assistant a proposé

**B** avec **F1**. Le réglage `CALC` en dernière ligne de l'applet, sans entrée assignable (comme `STEREO`), `INT` au
démarrage pour que rien ne change tant qu'on ne le touche pas (écrans de référence compris, sauf la place prise par la
ligne). Le banc entier de D-037 reste intact ; `Bank` porte les deux bancs, règle les deux à chaque changement de
paramètres (une division et une racine de plus, pas par échantillon) et n'exécute que celui choisi : basculer garde
chaque banc dans son propre état, sans recalcul. Les calculs de préparation restent en entier là où le résultat est une
table (la courbe de désaccord de Szabo, dont le polynôme de degré 11 perd toute précision en `f32`), convertis en
`f32` ensuite. `core` n'a ni `sqrt` ni `round` en `no_std` : la racine du normaliseur (préparation seulement) passe par
la racine entière déjà utilisée, et l'arrondi de sortie est écrit à la main.

## 4. Ce que nous avons décidé, et pourquoi

**B** et **F1**, proposés par l'assistant, en essai. Reste à trancher par la mesure sur le module, qui fera passer
cette entrée à `adoptée` (et décidera du sort de l'un des deux bancs) :

* `cargo probe stats` : `audio` en `INT` et en `F32`, moyenne et maximum, à A2 et à C8, 100 % de désaccord ;
* l'écoute, et le spectre par l'exemple `aliasing` (même repliement attendu, au bruit d'arrondi près) ;
* `.text` avant et après (D-038 : mesure par différence de compilations).

Révision de D-005 : le flottant entre dans `oc-core` **pour le DSP d'un banc audio seulement** ; le contrôle, les
paramètres publiés (`Params`), les conversions et la calibration restent en millivolts `i32`, et le banc rend des
millivolts. Les tests du banc `f32` le comparent au banc entier avec une tolérance, pas à l'égalité.

## 5. Ce qui nous ferait changer d'avis

* `F32` nettement plus coûteux que `INT` dans l'interruption (au-delà de ≈ 1 µs de plus en moyenne) : l'empilement des
  registres flottants ou les conversions pèsent plus que le calcul ; garder l'entier pour le chemin audio.
* `F32` au même coût ou moins : les futurs oscillateurs et filtres s'écrivent en `f32`, le banc entier est retiré au
  lot suivant, et D-005 est révisée pour de bon (frontière : millivolts aux bords du banc).
* Un écart audible ou mesuré entre les deux (justesse, repliement) : regarder d'abord la phase en `f32` (F2).

## Révision du 2026-11-21 — un seul banc préparé

Le § 3 réglait les deux bancs à chaque changement de paramètres, pour qu'une bascule de `CALC` se fasse sans recalcul.
Une CV qui bouge change les paramètres à chaque tick, et la boucle prépare le `Setup` deux fois (la voix du moteur et
`audio::publish`, mesuré sur le module : ≈ 10 µs par tick sous modulation, D-044) : la moitié de ce travail préparait
un banc qui ne tourne pas. Le `Setup` ne porte plus que les coefficients du banc choisi (un enum `Fixed` / `Float`) ;
l'autre banc garde ses phases et ses derniers coefficients, et basculer, qui est un changement de paramètres, lui en
prépare de nouveaux. Échantillons identiques au bit près, bascule comprise ; `Setup` 424 → 320 octets ; sur l'hôte,
`Setup::new` 66,8 → 57,3 ns en `INT`, 66,3 → 63,7 ns en `F32` (la préparation commune domine) ; firmware : `.text` +228 o, `.rodata` −96 o, `.data` −104 o (`MAILBOX`). Sur le module
(`2895915`, `NOTE` et `DETUNE` sous bruit, `STEREO ON`), le `Setup` coûte ≈ 4,7 µs en `F32` et ≈ 7 à 9 µs en `INT` (les
divisions 64 bits des inverses, émulées) ; en `F32`, `tick` 25,71 → 21,71 µs et `voice_params` 0,50 → 0,35 µs contre
`bbc05ae`.

Même relevé, pour trancher l'essai : `voice` en `INT` / `F32`, 1,25 / **1,04** µs avec `SHAPE` à 0 (`F32` 17 % moins
cher, comme au lot 9b), mais 1,37 / **1,57** µs avec `SHAPE` à 25 % (`F32` 15 % plus cher) : la seconde scie corrigée
par voix coûte ≈ 0,1 µs en entier et ≈ 0,5 µs en `f32`. Le reste de cette entrée est inchangé.

## Révision du 2026-11-21 — les comparaisons de phase par leurs bits (lot 9i)

Le surcoût de `SHAPE` en `F32` (≈ 0,5 µs par échantillon, contre ≈ 0,1 en `INT`) ne venait pas du calcul : chaque
comparaison de phase en `f32` est un `vcmp`, un transfert des drapeaux du FPU vers le cœur (`vmrs`) et un
branchement, et la seconde scie en ajoute trois par voix. Pour des valeurs positives ou nulles, l'ordre des bits
IEEE est celui des valeurs : le banc `f32` compare ses phases par `to_bits()`, en entier. Mêmes résultats, échantillons
identiques au bit près ; les paires `vcmp` / `vmrs` de `next_sample` passent de 79 à 13, `.text` −1 312 o.

Sur le module (`14aad64`, `STEREO ON`, `MIX` 50 %), `voice` : `F32` 1,56 → **1,18** µs à `SHAPE` 25 %, 1,03 →
**0,88** µs à 0 ; `INT` 1,36 et 1,25 µs, inchangé. `F32` est désormais moins cher aux deux réglages (−13 % et
−30 %). Sur l'hôte, où le transfert coûte plus que la comparaison, `F32` ralentit (≈ 29 → 39 ns) : les bancs criterion
restent un garde-fou, pas la mesure. Essayé et écarté : `-C target-cpu=cortex-m7`, qui aurait donné au compilateur les
sélections sans branchement de FPv5 ; le binaire produit ne contenait plus d'instruction flottante (`.text` +11 Kio),
et c'est un changement de chaîne (D-004).

## Révision du 2026-11-21 — F2 : les phases en entier (lot 9j)

Le seul écart mesuré en défaveur de `F32` venait de F1 (§ 2) : une phase en `f32` près de la fin de son tour ne garde
que 24 bits, et leur somme dérive, d'où un banc plus loin de la supersaw idéale aux notes basses. Le banc `f32` passe à
**F2** : phases en `u32` comme le banc entier, repli gratuit, lues en fraction de tour à chaque échantillon ; la
distance au saut est prise en entier avant de devenir une fraction, et la position contre le saut se décide en entier.
Le « mélange des deux arithmétiques » reproché à F2 se borne à la phase.

* Écart à la supersaw idéale (exemple `aliasing`) : celui du banc entier à 0,1 dB près, à toutes les notes (C2 sous
  10 kHz : −28,5 → −39,2 dB, C4 −32,1 → −34,5 dB).
* Module (`6b1b25d`), `voice` : `F32` 1,15 µs à `SHAPE` 25 %, 0,86 µs à 0 ; `INT` 1,37 et 1,25 µs.
* Empreinte des échantillons `F32` de `tests/voice_samples.rs` reprise ; écart au banc entier toujours sous la
  tolérance ; `.text` +252 o.

Avec le lot 9i, les deux conditions posées le 2026-11-21 pour trancher sont levées : `F32` est moins cher que `INT` à
tous les réglages mesurés (−16 % à `SHAPE` 25 %, −31 % à 0) et aussi précis. Reste l'écoute.
