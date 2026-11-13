//! Screens built on the framework. Campaign screens (drama, camp, battle) plug in through
//! [`crate::flow::node_screen`].

pub mod backdrop;
pub mod battle;
pub mod camp;
pub mod controls;
pub mod credits;
pub mod drama;
pub mod duel;
pub mod error;
pub mod gallery;
pub mod gameover;
pub mod loading;
#[cfg(not(target_arch = "wasm32"))]
pub mod original;
pub mod saveload;
pub mod settings;
pub mod title;
