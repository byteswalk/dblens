//! 达梦驱动：通过 DPI（运行时加载 dmdpi）访问 DM8。

#[allow(clippy::all)]
mod ffi;
// 本任务只有技术验证用到句柄封装，Task 4 接入驱动本体后去掉这两个 allow
#[allow(dead_code)]
mod handles;
#[allow(dead_code)]
mod load;

#[cfg(test)]
mod spike;
