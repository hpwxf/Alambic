# D-021 — Plusieurs applets résidents, menu par accord haut + bas, haut / bas agissent au relâchement

| | |
|---|---|
| **Identifiant** | D-021 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-09-03) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `d97db13` (haut + bas = remise à zéro de l'offset), `a839a03` (menu, `AppHost`, `ButtonReader`, `ScopeApp`), `0b0fe3b` (`apps/` en module, `InputSnapshot` et `TickContext` partagés) |

## 1. Contexte

`oc-core` n'avait qu'un applet codé en dur (`DiagnosticApp`, champ de `Engine`), sans identifiant ni registre.
L'O&C est un module multi-applets ; la façade n'a que deux boutons et deux encodeurs à poussoir, déjà tous utilisés
par le diagnostic. Il faut un geste pour changer d'applet, et décider du sort de l'état d'un applet quand on le
quitte.

## 2. Options envisagées

Pour le geste : appui long sur un bouton (temporisation à régler), ou **accord haut + bas**. Problème de l'accord :
haut et bas ont déjà chacun une action ; l'anti-rebond s'établit en 3 échantillons alors qu'un humain qui appuie
« ensemble » espace les deux appuis de 20 à 50 ms — le premier bouton agit bien avant que l'accord soit détectable.

Pour l'état : un `enum` d'applets (un seul vivant, l'état des autres perdu), ou **tous les applets résidents** dans
un `AppHost`, seul le courant étant mis à jour.

Pour le menu : écran plein qui suspend l'applet, ou **surimpression** pendant laquelle l'applet continue de tourner
et ses sorties restent vivantes.

## 3. Ce que l'assistant a proposé

Le message de `a839a03` (co-signé Claude) argumente chaque point : haut / bas **différés au relâchement** et annulés
si l'accord s'est formé pendant l'appui (« There is no timeout to tune, and the behaviour is identical at any tick
rate ») ; encodeurs à poussoir toujours sur l'appui (« they take no part in the chord ») ; applets résidents (« an
offset dialled into the diagnostic screen would vanish on a glance elsewhere and back ») ; menu en surimpression
(`InputSnapshot::silence_controls`) pour qu'ouvrir le menu n'interrompe jamais un patch. `ScopeApp`, volontairement
minimal, recopie CV1 sur les quatre sorties — signature qu'aucun mode du diagnostic ne produit, pour qu'une seule
assertion prouve la bascule.

## 4. Ce que nous avons décidé, et pourquoi

Les quatre propositions. Le coût de mise à jour reste constant (seul l'applet courant tourne), le firmware et l'ABI
n'ont pas changé, le simulateur gagne la touche `m` et Rack une entrée de menu contextuelle (D-015).
`Engine::app()` devient `diagnostic()` : « keeping a method called `app()` that returns the diagnostic applet while
another one runs would be an API lie ». `0b0fe3b` regroupe ensuite les applets sous `apps/` et y place
`InputSnapshot` / `TickContext`, contrat commun des applets.

## 5. Ce qui nous ferait changer d'avis

* Si le différé au relâchement se révélait désagréable au jeu (latence perçue sur haut / bas), il faudrait une
  fenêtre d'accord courte et bornée plutôt que le relâchement.
* Si le nombre d'applets ou leur état grossissait au point de peser sur la RAM (DTCM 320 Kio), les applets résidents
  devraient pouvoir sérialiser / restaurer leur état au lieu de rester tous vivants.
* Avec D-023, un applet devra déclarer sa partie signal et sa partie UI : le contrat `InputSnapshot` / `update` /
  `render` devra se scinder.

## Révision (2026-09-03, `a839a03`)

La veille, `d97db13` avait attribué l'accord haut + bas à la remise à zéro de l'offset du diagnostic. `a839a03`
réattribue l'accord au menu et supprime cette remise à zéro, déjà offerte par l'appui sur l'encodeur droit.

## Points ouverts (revue du 2026-11-16)

* Le différé au relâchement n'a été éprouvé qu'au simulateur et en tests ; à vérifier sur le module (niveau 9) et
  dans Rack, où l'accord passe par un menu contextuel.
