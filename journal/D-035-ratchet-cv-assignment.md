# D-035 — Ratchet : sous-menu d'assignation des entrées CV (échelle, décalage, bornes, effet)

| | |
|---|---|
| **Identifiant** | D-035 |
| **Date** | 2026-11-18 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ce commit (ADR, plan, implémentation) |
| **Relations** | remplace le choix d'entrée de D-032 (`--`, `CVn=`, `CVn+` au clic sur l'encodeur droit) ; garde ses correspondances (nombre +4/V, temps 1 V/oct, `SHAPE` +1,0/V, `PROB` +20 %/V) ; calcul entier (D-005) ; écran par `gfx` / `text` (D-030) |

## 1. Contexte

Dans D-032, un clic sur l'encodeur droit fait tourner celui-ci sur l'entrée du réglage choisi : `--`, `CV1=`..`CV4=`
(le réglage suit l'entrée), `CV1+`..`CV4+` (l'entrée s'ajoute à la valeur du panneau). Le signal reçu est pris tel
quel. L'utilisateur demande (2026-11-18) un sous-menu qui s'ouvre au clic, où l'on choisit l'entrée, voit sa valeur
en direct et ses bornes, règle une échelle et un décalage appliqués au signal reçu, et choisit l'effet sur la valeur
du panneau : multiplier (échelle) ou ajouter (décalage).

## 2. Options envisagées

* **Effets** : `SET` / `ADD` / `MUL` (le suivi de D-032 gardé, plus la multiplication) ; ou `ADD` / `MUL` seuls.
* **Bornes** : minimum et maximum observés sur l'entrée, en lecture ; bornes réglables qui écrêtent le signal ; les
  deux.
* **Portée de l'échelle et du décalage** : par réglage (chacun le sien) ; par entrée CV (partagés par les réglages
  qui la lisent).

## 3. Ce que l'assistant a proposé

`SET` / `ADD` / `MUL`, bornes observées, échelle et décalage par réglage ; sous-menu sur une page.

## 4. Ce que nous avons décidé, et pourquoi

L'utilisateur a choisi `SET` / `ADD` / `MUL`, **les deux** sortes de bornes, la portée **par réglage**.

* **Chaîne du signal** : `s = borne(cv × SCALE + SHIFT, MIN, MAX)`, en millivolts entiers, puis l'effet :
  * `SET` : le réglage suit `s` (l'ancien `CVn=`, mêmes correspondances, même valeur à 0 V) ;
  * `ADD` : `s` s'ajoute à la valeur du panneau (l'ancien `CVn+`) ;
  * `MUL` : la valeur du panneau est multipliée par `s / 5 V` (5 V : inchangée, 0 V : nulle, puis bornée comme
    toute valeur ; un `COUNT GATE` reste `GATE`). 5 V pour l'unité, comme une enveloppe ou un LFO unipolaire de
    0 à 5 V qui atténue ; `SCALE` change cette référence au besoin.
* **Réglages du sous-menu**, par réglage qui prend une entrée (`COUNT`, `FIRST` / `SPAN`, `SHAPE`, `PROB`) :
  `INPUT` (`--`, `CV1`..`CV4`) ; `SCALE` −200 % à +200 % par 5 % (défaut 100 %, négatif inverse) ; `SHIFT`
  −10 V à +10 V par 50 mV (défaut 0) ; `MIN` et `MAX` −10 V à +10 V par 100 mV (défauts −10 V et +10 V, donc sans
  effet ; `MIN` ne dépasse pas `MAX`) ; `EFFECT` (défaut `ADD`).
* **Un cadre par-dessus l'écran principal**, pour qu'on voie que c'est une surcouche : son bord supérieur porte,
  dans deux trouées, le nom du réglage à gauche et la valeur que prendrait une rafale lancée maintenant à droite ;
  sous le cadre, le bas de la frise reste visible. Les témoins de la barre de titre sont couverts. Une seule page.
* **Champs plutôt que lignes** : le champ que tourne l'encodeur droit est dans une case allumée. Trois lignes de
  texte : l'entrée, l'échelle et le décalage lus comme une formule (`CV2 x0.50 +0.50V`) ; `MIN` et `MAX` sur une
  ligne ; `EFFECT`.
