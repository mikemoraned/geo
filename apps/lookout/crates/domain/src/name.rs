use hashfn::hashfn;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is empty or holds a character a name cannot be written with")]
pub struct NameError(String);

#[hashfn]
pub fn checked(id: String) -> Result<String, NameError> {
    const RESERVED: [char; 3] = ['/', ' ', '='];
    if id.is_empty() || id.contains(RESERVED) {
        Err(NameError(id))
    } else {
        Ok(id)
    }
}
