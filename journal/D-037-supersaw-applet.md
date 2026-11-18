# D-037 — Applet Supersaw : sept scies PolyBLEP en entier, banc d'oscillateurs partagé par les sorties

| | |
|---|---|
| **Identifiant** | D-037 |
| **Date** | 2026-11-18 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR, plan) ; implémentation dans les lots de l'étape 8 (`docs/plan/etape-8-supersaw.md`) |
| **Relations** | premier oscillateur sur le chemin audio de D-033 ; étend `oc_core::audio` (D-033, révision du lot 7b) ; ajoute une applet à D-021 ; pas de flottant (D-005) ; sorties par la calibration (D-027) ; étude `docs/supersaw/` § 2–14 |

## 1. Contexte

L'étude `docs/supersaw/` (étape 2) est à l'origine du chemin audio : le rack de l'utilisateur ne produit pas de
supersaw jouable (sept scies désaccordées autour d'une hauteur commune, à la manière du JP-8000). Le chemin audio est
là depuis l'étape 7 (D-033) : DAC en DMA à 47 619 Hz, une `Voice` dans l'interruption du DMA, `audio` ≈ 3,4 µs sur
21 µs dans `TONE`. L'utilisateur demande l'applet (2026-11-18).

Faits pour ce choix :

* `Voice` rend, pour chaque voie, un niveau tenu ou une tonalité à une phase de 32 bits ; elle recalcule ses pas
  quand les paramètres changent, jamais par échantillon ;
* un supersaw stéréo répartit les **mêmes** sept oscillateurs entre deux sorties : un état par voie ne suffit plus ;
* la rétro-ingénierie d'Adam Szabo (*How to Emulate the Super Saw*, 2010) donne la courbe de désaccord (polynôme de
  degré 11 sur le bouton), les écarts relatifs des sept voix à désaccord maximal (−0,110 02 à +0,107 45), les deux
  lois de mixage (centrale `−0,553 66 x + 0,997 85`, latérales `−0,737 64 x² + 1,284 1 x + 0,044 372`) et le
  passe-haut accordé sur la fondamentale ;
* PolyBLEP corrige la discontinuité de la rampe sur ±1 échantillon : 30 à 40 dB d'alias en moins (étude § 9) ;
  sa forme se réduit à `−(1 − t/dt)²` juste après le saut et `(1 − (1 − t)/dt)²` juste avant ;
* l'étage de sortie (OPA2172, pôle ≈ 72 kHz) ne filtre pas les images de reconstruction (étude § 10) : aucune
  qualité de calcul ne les supprime, un passe-bas externe reste le remède ;
* le menu tient cinq applets et `SETUP` sous une ligne vide (D-033) : sans cette ligne, il en tient six.

## 2. Options envisagées

* **Sorties** : A = somme des sept voix, B–D libres ; A et B stéréo, C sub-octave ; deux supersaws indépendants.
* **Commandes** : note, désaccord et mix séparés ; désaccord seul, le mix lié par la loi de Szabo.
* **Anti-repliement** : PolyBLEP seul, suréchantillonnage ×4 si l'écoute le demande ; ×4 d'emblée ; les deux
  commutables avec la scie naïve.
* **Forme dans `oc-core`** :
  * sept tonalités `Saw` de plus par voie (pas de lois de mixage, ni de passe-haut, ni de partage stéréo) ;
  * un nouveau canal qui porte tout le supersaw (deux bancs pour A et B, gardés en phase par construction) ;
  * un **banc d'oscillateurs partagé** par la voix, décrit une fois dans `Params`, et des canaux qui en lisent une
    part.
* **Calcul** : flottant (le M7 a une FPU simple précision) ; entier, comme le reste du signal (D-005).

## 3. Ce que l'assistant a proposé

A et B stéréo, C sub-octave ; note, désaccord et mix ; PolyBLEP seul d'abord ; un banc partagé dans la voix ; calcul
entier.

## 4. Ce que nous avons décidé, et pourquoi

Choix de l'utilisateur (2026-11-18) : **A et B en stéréo, C sub-octave**, **note, désaccord et mix séparés**,
**PolyBLEP d'abord, ×4 seulement si le repliement s'entend**. Le reste, tranché par l'assistant :

* **Banc partagé** : `Params` porte, en plus des quatre canaux, les paramètres d'un supersaw (`Supersaw` :
  fréquence, désaccord, mix, niveau crête) ; un canal `Channel::Supersaw(Part)` lit une part du banc : `Mono` (les
  sept voix), `Left`, `Right`, `Sub`. La voix calcule le banc une fois par échantillon si un canal au moins le lit.
  Un seul banc pour l'instant : un second moteur partagé (une autre applet) généralisera en un choix de moteur,
  pas en un second champ.
