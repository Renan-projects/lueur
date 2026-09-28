//! `lueurctl.exe`: command-line control, for scripts and troubleshooting.

use lueur::autostart;
use lueur::config::Config;
use lueur::devices;
use lueur::effect::{Color, Effect, Speed};
use lueur::util::is_elevated;
use std::process::ExitCode;

const HELP: &str = "\
lueurctl — pilotage en ligne de commande de Lueur

UTILISATION
  lueurctl detect                   Liste les périphériques et leurs identifiants de zone
  lueurctl apply [--persist]        Applique config.toml une fois
  lueurctl set <effet> [options]    Modifie la configuration globale puis l'applique
      --color #rrggbb   --speed <vitesse>   --brightness <0-100>   --reverse   --persist
  lueurctl off                      Éteint toutes les LED (effet « off »)
  lueurctl effects                  Liste les effets et vitesses
  lueurctl autostart on|off         Lancement de Lueur à l'ouverture de session
  lueurctl config                   Affiche le chemin du fichier de configuration

  --persist enregistre aussi le réglage dans la mémoire flash des contrôleurs :
  il est conservé au redémarrage, même sans Lueur.

  L'accès aux barrettes de RAM (SMBus) demande un terminal administrateur et PawnIO.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            print!("{HELP}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("lueurctl {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("detect") => detect(),
        Some("apply") => load().and_then(|cfg| apply(&cfg, args.iter().any(|a| a == "--persist"))),
        Some("set") => set(&args[1..]),
        Some("off") => load().and_then(|mut cfg| {
            cfg.effect = Effect::Off;
            cfg.save()?;
            apply(&cfg, false)
        }),
        Some("effects") => {
            for e in Effect::ALL {
                println!(
                    "  {:<22} {}{}",
                    e.name(),
                    e.label(),
                    if e.uses_color() { " (utilise la couleur)" } else { "" }
                );
            }
            println!("\nVitesses : slowest, slow, normal, fast, fastest");
            Ok(())
        }
        Some("autostart") => match args.get(1).map(String::as_str) {
            Some("on") => autostart::enable().map(|_| println!("Lueur se lancera à l'ouverture de session.")),
            Some("off") => autostart::disable().map(|_| println!("Lancement automatique désactivé.")),
            _ => Err("attendu : autostart on|off".into()),
        },
        Some("config") => {
            println!("{}", Config::path().display());
            Ok(())
        }
        Some(other) => Err(format!("commande inconnue « {other} » (voir lueurctl --help)")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::FAILURE
        }
    }
}

fn load() -> Result<Config, String> {
    Config::load()
}

fn warn_if_not_admin() {
    if !is_elevated() {
        eprintln!("(terminal non administrateur : la RAM RGB ne sera pas détectée)");
    }
}

fn detect() -> Result<(), String> {
    warn_if_not_admin();
    let d = devices::detect();
    if d.devices.is_empty() {
        println!("Aucun périphérique pris en charge détecté.");
    }
    for dev in &d.devices {
        println!("{}", dev.name());
        for z in dev.zones() {
            println!("    {:<28} {}", z.id, z.name);
        }
    }
    for w in &d.warnings {
        println!("avertissement : {w}");
    }
    Ok(())
}

fn apply(cfg: &Config, persist: bool) -> Result<(), String> {
    warn_if_not_admin();
    let report = devices::apply(cfg, persist);
    for w in &report.warnings {
        println!("avertissement : {w}");
    }
    println!("{} zone(s) pilotée(s){}", report.zones.len(), if persist { ", enregistré dans le matériel" } else { "" });
    if report.errors.is_empty() {
        Ok(())
    } else {
        Err(report.errors.join("\n"))
    }
}

fn set(args: &[String]) -> Result<(), String> {
    let mut cfg = load()?;
    let mut persist = false;
    let mut it = args.iter();
    let effect = it.next().ok_or("effet manquant (voir lueurctl effects)")?;
    cfg.effect = Effect::from_name(effect).ok_or_else(|| format!("effet inconnu « {effect} »"))?;
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("valeur manquante après {a}"));
        match a.as_str() {
            "--color" => {
                let v = value()?;
                cfg.color = Color::parse(v).ok_or_else(|| format!("couleur invalide « {v} »"))?;
            }
            "--speed" => {
                let v = value()?;
                cfg.speed = Speed::from_name(v).ok_or_else(|| format!("vitesse inconnue « {v} »"))?;
            }
            "--brightness" => {
                let v = value()?;
                cfg.brightness =
                    v.parse::<u8>().ok().filter(|b| *b <= 100).ok_or_else(|| format!("luminosité invalide « {v} »"))?;
            }
            "--reverse" => cfg.reverse = true,
            "--persist" => persist = true,
            other => return Err(format!("option inconnue « {other} »")),
        }
    }
    cfg.save()?;
    apply(&cfg, persist)
}
