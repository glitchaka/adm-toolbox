#[cfg(windows)]
pub mod domain_probe;
#[cfg(windows)]
pub mod foreground;
#[cfg(windows)]
pub mod traffic_etw;

#[cfg(windows)]
pub use domain_probe::WindowsDomainProbe;