* **Sorties de `SUPERSAW`** : réglage `STEREO`. `OFF` (défaut) : A et B portent les sept voix, une sortie suffit.
  `ON` : la voix centrale sur A et B, les six autres réparties, une large, une moyenne et une étroite de chaque
  côté, de signes mêlés (A : −0,110, −0,020, +0,062 ; B : −0,063, +0,020, +0,107). **C** : une scie PolyBLEP à
  l'octave inférieure, sans passe-haut, au même niveau. **D** : 0 V.
* **Oscillateurs** : sept phases de 32 bits, libres (pas de remise à zéro à la note, comme le JP-8000), semées à des
  valeurs fixes réparties au démarrage de la voix : deux voix ne démarrent jamais en phase, et les tests restent
  déterministes. Fréquence de la voix `i` : `f0 (1 + écart_i × désaccord(x))`, en pas de phase.
* **Désaccord** : la courbe de Szabo en table de 33 points (16.16), interpolée ; le polynôme n'est évalué qu'en
  `f64` dans les tests, qui bornent l'écart de la table. **Mix** : les deux lois en 1.15, calculées au changement
  de paramètres.
* **PolyBLEP en entier** : la rampe naïve `phase − 1/2` en 1.15, la correction en 16.16 à partir de `t/dt`, obtenu
  par une multiplication par `2^48 / dt`, calculé au changement de paramètres : **aucune division par
  échantillon**.
* **Passe-haut** : un pôle, coupure à la fondamentale, coefficient `1 / (1 + 2π f/fs)` calculé au changement de
  paramètres, état sur 64 bits (pas de cycle limite) ; appliqué à `Mono`, `Left` et `Right`, pas à `Sub`.
* **Niveau** : la somme est divisée par `√(g_c² + n g_s²)` (n = 6 en mono, 3 par côté) : le niveau perçu ne
  dépend presque plus du mix ; les crêtes passent le niveau réglé et sont tenues aux bornes des sorties. Niveau 0
  à 3 V, défaut 1 V.
* **Applet `SUPERSAW`** (`AppId::Supersaw`, en fin d'énumération) : réglages en liste comme `RATCHET` — `NOTE`
  (C1 à C8, défaut A2), `DETUNE` (0 à 100 %, défaut 50 %), `MIX` (0 à 100 %, défaut 50 %), `LEVEL`, `STEREO` ;
  encodeur gauche, choisir ; encodeur droit, changer. CV1 en V/oct s'ajoute à la note ; CV2 au désaccord et CV3 au
  mix, +20 % par volt ; CV4 libre. Le menu perd la ligne vide au-dessus de `SETUP`.
* **Mesure** : `cargo probe stats` (`audio`) sur le module, l'écoute, et sur l'hôte un test spectral (DFT en
  `f64`) qui compare l'énergie hors harmoniques de la scie naïve et de la scie PolyBLEP.

## 5. Ce qui nous ferait changer d'avis

* Du repliement audible dans le haut du clavier : suréchantillonnage ×4 et filtre de décimation (lot 8d), dans la
  voix, pour toutes ses parts.
* Une interruption qui ne tient plus sa période : calcul par blocs (D-033 § 5).
* Des images de reconstruction gênantes : rien côté calcul, un passe-bas externe (étude § 10).
* Une sortie mono jugée trop pauvre en `STEREO ON` : mélanger une part des voix de l'autre côté.
* Un second moteur audio partagé : un choix de moteur dans `Params` à la place du champ `supersaw`.

## Révision du 2026-11-18 — entrées assignables par `oc_core::modulation` (D-036)

**Pourquoi.** Sur le module, l'écran de `SUPERSAW` « gigote » à fort désaccord, pas dans Rack. Le flux de la sonde,
entrées débranchées, montre un bruit de ±5 codes environ (CV1 −22 à +12 mV, CV2 −17 à +7, CV3 −22 à +7) : CV2 et CV3
s'ajoutaient sans seuil au désaccord et au mix (`mV / 5` millièmes), soit 997 à 1 000 sur la partie la plus raide de
la courbe de Szabo ; Rack lit exactement 0 V. Au lieu d'un seuil de présence (D-024), l'utilisateur demande que
les réglages prennent leur entrée par le sous-menu partagé de D-036, comme `RATCHET` : une entrée non choisie ne
touche rien.

**Ce qui change.**

* `NOTE`, `DETUNE`, `MIX` et `LEVEL` prennent chacun une entrée, choisie par un clic sur l'encodeur droit
  (`AssignEditor`) ; `STEREO` n'en prend pas. L'écran principal nomme l'entrée à droite de la valeur (`+CV1`,
  `*CV4`, `=CV2` seul quand elle fixe le réglage).
