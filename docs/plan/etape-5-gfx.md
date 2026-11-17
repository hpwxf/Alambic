# Étape 5 — Primitives graphiques et applet de démonstration (plan du 2026-11-17)

Décision : [D-030](../../journal/D-030-graphics-primitives-and-demo.md). Demande de l'utilisateur : une applet de
démonstration graphique qui essaie différentes polices, des potentiomètres et des courbes simulées, et extrait des
primitives utiles des écrans de Squares-and-Circles.

## Lots

| Lot | Contenu | Critère d'acceptation |
|---|---|---|
| **5a** — bases (fait) | `oc_core::gfx` : ligne horizontale / verticale, rectangle plein ou vide, zone inversée, pointillés en octets entiers ; ligne quelconque et cercle par `embedded-graphics` ; table de sinus entière | tests : pixels identiques à `embedded-graphics` pour les rectangles et lignes droites, bords et coupures ; banc criterion |
| **5b** — contrôles et motifs (fait) | potentiomètre, sélecteur, valeur encadrée, légende + valeur, barres ; trace, forme d'onde min / max, polyligne, histogramme, échelle à aiguille ; cases, points, anneau euclidien, mini-clavier ; bandeau de titre | tests de forme (pixels clés, bornes, symétries) ; aucun pixel hors de la boîte demandée |
| **5c** — applet de démo | `AppId::Gfx` dans le menu ; quatre pages ; animations tirées du temps écoulé (déterministes au simulateur) ; scénario doré par page | écrans dorés relus ; banc par page ; définition de « fait » |
| **5d** — mesure | `cargo probe stats` par page sur le module ; coût par primitive si une page déborde | aucun tick au-delà de la période ; chiffres dans `avancement/etape-5.md` |

Ordre : 5a → 5b → 5c → 5d.

## Risques

1. Un écran chargé (courbes, anneau) coûte plus que le Diagnostic : la mesure par page le dira ; parade : chemin
   rapide dédié ou dessin réparti.
2. Ajouter `AppId::Gfx` change le menu et l'énumération sérialisée (`startup_app`) : la variante s'ajoute en fin,
   un réglage enregistré reste lisible ; écran doré du menu à régénérer.
