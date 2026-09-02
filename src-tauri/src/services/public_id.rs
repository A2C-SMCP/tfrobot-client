//! Public identifier validation at the tfrobot-client trust boundary.
//!
//! Manager owns the full public-id grammar. The client deliberately validates only the stable
//! account-identity shape needed to distinguish `{org}:{employee}` from retired numeric IDs.

/// Validate the stable structural shape of an Account public ID.
///
/// Character-level rules remain Manager-owned so future employee-number formats do not require a
/// coordinated desktop release. This check only rejects empty, untrimmed, and non-composite IDs.
pub fn validate_account_public_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty() || value.trim() != value {
        return Err("must be non-empty and trimmed");
    }

    let mut parts = value.split(':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(organization), Some(employee), None)
            if !organization.is_empty() && !employee.is_empty() =>
        {
            Ok(())
        }
        _ => Err("must have the `{organization}:{employee}` shape"),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_account_public_id;

    #[test]
    fn accepts_composite_public_ids_without_assuming_employee_characters() {
        for value in ["turingfocus:000042", "org-with-dash:EMP010-R"] {
            assert_eq!(validate_account_public_id(value), Ok(()), "{value}");
        }
    }

    #[test]
    fn rejects_retired_or_ambiguous_shapes() {
        for value in [
            "",
            "42",
            "robot:turingfocus:000042",
            ":000042",
            "org:",
            " org:1",
        ] {
            assert!(validate_account_public_id(value).is_err(), "{value}");
        }
    }
}
