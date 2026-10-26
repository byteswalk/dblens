// 词条在编译时嵌入（rust-i18n）：词条文件变化时要重新编译本 crate。
fn main() {
    println!("cargo:rerun-if-changed=locales");
}
