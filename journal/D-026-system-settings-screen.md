# D-026 — Réglages système : un écran dans `oc-core`, accès calqués sur Phazerville

| | |
|---|---|
| **Identifiant** | D-026 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR) ; implémentation au lot 4c de l'étape 4 |
| **Relations** | **étend** D-021 (menu haut + bas : un item de plus) et D-020 (écran de démarrage : des gestes) ; sert D-002 (même écran sur les trois backends) ; s'appuie sur D-027 (calibration) et D-028 (persistance) |

## 1. Contexte

Sur le module, le firmware n'a aucun réglage modifiable : la calibration est figée dans `board.rs` (D-006), l'applet
de démarrage est toujours le diagnostic, l'écran et les encodeurs n'ont qu'une configuration. L'utilisateur demande
un menu de réglages système, utilisable en particulier sur le matériel, contenant une fonction de calibration, et une
persistance pour que ces réglages servent. Cible : **O&C à base de Teensy 4.0 uniquement** (le 4.1 viendra plus
tard).

Contenu demandé pour la première version : **calibration** (sorties et entrées, D-027), **applet au démarrage**
(reprendre le dernier choisi), **écran** (contraste, rotation 180°), **sens de chaque encodeur**. Accès demandé :
**menu et geste au démarrage**, ce dernier servant de secours. Consigne : pas de schéma exotique ; se rapprocher de
Phazerville pour que le module reste familier à ses utilisateurs.

Ce que font les firmwares de référence (lu le 2026-11-16) :

* **Phazerville** (`ebabdaa`, `OC_ui.cpp::Splashscreen`, `apps/SETTINGS.h`, `OC_calibration.h`) : pendant l'écran de
  démarrage, bouton de l'**encodeur gauche** maintenu ⇒ calibration ; encodeur droit ⇒ menu principal ;
  **`up` + `down`** ⇒ remise à zéro des réglages d'applets. L'écran l'affiche (`[L] => Calibrate`). La calibration
  est un assistant en étapes (`[PREV]` / `[NEXT]` sur les boutons, encodeur droit pour ajuster, appui long sur `down`
  pour mesurer ou interpoler), qui inclut aussi le décalage horizontal de l'écran et l'économiseur ; le
  retournement d'écran, le sens des encodeurs et le choix de démarrage sont des drapeaux de la même structure
  `CalibrationData`. Sauvegarde explicite, bloquante, confirmée à l'écran.
