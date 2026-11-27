# D-047 — Applet Melody : séquence cyclique de notes aléatoires contrôlables, gamme, accords diatoniques

| | |
|---|---|
| **Identifiant** | D-047 |
| **Date** | 2026-11-26 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | `af6ee37` (ADR, plan `docs/plan/etape-14-melodie.md`) ; implémentation `ee8914c` ; révision du 2026-11-26 : ce commit |
| **Relations** | ajoute une applet au registre de D-043 et une catégorie au menu de D-021 ; entrées et sorties routées par D-046 ; entrées CV par `oc_core::modulation` (D-036, D-045) ; sorties en millivolts tenus (D-005, D-033) ; arrondi au millivolt de la CV de hauteur laissé à D-041 ; tempo de l'horloge repris de Ratchet (D-032) |

## 1. Contexte

L'utilisateur demande (2026-11-26) une applet qui génère des mélodies, « une succession de notes aléatoires mais
contrôlables », dans l'esprit du meloDICER (Vermona), de Mimetic Digitalis (Noise Engineering) et de Marbles
(Mutable Instruments) :

* séquences cycliques de longueur réglable ;
* affichage en bas de l'écran des hauteurs des notes, avec un pointeur sur la position courante ;
* génération aléatoire selon une probabilité de remplacement ;
* le choix d'une gamme ;
* la note sur la première sortie, des accords sur les suivantes ;
* un trigger d'horloge et un trigger de reset ;
* une édition de l'affichage : un appui sur le clic gauche y entre, l'encodeur gauche choisit la note, le droit sa
  hauteur.

C'est le premier applet « séquence » : il lit des triggers et publie des CV de hauteur, quand Ratchet publie des
triggers et `TONE` / `SUPERSAW` de l'audio.

## 2. Options envisagées

* **Mémoire d'un pas** : un degré de gamme (toujours juste, mais changer de gamme décale les octaves d'une gamme de
  cinq notes à une de sept) ; ou un demi-ton au-dessus de la tonique, quantifié à la sortie (changer de gamme est
  réversible).
* **Aléa** : remplacement d'un pas à son passage avec une probabilité (Turing Machine, meloDICER) ; régénération de
  toute la séquence à chaque tour ; distribution uniforme seule, ou resserrée autour d'un centre (`SPREAD` /
  `BIAS` de Marbles) ; marche aléatoire depuis la note précédente (Mimetic Digitalis).
* **Accords** : tierces empilées dans la gamme, de type réglable ; intervalles chromatiques fixes (majeur, mineur…,
  qui sortent de la gamme) ; un décalage en degrés libre par sortie.
* **Gate** : aucune (A note, B–D accord, l'horloge sert de gate) ; D en gate au choix.
* **Transposition** : aucune ; par une entrée CV en V/oct, quantifiée sur la gamme.
* **Reset** : retour immédiat au premier pas ; ou retour armé, l'horloge suivante jouant le premier pas (convention
  Eurorack, qui évite un double premier pas quand reset et horloge arrivent ensemble).

## 3. Ce que l'assistant a proposé

Un demi-ton par pas, quantifié à la sortie ; remplacement au passage avec `PROB`, nouvelles notes dans `RANGE`
octaves ; gamme et tonique ; accords diatoniques à type réglable (`TRIAD`, `7TH`, `SUS2`, `SUS4`, `OCT`) avec
renversement ; D en gate au choix ; reset armé ; `TR1` horloge et `TR2` reset en ports d'entrée routables ; les
réglages continus assignables à une entrée CV par le sous-menu commun ; clic gauche pour entrer dans l'édition des
pas, `up` / `down` restant au panneau (D-046). En option, posées à l'utilisateur : transposition par CV, `SPREAD` /
`BIAS`, marche, absence de répétition, action `REROLL`.

## 4. Ce que nous avons décidé, et pourquoi

