# D-041 — Unité des sorties sous le millivolt : micro-volts entiers ou volts `f32` pour le chemin audio

| | |
|---|---|
| **Identifiant** | D-041 |
| **Date** | 2026-11-19 |
| **Auteur** | Pascal Havé |
| **Statut** | proposée |
| **Commits** | ce commit (ADR seul, aucun code) |
| **Relations** | révisera D-005 (unité de signal, son § 5 : « dixièmes de millivolt ou codes DAC pour l'accord fin ») ; touche D-006 et D-027 (conversion par la calibration, déjà sous le code), D-033 (`Params`, `Voice`), D-015 (ABI C en `int32_t` millivolts) ; indépendante de D-039 (l'arithmétique du banc), qui rend aujourd'hui des millivolts arrondis |

## 1. Contexte

D-005 a choisi le millivolt `i32` comme unité de tout le signal parce qu'il est « plus fin que le pas de l'ADC
(≈ 2,4 mV) et plus grossier que celui du DAC (≈ 0,14 mV) ». Le premier point vaut toujours : en entrée, le millivolt ne
perd rien, la limite est l'ADC (bruit mesuré de ±5 codes, ≈ 3 cents par code en V/oct). Le second était un constat
sans conséquence tant que les sorties portaient des niveaux de contrôle ; depuis le chemin audio (D-033) et le
Supersaw (D-037), il en a deux :

* **CV de hauteur** : 1 mV ≈ 1,2 cent à 1 V/oct, là où le DAC donne 0,17 cent. D-005 § 5 le signalait déjà.
* **Audio** : arrondir chaque échantillon au millivolt réduit la sortie à environ 9 000 niveaux (−3 à +6 V) au lieu
  des 65 536 du DAC, soit environ 3 bits de moins. Le bruit d'arrondi (≈ 1 mV / √12 ≈ 0,29 mV efficaces) est fixe ;
  ce qui compte est le niveau du signal à côté. Pour un sinus (6,02 N + 1,76 dB), à 1 V crête : ≈ 68 dB au
  millivolt contre ≈ 85 dB au code DAC ; à 100 mV : ≈ 48 contre ≈ 65 dB ; à 10 mV, une vingtaine de niveaux en tout
  (≈ 28 dB). Ces niveaux bas ne sont pas rares : `MUL` sur `LEVEL` fait du Supersaw un VCA sous enveloppe (D-036,
  D-037), et chaque fin de note les traverse.

Ce qui n'est pas mesuré : le plancher de bruit analogique de l'étage de sortie du module. S'il dépasse l'arrondi au
millivolt, une unité plus fine ne gagne rien d'audible.

## 2. Options envisagées

* **A — garder le millivolt** partout. Aucun travail ; les limites ci-dessus restent.
* **B — micro-volts `i32` pour tout ce qui sort** (±2 147 V de plage) : niveaux de `Params`, échantillons de la
  voix, entrée de `CvOutputCalibration::to_code`, et par cohérence les entrées converties (une seule unité de
  signal). Tout ce qui justifiait D-005 reste : entiers, conversions exactes, `Eq` / `Hash` sur `Params` (la
  détection de changement), tests et écrans comparés à l'égalité, déterminisme entre backends. La calibration a
  déjà une résolution sous le code (D-006) : elle n'a qu'à recevoir la finesse. Coût : un changement mécanique mais
  large (`MilliVolts` dans `platform`, `audio`, les applets, `modulation`, les tests, l'ABI C de Rack en `int32_t`
  millivolts et son module C++), et des débordements à revoir là où un produit de millivolts tenait en `i32`.
* **C — volts `f32` pour le chemin audio seulement** : la voix rend des échantillons `f32` que la calibration
  convertit directement en code DAC ; le contrôle (entrées, niveaux tenus, `Params`) reste en millivolts. Moins de
  code touché, et naturel si D-039 retient le `f32` pour le DSP (le banc ne ferait plus d'arrondi au millivolt).
  Coût : deux unités de sortie côte à côte (un niveau tenu en millivolts, un échantillon en volts), une seconde
  entrée de la calibration, des comparaisons à tolérance là où l'audio est testé, et la CV de hauteur reste au
  millivolt.

## 3. Ce que l'assistant a proposé

**B**, sous réserve de la mesure : le millivolt n'a jamais été choisi pour sa finesse, et le micro-volt garde les
propriétés qui fondaient D-005 en descendant sous le pas du DAC ; il règle aussi la CV de hauteur, que C laisse de
côté. C est l'alternative si le chantier de B paraît disproportionné, ou si D-039 adopte le `f32` et que seul
l'audio compte.

## 4. Ce que nous avons décidé, et pourquoi

Rien encore (2026-11-19). L'utilisateur garde B et C ouvertes et tranchera après la mesure de `TESTING.md` 9.3h :
le plancher de bruit de la sortie A sous `TONE` (sinus à 1 V, puis à 100 mV), comparé au bruit d'arrondi au
millivolt (≈ 0,29 mV efficaces, ≈ 68 dB sous un sinus de 1 V crête sur la bande 0–23,8 kHz).

* Plancher mesuré à ≈ 68 dB sous le sinus de 1 V (ou un plancher à 100 mV ≈ 20 dB plus haut relativement, à
  niveau absolu égal) : la sortie est limitée par l'arrondi, B ou C apportent jusqu'à ≈ 17 dB.
* Plancher nettement plus haut que l'arrondi : la sortie est limitée par l'analogique ; A suffit pour l'audio, et
  seule la CV de hauteur plaide encore pour B.

## 5. Ce qui nous ferait changer d'avis

* Une interface d'enregistrement dont le propre plancher n'est pas à au moins 15 dB sous celui qu'on mesure : la
  mesure ne tranche rien, il faut une autre chaîne (ou un firmware de test qui compare les deux unités à l'oreille).
* B qui fait déborder des calculs en `i32` dans les applets ou la modulation au point d'imposer `i64` partout : C
  redevient la voie raisonnable.
