/// 从语法错误信息找出出错位置：`near` 后面是从出错处开始的原文（可能被截断），在所报的行里找它的第一行；
/// 为空表示语句意外结束，位置在末尾。返回字节偏移。认得两种写法：
/// - MySQL、MariaDB、OceanBase：`… near 'FROM t' at line 2`（OceanBase 在后面另起一行附上节点和追踪号）；
/// - TiDB：`… line 2 column 21 near "FROM t" `。
pub fn mysql_error_offset(sql: &str, message: &str) -> Option<usize> {
    let (line, near) = mysql_style(message).or_else(|| tidb_style(message))?;
    let line_start = sql.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum::<usize>();
    let snippet = near.split('\n').next().unwrap_or_default();
    if snippet.is_empty() {
        return Some(sql.trim_end().len());
    }
    sql.get(line_start..)?.find(snippet).map(|at| line_start + at)
}

/// `near '…' at line N`：返回行号和 near 后的原文。
fn mysql_style(message: &str) -> Option<(usize, &str)> {
    let (head, tail) = message.rsplit_once("' at line ")?;
    let digits = tail.find(|c: char| !c.is_ascii_digit()).unwrap_or(tail.len());
    let line = tail[..digits].parse().ok()?;
    Some((line, &head[head.find(" near '")? + 7..]))
}

/// TiDB 的 `line N column M near "…" `：near 后的原文到最后一个双引号为止。
fn tidb_style(message: &str) -> Option<(usize, &str)> {
    let at = message.find(" line ")?;
    let rest = &message[at + 6..];
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    let line = rest[..digits].parse().ok()?;
    let near = &rest[rest.find(" near \"")? + 7..];
    Some((line, &near[..near.rfind('"')?]))
}
