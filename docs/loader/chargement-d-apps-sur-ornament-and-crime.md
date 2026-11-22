# Charger des apps en binaires indépendants sur O&C / Teensy 4.0

*Notes de travail — étude du loader de Squares-and-Circles, et options pour Alambic*

---

## 1. Le point de départ

Le module dispose actuellement du firmware **Squares-and-Circles** (eh2k), qui organise ses fonctionnalités en quatre instances d'engines chargeables indépendamment (synthé, filtre, séquenceur, effet…), chacune compilée en binaire séparé et chargée dynamiquement par un loader propriétaire.

Test effectué sur le matériel : le moteur M-OSC/Waveforms, formes CSAW et consorts, sonne proprement — aucun tic lié au rafraîchissement de l'écran, alors que le DAC8565 et l'OLED partagent le même bus SPI (mêmes broches 11/13 que celles utilisées par Alambic, cf. `crates/oc-firmware/src/board.rs`). Ce résultat, documenté dans `docs/supersaw/supersaw-sur-ornament-and-crime.md` (§13), écarte l'hypothèse d'un défaut de câblage propre à cet exemplaire : le partage de bus est gérable en logiciel sur le matériel O&C stock.

Cette observation a amené une question distincte du sujet supersaw : **comment S&C encapsule-t-il chaque engine en binaire indépendant, et cette approche a-t-elle un intérêt pour Alambic ?**

---

## 2. Comment Squares-and-Circles encapsule ses engines

Le code source des engines est public (`squares-and-circles/src/*/*.cpp` → `app/*/*.bin`), le **loader qui les charge est fermé**. Ce qui est observable :

* **Compilation isolée.** `src/build.sh` compile chaque `.cpp` séparément avec `arm-none-eabi-gcc`, puis le passe à `mkmodule` (outil du sous-module `lib/udynlink`), qui produit un `.bin` autonome, non lié à une adresse finale.
* **Surface d'ABI minimale.** Chaque engine n'exporte que six symboles : `setup`, `process`, `draw`, `screensaver`, `__ui_event_handler`, `__midi_event_handler`. `src/squares-and-circles-api.h` définit en retour ce que l'engine peut appeler côté firmware (constantes de routage `OUT_CV`, `OUT_AUDIO`…, fonctions utilitaires).
* **Manifeste.** `app/index.json` liste les `.bin` disponibles ; `app/squares-and-circles-loader.sha` pin le commit du loader (fermé) contre lequel cette version de l'API a été validée — gestion explicite de la compatibilité ABI entre deux dépôts qui évoluent séparément.

Cette architecture répond à un besoin précis : **distribuer des engines à des utilisateurs qui ne recompilent jamais le firmware complet**. C'est un besoin de plateforme communautaire, pas nécessairement celui d'un firmware personnel.

---

## 3. Le principe d'udynlink

