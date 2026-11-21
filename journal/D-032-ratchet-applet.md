# D-032 — Applet Ratchet : rafales de triggers, sorties trigger sur le DAC, entrées au choix

| | |
|---|---|
| **Identifiant** | D-032 |
| **Date** | 2026-11-17 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR, plan) ; implémentation dans le commit suivant (`docs/plan/etape-6-ratchet.md`) |
| **Relations** | ajoute une applet à D-021 ; premier usage musical de C+ (D-023 : 1 kHz, fronts capturés D-019) ; sorties par la calibration de D-027 ; écran par `gfx` / `text` (D-030) ; pas de flottant (D-005) |

## 1. Contexte

Premier applet musical demandé par l'utilisateur (2026-11-17) : un **ratchet**. Un trigger arrive sur TR1 ; il est
réémis, et suivi de quelques triggers dont le nombre (ou « tant que le gate est haut ») et la répartition dans le
temps se paramètrent, avec une probabilité, une synchronisation sur le tempo de TR1 et des entrées CV au choix.

Il pose des conventions qui serviront aux applets suivantes : niveau et largeur d'un trigger émis par le DAC, forme
des réglages et de leurs entrées, pages de réglages, quantification au tick.

## 2. Options envisagées

* **Sorties** : A copie de TR1 et B rafale ; une sortie de fin de rafale en plus ; A et B identiques.
* **Répartition des répétitions** :
  * un premier intervalle et un *rapport* du dernier au premier, la forme (constante, linéaire, exponentielle,
    logarithmique) ne pliant que la courbe entre ces deux bornes : essayé, écarté, les formes s'entendaient à peine
    (mêmes bornes, même sens, une quinzaine de ms d'écart) ;
  * une forme qui donne le sens (accélère, ralentit) et une intensité : retenu, sous la forme d'une seule valeur
    signée ;
  * deux placements : à partir d'un premier intervalle, ou dans une durée de rafale.
* **Entrées CV** : modulation figée (CV1 → nombre, CV2 → intervalle, CV3 → rapport) ; ou une entrée au choix par
  réglage, qui suit l'entrée ou s'y ajoute.
* **Nouveau front pendant une rafale** : redémarre, ou ignoré jusqu'à la fin.

## 3. Ce que l'assistant a proposé

A copie, B rafale ; redémarrage ; puis, au fil des essais demandés par l'utilisateur : la forme signée, les deux
placements à comparer en situation, l'entrée au choix par réglage, le tempo de TR1 et la synchronisation.

## 4. Ce que nous avons décidé, et pourquoi

Choix de l'utilisateur (2026-11-17), avec les détails tranchés par l'assistant :

* **Sorties** : **A**, une impulsion par front de TR1 ; **B**, la rafale, le trigger d'entrée recopié en tête si
  `PASSTHRU ON` (défaut). C et D à 0 V. Trigger : 5 V pendant 5 ms par la calibration (D-027), la moitié de
  l'intervalle si les répétitions se suivent de plus près (au moins 1 ms).
* **Un nouveau front redémarre la rafale.** Les réglages, entrées comprises, sont lus au front et tiennent toute la
  rafale.
* **Réglages**, page 1 : `COUNT`, `FIRST` ou `SPAN`, `SHAPE`, `PROB` ; page 2 : `PASSTHRU`, `SYNC`, `MODE`. Encodeur
  gauche : choisir le réglage (l'écran montre sa page ; une flèche ▲ / ▼ dans la barre de titre indique l'autre
  page) ; encodeur droit : le changer ; `up` : une rafale à la main.
  * `COUNT` : 1 à 32 répétitions, ou `GATE` (en tournant au-delà de 32) : des répétitions tant que TR1 reste haut,
    au plus 64.
  * `SHAPE` : une valeur signée, −3,0 à +3,0 par pas de 0,1, un codage abstrait et non une unité : d'une courbe
    **exponentielle** à −3,0, par une **droite** à 0 (**uniforme**), à une courbe **logarithmique** à +3,0 ; en
    dessous de 0 la rafale **accélère**, au-dessus elle **ralentit** ; la valeur absolue est l'intensité. Chaque
    modèle la lit à sa façon (exposant du dernier facteur pour `FIRST`, moitié de la courbure `c` pour `SPAN`).
  * `MODE` choisit le placement, deux modèles gardés côte à côte pour être comparés en situation :
    * `FIRST` : un premier intervalle, puis des intervalles qui varient géométriquement jusqu'à
      `FIRST × 2^SHAPE` pour le dernier ;
    * `SPAN` : les N répétitions aux positions `k / (N + 1)` d'une durée `SPAN`, pliées par
      `(2^(c x) − 1) / (2^c − 1)`, `c = 2 |SHAPE|` (ralentit) ou son miroir (accélère) ; avec `SYNC`, `SPAN ×1`
      remplit exactement le pas jusqu'au trigger suivant. Une rafale `GATE` s'y étale sur 8 répétitions de
      référence puis garde son dernier intervalle.
  * `PROB` : la chance de chaque répétition de B, 1 à 100 % ; ni A ni le trigger recopié n'y sont soumis ; une
    répétition qui ne part pas laisse son temps s'écouler, les suivantes restent à leur place ; tirage xorshift
    semé à l'identique à chaque démarrage (une même séquence d'entrées rejoue la même rafale).
  * `SYNC ON` : `FIRST` ou `SPAN` devient une fraction de la période de TR1 : ×2, ×1.5, ×4/3, ×1, ×3/4, ×2/3,
    /2, /3, /4, /6, /8, /12, /16, /24, /32 (défaut /4) ; sans tempo connu, la valeur en ms.
