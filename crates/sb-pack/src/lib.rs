//! Shader pack loading for ShaderBridge.
//!
//! * [`vfs`]: read-only file systems over a pack's `shaders/` root — directory
//!   ([`DirVfs`]), zip ([`ZipVfs`]) or memory ([`MemVfs`]) — with a recorded
//!   case-insensitive fallback for packs developed on Windows.
//! * [`ShaderPack`]: an opened pack with program discovery per folder
//!   ([`ShaderPack::program_set`] → [`ProgramSet`]), world folders, lang files and a
//!   content hash.
//! * [`properties`]: a faithful `java.util.Properties` parser.
//! * [`shaders_properties`]: typed `shaders.properties` ([`ShadersProperties`]).
//! * [`options`]: option discovery (Iris rules), user values, profiles, the GUI model,
//!   and [`EditedSources`], the [`sb_core::SourceProvider`] with options applied.
//! * [`idmap`]: the id map files (`block`, `item`, `entity` and `dimension.properties`).
//! * [`lang`]: the language files (`lang/<code>.lang`).
//!
//! This crate never preprocesses: callers run `.properties` files through the
//! preprocessor (with [`DiscoveredOptions::property_macros`] and environment macros)
//! before handing the text to the typed parsers.
//!
//! Typical flow:
//!
//! ```
//! use sb_pack::{ShaderPack, options, properties, shaders_properties};
//! use std::sync::Arc;
//!
//! let pack = ShaderPack::from_files("demo", [
//!     ("shaders.properties", "sun=false\nscreen=SHADOWS"),
//!     ("composite.fsh", "#define SHADOWS\n#ifdef SHADOWS\n#endif\nvoid main() {}"),
//! ]);
//! let (opts, _diags) = options::discover(&pack, &pack.option_start_files());
//! let raw = properties::parse(&pack.read_latin1("shaders.properties").unwrap());
//! // (the preprocessed text would normally differ from the raw one)
//! let (props, _diags) = shaders_properties::parse(&raw, &raw);
//! assert!(!props.settings().sun);
//!
//! let values = options::OptionValues::from_pairs([("SHADOWS", "false")]);
//! let sources = options::EditedSources::new(Arc::new(pack), opts, values);
//! use sb_core::SourceProvider;
//! assert!(sources.read("composite.fsh").unwrap().starts_with("//#define SHADOWS"));
//! ```

pub mod error;
pub mod idmap;
pub mod includes;
pub mod lang;
pub mod options;
mod pack;
pub mod programs;
pub mod properties;
pub mod shaders_properties;
pub mod text;
pub mod vfs;

pub use error::PackError;
pub use includes::IncludeGraph;
pub use options::{DiscoveredOptions, EditedSources, OptionValues, Profile};
pub use pack::{STANDARD_WORLD_FOLDERS, ShaderPack};
pub use programs::{ProgramSet, ProgramSources};
pub use properties::PropEntry;
pub use shaders_properties::ShadersProperties;
pub use vfs::{CaseFallback, DirVfs, MemVfs, Vfs, ZipVfs};
