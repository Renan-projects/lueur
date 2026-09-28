# Lueur

**Contrôleur RGB ultra-léger pour Windows.** Une alternative minimaliste à SignalRGB, iCUE ou Armoury Crate pour piloter les LED de son PC, sans rien consommer.

> *English summary below.*

| | Lueur | Logiciels RGB habituels |
|---|---|---|
| CPU au repos | **0 %** (le processus dort) | 1 à 5 % en continu |
| Mémoire | **~2 Mo privés** (~13 Mo avec les DLL système partagées) | 150 à 500 Mo |
| Taille | ~550 Ko, un seul exécutable | plusieurs centaines de Mo |
| Services en arrière-plan | aucun | un ou plusieurs |
| Télémétrie, compte, pubs | aucun | souvent |

Mesuré sur un Ryzen 5 3500X : 0 ms de CPU consommé sur 30 s au repos, détection et application en 0,1 s.

## Comment c'est possible ?

Les contrôleurs RGB savent tous animer leurs LED eux-mêmes (arc-en-ciel, respiration, vague…). Les logiciels classiques ignorent ces effets matériels et recalculent chaque image sur le processeur, 30 à 60 fois par seconde, pour les envoyer au matériel.

Lueur fait l'inverse : il **règle l'effet une fois dans le contrôleur, puis s'endort**. Il ne se réveille que lorsque :

