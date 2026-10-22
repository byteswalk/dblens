//! DBLens 公共类型：错误、值、列类型、结构模型。本 crate 不做任何 IO。

mod error;
pub mod schema;
mod types;
mod value;

pub use error::{Error, Result};
pub use types::{ColumnType, LogicalType};
pub use value::{DEFAULT_LOB_PREVIEW_BYTES, LobPreview, Row, Value};