`lib/udynlink` (sous-module cloné, https://github.com/eh2k/udynlink, auteur original Bogdan Marinescu 2016, Apache-2.0) implémente un **linker à indirection façon GOT**, pas un chargeur ELF générique. Environ 1400 lignes au total, dont le cœur runtime tient dans 436 lignes (`udynlink/udynlink.c`) + 179 lignes d'en-tête.

### Format et mécanique

* Format binaire maison (signature `"UDLM"`), pas de l'ELF brut à l'exécution : `mkmodule` le fabrique en amont à partir de l'ELF produit par GCC.
* Le compilateur fait le gros du travail : tout accès à un symbole externe ou global passe par une entrée de la **LOT** (*Linkage Offset Table*, l'équivalent d'une GOT), via la relocation `R_ARM_GOT_BREL`. Au chargement, seule la LOT est réécrite — le code machine lui-même n'est quasiment pas touché.
* `mkmodule` (script Python, 477 lignes) lit les relocations de l'ELF compilé (`readelf`/`objdump` parsés en texte) et les classe en trois familles :
  * `R_ARM_GOT_BREL` → entrée LOT (locale ou externe),
  * `R_ARM_ABS32` / `R_ARM_TARGET1` → pointeurs bruts en `.data`, patchés par un delta,
  * `R_ARM_THM_CALL` / `R_ARM_THM_JUMP24` → branchements PC-relatifs internes, **ignorés** (l'encodage reste valide quelle que soit l'adresse de chargement).
* Au runtime, `udynlink_load_module` alloue la RAM nécessaire, copie code/données selon le mode (copie totale, copie du code seul, ou exécution en place), met à zéro le BSS, puis boucle sur la table de relocations : pour chaque entrée, calcule un offset relatif au code/données rechargés, ou appelle `udynlink_external_resolve_symbol(nom)` — callback fourni par le firmware hôte qui expose une table de symboles fixe (pas de `dlopen`, juste un lookup nom → adresse).
* Le C++ minimal : pas de `crt0` complet, `udynlink_cpp_init` appelle `__init_array` à la main pour les constructeurs globaux.

### Ce qui est simple, ce qui ne l'est pas

Le fichier `udynlink.c` en lui-même n'est pas compliqué — une boucle de patch de table, de l'ordre d'un jour de travail pour quelqu'un à l'aise avec les relocations ARM. La difficulté réelle est ailleurs :

1. **`mkmodule`** doit dresser le compilateur pour qu'il ne produise *que* les catégories de relocations que le loader sait gérer. D'où les flags `-fno-exceptions -fno-rtti --no-opt` dans `build.sh` : sans eux, GCC génère des relocations (RTTI, tables d'exceptions, optimisations qui cassent l'hypothèse d'indirection GOT) que `mkmodule` refuse (`error()` sur tout type inconnu). Ce dressage a demandé des itérations réelles.
2. **Portage vers une toolchain Rust/LLVM.** `mkmodule` parle le format de relocations GCC. rustc/LLVM sait générer du code position-indépendant sur Cortex-M (`-C relocation-model=pic`), mais ce chemin est beaucoup moins éprouvé sur cette cible que le PIC ARM classique de GCC — il faudrait ré-écrire l'équivalent de `mkmodule` contre les relocations LLVM et valider empiriquement lesquelles sont réellement émises en pratique.
3. **La table de symboles hôte** est le coût récurrent, pas ponctuel : chaque fonction/variable appelable par un engine doit être explicitement exportée et versionnée — d'où le fichier `.sha` de pin de compatibilité.

---

## 4. Options pour un firmware personnel

La question posée : d'autres façons de charger des « apps », chacune encapsulée dans un binaire unique, adaptées à un firmware **personnel** plutôt qu'à une plateforme distribuée à des tiers.

| Option | Principe | Relocation runtime | Simultanéité | Coût principal |
|---|---|---|---|---|
| **A. Firmware monolithique multi-apps** | Toutes les apps compilées ensemble (comme l'O&C original Mutable Instruments), sélection au menu via un enum/trait-object | Aucune | Autant que voulu, au choix du dispatch | Ajouter une app = recompiler + reflasher tout |
| **B. Multiboot / chainload à adresses fixes** (façon MCUboot) | Chaque app = binaire Rust `no_std` autonome, sa propre table de vecteurs, liée à une adresse flash fixe par script linker ; un petit stage résident saute dessus | Aucune (adresse connue au link) | Une seule app active à la fois | Une app = un build par emplacement flash si on veut la déplacer |
| **C. Slots fixes + table de fonctions à adresse connue** | Variante de B qui reste « vivante » : N régions flash fixes, chaque app liée pour son slot, un mot de tête = vtable de pointeurs (`setup`/`process`/…) appelée directement par le firmware hôte | Aucune | Plusieurs slots simultanés, comme S&C | Une app = un build par slot cible |
| **D. PIC générique** (équivalent udynlink en Rust/LLVM) | `-C relocation-model=pic` + réécriture de `mkmodule` contre les relocations LLVM | Oui, complète | Plusieurs, apps interchangeables entre slots sans recompiler | Chemin peu rodé sur Cortex-M bare-metal ; effort de dressage compilateur comparable à celui d'eh2k côté GCC |
| **E. VM bytecode** (Lua embarqué, wasm3/WAMR, interpréteur maison) | Apps = données interprétées, position-indépendantes par nature | Aucune (pas de code natif chargé) | Oui | Overhead d'interprétation incompatible avec un budget audio temps réel (7 voix supersaw à 48 kHz) ; utilisable seulement pour des apps non temps-réel (séquenceur, config) |

### Recommandation

Pour un firmware personnel, l'option **A** évite l'essentiel du problème que résout udynlink — problème qui existe chez eh2k pour une raison qui ne s'applique pas ici : distribuer des engines à des utilisateurs qui ne recompilent jamais le firmware. `cargo xtask flash` rend déjà le cycle recompiler-reflasher rapide et sûr (validation d'image, confirmation interactive, dry-run) ; le coût qu'évite un vrai chargeur dynamique est donc déjà faible dans ce contexte.

Si le besoin de simultanéité façon S&C (plusieurs engines actifs en même temps, mixés) se confirme, l'option **C** est le compromis à privilégier avant d'aller vers un PIC générique (D) : elle donne l'essentiel du bénéfice (apps chargées/mixées à l'exécution) sans le coût d'ingénierie d'un vrai relocateur, au prix d'un build par slot si une app doit pouvoir occuper plusieurs emplacements.

---

## 5. Décision en attente

Le choix entre A et C dépend d'une question fonctionnelle non tranchée : le firmware personnel doit-il permettre **un seul app actif à la fois** (menu de sélection, comme l'O&C d'origine), ou **plusieurs slots simultanés mixés** (comme les quatre instances d'engines de S&C) ? Cette réponse détermine directement l'architecture à retenir.
