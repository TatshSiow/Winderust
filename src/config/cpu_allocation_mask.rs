use serde::{Deserialize, Deserializer, Serializer};

// TOML integers are signed 64-bit; strings preserve every supported processor bit.
pub(super) fn serialize<S: Serializer>(mask: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("0x{mask:016x}"))
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = String::deserialize(deserializer)?;
    let digits = value
        .strip_prefix("0x")
        .filter(|digits| digits.len() == 16 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            <D::Error as serde::de::Error>::custom(
                "CPU Allocation masks require 0x followed by 16 hex digits",
            )
        })?;
    u64::from_str_radix(digits, 16).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Selection {
        #[serde(with = "super")]
        mask: u64,
    }

    #[test]
    fn full_width_masks_round_trip_as_canonical_hex_strings() {
        for (mask, encoded) in [
            (0, "0x0000000000000000"),
            (1, "0x0000000000000001"),
            (1 << 63, "0x8000000000000000"),
            (u64::MAX, "0xffffffffffffffff"),
        ] {
            let selection = Selection { mask };
            let raw = toml::to_string(&selection).unwrap();
            assert_eq!(raw, format!("mask = \"{encoded}\"\n"));
            assert_eq!(toml::from_str::<Selection>(&raw).unwrap(), selection);
        }
        let upper: Selection = toml::from_str("mask = '0xFFFFFFFFFFFFFFFF'").unwrap();
        assert_eq!(upper.mask, u64::MAX);
        assert_eq!(
            toml::to_string(&upper).unwrap(),
            "mask = \"0xffffffffffffffff\"\n"
        );
    }

    #[test]
    fn masks_reject_numeric_or_malformed_encodings() {
        for value in [
            "0",
            "-1",
            "0x7fff",
            "''",
            "'0000000000000000'",
            "'0X0000000000000000'",
            "'0x000000000000000'",
            "'0x00000000000000000'",
            "'0x000000000000000g'",
            "'0x000000000000000\u{e9}'",
            "' 0x0000000000000000'",
            "'0x0000000000000000 '",
            "'0x+000000000000000'",
            "'0x00000000000000_0'",
        ] {
            assert!(
                toml::from_str::<Selection>(&format!("mask = {value}")).is_err(),
                "{value}"
            );
        }
    }
}
