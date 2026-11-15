pub mod milliseconds {
    use chrono::TimeDelta;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        duration: &TimeDelta,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(duration.num_milliseconds())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<TimeDelta, D::Error> {
        i64::deserialize(deserializer).map(TimeDelta::milliseconds)
    }
}

pub mod milliseconds_option {
    use chrono::TimeDelta;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        duration: &Option<TimeDelta>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match duration {
            Some(duration) => serializer.serialize_some(&duration.num_milliseconds()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<TimeDelta>, D::Error> {
        Option::<i64>::deserialize(deserializer)
            .map(|duration| duration.map(TimeDelta::milliseconds))
    }
}
