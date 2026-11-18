# Étape 6 — Premier applet musical : Ratchet (plan du 2026-11-17)

Décision : [D-032](../../journal/D-032-ratchet-applet.md). Demande de l'utilisateur : deux applets simples mais
utiles ; la première est un ratchet (rafales de triggers), la seconde reste à décrire.

## Lots

| Lot | Contenu | Critère d'acceptation |
|---|---|---|
| **6a** — calcul | virgule fixe : `log2`, `exp2` en 16.16 (`oc_core::fixed`) ; intervalles d'une rafale selon le nombre, le temps, `SHAPE` signé et le placement (`FIRST`, `SPAN`) ; entrées (suit / s'ajoute) ; tempo de TR1 ; bornes | tests : contre `f64` sur l'hôte, formes, bornes, mode `GATE`, entrées, tempo |
| **6b** — applet | `RatchetApp` : front de TR1, copie sur A, rafale sur B (`PASSTHRU`), redémarrage, impulsions de 5 ms, `PROB`, `SYNC`, entrée au choix par réglage ; `AppId::Ratchet` en fin d'énumération | tests sur `mock_engine` : comptes, espacements, gate, redémarrage, probabilité, synchronisation, entrées ; scénarios dorés |
| **6c** — écran | deux pages de réglages et leur curseur, entrées, BPM, témoins `1` / `A` / `B`, frise de la rafale avec le repère du prochain trigger | écrans dorés relus |
| **6d** — module | niveau 9 (TESTING 9.3c) : TR1 au Pamela, rebouclage A → TR2, B → TR3, `cargo probe triggers` ; comparer `MODE FIRST` et `SPAN` à l'oreille | comptes et espacements conformes, au tick près ; choix du placement |
| **6e** — écran et entrées | ascenseur, frise de la rafale en cours (révision de D-032) ; sous-menu d'assignation des entrées : entrée, lecture et bornes vues, échelle, décalage, bornes, effet `SET` / `ADD` / `MUL` (D-035) | tests sur `mock_engine`, écrans dorés relus ; TESTING 9.3c sur le module |

Ordre : 6a → 6b → 6c → 6d → 6e.

## Risques

1. Quantification à 1 ms : sensible pour des intervalles de quelques ms et des formes très accélérées (D-032 § 5).
2. `AppId` sérialisé : la variante s'ajoute en fin ; menu DIAGNOSTIC, SCOPE, GFX DEMO, RATCHET.
3. Le rebouclage d'une sortie sur une entrée trigger suppose que 5 V passent le seuil de l'entrée : à vérifier
   (6d).
