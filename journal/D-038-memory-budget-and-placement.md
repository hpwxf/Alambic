# D-038 — Budget mémoire du Teensy et placement des sections : le constat, la mesure, les options

| | |
|---|---|
| **Identifiant** | D-038 |
| **Date** | 2026-11-18 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | ce commit (ADR seul, aucun code) ; révision : profondeur de la pile estimée par `cargo xtask size`, moteur hors de la pile, `main` coupé en `boot` et `run` |
| **Relations** | précise D-004 (script de lien généré par `imxrt-rt`, disposition vérifiée par `cargo xtask size`) ; DMA et anneau du DAC (D-033) ; tampons de mesure à venir (capture du signal par la sonde, D-029) ; firmware de test par feature (`settings-stress`, D-028) ; applets résidents (D-021) |

## 1. Contexte

En fermant l'étape 8 (`SUPERSAW`), trois questions se sont posées ensemble (2026-11-18) : quelle place prend chaque
applet, la mémoire de code est-elle une limite dure, et où loger un grand tampon temporaire (la capture des
échantillons du DAC pour une analyse spectrale) sans empiéter sur les besoins à venir. Aucune entrée ne traitait du
budget mémoire : D-004 décide qui écrit le script de lien, pas comment la mémoire se répartit ni quand la changer.

**Le constat** (`cargo xtask size`, `3502881`). La disposition est celle de `teensy4-bsp-0.6.0/build.rs`
(feature `rt`), que le dépôt reprend sans la modifier :

```text
RuntimeBuilder::from_flexspi(Family::Imxrt1060, 1984 * 1024)
    .flexram_banks(FlexRamBanks { ocram: 0, itcm: 6, dtcm: 10 })   // 16 bancs de 32 Kio : 192 + 320
    .text(Memory::Itcm) .data(Memory::Dtcm) .bss(Memory::Dtcm) .stack(Memory::Dtcm) .vectors(Memory::Dtcm)
    .heap(Memory::Ocram) .heap_size(16 * 1024) .uninit(Memory::Ocram)
```

