//! 执行入口的约束（审计版设计 3.1 节）：新增的代码绕不过策略和凭据来源。
//! 扫描 src 下非测试部分的源码。

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// 文件中 `#[cfg(test)]` 之前的部分；只用于测试的文件没有这部分。
fn production(path: &Path) -> String {
    if matches!(path.file_name().and_then(|n| n.to_str()), Some("fake.rs" | "path_tests.rs")) {
        return String::new();
    }
    let text = std::fs::read_to_string(path).unwrap();
    text.split("#[cfg(test)]\nmod tests").next().unwrap().to_string()
}

fn users_of(needle: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut found: Vec<String> = files
        .iter()
        .filter(|f| production(f).contains(needle))
        .map(|f| f.strip_prefix(&root).unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
        .collect();
    found.sort();
    found
}

#[test]
fn only_ask_calls_before_execute() {
    assert_eq!(users_of(".before_execute("), ["policy.rs"]);
}

#[test]
fn passwords_come_from_the_credential_source() {
    assert_eq!(users_of(".secrets.get("), Vec::<String>::new());
}

#[test]
fn connections_are_opened_in_known_places() {
    // 新增建连路径时要先确认它经过策略，再加进这张表
    assert_eq!(
        users_of(".registry.connect("),
        [
            "design.rs",
            "export/job.rs",
            "import/job.rs",
            "metadata.rs",
            "migrate/run.rs",
            "privilege.rs",
            "service.rs",
            "sqlfile.rs",
        ]
    );
}
