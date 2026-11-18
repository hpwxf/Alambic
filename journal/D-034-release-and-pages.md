# D-034 — Publication : le firmware à chaque étiquette, la page web sur GitHub Pages

| | |
|---|---|
| **Identifiant** | D-034 |
| **Date** | 2026-11-18 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ce commit (ADR, `.github/workflows/release.yml`, `.github/workflows/pages.yml`) |
| **Relations** | prolonge D-017 (CI, image firmware en artefact) et D-031 (sonde et flasheur dans le navigateur) ; garde la barrière d'avant flash de D-016 |

## 1. Contexte

La CI (D-017) construit l'image du firmware à chaque poussée, mais seulement en artefact de 90 jours, attaché à une
exécution. Une version (0.5.0 et suivantes) n'a donc pas d'image durable à flasher, et la page de D-031 (sonde par
Web Serial, flash par WebHID) ne se construit qu'en local (`cargo xtask web`), alors qu'elle n'a besoin que d'un
contexte sûr (HTTPS) pour fonctionner. L'utilisateur demande (2026-11-18) un build du firmware à chaque étiquette et
la page publiée sur GitHub, comme pour `hpwxf/pdf-shrinker` (déploiement Pages par GitHub Actions).

## 2. Options envisagées

* **Firmware** : artefacts d'exécution seulement (l'existant) ; ou une *release* GitHub par étiquette `v*`, avec les
  images attachées ; et, pour les images, l'ELF seul, le HEX seul, ou les deux, une ou plusieurs variantes d'écran.
* **Page** : déployée à la main depuis `master` (`pdf-shrinker`) ; à chaque étiquette ; ou à chaque poussée sur
  `master`.

## 3. Ce que l'assistant a proposé

Deux workflows :

* `release.yml`, sur une étiquette `v*` (ou à la main, l'étiquette en paramètre) : vérifier que l'étiquette est la
  version du workspace, construire et mesurer (`xtask build`, `size`), passer la barrière d'avant flash
  (`xtask flash --dry-run`), puis attacher à une *release* l'ELF et le HEX de l'écran par défaut (SH1106) et le HEX
  des variantes SSD1306 et SSD1309, avec leurs empreintes SHA-256 ; les notes viennent de la section du
  `CHANGELOG.md` de cette version ;
* `pages.yml`, à chaque étiquette `v*` et à la main : construire la page (`xtask web`, CLI `wasm-bindgen` à la
  version de `Cargo.lock`) et la déployer sur GitHub Pages. La page publiée correspond donc à la dernière version, pas
  à un état intermédiaire de `master`.

## 4. Ce que nous avons décidé, et pourquoi

La proposition. Les raisons :

* **Une release par étiquette** : l'image d'une version doit rester téléchargeable, avec son empreinte, sans
  dépendre de la durée de vie d'un artefact ; l'ELF garde toutes les vérifications de la barrière (D-016), le HEX est
  ce que demandent `teensy_loader_cli` et la page (D-031).
* **Les variantes d'écran** : ce sont les seules features d'usage (`ssd1306`, `ssd1309`) ; `oled-spi-8mhz` et
  `settings-stress` sont des outils de mise au point, pas des images à distribuer.
* **La page sur étiquette** : la page flashe des images de release ; la publier au même moment garde les deux
  accordées. Le déclenchement manuel reste pour republier sans nouvelle version.
* **Une étiquette posée avant ces workflows** (`v0.5.0`, sur `e645c63`) ne les déclenche pas : GitHub lit le
  workflow dans le commit étiqueté. On les lance alors à la main avec l'étiquette en paramètre ; le firmware est bien
  construit depuis l'étiquette.
* **Réglages uniques** : Settings › Pages › Build and deployment › Source : « GitHub Actions » (la page est ensuite à
  `https://hpwxf.github.io/Alambic/`) ; et, pour le déploiement sur étiquette, Settings › Environments ›
  `github-pages` › Deployment branches and tags : ajouter la règle d'étiquette `v*` (par défaut seule la branche par
  défaut peut déployer). Lancé à la main depuis `master`, l'étiquette en paramètre `ref`, il n'en faut pas plus.

## 5. Ce qui nous ferait changer d'avis

* Des utilisateurs d'autres cartes (Teensy 4.1, D-003) : des images par carte.
* Le plugin VCV Rack à distribuer : il demande le SDK Rack dans la CI et une construction par plateforme.
* Une page qui doit suivre `master` (essais entre deux versions) : déploiement sur poussée, ou une seconde page.
