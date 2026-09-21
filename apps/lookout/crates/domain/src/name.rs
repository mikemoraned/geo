//! The rule an id follows so that it can be written as a name.

/// A name that could not be written as one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is empty or holds a character a name cannot be written with")]
pub struct NameError(String);

/// Characters an id cannot hold, because whatever writes one out — a directory a store
/// partitions by, a path, a query — would read them as punctuation rather than as the name.
const RESERVED: [char; 3] = ['/', ' ', '='];

/// `id`, where it can be written as a name.
///
/// # Errors
///
/// Returns an error where the id is empty or holds a reserved character.
pub(crate) fn checked(id: String) -> Result<String, NameError> {
    if id.is_empty() || id.contains(RESERVED) {
        Err(NameError(id))
    } else {
        Ok(id)
    }
}
