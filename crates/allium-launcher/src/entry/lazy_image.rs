use std::path::{Path, PathBuf};

use common::constants::ALLIUM_SD_ROOT;
use log::debug;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LazyImage {
    /// Path to the file
    Unknown(PathBuf),
    /// Path to the found image
    Found(PathBuf),
    NotFound,
}

impl LazyImage {
    pub fn from_path(path: &Path, image: Option<PathBuf>) -> Self {
        match image {
            Some(image) => Self::Found(image),
            _ => Self::Unknown(path.to_path_buf()),
        }
    }

    /// Searches for the image path, caches it, and returns it
    pub fn image(&mut self) -> Option<&Path> {
        let path = match self {
            Self::Unknown(path) => path,
            Self::Found(path) => return Some(path.as_path()),
            Self::NotFound => return None,
        };

        const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "gif"];

        // Search for Imgs folder upwards, recursively. For the root directory specifically, we
        // treat /Imgs/ as /Roms/Imgs/ for searching purposes.
        // For example, if path is /path/to/game/file.ext,
        // we look for:
        // - /Roms/path/to/game/Imgs/file.png
        // - /Roms/path/to/Imgs/file.png
        // - /Roms/path/to/Imgs/game/file.png
        // - /Roms/path/Imgs/file.png
        // - /Roms/path/Imgs/to/game/file.png
        // - /Roms/Imgs/file.png
        // - /Roms/Imgs/path/to/game/file.png
        // - /Imgs/path-to/game/file.png
        let mut parent = path.clone();
        let mut image = None;
        let file_name = path.file_name().unwrap();
        'image: while parent.pop() {
            let mut image_path = parent.join("Imgs");
            if image_path.is_dir() {
                image_path.push(file_name);
                debug!("Searching for image at {:?}", image_path);
                for ext in &IMAGE_EXTENSIONS {
                    image_path.set_extension(ext);
                    if image_path.is_file() {
                        debug!("Found image at {:?}", image_path);
                        image = Some(image_path);
                        break 'image;
                    }
                }
                image_path.pop();
                image_path.extend(path.strip_prefix(&parent).unwrap());
                debug!("Searching for image at {:?}", image_path);
                for ext in &IMAGE_EXTENSIONS {
                    image_path.set_extension(ext);
                    if image_path.is_file() {
                        debug!("Found image at {:?}", image_path);
                        image = Some(image_path);
                        break 'image;
                    }
                }
            }
            if parent.as_path() == ALLIUM_SD_ROOT.as_path()
                && let Ok(rel_path) = path.strip_prefix(ALLIUM_SD_ROOT.join("Roms"))
            {
                let mut image_path = parent.join("Imgs");
                image_path.extend(rel_path);
                debug!("Searching for image at {:?}", image_path);
                for ext in &IMAGE_EXTENSIONS {
                    image_path.set_extension(ext);
                    if image_path.is_file() {
                        debug!("Found image at {:?}", image_path);
                        image = Some(image_path);
                        break 'image;
                    }
                }
                break;
            }
        }

        // If it is itself an image, use that instead
        if image.is_none()
            && let Some(ext) = path.extension().and_then(std::ffi::OsStr::to_str)
            && IMAGE_EXTENSIONS.contains(&ext)
        {
            image = Some(path.clone());
        }

        *self = match image {
            Some(image) => Self::Found(image),
            None => Self::NotFound,
        };

        match self {
            Self::Found(path) => Some(path.as_path()),
            _ => None,
        }
    }

    pub fn try_image(&self) -> Option<&Path> {
        match self {
            Self::Found(path) => Some(path.as_path()),
            _ => None,
        }
    }
}

impl From<PathBuf> for LazyImage {
    fn from(path: PathBuf) -> Self {
        Self::Found(path)
    }
}
