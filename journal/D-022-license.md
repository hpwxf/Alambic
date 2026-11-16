# D-022 — Licence `MIT OR Apache-2.0`, jamais écrite comme décision

| | |
|---|---|
| **Identifiant** | D-022 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `1524760` (`license = "MIT OR Apache-2.0"` dans `[workspace.package]`), `40e8c3f` / `f12fd71` (`plugin.json` : `"license": "MIT"`) |

## 1. Contexte

Le code dérive des faits techniques (brochage, octets de commande du DAC8565) d'un firmware de référence publié
sous sa propre licence, sera lié au Rack SDK (dont la licence encadre les plugins), et s'appuie sur des crates
tiers. Aucun fichier `LICENSE-*` n'existe dans le dépôt.

## 2. Options envisagées

**A** — `MIT OR Apache-2.0`, convention de l'écosystème Rust, compatible avec l'intégration dans un plugin Rack.

**B** — GPL-3.0-or-later, alignée sur l'écosystème VCV et sur certains firmwares O&C dérivés.

## 3. Ce que l'assistant a proposé

Non consigné à l'époque : la valeur apparaît dans le `Cargo.toml` du premier commit, sans commentaire.

## 4. Ce que nous avons décidé, et pourquoi

**A**, de fait, pour tous les crates. Le manifeste du plugin Rack déclare `MIT` seul.

## 5. Ce qui nous ferait changer d'avis

* Une distribution du plugin par la bibliothèque VCV, dont les conditions de licence pour les plugins liés au SDK
  sont à relire au moment de publier.
* Une reprise de **code** (et non de faits) d'un firmware O&C dérivé sous GPL : A deviendrait intenable pour les
  fichiers concernés.

## Points ouverts (revue du 2026-11-16)

* Décision implicite à confirmer : pas de fichiers `LICENSE-MIT` / `LICENSE-APACHE` (présents dans `make_sound`),
  et `plugin.json` (`MIT`) contredit le workspace (`MIT OR Apache-2.0`).
* Licences des dépendances non contrôlées (pas de `cargo deny`, D-017).
* Licence du firmware de référence dont dérivent `board.rs` et `dac8565.rs` à noter dans la provenance, même si seuls
  des faits techniques ont été repris.