| Mémoire | Taille | Occupé | Libre | Contenu |
|---|---|---|---|---|
| ITCM | 192 Kio | 138,5 Kio (`.text`) | ≈ 53,5 Kio | tout le code |
| DTCM | 320 Kio | 44,6 Kio | ≈ 275 Kio | pile 16 Kio, vecteurs, `.data` 7,7 Kio, alignement 3,6 Kio, `.bss` 16,6 Kio (dont l'anneau du DMA) |
| OCRAM | 512 Kio | 46,6 Kio | ≈ 465 Kio | `.rodata` 31 Kio (copiée de la flash au démarrage), tas 16 Kio inutilisé, `.uninit` vide |
| Flash | 1 984 Kio | ≈ 185 Kio | ≈ 1 800 Kio | image chargée (boot, code, données initiales, `.rodata`) |

Trois faits en découlent :

* **La première limite atteinte sera l'ITCM**, pas la RAM de données ni la flash.
* **L'ITCM n'est pas une limite du matériel** : c'est la conséquence de `.text(Memory::Itcm)` et du partage 6 / 10 des
  bancs de FlexRAM, deux choix du BSP.
* **La DTCM n'est pas cachée, l'OCRAM l'est** (cache de données du Cortex-M7). L'anneau que le DMA lit (D-033) est en
  `.bss`, donc en DTCM, et c'est pour cela que le DMA voit les échantillons sans maintenance de cache. Déplacer `.bss`
  en OCRAM casserait ce point sans erreur de compilation.

**Le coût d'une applet.** Les symboles du binaire ne le disent pas : l'inlining verse le code des applets dans
`Engine::tick` et `main` (`llvm-nm` attribue 700 octets à `SUPERSAW`). La mesure fiable est la **différence entre deux
compilations**, avant et après le lot qui ajoute l'applet. `SUPERSAW` (banc de scies et applet), de `69e54f3` à
`3502881` : `.text` +10,5 Kio, `.rodata` +1,4 Kio, `.data` +0,3 Kio, soit ≈ 12 Kio. Au même rythme, l'ITCM libre
loge environ cinq applets de cette taille.

**La comparaison avec Phazerville.** `o_C-phazerville-v2.0.2_T40-92740448e5.hex` fait 1,4 Mo, mais un fichier Intel HEX
est du texte (deux caractères par octet, plus l'en-tête de chaque ligne) : ≈ 2,8 fois l'image binaire. Notre
`dist/oc-firmware.hex` fait 523 Ko pour ≈ 185 Kio d'image, ce qui vérifie le rapport ; Phazerville représente donc
≈ 500 Kio d'image en flash, code et données confondus. Le chiffre ne se compare pas à l'ITCM : la FlexRAM entière fait
512 Kio, et Teensyduino, qui place le code en ITCM par défaut, laisse en flash ce qui est marqué `FLASHMEM`.
Phazerville exécute donc une bonne part de son code depuis la flash (non vérifié dans ses sources).

## 2. Options envisagées

**Pour le code, quand l'ITCM manquera** (par ordre de coût croissant) :

* **A — Ne rien changer** tant que la marge le permet. Disposition du BSP, vérifiée par `cargo xtask size`.
* **B — Changer le partage des bancs** (`FlexRamBanks`), par exemple `itcm: 8, dtcm: 8` (256 + 256 Kio). Tout le code
  reste en mémoire sans attente ; la DTCM n'utilise aujourd'hui que 45 Kio. Le plafond reste 512 Kio pour code et
  données réunis.
* **C — Exécuter le code depuis la flash** (`.text(Memory::Flash)`, XIP par FlexSPI), le chemin audio et la boucle
  gardés en ITCM. ≈ 1,8 Mio de place ; mais le code en flash passe par un cache d'instructions de 32 Kio : rapide s'il y
  est, lent sinon, et un défaut de cache dans l'interruption à 47,6 kHz coûterait cher. Il faut un moyen de garder des
  fonctions choisies en ITCM (l'équivalent de `FASTRUN` de Teensyduino) : à vérifier dans `imxrt-rt`, sinon un script de
  lien complété. **Et la sauvegarde des réglages en dépend** : `flash_store.rs` efface et programme la flash par la ROM
  de boot, ce qui n'est sûr aujourd'hui que parce que rien ne lit la flash pendant ce temps (`MEMORY.md`). Avec du code
  en flash, tout ce qui peut s'exécuter pendant une écriture (le chemin de la sauvegarde, l'interruption audio et ce
  qu'elle appelle, les gestionnaires de faute) devrait rester en RAM, ou les interruptions être masquées pendant
  l'écriture.
* **D — Les données constantes en flash** (`.rodata(Memory::Flash)`) : libère l'OCRAM, pas l'ITCM ; sans objet tant que
  l'OCRAM est presque vide.

B, C et D demandent de couper la feature `rt` de `teensy4-bsp` et d'écrire le `build.rs` de `oc-firmware` avec
`imxrt-rt::RuntimeBuilder` (reprise de la main que D-004 avait prévue « si un contrôle fin du lien » devenait utile).

**Pour un grand tampon temporaire** (capture, mesure) :

* **E — Une `static` dans le firmware normal** : réserve sa taille en permanence pour un outil de mesure.
* **F — Une `static` derrière une feature de test**, placée par `#[link_section]` dans `.uninit` (OCRAM, non mise à
  zéro au démarrage), en `MaybeUninit`, de taille fixée à la compilation : elle n'existe que dans le firmware de test,
  et l'éditeur de liens refuse l'image si elle ne tient plus.
* **G — « Le reste de l'OCRAM »** entre la fin des sections et la fin de la mémoire, par les symboles du script de
  lien : la taille s'adapte, mais par de l'`unsafe` sur une zone que rien ne protège, en conflit avec un futur tas.
* **H — Le tas** (16 Kio) : trop petit, et le dépôt n'alloue pas.
* **I — Une arena** : une seule grande zone fixe (par exemple 256 Kio en `.uninit`, OCRAM), de taille constante définie
  dans `oc-core` et identique sur le module, le simulateur et Rack, pour que le comportement soit le même partout. Le
  backend la confie une fois à l'`Engine` : sur le firmware par la seule ligne `unsafe` (module à part), sur l'hôte
  par un tampon ordinaire ; `oc-core` reste `forbid(unsafe_code)` et sans `alloc`. Chaque usage (enregistrement de CV,
  ligne à retard, capture) en reçoit une part sous forme d'emprunt typé (`&mut [i16]`, …), que l'ownership empêche
  d'utiliser deux fois. Trois partages possibles :
  * **bail unique** : un seul détenteur prend toute la zone ; trivial, sans fragmentation, mais deux usages
    s'excluent ;
  * **bump par époque** : allocations successives, toutes rendues d'un coup quand l'applet active change ; l'arena
    classique, mais l'état d'une applet en arrière-plan disparaît ;
  * **parts fixes** : une table à la compilation (par exemple 192 Kio aux applets, 64 Kio à la capture) ;
    déterministe, rien ne se perd, mais la répartition est à décider.

  La tension est avec **D-021** : les applets restent résidents pour qu'un réglage survive à un détour par une autre
  applet. Si l'arena est rendue au changement d'applet, un enregistrement disparaît au premier coup d'œil ailleurs.
  Trois réponses : l'applet garde son bail jusqu'à y renoncer, et une autre qui le demande reçoit « mémoire occupée »,
  affiché à l'écran ; seule la part de l'applet active est garantie, la perte au changement étant explicite et
  annoncée ; l'enregistrement qui doit survivre va en flash (≈ 1,8 Mio libres, mais usure et durée d'écriture à
  traiter, D-028).
  Rien de ce qu'un DMA lit ne va dans l'arena : l'OCRAM est cachée.

## 3. Ce que l'assistant a proposé

Constater et mesurer plutôt que changer : garder la disposition du BSP (**A**) et noter, dans l'ordre, ce qui la
ferait changer (**B**, puis **C**) ; mesurer le coût d'une applet par différence de compilations ; pour un tampon
temporaire, **F** avec une taille modeste (256 Kio pour la capture, ≈ 2,7 s sur une voie ou 1,4 s en stéréo à
47 619 Hz), qui laisse ≈ 200 Kio d'OCRAM même dans le firmware de test.

L'arena (**I**) vient de l'utilisateur, pour les grands usages d'applet (enregistrement de CV) comme pour la capture.
L'assistant l'a jugée la bonne forme dès qu'il y aurait plus d'un grand usage, a relevé la tension avec D-021, et a
proposé de l'attendre jusqu'au premier usage réel dans une applet, en partant du bail unique.

## 4. Ce que nous avons décidé, et pourquoi

La proposition. Rien ne manque aujourd'hui, et chaque changement de disposition a un coût (un `build.rs` à maintenir à
la place de celui du BSP, ou des défauts de cache dans l'interruption) qu'aucun besoin ne justifie encore.

* **Disposition : A.** `teensy4-bsp` avec `rt`, `.text` en ITCM, bancs 6 / 10, sans modification.
* **Mesure du coût** : la différence de `cargo xtask size` entre le commit qui précède un lot et celui qui le
  termine, relevée dans l'avancement de l'étape quand le lot ajoute du code notable. Une commande
  `cargo xtask size --against <rev>` pourrait l'automatiser ; pas faite.
* **Quand l'ITCM manquera : B d'abord**, C seulement si 512 Kio de FlexRAM ne suffisent plus. Ce changement se fera par
  une révision de cette entrée.
* **Un tampon temporaire : F.** Feature de test, `.uninit` en OCRAM, taille constante, `unsafe` confiné dans un module
  à lui avec son commentaire `SAFETY:` (règle du dépôt). La capture elle-même (protocole, état, transfert) aura son
  entrée.
* **L'arena (I) attend son premier usage réel dans une applet** (enregistrement de CV, ligne à retard). Ce jour-là,
  elle se fait dans `oc-core` et remplace F : la capture y devient une part comme les autres, sans feature. Forme de
  départ : le bail unique avec refus explicite, la plus simple ; les parts fixes si deux usages doivent coexister.
  Faire l'arena avant ce besoin serait décider sa forme sans savoir ce qu'elle doit servir.
* **Ce qu'un DMA lit ou écrit reste en DTCM** (`.bss`), ou dans une section non cachée explicitement configurée ;
  jamais en OCRAM sans maintenance du cache.

## 5. Ce qui nous ferait changer d'avis

* **L'ITCM sous ≈ 16 Kio libres** (`cargo xtask size`) : passer à B, avant d'être bloqué par une applet.
* **Le code au-delà de ce que B peut donner**, la DTCM nécessaire déduite des 512 Kio de FlexRAM : C, avec le chemin audio mesuré au
  profil (`audio`) avant et après, et la sauvegarde des réglages repassée sous charge audio (TESTING 9.5c, firmware
  `settings-stress`).
* **Un besoin durable d'un grand tampon dans le firmware normal** (une ligne à retard, un échantillonneur) : la mémoire
  devient un budget à répartir entre applets, et F ne suffit plus : l'arena (I), par révision de cette entrée.
* **Deux usages de l'arena qui doivent coexister** (une applet qui enregistre pendant une capture) : passer du bail
  unique aux parts fixes.
* **Un enregistrement qui doit survivre au changement d'applet ou à l'extinction** : bail gardé par l'applet, ou
  sauvegarde en flash, à trancher alors avec D-021 et D-028.
* **Un changement de disposition par `teensy4-bsp` ou `imxrt-rt`** : la liste de contrôle de `cargo xtask size` le
  dirait ; ce constat serait alors à refaire.

## Révision (2026-11-22) — la profondeur de la pile estimée par `cargo xtask size`

L'entrée ne parlait que de l'ITCM, de la DTCM et de l'OCRAM ; la pile principale, 16 Kio au bas de la DTCM, partagée
par la boucle et toutes les interruptions, n'était surveillée par rien. Le 2026-11-22, un ajout au serveur de la sonde
(l'assignation des CV, sur la branche `probe-cv-assign`) a fait planter le module sur `cargo probe app supersaw` : le
chemin le plus profond depuis `main` (`main` 9 792 o, `Usb::serve`, `describe`, `send_spec`, `send`, le sérialiseur)
atteignait ≈ 16 260 o avant l'interruption audio, la pile sortait sous `0x2000_0000` et la première écriture faisait
faute. Sans cet ajout, le même chemin en est à 15 192 o. Rien ne le laissait voir :
les tests tournent sur l'hôte, sur une grande pile, et un cadre grossit en silence quand l'optimiseur inline une
fonction aux grandes variables locales (un `Reply` de ≈ 460 o, deux copies du framebuffer).

**Décision** : `cargo xtask size` estime la profondeur de la pile et l'ajoute à sa liste de contrôle (`stack-depth`),
par une fonction pure sur le texte de `llvm-objdump` (`xtask::stack_check`), testée sur un désassemblage figé :

* le cadre de chaque fonction lu dans son prologue (`push`, `vpush`, `str … [sp, #-n]!`, `sub sp`) ;
* les appels directs suivis (`bl`, et les appels terminaux `b.w` vers le début d'une autre fonction), le chemin de
  panique écarté (il finit dans une boucle), la récursion coupée à la première répétition ;
* le chemin le plus profond depuis `main`, plus celui de chaque gestionnaire de la table des vecteurs avec son cadre
  d'exception étendu (104 o, registres flottants compris), comme s'ils s'imbriquaient tous ; les gestionnaires qui ne
  reviennent pas (`HardFault`, le gestionnaire par défaut) écartés ;
* `FAIL` au-delà de la taille de `.stack`, `warn` sous 2 Kio de marge.

Ce qu'elle ne voit pas, et pour quoi la marge existe : les appels par pointeur (`blx rN` : objets-traits, pointeurs de
fonction), un cadre qui grandit à l'exécution, la récursion. Sur le firmware fautif elle donne `FAIL` (≈ 17 152 o,
768 o de trop). Sur celui de ce commit, `warn` : ≈ 16 064 o avec les interruptions, 320 o de marge, dont 9 776 o pour le
seul cadre de `main`.

**Écarté pour l'instant** : peindre la pile au démarrage et lire la hauteur atteinte par la sonde. Ce serait la mesure
réelle, appels par pointeur compris, mais seulement des chemins parcourus pendant l'essai ; l'estimation statique
couvre tous les chemins directs à chaque compilation. Les deux se complètent ; la mesure viendra si l'estimation et le
module divergent.

**Ce qui ferait changer d'avis** : un plantage de pile que l'estimation ne voyait pas (un appel par pointeur sur le
chemin le plus profond) : mesure sur le module ; la marge sous 2 Kio durablement : réduire le cadre de `main` (le
moteur et le serveur de la sonde hors de la pile) ou agrandir `.stack` (`TEENSY4_STACK_SIZE`), par révision de cette
entrée.

Suite, le même jour : le moteur (≈ 3,5 Kio : toutes les apps, le framebuffer, le profil) sort de la pile de `main`
pour une `static` (`StaticCell`), construit par une fonction non inlinée pour que sa valeur temporaire ne reste pas sur
le cadre de la boucle ; le serveur de la sonde y était déjà (`ConstStaticCell`). Cadre de `main` 9 776 → 6 120 o,
estimation 16 064 → 12 336 o : `pass`, 4 048 o de marge ; `.bss` +4 Kio en DTCM.

