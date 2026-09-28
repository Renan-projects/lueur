//! User configuration, stored as TOML in `%APPDATA%\Lueur\config.toml`.

use crate::effect::{Color, Effect, Lighting, Speed};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub effect: Effect,
    pub color: Color,
    pub speed: Speed,
    /// 0–100, only affects effects that use `color`.
    pub brightness: u8,
    pub reverse: bool,
    /// Per-device or per-zone overrides, keyed by the IDs printed by `lueur detect`.
    /// A zone first uses its own entry, then its device's entry, then the global values.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub devices: BTreeMap<String, Override>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Override {
    /// `false` = Lueur never touches this device/zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<Speed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            effect: Effect::Rainbow,
            color: Color::new(0x00, 0x8c, 0xff),
            speed: Speed::Normal,
            brightness: 100,
            reverse: false,
            devices: BTreeMap::new(),
        }
    }
}

const HEADER: &str = "\
# Configuration de Lueur — aussi modifiée par le menu de l'icône (zone de notification).
# Lueur recharge ce fichier automatiquement dès qu'il est enregistré.
#
# effect     : off, static, breathing, flashing, spectrum-cycle, rainbow,
#              spectrum-breathing, chase-fade, spectrum-chase-fade, chase,
#              spectrum-chase, spectrum-wave, chase-rainbow-pulse, random-flicker
# color      : \"#rrggbb\" (utilisée par static, breathing, flashing, chase, chase-fade, random-flicker)
# speed      : slowest, slow, normal, fast, fastest (barrettes de RAM uniquement)
# brightness : 0 à 100 (s'applique à la couleur)
# reverse    : sens de l'animation (barrettes de RAM uniquement)
#
# Réglages par périphérique ou par zone (identifiants donnés par « lueur detect ») :
# [devices.\"aura-usb:19af/argb1\"]
# effect = \"static\"
# color = \"#ff0000\"
#
# [devices.\"ene-dram:piix4:0x39\"]
# enabled = false   # Lueur ne touche plus à ce périphérique

";

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        base.join("Lueur").join("config.toml")
    }

    /// Loads the configuration, creating a default file on first run.
    pub fn load() -> Result<Config, String> {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{} : {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cfg = Config::default();
                cfg.save()?;
                Ok(cfg)
            }
            Err(e) => Err(format!("lecture de {} impossible : {e}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let mut cfg: Config = toml::from_str(text).map_err(|e| e.message().to_owned())?;
        cfg.brightness = cfg.brightness.min(100);
        Ok(cfg)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("création de {} impossible : {e}", dir.display()))?;
        }
        let body = toml::to_string(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, format!("{HEADER}{body}"))
            .map_err(|e| format!("écriture de {} impossible : {e}", path.display()))
    }

    /// Resolves the lighting of a zone. `None` means the zone must be left untouched.
    /// Zone IDs look like `device-id` or `device-id/zone`.
    pub fn lighting_for(&self, zone_id: &str) -> Option<Lighting> {
        let mut l = Lighting {
            effect: self.effect,
            color: self.color,
            speed: self.speed,
            brightness: self.brightness,
            reverse: self.reverse,
        };
        let device_id = zone_id.split('/').next().unwrap_or(zone_id);
        let layers = [self.devices.get(device_id), if device_id != zone_id { self.devices.get(zone_id) } else { None }];
        for o in layers.into_iter().flatten() {
            if o.enabled == Some(false) {
                return None;
            }
            l.effect = o.effect.unwrap_or(l.effect);
            l.color = o.color.unwrap_or(l.color);
            l.speed = o.speed.unwrap_or(l.speed);
            l.brightness = o.brightness.map_or(l.brightness, |b| b.min(100));
            l.reverse = o.reverse.unwrap_or(l.reverse);
        }
        Some(l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let cfg = Config::default();
        let text = format!("{HEADER}{}", toml::to_string(&cfg).unwrap());
        assert_eq!(Config::parse(&text).unwrap(), cfg);
    }

    #[test]
    fn overrides() {
        let cfg = Config::parse(
            r##"
            effect = "static"
            color = "#ff0000"
            [devices."aura-usb:19af"]
            effect = "breathing"
            [devices."aura-usb:19af/argb2"]
            color = "#00ff00"
            [devices."ene-dram:piix4:0x3a"]
            enabled = false
            "##,
        )
        .unwrap();
        let a1 = cfg.lighting_for("aura-usb:19af/argb1").unwrap();
        assert_eq!((a1.effect, a1.color), (Effect::Breathing, Color::new(255, 0, 0)));
        let a2 = cfg.lighting_for("aura-usb:19af/argb2").unwrap();
        assert_eq!((a2.effect, a2.color), (Effect::Breathing, Color::new(0, 255, 0)));
        assert!(cfg.lighting_for("ene-dram:piix4:0x3a").is_none());
        assert_eq!(cfg.lighting_for("ene-dram:piix4:0x39").unwrap().effect, Effect::Static);
    }

    #[test]
    fn rejects_typos() {
        assert!(Config::parse("efect = \"static\"").is_err());
        assert!(Config::parse("effect = \"disco\"").is_err());
    }
}
