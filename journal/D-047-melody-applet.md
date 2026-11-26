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
  par tons), `ROOT` de C à B, `OCTAVE`, et `TRANSPOSE` en demi-tons assignable, qui prend `CV1` en `ADD` au démarrage
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