Les propositions, avec toutes les options (réponses de l'utilisateur, 2026-11-26) :

* **Pas** : 32 au plus, une note en demi-tons au-dessus de la tonique (0 à 48), quantifiée à la sortie au plus proche
  dans la gamme (égalité vers le bas). Une gamme se change sans perdre la séquence.
* **Aléa** : à chaque pas atteint, la note est remplacée avec la probabilité `PROB` (0 % : la boucle se répète,
  100 % : tout est neuf). Sans `JUMP`, la note est tirée dans `RANGE` octaves, uniforme à `SPREAD` 100 %, en
  triangle autour de `BIAS` en dessous ; `REPEAT OFF` refait un tirage égal à la note précédente. Avec `JUMP` 1 à 7,
  marche de ±1 à `JUMP` degrés depuis le pas précédent, jamais nulle, réfléchie aux bords. `REROLL` (panneau et
  sonde) retire toute la séquence. Générateur xorshift à graine fixe, comme Ratchet : un rejeu est identique.
* **Gamme** : `SCALE` parmi treize (chromatique, majeur, mineur, les modes, mineur harmonique, pentatoniques, blues,
  par tons), `ROOT` de C à B, `OCTAVE`, et `TRANSPOSE` en demi-tons assignable, qui prenait `CV1` en `ADD` au démarrage (retiré, voir la révision)
  (1 V = 12 demi-tons), comme `NOTE` de `SUPERSAW`.
* **Accords** : B, C, D portent des tierces empilées dans la gamme depuis le degré de A, de type `CHORD`, renversées
  par `INVERT` ; la gamme chromatique emprunte les degrés de la gamme majeure, sans quoi ses « tierces » seraient des
  secondes.
* **Gate** : `OUT D` met D en gate (5 V pendant `GATE` % de la période de l'horloge mesurée, 10 ms tant qu'elle est
  inconnue) ; accord par défaut, comme demandé.
* **Reset armé** ; horloge et reset en ports d'entrée (`CLOCK`, `RESET`), routables comme ceux de Ratchet.
* **Sorties** : niveaux tenus (`Params::levels`), changés à l'horloge seulement : un `Setup` par pas, aucun calcul
  par échantillon ; demi-tons × 1000 / 12 arrondi, soit 0,5 mV (≈ 0,6 cent) au plus, ce que D-041 tranchera.
* **Menu** : catégorie nouvelle `SEQUENCE`. `AppId::Melody` vient après `NOTHING` (index persisté 7) ; le menu
  trie par catégorie, `NOTHING` reste en bas de `SYSTEM`.
* **Partage** : le tempo de Ratchet sort dans `oc_core::tempo` plutôt que d'être importé d'une applet à l'autre.

Pourquoi : chaque choix suit la demande ou un modèle cité ; le demi-ton en mémoire garde la séquence à travers les
gammes ; des niveaux tenus ne coûtent rien au chemin audio ; tout vit dans `oc-core` et s'essaie au simulateur.

## 5. Ce qui nous ferait changer d'avis

* Une séquence qui ne « sonne » pas après un changement de gamme (notes qui se doublent par la quantification) :
  passer à une mémoire en degrés.
* Un accord qui grince au module (renversements, gamme chromatique) : intervalles libres par sortie.
* L'arrondi au millivolt audible sur des accords tenus : trancher D-041.
* Le besoin de silences ou d'une densité de gates (meloDICER) : une sortie gate par pas, au prix d'un accord à deux
  sons.
* Le coût en ITCM (`cargo xtask size`) sous l'alerte de 16 Kio : code des applets en flash (D-038).

## Révision du 2026-11-26 : la gate est un port, l'accord a deux tons

**Contexte.** `OUT D` choisissait dans l'applet ce que portait D (le troisième ton de l'accord ou la gate).
L'utilisateur l'a refusé à la relecture : décider de ce qui atteint une prise est le rôle de la page de routage
(D-046), pas d'un réglage de l'applet.

**Options.** Une route a quatre voies, une par prise (`Route`, `BasicParams`, les voix en `CV_CHANNELS`, D-044,
D-046) : la note, trois tons et une gate font cinq ports.