- tu changes un réglage (menu de l'icône ou fichier de configuration) ;
- le PC sort de veille (certains contrôleurs perdent leur réglage) ;
- tu cliques sur l'icône.

Tu peux même **enregistrer le réglage dans la mémoire flash des contrôleurs** : il est alors conservé au redémarrage, même si Lueur n'est pas lancé.

## Matériel pris en charge

| Matériel | Connexion | Détails |
|---|---|---|
| Cartes mères ASUS Aura (contrôleurs USB `AULA3-*`, VID 0B05, PID 18F3 / 1939 / 19AF / 1AA6 / 1BED) | USB | en-têtes RGB 12 V, LED de la carte, en-têtes ARGB 5 V (ventilateurs, bandes) |
| RAM RGB « Aura compatible » à contrôleur ENE : ADATA XPG Spectrix, G.Skill Trident Z RGB, Geil Super Luce, Team T-Force… | SMBus (via [PawnIO](https://pawnio.eu)) | chipsets AMD (PIIX4/FCH) et Intel (i801) |

Les firmwares ENE inconnus sont volontairement ignorés : Lueur n'écrit jamais sur un composant dont il ne connaît pas exactement les registres.

Tu veux ajouter ton matériel ? Voir [Contribuer](#contribuer).

## Installation

1. Télécharge `Lueur-x.y.z-windows-x64.zip` dans les [Releases](../../releases) et décompresse-le.
2. Double-clique sur **`Installer.cmd`** et accepte l'invite administrateur.

Le script :
- installe [PawnIO](https://pawnio.eu) s'il est absent (pilote signé, open source, utilisé aussi par OpenRGB, FanControl et LibreHardwareMonitor) : c'est lui qui donne accès au bus SMBus de la RAM ;
- copie Lueur dans `%LOCALAPPDATA%\Programs\Lueur` et crée un raccourci dans le menu Démarrer ;
- active le lancement à l'ouverture de session (tâche planifiée avec privilèges élevés, donc **aucune invite UAC** à chaque démarrage) ;
- lance Lueur.

> **Ferme les autres logiciels RGB** (SignalRGB, iCUE, Armoury Crate, OpenRGB…) : deux logiciels qui pilotent le même contrôleur se marchent dessus.

Désinstallation : `uninstall.ps1` (ajouter `-Purge` pour supprimer aussi la configuration).

## Utilisation

Clic (gauche ou droit) sur l'icône arc-en-ciel de la zone de notification :

- **Effet** : couleur fixe, respiration, clignotement, cycle de couleurs, arc-en-ciel, vague, poursuite, scintillement, éteint…
- **Couleur…** : sélecteur de couleur Windows (bascule en couleur fixe si l'effet en cours n'utilise pas de couleur) ;
- **Luminosité**, **Vitesse** ;
- **Périphériques** : ce qui a été détecté, avec les identifiants de zone ;
- **Enregistrer dans le matériel** : écrit le réglage dans la mémoire des contrôleurs ;
- **Lancer au démarrage de Windows**, **Modifier la configuration…**, **Ouvrir le journal**.

### Configuration avancée

Le fichier `%APPDATA%\Lueur\config.toml` est rechargé automatiquement dès qu'il est enregistré. Il permet notamment un réglage différent par périphérique ou par zone :

```toml
effect = "spectrum-wave"
speed = "slow"

# Ventilateurs branchés sur l'en-tête ARGB 1 : bleu fixe
[devices."aura-usb:19af/argb1"]
effect = "static"
color = "#0050ff"

# Ne jamais toucher à cette barrette
[devices."ene-dram:piix4:0x3a"]
enabled = false
```

Les identifiants sont donnés par le menu **Périphériques** ou par `lueurctl detect`.

### Ligne de commande

`lueurctl.exe` (à lancer depuis un terminal administrateur pour la RAM) :

```
lueurctl detect                          liste les périphériques et leurs zones
lueurctl set static --color #ff0000      couleur fixe rouge
lueurctl set rainbow --speed fast --persist
lueurctl off                             tout éteindre
lueurctl effects                         liste des effets
lueurctl autostart on|off
```

## Limites connues

- Les effets tournent dans chaque contrôleur : ils ne sont pas synchronisés parfaitement entre la carte mère et la RAM.
- La vitesse et le sens ne sont réglables que sur la RAM (les contrôleurs USB ASUS ne l'exposent pas).
- La luminosité s'applique à la couleur choisie ; les effets arc-en-ciel sont toujours à pleine intensité.
- Pas d'effets calculés par logiciel (audio, écran…) : c'est un choix, ils imposent une boucle de rendu permanente.

## Compiler

```powershell
cargo build --release          # target\release\lueur.exe et lueurctl.exe
cargo test
.\scripts\package.ps1          # dist\Lueur-x.y.z-windows-x64.zip (télécharge les modules PawnIO signés)
```

Rust stable (MSVC). Pour tester la RAM hors installation, copier `SmbusPIIX4.bin` / `SmbusI801.bin` des [modules PawnIO](https://github.com/namazso/PawnIO.Modules/releases) à côté des exécutables.

## Contribuer

Chaque contrôleur est un fichier dans `src/devices/` qui implémente le trait `Device` (`zones()` et `apply()`). Règles du projet :

- **effets matériels uniquement** : pas de boucle de rendu ;
- **aucune écriture sans identification certaine** du composant (firmware, signature de registres) ;
- **SMBus** : toujours passer par `smbus::Bus`, qui prend le mutex système partagé `Access_SMBUS.HTP.Method`.

## Remerciements et licence

Les protocoles matériels ont été documentés par le projet [OpenRGB](https://gitlab.com/CalcProgrammer1/OpenRGB) ; l'accès au SMBus repose sur [PawnIO](https://pawnio.eu) et ses [modules](https://github.com/namazso/PawnIO.Modules) (LGPL-2.1, redistribués sans modification).

Lueur est distribué sous licence **GPL-2.0-or-later**.

---

## English summary

Lueur is a tiny (~550 KB, ~2 MB private memory, 0 % idle CPU) RGB controller for Windows. Instead of rendering effects on the CPU like most RGB suites, it programs the lighting controllers' built-in hardware effects once and sleeps until something changes (settings, resume from sleep). Settings can also be saved to the controllers' flash memory.

Supported: ASUS Aura USB motherboard controllers (12 V RGB, onboard and ARGB headers) and ENE-based "Aura compatible" RGB RAM over SMBus (AMD and Intel chipsets, through the signed [PawnIO](https://pawnio.eu) driver). Tray icon UI, TOML config with per-device/per-zone overrides, `lueurctl` CLI. UI is in French for now. GPL-2.0-or-later.
