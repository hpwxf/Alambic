# Étape 1 — mise en route sur le module (2026-08-02)

Reconstruit le 2026-11-16 depuis `git log`. Aucun plan écrit : lots au fil des essais sur le module.

## Fait

| Commit | Contenu |
|---|---|
| `350ee90` | façade VCV alignée sur la disposition du module, taille de pixel recalculée |
| `0c2cc90` | écran de démarrage (D-020) sur les trois backends |
| `384cb2b` | glyphes octants dans le simulateur (feature `octant`) |
| `6f81467` | liste de contrôle de disposition mémoire (`size_check.rs`) branchée sur la validation (D-016) |
| `3f50a42` | OLED SH1106 sur le module (D-009), tronçons SPI ≤ 512 octets (D-010), USB CDC et LED de démarrage (D-011) |
| `ed69cbc` | bannière « O&C Alambic » |
| `a8f3560` | encodeurs et boutons : pull-up 22 kΩ + hystérésis sur les pastilles (D-007) |
| `f12fd71` | `OrnamentCrimeRust` → `OrnamentCrimeAlambic`, `xtask vcv clean`, URL du dépôt |
| `2deca8a` | `--features` relayées par toutes les sous-commandes `xtask` |

## Mesuré

* Sur le module : seules ≈ 8 lignes de pixels vivantes avec le mode SSD1306 → la dalle est un SH1106.
* Journal USB (rapporté par le plan ISR local) : travail d'un tick ≈ 20 µs hors rendu.

## Corrigé

* Défaut OLED : SH1106 par défaut au lieu de SSD1306 (révision de D-009).
* Entrées numériques flottantes : `Port::input` ne configure pas la pastille (D-007).

## Non fait

* Niveau 9 de `TESTING.md` : calibration, entrées CV, triggers non vérifiés.
* `README.md` / `AGENTS.md` n'ont pas été mis à jour : ils disent encore que le firmware n'a jamais tourné sur le
  module (D-011, point ouvert).
