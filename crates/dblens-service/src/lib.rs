//! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。

mod profile;
mod store;

pub use profile::{Environment, Profile, ProfileInput};
pub use store::{Store, default_data_dir};
