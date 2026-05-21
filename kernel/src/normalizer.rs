pub fn normalize_entity_name(raw_name: &str) -> String {
    let trimmed = raw_name.trim();

    let without_prefix = trimmed
        .strip_prefix("The ")
        .or_else(|| trimmed.strip_prefix("the "))
        .or_else(|| trimmed.strip_prefix("A "))
        .or_else(|| trimmed.strip_prefix("a "))
        .or_else(|| trimmed.strip_prefix("An "))
        .or_else(|| trimmed.strip_prefix("an "))
        .unwrap_or(trimmed);

    without_prefix
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|word| {
            let without_possessive = word
                .strip_suffix("'s")
                .or_else(|| word.strip_suffix("\u{2019}s"))
                .unwrap_or(word);
            to_title_case(without_possessive)
        })
        .collect::<Vec<_>>()
        .join("_")
        .to_uppercase()
}

fn to_title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(|c| c.to_lowercase()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_normalization() {
        assert_eq!(normalize_entity_name("John Doe"), "JOHN_DOE");
        assert_eq!(normalize_entity_name("john doe"), "JOHN_DOE");
        assert_eq!(normalize_entity_name("JOHN DOE"), "JOHN_DOE");
    }

    #[test]
    fn test_whitespace_handling() {
        assert_eq!(normalize_entity_name("  John  Doe  "), "JOHN_DOE");
        assert_eq!(normalize_entity_name("\tJohn\nDoe\r"), "JOHN_DOE");
        assert_eq!(normalize_entity_name("John   Doe"), "JOHN_DOE");
    }

    #[test]
    fn test_prefix_removal() {
        assert_eq!(normalize_entity_name("The Company"), "COMPANY");
        assert_eq!(normalize_entity_name("the company"), "COMPANY");
        assert_eq!(normalize_entity_name("A Person"), "PERSON");
        assert_eq!(normalize_entity_name("An Event"), "EVENT");
    }

    #[test]
    fn test_possessive_removal() {
        assert_eq!(normalize_entity_name("John's"), "JOHN");
        assert_eq!(normalize_entity_name("Company's Products"), "COMPANY_PRODUCTS");
    }

    #[test]
    fn test_title_case_conversion() {
        assert_eq!(normalize_entity_name("jOHN dOE"), "JOHN_DOE");
        assert_eq!(normalize_entity_name("mCdonald"), "MCDONALD");
    }

    #[test]
    fn test_empty_and_edge_cases() {
        assert_eq!(normalize_entity_name(""), "");
        assert_eq!(normalize_entity_name("   "), "");
        assert_eq!(normalize_entity_name("A"), "A");
        assert_eq!(normalize_entity_name("I"), "I");
    }

    #[test]
    fn test_special_characters_preserved() {
        assert_eq!(normalize_entity_name("New-York"), "NEW-YORK");
        assert_eq!(normalize_entity_name("C++"), "C++");
    }
}
