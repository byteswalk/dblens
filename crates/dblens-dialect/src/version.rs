//! 服务器版本号的解析与编码。

/// 把版本文本编码为 `major*10000 + minor*100 + patch`（minor、patch 超过 99 时按 99 计），
/// 便于模板里写 `{% if version >= 80016 %}`。识别不出时返回 None。
///
/// 先找第一个独立的 `数字.数字[.数字]`（如 `8.0.36`、`PostgreSQL 18.6`、`Release 23.26.3.0.0`）；
/// 找不到时再识别金仓、达梦的 `V008R006`、`V8` 写法。
pub fn parse_version(text: &str) -> Option<u32> {
    let b = text.as_bytes();
    let num = |i: usize| -> (u32, usize) {
        let mut j = i;
        let mut n: u32 = 0;
        while j < b.len() && b[j].is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add(u32::from(b[j] - b'0'));
            j += 1;
        }
        (n, j)
    };
    let digit_at = |i: usize| i < b.len() && b[i].is_ascii_digit();
    let encode =
        |major: u32, minor: u32, patch: u32| major.min(9999) * 10000 + minor.min(99) * 100 + patch.min(99);
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let (major, j) = num(i);
            if j < b.len() && b[j] == b'.' && digit_at(j + 1) {
                let (minor, k) = num(j + 1);
                let patch = if k < b.len() && b[k] == b'.' && digit_at(k + 1) { num(k + 1).0 } else { 0 };
                return Some(encode(major, minor, patch));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    for i in 0..b.len() {
        let starts = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if starts && b[i].eq_ignore_ascii_case(&b'V') && digit_at(i + 1) {
            let (major, j) = num(i + 1);
            let minor = if j < b.len() && b[j].eq_ignore_ascii_case(&b'R') && digit_at(j + 1) {
                num(j + 1).0
            } else {
                0
            };
            return Some(encode(major, minor, 0));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_banners() {
        let cases = [
            ("9.7.4", Some(90704)),
            ("8.0.16", Some(80016)),
            ("12.3.3-MariaDB", Some(120303)),
            ("5.7.25-OceanBase_CE-v4.2.1.2", Some(50725)),
            ("PostgreSQL 18.6 (Ubuntu 18.6-1.pgdg26.04+2) on x86_64-pc-linux-gnu", Some(180600)),
            (
                "Oracle AI Database 26ai Free Release 23.26.3.0.0 - Develop, Learn, and Run for Free\nVersion 23.26.3.0.0",
                Some(232603),
            ),
            ("Oracle Database 11g Enterprise Edition Release 11.2.0.4.0 - 64bit", Some(110200)),
            ("KingbaseES V008R006C008B0014 on x86_64-pc-linux-gnu", Some(80600)),
            ("DM Database Server 64 V8", Some(80000)),
            ("(openGauss 5.0.1 build 33b035fd) compiled at 2023-12-15", Some(50001)),
            ("8.1.3.62", Some(80103)),
            ("no digits here", None),
            ("", None),
        ];
        for (text, want) in cases {
            assert_eq!(parse_version(text), want, "{text}");
        }
    }

    #[test]
    fn caps_large_components_and_never_panics() {
        assert_eq!(parse_version("1.234.567"), Some(19999));
        assert_eq!(parse_version("99999999999999999999.1"), Some(99_990_100));
        for s in ["1.", ".1", "V", "v.", "V1R", "数洞 V9", "1..2", "a1.2"] {
            let _ = parse_version(s);
        }
        assert_eq!(parse_version("数洞 V9"), Some(90000));
        assert_eq!(parse_version("a1.2"), None);
    }
}
