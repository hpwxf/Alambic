# D-018 — Profil release : LTO, `overflow-checks` actifs ; une panique arrête le module (SOS)

| | |
|---|---|
| **Identifiant** | D-018 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective) |
| **Commits** | `1524760` (`[profile.release]`), `ec140ed` (`teensy4-panic`), `3f50a42` (`teensy4-panic` 0.3 + `log`) |

## 1. Contexte

Le firmware tourne sans OS ; une panique n'a nulle part où remonter. Il faut choisir ce que fait le module sur une
erreur arithmétique, et avec quel profil il est construit (taille, vitesse, informations de débogage).

## 2. Options envisagées

**A** — Profil release classique : `overflow-checks = false`, arithmétique enveloppante silencieuse en cas de
débordement. Le module continue, avec une valeur fausse.

**B** — `overflow-checks = true` en release : tout débordement panique ; `teensy4-panic` journalise le message (USB,
feature `log`) puis fait clignoter un SOS sur la LED. Le module s'arrête ; le DAC garde son dernier code.

**C** — `overflow-checks = false` en release, mais arithmétique explicitement saturante (`saturating_*`) dans le
chemin signal, et les débordements traqués en tests (profil `dev`/`test`, `proptest`).

## 3. Ce que l'assistant a proposé

**B**, non argumenté à l'époque (`1524760`). Le reste du profil est commenté : `debug = 2` « costs nothing in
flash » et sert à `cargo xtask size` ; `lto = "fat"`, `codegen-units = 1`, `opt-level = 3`.

## 4. Ce que nous avons décidé, et pourquoi

**B**, de fait. Raison plausible (non consignée) : en phase de mise au point, un arrêt visible vaut mieux qu'une
tension fausse silencieuse, et le code de `oc-core` est déjà écrit en arithmétique saturante et bornée
(`clamp_duration`, `saturating_add` des compteurs).

## 5. Ce qui nous ferait changer d'avis

* Le premier usage musical : un module qui se fige au milieu d'un patch, sorties gelées à une tension arbitraire
  (jusqu'à +6 V), est pire pour l'utilisateur qu'un compteur qui reboucle. Signal : le premier applet destiné au
  jeu, pas au diagnostic.
* Une panique dans une ISR (D-023) : le SOS arrêterait aussi la boucle UI, donc l'écran ne dirait rien.

## Points ouverts (revue du 2026-11-16)

* Décision à confirmer explicitement, avec un critère de sortie (par exemple : `overflow-checks` en release tant que
  le module n'exécute que des applets de diagnostic ; ensuite C, avec un compteur d'erreurs visible plutôt qu'un
  arrêt). Le profil est partagé par tout le workspace : il s'applique aussi à `xtask` et à la `staticlib` VCV, où
  une panique attrapée par `catch_unwind` laisse un moteur « périmé » (D-015).
