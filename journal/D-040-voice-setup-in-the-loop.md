# D-040 — Préparer la voix dans la boucle : l'interruption audio ne reçoit que des coefficients prêts

| | |
|---|---|
| **Identifiant** | D-040 |
| **Date** | 2026-11-19 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR et code) |
| **Relations** | révise D-033 (partage du travail entre la boucle et l'interruption : la voix y était préparée à chaque changement de paramètres) ; mesurée par D-029 (étapes `voice`, `voice_params`, D-039) ; le banc du Supersaw de D-037 et D-039 en est le premier bénéficiaire |

## 1. Contexte

D-033 fait publier par la boucle les `Params` d'une applet (ce que porte chaque sortie) et laisse l'interruption à
47 619 Hz les prendre dans sa propre `Voice` : `Voice::set_params` y recalcule, quand les paramètres changent, le pas
de phase de chaque sortie et, pour le Supersaw, tout le banc (huit divisions `u64`, deux racines `u64`, le coefficient
du passe-haut, et depuis D-039 la préparation du banc `f32`). Le Cortex-M7 n'a pas de division 64 bits matérielle :
ces divisions sont des routines logicielles.

Relevés sur le module du 2026-11-19 (TESTING 9.3g) : `audio` vaut 4,9 µs au plus quand les paramètres sont fixes, et
**11 µs** au plus quand une CV change la note à chaque tick, dans les deux modes de `CALC`. Ce pic dépasse de 6 µs
l'attente de la rafale du DAC, sur une période de 21 µs : il retarde les morceaux d'écran (trame de 1 344 à 1 381 µs)
et réduit la marge de l'interruption à chaque tick où une entrée bouge, c'est-à-dire tout le temps dès qu'une CV est
branchée (et même débranchée : le bruit de CV1 sur `NOTE` change la fréquence). Ce travail n'a rien à faire à 48 kHz :
il ne dépend que des paramètres, qui changent au plus une fois par tick.

## 2. Options envisagées

* **A — préparer dans la boucle.** Une fonction pure, `Setup::new(&Params, rate)`, calcule tout ce qui dépend des
  paramètres (pas, inverses, gains, normaliseurs, coefficients des deux bancs) ; `Voice::apply(&Setup)` ne fait que
  copier en gardant les phases et l'état des filtres. La boucle calcule le `Setup` hors section critique quand les
  paramètres changent et le dépose dans `MAILBOX` ; l'interruption l'applique. Coût : une `MAILBOX`
  plus grande (≈ 300 octets copiés sous section critique à chaque changement, côté boucle), la même latence (la
  période suivante).
* **B — garder la préparation dans l'interruption et la rendre moins chère** : inverses en `f32` ou en 32 bits,
  racine approchée. Le pic baisse sans disparaître, et chaque futur oscillateur devra refaire l'exercice.
* **C — étaler la préparation sur plusieurs périodes** (une voix par période). Le pic disparaît, mais un changement
  met plusieurs périodes à s'appliquer, les voix changent à des instants différents, et l'état intermédiaire est à
  tester.

## 3. Ce que l'assistant a proposé

**A**, à la suite de la lecture des relevés (D-039, lot 9b). `Voice::set_params` reste, défini comme
`apply(&Setup::new(params, rate))`, pour la voix du moteur à 1 kHz, le simulateur et Rack : le comportement y est le
même par construction, et les écrans de référence ne bougent pas. Le `Setup` est `Copy`, sans allocation, et sa construction
ne touche pas à l'état de la voix ; le calculer hors section critique évite d'y bloquer l'interruption.

## 4. Ce que nous avons décidé, et pourquoi

**A**, demandé par l'utilisateur le 2026-11-19. La règle qui en sort pour `oc_core::audio` : **tout ce qui ne dépend que
des paramètres se calcule dans `Setup::new`**, jamais par échantillon ni dans `apply`. L'interruption ne fait plus
que copier un `Setup` et rendre des échantillons ; `voice_params` mesure cette copie.

## 5. Ce qui nous ferait changer d'avis

* `voice_params` encore au-delà d'une microseconde sur le module : la copie elle-même pèse ; passer à un double tampon
  dont l'interruption échange l'indice.
* Un `Setup` qui grossit au point de rendre la section critique de la boucle visible (plusieurs kilo-octets, avec des
  applets à plusieurs bancs) : même remède.
* Un paramètre qui doit changer plus vite qu'une fois par tick (modulation audio) : il entrerait alors dans le calcul
  par échantillon, pas dans le `Setup`.

## Révision du 2026-11-21 — une révision plutôt qu'une comparaison

`audio::publish` comparait à chaque tick, sous section critique, les `Params` (80 octets) et la calibration des
sorties à ceux de `MAILBOX` : 1,55 µs par tick sur le module, que les paramètres changent ou non (D-044, étape
`publish`). Le moteur numérote désormais ses paramètres (`Engine::params_revision`, qui avance quand `params` change) ;
le firmware garde dans `audio::Publisher`, côté boucle, la révision et la calibration publiées, et ne prend la
section critique que quand l'une des deux a changé. `rest_outputs` oublie la révision, pour que la publication
suivante rende les paramètres. Mesuré : un tick inchangé coûte 1,02 µs à `publish` au lieu de 1,46 à 1,56. Le reste
de cette entrée est inchangé.

## Révision du 2026-11-22 — le moteur ne rend aucune voix, chaque backend joue la sienne

Le moteur rendait chaque tick un échantillon de sa propre voix, à 1 kHz, pour les sorties qu'il écrit lui-même : un
héritage des sorties de CV d'avant le chemin audio. Sur le module, ces sorties sont muettes (`MutedOutputs`) et
l'interruption rend les mêmes paramètres à 47 619 Hz ; dans Rack, la voix de `oc_engine_audio`, à la fréquence de
Rack, écrase aussitôt la sortie du moteur. Sous une CV qui bouge, la boucle préparait donc deux `Setup` par tick
(≈ 4,7 µs chacun en `f32`), le premier pour des échantillons que personne ne joue. Un `Setup` dépend de la fréquence
d'échantillonnage : celui du moteur ne peut pas servir à l'interruption. Seul le simulateur jouait vraiment la voix
du moteur.

Un premier essai coupait la voix du moteur sur le module par un mode (`play_elsewhere`) : un choix propre à un backend,
caché dans le moteur. Décidé avec l'utilisateur à la place : **le moteur ne rend plus aucune voix**. `Engine::tick`
publie `params()` et sa révision ; ce que portent les sorties est une entrée du tick, comme les CV lues, que le moteur
rapporte à `TickReport::cv_out` et à l'écran (`TickContext::cv_out`, `DIAGNOSTIC`). Chaque backend joue la voix à sa
cadence :

* **`oc_core::audio::Follower`** suit les paramètres par leur révision et ne rend un `Setup`, à la fréquence qu'on lui
  donne, que quand elle a bougé : le seul code qui décide de préparer, partagé par tous les backends ;
* **`oc_core::audio::Player`** (une `Voice` et son `Follower`) rend des millivolts à la cadence du backend, sans rien
  écrire. Le simulateur le joue une fois par tick, après `Engine::tick` (un pour un : il ne produit pas de son), et
  écrit l'échantillon par `Engine::write_outputs`, qui le convertit par la calibration du moteur (D-027) et le retient
  pour le tick suivant ; les tests font de même par `testing::MockEngine`. Rack le joue à chaque `process()` et, lisant
  des millivolts, n'écrit pas de codes : il rapporte l'échantillon par `Engine::set_played` ;
* **le firmware** garde son partage (`MAILBOX`) : `audio::Publisher` prépare par un `Follower` à 47 619 Hz,
  l'interruption joue et dépose les millivolts de son dernier échantillon, la boucle les donne au moteur avant chaque
  tick (`Engine::set_played`) ; ses sorties muettes ne sont plus écrites.

La règle « `Engine::tick` est l'unique point d'entrée » tient : le comportement reste dans le moteur, et la `Voice`
que chaque backend appelle est le code d'`oc-core`, testé une fois. Pas de fil d'exécution pour le simulateur : il
reste déterministe parce qu'il joue au rythme de ses ticks ; un vrai son (par exemple `cpal`) serait un backend en
plus, qui lirait les `Params` publiés comme l'interruption.

Conséquences : ce que le moteur voit des sorties (l'écran, `TickReport::cv_out`) a un tick de retard ; ce que les jacks
portent (`Simulator::cv_out`, Rack) ne change pas ; sur le module, `cv_out` montre les échantillons de
l'interruption au lieu de ceux d'une voix à 1 kHz. L'étape `outputs` du profil mesure `Engine::write_outputs` et
disparaît du module et de Rack. Mesures : `docs/plan/avancement/etape-11.md`.
