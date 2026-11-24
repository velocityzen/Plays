//! Text as a sentence shows it.

/// `text` begun with a capital, as a sentence is.
pub fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
#[path = "tests/sentence.rs"]
mod tests;
