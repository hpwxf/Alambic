# D-027 — Calibration dans `oc-core`, modèle et procédure de Phazerville

| | |
|---|---|
| **Identifiant** | D-027 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR) ; implémentation aux lots 4a (conversion) et 4d (assistant) de l'étape 4 |
| **Relations** | **révise** D-006 (sorties : affine ⇒ table par octave ; constantes ni réglables ni persistées ⇒ mesurées et persistées) et D-002 (traits `AnalogIn` / `AnalogOut`) ; sert D-026 ; s'appuie sur D-028 |

## 1. Contexte

Aujourd'hui la conversion code ↔ millivolts se fait **dans les backends** : `oc-firmware/src/cv_in.rs` applique
`CvInputCalibration`, le pilote `Dac8565` applique `CvOutputCalibration` ; `oc-core` ne voit que des millivolts.
Une procédure de calibration doit lire des codes bruts, imposer des codes de sortie et remplacer les constantes à
l'exécution ; selon D-002, elle doit vivre dans `oc-core`.

Consigne de l'utilisateur : pas de schéma exotique ; reprendre Phazerville pour rester familier. Ce que fait
Phazerville (`ebabdaa`, `OC_calibration.{h,cpp}`, `OC_ADC.cpp`, `apps/SETTINGS.h`) :

* **Sorties** : table de **11 points par canal**, une par octave de −3 V à +7 V (`calibrated_octaves[4][11]`,
  interpolation linéaire entre points). Au multimètre : point bas (octave 0, −3 V) puis point haut (octave 8,
  +5 V), interpolation des autres (appui long sur `down`), affinage facultatif octave par octave. « Start fresh? »
  repart des valeurs d'usine et recopie le canal A sur les autres.
* **Entrées** : un **zéro par canal** (`offset[4]`, nominal 2/3 de l'échelle 12 bits ≈ 2730, « ADC offset @2.2V »),
  capturé entrées débranchées (moyenne lissée, appui long pour recapturer) puis ajustable ; **un gain unique**
  (`pitch_cv_scale`) mesuré sur **CV1** en rebouclant une sortie calibrée, qui produit 1 V puis 3 V
  (`set_all_octave`) — méthode Mutable Instruments, extrapolée de deux octaves.

La plage d'entrée réelle n'est donc pas ±5 V autour de mi-échelle comme `board.rs` le suppose. Aucune mesure passée
n'est retenue ; seule la procédure produira les valeurs du module.

## 2. Options envisagées

**Frontière** (qui convertit codes ↔ millivolts) :

**A** — Les backends ne rendent et ne reçoivent que des **codes bruts** ; `oc-core` convertit avec la calibration
courante. Une seule conversion pour les trois backends ; le simulateur et Rack modélisent un convertisseur idéal
(volts ⇒ codes nominaux), donc la quantification 12 bits de l'ADC apparaît partout.

**B** — Les backends gardent la conversion ; `oc-core` leur pousse la calibration (`set_calibration`) et lit des
codes bruts pendant la procédure. Moins de changement, mais la conversion vit dans chaque backend et un backend
détient un état piloté par le cœur.

**Modèle et procédure** : **Phazerville** (ci-dessus) ou **Squares-and-Circles** (3 points 0 / −2 / +2 V par canal,
entrées par canal en rebouclage).

## 3. Ce que l'assistant a proposé

**A** et le **modèle de Phazerville** :

* sorties : table de 11 points par octave et par canal, valeurs d'usine de `board.rs` pour le firmware (dérivées de
  `kCalibrationDefaults` + `DAC_OFFSET` en tenant compte de l'inversion de l'étage, sourcées) ; assistant point bas,
  point haut, interpolation, affinage ;
* entrées : zéro par canal (capture + ajustement), gain unique mesuré sur CV1 à 1 V et 3 V par rebouclage ;
* « Start fresh? » au début, « Save values? » à la fin ;
* le modèle affine actuel reste pour les entrées (zéro par canal, pente commune) ; pour les sorties il devient un cas
  particulier de la table ;
* valeurs d'usine par backend tant que le stockage est vide ou invalide : firmware = constantes de référence ;
  simulateur et Rack = convertisseur idéal.

## 4. Ce que nous avons décidé, et pourquoi