* **Deux barres empilées**, la place gagnée sur le texte :
  * `CV`, le signal, de −10 V à +10 V, un repère à 0 V : le signal **mis en forme** (après `SCALE`, `SHIFT`,
    `MIN` / `MAX`, ce que reçoit le réglage) en bande depuis 0 V et en bloc, pour que l'échelle et le décalage se
    voient ; la tension reçue, brute, en trait fin plus haut que la barre ; le minimum / maximum bruts **récents**
    (la « plage vue », voir plus bas) en ligne sur le haut ; `MIN` / `MAX`, quand ils
    bornent quelque chose, en crochets. Une première version montrait le signal brut : un `SCALE` de 0,5 n'y
    changeait rien (remarque de l'utilisateur) ;
  * `VAL`, la modulation du réglage sur toute sa plage (le nombre de 1 à `GATE`, le temps en échelle logarithmique
    comme les entrées le bougent, `SHAPE` de −3,0 à +3,0 repère au milieu, `PROB` de 0 à 100 %) : la valeur du
    panneau en trait, la valeur prise maintenant en bloc, l'écart entre les deux en bande ; avec `SET`, pas de trait
    (la valeur du panneau ne sert pas), la bande part du début ou du milieu. Les deux barres n'ont pas la même unité (volts,
    plage du réglage) : la sensibilité de chaque réglage (nombre +4/V, temps 1 V/oct…) relie l'une à l'autre.
* **Panneau** : sur l'écran principal, un clic sur l'encodeur droit ouvre le sous-menu du réglage choisi (rien pour
  `PASSTHRU`, `SYNC`, `MODE`) ; dans le sous-menu, l'encodeur gauche choisit la ligne, le droit la change, un clic sur l'encodeur droit ou `down` le ferme ; `up` lance toujours une rafale. La colonne
  d'entrée de l'écran principal montre l'opération sur la valeur du panneau : `+CVn` ou `*CVn` à côté de la valeur ;
  `=CVn` seul, sans la valeur du panneau, qui n'a plus de sens quand le réglage suit l'entrée.
* **`CENTER`** (demandé ensuite par l'utilisateur, un signal reçu de −3,6 à +4,9 V à « utiliser centré ») : un
  bouton sur la ligne d'`EFFECT` ; choisi, le clic droit règle `SCALE` et `SHIFT` pour porter la plage vue (minimum /
  maximum bruts) sur la **plage utile** du réglage, celle du signal mis en forme qui le balaie de bout en bout avec
  les correspondances ci-dessus : avec `SET`, du signal qui donne sa plus petite valeur à celui qui donne la plus
  grande (`COUNT` 0 à 7,75 V, `FIRST` −4,3 à +5,6 V autour de 100 ms, `SPAN` −4,3 à +7,6 V autour de 400 ms,
  `SHAPE` ±3 V, `PROB` 0 à 5 V) ; avec `ADD`, la même largeur **centrée sur 0 V**, donc autour de la valeur du
  panneau ; avec `MUL`, 0 à 10 V (×0 à ×2, ×1 au milieu). Les milieux se rejoignent, les largeurs aussi dans la
  limite de `SCALE` (5 à 200 %) ; arrondis aux pas des encodeurs ; rien si la plage vue fait moins de 100 mV. Le
  sous-menu reste ouvert (`down` le ferme). Ex. : −3,6 à +4,9 V sur `COUNT ADD` → ×0,90, −0,60 V, soit −3,84 à
  +3,81 V. `MIN` / `MAX` ne changent pas. L'ordre à l'écran est celui du calcul : échelle, décalage, puis bornes.
* **Plage vue glissante** : le minimum / maximum de deux tranches de 2 s, celle en cours et la précédente, soit les
  2 à 4 dernières secondes ; remise à zéro à l'ouverture du sous-menu et au changement d'entrée. Une première
  version gardait tout depuis l'ouverture : après un passage du LFO d'un sinus à un carré plus étroit sur la même
  entrée, `CENTER` retrouvait la même échelle (remarque de l'utilisateur). 2 s par tranche gardent un cycle entier
  d'un LFO jusqu'à 0,25 Hz ; plus lent, la plage n'en voit qu'une partie. Deux formes de même amplitude (sinus et
  carré d'un même LFO) donnent la même plage, donc le même `CENTER` : c'est attendu.
* **Par réglage** : deux réglages peuvent lire la même entrée avec des échelles différentes, ce qui est le cas
  d'usage (un LFO qui accélère la rafale et en réduit le nombre).
* **Non persistant**, comme les autres réglages de l'applet (D-032).

## 5. Ce qui nous ferait changer d'avis

* `MUL` à 5 V pour l'unité qui ne convient pas en situation (enveloppes à 8 V, LFO bipolaires) : changer la
  référence ou la rendre réglable.
* Des pas trop grossiers ou trop fins à l'encodeur : accélération de l'encodeur, ou pas variables.
* Le besoin de garder les assignations après coupure : les faire entrer dans les réglages persistants (D-028).
