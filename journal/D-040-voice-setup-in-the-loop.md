# D-040 — Préparer la voix dans la boucle : l'interruption audio ne reçoit que des coefficients prêts

| | |
|---|---|
| **Identifiant** | D-040 |
| **Date** | 2026-11-19 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
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
même par construction, et les écrans dorés ne bougent pas. Le `Setup` est `Copy`, sans allocation, et sa construction
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
