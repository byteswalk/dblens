//! SQL 语句拆分：处理字符串、注释、PG 的 $$ 引号、MySQL 的 DELIMITER 命令，
//! 以及 Oracle/达梦的 PL/SQL 块和 `/` 结束符。保留每条语句在原文中的字节范围和起始行号，便于定位出错位置。
//!
//! SQL Server 不按 `;` 拆分，而是按单独一行的 `GO` 分批（与 SSMS、sqlcmd 一致），每批整段交给服务器：
//! `GO` 不区分大小写，前后可有空白，可带次数（`GO 5` 把该批重复 5 次，上限 `MAX_GO_COUNT`）和行尾 `--` 注释；
//! 字符串、`[…]`、`"…"` 标识符和（可嵌套的）块注释里的 `GO` 不算。
//!
//! 可以分块喂入（`StreamSplitter`，执行 SQL 文件用）：跨块保留结束符、PL/SQL 块、字符串和注释的状态，
//! 从上次停下的位置继续扫描。块末尾不足以判断的结构（被切开的结束符、未闭合的字符串或注释、
//! 未读到换行的行首命令、PL/SQL 块开头的关键字）等下一块再判断，文本结束时按结束处理；
//! 因此任意切块方式的结果都与整段拆分相同。`split_statements` 就是一次喂入全文。
//!
//! ## 已知限制
//! - Oracle/达梦的 PL/SQL 块必须有单独一行的 `/` 才会结束；没有的话，后面的语句会被并入（与 SQL*Plus 一致）。
//! - `CREATE TYPE ... AS OBJECT` 的类型定义与 `CREATE JAVA SOURCE` 不被当作块处理。
//! - 不支持 PG 的 `BEGIN ATOMIC`。
//! - MySQL 的存储过程等程序体需要先用 `DELIMITER` 改结束符。
//! - 单独一行、没有参数的 `DELIMITER` 不是有效命令，会当作 SQL 原样发给服务器。
//! - `DELIMITER` 只在语句开头识别，与 mysql 客户端一致。
//! - 不识别 SQL*Plus 命令（`SET`、`PROMPT`、`SPOOL`、`@` 等）。
//! - `/` 后面跟注释时不算结束符。
//! - `q''` 的定界符必须是 ASCII 字符。
//! - 达梦是否支持 `q''` 字符串尚未验证。
//! - 分块喂入时，跨越多块的单个字符串、注释或 `$$` 函数体，每来一块都从它的开头重新查找结尾。
//! - SQL Server 不识别 sqlcmd 的其他命令（`:setvar`、`:r` 等），按 SQL 原样发给服务器。

use crate::Family;

/// `GO n` 的次数上限，防止误写的大数把一批执行成千上万次。
pub const MAX_GO_COUNT: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatement {
    /// 语句文本：不含结束符，已去掉首尾空白。
    pub text: String,
    /// 在原文中的字节范围，满足 `&sql[start..end] == text`。
    pub start: usize,
    pub end: usize,
    /// 语句开头所在的行号，从 1 开始。
    pub line: usize,
}

/// 按家族的语法规则拆分多条语句。
pub fn split_statements(sql: &str, family: Family) -> Vec<SqlStatement> {
    split_with(StreamSplitter::new(family), sql)
}

/// 用给定的拆分器拆分整段文本（`Dialect::split` 用它带上兼容模式）。
pub(crate) fn split_with(mut s: StreamSplitter, sql: &str) -> Vec<SqlStatement> {
    let mut out = s.feed(sql);
    out.extend(s.finish());
    out
}

