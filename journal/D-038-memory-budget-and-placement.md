# D-038 — Budget mémoire du Teensy et placement des sections : le constat, la mesure, les options

| | |
|---|---|
| **Identifiant** | D-038 |
| **Date** | 2026-11-18 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | `0438059` (ADR seul, aucun code) ; révisions : profondeur de la pile estimée par `cargo xtask size`, moteur hors de la pile, `main` coupé en `boot` et `run` ; seuil de l'ITCM dans `xtask size`, runtime configuré par `oc-firmware`, code froid en `s` |
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
`dist/oc-firmware.hex` fait 523 Ko pour ≈ 185 Kio d'image, ce qui vérifie le rapport. Décodée (en-têtes de boot et
valeurs que charge `ResetHandler`), l'image de Phazerville fait **468 Kio**, et la plus grande part de son code
s'exécute **en ITCM** : ≈ 284 Kio y sont copiés au démarrage, et seuls ≈ 84 Kio restent en flash (démarrage, fonctions
`FLASHMEM`, données `PROGMEM`, descripteurs USB). Le partage des bancs le permet : le script de lien de Teensyduino
(`teensy4/imxrt1062.ld`) déclare ITCM et DTCM de 512 Kio chacune et calcule à l'édition de liens le nombre de bancs
d'ITCM d'après la taille du code (`_itcm_block_count`, arrondi aux 32 Kio), la DTCM prenant le reste. L'image écrit
`0xAAABFFFF` dans `GPR17` : 9 bancs d'ITCM (288 Kio), 7 de DTCM (224 Kio), `.rodata` et `.data` (≈ 88 Kio) en DTCM.
Teensyduino fait donc automatiquement ce que l'option **B** ci-dessous fait à la main.

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

Puis : `main` coupé en `boot()` et `run()`, non inlinées. L'optimiseur avait inliné tout le démarrage dans `main`
(l'USB, le chargement des réglages, l'OLED, le chemin audio), et ses temporaires (≈ 4 Kio) restaient sur le cadre
pendant toute la boucle. `boot()` rend ce que la boucle garde (`Running` : le moteur par référence, l'`Usb`, le
magasin de réglages) directement sur le cadre de `main`, qui le prête à `run()`. Le cadre de `main` tombe à 1 992 o.
Pendant la boucle, la pile atteint ≈ 8 464 o (≈ 9 336 o avec les interruptions), contre ≈ 11 464 o. Le chemin le plus
profond est maintenant celui du démarrage : `boot` 6 408 o et `engine_in_static` 2 328 o, ≈ 11 688 o au total avec
les interruptions, `pass` avec 4 696 o de marge. Le moteur reste une `static` : sur la pile, ses 3,6 Kio
s'ajouteraient à `Running`, donc sous les deux chemins.

Pour la vitesse, la DTCM est la même mémoire, sans cache, que la pile. Mesuré sur le module le même jour, sur les
images de la branche `probe-cv-assign` qui portent ces deux changements (`scripts/profile-supersaw.sh 10`, `NOTE` et
`DETUNE` sous bruit, trois relevés de 10 s par image ; l'image en `static` mesurée avant et après l'autre pour vérifier
la reproductibilité) ; la comparaison couvre ensemble le moteur en `static` et le découpage :

| µs, moyenne | moteur sur la pile, `main` d'un seul tenant | moteur en `static`, `boot` / `run` |
|---|---|---|
| `tick` | 16,31 | 15,46 à 15,64 |
| `update` | 4,36 (min 2,14, max 7,30) | 2,24 à 2,25 (min 2,16, max 7,3) |
| `render` | 99,5 | 100,1 à 100,3 |
| `publish` | 4,95 | 4,93 |
| `audio` | 3,87 | 3,90 |
| `voice` | 0,99 | 0,98 |
| `voice_params` | 0,34 | 0,36 |

Rien n'est plus lent. Le chemin audio, qui ne touche pas au moteur, varie de ±0,03 µs d'une image à l'autre : c'est
l'ordre de grandeur des écarts dus au code produit. `update` est deux fois plus court en moyenne, pour des extrêmes
identiques : plus de ticks prennent le chemin court. La cause n'est pas établie (le code d'`oc-core` est le même dans
les deux images ; reste ce que l'optimiseur fait de la boucle autour).


## Révision (2026-11-22) — le seuil de l'ITCM surveillé par `cargo xtask size`

