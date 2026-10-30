//! M8b 才支持的功能：GBase 8s 在这些入口报错，前端同时置灰。

use dblens_core::{Error, Result, msg};
use dblens_dialect::{Dialect, Family};

pub(crate) fn ensure_full(dialect: &Dialect) -> Result<()> {
    if dialect.family() == Family::Informix {
        return Err(Error::invalid(msg!("gbase.later")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dblens_dialect::Variant;

    #[test]
    fn gbase_is_rejected_others_pass() {
        assert!(ensure_full(&Dialect::new(Variant::Gbase8s)).is_err());
        assert!(ensure_full(&Dialect::new(Variant::MySql)).is_ok());
    }
}
