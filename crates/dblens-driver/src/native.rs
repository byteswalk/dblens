//! 查找运行时动态加载的原生客户端库（Oracle Instant Client）。
//! 查找顺序：设置中指定的目录（指定后只查这一处）→ 专属环境变量 → 库搜索路径变量 → 常见安装位置。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use dblens_core::{Error, Msg, Result, msg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeLib {
    OracleClient,
    /// GBase 8s 的 JDBC 驱动。不是原生库，但同样由用户放置、在设置中确认目录。
    GbaseJdbc,
}

impl NativeLib {
    /// 当前平台上的库文件名。
    pub fn file_name(self) -> &'static str {
        match self {
            NativeLib::OracleClient => {
                if cfg!(windows) {
                    "oci.dll"
                } else if cfg!(target_os = "macos") {
                    "libclntsh.dylib"
                } else {
                    "libclntsh.so"
                }
            }
            NativeLib::GbaseJdbc => "gbasedbtjdbc.jar",
        }
    }

    fn display_name(self) -> Msg {
        match self {
            NativeLib::OracleClient => msg!("native.oracle_client"),
            NativeLib::GbaseJdbc => msg!("native.gbase_jdbc"),
        }
    }

    fn install_hint(self) -> Msg {
        match self {
            NativeLib::OracleClient => msg!("native.install.oracle_client"),
            NativeLib::GbaseJdbc => msg!("native.install.gbase_jdbc"),
        }
    }

    /// （环境变量，其下要查找的子目录）
    fn home_vars(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            NativeLib::OracleClient => &[("ORACLE_HOME", &["", "bin", "lib"])],
            NativeLib::GbaseJdbc => &[],
        }
    }

    /// 常见安装位置。以 `*` 结尾表示按前缀匹配同级的所有子目录。
    fn common_patterns(self) -> &'static [&'static str] {
        match self {
            NativeLib::OracleClient => {
                if cfg!(windows) {
                    &[
                        "C:\\oracle\\instantclient*",
                        "C:\\instantclient*",
                        "D:\\oracle\\instantclient*",
                        "E:\\oracle\\instantclient*",
                    ]
                } else if cfg!(target_os = "macos") {
                    &["/opt/oracle/instantclient*"]
                } else {
                    &[
                        "/opt/oracle/instantclient*",
                        "/usr/lib/oracle/23/client64/lib",
                        "/usr/lib/oracle/21/client64/lib",
                        "/usr/lib/oracle/19/client64/lib",
                    ]
                }
            }
            NativeLib::GbaseJdbc => &[],
        }
    }
}

fn search_path_var() -> &'static str {
    if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    }
}

/// 查找库所在的目录。
pub fn locate(lib: NativeLib, configured: Option<&Path>) -> Result<PathBuf> {
    let common = expand_common(lib.common_patterns());
    locate_with(lib, configured, &|k| std::env::var_os(k), &common)
}

/// 可注入环境变量和常见目录的查找，供测试使用。
pub fn locate_with(
    lib: NativeLib,
    configured: Option<&Path>,
    env: &dyn Fn(&str) -> Option<OsString>,
    common: &[PathBuf],
) -> Result<PathBuf> {
    // 相对路径会按当前工作目录解析，存在从工作目录加载库的风险，一律拒绝
    if let Some(dir) = configured.filter(|d| !d.is_absolute()) {
        return Err(Error::DriverUnavailable {
            hint: msg!("native.dir_not_absolute", lib = lib.display_name(), dir = dir.display()),
        });
    }
    let candidates: Vec<PathBuf> = match configured {
        Some(dir) => vec![dir.to_path_buf()],
        None => {
            let mut c = Vec::new();
            for (var, subdirs) in lib.home_vars() {
                if let Some(home) = env(var).filter(|h| Path::new(h).is_absolute()) {
                    // 空子目录表示 home 本身，不做 join，避免末尾多出分隔符
                    c.extend(subdirs.iter().map(|s| {
                        let base = PathBuf::from(&home);
                        if s.is_empty() { base } else { base.join(s) }
                    }));
                }
            }
            // 只接受绝对路径：空项和相对路径会按当前目录解析，存在库被从工作目录加载的风险
            if let Some(paths) = env(search_path_var()) {
                c.extend(std::env::split_paths(&paths).filter(|p| p.is_absolute()));
            }
            c.extend(common.iter().cloned());
            c
        }
    };
    candidates.iter().find(|d| d.join(lib.file_name()).is_file()).cloned().ok_or_else(|| {
        let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
        Error::DriverUnavailable {
            hint: msg!(
                "native.not_found",
                lib = lib.display_name(),
                file = lib.file_name(),
                install = lib.install_hint(),
                tried = tried.join("; ")
            ),
        }
    })
}

