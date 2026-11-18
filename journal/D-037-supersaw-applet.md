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
borne de −3 V au-delà d'environ 1,1 V de `LEVEL` : question laissée ouverte.

**Ce qui ne change pas.** Le banc, la voix, les parts et `STEREO`. Les assignations ne sont pas sauvegardées, comme
celles de `RATCHET` (D-035).

