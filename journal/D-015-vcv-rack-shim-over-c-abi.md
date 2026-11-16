# D-015 — Module VCV Rack 2 : coquille C++ sur `staticlib`, ABI C défensive, tick décimé à 1 kHz

| | |
|---|---|
| **Identifiant** | D-015 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01, révisée le 2026-09-03) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `40e8c3f` (ABI, plugin, `xtask vcv`), `350ee90` (façade alignée sur le module), `0c2cc90` (écran de démarrage via l'ABI), `f12fd71` (`OrnamentCrimeRust` → `OrnamentCrimeAlambic`, `vcv clean`), `a839a03` (entrée de menu contextuel pour l'accord), `190fb3f` (SDK ignoré), `8bdd9bc` (module `Diagnostic` → `Alambic`) |

## 1. Contexte

Le module doit exister dans VCV Rack 2 avec les mêmes ports et le même écran que le matériel. Le Rack SDK impose du
C++ (déclaration du module, widgets, `Makefile`). Rack exécute `process()` à la fréquence audio (44,1 à 192 kHz) ;
`oc-core` est pensé pour un tick à 1 kHz.

## 2. Options envisagées

**A** — Réécrire le comportement en C++ dans le plugin. Contraire à D-002.

**B** — Une `staticlib` Rust (`oc-vcv-ffi`) exposant une ABI C (`oc_engine_new/free/set_cv_in/set_trigger/encoder/
button/tick/cv_out/framebuffer`, plus des accesseurs de nombre de canaux), et un C++ réduit à la déclaration des
ports et à un widget qui dessine le framebuffer en rectangles NanoVG.

**C** — Bindings C++ générés (`cxx`). Plus de types partagés, plus de surface et un outil de plus dans la build.

## 3. Ce que l'assistant a proposé

**B**. Plan de fondation, Key Decision 5 (validée), et `40e8c3f` : chaque fonction tolère un pointeur nul ou un index
hors bornes et enveloppe son corps dans `catch_unwind` (avec un `AssertUnwindSafe` justifié) ; l'en-tête est
régénéré par `cbindgen` à chaque build ; l'ABI n'utilise que des types C primitifs pour que `cbindgen` n'ait jamais
à lire `oc-core`. La décimation à ≈ 1 kHz se fait par accumulateur de microsecondes dans `process()`.

## 4. Ce que nous avons décidé, et pourquoi

**B**. La build passe par `cargo xtask vcv build|install|clean` (D-016), qui compile la `staticlib`, copie l'en-tête
puis pilote le `Makefile` du SDK. `isConnected()` de chaque port est transmis à `oc_engine_set_cv_in`. Les
encodeurs sont émulés par des potentiomètres lus en **mouvement relatif**.

Le geste matériel « haut + bas ensemble » est injouable à la souris : `a839a03` ajoute une entrée de menu
contextuel qui maintient les deux boutons pendant dix ticks moteur, décomptés dans la boucle de tick et non dans
`process()` (qui tourne des dizaines de fois plus vite et ferait expirer l'appui avant qu'un tick le voie).

## 5. Ce qui nous ferait changer d'avis

* Un applet audio (D-023) : un tick moteur à 1 kHz rend des sorties en escalier ; il faudrait soit appeler la
  partie signal à la fréquence de Rack, soit interpoler côté C++ (ce qui mettrait du comportement en C++).
* Une panique attrapée qui laisse un moteur dans un état incohérent sans que l'utilisateur le voie : il faudrait un
  drapeau d'erreur dans l'ABI et un affichage dans le widget.

## Révision (2026-09-03, `8bdd9bc`)

Le module, nommé d'après son seul applet d'origine, apparaissait comme « Diagnostic » dans le navigateur de Rack
alors qu'il héberge tout le moteur. Renommé `Alambic` (slug, nom, classe, fichiers) : les patchs enregistrés avec
l'ancien slug ne retrouvent plus le module, coût jugé nul puisque rien n'est distribué. Le `DiagnosticApp` de
`oc-core` garde son nom.

## Points ouverts (revue du 2026-11-16)

* **Divergence de comportement entre backends** : Rack transmet un vrai `isConnected()`, le firmware répond toujours
  `true` (pas de détection de jack, D-019). L'écran de diagnostic affiche donc des `P` différents dans Rack et sur
  le module, contrairement à la promesse de D-002.
* `AGENTS.md` et `README.md` disent que le plugin n'a « jamais été chargé dans une instance de Rack » ; le message de
  `8bdd9bc` décrit pourtant son nom « dans le navigateur de Rack ». À confirmer, puis mettre à jour les « Known
  gaps » (panneau, émulation d'encodeur, entrée de menu).
* Le plugin n'est pas construit en CI (le SDK est téléchargé à la main) : une rupture de l'ABI côté C++ ne se voit
  qu'en local.
