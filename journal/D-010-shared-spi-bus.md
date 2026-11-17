# D-010 — Bus SPI partagé DAC / OLED : `SharedBus` sur `RefCell`, CS en GPIO, 8 MHz, tronçons ≤ 512 octets

| | |
|---|---|
| **Identifiant** | D-010 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01, complétée le 2026-08-02) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `ec140ed` (`shared_bus.rs`, `SPI_CLOCK_HZ`), `3f50a42` (tronçons de 512 octets, `flush` avant relâchement du CS) |

## 1. Contexte

Sur la carte, le DAC8565 et l'OLED partagent MOSI (11) et SCK (13) ; chacun a son chip select (10 et 8). Le DAC
attend des mots de 24 bits ; l'OLED des rafales de commandes et de données, avec une ligne D/C. `embedded-hal` 1.0
offre `SpiDevice` (un périphérique possède son CS) et `SpiBus` (le bus brut).

## 2. Options envisagées

**A** — Deux `SpiDevice` via `embedded-hal-bus` avec CS matériel (PCS LPSPI). Le PCS matériel découpe les trames à
sa façon, ce qui complique les mots de 24 bits du DAC et la ligne D/C de l'OLED.

**B** — Un `SharedBus` maison : un `RefCell<SpiBus>` emprunté par chaque pilote le temps d'une transaction, CS
piloté en GPIO par le pilote lui-même.

**C** — Séparer physiquement les bus (I²C pour l'OLED, comme le suggère `docs/supersaw/` § 13 pour l'O.R.N.8) :
hors de portée sur une carte existante.

## 3. Ce que l'assistant a proposé

**B**. `ec140ed` : « `shared_bus` lets the DAC and the OLED share one SPI bus while each keeps its own chip select,
which `SpiDevice` cannot express ». `main.rs` : « Pin 10 is used as a plain GPIO chip select, not as the LPSPI
hardware PCS, so that the DAC's 24-bit framing stays in software. » Horloge à 8 MHz : « the slower part sets the
rate » (OLED ≈ 10 MHz max, DAC 50 MHz).

## 4. Ce que nous avons décidé, et pourquoi

**B**. Le `RefCell` est correct **parce que** le firmware est mono-contexte (D-008) : aucune transaction ne peut
en interrompre une autre. Corrections de la mise en route (`3f50a42`) : le LPSPI rejette une trame au-delà de
4 096 bits, donc `ssd130x` écrit par tronçons de 512 octets ; chaque pilote attend la fin du transfert (`flush`)
avant de relâcher son CS.

## 5. Ce qui nous ferait changer d'avis

* Dès qu'une ISR écrit le DAC pendant que la boucle principale pousse l'écran (D-023), `RefCell` peut paniquer
  (double emprunt) ou retarder le DAC d'une trame entière. Il faudra un arbitrage à deux contextes (section
  critique, drapeau `spi_busy`, ou trame OLED découpée et cédée entre deux échantillons).
* Si l'OLED supporte en pratique plus de 8 MHz, ou si le DAC passait en DMA, la contrainte « le plus lent fixe
  l'horloge » pourrait se relâcher par un changement d'horloge entre transactions.

Note : `docs/supersaw/` § 13 rapporte que le firmware Squares-and-Circles, sur ce même module et avec ce même bus
partagé, produit de l'audio sans tic lié à l'écran. Le partage est donc soluble en logiciel ; la manière n'est pas
connue (code fermé).

## Révision (2026-11-17, lot 3c2) — une page OLED par tick sur le bus

Le partage reste celui décrit plus haut (`SharedBus`, CS en GPIO, 8 MHz). Ce qui change est la taille des
transactions OLED dans la boucle : une page (≈ 145 µs mesurées, indicatif) par tick au lieu d'une trame (≈ 1,16 ms),
D-009 et D-023 (option C+). L'écriture du DAC n'attend donc plus jamais une trame entière, sauf pendant l'écran de
démarrage, où les sorties sont au repos.