* quatre ports, `NOTE`, `CH1`, `CH2`, `GATE`, l'accord réduit à deux tons ;
* plus de voies que de prises (jusqu'à huit ports, la page de routage choisissant lesquels atteignent A–D) : révise
  D-044 et D-046 (`Route`, `Params`, `Voices`, `Setup`, protocole, page de routage), coût du chemin audio à mesurer ;
* pas de gate, quatre ports de hauteur.

**Proposé et décidé.** L'assistant a recommandé, et l'utilisateur a retenu, **quatre ports** : `NOTE` (A), `CH1`
(B), `CH2` (C), `GATE` (D) par défaut, chacun routable ou coupé ; `OUT D` retiré. L'accord a deux tons :
`TRIAD` tierce et quinte, `7TH` tierce et septième (sans la quinte), `SUS2` seconde et quinte, `SUS4` quarte et
quinte, `OCT` une octave en dessous et une au-dessus ; `INVERT` 0 ou 1 (le ton grave à l'octave). `GATE` règle
toujours la longueur. Aucun changement d'architecture.

Dans le même temps : un reset arme sans toucher aux sorties, la note jouée tient jusqu'à l'horloge suivante (avant,
les sorties sautaient au premier pas dès le reset) ; l'applet a son guide, `crates/oc-core/src/apps/melody/README.md`.

**Ce qui ferait changer d'avis.** Un accord à trois tons réclamé avec la gate : plus de voies que de prises, la
deuxième option, à décider comme une révision de D-046.

Toujours le 2026-11-26, à la relecture : **aucune entrée au démarrage**. `TRANSPOSE` prenait `CV1` en `ADD`, comme
`NOTE` de `SUPERSAW` ; l'utilisateur veut un réglage modulable comme les autres, sans entrée tant que le sous-menu
ne lui en donne pas. Un `CV1` branché pour une autre app ne transpose donc pas `MELODY` par surprise.

Puis, le même jour, **trois pages de réglages au plus**, demandées par l'utilisateur. Quinze lignes sur quatre pages
deviennent douze sur trois :

* **`REPEAT` dans `JUMP`** : `JUMP` vaut `OFF` (tirage libre), `NO REP` (tirage sans répétition du pas précédent)
  ou 1 à 7 (marche, qui ne répète jamais) ; `REPEAT` n'avait de sens que sans marche. Pendant une marche, les
  valeurs de `SPREAD` et `BIAS` sont masquées ; `JUMP` passe devant eux, sur leur page.
* **Deux lignes à deux champs** : `KEY` (la tonique puis la gamme) et `CHORD` (l'accord puis son inversion).
  L'encodeur gauche parcourt les champs un à un, même sur une même ligne ; l'encodeur droit change le champ choisi,
  seul encadré (choix de l'utilisateur, contre un clic droit qui changerait de champ).
* **L'inversion en flèche, à trois états** : ↑ monte le ton grave d'une octave, ↓ descend le ton aigu, un tiret
  laisse l'accord serré (`INVERT` de −1 à +1 pour la sonde).

Les paramètres de la sonde restent un par champ (`ROOT`, `SCALE`, `CHORD`, `INVERT`, `JUMP` par ses noms), dans
l'ordre des champs ; les emplacements des entrées CV ne bougent pas.

## Révision du 2026-11-27 : une boucle qui se déforme sans se perdre

**Statut de cette révision** : décidée le 2026-11-27, implémentée dans le commit qui suit celui de cette section
(`melody/window.rs`, `melody/sequence.rs`).

**Contexte.** La comparaison avec le Gamut Repetitor de Noise Engineering (manuel officiel, relu le 2026-11-27) a
montré la différence qui compte le plus : sur le GR, une boucle verrouillée se resserre, s'étire ou glisse quand on
tourne `Spread` ou `Down/Up`, en direct, sans perdre sa forme. Dans `MELODY`, un pas garde des demi-tons fixes au-dessus
de la tonique : `RANGE`, `SPREAD` et `BIAS` ne jouent que sur les tirages suivants, à retardement, et l'étendue se règle
à l'octave, au-dessus de la tonique seulement. Le centrage du GR (`Spread` 24, `Down/Up` au milieu) ne se reproduit
qu'à peu près (`RANGE 2`, `OCTAVE −1`, `BIAS 50 %`), à l'octave près, et pas pour une étendue impaire.

**Options.**

* garder le modèle en demi-tons et ajouter un décalage de fenêtre en demi-tons : le centrage, mais toujours rien en
  direct sur la boucle ;
* **garder par pas une position normalisée dans une fenêtre**, projetée à la sortie : tout réglage de la fenêtre agit
  en direct, et une projection monotone garde le contour ;
* en plus de la fenêtre en direct, garder une concentration au tirage (l'actuel `SPREAD`, façon Marbles).

**Proposé par l'assistant et décidé par l'utilisateur : la position normalisée, sans concentration au tirage.**

* **Ce qu'un pas garde** : `u`, une position de 0 à 1 dans la fenêtre (16 bits), tirée une fois, uniformément ; elle
  ne change qu'à un remplacement (`PROB`), un `REROLL` ou une édition. Les 32 pas prennent 64 o au lieu de 32
  (`MelodyApp` ≈ 330 o, chaque emplacement grandit d'autant ; à remesurer, D-038).
* **La projection, à la sortie**, réglage par réglage, chacun en direct et monotone, si bien qu'une note plus haute
  qu'une autre le reste et que le contour de la mélodie survit à tout réglage :
  1. `BIAS` courbe `u` ;
  2. `SPREAD` et `CENTER` placent le résultat dans une fenêtre de hauteurs autour de la tonique ;
  3. `TRANSPOSE`, puis la quantification dans la gamme ; `ROOT` et `OCTAVE` placent le tout.
* **`SPREAD`, la largeur** : de 0 à 48 demi-tons, la taille de la fenêtre ; la mélodie s'étire ou se resserre, à 0
  elle tient sur une seule note. C'est le `Spread` du GR (0 à 24), au demi-ton près. Il remplace `RANGE`, qui
  disparaît. Par défaut 24 (deux octaves, comme `RANGE 2`), `CENTER` et `BIAS` à 0.
* **`CENTER`, la position** : de −100 % à +100 %, où la fenêtre se place par rapport à la tonique : à −100 % elle
  finit sur la tonique, à 0 elle est centrée dessus, à +100 % elle commence dessus. La mélodie glisse sans changer de
  forme ; c'est le `Down/Up` du GR. La tonique au centre de la fenêtre devient le réglage par défaut.
* **`BIAS`, l'inclinaison** : de −100 % à +100 %, une courbe appliquée à `u`, `u' = u^(2^(−k·b))`, `b` de −1 à +1,
  `k` de l'ordre de 2 (exposant de 1/4 à 4), sur le modèle du `SHAPE` signé de `RATCHET` (D-032). À 0 la projection
  est linéaire ; vers + les notes se tassent vers le haut de la fenêtre, la mélodie passe plus de temps dans l'aigu ;
  vers − dans le grave. Les bornes restent fixes (0 → 0, 1 → 1) : `BIAS` ne rétrécit jamais l'étendue, il redistribue
  les notes à l'intérieur, ce qui le distingue de `SPREAD` et de `CENTER`. Son sens change donc : il ne dit plus où
  tombent les prochains tirages, il déforme toutes les notes en direct.
* **L'ancien `SPREAD`, la concentration au tirage, disparaît** : il recréait le comportement à retardement qu'on
  supprime, et resserrer autour du centre est désormais réduire la largeur ; les tirages sont uniformes en `u`, comme
  sur le GR.
* **`JUMP`** : une marche de ±1 à 7 degrés depuis la note *jouée* du pas précédent, convertie en `u` par la
  projection courante, inversée (la courbe de `BIAS` est inversible, la quantification par le milieu de la note
  d'arrivée), réfléchie aux bords de la fenêtre. Tant qu'on ne touche pas à la fenêtre, c'est une marche en degrés ; si
  on la resserre ensuite, les pas se resserrent avec elle, comme les autres. `NO REP` compare les notes jouées.
* **L'édition au panneau** : l'encodeur droit monte ou descend d'un degré la note *affichée*, et le `u` du pas est
  recalculé par la projection inversée ; à `SPREAD 0`, où toute la fenêtre est une note, l'édition ne fait rien.
* **Plusieurs pas sur une même note** : en resserrant la fenêtre, des pas voisins en hauteur tombent sur la même
  note de la gamme ; en la ré-étirant, ils se séparent de nouveau, `u` n'ayant rien perdu. C'est voulu, comme sur le
  GR.
* **Ce qui ne change pas** : `ROOT`, `SCALE`, `OCTAVE`, les accords (empilés sur la note jouée), la gate, l'horloge,
  le reset armé, `PROB`, `REROLL`, `LENGTH`. Les sorties restent des niveaux tenus ; la projection se calcule à
  chaque tick pour le pas joué, pas par échantillon, et les marques de l'écran suivent la projection (les hauteurs
  jouées).
* **Entrées CV** : `SPREAD` (12 demi-tons par volt), `CENTER` et `BIAS` (20 % par volt) sont assignables, comme
  `RANGE`, `SPREAD` et `BIAS` l'étaient ; ils agissent en direct sur la boucle, plus seulement à l'horloge. Les
  emplacements des entrées gardent leur rang (`SPREAD` prend celui de `RANGE`, `CENTER` celui de l'ancien `SPREAD`).
* **Pages de réglages** : le compte de lignes ne change pas (`SPREAD`, `CENTER`, `BIAS` à la place de `RANGE`,
  `SPREAD`, `BIAS`). Le dépassement des pages qui viendrait d'autres ajouts sera traité à part : les réglages
  secondaires passeront sur une page d'options de l'app, ouverte par un appui long sur l'encodeur droit (décision à
  venir, hors de cette révision).

**Ce qui ferait changer d'avis.**

* Des boucles qui sonnent pauvres une fois resserrées (trop de pas sur la même note) : garder aussi une
  concentration au tirage, la troisième option.
* Une marche (`JUMP`) qui dérive en degrés après des réglages de fenêtre et gêne à l'usage : la garder en degrés
  absolus, hors de la projection.
* Le surcoût de 32 o par pas (×4 emplacements) qui compterait dans le budget de D-038 : `u` sur 8 bits (256 positions,
  ≈ 0,19 demi-ton à 48 demi-tons de largeur).

## Révision du 2026-11-27 (suite) : un rythme aléatoire qui boucle

**Contexte.** `MELODY` déclenche une gate à chaque pas d'horloge. Le Gamut Repetitor génère des rythmes aléatoires
répétés sur ses sorties de triggers ; le meloDICER a une section rythme (silences, liaisons) tirée à part de la
mélodie (manuels relus le 2026-11-27). C'était le premier manque relevé face aux deux.

**Décidé (réponses de l'utilisateur, détails proposés par l'assistant).**

* **Ce qu'un pas garde en plus** : deux valeurs de 16 bits tirées une fois, comme sa place : `r`, son rang de jeu, et
  `l`, son rang de liaison. Elles bouclent avec la séquence.
* **`DENSITY`** (0 à 100 %, 100 % par défaut, le comportement d'avant) : un pas joue si `r` est sous le seuil de la
  densité. Le seuil se compare en direct, comme la fenêtre : monter la densité ajoute des pas, la baisser en retire,
  toujours les mêmes et dans le même ordre ; assignable à une entrée CV (20 % par volt), qui agit tout de suite.
* **`LEGATO`** (0 à 100 %, 0 par défaut) : un pas joué dont `l` est sous le seuil se lie au pas joué qui le précède :
  sa note change sans nouvelle gate, la gate de l'autre tenue jusqu'à lui. Un pas lié après un silence joue
  normalement. Une gate qui suit une gate tenue sans liaison commence par un tick bas, pour que l'enveloppe voie le
  front.
* **Un pas silencieux** : la gate ne s'ouvre pas, et `NOTE` et l'accord gardent la dernière note jouée, qui suit
  toujours la fenêtre en direct ; le pointeur, lui, avance.
* **Un tirage à part, comme le meloDICER** : `PROB` et `REROLL` ne tirent plus que les places ; le rythme a les
  siens, une chance de nouveau tirage au passage (`RHYTHM`, 0 % par défaut) et un `REROLL` du rythme seul. Un
  rythme figé sous une mélodie qui change, ou l'inverse. L'action de sonde `REROLL` reste celle des notes ; une
  action `RHYTHM` tire le rythme.
* **L'éditeur des pas** : un clic droit court bascule la gate du pas encadré (son `r` à 0, joué dès que la densité
  dépasse 0 %, ou au plus haut, silencieux même à 100 %) ; un appui long sur l'encodeur droit tire une nouvelle note
  au hasard pour ce seul pas, comme le clic droit le faisait. Hors de l'éditeur, l'appui long droit reste réservé à
  la future page d'options.
* **L'écran** : un pas silencieux garde sa marque de hauteur, en pointillés ; un pas lié est relié à la marque du
  précédent.
* **Pages** : `DENSITY`, `LEGATO`, `RHYTHM` et le `REROLL` du rythme ajoutent des réglages ; le dépassement des trois
  pages sera repris par la page d'options (hors de cette révision).

**Ce qui ferait changer d'avis.** Une liaison qui sonne mal avec des gates courtes (`GATE` bas) : la tenir à la
longueur de `GATE` au lieu de l'étendre au pas suivant. Un rythme manuel écrasé par `DENSITY 0 %` qui gêne : un
verrou par pas, hors des seuils.
