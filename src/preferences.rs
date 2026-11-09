use std::{env, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use slint::{Color, ComponentHandle};

use crate::{Desktop, Theme};

#[derive(Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    design: u8,
    dark: bool,
    accent: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            design: 0,
            dark: true,
            accent: 0,
        }
    }
}

const ACCENTS: [Color; 3] = [
    Color::from_rgb_u8(66, 102, 167),
    Color::from_rgb_u8(56, 116, 91),
    Color::from_rgb_u8(118, 84, 154),
];

impl Preferences {
    pub fn load() -> io::Result<Self> {
        let contents = match fs::read(path()?) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let preferences: Self = serde_json::from_slice(&contents)?;
        if preferences.design > 2 || usize::from(preferences.accent) >= ACCENTS.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported theme",
            ));
        }
        Ok(preferences)
    }

    pub fn apply(&self, ui: &Desktop) {
        let theme = ui.global::<Theme>();
        theme.set_design(i32::from(self.design));
        theme.set_dark(self.dark);
        theme.set_seed(ACCENTS[usize::from(self.accent)]);
    }

    pub fn save(ui: &Desktop) -> io::Result<()> {
        let theme = ui.global::<Theme>();
        let preferences = Self {
            design: u8::try_from(theme.get_design())
                .ok()
                .filter(|design| *design <= 2)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Unsupported theme"))?,
            dark: theme.get_dark(),
            accent: ACCENTS
                .iter()
                .position(|color| *color == theme.get_seed())
                .unwrap_or(0) as u8,
        };
        let destination = path()?;
        let directory = destination.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Missing configuration directory")
        })?;
        fs::create_dir_all(directory)?;
        let temporary = destination.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temporary, serde_json::to_vec_pretty(&preferences)?)?;
        if let Err(error) = fs::rename(&temporary, &destination) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }
}

fn path() -> io::Result<PathBuf> {
    if let Some(directory) = env::var_os("RELAY_CONFIG_DIR").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(directory).join("preferences.json"));
    }
    #[cfg(windows)]
    let directory = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let directory = env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    directory
        .map(|directory| directory.join("relay-desktop").join("preferences.json"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Missing configuration directory"))
}
