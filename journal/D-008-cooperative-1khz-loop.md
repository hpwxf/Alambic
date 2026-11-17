# D-008 — Boucle coopérative à 1 kHz, sans RTOS ni interruption ; rendu découplé

| | |
|---|---|
| **Identifiant** | D-008 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `555f492` (`Engine::set_render_interval`, bancs), `ec140ed` (boucle de `main.rs`, `TICK_PERIOD_MICROS`, `RENDER_INTERVAL_TICKS`) |

## 1. Contexte

Le premier applet est un diagnostic d'E/S : lire 4 CV et 4 triggers, piloter 4 sorties, afficher. Le firmware de
référence O&C tourne sur deux ISR (cœur à 60 µs, UI à 1 ms). Il faut décider du modèle d'exécution du premier
jalon, et du coût de l'écran : les bancs `criterion` montrent que le rendu coûte environ **mille fois** la mise à
jour d'état, et une trame SH1106 complète (8 pages × 131 octets) prend ≈ 1,05 ms de SPI à 8 MHz.

## 2. Options envisagées

**A** — Boucle `loop` cadencée par attente active sur l'horloge µs (GPT1), sans interruption, un `tick()` par
milliseconde.

**B** — RTIC (priorités, ressources partagées) ou Embassy (async).

**C** — Reprendre directement les deux ISR de la référence (60 µs + 1 ms).

Et, pour l'écran : redessiner à chaque tick, ou découpler la cadence de rendu de la boucle de contrôle.

## 3. Ce que l'assistant a proposé

**A**. Plan de fondation, Key Decision 7 (validée) : « Boucle principale coopérative sans RTOS au départ : `loop` +
timer, pas d'ordonnanceur. On évite la complexité concurrente tant qu'elle n'est pas justifiée ; `rtic` reste une
option ultérieure documentée. » Le découplage du rendu vient des bancs de `555f492` : « the redraw rate is decoupled
from the control loop with Engine::set_render_interval; the CV outputs are still refreshed on every tick ».

## 4. Ce que nous avons décidé, et pourquoi

**A**, avec rendu une fois tous les 20 ticks (≈ 50 Hz, `RENDER_INTERVAL_TICKS`). Le pas suit l'horloge : un tick en
retard enchaîne immédiatement le suivant sans accumuler de dette ; les applets reçoivent le temps réellement écoulé
(`elapsed_micros`), pas un pas nominal. USB CDC est sondé dans la boucle et dans l'attente (D-011). Mesure
rapportée par le plan ISR : travail d'un tick ≈ 20 µs hors rendu.

Raisons : la boucle est triviale à raisonner, il n'y a aucun état partagé entre contextes (`RefCell` suffit pour le
bus SPI, D-010), et D-002 garde le comportement indépendant de ce choix.

## 5. Ce qui nous ferait changer d'avis

* Toute exigence de cadence de sortie au-delà du kilohertz (enveloppes rapides, audio) : c'est D-023.
* Une gigue mesurée gênante sur les sorties. Elle existe par construction : un tick sur vingt porte le transfert
  OLED (≈ 1 ms), donc l'échantillonnage et la mise à jour des sorties sautent ≈ 1 ms toutes les 20 ms.

## Points ouverts (revue du 2026-11-16)

* La gigue ci-dessus n'est ni mesurée ni consignée ; elle aggrave la perte de triggers courts (D-019). Elle est
  acceptable pour un diagnostic, pas pour le premier applet musical.
* Le modèle est en voie d'être remplacé (D-023, `proposée`) : tant que D-023 n'est pas tranchée, aucun applet ne
  devrait dépendre d'une cadence supérieure à 1 kHz.

## Révision du 2026-11-17 — une interruption à côté de la boucle (D-033, lot 7c)

La boucle à 1 kHz reste le contexte de contrôle (entrées, panneau, applets, dessin, USB). Une interruption de
timer à 48 kHz, priorité la plus haute, s'y ajoute : elle seule écrit le DAC et envoie l'écran. La boucle lui
passe les paramètres des sorties et la trame dessinée par des boîtes aux lettres sous section critique ; la gigue
de la boucle ne touche plus les sorties.
