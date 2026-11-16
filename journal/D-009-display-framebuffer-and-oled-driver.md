# D-009 — Écran : framebuffer 1 bpp dans l'ordre du contrôleur, pilote maison, SH1106 par défaut

| | |
|---|---|
| **Identifiant** | D-009 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01, révisée le 2026-08-02) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `555f492` (`framebuffer.rs`), `ec140ed` (`ssd130x.rs`, défaut SSD1306), `3f50a42` (SH1106 par défaut), `2deca8a` (features relayées par `xtask`) |

## 1. Contexte

L'OLED 128×64 monochrome est l'unique retour visuel du module. Le même écran doit s'afficher dans le simulateur
(braille / octants) et dans Rack (NanoVG). Le contrôleur exact de la dalle n'était pas connu : SSD1306, SSD1309 ou
SH1106 selon les fabrications.

## 2. Options envisagées

Pour la mémoire d'image :

**A** — `embedded-graphics` avec un `DrawTarget` propre à chaque backend (`SimulatorDisplay` côté hôte, crate
`ssd1306` côté cible), comme le suggérait le plan.

**B** — Un seul `FrameBuffer` de 1 024 octets **dans l'ordre mémoire du contrôleur** (pages de 8 lignes, un octet =
une colonne de 8 pixels), écrit par `embedded-graphics` dans `oc-core`, poussé tel quel par le firmware et lu octet
pour octet par le simulateur et l'ABI C.

Pour le contrôleur :

**C** — Crates tiers `ssd1306` / `sh1106`. **D** — Un pilote maison `ssd130x` paramétré par `Controller`, testé
contre un bus simulé.

## 3. Ce que l'assistant a proposé

**B** et **D**. `555f492` : « 128x64 1bpp in the OLED controller order, so the firmware can push it unrepacked and
every backend reads the same bytes ». `ec140ed` : `ssd130x` « supports both OLED controllers the panel ships with;
a test asserts that only the charge-pump and pre-charge bytes differ ». Défaut initial : SSD1306, SSD1309 en
feature.

## 4. Ce que nous avons décidé, et pourquoi

**B** et **D**. Un seul format d'image partagé est ce qui rend les écrans dorés du simulateur (D-013) représentatifs
du module, et le pilote maison laisse `oc-drivers` testable sans dépendre des choix d'API d'un crate tiers.

## 5. Ce qui nous ferait changer d'avis

* Un rendu partiel (ne pousser que les pages modifiées, ou une page par période d'ISR — piste de D-023) : le format
  page-major s'y prête, mais il faudra un suivi de pages sales dans `FrameBuffer`.
* Une dalle en niveaux de gris ou plus grande : le format « ordre du contrôleur » cesserait d'être universel.

## Révision (2026-08-02, `3f50a42`)

Premier essai sur le module : seules ≈ 8 lignes de pixels semblaient vivantes. Le module porte une dalle **SH1106**
(132 colonnes, décalage de 2, rafraîchissement par page), pas un SSD1306 en adressage horizontal. Le défaut devient
SH1106 ; `ssd1306` et `ssd1309` deviennent des features. Le même commit découpe les écritures SPI sous le plafond de
trame LPSPI de 512 octets (D-010). `2deca8a` relaie `--features` à travers `cargo xtask build|size|hex|flash`.

## Points ouverts (revue du 2026-11-16)

* Les features `ssd1306` et `ssd1309` sont mutuellement exclusives, mais rien ne l'impose : avec les deux,
  `ssd1309` gagne silencieusement. Un `compile_error!` sur la combinaison coûterait deux lignes.