Le seuil de §5 (moins de 16 Kio d'ITCM libres) n'était relevé par aucun outil : `text-itcm` restait `pass` jusqu'au
dernier octet, et les budgets affichés (192 et 320 Kio) étaient des constantes d'`oc-image` copiées du choix du BSP.
Un essai (2026-11-22, `.text` rempli par une `static` placée dans `.text.itcm_fill`) a montré que le dépassement
lui-même n'est pas un danger : à l'octet près, l'éditeur de liens refuse l'image
(`section '.text' will not fit in region 'ITCM': overflowed by 4 bytes`), et une ITCM remplie exactement passe tous
les contrôles. Ce qui manquait, c'était le préavis.

Décidé : `cargo xtask size` lit le partage réellement appliqué dans l'ELF, le symbole absolu `__flexram_config` que
le script de `imxrt-rt` définit et que son code de démarrage écrit dans `GPR17` (deux bits par banc), au lieu de le
supposer. Les budgets d'ITCM et de DTCM en découlent ; ils suivront un repartage sans qu'on touche à l'outil.
`text-itcm` passe à `warn` sous 16 Kio libres, avec le renvoi à cette entrée, et à `FAIL` si `.text` dépasse les
bancs alloués ; un contrôle `dtcm-budget` fait de même pour la fin des sections de la DTCM. Un ELF sans
`__flexram_config` est refusé : on ne vérifierait plus rien de sûr. Les constantes `ITCM_USABLE` et `DTCM_USABLE`
d'`oc-image` disparaissent, l'outil web n'en faisait pas usage.

## Révision (2026-11-22) — le runtime configuré par `oc-firmware`, hors de la feature `rt` du BSP

§2 notait que B, C et D demandent de couper la feature `rt` de `teensy4-bsp` et d'écrire le `build.rs` de
`oc-firmware` avec `imxrt-rt::RuntimeBuilder`. L'ITCM perd ≈ 21 Kio en quatre jours (32 Kio libres le 2026-11-22) :
B viendra bientôt, et l'utilisateur veut que ce jour-là le repartage ne coûte qu'une constante. C'est fait d'avance,
sans changer la disposition.

* **Ce que `rt` apportait** : la dépendance `imxrt-rt` (code de démarrage, en-têtes de boot, `#[entry]`,
  `#[interrupt]`), la feature `rt` d'`imxrt-ral` (la table des vecteurs propres à la puce), la réexportation
  `teensy4_bsp::rt`, et le `build.rs` du BSP, seul à fixer le partage 6 / 10. Rien d'autre dans le BSP n'en dépend.
* **Décidé** : `teensy4-bsp` sans `rt` ; `oc-firmware` dépend d'`imxrt-rt` (`device`, à l'exécution et au build) et
  d'`imxrt-ral` (pour sa seule feature `rt`) ; son `build.rs` reprend la configuration du BSP, le partage en constante
  (`FLEXRAM_BANKS`), le nom `t4link.x` et les surcharges `TEENSY4_STACK_SIZE` / `TEENSY4_HEAP_SIZE` gardés, pour ne
  toucher ni `.cargo/config.toml` ni les habitudes. `bsp::rt::entry` devient `imxrt_rt::entry`, et
  `teensy4_bsp::rt::interrupt` `imxrt_rt::interrupt`.
* **Vérifié** : le `t4link.x` généré est identique, octet pour octet, à celui du BSP ; `.text` aussi (`0x28020`) ;
  `cargo xtask size` lit le même `__flexram_config` (`0xfffaaaaa`).
* **Écarté pour l'instant** : attendre une surcharge en amont. `imxrt-rt` 0.1.8 n'en a pas pour le partage des bancs
  (il en a pour la pile et le tas) ; une méthode `flexram_banks_env_override` dans `imxrt-rt` et son usage par
  `teensy4-bsp` (`TEENSY4_FLEXRAM_BANKS`) serviraient d'autres cartes, et une issue sera proposée en amont. Si elle
  aboutit dans une version publiée, revenir à `rt` avec la variable dans le `[env]` de `.cargo/config.toml` se
  discutera alors ; d'ici là, rien n'en dépend.

Ce qui ferait changer d'avis : une version de `teensy4-bsp` dont le `build.rs` changerait la disposition par défaut
ne nous toucherait plus, et c'est voulu ; mais une évolution d'`imxrt-rt` qui changerait le script généré se verrait
dans la liste de contrôle de `cargo xtask size`, comme avant.

