//-- ./src/database/users/mod.rs

//! User database module for the authentication service.
//!
//! This module provides all user-related database operations, including creation, reading, updating, and deletion (CRUD).
//! It organises logic into submodules for each operation and re-exports the main `Users` model for convenient use elsewhere.
//!
//! # Contents
//! - User deletion logic
//! - User insertion/creation logic
//! - User struct definition and model-level helpers
//! - User read/query logic
//! - User update logic

// #![allow(unused)] // For development only

pub use model::Users;

mod delete;
mod insert;
mod model;
mod read;
mod update;