* **Tempo de TR1** : moyenne des 4 derniers intervalles entre fronts montants, un trigger comptant pour un temps,
  oublié après 2 s sans front ; affiché en BPM dans la barre de titre (`--- BPM` inconnu). Sur la frise de la
  rafale, une ligne pointillée marque l'instant où le prochain trigger est attendu ; les répétitions au-delà, que le
  redémarrage couperait, sont dessinées courtes.
* **Entrées** : `COUNT`, `FIRST` / `SPAN`, `SHAPE` et `PROB` prennent chacun `--` (le panneau), `CV1=`..`CV4=`
  (le réglage **suit** l'entrée) ou `CV1+`..`CV4+` (l'entrée **s'ajoute** à la valeur du panneau) ; un clic sur
  l'encodeur droit bascule ce qu'il tourne, la valeur ou l'entrée. Correspondances : nombre +4 par volt (1 à 0 V
  quand il suit ; `GATE` reste `GATE` quand on ajoute) ; temps 1 V/oct sur la cadence (100 ms pour `FIRST`, 400 ms
  pour `SPAN` à 0 V quand il suit) ; `SHAPE` +1,0 par volt (0 à 0 V) ; `PROB` +20 % par volt (0 % à 0 V).
* **Temps** : une répétition tombe au premier tick qui atteint son heure, le reste reporté sur la suivante : les
  intervalles se quantifient au tick (1 ms) sans dériver. Intervalles bornés à 2 ms – 2 s, `SPAN` à 8 s.
* **Calcul entier** : `log2` et `exp2` en virgule fixe 16.16 (`oc_core::fixed`, tables de 65 points), à quelques
  unités près de `f64` (`tests/fixed.rs`) ; `oc-core` reste sans flottant.
* **Écran** : réglages de la page en liste (le réglage choisi en vidéo inverse, son entrée dans une case sombre
  quand on la choisit), témoins `1`, `A`, `B`, BPM, frise de la rafale sur les trois lignes du bas.

## 5. Ce qui nous ferait changer d'avis

* L'essai en situation départagera `MODE FIRST` et `SPAN` : garder l'un, les deux, ou les fondre.
* Des rafales qui sonnent irrégulières à cause de la quantification à 1 ms (intervalles courts, formes très
  accélérées) : cadence plus fine pour le chemin trigger (D-023, option A).
* Une largeur d'impulsion que des modules en aval ne voient pas : la rendre réglable.
* Une modulation qui demande d'agir pendant la rafale (pas seulement au front) : échantillonnage continu.

## Révision du 2026-11-18 — essai sur le module (lot 6d)

L'utilisateur a essayé `RATCHET` sur le module, désormais sur le chemin audio (sorties par DMA, D-033) : correct.
`MODE FIRST` et `SPAN` sont **gardés tous les deux** pour l'instant ; le choix reste ouvert (§ 5).

## Révision du 2026-11-18 — écran : ascenseur, frise de la rafale en cours

Demandée par l'utilisateur après l'essai sur le module. La décision ne change pas, seulement l'écran (§ 4,
« Écran » et l'encodeur gauche) :

* la flèche ▲ / ▼ de la barre de titre est remplacée par un **ascenseur** à gauche des réglages : un rail
  pointillé sur la hauteur des quatre lignes, un curseur plein sur la part de la page affichée ; les lignes se
  décalent de 4 pixels à droite ;
* la **frise** est réduite à 15 pixels (lignes 48 à 63) ; les pages gardent quatre réglages, les options restent
  en page 2 ;
* **pendant une rafale**, la frise montre la rafale en cours (ses réglages lus au front) et non celle que lancerait
  un front maintenant : une tête de lecture ▼ au-dessus suit le temps écoulé, les triggers partis sur B sont en
  trait épais, les répétitions écartées par `PROB` réduites à un point ; hors rafale, l'aperçu reste celui des
  réglages courants.

## Révision du 2026-11-18 — entrées dans un sous-menu (D-035)

Le choix d'entrée au clic (`--`, `CVn=`, `CVn+` à l'encodeur droit) est remplacé par un sous-menu d'assignation :
entrée, lecture en direct et bornes vues, échelle, décalage, bornes réglables, effet `SET` / `ADD` / `MUL`. Les
correspondances de ce § 4 restent celles de `SET` (l'ancien `=`) et `ADD` (l'ancien `+`). Voir D-035.

## Révision du 2026-11-21 — `SHAPE` n'est pas en octaves

Correction de l'utilisateur, à l'essai du modèle de paramètres (D-043) qui affichait `SHAPE` en `OCT` : `SHAPE` est
un codage abstrait, de l'exponentiel au logarithmique en passant par le linéaire à 0, pas une grandeur en octaves ;
le réglage de temps (`FIRST` ou `SPAN`) est une durée en millisecondes ou, avec `SYNC ON`, une fraction de la
période de TR1 (le dernier intervalle de `FIRST` valant `FIRST × 2^SHAPE`). Le comportement ne change pas : la
description ci-dessus, le README, la documentation de `ratchet` et `burst` et le modèle de paramètres (sans unité,
avec son signe) disent désormais cela. L'entrée CV du temps le divise toujours par deux à chaque volt.