## Révision (2026-11-22) — le code froid compilé en `s`

Mesuré le même jour sur le module (`SUPERSAW`, sans CV) : tout le firmware en `opt-level = "s"` gagne 49 Kio d'ITCM
mais double `voice` (0,98 → 2,05 µs) ; en `z`, il révélait un défaut de `Lpspi::flush` (écran noir, corrigé
par `FramedSpi`). La quasi-totalité de l'écart vient d'`oc-core` et d'`oc-firmware`, où le DSP côtoie l'interface ;
`#[optimize(size)]` n'étant pas stable, la granularité est le crate.

Décidé : en `s`, par `[profile.release.package.<crate>]`, les crates que seule la boucle à 1 kHz appelle — pile USB
(`usb-device`, `usbd-serial`, `imxrt-usbd`), protocole de la sonde (`postcard`, `cobs`, `serde`, `oc-protocol`,
`heapless`), réglages (`sequential-storage`, `embassy-futures`, `futures-util`), `log`, `teensy4-panic`. `s` plutôt
que `z`, dont l'outliner est le chemin le moins éprouvé. Avec LTO fat, une fonction garde le niveau du crate qui la
définit (vérifié par la bissection de l'écran noir). Gain : `.text` −5,4 Kio (37,5 Kio libres) ; `usb` inchangé,
`voice` identique ; `audio` 3,86 → 4,10 µs en moyenne, minimum et maximum inchangés, alors que le code de
l'interruption est identique, octet pour octet : un effet de placement (les fonctions qu'elle appelle ont glissé), que
tout changement de taille du code peut produire.

Écarté pour l'instant : séparer le DSP d'`oc-core` dans un crate gardé en O3 pour passer le reste en `s`
(≈ 45 Kio de plus), qui toucherait D-044 et l'interruption d'`oc-firmware` ; l'option **B** (repartage des bancs)
donne plus, sans coût de temps, et reste la suite quand `xtask size` avertira.

## Révision (2026-11-25) — le magasin des réglages dans une `static`, hors du cadre du démarrage

Le chemin le plus profond de la pile était celui du démarrage : `main` 1 992 o, `boot` 6 400 o, `engine_in_static`
2 304 o, `AppHost::add` 840 o (≈ 11 808 o, 3 704 o libres avec les interruptions). `boot` est séparé de `run` pour que
son cadre disparaisse avant celui de la boucle (`main`), mais c'est lui qui fixait la marge. Son cadre (0x190c)
gardait surtout le magasin des réglages : le bloc de configuration de la flash (0x200), `SettingsFlash` (0x3a8) et
deux copies de `SettingsStore` (0x410), plus le `SettingsStore` porté par `Running` dans le cadre de `main`.

* **`SETTINGS: StaticCell<SettingsStore<SettingsFlash>>`**, ouvert par `settings_in_static` (non inliné), comme
  `ENGINE` par `engine_in_static` ; `Running` n'en garde qu'une référence.
* Mesuré (`cargo xtask size`) : `boot` 6 400 → 2 976 o, `main` 1 992 → 952 o ; le chemin le plus profond devient celui
  de la boucle (`main` 952, `run` 1 744, `Usb::serve` 2 864, `Server::serve_instances` 1 424, `AppHost::add` 840) :
  ≈ 8 968 o avec les interruptions, **7 416 o libres** (contre 3 704). `.bss` +4 096 o : 1 048 o pour le magasin,
  le reste en alignement des tampons USB sur 4 Kio, en DTCM (320 Kio) ; `.text` −292 o.

## Révision (2026-11-26) — `s` par défaut, `3` là où c'est justifié, le DSP dans `oc-dsp`

À la demande de l'utilisateur, la règle s'inverse : le profil `release` est en `opt-level = "s"`, et une crate ne
passe à `3` que si sa raison est écrite à côté d'elle dans le `Cargo.toml` racine. La plupart du code (moteur,
contrôle des applets, menus, texte, graphisme, USB, sonde, réglages) tourne à 1 kHz ou moins, loin de son budget, dans
l'ITCM, la mémoire rare. Avec LTO `fat`, une fonction garde le niveau de la crate où elle est **générée** : le code
générique et `#[inline]` l'est là où il est instancié ; Rust stable n'a pas de niveau par fonction (`#[optimize]`
instable).