* **Squares-and-Circles** (README seul, le cœur n'est pas publié) : `LEFT` maintenu à la mise sous tension ⇒
  procédure de réglage (écran : rotation, luminosité ; sens des encodeurs ; calibration DAC puis ADC ; test d'E/S).

## 2. Options envisagées

**A** — Les réglages comme un applet de plus (`AppId::System`) dans `AppHost`, comme Phazerville. Mais un applet
reste résident et serait mémorisable comme applet de démarrage ; les réglages doivent prendre la main sur les
sorties (consignes de calibration).

**B** — Un écran à part, au même niveau que `Splash`, `App` et `Menu` dans l'`Engine` (`Screen::System`), module
`oc_core::system` ; même interaction que Phazerville, autre structure interne.

**C** — Réglages hors module (outil hôte par USB CDC). Écarté : la demande vise l'usage sur le matériel.

## 3. Ce que l'assistant a proposé

**B**, avec l'interaction de Phazerville reprise telle quelle là où elle existe :

* **Gestes au démarrage**, lus pendant l'écran de démarrage et rappelés sur celui-ci :
  * bouton de l'encodeur **gauche** maintenu ⇒ assistant de calibration (D-027) ;
  * **`up` + `down`** maintenus ⇒ réglages système **sans** les réglages d'écran et d'encodeurs chargés (secours si un
    réglage rend le module difficile à utiliser) ; rien n'est effacé tant que l'utilisateur ne sauve pas.
* **Menu** : item `SETUP` en fin du menu haut + bas, séparé des applets ; il ouvre la liste des réglages, dont
  l'entrée « Calibrate ».
* **Navigation** : celle de l'assistant Phazerville — `up` / `down` = étape précédente / suivante, encodeur droit =
  ajuster (appui = pas grossier), appui long sur `down` = mesurer ou interpoler ; dernière étape « Save values? ».
* **Sauvegarde explicite**, bloquante, sorties au repos, confirmée à l'écran.
* **Sorties** : 0 V dans l'écran système, sauf pendant une étape de calibration qui impose sa consigne.
* **Réglages v1** : applet au démarrage, contraste, rotation 180°, décalage horizontal de l'écran (« Center
  Display » de Phazerville, utile au SH1106), sens de chaque encodeur.
* **Rotation** faite dans `oc-core` (retournement du framebuffer avant `present`) : identique sur les trois backends
  et sur SH1106 / SSD1306 / SSD1309. **Contraste** et **décalage** demandent au backend `Display` deux capacités de
  plus, sans effet dans Rack et le simulateur.

## 4. Ce que nous avons décidé, et pourquoi

**B**, avec l'interaction de Phazerville proposée ci-dessus : plan accepté par l'utilisateur le 2026-11-16, avec la
consigne de coller à Phazerville pour que le module reste familier à ses utilisateurs. Un écran à part plutôt qu'un
applet, parce qu'il pilote les sorties pendant la calibration et ne doit jamais devenir l'applet de démarrage.

## 5. Ce qui nous ferait changer d'avis

* Un geste de démarrage qui entre en conflit avec un usage d'applet : décaler vers un autre geste Phazerville.
* Un applet qui a besoin de réglages propres persistants : généraliser l'écran en pages (système, puis une par
  applet) plutôt que dupliquer.

## Révision (2026-11-16, lot 4c) — ce que l'implémentation a fixé

* **Item du menu** : `SETUP` (et non `SYSTEM`), dernière entrée, une ligne vide sous les applets
  (`MenuEntry::Setup`, `MenuOutcome::OpenSetup`).
* **Brouillon** : l'écran édite une copie des `Settings` que l'`Engine` applique pendant qu'il est ouvert (aperçu
  immédiat du contraste, de la rotation, du décalage). `SAVE` garde le brouillon et lève une demande de sauvegarde
  (`Engine::take_save_request`), que le backend satisfait avec son `SettingsStore` (D-028) ; `EXIT` ou l'accord
  `up` + `down` rétablit les réglages d'entrée. Pas de confirmation de sortie : rien n'est perdu de ce qui était
  enregistré.
* **Encodeurs** : l'écran lit toujours les encodeurs tels que câblés, quelle que soit leur inversion (au lieu
  d'ignorer l'inversion du seul réglage en cours d'édition) : plus simple, et l'écran qui répare un réglage ne peut
  jamais être rendu inutilisable par lui.
* **Applet de démarrage** : une ligne explicite (`START APP`), et non « le dernier choisi » mémorisé à chaque
  lancement depuis le menu : une écriture flash par changement d'applet serait une usure sans demande explicite.
* **Gestes** : lus sur l'état anti-rebondi des boutons à la fin de l'écran de démarrage ; l'accord `up` + `down`
  tenu pendant le démarrage n'ouvre pas le menu. Rappel en bas de l'écran de démarrage :
  `L:CALIB  UP+DN:SAFE`.
* **Sorties** : l'applet est en pause et les sorties à 0 V tant que l'écran est ouvert.
* **Écran** : rotation (`FrameBuffer::rotate_half_turn`) puis décalage (`FrameBuffer::shift_columns`, ±8 colonnes)
  appliqués aux pixels dans le cœur avant `present`, sur tous les écrans, démarrage compris ; contraste par
  `Display::set_contrast` (méthode à corps vide par défaut, commande `0x81` sur SSD130x), envoyé seulement quand il
  change.
* **Calibration** : la ligne `CALIBRATE` existe mais reste inerte jusqu'au lot 4d.
