//! Argument types shared by the profile commands.

use std::process::ExitCode;
use std::str::FromStr;

use anyhow::Context;
use jobhunt_profile::{EmploymentKind, PartialDate, Stance};

/// A value that can also be cleared with `none` (or an empty string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clearable<T>(pub Option<T>);

impl<T: FromStr> FromStr for Clearable<T>
where
    T::Err: std::fmt::Display,
{
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("none") {
            return Ok(Self(None));
        }
        t.parse().map(|v| Self(Some(v))).map_err(|e| e.to_string())
    }
}

/// `full-time`, `contract`, `freelance`, ...
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Employment(pub EmploymentKind);

impl FromStr for Employment {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EmploymentKind::recognize(s)
            .or_else(|| {
                let canonical = s.trim().replace('-', "_").to_lowercase();
                matches!(
                    canonical.as_str(),
                    "full_time" | "part_time" | "contract" | "freelance" | "internship"
                )
                .then(|| EmploymentKind::from_canonical(&canonical))
            })
            .map(Self)
            .ok_or_else(|| {
                format!("{s:?} is not an employment type (full-time, part-time, contract, freelance, internship)")
            })
    }
}

/// How the user feels about a preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StanceArg {
    /// A hard requirement.
    #[value(alias = "required", alias = "must")]
    Require,
    #[value(alias = "wanted", alias = "prefer")]
    Want,
    #[value(alias = "acceptable", alias = "ok")]
    Accept,
    #[value(alias = "unwanted", alias = "no")]
    Avoid,
}

impl From<StanceArg> for Stance {
    fn from(value: StanceArg) -> Self {
        match value {
            StanceArg::Require => Stance::Required,
            StanceArg::Want => Stance::Wanted,
            StanceArg::Accept => Stance::Acceptable,
            StanceArg::Avoid => Stance::Unwanted,
        }
    }
}

pub fn date(s: &str) -> Result<Clearable<PartialDate>, String> {
    s.parse()
}

/// Ignores a closed pipe (`jobhunt profile | head`).
pub fn finish(result: std::io::Result<()>) -> anyhow::Result<ExitCode> {
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        other => {
            other.context("could not write the output")?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearable_values() {
        assert_eq!(
            "none".parse::<Clearable<String>>().unwrap(),
            Clearable(None)
        );
        assert_eq!(
            "Acme".parse::<Clearable<String>>().unwrap(),
            Clearable(Some("Acme".into()))
        );
        assert_eq!(
            date("2021-03").unwrap(),
            Clearable(Some("2021-03".parse().unwrap()))
        );
        assert!(date("March").is_err());
        assert_eq!(
            "Full-time".parse::<Employment>().unwrap(),
            Employment(EmploymentKind::FullTime)
        );
        assert!("sometimes".parse::<Employment>().is_err());
    }
}
