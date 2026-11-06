//! M10b 才支持的功能：SQLite 在这些入口报错，前端同时置灰。

use dblens_core::{Error, Result, msg};
use dblens_dialect::{Dialect, Family};

pub(crate) fn ensure_full(dialect: &Dialect) -> Result<()> {
    if dialect.family() == Family::Sqlite {
        return Err(Error::invalid(msg!("sqlite.later")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dblens_dialect::Variant;

    #[test]
    fn sqlite_is_rejected_others_pass() {
        assert!(ensure_full(&Dialect::new(Variant::Sqlite)).is_err());
        assert!(ensure_full(&Dialect::new(Variant::Gbase8s)).is_ok());
    }
}