* **En `3`** : `oc-dsp` (nouvelle crate : les coefficients préparés à chaque changement, `publish`, et les blocs par
  échantillon), `oc-firmware` (l'interruption audio et le chemin par échantillon qu'il instancie : `Voices`, les
  générateurs, inlinés depuis `oc-core` et `oc-dsp`), `oc-vcv-ffi` (le même chemin, à la cadence de Rack). Les lignes
  `s` par dépendance (code froid) deviennent inutiles.
* **`oc-dsp`** : le DSP du supersaw (`supersaw::{Supersaw, Coefficients, Oscillator, …}`), `fixed` (`log2`, `exp2`,
  `div_wide`), `phase_step` ; sans applet, moteur ni plateforme ; `oc-core` les ré-exporte aux mêmes chemins
  (`apps::supersaw::dsp`, `fixed`, `audio::phase_step`). Révision de D-044 du même jour.

Mesures (`cargo xtask size` ; module, `scripts/profile-supersaw.sh` à `DETUNE_INPUT=cv1`, `NOTE` et `DETUNE` sous le
bruit de CV1, 10 s ; µs, moyenne ; détail dans `docs/plan/avancement/budget-memoire.md`) :

| | référence (`3`, froid en `s`) | `oc-core` en `s` | + `oc-dsp` en `3` | **`s` par défaut** | `oc-firmware` en `s` aussi |
|---|---|---|---|---|---|
| ITCM libre | 26,4 Kio | 43,6 Kio | 42,2 Kio | **37,2 Kio** | 73,0 Kio |
| pile | 9 312 o | 10 272 o | 10 184 o | **11 096 o** | 9 200 o |
| `publish`, 1 / 4 `SUPERSAW` | 16,9 / 32,9 | 18,8 / 43,8 | 16,7 / 32,0 | **16,7 / 31,8** | 12,3 / 39,2 |
| `audio`, 4 `SUPERSAW` | 5,54 | 5,45 | 5,46 | **5,43** | 9,87 |
| `voice`, 4 `SUPERSAW` | 3,18 | 3,21 | 3,20 | **3,18** | 6,72 |
| `update`, 4 `SUPERSAW` | 12,1 | 17,1 | 16,8 | **17,0** | 23,0 |
| `tick`, 4 `SUPERSAW` | 24,7 | 30,5 | 30,4 | **30,4** | 48,9 |

* `oc-dsp` en `3` rend à `publish` ce que `oc-core` en `s` lui coûtait ; `voice` et `audio` ne bougent pas tant que
  `oc-firmware` est en `3`. `oc-firmware` en `s` gagnerait 36 Kio de plus, mais double `voice` et porte `audio` près de
  la moitié de la période (10,5 µs) : refusé.
* Retenu : 10,8 Kio d'ITCM de plus que la référence. Contre l'essai à `3` par défaut avec `oc-core` en `s`, 5 Kio de
  moins : `boot` (en `3`, dans `oc-firmware`) inline davantage les crates de la carte passées en `s` (11 276 →
  16 340 o) ; code froid, à reprendre si l'ITCM se resserre.
* La pile, 11 096 o (5 288 o libres) : le démarrage (`boot` 3 936 o, `engine_in_static` 3 808 o, `Instance::new`
  504 o en `s`).
* `update` et `render` (contrôle, dessin) prennent ≈ 40 % et ≈ 17 % : à 1 kHz et tous les 20 ticks, sans enjeu.

## Révision (2026-11-26, suite) — le chemin temps réel en `3`, le firmware en `s`

`boot`, compilé en `3` dans `oc-firmware`, grossissait de 5 Kio quand les crates de la carte passaient en `s` (11 276
→ 16 340 o) ; ses appels restaient les mêmes : le volume vient du code généré sur place, que `#[inline(never)]` ne
réduirait pas (une fonction ainsi marquée reste compilée au niveau de sa crate). Le levier : ne plus mettre en `3` que
ce qui doit l'être, dans une crate à part.

* **`oc-realtime`** (`3`) : l'ex-`oc-firmware/src/realtime/` (l'interruption, la boîte aux lettres, le `Publisher`),
  où s'instancient `Voices<Sound, 4>`, les générateurs et `Follower::follow` ; son `irq.rs` reste le seul `unsafe` du
  chemin (AGENTS.md).