/// 分块拆分：`feed` 返回本块中已经完整的语句，`finish` 返回文本末尾剩下的。
/// 语句的位置和行号是在全部已喂入文本中的。
pub struct StreamSplitter {
    family: Family,
    /// PG 家族的 Oracle 兼容模式（人大金仓、openGauss）：另外识别 PL/SQL 块和独占一行的 `/`。
    oracle_blocks: bool,
    /// Informix：当前语句是这种过程（`PROCEDURE` 或 `FUNCTION`），要等到对应的 `END`。
    routine: Option<&'static str>,
    /// 尚未拆完的文本，从全文的 `base` 处开始：比当前语句的开头多留几个字节，供向前查看（标识符、行首）。
    buf: String,
    base: usize,
    /// 扫描位置；`buf` 内的位置，下同。
    i: usize,
    stmt_start: usize,
    /// 当前语句是否已出现注释以外的内容。
    has_code: bool,
    /// 是否处于 PL/SQL 块中（块内 `;` 不拆分）。
    block: bool,
    delimiter: Vec<u8>,
    /// 行号：`line_pos` 处位于第 `line` 行。
    line_pos: usize,
    line: usize,
    out: Vec<SqlStatement>,
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// 不算语句内容的字符：Unicode 空白与 BOM。
fn is_blank(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// 扫描的一步：处理了（`Done`）、不是这种结构（`No`）、块末尾的内容还不足以判断（`Wait`）。
enum Step {
    Done,
    No,
    Wait,
}

impl StreamSplitter {
    pub fn new(family: Family) -> Self {
        Self {
            family,
            oracle_blocks: false,
            routine: None,
            buf: String::new(),
            base: 0,
            i: 0,
            stmt_start: 0,
            has_code: false,
            block: false,
            delimiter: b";".to_vec(),
            line_pos: 0,
            line: 1,
            out: Vec::new(),
        }
    }

    /// 喂入一块文本，返回已经完整的语句。
    pub fn feed(&mut self, chunk: &str) -> Vec<SqlStatement> {
        self.buf.push_str(chunk);
        self.run(false);
        self.compact();
        std::mem::take(&mut self.out)
    }

    /// 文本结束：等待中的结构按结束处理，返回剩下的语句（包括没有结束符的最后一条）。
    pub fn finish(mut self) -> Vec<SqlStatement> {
        self.run(true);
        let end = self.buf.len();
        self.finish_stmt(end);
        self.out
    }

    /// PG 家族在 Oracle 兼容模式下的拆分器：PL/SQL 块（`CREATE PROCEDURE … END;` 后跟独占一行的 `/`）
    /// 与 `$$` 引号都识别。块中出现 `$$` 时按 PG 的写法处理，语句在 `;` 处结束。
    pub fn with_oracle_blocks(self) -> Self {
        Self { oracle_blocks: self.family == Family::Postgres, ..self }
    }

    /// Informix 的过程到 `END PROCEDURE` / `END FUNCTION` 才结束，过程体内的 `;` 不切分。
    fn in_informix_routine(&self) -> bool {
        let Some(kind) = self.routine else { return false };
        let mut tail = self.buf[self.stmt_start..self.i].split_whitespace().rev();
        !(tail.next().is_some_and(|w| w.eq_ignore_ascii_case(kind))
            && tail.next().is_some_and(|w| w.eq_ignore_ascii_case("END")))
    }

    /// 语句是否以 `CREATE [DBA] PROCEDURE|FUNCTION` 开头。
    fn informix_routine_at(&self) -> Option<&'static str> {
        if self.family != Family::Informix {
            return None;
        }
        let head = self.buf.get(self.i..)?;
        let mut w = head.split_whitespace();
        if !w.next()?.eq_ignore_ascii_case("CREATE") {
            return None;
        }
        let mut kind = w.next()?;
        if kind.eq_ignore_ascii_case("DBA") {
            kind = w.next()?;
        }
        ["PROCEDURE", "FUNCTION"].into_iter().find(|k| kind.eq_ignore_ascii_case(k))
    }

    fn plsql(&self) -> bool {
        matches!(self.family, Family::Oracle | Family::Dameng) || self.oracle_blocks
    }

    /// 丢掉当前语句之前的文本（留几个字节供向前查看），内存只与当前语句的长度有关。
    fn compact(&mut self) {
        let mut cut = self.stmt_start.min(self.i).saturating_sub(4);
        while !self.buf.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 {
            return;
        }
        if self.line_pos < cut {
            self.line += newlines(&self.buf.as_bytes()[self.line_pos..cut]);
            self.line_pos = cut;
        }
        self.buf.drain(..cut);
        self.base += cut;
        self.i -= cut;
        self.stmt_start -= cut;
        self.line_pos -= cut;
    }

    fn run(&mut self, eof: bool) {
        while self.i < self.buf.len() {
            if self.base + self.i == 0 || self.buf.as_bytes()[self.i - 1] == b'\n' {
                match self.line_command(eof) {
                    Step::Done => continue,
                    Step::Wait => return,
                    Step::No => {}
                }
            }
            match self.skip_comment(eof) {
                Step::Done => continue,
                Step::Wait => return,
                Step::No => {}
            }
            match self.skip_string(eof) {
                Step::Done => continue,
                Step::Wait => return,
                Step::No => {}
            }
            let rest = &self.buf.as_bytes()[self.i..];
            // SQL Server 只按 GO 分批
            if !self.block && self.family != Family::SqlServer && !self.in_informix_routine() {
                if rest.starts_with(&self.delimiter) {
                    let end = self.i;
                    self.i += self.delimiter.len();
                    self.finish_stmt(end);
                    continue;
                }
                // 结束符可能被块边界切开（如 `$$` 只读到第一个 `$`）
                if !eof && rest.len() < self.delimiter.len() && self.delimiter.starts_with(rest) {
                    return;
                }
            }
            let c = self.char_here();
            if !is_blank(c) {
                // 判断 PL/SQL 块要看开头的几个关键字
                let needs_words = self.plsql() || self.family == Family::Informix;
                if !self.has_code && !eof && needs_words && !words_complete(rest, 6) {
                    return;
                }
                self.mark_code();
            }
            self.i += c.len_utf8();
        }
    }

    /// `self.i` 处的字符；`self.i` 始终位于字符边界上。
    fn char_here(&self) -> char {
        self.buf[self.i..].chars().next().unwrap_or('\0')
    }

    /// 遇到第一段非注释内容时记下，并判断是否是 PL/SQL 块的开头。
    fn mark_code(&mut self) {
        if !self.has_code {
            self.has_code = true;
            self.block = self.plsql() && starts_plsql(&self.buf.as_bytes()[self.i..]);
            self.routine = self.informix_routine_at();
        }
    }

    /// 结束当前语句（范围到 `end` 为止），并从 `self.i` 开始新语句。
    fn finish_stmt(&mut self, end: usize) {
        if self.has_code {
            let seg = &self.buf[self.stmt_start..end];
            let rest = seg.trim_start_matches(is_blank);
            let text = rest.trim_end_matches(is_blank).to_string();
            if !text.is_empty() {
                let s = end - rest.len();
                let e = s + text.len();
                self.line += newlines(&self.buf.as_bytes()[self.line_pos..s]);
                self.line_pos = s;
                self.out.push(SqlStatement {
                    text,
                    start: self.base + s,
                    end: self.base + e,
                    line: self.line,
                });
            }
        }
        self.stmt_start = self.i;
        self.has_code = false;
        self.block = false;
    }

    /// 行首命令：MySQL 的 `DELIMITER xx`，Oracle/达梦单独一行的 `/`，SQL Server 单独一行的 `GO [n]`。
    /// 行还没读完时，能确定不是命令就返回 `No`，否则等下一块。
    fn line_command(&mut self, eof: bool) -> Step {
        let b = self.buf.as_bytes();
        let mut j = self.i;
        while j < b.len() && matches!(b[j], b' ' | b'\t') {
            j += 1;
        }
        let newline = b[j..].iter().position(|&c| c == b'\n').map(|p| j + p);
        let complete = newline.is_some() || eof;
        let line_end = newline.unwrap_or(b.len());
        let head = &b[j..line_end];
        let line = head.trim_ascii_end();
        let next_line = (line_end + 1).min(b.len());
        match self.family {
            // 只在语句开头识别；语句中间的 delimiter 是普通标识符（如 `SELECT a,\n  delimiter AS d`）
            Family::MySql if !self.has_code => {
                if !complete {
                    let n = head.len().min(9);
                    let maybe = head[..n].eq_ignore_ascii_case(&b"delimiter"[..n])
                        && head.get(9).is_none_or(u8::is_ascii_whitespace);
                    return if maybe { Step::Wait } else { Step::No };
                }
                if line.len() > 9
                    && line[..9].eq_ignore_ascii_case(b"delimiter")
                    && line[9].is_ascii_whitespace()
                {
                    let delimiter = line[9..].trim_ascii().to_vec();
                    self.finish_stmt(self.i);
                    self.delimiter = delimiter;
                    self.i = next_line;
                    self.stmt_start = next_line;
                    return Step::Done;
                }
                Step::No
            }
            Family::SqlServer => {
                if !complete {
                    let n = head.len().min(2);
                    let maybe = head[..n].eq_ignore_ascii_case(&b"go"[..n])
                        && head.get(2).is_none_or(|c| !is_ident_byte(*c));
                    return if maybe { Step::Wait } else { Step::No };
                }
                let Some(count) = go_count(line) else { return Step::No };
                let before = self.out.len();
                self.finish_stmt(self.i);
                // 这一批出现 count 次（count 为 0 时不执行）
                if self.out.len() > before
                    && let Some(batch) = self.out.pop()
                {
                    self.out.extend(std::iter::repeat_n(batch, count));
                }
                self.i = next_line;
                self.stmt_start = next_line;
                Step::Done
            }
            _ if self.plsql() => {
                if !complete {
                    let maybe = head.first().is_none_or(|&c| c == b'/')
                        && head.iter().skip(1).all(u8::is_ascii_whitespace);
                    return if maybe { Step::Wait } else { Step::No };
                }
                if line == b"/" {
                    self.finish_stmt(self.i);
                    self.i = next_line;
                    self.stmt_start = next_line;
                    return Step::Done;
                }
                Step::No
            }
            _ => Step::No,
        }
    }

    fn skip_comment(&mut self, eof: bool) -> Step {
        let b = self.buf.as_bytes();
        let i = self.i;
        let mysql = self.family == Family::MySql;
        // 注释到行尾（换行本身不算注释）；还没读到换行时等下一块
        let line_end = || match b[i..].iter().position(|&c| c == b'\n') {
            Some(p) => Some(i + p),
            None => eof.then_some(b.len()),
        };
        let (end, hint) = match (b[i], b.get(i + 1)) {
            (b'-' | b'/', None) => return if eof { Step::No } else { Step::Wait },
            (b'-', Some(b'-')) => {
                // MySQL 要求 `--` 后跟空白或行尾才是注释
                if mysql {
                    match b.get(i + 2) {
                        None if !eof => return Step::Wait,
                        None | Some(b' ' | b'\t' | b'\r' | b'\n') => {}
                        Some(_) => return Step::No,
                    }
                }
                (line_end(), false)
            }
            (b'#', _) if mysql => (line_end(), false),
            (b'{', _) if self.family == Family::Informix => match b[i..].iter().position(|&c| c == b'}') {
                Some(p) => (Some(i + p + 1), false),
                None => (eof.then_some(b.len()), false),
            },
            (b'/', Some(b'*')) => {
                // MySQL 的 /*! … */ 与 Oracle 的 /*+ … */ 提示属于语句内容
                let hint = match b.get(i + 2) {
                    None if !eof => return Step::Wait,
                    c => matches!(c, Some(b'!' | b'+')),
                };
                let nested = matches!(self.family, Family::Postgres | Family::SqlServer);
                let mut depth = 1;
                let mut j = i + 2;
                while j < b.len() && depth > 0 {
                    if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                    } else if nested && b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                ((depth == 0 || eof).then_some(j.min(b.len())), hint)
            }
            _ => return Step::No,
        };
        let Some(end) = end else { return Step::Wait };
        if hint {
            self.mark_code();
        }
        self.i = end;
        Step::Done
    }

    fn skip_string(&mut self, eof: bool) -> Step {
        let b = self.buf.as_bytes();
        let i = self.i;
        let c = b[i];
        let prev_is_ident = i > 0 && is_ident_byte(b[i - 1]);
        let oracle = self.family == Family::Oracle;
        // 判断 q-quote 要往后看两三个字节
        let need = |n: usize| !eof && b.len() < i + n;
        let end = if c == b'\'' {
            let pg_escape = self.family == Family::Postgres
                && i > 0
                && matches!(b[i - 1], b'E' | b'e')
                && (i < 2 || !is_ident_byte(b[i - 2]));
            scan_quoted(b, i + 1, b'\'', self.family == Family::MySql || pg_escape, eof)
        } else if c == b'"' {
            scan_quoted(b, i + 1, b'"', self.family == Family::MySql, eof)
        } else if c == b'`' && self.family == Family::MySql {
            scan_quoted(b, i + 1, b'`', false, eof)
        } else if c == b'[' && self.family == Family::SqlServer {
            // 方括号标识符，`]]` 表示一个 `]`
            scan_quoted(b, i + 1, b']', false, eof)
        } else if matches!(c, b'n' | b'N') && oracle && !prev_is_ident {
            // Oracle 的 nq'X…X' 字符串（national 字符集的 q-quote）
            if need(3) && b.get(i + 1).is_none_or(|c| matches!(c, b'q' | b'Q')) {
                return Step::Wait;
            }
            if matches!(b.get(i + 1), Some(b'q' | b'Q')) && b.get(i + 2) == Some(&b'\'') {
                scan_q_quote(b, i + 3, eof)
            } else {
                return Step::No;
            }
        } else if matches!(c, b'q' | b'Q') && oracle && !prev_is_ident {
            if need(2) {
                return Step::Wait;
            }
            if b.get(i + 1) == Some(&b'\'') {
                scan_q_quote(b, i + 2, eof)
            } else {
                return Step::No;
            }
        } else if c == b'$' && self.family == Family::Postgres && !prev_is_ident {
            match dollar_tag_len(b, i, eof) {
                Tag::Wait => return Step::Wait,
                Tag::No => return Step::No,
                Tag::Len(len) => {
                    // Oracle 兼容模式下 `CREATE FUNCTION … AS $$ … $$` 是 PG 的写法，不按 PL/SQL 块等 `/`
                    self.block = false;
                    scan_dollar(b, i, len, eof)
                }
            }
        } else {
            return Step::No;
        };
        match end {
            Some(e) => {
                self.mark_code();
                self.i = e;
                Step::Done
            }
            None => Step::Wait,
        }
    }
}

/// 一行是否是 `GO [n] [-- 注释]`：是则返回次数（没写为 1，超过上限按上限）。
fn go_count(line: &[u8]) -> Option<usize> {
    let line = line.trim_ascii();
    if line.len() < 2 || !line[..2].eq_ignore_ascii_case(b"go") {
        return None;
    }
    let mut rest = &line[2..];
    if rest.first().is_some_and(|c| !c.is_ascii_whitespace() && *c != b'-') {
        return None;
    }
    rest = rest.trim_ascii_start();
    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    let count = if digits == 0 {
        1
    } else {
        std::str::from_utf8(&rest[..digits]).ok()?.parse::<usize>().unwrap_or(MAX_GO_COUNT).min(MAX_GO_COUNT)
    };
    rest = rest[digits..].trim_ascii_start();
    (rest.is_empty() || rest.starts_with(b"--")).then_some(count)
}

fn newlines(b: &[u8]) -> usize {
    b.iter().filter(|&&c| c == b'\n').count()
}

/// 扫描引号字符串，返回结束引号之后的位置；没有结束引号时：文本已结束则返回末尾，否则 None（等下一块）。
fn scan_quoted(b: &[u8], mut j: usize, quote: u8, backslash: bool, eof: bool) -> Option<usize> {
    while j < b.len() {
        let c = b[j];
        if backslash && c == b'\\' {
            if j + 1 >= b.len() && !eof {
                return None;
            }
            j += 2;
            continue;
        }
        if c == quote {
            match b.get(j + 1) {
                // 末尾的引号：下一块可能以同一引号开头（两个引号表示一个）
                None if !eof => return None,
                Some(&n) if n == quote => {
                    j += 2;
                    continue;
                }
                _ => return Some(j + 1),
            }
        }
        j += 1;
    }
    eof.then_some(b.len())
}

/// Oracle 的 q'X…X' 字符串，`start` 指向定界符 X。
fn scan_q_quote(b: &[u8], start: usize, eof: bool) -> Option<usize> {
    let Some(&open) = b.get(start) else { return eof.then_some(b.len()) };
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        b'(' => b')',
        b'<' => b'>',
        c => c,
    };
    let mut j = start + 1;
    while j + 1 < b.len() {
        if b[j] == close && b[j + 1] == b'\'' {
            return Some(j + 2);
        }
        j += 1;
    }
    eof.then_some(b.len())
}

