#![allow(clippy::large_enum_variant)]

#[macro_use]
mod conjugation;
pub use self::conjugation::{Conjugation, Flag, Form};

pub mod adjective;

mod concat;
pub use self::concat::Concat;

pub mod elements;

mod entities;
pub use self::entities::PartOfSpeech;

mod furigana;
pub use self::furigana::Furigana;

mod kana;

pub mod verb;

mod parser;

mod priority;

pub mod database;

mod musli;

#[doc(hidden)]
pub mod macro_support {
    pub use fixed_map;
}
