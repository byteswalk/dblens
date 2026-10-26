//! 日志（设计文档 4.3 节）：按天滚动，写在数据目录下的 `logs/`，保留最近 14 天。
//! 调试构建同时输出到控制台。日志中不得出现密码和访问令牌。

use std::path::Path;

use dblens_core::msg;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub const KEEP_DAYS: usize = 14;

/// 按天滚动的日志文件：`dblens.2026-10-25.log`。
pub fn appender(dir: &Path) -> Result<RollingFileAppender, String> {
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("dblens")
        .filename_suffix("log")
        .max_log_files(KEEP_DAYS)
        .build(dir)
        .map_err(|e| msg!("desktop.log_dir_failed", dir = dir.display(), error = e).current())
}

/// 初始化全局日志。返回的 guard 要一直持有到程序退出，否则缓冲中的日志会丢失。
pub fn init(dir: &Path) -> Result<WorkerGuard, String> {
    let (writer, guard) = tracing_appender::non_blocking(appender(dir)?);
    let file = fmt::layer().with_writer(writer).with_ansi(false);
    let console = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .with(file)
        .with(console)
        .try_init()
        .map_err(|e| e.to_string())?;
    // panic 也写进日志：发布版没有控制台，不写就看不到
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("程序异常：{info}");
        default_hook(info);
    }));
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn writes_daily_file_in_given_dir() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        let mut a = appender(&logs).unwrap();
        writeln!(a, "hello").unwrap();
        a.flush().unwrap();
        let names: Vec<String> = std::fs::read_dir(&logs)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        let name = &names[0];
        // dblens.YYYY-MM-DD.log
        assert!(
            name.starts_with("dblens.")
                && name.ends_with(".log")
                && name.len() == "dblens.2026-10-25.log".len()
        );
        assert_eq!(std::fs::read_to_string(logs.join(name)).unwrap(), "hello\n");
    }
}
