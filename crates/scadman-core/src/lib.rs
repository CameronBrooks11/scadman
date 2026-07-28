//! Core library for scadman — the project-oriented OpenSCAD dependency manager.
//!
//! This crate holds the data model and logic that the CLI (and, later, editor and
//! OpenSCAD integrations) build on. The design is grounded in an ecosystem survey of
//! real OpenSCAD libraries (see `docs/ecosystem-survey.md`): most libraries have no
//! releases, many assume they are installed under their own name, and the namespace is
//! flat and collision-prone. The model is shaped accordingly.

pub mod environment;
pub mod imports;
pub mod lockfile;
pub mod manifest;
pub mod resolver;
pub mod source;
pub mod store;

pub use environment::{Environment, build as build_environment};
pub use imports::{Installed, UnmetImport, unmet_imports};
pub use lockfile::{LockedPackage, Lockfile};
pub use manifest::{Dependency, GitDependency, Manifest, PathDependency, Project};
pub use resolver::{
    Conflict, Fetched, Fetcher, GitFetcher, ResolveError, Resolved, ResolvedSet, canonical_url,
    lock_staleness, resolve,
};
pub use source::{Acquired, fetch_git};
pub use store::{Store, StoreEntry, content_hash};