enum Tag {
    Len(usize),
    No,
    Wait,
}

/// PG 的 `$标签$` 或 `$$`：标签长度（含两端的 `$`）。
fn dollar_tag_len(b: &[u8], i: usize, eof: bool) -> Tag {
    let mut j = i + 1;
    match b.get(j) {
        None if !eof => return Tag::Wait,
        Some(b'$') => return Tag::Len(2),
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {}
        _ => return Tag::No,
    }
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
        j += 1;
    }
    match b.get(j) {
        Some(b'$') => Tag::Len(j - i + 1),
        None if !eof => Tag::Wait,
        _ => Tag::No,
    }
}

fn scan_dollar(b: &[u8], i: usize, tag_len: usize, eof: bool) -> Option<usize> {
    let tag = &b[i..i + tag_len];
    let body = i + tag_len;
    match b[body..].windows(tag_len).position(|w| w == tag) {
        Some(p) => Some(body + p + tag_len),
        None => eof.then_some(b.len()),
    }
}

/// 语句开头是否是 PL/SQL 块（Oracle/达梦）。
fn starts_plsql(s: &[u8]) -> bool {
    let (words, _) = leading_words(s, 6);
    let w = |k: usize| words.get(k).map(String::as_str).unwrap_or("");
    match w(0) {
        "DECLARE" | "BEGIN" => true,
        "CREATE" => {
            let mut k = 1;
            if w(1) == "OR" && w(2) == "REPLACE" {
                k = 3;
            }
            if matches!(w(k), "EDITIONABLE" | "NONEDITIONABLE" | "EDITIONING") {
                k += 1;
            }
            matches!(w(k), "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE" | "LIBRARY" | "CLASS")
        }
        _ => false,
    }
}

