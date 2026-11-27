# D-050 — Sauvegarder et restaurer le patch du module par la sonde, dans un fichier TOML

| | |
|---|---|
| **Identifiant** | D-050 |
| **Date** | 2026-11-27 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ADR et implémentation : ce commit |
| **Relations** | lit et écrit par la sonde (D-029) et son modèle d'apps (D-043, D-045, D-046, D-048) ; ajoute à `App` un état propre (les pas de `MELODY`, D-047) ; protocole 12 ; ne touche ni la calibration ni les réglages système (D-027, D-028) ; premier pas vers un patch sauvegardé sur le module (réserve des étapes 6 à 15) |

## 1. Contexte

L'utilisateur demande (2026-11-27) un accessoire qui sauvegarde par la sonde l'état du module, puis le restaure plus
tard, dans un fichier local simple : une suite d'instructions ou, mieux, un fichier structuré. Le module ne garde
toujours pas son patch (apps, routes, réglages, assignations) d'une mise sous tension à l'autre.

La sonde lit déjà presque tout : les apps par emplacement, leurs routes de sortie et d'entrée, la valeur de chaque
paramètre, l'entrée de chaque paramètre assignable. Il lui manque ce qui n'est pas un paramètre : les pas de `MELODY`
(leurs places et leurs rythmes).

## 2. Options envisagées

* **Contenu** : ce que le protocole donne, les pas de `MELODY` retirés au chargement ; ou tout, les pas compris, par
  une extension du protocole.
* **Format** : TOML par noms ; JSON par noms ; une suite de commandes `cargo probe` à rejouer.
* **Clés** : par index (fragile quand un réglage s'ajoute) ; par noms (`MELODY`, `LENGTH`, `CLOCK`), comme la sonde
  les lit déjà.
* **Chargement** : remplacer les quatre emplacements ; ou fusionner ceux du fichier avec ce qui tourne.
* **État propre d'une app** : des paramètres cachés (`STEP 1` à `STEP 32`) ; ou un état opaque que l'app sérialise
  elle-même (`postcard`), échangé par morceaux, que l'outil hôte, qui partage `oc-core`, sait relire pour les apps
  qu'il connaît.

## 3. Ce que l'assistant a proposé

Tout, pas compris ; TOML par noms ; remplacer les quatre emplacements, jamais la calibration ni les réglages système ;
un état propre par app (`App::save_state` / `App::load_state`), échangé par morceaux de 32 octets parce que les
requêtes ne portent pas de tranche de longueur libre ; commandes `cargo probe save` et `cargo probe load`.

## 4. Ce que nous avons décidé, et pourquoi

Les propositions (réponses de l'utilisateur, 2026-11-27) :

* **Fichier TOML**, clés par noms, valeurs écrites comme la sonde les écrit et les relit (`"16"`, `"MAJOR"`, `"A4"`,
  `"1:THRU"`, `"x0.50"`) : éditable à la main, robuste à l'ordre des réglages. Un tableau `[[slot]]` par emplacement
  (`slot` de 1 à 4, `app`), ses tables `outputs`, `inputs`, `params`, `assigns` (champs `input`, `scale`, `shift`,
  `min`, `max`, `effect`) et `state` ; `focus` en tête ; `format = 1`.
* **État propre** : `App::save_state` écrit l'état sérialisé par `postcard` (au plus `STATE_MAX`, 256 octets),
  `App::load_state` le relit et le borne ; rien par défaut. `MELODY` y met ses pas (`melody::Steps` : places et rangs
  de rythme), sa position et son tirage restant les siens. L'outil écrit les pas de `MELODY` en tableaux lisibles
  (`places`, `plays`, `ties`), l'état d'une app qu'il ne connaît pas en hexadécimal.
* **Protocole 12** : `Request::GetState { app, offset }` répond `Reply::State` (le total et 64 octets au plus à partir
  de `offset`) ; `Request::SetState { app, offset, length, bytes: [u8; 32], last }` envoie un morceau, que le serveur
  assemble et fait lire à l'app au dernier. Nouvelles variantes en dernier (rangs stables).
* **Chargement** : les quatre emplacements prennent le fichier (`NOTHING` pour un absent) ; toutes les sorties d'abord
  coupées, puis les routes du fichier ; les paramètres posés deux fois (bornes croisées) ; les assignations et les
  entrées après, quand toutes les apps sont là (elles peuvent lire un port d'une autre) ; l'état propre ; le focus en
  dernier. La calibration et les réglages système ne sont jamais touchés. Un nom inconnu (réglage retiré depuis) est
  signalé et passé, le reste chargé.

Pourquoi : un fichier qu'on lit et retouche, indépendant de l'ordre interne ; le chemin de la sonde déjà testé pour
tout, sauf l'état propre, qui reste l'affaire de l'app et passe par un canal générique.

## 5. Ce qui nous ferait changer d'avis

* Le patch sauvegardé sur le module lui-même (réserve de toujours) : le même contenu, sérialisé par `postcard` dans la
  flash ; ce format TOML resterait l'échange avec l'hôte.
* Un état propre plus grand que 256 octets (une app à échantillons) : morceaux plus nombreux, ou un tampon plus grand.
* La page web voulant le même fichier : déplacer la lecture et l'écriture TOML dans un module sans `cli`.
