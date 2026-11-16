# D-020 — Écran de démarrage : 1,5 s, sorties au repos, rendu à chaque tick

| | |
|---|---|
| **Identifiant** | D-020 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-02) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `0c2cc90` (`splash.rs`, `Engine::tick_splash`, `Engine::reset`, ABI), `ed69cbc` (bannière « O&C Alambic ») |

## 1. Contexte

Au démarrage, rien ne disait quel firmware tournait ni que le module avait fini de s'initialiser ; dans Rack,
l'action « Initialize » devait se comporter comme une remise sous tension. Question connexe : que doivent faire les
sorties CV pendant ce temps ?

## 2. Options envisagées

**A** — Pas d'écran de démarrage ; l'applet démarre au premier tick.

**B** — Un écran de démarrage de durée fixe (nom, version, bordure tracée progressivement) pendant lequel les applets
ne voient aucune entrée et **les sorties restent à 0 V** ; `Engine::reset` le rejoue (Rack `onReset`) ;
`skip_splash` pour les tests et les hôtes qui n'en veulent pas.

## 3. Ce que l'assistant a proposé

Non consigné à l'époque (commit d'une ligne). Le code commente le choix des sorties : « nothing downstream can
react to a signal before the module has visibly finished starting up ».

## 4. Ce que nous avons décidé, et pourquoi

**B**, 1,5 s (`splash::DURATION_MICROS`). L'animation est rendue **à chaque tick**, indépendamment de
`render_interval`, pour que la bordure se trace sans à-coups. Les entrées de contrôle sont lues et jetées pour ne
pas être rejouées d'un bloc à la sortie de l'écran.

## 5. Ce qui nous ferait changer d'avis

* Si l'on voulait que le module soit opérationnel immédiatement à la mise sous tension (un rack qu'on allume au
  milieu d'un patch en cours), 1,5 s de sorties à 0 V seraient un défaut.

## Points ouverts (revue du 2026-11-16)

* Sur le module, une trame SH1106 coûte ≈ 1,05 ms de SPI à 8 MHz : rendre à chaque tick d'une boucle à 1 ms fait
  **dépasser chaque tick** pendant toute la durée de l'écran. Sans conséquence aujourd'hui (sorties au repos, durée
  mesurée en temps écoulé), mais incompatible avec D-023, qui interdit un `present()` à cadence signal. À reprendre
  avec D-023 (rendu du splash sur la seule cadence UI).
* « 0 V » en sortie suppose un DAC déjà sorti de reset et une calibration correcte ; avec les étages inverseurs
  (D-006), un DAC encore en reset donne ≈ +6 V. L'ordre d'initialisation de `main.rs` relâche le reset avant la
  première écriture, mais rien ne le teste.