* **Au démarrage**, `NOTE` prend `CV1` en `ADD` (le V/oct, contrat d'un oscillateur) ; les autres aucune. Le
  bruit de `CV1` reste donc sur la note tant que l'entrée n'est pas retirée (≈ ±14 cents mesurés), question
  laissée au filtrage des entrées.
* **Effets** (plage `SET` de chaque réglage, d'où `usable_range` tire `ADD` et `MUL`) :
  * `NOTE` : `ADD` à un volt par octave sur la note du panneau ; `SET` à un volt par octave depuis C4 à 0 V
    (−3 V pour C1, +4 V pour C8) ; `MUL` multiplie la fréquence (5 V la laissent, 10 V la doublent).
  * `DETUNE`, `MIX` : 20 % par volt, de 0 % à 0 V à 100 % à 5 V pour `SET` ; ajoutés pour `ADD` ; multipliés pour
    `MUL`.
  * `LEVEL` : de 0 à 3 V sur 0 à 5 V pour `SET` ; ajouté pour `ADD` ; multiplié pour `MUL`, ce qui en fait un VCA
    piloté par une enveloppe.
* Le sous-menu montre la valeur jouée (la fréquence pour `NOTE`) et sa place sur la course ; `NOTE` y est placée
  en octaves de C1 à C8.

**Le ×4, tranché sur mesure (lot 8c′).** L'utilisateur a voulu une mesure objective plutôt que l'oreille.
L'exemple `aliasing` d'`oc-core` rend le banc réel à 47 619 Hz et sépare par FFT les harmoniques de chaque voix (son
pas exact) de ce qui s'est replié. À 1 V, de C2 à C8 : sous 10 kHz, le repli reste à −45 dB ou moins des harmoniques
(−57 dB jusqu'à C5) ; les raies repliées les plus fortes (−22 dBc à C8) sont toutes entre 20 et 23,8 kHz. Le
suréchantillonnage ×4 (lot 8d) n'est donc pas fait. La même mesure montre que la somme des voix est écrêtée à la
borne de −3 V au-delà d'environ 1,1 V de `LEVEL` : tranché par la révision suivante (polarité inversée).

**Ce qui ne change pas.** Le banc, la voix, les parts et `STEREO`. Les assignations ne sont pas sauvegardées, comme
celles de `RATCHET` (D-035).

## Révision du 2026-11-18 — scies descendantes : la polarité inversée contre l'écrêtage (lot 8c″)

**Le constat.** L'exemple `aliasing` (lot 8c′) a montré la somme des voix écrêtée bien en dessous du `LEVEL`
maximal : sur toutes les notes, désaccords et mix mesurés, 8 s chacun, le niveau le plus haut sans écrêtage était
**1,00 V** (le pire cas : C5, désaccord 100 %, mix 50 %) ; à 3 V, 2,9 % des échantillons écrêtés à C4 (mix 50 %),
6,3 % à mix 0 %. Les paires de WAV (`clean`, `clipped`, `ab`) le faisaient entendre : un timbre plus dur à mix 0 %,
du grain au rythme des battements à mix 50 %.

**Pourquoi.** Deux asymétries se rencontrent.

1. Les sorties ne sont pas centrées sur 0 V : elles vont de −3 V à +6 V.

   ```text
    +6 V ┬─────────────  ← 6 V de marge au-dessus de 0
         │
    0 V  ┼─────────────
         │  ← 3 V seulement en dessous
    −3 V ┴─────────────
   ```

2. La forme d'onde ne l'est pas non plus. Une scie montante monte lentement puis chute d'un coup ; le passe-haut
   sur la fondamentale (ce même ADR, repris du JP-8000) retire le contenu lent, la rampe, mais laisse passer le
   contenu rapide, la chute : chaque chute devient une pointe profonde, la rampe s'aplatit.

   ```text
      /|  /|  /|                ‾‾\_   ‾‾\_   ‾‾\_      un peu au-dessus de 0
     / | / | / |       →            |      |      |
    /  |/  |/  |                    ↓      ↓      ↓     pointe profonde vers le bas
    scie montante               après le passe-haut
   ```

   À 1 V de `LEVEL`, une scie seule va de +0,4 V à −1,8 V ; la somme des sept voix, au pire, de +0,8 V à −2,8 V.
   La grande pointe va vers −3 V, le côté qui n'a que 3 V de marge, et l'atteint dès 1,0 V de `LEVEL`, alors que
   plus de 5 V restent inutilisés en haut. (La somme des voix y ajoute : Σg / √(Σg²) ≈ 2,6 à mix 50 et 100 %.)

**Les options envisagées.**

* **Plafonner `LEVEL`** vers 1,1 V : aucun écrêtage, le volume constant au mix gardé, mais une sortie faible
  (≈ −13 dB en efficace sous une scie Eurorack de ±5 V).
* **`LEVEL` en crête de la somme** (diviser par Σg au lieu de √(Σg²)) : aucun écrêtage jusqu'à 3 V, mais le volume
  dépend du mix (≈ −8 dB à mix 100 % sous mix 0 %), la compensation de ce même ADR perdue.
* **Décaler la somme** vers +1,5 V (plage symétrique ±4,5 V) : +3,5 dB seulement, une composante continue sur une
  sortie audio (que le passe-haut venait d'ôter), des « tocs » à chaque changement d'applet.
* **Inverser la polarité** : des scies descendantes, dont le saut monte. Proposée par l'assistant en vérifiant les
  paires de WAV, qui montraient les crêtes : −8,4 V contre +2,4 V à 3 V.

**L'inversion.** C'est multiplier le signal par −1 : la scie descend lentement puis remonte d'un coup, et les
pointes changent de côté.

```text
      ↑      ↑      ↑     pointe vers le haut, vers +6 V
      |      |      |
  __/‾   __/‾   __/‾      un peu en dessous de 0
```

À 1 V, la somme va de −0,8 V à +2,8 V : la grande pointe a 6 V de marge au lieu de 3, la petite partie reste loin
de −3 V.

* **Pourquoi le son ne change pas.** L'oreille n'entend pas la polarité absolue d'un son périodique : l'inversion
  décale toutes les harmoniques de la même demi-période, leurs niveaux et leurs phases relatives restent
  identiques, le spectre aussi. Les tableaux de repliement et d'écart à l'idéal de l'exemple `aliasing` sont
  identiques au dB près. Une scie descendante est aussi courante qu'une montante sur les synthés.
* **La sous-octave (C) est inversée aussi** : la polarité ne compte que quand on additionne un signal à un autre en
  phase avec lui ; mélangée à A, elle garde la relation d'avant.
* **L'image** : un objet plus haut d'un côté, dans un coffre plus haut d'un côté ; on ne change pas l'objet, on le
  tourne pour que sa partie haute aille du côté où il y a de la place.

**Mesuré** (le même outil, l'ancien signe remis le temps d'un passage) :

| | avant | après |
|---|---|---|
| niveau le plus haut sans écrêtage, tous les cas | **1,00 V** | **2,01 V** (+6 dB) |
| C4, désaccord 100 %, mix 0 / 50 / 100 % | 1,72 / 1,25 / 1,23 V | 3,44 / 2,51 / 2,47 V |
| échantillons écrêtés à 3 V, C4, mix 50 % | 2,9 % | 0,05 % |
| écart à l'idéal à 3 V (C3 / C4 / C6, mix 0 %) | −14,0 / −14,2 / −15,5 dB | −30,4 / −27,3 / −21,4 dB, comme sans écrêtage |

**Décidé.** Les parts `Mono`, `Left`, `Right` et `Sub` sont des scies descendantes (`audio::supersaw`, le signe à la
sortie du passe-haut et de la sous-octave). **`LEVEL` garde son plafond de 3 V** : l'utilisateur a écouté le pire
cas d'avant après inversion (`C3-mix0-3V-ab.wav`, la moitié propre et la moitié à 3 V alternées) et le juge très
correct ; un plafond à 2 V, sans écrêtage sur tous les cas mesurés, serait une surprotection. Le prix accepté :
au-delà d'environ 2 V, de rares crêtes (0,05 % des échantillons à 3 V, C4, mix 50 %) touchent la borne de +6 V,
sans écart mesurable à l'idéal.

**Ce qui ferait changer d'avis.** Un écrêtage entendu à 3 V sur un cas que l'exemple ne mesure pas (une autre note,
une modulation rapide de `DETUNE` ou `MIX`) : plafonner `LEVEL` à 2 V. Un étage de sortie autre (une autre
fabrication aux bornes différentes) : refaire la mesure, les bornes venant de `calibration::CV_OUT_MIN_MV` et
`CV_OUT_MAX_MV`.

