# D-012 — Harnais principal = simulation native ; Renode limité à un smoke test, resté non fait

| | |
|---|---|
| **Identifiant** | D-012 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `84f9d86` (simulateur), `d173510` (Renode consigné comme non fait) |

## 1. Contexte

Le firmware ne se teste pas sans matériel. Deux façons de rapprocher les tests de la cible : émuler le
microcontrôleur (Renode a une plateforme i.MX RT1064 et un modèle GPT), ou exécuter le comportement sur l'hôte
derrière les traits de D-002.

## 2. Options envisagées

**A** — Renode comme harnais principal, avec des modèles C# de l'ADC, du LPSPI, du DAC8565 et de l'OLED. Teste le
vrai binaire ARM, mais le plan estime ces modèles à « 1 à 2 semaines », disproportionné à ce stade.

**B** — Simulation native de `oc-core` comme harnais quotidien, Renode réduit à un **smoke test** : « le vrai ELF
ARM boote et journalise sa bannière sur LPUART ».

**C** — Aucun émulateur ; le module réel comme seul test du binaire.

## 3. Ce que l'assistant a proposé

**B**. Plan de fondation, Key Decision 3 (validée). Puis, quand D-011 a supprimé l'UART, l'assistant a refusé de
supprimer l'item en silence (`d173510`) : « The item is left open with the alternatives spelled out rather than
quietly dropped » — bannière sur USB CDC, semihosting, ou observation des écritures SPI.

## 4. Ce que nous avons décidé, et pourquoi

**B** pour le harnais ; le smoke test Renode, tel qu'écrit, est infaisable et **n'a pas été remplacé**. En pratique,
c'est **C** qui couvre le binaire : la mise en route sur le module (D-011) a trouvé en une journée les défauts
qu'un smoke test de démarrage n'aurait pas vus (mode d'adressage OLED, pull-ups absents).

## 5. Ce qui nous ferait changer d'avis

* Un défaut de démarrage (FCB, IVT, initialisation d'horloge) qui ne se voit que sur le module et coûte plusieurs
  cycles flash / essai : un smoke test Renode « atteint `main` et écrit sur le SPI » redeviendrait rentable.
* L'arrivée d'une ISR et d'un DMA (D-023) : le comportement temporel ne se teste ni dans le simulateur ni dans
  Renode sans modèles fidèles ; la mesure sur le module (GPIO + oscilloscope, `docs/supersaw/` § 13) restera le seul
  juge.

## Points ouverts (revue du 2026-11-16)

* L'item est « ouvert » depuis le 2026-08-01 dans `AGENTS.md` et le plan Junie, sans échéance. Il faut soit
  l'abandonner formellement (statut `abandonnée` d'un sous-point, et retrait des « Known gaps »), soit le reformuler
  en tâche datée (ELF qui atteint `main` et écrit le premier mot DAC, observé dans Renode). Le laisser ouvert
  indéfiniment dilue la liste des vrais manques.
