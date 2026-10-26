//! DBLens 公共类型：错误、值、列类型、结构模型、面向用户的文字。本 crate 不做任何 IO。

mod error;
mod lang;
mod msg;
pub mod schema;
mod types;
mod value;

rust_i18n::i18n!("locales", fallback = "zh-CN");

pub use error::{Error, Result, char_position};
pub use lang::{Lang, current_lang, default_lang, scope_lang, set_default_lang, with_lang};
pub use msg::{Msg, ToMsg};
pub use types::{ColumnType, LogicalType};
pub use value::{DEFAULT_LOB_PREVIEW_BYTES, LobPreview, Row, Value};
