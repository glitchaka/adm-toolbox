use anyhow::Result;

pub struct AlternateScreenGuard;

impl AlternateScreenGuard {
    pub fn enter() -> Result<Self> {
        super::io::enter()?;
        Ok(Self)
    }
}

impl Drop for AlternateScreenGuard {
    fn drop(&mut self) {
        super::io::leave();
    }
}


pub struct RawModeGuard;

impl RawModeGuard {
    pub fn enter() -> Result<Self> {
        super::io::enter_raw()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        super::io::leave_raw();
    }
}
