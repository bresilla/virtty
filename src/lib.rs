//! virtty.

pub mod incus;
pub mod journal;
pub mod model;
pub mod openfga;
pub mod remotes;
pub mod schedules;
pub mod tui;
pub mod views;

/// Returns this crate's display name.
pub fn name() -> &'static str {
    "virtty"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_name() {
        assert_eq!(name(), "virtty");
    }
}
