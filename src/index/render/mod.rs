//! Terminal rendering helpers: ANSI color for types and formatted signature lines.

pub mod color;
pub mod signature;

pub use color::colorize_parameter;
pub use color::colorize_type;
pub use signature::format_enum_signature;
pub use signature::format_function_signature;
pub use signature::format_struct_signature;