* **`oc-board`** (`s`) : l'ex-`board.rs`, tel quel, avec `CORE_CLOCK_HZ` ; lu par le firmware et par `oc-realtime`,
  qui ne peut dépendre d'un binaire (une lib dans le paquet `oc-firmware` partagerait son niveau et ferait un cycle
  `oc-firmware` → `oc-realtime` → `oc-firmware`). Il reste le seul fichier à nommer une broche (D-007).
* **`oc-firmware`** (`s`, le défaut) : `boot`, la boucle, l'USB, les réglages, `Engine<…>` instancié.
* Les trois crates hors de `default-members` ; la CI passe clippy sur les trois.

Mesures (module, mêmes réglages que la révision précédente ; µs, moyenne) :

| | `s` par défaut, `oc-firmware` en `3` | **`oc-realtime` en `3`, `oc-firmware` en `s`** |
|---|---|---|
| ITCM libre | 37,2 Kio | **69,2 Kio** |
| pile | 11 096 o | **9 424 o** |
| `voice`, 1 / 4 `SUPERSAW` | 1,08 / 3,18 | 1,11 / 3,58 |
| `audio`, 1 / 4 | 3,93 / 5,43 | 4,15 / 5,84 |
| `voice_params`, 1 / 4 | 0,59 / 1,03 | 0,77 / 1,09 |
| `publish`, 1 / 4 | 16,7 / 31,8 | 16,6 / 33,9 |
| `update`, 4 | 17,0 | 15,7 |
| `render`, 4 | 127,7 | 133,8 |
| `tick`, 4 | 30,4 | 35,9 |

* 32 Kio d'ITCM de plus ; le chemin par échantillon perd ≈ 12 % à quatre `SUPERSAW` (`voice` 3,18 → 3,58 µs,
  `audio` 5,43 → 5,84 µs, loin de la moitié de la période, 10,5 µs), rien à une. Avec LTO `fat`, la crate racine
  fixe le niveau des passes communes (déroulage, vectorisation) : le code de `oc-realtime`, compilé en `3`, n'en
  garde qu'une part ; explication probable, non vérifiée.
* La pile du démarrage redescend (`boot` 3 016 o, `engine_in_static` 3 024 o) : 6 960 o libres.

## Révision (2026-11-27) — les appels `dyn App` suivis par `stack-depth`

**Contexte.** `stack-depth` ne suivait pas les appels par pointeur (`blx rN`). Or toutes les applets sont appelées
ainsi, par `&dyn App` (D-043), et certaines de leurs méthodes depuis des cadres profonds : le serveur de la sonde
(`instance_reply`, `Server::describe`), `AppHost::select_at` au démarrage. L'estimation ne disait rien d'une applet
dont l'`update`, le `render`, l'`act` ou le `set` serait gourmand. Mesure faite à la main (étape 14) : aucun de ces
chemins ne dépassait encore celui du démarrage (au plus 8 360 + 908 o contre 9 464 o), mais rien ne l'aurait signalé.

**Décidé (demande de l'utilisateur).** Un appel par pointeur (`blx rN`, ou `bx rN` en appel terminal) depuis une
fonction des crates du dépôt (`oc_…`) compte comme un appel à la plus profonde des méthodes `App` des applets
(`<_ as oc_core::apps::App>::*`, par ses appels directs) : une borne supérieure, la même à chaque tel appel, sans
lire les vtables. Les appels par pointeur des crates tierces (le journal `log`, les wakers du stockage) restent non
suivis : sans quoi chaque interruption, dont le seul appel par pointeur est le journal, recevrait une applet qu'elle
n'appelle jamais. Le rapport de `cargo xtask size` imprime la méthode retenue (`deepest applet method, at every dyn
App call`) ; la ligne `stack-depth` dit `direct calls and dyn App at its deepest method`.

**Effet aujourd'hui** : aucun (≈ 10 432 o) ; la méthode retenue est `SupersawApp::render`, 908 o, appelée au plus
profond depuis `select_at`, sous le chemin du démarrage.

**Ce qu'elle ne voit toujours pas** : les autres appels par pointeur, une méthode d'applet qui appelle elle-même par
pointeur (comptée par ses appels directs), un cadre qui grandit à l'exécution, la récursion. **Ce qui ferait
changer d'avis** : une borne trop pessimiste qui ferait alerter à tort (une applet au `render` lourd mais jamais
appelée profond) : lire les vtables et les méthodes réellement appelées à chaque site.
