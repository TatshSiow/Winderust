use serde::{de, Deserialize, Deserializer, Serializer};

const MILLIS_PER_SECOND: u64 = 1_000;

#[derive(Deserialize)]
#[serde(untagged)]
enum SavedSeconds {
    Whole(u64),
    Fractional(f64),
}

pub(super) fn serialize<S: Serializer>(
    milliseconds: &u64,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if milliseconds.is_multiple_of(MILLIS_PER_SECOND) {
        serializer.serialize_u64(milliseconds / MILLIS_PER_SECOND)
    } else {
        serializer.serialize_f64(*milliseconds as f64 / MILLIS_PER_SECOND as f64)
    }
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let milliseconds = match SavedSeconds::deserialize(deserializer)? {
        SavedSeconds::Whole(seconds) => seconds.checked_mul(MILLIS_PER_SECOND),
        SavedSeconds::Fractional(seconds) => {
            let scale = MILLIS_PER_SECOND as f64;
            let milliseconds = (seconds * scale).round();
            // Compare in seconds so valid decimal milliseconds survive IEEE 754 rounding.
            (!seconds.is_sign_negative()
                && (0.0..u64::MAX as f64).contains(&milliseconds)
                && milliseconds / scale == seconds)
                .then_some(milliseconds as u64)
        }
    };
    milliseconds.ok_or_else(|| {
        de::Error::custom("restraint time must be nonnegative seconds with millisecond precision")
    })
}
