# Étape 8 — Premier oscillateur : Supersaw (plan du 2026-11-18)

Décision : [D-037](../../journal/D-037-supersaw-applet.md). Demande de l'utilisateur : l'applet supersaw de l'étude
`docs/supersaw/`, sur le chemin audio de l'étape 7 (D-033). Choix : A et B stéréo, C sub-octave ; note, désaccord et
mix ; PolyBLEP d'abord, ×4 seulement si le repliement s'entend.

## Lots

| Lot | Contenu | Critère d'acceptation |
|---|---|---|
| **8a** — calcul | `oc_core::audio::supersaw` : scie PolyBLEP entière (aucune division par échantillon), table de désaccord de Szabo, lois de mixage, passe-haut à un pôle sur la fondamentale, parts `Mono` / `Left` / `Right` / `Sub` ; `Params::supersaw`, `Channel::Supersaw(Part)`, banc dans `Voice` ; banc criterion | tests : table et lois contre `f64`, PolyBLEP continue au saut, alias réduits d'au moins 20 dB contre la scie naïve (DFT), passe-haut sans continu, parts cohérentes, bornes ; banc de `next_sample` |
| **8b** — applet | `SupersawApp`, `AppId::Supersaw` (« SUPERSAW ») en fin d'énumération ; réglages `NOTE`, `DETUNE`, `MIX`, `LEVEL`, `STEREO` ; CV1 V/oct, CV2 désaccord, CV3 mix ; menu à six applets sans ligne vide ; écran (réglages, entrées, aperçu des sept voix) | tests sur `mock_engine` ; scénarios dorés relus ; menu doré régénéré |
| **8c** — module | niveau 9 : `audio` au profil, écoute du grave à l'aigu, `STEREO`, CV ; écran en marche | interruption sous sa période avec marge ; pas de tic ; décision sur le ×4 |
| **8d** — ×4 (si 8c le demande) | suréchantillonnage ×4 dans la voix, filtre de décimation | 8c repassé ; `audio` mesuré |

Ordre : 8a → 8b → 8c → 8d éventuel. Version 0.6.0 à la clôture.

## Risques

1. Coût dans l'interruption : estimé à quelques centaines de cycles par échantillon (≈ 0,5 µs) ; mesuré en 8c.
2. Débordements en entier : écarts de pas, gains et passe-haut bornés par des tests aux extrêmes (note C8,
   désaccord 100 %, mix 0 et 100 %).
3. `Params` grossit (banc en plus) : copie dans la boîte aux lettres un peu plus longue, sous section critique ;
   à vérifier au profil.
4. `AppId` sérialisé : la variante s'ajoute en fin ; menu DIAGNOSTIC, SCOPE, GFX DEMO, RATCHET, TONE, SUPERSAW.
5. Images de reconstruction non filtrées par la sortie (étude § 10) : hors de portée du calcul, noté à l'écoute.