/// 展开常见安装位置。`前缀*` 匹配同级目录中名字以该前缀开头的子目录，按名称倒序（新版本在前）。
pub fn expand_common(patterns: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for pattern in patterns {
        let Some(prefix) = pattern.strip_suffix('*') else {
            out.push(PathBuf::from(pattern));
            continue;
        };
        let prefix = Path::new(prefix);
        let (Some(parent), Some(stem)) = (prefix.parent(), prefix.file_name()) else { continue };
        let stem = stem.to_string_lossy().into_owned();
        let Ok(entries) = std::fs::read_dir(parent) else { continue };
        // 版本键：目录名中的数字段按数值比较（23_26 > 23_9），非数字段忽略，溢出的段跳过
        let mut found: Vec<(Vec<u64>, PathBuf)> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&stem)))
            .map(|p| {
                let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let key: Vec<u64> =
                    name.split(|c: char| !c.is_ascii_digit()).filter_map(|d| d.parse().ok()).collect();
                (key, p)
            })
            .collect();
        // 降序：版本号大的在前，版本键相同时按路径名降序作为稳定的次序
        found.sort_by(|a, b| b.cmp(a));
        out.extend(found.into_iter().map(|(_, p)| p));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dblens_core::Lang;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dblens-native-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put_lib(dir: &Path, lib: NativeLib) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(lib.file_name()), b"").unwrap();
    }

    fn no_env(_: &str) -> Option<OsString> {
        None
    }

    #[test]
    fn configured_dir_is_the_only_place_searched() {
        let root = temp("configured");
        let good = root.join("good");
        put_lib(&good, NativeLib::OracleClient);
        assert_eq!(locate_with(NativeLib::OracleClient, Some(good.as_path()), &no_env, &[]).unwrap(), good);

        // 指定了目录但里面没有库：即使环境变量里能找到，也要报错
        let empty = root.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let env = |k: &str| (k == "ORACLE_HOME").then(|| good.clone().into_os_string());
        let err = locate_with(NativeLib::OracleClient, Some(empty.as_path()), &env, &[]).unwrap_err();
        assert!(matches!(err, Error::DriverUnavailable { .. }), "{err:?}");
    }

    #[test]
    fn relative_configured_dir_and_home_are_rejected() {
        let root = temp("relative");
        put_lib(&root, NativeLib::OracleClient);
        let err =
            locate_with(NativeLib::OracleClient, Some(Path::new("drivers/dpi")), &no_env, &[]).unwrap_err();
        assert!(
            matches!(&err, Error::DriverUnavailable { hint } if hint.render(Lang::ZhCn).contains("绝对路径")),
            "{err:?}"
        );
        // 相对路径的 ORACLE_HOME 被忽略，继续查找常见目录
        let env = |k: &str| (k == "ORACLE_HOME").then(|| OsString::from("dmdbms"));
        assert_eq!(
            locate_with(NativeLib::OracleClient, None, &env, std::slice::from_ref(&root)).unwrap(),
            root
        );
    }

    #[test]
    fn home_variable_subdirectories() {
        let home = temp("home");
        put_lib(&home.join("bin"), NativeLib::OracleClient);
        let env = |k: &str| (k == "ORACLE_HOME").then(|| home.clone().into_os_string());
        assert_eq!(locate_with(NativeLib::OracleClient, None, &env, &[]).unwrap(), home.join("bin"));
    }

    #[test]
    fn search_path_variable() {
        let root = temp("path");
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).unwrap();
        put_lib(&b, NativeLib::OracleClient);
        let joined = std::env::join_paths([&a, &b]).unwrap();
        let var = if cfg!(windows) {
            "PATH"
        } else if cfg!(target_os = "macos") {
            "DYLD_LIBRARY_PATH"
        } else {
            "LD_LIBRARY_PATH"
        };
        let env = move |k: &str| (k == var).then(|| joined.clone());
        assert_eq!(locate_with(NativeLib::OracleClient, None, &env, &[]).unwrap(), b);
    }

    #[test]
    fn search_path_ignores_empty_and_relative_entries() {
        let root = temp("path-relative");
        let good = root.join("good");
        put_lib(&good, NativeLib::OracleClient);

        // 只有空项和相对路径：都不能成为候选，因此候选列表为空
        let joined = std::env::join_paths([Path::new(""), Path::new("relative_dir")]).unwrap();
        let env = move |k: &str| (k == search_path_var()).then(|| joined.clone());
        let err = locate_with(NativeLib::OracleClient, None, &env, &[]).unwrap_err();
        let Error::DriverUnavailable { hint } = err else { panic!("应为 DriverUnavailable") };
        let hint = hint.render(Lang::ZhCn);
        assert!(hint.ends_with("已查找："), "{hint}");

        // 空项与绝对路径混合：绝对路径仍然有效
        let joined = std::env::join_paths([Path::new(""), good.as_path()]).unwrap();
        let env = move |k: &str| (k == search_path_var()).then(|| joined.clone());
        assert_eq!(locate_with(NativeLib::OracleClient, None, &env, &[]).unwrap(), good);
    }

    #[test]
    fn newer_versions_sort_numerically() {
        let root = temp("numeric");
        for name in ["instantclient_23_9", "instantclient_23_26", "instantclient_9_2", "instantclient_19_8"] {
            put_lib(&root.join(name), NativeLib::OracleClient);
        }
        let pattern = format!("{}*", root.join("instantclient_").display());
        let dirs = expand_common(&[pattern.as_str()]);
        assert_eq!(
            dirs,
            vec![
                root.join("instantclient_23_26"),
                root.join("instantclient_23_9"),
                root.join("instantclient_19_8"),
                root.join("instantclient_9_2"),
            ]
        );
    }

    #[test]
    fn common_dirs_and_prefix_expansion_prefers_newest() {
        let root = temp("common");
        put_lib(&root.join("instantclient_19_8"), NativeLib::OracleClient);
        put_lib(&root.join("instantclient_23_5"), NativeLib::OracleClient);
        let pattern = format!("{}*", root.join("instantclient_").display());
        let dirs = expand_common(&[pattern.as_str()]);
        assert_eq!(dirs, vec![root.join("instantclient_23_5"), root.join("instantclient_19_8")]);
        assert_eq!(
            locate_with(NativeLib::OracleClient, None, &no_env, &dirs).unwrap(),
            root.join("instantclient_23_5")
        );
    }

    #[test]
    fn not_found_hint_lists_tried_dirs() {
        let root = temp("missing");
        let err =
            locate_with(NativeLib::OracleClient, None, &no_env, std::slice::from_ref(&root)).unwrap_err();
        let Error::DriverUnavailable { hint } = err else { panic!("应为 DriverUnavailable") };
        let hint = hint.render(Lang::En);
        assert!(hint.contains(NativeLib::OracleClient.file_name()), "{hint}");
        assert!(hint.contains(&root.display().to_string()), "{hint}");
    }
}
