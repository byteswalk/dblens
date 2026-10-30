//! 拼音启发式。只在能明显判定时返回 true，宁可漏报也别误伤英文。

/// 合法的汉语拼音音节（不含声调），按长度从长到短排列便于贪心切分。
const SYLLABLES: &[&str] = &[
    "zhuang", "chuang", "shuang", "jiang", "qiang", "xiang", "niang", "liang", "guang", "kuang", "huang",
    "zhang", "chang", "shang", "zhong", "chong", "sheng", "zheng", "cheng", "zhuan", "chuan", "shuan",
    "jiong", "qiong", "xiong", "bang", "pang", "mang", "fang", "dang", "tang", "nang", "lang", "gang",
    "kang", "hang", "zang", "cang", "sang", "yang", "wang", "beng", "peng", "meng", "feng", "deng", "teng",
    "neng", "leng", "geng", "keng", "heng", "zeng", "ceng", "seng", "reng", "weng", "ying", "bing", "ping",
    "ming", "ding", "ting", "ning", "ling", "jing", "qing", "xing", "bian", "pian", "mian", "dian", "tian",
    "nian", "lian", "jian", "qian", "xian", "zhai", "chai", "shuai", "zhao", "chao", "shao", "zhou", "chou",
    "shou", "zhan", "chan", "shan", "zhen", "chen", "shen", "zhei", "guan", "kuan", "huan", "zuan", "cuan",
    "suan", "duan", "tuan", "nuan", "luan", "ruan", "juan", "quan", "xuan", "yuan", "zhua", "chua", "shua",
    "zhuo", "chuo", "shuo", "zhui", "chui", "shui", "zhun", "chun", "shun", "gong", "kong", "hong", "dong",
    "tong", "nong", "long", "zong", "cong", "song", "rong", "yong", "huai", "guai", "kuai", "shai", "lang",
    " gai", "ang", "eng", "bai", "pai", "mai", "dai", "tai", "nai", "lai", "gai", "kai", "hai", "zai", "cai",
    "sai", "bao", "pao", "mao", "dao", "tao", "nao", "lao", "gao", "kao", "hao", "zao", "cao", "sao", "rao",
    "yao", "ban", "pan", "man", "fan", "dan", "tan", "nan", "lan", "gan", "kan", "han", "zan", "can", "san",
    "ran", "ben", "pen", "men", "fen", "den", "nen", "gen", "ken", "hen", "zen", "cen", "sen", "ren", "bei",
    "pei", "mei", "fei", "dei", "nei", "lei", "gei", "hei", "zei", "wei", "wai", "wan", "wen", "yan", "yin",
    "yun", "yue", "jun", "qun", "xun", "lun", "gun", "kun", "hun", "zun", "cun", "sun", "dun", "tun", "nun",
    "run", "jue", "que", "xue", "nue", "lue", "bie", "pie", "mie", "die", "tie", "nie", "lie", "jie", "qie",
    "xie", "you", "diu", "niu", "liu", "jiu", "qiu", "xiu", "miu", "bin", "pin", "min", "nin", "lin", "jin",
    "qin", "xin", "bai", "cou", "dou", "fou", "gou", "hou", "kou", "lou", "mou", "nou", "pou", "rou", "sou",
    "tou", "zou", "gua", "kua", "hua", "zhu", "chi", "shi", "zhi", "chu", "shu", "zha", "cha", "sha", "zhe",
    "che", "she", "rua", "ren", "ban", "ba", "po", "mo", "fo", "me", "de", "te", "ne", "le", "ge", "ke",
    "he", "ze", "ce", "se", "re", "zi", "ci", "si", "ri", "bi", "pi", "mi", "di", "ti", "ni", "li", "ji",
    "qi", "xi", "bu", "pu", "mu", "fu", "du", "tu", "nu", "lu", "gu", "ku", "hu", "zu", "cu", "su", "ru",
    "ju", "qu", "xu", "yu", "wu", "yi", "da", "ta", "na", "la", "ga", "ka", "ha", "za", "ca", "sa", "ya",
    "wa", "pa", "ma", "fa", "bo", "lo", "ao", "ou", "ai", "ei", "an", "en", "er", "ah", "a", "e", "o",
];

/// 两字母的弱音节（ti、me、na、da…）在英文里太常见（time=ti+me、name=na+me、date=da+te），
/// 必须至少有一个"强"音节（三字母以上，或带 zh/ch/sh/ng/ao/ou 这类拼音特征）才算拼音。
fn is_strong(syl: &str) -> bool {
    syl.len() >= 3 || ["zh", "ch", "sh", "ng", "ao", "ou", "ui", "ue"].iter().any(|m| syl.contains(m))
}

/// 把一个连续的字母串切成拼音音节；切不干净返回 false。
fn is_pinyin_run(run: &str) -> bool {
    if run.len() < 2 {
        return false;
    }
    let bytes = run.as_bytes();
    let mut pos = 0;
    let mut syllables = 0;
    let mut strong = 0;
    while pos < bytes.len() {
        let rest = &run[pos..];
        let Some(syl) = SYLLABLES.iter().filter(|s| !s.starts_with(' ')).find(|s| rest.starts_with(**s))
        else {
            return false;
        };
        pos += syl.len();
        syllables += 1;
        if is_strong(syl) {
            strong += 1;
        }
    }
    // 两个及以上音节，且至少一个强音节，才判为拼音
    syllables >= 2 && strong >= 1
}

/// 名字像汉语拼音时返回 true。按下划线和数字切成小段，任一段是多音节拼音就算。
pub fn looks_like_pinyin(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.split(|c: char| c == '_' || c.is_ascii_digit()).any(is_pinyin_run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_obvious_pinyin() {
        for name in ["xingming", "shuliang", "dingdan_bianhao", "gongsi_mingcheng", "yonghu_shouji"] {
            assert!(looks_like_pinyin(name), "{name}");
        }
    }

    #[test]
    fn leaves_english_alone() {
        for name in ["user_name", "order_id", "amount", "create_time", "customer", "product_code", "status"] {
            assert!(!looks_like_pinyin(name), "{name}");
        }
    }
}
