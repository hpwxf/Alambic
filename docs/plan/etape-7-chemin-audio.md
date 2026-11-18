# Étape 7 — Chemin audio : DAC à 48 kHz (plan du 2026-11-17)

Décision : [D-033](../../journal/D-033-audio-path.md). Demande de l'utilisateur : améliorer le chemin du DAC pour
générer des sons sur les sorties du module. Choix : 48 kHz, les quatre sorties, une applet `TONE` de test d'abord.

## Lots

| Lot | Contenu | Critère d'acceptation |
|---|---|---|
| **7a** — DAC rapide, encore à 1 kHz | `DAC_CS` en PCS0 matériel (`board.rs`), trames de 24 bits ; le bus partagé choisit le PCS et l'horloge de chaque périphérique (OLED sur PCS3, sans broche) ; DAC à 22 MHz, OLED à 22 MHz ou 8 MHz (feature) ; une voie écrite seulement si son code change ; révision de D-010 | sur le module : sorties justes au multimètre (niveau 9), OLED intact à 22 MHz ou repli à 8 MHz, écriture des quatre voies ≈ 5 µs (`cargo probe stats`, étape `outputs`) |
| **7b** — cœur audio | `oc-core` : voix (phase 32 bits, table de sinus, formes), paramètres publiés par tick, échantillon → code par la calibration ; applet `TONE` (partie contrôle + voix) ; Rack appelle la voix à chaque `process()` ; simulateur : voix appelée à sa cadence | tests hôte : justesse (V/oct, Hz), amplitudes, formes, bornes, pas de flottant ; écran doré ; Rack compilé si le SDK est là |
| **7c** — interruption à 48 kHz | PIT, priorité NVIC la plus haute ; l'interruption possède le bus : DAC chaque période, tronçons d'OLED ; la boucle remet la trame (double tampon) ; profil : durée de l'interruption, dépassements, débit de l'écran ; révisions de D-008, D-009, D-010 | firmware compilé, `xtask size` ; sur le module : 48 000 interruptions par seconde, aucun dépassement, écran à ≥ 50 trames/s |
| **7d** — module | niveau 9 : `TONE` à l'oreille (clics), à l'oscilloscope, à l'accordeur (justesse en V/oct) ; écran en marche pendant l'écoute | aucun tic lié à l'écran ; justesse à quelques cents ; décision sur le DMA |
| **7e** — DMA (si 7d le demande) | DAC en DMA déclenché par le PIT, double tampon, cache | 7d repassé sans défaut |

État (2026-11-17) : 7a mesuré sur le module (écran propre à 22 MHz, sorties justes, `outputs` 3,0 µs en moyenne,
page d'OLED 49 µs ; D-033, révision) ; 7b fait sur l'hôte (le firmware joue `TONE` échantillonné à 1 kHz jusqu'au
lot 7c ; Rack non compilé faute de SDK sur cette machine). 7c écrit : GPT2 sur le quartz de 24 MHz plutôt que le PIT
(PERCLK à 1 MHz : 48 kHz n'y tombe pas juste), l'interruption attend les mots du DAC avant le tronçon d'OLED, délai
entre deux mots du DAC porté à 45 ns. Sur le module (2026-11-18, D-033) : quartz muet, repli PERCLK à 47 619 Hz ;
fin d'envoi lue dans l'écho reçu ; interruption ≈ 60 % du processeur dans `TONE`, gigue ±3 µs sur l'écriture du
DAC : arguments du lot 7e. 7e (2026-11-18) : DAC en DMA déclenché par le PIT, interruption du DMA pour l'échantillon et
l'écran ; `audio` 3,4 µs sur 21, écran et son corrects (D-033, révisions du lot 7e).

Ordre : 7a → 7b (indépendant du matériel, peut avancer en parallèle) → 7c → 7d → 7e éventuel.

## Risques

1. Horloge : 22 MHz sous le plafond du DAC en 3,3 V (25 MHz) ; l'OLED à cette vitesse reste à essayer (7a).
2. Le PCS matériel s'affirme à chaque transaction : l'OLED sur PCS0 écrirait dans le DAC ; testé sur bus simulé
   (7a).
3. Deux contextes : la boucle et l'interruption partagent les paramètres et la trame ; section critique courte,
   aucune allocation ni attente dans l'interruption (7c).
4. Le CS de l'OLED doit être relevé avant les mots du DAC, sinon l'OLED les recevrait : l'ordre est vérifié par un
   test du pilote sur bus simulé (7c).
5. Applets existantes (Diagnostic, Scope, Ratchet) : leurs niveaux tenus passent par l'interruption sans changer de
   comportement ; goldens et tests inchangés.
