# D-011 — Aucun UART ; diagnostic par USB CDC sondé et LED de démarrage

| | |
|---|---|
| **Identifiant** | D-011 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01, révisée le 2026-08-02) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `ec140ed` (pas d'UART), `d173510` (conséquence sur Renode), `3f50a42` (USB CDC, `boot_led.rs`) |

## 1. Contexte

Le plan de fondation prévoyait une bannière de version sur LPUART, et un smoke test Renode qui la vérifie. Au moment
d'écrire le firmware, le brochage (D-007) montre que **tous** les LPUART exposés sur les broches 0–23 entrent en
conflit avec la façade : LPUART2 (14/15) = bouton et ligne B de l'encodeur droit, LPUART6 (0/1) = TR1/TR2,
LPUART4 (7/8) = reset et CS de l'OLED. Piloter l'une de ces broches en émission est exactement le risque électrique
que la table interdit.

## 2. Options envisagées

**A** — Garder un UART en sacrifiant une fonction de façade. Rejeté : risque électrique.

**B** — Aucun canal hôte ; l'OLED seul. Mais une dalle noire (cas réel, D-009) ne dit alors rien.

**C** — **USB CDC** sur le port USB natif du Teensy (`imxrt-log`), sondé depuis la boucle (pas d'interruption USB),
plus des **éclats de LED** sur la broche 13 aux étapes de démarrage, *avant* que le SPI ne s'empare de la pastille
(SCK).

**D** — Semihosting / sonde de débogage. Le Teensy 4.0 n'expose pas de SWD utilisable sans modification.

## 3. Ce que l'assistant a proposé

D'abord **B** (`ec140ed` : « There is deliberately no UART: every LPUART on pins 0-23 collides with the panel »),
avec l'item Renode laissé ouvert et ses alternatives écrites (`d173510`). Puis **C** lors de la mise en route de
l'OLED (`3f50a42` : « Add USB CDC boot log and pre-SPI LED stage flashes so the next blank panel is diagnosable
without a probe »).

## 4. Ce que nous avons décidé, et pourquoi

**C**. Le journal USB annonce chaque étape (`adc ok`, `triggers ok`, `spi ok`, `oled init ok (SH1106)`…) puis un
battement par seconde (`tick=… last_us=…`) ; la LED clignote 1, 2, 3 fois avant la prise du SPI. L'USB est sondé
(`Interrupts::Disabled`) pour rester dans le modèle mono-contexte de D-008. Un échec d'init de l'OLED est journalisé
et **n'arrête pas** le module : le chemin CV prime.

## 5. Ce qui nous ferait changer d'avis

* Si D-023 introduit une ISR, le sondage USB depuis la boucle UI reste valable ; une interruption USB deviendrait
  nécessaire seulement si le débit de journal devait survivre à une boucle UI bloquée.
* Si un module de débogage (SWD) était ajouté, `defmt` + RTT remplacerait avantageusement le journal texte.

## Révision (2026-08-02, `3f50a42`)

Ajout de l'USB CDC et des éclats de LED. Les documents n'ont pas tous suivi — voir ci-dessous.

## Points ouverts (revue du 2026-11-16)

* `README.md` (« not been run on real hardware »), `AGENTS.md` (« The firmware has never run on real hardware ») et la
  section « Unverified » de `board.rs` sont **périmés** : `3f50a42` et `a8f3560` corrigent des défauts observés sur
  le module (OLED, encodeurs, boutons), `docs/supersaw/` rapporte des essais sur le Teensy du module, et le plan ISR
  cite une mesure « log USB antérieur ≈ 20 µs ». Il faut écrire ce qui a été vérifié (écran, panneau) et ce qui ne
  l'a pas été (calibration, entrées CV, triggers).
