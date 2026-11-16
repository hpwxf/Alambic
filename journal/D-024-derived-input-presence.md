# D-024 — Présence d'une entrée déduite du signal : un mécanisme, deux réglages, une surcharge à venir

| | |
|---|---|
| **Identifiant** | D-024 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR) ; implémentation dans le commit suivant (`oc_core::presence`) |
| **Relations** | **complète** D-019 (point « activité » : `SignalDetector` en est une source) ; **respecte** D-019 (aucune présence de câble fournie par la plateforme) ; sert D-002 (même comportement sur les trois backends) |

## 1. Contexte

La révision R5 de D-019 a retiré `AnalogIn::is_patched` : le module ne peut pas détecter un câble (jacks sans
contact relié au microcontrôleur), et la valeur que fournissait Rack (`isConnected()`) faisait diverger les backends.
Le besoin reste : un applet veut savoir si une entrée « porte quelque chose » pour prendre une **valeur par défaut**
sinon (une horloge interne faute d'horloge externe, un paramètre qui revient à son réglage, etc.).

Contraintes posées par l'utilisateur : comportement **portable** entre module, simulateur et Rack ; **aucune
modification matérielle**, même sur une broche libre — le module TLM n'est qu'un support d'exemple, le firmware doit
tourner sur tout O&C.

Ce que font les firmwares de référence (code lu le 2026-11-16, Phazerville `ebabdaa`, Squares-and-Circles
`dbbd707`) : aucun ne détecte de câble par le matériel. Trois idiomes logiciels :

* **Phazerville, `Calculate.h`** : un canal Rand est « non cadencé » jusqu'au premier front d'horloge, puis le reste
  (`rand_clocked[ch] = 1`) ; le canal 2 reprend l'horloge de TR1 tant qu'il n'a pas la sienne (normalisation
  logicielle) ;
* **Squares-and-Circles, `hasTrig()` / `hasAccent()`** : l'entrée est « branchée » si l'utilisateur l'a assignée dans
  la page I/O-Configuration (routage déclaré, mémorisé) ; Braids utilise alors le Decay comme VCA, Plaits passe en
  `TRIGGER_UNPATCHED` ;
* **Squares-and-Circles, Rings** : l'entrée compte comme branchée si son énergie dépasse −50 dBFS dans les 100
  dernières ms (« There are no jacks here ») — activité avec maintien.

## 2. Options envisagées

**A** — Rétablir `is_patched` en trois états (`Patched` / `Unpatched` / `Unknown`), le module répondant `Unknown`.
Garde l'information exacte de Rack, mais le module n'utilise jamais la valeur par défaut : le comportement testé sur
l'hôte n'est pas celui du module.

**B** — Laisser chaque applet choisir et coder son idiome parmi les trois. Souple, mais chaque applet a sa propre
notion de « branché » : l'utilisateur ne peut plus prévoir quand un défaut s'applique, et chaque variante se teste à
part.

**C** — **Un mécanisme unique dans `oc-core`** : *activité récente avec maintien*. Une entrée est présente si elle a
montré une preuve de signal depuis moins de T. Le premier front de Phazerville en est le cas T infini. Deux réglages
fixés, au choix de l'applet dans une **liste fermée** (CV ; trigger en verrou ou en horloge). Le routage déclaré de
Squares-and-Circles devient plus tard une **surcharge** (forcer présente / absente depuis un menu), pas une troisième
détection.

**D** — Ne rien fournir : convention « débranché = 0 V », chaque paramètre = réglage + CV. Couvre les défauts
additifs, pas un défaut qui *remplace* la valeur (horloge interne, source interne).

## 3. Ce que l'assistant a proposé

**C**, avec D comme règle de conception complémentaire (choisir, quand c'est possible, un défaut proche de ce que
donnerait 0 V, pour que l'erreur résiduelle soit inoffensive), et l'exclusion explicite de toute source qu'un seul
backend possède.

## 4. Ce que nous avons décidé, et pourquoi

**C**, implémenté dans `oc_core::presence` :

* **`CvPresence`** — preuve = activité de `SignalDetector` (crête-à-crête ≥ 40 mV, D-019) **ou** niveau à plus de
  `CV_LEVEL_THRESHOLD_MV` = 100 mV de 0 V ; maintien `CV_HOLD_MICROS` = 300 ms. Le maintien évite de basculer quand
  un LFO traverse 0 V ; 100 mV dépasse l'erreur de décalage attendue d'une calibration provisoire (D-006) tout en
  restant musicalement négligeable (un peu plus d'un demi-ton).
* **`TriggerPresence`** — preuve = front montant ; deux réglages : `latch()` (présente dès le premier front, jusqu'à
  `reset()`, idiome Phazerville) et `clock()` (maintien `CLOCK_HOLD_MICROS` = 2 s : une horloge arrêtée redevient
  absente).
* Le détecteur générique (maintien, verrou) est **privé** au module : un applet ne peut prendre qu'un des trois
  réglages, il ne peut pas en inventer un.
* Le temps est compté en **microsecondes écoulées** (`InputSnapshot::elapsed_micros`), pas en ticks, pour que les
  durées tiennent si la cadence change (D-023).
* Tout est calculé dans `oc-core` à partir des seuls niveaux lus : module, simulateur et Rack donnent le même
  résultat pour les mêmes niveaux. **`isConnected()` de Rack reste inutilisé**, et aucun backend ne peut fournir de
  présence (le trait `AnalogIn` n'en a pas, D-019).
* `DiagnosticApp` affiche la présence CV (`P` / `.`), ce qui rend l'état visible et vérifiable au niveau 9.

Limite assumée : une entrée branchée sur exactement 0 V fixe (à 100 mV près) est vue absente. Les deux règles
d'accompagnement : défaut proche de 0 V quand c'est possible ; état présent / absent visible à l'écran de l'applet
qui s'en sert.

Différé : la surcharge par routage déclaré, qui attend des menus de réglage et la persistance (D-006, revue V1). Elle
s'ajoutera par-dessus la détection, sans en changer la définition.

## 5. Ce qui nous ferait changer d'avis

* Au niveau 9, une entrée débranchée lue au-delà de 100 mV (décalage de calibration plus grand que prévu) : relever le
  seuil ou attendre la calibration mesurée, plutôt qu'inventer une autre détection.
* Un applet réel qu'aucun des trois réglages ne sert : ajouter un réglage à la liste fermée (révision de cette
  entrée), pas un idiome local.
* Une carte O&C dont les contacts de jack seraient reliés au microcontrôleur : elle pourrait fournir une preuve de
  plus, à condition que les autres backends aient un équivalent ; sinon, ce serait une divergence (D-002).

## Révision (2026-11-16, étape 4, lot 4f) — seuil CV mesuré : 50 mV

Le seuil de 100 mV couvrait l'erreur de la calibration provisoire (zéro d'entrée à mi-échelle, faux : D-027). Mesure
sur le module TLM calibré par l'assistant (avancement de l'étape 4) : entrées débranchées à ±10 mV de 0 V au pire,
17 mV crête à crête au pire, aucun `~` au repos. C'est la case « quelques mV, pas de `~` » du tableau de décision
préparé avant la mesure : `CV_LEVEL_THRESHOLD_MV` passe à **50 mV**, environ trois fois la pire excursion au repos,
six dixièmes de demi-ton à 1 V/oct. Maintien inchangé (300 ms). À revoir si un autre module, non calibré ou plus bruité,
lit `P` débranché.

