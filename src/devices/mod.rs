//! Device detection and the common interface of every supported controller.

mod aura_usb;
mod ene_dram;

use crate::config::Config;
use crate::effect::Lighting;
use crate::smbus::Bus;

/// An independently configurable part of a device (a header, a RAM stick…).
pub struct Zone {
    /// Stable ID used in the configuration: `device-id` or `device-id/zone`.
    pub id: String,
    pub name: String,
}

pub trait Device {
    fn name(&self) -> String;
    fn zones(&self) -> Vec<Zone>;
    /// Applies the lighting of each zone (`None` = leave the zone alone).
    /// With `persist`, the controller also stores it in its own flash memory
    /// so it survives reboots even when Lueur is not running.
    fn apply(&mut self, lighting: &dyn Fn(&str) -> Option<Lighting>, persist: bool) -> Result<(), String>;
}

pub struct Detection {
    pub devices: Vec<Box<dyn Device>>,
    pub warnings: Vec<String>,
}

pub fn detect() -> Detection {
    let mut devices: Vec<Box<dyn Device>> = Vec::new();
    let mut warnings = Vec::new();

    aura_usb::detect(&mut devices, &mut warnings);

    for bus in Bus::open_all(&mut warnings) {
        ene_dram::detect(bus, &mut devices, &mut warnings);
    }

    Detection { devices, warnings }
}

/// Outcome of applying a configuration, for display.
#[derive(Default)]
pub struct Report {
    pub zones: Vec<(String, String)>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn apply(cfg: &Config, persist: bool) -> Report {
    let Detection { mut devices, warnings } = detect();
    let mut report = Report { warnings, ..Default::default() };
    for dev in &mut devices {
        for z in dev.zones() {
            report.zones.push((z.id, format!("{} — {}", dev.name(), z.name)));
        }
        if let Err(e) = dev.apply(&|zone| cfg.lighting_for(zone), persist) {
            report.errors.push(format!("{} : {e}", dev.name()));
        }
    }
    report
}
