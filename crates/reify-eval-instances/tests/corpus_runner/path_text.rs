//! A path an entry names is read when the entry loads: its text parses into
//! a `reify_core::InstancePath` and must print back as the same text, so an
//! entry holding a text the one text form does not give fails the load,
//! naming the text.

use reify_core::InstancePath;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

pub fn path<'de, D: Deserializer<'de>>(deserializer: D) -> Result<InstancePath, D::Error> {
    let text = String::deserialize(deserializer)?;
    read(&text).map_err(D::Error::custom)
}

pub fn optional_path<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<InstancePath>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|text| read(&text))
        .transpose()
        .map_err(D::Error::custom)
}

fn read(text: &str) -> Result<InstancePath, String> {
    let path: InstancePath = text
        .parse()
        .map_err(|error| format!("`{text}` is not a path's text: {error}"))?;
    let printed = path.to_string();
    if printed != text {
        return Err(format!("the path `{text}` prints back as `{printed}`"));
    }
    Ok(path)
}
