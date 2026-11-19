//! Reading and writing files without changing bytes the user did not edit.

pub mod encoding;
pub mod fortiweb;
pub mod load;
pub mod save;
pub mod session;

pub use encoding::{
    EncodingMismatch, OpenMode, TextEncoding, UnrepresentableChar, decode_bytes, dominant_eol,
    encode_text, reopen_as, reopen_with,
};
pub use load::{FileKind, LoadedFile, SourceIdentity, load_file};
pub use save::{Chunk, SaveError, atomic_write};
