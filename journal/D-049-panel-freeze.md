# D-049 — Freeze : éditer une app sans publier, puis tout appliquer d'un coup

| | |
|---|---|
| **Identifiant** | D-049 |
| **Date** | 2026-11-27 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ADR et implémentation : ce commit |
| **Relations** | geste au panneau de l'app au focus (D-046) ; reprend l'appui long de l'encodeur droit, réservé à la page d'options (lot 14q), qui ne s'ouvrira plus que par `down` ; applique par le modèle de paramètres de D-043 et les assignations de D-045 ; l'éditeur de pas de `MELODY` (D-047) garde son appui long |

## 1. Contexte

L'utilisateur demande (2026-11-27) un geste : un appui long sur l'encodeur droit fait passer en **freeze** ; on peut
alors parcourir les réglages et en changer les valeurs sans qu'ils soient publiés ; ils le sont tous au relâchement du
prochain appui long sur l'encodeur droit. Changer plusieurs réglages à la fois sur un son qui tourne, sans entendre
les états intermédiaires.

Deux contraintes du panneau : l'encodeur droit agit aujourd'hui **à l'appui** (sous-menu d'entrée, `REROLL`), donc un
appui long déclencherait d'abord le clic ; et l'appui long droit était réservé à la page d'options (lot 14q) et sert,
dans l'éditeur de pas de `MELODY`, à tirer une nouvelle note.

## 2. Options envisagées

* **Portée** : l'app au focus ; ou tout le module, les quatre apps figées ensemble, la navigation entre elles permise.
* **Mécanisme** :
  * une copie de l'app (son `Instance`) que le panneau édite pendant que l'originale joue, ses changements reportés
    au relâchement ;
  * une mise en attente dans chaque app (des réglages « en cours » à côté des réglages joués), à écrire app par app ;
  * le moteur qui retiendrait les `set` du panneau : impossible, le panneau agit dans l'app, pas par le modèle de
    paramètres.
* **Ce qui est retenu** : les valeurs des réglages, les assignations CV, les pas édités de `MELODY`, les actions.
* **Le clic droit** : au relâchement d'un appui court, ou toujours à l'appui, l'appui long venant en plus.

## 3. Ce que l'assistant a proposé

L'app au focus seule, changer d'app (et ouvrir le menu ou la page de routage) bloqué pendant le freeze ; une copie de
l'instance éditée par le panneau, reportée au relâchement par le modèle de paramètres (`get` / `set`) et les
assignations, plus ce que chaque app sait seule reporter ; le clic droit au relâchement d'un appui court là où l'appui
long sert ; le freeze hors de l'éditeur de pas, la page d'options par `down` seulement.

## 4. Ce que nous avons décidé, et pourquoi

Les propositions, et les pas édités de `MELODY` retenus aussi (réponses de l'utilisateur, 2026-11-27) :

* **Geste** : l'appui long droit (1 s) entre en freeze dès qu'il se déclenche ; le relâchement du **prochain** appui
  long droit applique et sort. Pas dans l'éditeur de pas de `MELODY`, où l'appui long tire toujours une nouvelle note
  (`App::takes_long_right`) ; la page d'options (lot 14q) s'ouvrira par l'appui long de `down` seulement.
* **Clic droit au relâchement** d'un appui court (`ButtonEvents::tapped`) dans les apps et le sous-menu d'entrée ; le
  menu, l'écran `SETUP` et l'assistant, sans freeze, gardent le clic à l'appui.
* **Copie d'édition** : `AppHost` garde une copie de l'instance au focus (`Instance`, la taille d'un emplacement).
  L'originale continue de jouer, sans le panneau, comme une app sans focus ; la copie reçoit le panneau et les CV, mais
  ni trigger ni temps (aucune horloge, aucun pas tiré) ; l'écran montre la copie, la barre de titre en inverse vidéo.
* **Report au relâchement** (`AppHost::apply_freeze`) : chaque paramètre dont la valeur diffère est posé sur
  l'originale par `App::set`, dans l'ordre, deux passes pour les bornes qu'un autre réglage déplace ; les assignations
  sont recopiées ; puis `App::adopt` reporte ce que l'app seule connaît : la sélection et le sous-menu, et, pour
  `MELODY`, les places et les rythmes des pas (sa position et son tirage restent ceux de l'originale).
* **Actions** : faites sur la copie, elles ne valent que par ce qu'elles y changent : un `REROLL` de `MELODY` tire
  les pas de la copie, publiés avec eux ; une rafale à la main de `RATCHET` n'atteint aucune sortie.
* **Pendant le freeze** : `up` / `down` court, le menu et la page de routage sont sans effet ; la sonde agit sur
  l'originale (un réglage qu'elle change et que la copie a changé aussi prend, au relâchement, la valeur de la copie).

Pourquoi : une copie ne demande rien aux apps pour figer et réutilise le chemin de la sonde pour appliquer, déjà testé ;
seul `adopt` est propre à chaque app. Une app au focus, une copie : la mémoire d'un emplacement de plus.

## 5. Ce qui nous ferait changer d'avis

* Le besoin de figer plusieurs apps ensemble (un accord de réglages entre `MELODY` et `SUPERSAW`) : une copie par
  emplacement, la navigation permise.
* Une action attendue immédiate pendant le freeze (une rafale à la main) : la transmettre à l'originale.
* La pile ou la DTCM (`cargo xtask size`) : la copie construite en place plutôt que clonée sur la pile.
* Le clic au relâchement ressenti comme lent : un appui long plus court, ou un autre geste pour le freeze.
