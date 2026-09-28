//! Effects, speeds and colors shared by every device.
//!
//! Effect numbers match the mode IDs used by ASUS Aura (USB and ENE SMBus)
//! controllers, so they can be written to the hardware as-is.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    Off = 0,
    Static = 1,
    Breathing = 2,
    Flashing = 3,
    SpectrumCycle = 4,
    Rainbow = 5,
    SpectrumBreathing = 6,
    ChaseFade = 7,
    SpectrumChaseFade = 8,
    Chase = 9,
    SpectrumChase = 10,
    SpectrumWave = 11,
    ChaseRainbowPulse = 12,
    RandomFlicker = 13,
}

impl Effect {
    pub const ALL: [Effect; 14] = [
        Effect::Static,
        Effect::Breathing,
        Effect::Flashing,
        Effect::SpectrumCycle,
        Effect::Rainbow,
        Effect::SpectrumWave,
        Effect::SpectrumBreathing,
        Effect::Chase,
        Effect::ChaseFade,
        Effect::SpectrumChase,
        Effect::SpectrumChaseFade,
        Effect::ChaseRainbowPulse,
        Effect::RandomFlicker,
        Effect::Off,
    ];

    pub fn hw_mode(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Effect::Off => "Éteint",
            Effect::Static => "Couleur fixe",
            Effect::Breathing => "Respiration",
            Effect::Flashing => "Clignotement",
            Effect::SpectrumCycle => "Cycle de couleurs",
            Effect::Rainbow => "Arc-en-ciel",
            Effect::SpectrumBreathing => "Respiration arc-en-ciel",
            Effect::ChaseFade => "Poursuite fondue",
            Effect::SpectrumChaseFade => "Poursuite fondue arc-en-ciel",
            Effect::Chase => "Poursuite",
            Effect::SpectrumChase => "Poursuite arc-en-ciel",
            Effect::SpectrumWave => "Vague arc-en-ciel",
            Effect::ChaseRainbowPulse => "Pulsation arc-en-ciel",
            Effect::RandomFlicker => "Scintillement",
        }
    }

    /// Whether the effect is drawn with the user's color (otherwise the
    /// hardware generates its own rainbow).
    pub fn uses_color(self) -> bool {
        matches!(
            self,
            Effect::Static
                | Effect::Breathing
                | Effect::Flashing
                | Effect::ChaseFade
                | Effect::Chase
                | Effect::RandomFlicker
        )
    }

    pub fn from_name(s: &str) -> Option<Effect> {
        toml::Value::String(s.to_owned()).try_into().ok()
    }

    pub fn name(self) -> String {
        match toml::Value::try_from(self) {
            Ok(toml::Value::String(s)) => s,
            _ => String::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Speed {
    Slowest,
    Slow,
    Normal,
    Fast,
    Fastest,
}

impl Speed {
    pub const ALL: [Speed; 5] = [Speed::Slowest, Speed::Slow, Speed::Normal, Speed::Fast, Speed::Fastest];

    /// Value of the ENE speed register (0 = fastest, 4 = slowest).
    pub fn ene(self) -> u8 {
        match self {
            Speed::Slowest => 4,
            Speed::Slow => 3,
            Speed::Normal => 2,
            Speed::Fast => 1,
            Speed::Fastest => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Speed::Slowest => "Très lente",
            Speed::Slow => "Lente",
            Speed::Normal => "Normale",
            Speed::Fast => "Rapide",
            Speed::Fastest => "Très rapide",
        }
    }

    pub fn from_name(s: &str) -> Option<Speed> {
        toml::Value::String(s.to_owned()).try_into().ok()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }

    pub fn parse(s: &str) -> Option<Color> {
        let hex = s.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some(Color::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn scaled(self, percent: u8) -> Color {
        let p = u16::from(percent.min(100));
        let f = |c: u8| ((u16::from(c) * p + 50) / 100) as u8;
        Color::new(f(self.r), f(self.g), f(self.b))
    }

    /// Win32 COLORREF (0x00BBGGRR).
    pub fn to_colorref(self) -> u32 {
        u32::from(self.r) | (u32::from(self.g) << 8) | (u32::from(self.b) << 16)
    }

    pub fn from_colorref(c: u32) -> Color {
        Color::new(c as u8, (c >> 8) as u8, (c >> 16) as u8)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).ok_or_else(|| de::Error::custom(format!("couleur invalide « {s} » (attendu : \"#rrggbb\")")))
    }
}

/// What a zone should display, fully resolved from the configuration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lighting {
    pub effect: Effect,
    pub color: Color,
    pub speed: Speed,
    pub brightness: u8,
    pub reverse: bool,
}

impl Lighting {
    /// Color actually sent to the hardware, brightness applied.
    pub fn output_color(&self) -> Color {
        self.color.scaled(self.brightness)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_roundtrip() {
        let c = Color::parse("#12aBef").unwrap();
        assert_eq!(c, Color::new(0x12, 0xab, 0xef));
        assert_eq!(c.to_string(), "#12abef");
        assert_eq!(Color::from_colorref(c.to_colorref()), c);
        assert!(Color::parse("12345").is_none());
        assert!(Color::parse("#zzzzzz").is_none());
    }

    #[test]
    fn brightness() {
        assert_eq!(Color::new(255, 100, 0).scaled(50), Color::new(128, 50, 0));
        assert_eq!(Color::new(255, 100, 0).scaled(0), Color::new(0, 0, 0));
    }

    #[test]
    fn effect_names() {
        for e in Effect::ALL {
            assert_eq!(Effect::from_name(&e.name()), Some(e));
        }
        assert_eq!(Effect::from_name("spectrum-wave"), Some(Effect::SpectrumWave));
        assert_eq!(Speed::from_name("fastest"), Some(Speed::Fastest));
    }
}
