#![deny(clippy::all)]
#![warn(rust_2018_idioms)]

mod allium_menu;
mod retroarch_info;
pub mod view;

pub use allium_menu::AlliumMenu;
pub use retroarch_info::RetroArchInfo;
