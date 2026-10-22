//! 驱动接口：所有数据库驱动都实现这里的 `Driver` / `Connection` / `ExecStream`。

pub mod blocking;
mod cancel;
mod exec;
pub mod native;
mod params;
mod traits;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use cancel::CancelHandle;
pub use exec::{ColumnMeta, ExecEvent, ExecStream, collect_all};
pub use params::{Capabilities, ConnectParams, DriverId, ServerInfo};
pub use traits::{Connection, Driver};
