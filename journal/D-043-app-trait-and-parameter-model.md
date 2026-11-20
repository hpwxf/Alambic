# D-043 — Un trait `App` : registre ouvert, modèle de paramètres et d'observables, écran propre à chaque app

| | |
|---|---|
| **Identifiant** | D-043 |
| **Date** | 2026-11-20 |
| **Auteur** | Pascal Havé |
| **Statut** | proposée |
| **Commits** | ce commit (ADR seule) ; implémentation au premier lot qui l'adoptera |
| **Relations** | révisera D-021 (`AppHost` fermé, une instance de chaque app) et précisera D-002 (ce que le cœur sait des apps) ; laisse intacts D-033 et D-040 (l'interruption ne voit que `Setup` et `Voice`) ; la modulation de D-035 / D-036 et la persistance de D-028 deviennent des consommateurs du modèle ; la sonde de D-029 en est un consommateur prévu ; D-042 se rouvre quand plusieurs apps tourneront ensemble |

## 1. Contexte

Les applets vivent dans `oc_core::apps`. Elles ne dépendent que de la boîte à outils du cœur (`platform`,
`framebuffer`, `text`, `gfx`, `fmt`, `fixed`, `modulation`, `presence`, `audio::wave`), ce qui est sain. L'inverse
ne l'est pas : le cœur nomme chacune d'elles.

| Couplage | Où | Effet |
|---|---|---|
| `AppHost` : un champ par app, aiguillage par `match`, aucun trait | `apps/mod.rs` | ensemble fermé : une app de plus modifie le cœur |
| `AppId`, enum fermé, persisté par son indice | `menu.rs`, `system.rs`, `settings.rs` | « chaque nouvelle app vient en dernier » pour relire une sauvegarde |
| un accesseur par app : `Engine::diagnostic()`, `Engine::apps().ratchet()`… | `engine.rs`, `apps/mod.rs`, 26 usages dans `oc-sim` et ses tests | les backends et les tests manipulent les types concrets |
| un modèle de réglages refait à la main dans chaque app : `enum Setting`, `label`, `value_text`, `write_value` ; `modulation::editor::Param` rempli app par app au dessin | `apps/supersaw.rs`, `apps/ratchet/` | rien de commun à lire de l'extérieur, rien à sauvegarder d'une app |
| la voix audio connaît le Supersaw (`Channel::Supersaw`, `Params::supersaw`, le banc dans `Voice`) | `audio/mod.rs` | hors du périmètre de cette entrée (§ 4, étape 2) |

Ce que l'utilisateur a fixé le 2026-11-20 :

* la modularité passe par un **trait**, maintenant ; la composition en crates (un paquet par app, un crate qui
  assemble une image) viendra plus tard, si le besoin se confirme ;
* à terme, le cœur doit faire **tourner plusieurs apps à la fois** et **router** des signaux entre elles, pas
  seulement vers les entrées et sorties physiques ;
* cette interface unifiée servira aussi à **interroger et piloter les apps à distance**, par l'USB ;
* **l'écran reste propre à chaque app** : sur 128 × 64, chaque rendu s'optimise pour lui-même.

## 2. Options envisagées

* **Aiguillage** :
  * **A** — l'enum fermé actuel, sans trait ;
  * **B** — un trait `App` et des `&mut dyn App` sur des instances statiques (pas d'allocateur dans le firmware) ;
  * **C** — un trait `App` et un enum d'aiguillage généré par une macro (statique, inliné).
* **Accès aux apps depuis l'extérieur** : les accesseurs par app (actuel) ; un accès générique par identifiant
  (`get(id) -> &dyn App`), plus un accès typé par `core::any::Any` pour les tests ; le modèle de paramètres seul.
* **État des apps** : chaque app garde son modèle à elle (actuel) ; un modèle de paramètres et d'observables dans le
  trait ; un registre de paramètres global dans le cœur, séparé des apps.
* **Identité** : l'indice de l'enum `AppId` ; une clé stable déclarée par l'app.
* **Écran et navigation aux encodeurs** : génériques, tirés du modèle ; propres à chaque app.

## 3. Ce que l'assistant a proposé

* **B** pour le contexte de contrôle : le plus simple, sans macro, et un `dyn` produit plutôt moins de code que la
  monomorphisation. C reste la voie des nœuds DSP de l'étape 2, là où le coût par échantillon compte.
* **Un registre sans accesseurs par app** : accès générique par identifiant ; accès typé par `Any` réservé aux tests ;
  les tests qui le peuvent passent par le modèle de paramètres, comme le ferait l'hôte.
* **Un modèle dans le trait**, sans allocation, descripteurs statiques :

  ```rust
  pub trait App {
      fn key(&self) -> AppKey;                     // stable, persisted
      fn name(&self) -> &'static str;
      fn params(&self) -> &'static [ParamSpec];    // name, kind, range, unit
      fn get(&self, index: usize) -> ParamValue;
      fn set(&mut self, index: usize, value: ParamValue) -> Result<(), Refused>;
      fn observe(&self, index: usize) -> Observed; // read-only: BPM, played pitch, counters
      fn action(&mut self, index: usize);          // e.g. fire a burst by hand
      fn update(&mut self, input: &InputSnapshot) -> Params;
      fn render(&self, frame: &mut FrameBuffer, context: &TickContext);
  }
  ```

  Un même `set`, avec les mêmes bornes, pour un cran d'encodeur, une écriture par USB, une restauration depuis la
  flash et, plus tard, une route depuis une autre app. Une valeur jouée (déplacée par une CV) est un observable,
  pas un paramètre écrit.
* **L'écran et la navigation restent dans l'app** : ordre des réglages, pages, mise en page. Le modèle décrit l'état,
  pas l'affichage ; une app peut s'en servir pour écrire un libellé, sans obligation. Le sous-menu d'assignation des
  CV (D-035, D-036), déjà commun, peut lire plage et valeur jouée dans le modèle ; son dessin ne change pas.
* **Une clé stable** `AppKey(u16)`, de valeur égale à l'indice actuel de chaque app : une sauvegarde de
  `startup_app` faite avant ce changement se relit telle quelle (D-028).
* **La sonde** : le serveur tourne déjà dans la boucle ; une écriture distante suit le chemin d'un cran d'encodeur,
  sans synchronisation nouvelle, testable sur `mock_engine`. Messages prévus (`oc-protocol`, version à monter) :
  `ListApps`, `DescribeApp`, `GetParam`, `SetParam`, `Act`, `Select`, et des valeurs choisies dans `Stream`.
* **Une trajectoire par étapes**, chacune avec son ADR, tout dans `oc-core` jusqu'à la quatrième :
  1. le trait, le registre et le modèle (cette entrée) ;
  2. une voix extensible : chaque app apporte son nœud DSP (`Setup` et rendu), `audio` ne nomme plus le Supersaw ;
  3. plusieurs instances, une seule ayant le focus (panneau et écran), le routage au rythme de contrôle (les
     `Source`s de `modulation` étendues aux sorties d'une autre instance), le patch persisté ;
  4. la composition en crates ou en features par app, si le budget ITCM (D-038) ou des apps tierces le demandent ;
  5. le routage audio entre apps, en rouvrant D-042 (calcul par blocs).
* **Performances** : le trait vit dans la boucle à 1 kHz ; depuis D-040 l'interruption ne voit que `Setup` et
  `Voice`. Coût audio de l'étape 1 : nul par construction. Dans la boucle, quelques appels indirects par tick sur
  600 000 cycles ; `get` et `set` hors du chemin de chaque tick ; descripteurs en flash. Le coût audio réel se pose
  à l'étape 2 (un appel indirect par nœud et par échantillon, estimé à 10 à 20 cycles, non mesuré, contre ≈ 700
  pour la voix du Supersaw), et surtout quand plusieurs apps actives additionneront leurs DSP.

## 4. Ce que nous avons décidé, et pourquoi

**Proposée.** Tranché par l'utilisateur le 2026-11-20 : un trait maintenant, la composition plus tard ; l'écran propre
à chaque app ; l'interface unifiée comme base du contrôle à distance.

Reste à trancher au premier lot :

* l'aiguillage, B ou C, par le coût en code mesuré ;
* la forme exacte de `ParamSpec`, `ParamValue` et `Observed` (types de valeur : entier borné, choix dans une liste,
  booléen ; unité ; pas d'un cran), éprouvée sur `RATCHET` et `SUPERSAW`, les deux apps les plus riches en réglages ;
* ce que les tests lisent par le modèle et ce qu'ils gardent par l'accès typé.

Premier lot : le trait et le registre ; `DIAGNOSTIC` et `RATCHET` migrées d'abord, puis les autres ; les accesseurs
par app retirés d'`Engine`, d'`AppHost` et d'`oc-sim`. Les messages de la sonde viennent dans un lot à part.

**Critères d'acceptation du premier lot** (ce qui le fait passer à `adoptée`) :

* sur le module, `cargo probe stats` : `voice` et `audio` inchangés au bruit près ; `update` et `render` à quelques
  microsecondes de leur valeur d'avant ;
* criterion : `engine/tick_idle`, `engine/tick_busy` et `voice/*` inchangés au bruit près ;
* coût en `.text` relevé par différence de `cargo xtask size` (méthode de D-038) ;
* écrans dorés inchangés, sans `UPDATE_SCREENS` ; une sauvegarde faite avant le lot relue avec la même app de
  démarrage.

## 5. Ce qui nous ferait changer d'avis

* `voice` ou `audio` qui bougent : le trait a fui dans l'interruption ; le ramener dans la boucle.
* Un coût en `.text` de plus de quelques kilo-octets, ou l'ITCM à court (D-038) : C au lieu de B.
* Une app réelle que le modèle ne décrit qu'avec des contorsions (paramètre composite, bornes qui dépendent d'un autre
  réglage, liste de longueur variable) : garder le modèle minimal et laisser ce réglage hors du modèle plutôt que de
  le compliquer pour tous.
* Des tests moins lisibles par le modèle que par le type concret : l'accès typé leur reste.
* Un contrôle distant qui demande d'écrire hors de la boucle (latence) : une file de commandes vidée par la boucle,
  jamais d'accès direct depuis un autre contexte.
