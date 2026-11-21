# Étape 5 — primitives graphiques et applet de démonstration (avancement, terminée le 2026-11-17)

Plan : [etape-5-gfx.md](../etape-5-gfx.md). Décision : D-030, adoptée puis révisée (lignes et cercles écrits
ici pour pouvoir inverser ; cinq pages ; `embedded-graphics` en dépendance de test ; un banc pour le graphisme).
D-009 révisée en chemin (texte par colonnes, police en paramètre). Valeurs mesurées sur un module et un hôte :
indicatives.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| texte | `53ee310`, `ff1bc36` | `oc_core::text` : polices en colonnes d'octets (`ColumnFont`, six polices générées et vérifiées contre `embedded-graphics`), police dans le `TextStyle` |
| plan | `288db18` | D-030, plan, feuille de route renumérotée |
| 5a | `ee4e07c` | bases `gfx` : formes droites en octets, ligne, cercle, disque, `Ink::Invert`, sinus entier |
| 5b | `0bf24b3` | contrôles, courbes, motifs ; `Ratio`, `Area` |
| 5c | `da8d31b` | applet `GFX DEMO`, cinq pages, écrans de référence, banc par page |
| 5d | `e8575f3` | mesure sur module |
| suite | `bb3ab08`, ce commit | `embedded-graphics` en dépendance de test ; banc `graphics.rs`, rapports HTML de criterion |

## Mesures

* **Dessin sur le module** (`cargo probe stats`) : Diagnostic 645,5 → 87,8 µs avec le texte par colonnes ; page des
  courbes de la démo 105,2 µs ; pire tick 169 µs (envoi d'une page) ; périodes 997,5 – 1 002,4 µs.
* **Hôte** (criterion, `benches/graphics.rs`) : texte ≈ × 10, rectangles × 7 à × 90, cercles ≈ × 2 face à
  `embedded-graphics` ; lignes quelconques ≈ × 1,2. Détail : D-030, dernière révision.
* **Firmware** : `.text` −952 octets, `.rodata` −336 sans `embedded-graphics`.
* **À l'œil** : les cinq pages de `GFX DEMO` validées par l'utilisateur sur le module.

## Reste ouvert

* Budget d'un tick : l'ADC bloquant (≈ 288 µs à chaque tick, ≈ 29 %) domine désormais, l'envoi OLED vient ensuite
  (≈ 6 % en moyenne, 145 µs les ticks d'envoi) ; pistes : échantillonnage non bloquant (D-023, option A),
  n'envoyer que ce qui a changé (D-009).
* Lignes quelconques posées pixel par pixel : sans enjeu tant qu'aucune page n'en trace beaucoup.