/// 开头的 n 个单词是否已经完整：取词停在文本末尾之前（末尾的单词可能还没读完）。
fn words_complete(s: &[u8], n: usize) -> bool {
    leading_words(s, n).1 < s.len()
}

/// 开头的至多 n 个单词（大写），以及取词停下的位置。
fn leading_words(s: &[u8], n: usize) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut j = 0;
    while out.len() < n {
        while j < s.len() && s[j].is_ascii_whitespace() {
            j += 1;
        }
        let start = j;
        while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'_') {
            j += 1;
        }
        if start == j {
            break;
        }
        out.push(String::from_utf8_lossy(&s[start..j]).to_ascii_uppercase());
    }
    (out, j)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拆分并校验偏移量与文本一致，返回各语句文本。
    fn texts(sql: &str, family: Family) -> Vec<String> {
        let out = split_statements(sql, family);
        for s in &out {
            assert_eq!(&sql[s.start..s.end], s.text, "偏移量与文本不一致：{s:?}");
        }
        out.into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn basic_and_trailing() {
        assert_eq!(texts("SELECT 1; SELECT 2", Family::MySql), ["SELECT 1", "SELECT 2"]);
        assert_eq!(texts("  SELECT 1 ;\n\n", Family::Postgres), ["SELECT 1"]);
        assert!(texts("", Family::MySql).is_empty());
        assert!(texts(";;; \n ;", Family::Oracle).is_empty());
        assert!(texts("-- 只有注释\n/* 还有 */", Family::Postgres).is_empty());
    }

    #[test]
    fn offsets_are_byte_positions() {
        let sql = "SELECT '数洞;'; SELECT 2";
        let out = split_statements(sql, Family::MySql);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].start, out[0].end), (0, 16));
        assert_eq!(out[1].text, "SELECT 2");
        assert_eq!(out[1].start, 18);
    }

    #[test]
    fn mysql_quotes_and_escapes() {
        assert_eq!(
            texts("SELECT 'a;b'; SELECT \"c;d\"; SELECT `e;f`", Family::MySql),
            ["SELECT 'a;b'", "SELECT \"c;d\"", "SELECT `e;f`"]
        );
        assert_eq!(
            texts("SELECT 'it''s; ok', 'a\\';b'; SELECT 2", Family::MySql),
            ["SELECT 'it''s; ok', 'a\\';b'", "SELECT 2"]
        );
    }

    #[test]
    fn mysql_comments() {
        assert_eq!(
            texts("-- x;\nSELECT 1; # y;\nSELECT 2 /* ; */;", Family::MySql),
            ["-- x;\nSELECT 1", "# y;\nSELECT 2 /* ; */"]
        );
        // MySQL 中 `--` 后不跟空白就不是注释
        assert_eq!(texts("SELECT 5--2; SELECT 1", Family::MySql), ["SELECT 5--2", "SELECT 1"]);
    }

    #[test]
    fn mysql_delimiter() {
        let sql =
            "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;";
        assert_eq!(
            texts(sql, Family::MySql),
            ["CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END", "SELECT 3"]
        );
        assert_eq!(texts("SELECT 0;\ndelimiter //\nSELECT 1//", Family::MySql), ["SELECT 0", "SELECT 1"]);
        // 语句中间的 delimiter 不是命令
        assert_eq!(
            texts("SELECT a,\n  delimiter AS d FROM t", Family::MySql),
            ["SELECT a,\n  delimiter AS d FROM t"]
        );
    }

    /// 人大金仓、openGauss 的 Oracle 兼容模式：PL/SQL 块以独占一行的 `/` 结束，`$$` 写法仍在 `;` 处结束。
    #[test]
    fn postgres_oracle_blocks() {
        let oracle = |sql: &str| -> Vec<String> {
            split_with(StreamSplitter::new(Family::Postgres).with_oracle_blocks(), sql)
                .into_iter()
                .map(|s| s.text)
                .collect()
        };
        let proc = "CREATE OR REPLACE PROCEDURE p AS BEGIN INSERT INTO t VALUES (1); END;";
        let sql = format!("{proc}\n/\nSELECT 1;\nSELECT 2");
        assert_eq!(oracle(&sql), [proc, "SELECT 1", "SELECT 2"]);
        assert!(texts(&sql, Family::Postgres).len() > 3, "PG 模式不认 PL/SQL 块");
        let func = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql";
        assert_eq!(oracle(&format!("{func};\nSELECT 2;")), [func, "SELECT 2"]);
        let block = "DECLARE n int; BEGIN n := 1; END;";
        assert_eq!(oracle(&format!("{block}\n/\n")), [block]);
        // 只在 PG 家族上生效
        let my = StreamSplitter::new(Family::MySql).with_oracle_blocks();
        assert!(!my.plsql());
    }

    #[test]
    fn postgres_dollar_quotes_and_strings() {
        let f = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1";
        assert_eq!(texts(f, Family::Postgres).len(), 2);
        assert_eq!(texts("DO $body$ BEGIN PERFORM 1; END $body$; SELECT 2", Family::Postgres).len(), 2);
        assert_eq!(texts("SELECT $1; SELECT 2", Family::Postgres), ["SELECT $1", "SELECT 2"]);
        assert_eq!(texts("SELECT E'a\\';b'; SELECT 2", Family::Postgres), ["SELECT E'a\\';b'", "SELECT 2"]);
        // 普通字符串里反斜杠不转义
        assert_eq!(texts("SELECT 'a\\'; SELECT 2", Family::Postgres), ["SELECT 'a\\'", "SELECT 2"]);
        assert_eq!(texts("SELECT \"a;b\" FROM t; SELECT 2", Family::Postgres).len(), 2);
    }

    #[test]
    fn postgres_nested_comments_and_dash_comments() {
        assert_eq!(
            texts("/* a /* b */ ; */ SELECT 1; SELECT 2", Family::Postgres),
            ["/* a /* b */ ; */ SELECT 1", "SELECT 2"]
        );
        // PG 中 `--` 总是注释，会吃掉行内的分号
        assert_eq!(texts("SELECT 5--2;\nSELECT 1", Family::Postgres), ["SELECT 5--2;\nSELECT 1"]);
    }

    #[test]
    fn oracle_plsql_blocks_and_slash() {
        let sql = "BEGIN\n  NULL;\n  NULL;\nEND;\n/\nSELECT 1 FROM DUAL;";
        assert_eq!(texts(sql, Family::Oracle), ["BEGIN\n  NULL;\n  NULL;\nEND;", "SELECT 1 FROM DUAL"]);
        let proc = "CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND;\n/\n";
        assert_eq!(texts(proc, Family::Oracle), ["CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND;"]);
        let ed = "create or replace editionable function f return number is begin return 1; end;\n/";
        assert_eq!(texts(ed, Family::Oracle).len(), 1);
        assert_eq!(
            texts("CREATE TABLE t (a INT);\nINSERT INTO t VALUES (1);", Family::Oracle),
            ["CREATE TABLE t (a INT)", "INSERT INTO t VALUES (1)"]
        );
        assert_eq!(
            texts("SELECT 1 FROM DUAL\n/\nSELECT 2 FROM DUAL\n  /  \n", Family::Oracle),
            ["SELECT 1 FROM DUAL", "SELECT 2 FROM DUAL"]
        );
        assert_eq!(texts("SELECT 4 / 2 FROM DUAL;", Family::Oracle), ["SELECT 4 / 2 FROM DUAL"]);
    }

    #[test]
    fn oracle_q_quote() {
        assert_eq!(
            texts("SELECT q'[a;b]' FROM DUAL; SELECT Q'#c;d#' FROM DUAL", Family::Oracle),
            ["SELECT q'[a;b]' FROM DUAL", "SELECT Q'#c;d#' FROM DUAL"]
        );
    }

    #[test]
    fn dameng_uses_plsql_rules() {
        let sql = "DECLARE\n  x INT;\nBEGIN\n  x := 1;\nEND;\n/\nSELECT 1;";
        assert_eq!(texts(sql, Family::Dameng), ["DECLARE\n  x INT;\nBEGIN\n  x := 1;\nEND;", "SELECT 1"]);
    }

    #[test]
    fn unterminated_string_runs_to_end() {
        assert_eq!(texts("SELECT 'abc; SELECT 2", Family::MySql), ["SELECT 'abc; SELECT 2"]);
    }

    #[test]
    fn unicode_whitespace_and_bom_are_not_code() {
        assert!(texts("\u{3000};", Family::MySql).is_empty());
        assert!(texts("\u{feff}", Family::Postgres).is_empty());
        assert_eq!(
            texts("\u{feff}SELECT 1;\u{3000}SELECT 2\u{3000}", Family::MySql),
            ["SELECT 1", "SELECT 2"]
        );
    }

    #[test]
    fn sqlserver_batches_split_on_go_lines() {
        let sql = "CREATE TABLE t (id int);\nINSERT INTO t VALUES (1); SELECT 1\n  go  \n\
                   CREATE PROCEDURE p AS\nBEGIN\n  SELECT 'GO';\n  /* GO\n /* GO */ GO */\nEND\nGo -- 结束\n\
                   SELECT [go\nGO], \"x\nGO\" FROM t\nGO";
        let s = split_statements(sql, Family::SqlServer);
        let t: Vec<&str> = s.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            t,
            [
                "CREATE TABLE t (id int);\nINSERT INTO t VALUES (1); SELECT 1",
                "CREATE PROCEDURE p AS\nBEGIN\n  SELECT 'GO';\n  /* GO\n /* GO */ GO */\nEND",
                "SELECT [go\nGO], \"x\nGO\" FROM t",
            ]
        );
        assert_eq!((s[0].line, s[1].line, s[2].line), (1, 4, 11));
        for st in &s {
            assert_eq!(&sql[st.start..st.end], st.text);
        }
    }

    #[test]
    fn sqlserver_go_count_and_lookalikes() {
        assert_eq!(
            texts("INSERT INTO t DEFAULT VALUES\nGO 3\nSELECT 1", Family::SqlServer),
            [
                "INSERT INTO t DEFAULT VALUES",
                "INSERT INTO t DEFAULT VALUES",
                "INSERT INTO t DEFAULT VALUES",
                "SELECT 1"
            ]
        );
        assert_eq!(texts("SELECT 1\nGO 0\nSELECT 2", Family::SqlServer), ["SELECT 2"]);
        assert_eq!(texts("SELECT 1\nGO 99999999999999999999999", Family::SqlServer).len(), MAX_GO_COUNT);
        // 不是 GO 命令：GOTO、GO 后跟别的、行中间的 GO
        assert_eq!(texts("GOTO x\nSELECT 1 GO\nGO x\nGO;", Family::SqlServer).len(), 1);
        assert_eq!(texts("SELECT 1;\nSELECT 2;", Family::SqlServer), ["SELECT 1;\nSELECT 2;"]);
        assert_eq!(go_count(b"go--x"), Some(1));
        assert_eq!(go_count(b"  GO 2 -- x "), Some(2));
        assert_eq!(go_count(b"GOx"), None);
    }

    #[test]
    fn mysql_bare_delimiter_is_plain_statement() {
        assert_eq!(texts("DELIMITER\nSELECT 1;", Family::MySql), ["DELIMITER\nSELECT 1"]);
    }

    #[test]
    fn oracle_nq_quote() {
        assert_eq!(texts("SELECT nq'[it's;x]' FROM dual; SELECT 2 FROM dual", Family::Oracle).len(), 2);
    }

    #[test]
    fn every_char_prefix_splits_without_panic() {
        let sql = "-- 注释;\nSELECT '数据;\\' FROM t; /* 块 */ # 井\nDELIMITER //\n\
                   SELECT $$体;$$, $a$标;$a$, q'[中;]', nq'<x>' FROM DUAL//\nDELIMITER ;\n\
                   SELECT \"双;\", `反;` ;\nBEGIN NULL; END;\n/\n\u{3000}SELECT 1 -- 行尾\u{feff}";
        let families = [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng, Family::SqlServer];
        let ends = sql.char_indices().map(|(i, _)| i).chain(std::iter::once(sql.len()));
        for end in ends {
            let prefix = &sql[..end];
            for family in families {
                for s in split_statements(prefix, family) {
                    assert!(!s.text.is_empty(), "空语句：{prefix:?}");
                    assert_eq!(&prefix[s.start..s.end], s.text, "偏移量与文本不一致：{prefix:?}");
                }
            }
        }
    }

    /// 按伪随机长度（1..=max 字节，切在字符边界上）把文本切块喂入。
    fn split_in_chunks(sql: &str, family: Family, seed: u64, max: usize) -> Vec<SqlStatement> {
        let mut s = StreamSplitter::new(family);
        let mut out = Vec::new();
        let mut rng = seed;
        let mut pos = 0;
        while pos < sql.len() {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let mut end = (pos + 1 + (rng >> 33) as usize % max).min(sql.len());
            while !sql.is_char_boundary(end) {
                end += 1;
            }
            out.extend(s.feed(&sql[pos..end]));
            pos = end;
        }
        out.extend(s.finish());
        out
    }

    const FAMILIES: [Family; 5] =
        [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng, Family::SqlServer];

    /// 上面各测试用过的文本，加上结构可能恰好被切开的例子。
    const SAMPLES: &[&str] = &[
        "SELECT 1; SELECT 2",
        "  SELECT 1 ;\n\n",
        ";;; \n ;",
        "-- 只有注释\n/* 还有 */",
        "SELECT '数洞;'; SELECT 2",
        "SELECT 'a;b'; SELECT \"c;d\"; SELECT `e;f`",
        "SELECT 'it''s; ok', 'a\\';b'; SELECT 2",
        "-- x;\nSELECT 1; # y;\nSELECT 2 /* ; */;",
        "SELECT 5--2; SELECT 1",
        "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;",
        "SELECT 0;\ndelimiter //\nSELECT 1//",
        "SELECT a,\n  delimiter AS d FROM t",
        "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1",
        "DO $body$ BEGIN PERFORM 1; END $body$; SELECT 2",
        "SELECT $1; SELECT 2",
        "SELECT E'a\\';b'; SELECT 2",
        "SELECT 'a\\'; SELECT 2",
        "/* a /* b */ ; */ SELECT 1; SELECT 2",
        "SELECT 5--2;\nSELECT 1",
        "BEGIN\n  NULL;\n  NULL;\nEND;\n/\nSELECT 1 FROM DUAL;",
        "CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND;\n/\n",
        "create or replace editionable function f return number is begin return 1; end;\n/",
        "SELECT 1 FROM DUAL\n/\nSELECT 2 FROM DUAL\n  /  \n",
        "SELECT 4 / 2 FROM DUAL;",
        "SELECT q'[a;b]' FROM DUAL; SELECT Q'#c;d#' FROM DUAL",
        "SELECT nq'[it's;x]' FROM dual; SELECT 2 FROM dual",
        "DECLARE\n  x INT;\nBEGIN\n  x := 1;\nEND;\n/\nSELECT 1;",
        "SELECT 'abc; SELECT 2",
        "\u{feff}SELECT 1;\u{3000}SELECT 2\u{3000}",
        "DELIMITER\nSELECT 1;",
        "-- 注释;\nSELECT '数据;\\' FROM t; /* 块 */ # 井\nDELIMITER //\n\
         SELECT $$体;$$, $a$标;$a$, q'[中;]', nq'<x>' FROM DUAL//\nDELIMITER ;\n\
         SELECT \"双;\", `反;` ;\nBEGIN NULL; END;\n/\n\u{3000}SELECT 1 -- 行尾\u{feff}",
        "SELECT 1;\r\nSELECT 2;\r\n/\r\nCREATE OR REPLACE\r\n  PACKAGE pk AS x INT; END;\r\n/\r\n",
        "INSERT INTO t VALUES ('a''b', E'\\\\', $q$x$q$, 'c'); -- 末尾注释",
        "SELECT 1 /*+ hint */; SELECT /*! 2 */; #",
        "SELECT 1\nGO\nSELECT [a]]\nGO], N'b\nGO' /* x /* GO */\nGO */\n  go 2 -- c\nSELECT 3\ngo",
    ];

    #[test]
    fn chunked_feeding_matches_whole_text() {
        for sql in SAMPLES {
            for family in FAMILIES {
                let whole = split_statements(sql, family);
                for max in [1, 2, 3, 5, 8, 64] {
                    for seed in 0..10 {
                        assert_eq!(
                            split_in_chunks(sql, family, seed, max),
                            whole,
                            "{family:?} max={max} seed={seed}：{sql:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_two_chunk_split_matches_whole_text() {
        for sql in SAMPLES {
            for family in FAMILIES {
                let whole = split_statements(sql, family);
                for (k, _) in sql.char_indices().skip(1) {
                    let mut s = StreamSplitter::new(family);
                    let mut out = s.feed(&sql[..k]);
                    out.extend(s.feed(&sql[k..]));
                    out.extend(s.finish());
                    assert_eq!(out, whole, "{family:?} 切在 {k}：{sql:?}");
                }
            }
        }
    }

    #[test]
    fn line_numbers_and_offsets_across_chunks() {
        let sql = "SELECT 1;\nSELECT 2;\n\n  -- c\n  SELECT\n3;\nSELECT '多\n行'; SELECT 4";
        let lines: Vec<usize> = split_statements(sql, Family::Postgres).iter().map(|s| s.line).collect();
        assert_eq!(lines, [1, 2, 4, 7, 8]);
        // 长文本分块喂入：位置和行号是全文中的
        let big: String = (0..2000).map(|n| format!("INSERT INTO t VALUES ({n}, '行{n}');\n")).collect();
        let out = split_in_chunks(&big, Family::MySql, 7, 4096);
        assert_eq!(out.len(), 2000);
        for (n, s) in out.iter().enumerate() {
            assert_eq!(&big[s.start..s.end], s.text);
            assert_eq!(s.line, n + 1);
        }
    }

    #[test]
    fn statements_come_out_as_soon_as_complete() {
        let mut s = StreamSplitter::new(Family::MySql);
        assert!(s.feed("SELECT 1").is_empty());
        let done = s.feed("; SELECT 2;\nSELECT");
        assert_eq!(done.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["SELECT 1", "SELECT 2"]);
        assert!(s.feed(" 3").is_empty());
        assert_eq!(s.finish()[0].text, "SELECT 3");
    }

    #[test]
    fn informix_procedures_and_brace_comments() {
        let sql = "CREATE PROCEDURE p()\n  DEFINE i INT;\n  LET i = 1;\nEND PROCEDURE;\n{ 注释; 不算 }\nSELECT 1 FROM t;";
        let got = texts(sql, Family::Informix);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].ends_with("END PROCEDURE"), "{got:?}");
        assert!(got[1].ends_with("SELECT 1 FROM t"), "{got:?}");
    }

    #[test]
    fn informix_function_block_ends_at_end_function() {
        let sql = "CREATE FUNCTION f() RETURNING INT;\n  RETURN 1;\nEND FUNCTION; SELECT 2;";
        assert_eq!(texts(sql, Family::Informix).len(), 2);
    }
}
