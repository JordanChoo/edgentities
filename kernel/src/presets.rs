use std::collections::HashMap;

pub fn list_presets() -> HashMap<String, Vec<String>> {
    let mut presets = HashMap::new();

    presets.insert(
        "general".to_string(),
        vec![
            "PERSON", "ORGANIZATION", "LOCATION", "EVENT",
            "CONCEPT", "TECHNOLOGY", "PRODUCT", "OTHER",
        ].into_iter().map(String::from).collect(),
    );

    presets.insert(
        "manufacturing".to_string(),
        vec![
            "EQUIPMENT", "COMPONENT", "PROCESS", "MATERIAL",
            "DEFECT", "MEASUREMENT", "STANDARD", "FACILITY",
            "OPERATOR", "PRODUCT", "ORGANIZATION", "OTHER",
        ].into_iter().map(String::from).collect(),
    );

    presets.insert(
        "healthcare".to_string(),
        vec![
            "PATIENT", "CONDITION", "MEDICATION", "PROCEDURE",
            "PROVIDER", "FACILITY", "SYMPTOM", "DIAGNOSIS",
            "ANATOMY", "DEVICE", "ORGANIZATION", "OTHER",
        ].into_iter().map(String::from).collect(),
    );

    presets.insert(
        "legal".to_string(),
        vec![
            "PARTY", "STATUTE", "CASE", "COURT", "JURISDICTION",
            "OBLIGATION", "RIGHT", "PROVISION", "DATE", "MONETARY_AMOUNT",
            "ORGANIZATION", "PERSON", "OTHER",
        ].into_iter().map(String::from).collect(),
    );

    presets.insert(
        "research".to_string(),
        vec![
            "AUTHOR", "PUBLICATION", "METHOD", "DATASET", "METRIC",
            "INSTITUTION", "FUNDER", "CONCEPT", "FINDING",
            "EXPERIMENT", "OTHER",
        ].into_iter().map(String::from).collect(),
    );

    presets.insert(
        "finance".to_string(),
        vec![
            "INSTRUMENT", "ENTITY", "TRANSACTION", "MARKET",
            "METRIC", "REGULATION", "PERSON", "ORGANIZATION",
            "DATE", "MONETARY_AMOUNT", "EVENT", "OTHER",
        ].into_iter().map(String::from).collect(),
    );

    presets
}

pub fn get_preset(name: &str) -> Option<Vec<String>> {
    list_presets().remove(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_presets_exist() {
        let presets = list_presets();
        assert!(presets.contains_key("general"));
        assert!(presets.contains_key("manufacturing"));
        assert!(presets.contains_key("healthcare"));
        assert!(presets.contains_key("legal"));
        assert!(presets.contains_key("research"));
        assert!(presets.contains_key("finance"));
        assert_eq!(presets.len(), 6);
    }

    #[test]
    fn test_general_preset() {
        let types = get_preset("general").unwrap();
        assert!(types.contains(&"PERSON".to_string()));
        assert!(types.contains(&"OTHER".to_string()));
    }

    #[test]
    fn test_unknown_preset_returns_none() {
        assert!(get_preset("nonexistent").is_none());
    }
}