**A** et le **modèle de Phazerville**, choisis par l'utilisateur le 2026-11-16. Phazerville parce qu'un schéma
familier rend le module accessible à ses utilisateurs. A parce qu'une seule conversion, dans le cœur, se teste une
fois et ne peut pas diverger entre backends ; le prix (trois backends à adapter, convertisseur idéal à modéliser
dans le simulateur et Rack) est payé une fois.

## 5. Ce qui nous ferait changer d'avis

* Gains d'entrée qui diffèrent nettement d'un canal à l'autre sur le module : un gain par canal (façon
  Squares-and-Circles), au prix d'un écart à Phazerville.
* Bruit d'entrée qui empêche un zéro stable : moyenne plus longue.
* Coût mesurable de la conversion dans `oc-core` à la cadence de D-023 : précalculer.

## Révision (2026-11-16, lot 4a) — une seule calibration d'usine, partagée

Écart à l'implémentation : il n'y a pas de valeurs d'usine **par backend**. `Calibration::FACTORY` (dans
`oc_core::calibration`) porte les défauts de Phazerville — entrée : zéro au code 2730, 10 V sur 4096 codes, pente
négative ; sortie : `kCalibrationDefaults` décalé de `DAC_OFFSET` puis inversé, 0 V au code 40 984 — et sert aux
trois backends. Le simulateur et Rack ne modélisent donc pas un convertisseur « idéal » arbitraire mais **celui du
module** (mocks de `oc_core::testing`, qui convertissent avec `FACTORY`) : à calibration d'usine, les trois backends
lisent et produisent les mêmes codes. C'est plus fidèle à D-002 et retire toute constante de calibration de
`board.rs`.

Conséquences mesurées : les entrées du simulateur et de Rack sont quantifiées à 12 bits (≈ 2,44 mV : 1,000 V se lit
1,001 V), la plage d'entrée devient −3,333 V…+6,665 V (au lieu de ±5 V), et le 0 V de sortie sur le module passe du
code 43 690 au code 40 984 (≈ 0,4 V d'écart à demande égale, jusqu'à la calibration). Deux écrans de référence régénérés
(`cv_passthrough`, `app_menu`), diff expliqué par ces deux effets seuls.

## Révision (2026-11-16, lot 4d) — l'assistant tel qu'implémenté

`oc_core::wizard::CalibrationWizard`, 13 étapes dans l'ordre de Phazerville : « Start fresh? », DAC A–D (point bas
−3 V = point 0 de la table, point haut +5 V = point 8), zéro ADC, gain ADC à +1 V puis +3 V sur CV1, « Save
values? ». `up` / `down` = étape précédente / suivante, encodeur droit = réglage ou choix, appui = pas grossier
(128 codes ≈ 20 mV, sinon 8 codes ≈ 1,2 mV) sur un point DAC, capture sur une étape ADC ; l'accord `up` + `down`
abandonne. Le geste « encodeur gauche au démarrage » ouvre directement l'assistant (D-026). Écarts et précisions :

* **pas d'affinage octave par octave** : la table est redessinée en droite par les deux points quand on quitte le
  point haut — mais seulement si l'un des deux a été réglé, sinon traverser les étapes réécrirait une table d'usine
  qui n'est pas exactement linéaire. L'affinage par octave (Phazerville, hors « fresh ») reste possible plus tard :
  la table le permet ;
* **copie de A** vers B, C, D en quittant le point haut de A, en mode « fresh » seulement, comme la référence ;
* **moyenne** des codes ADC sur 256 ticks, après 32 ticks ignorés à chaque changement d'étape : les sorties ne
  prennent leur nouveau niveau qu'au tick suivant, et le test de bout en bout (rebouclage OUT A → CV1) mesurait sans
  cela 816 codes au lieu de 819 entre 1 V et 3 V ;
* **fenêtres d'acceptation** : zéro à ±200 codes du code d'usine (sinon « REFUSED: UNPLUG CV IN ») ; gain à ±10 %
  de la pente d'usine (sinon « REFUSED: CHECK PATCH ») ; une seule lecture de gain ne change rien ;
* **gain commun** aux quatre entrées, chacune gardant son zéro (référence) ;
* depuis `SETUP`, l'assistant part du brouillon de l'écran système ; abandonner revient aux réglages en vigueur avant
  `SETUP`.
