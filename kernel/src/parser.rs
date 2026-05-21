use crate::normalizer::normalize_entity_name;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEntity {
    pub name: String,
    pub entity_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedRelationship {
    pub source: String,
    pub target: String,
    pub keywords: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParseResult {
    pub entities: Vec<ParsedEntity>,
    pub relationships: Vec<ParsedRelationship>,
    pub is_complete: bool,
    pub malformed_lines_dropped: u32,
}

pub fn parse_response(raw: &str, tuple_delim: &str, completion_delim: &str) -> ParseResult {
    let mut entities = Vec::new();
    let mut relationships = Vec::new();
    let mut is_complete = false;
    let mut malformed_lines_dropped: u32 = 0;

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == completion_delim {
            is_complete = true;
            break;
        }

        let parts: Vec<&str> = line.split(tuple_delim).collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0].trim().to_lowercase().as_str() {
            "entity" if parts.len() >= 4 => {
                let raw_name = parts[1].trim();
                let entity_type = parts[2].trim().to_uppercase();
                let description = parts[3].trim().to_string();

                if raw_name.is_empty() {
                    malformed_lines_dropped += 1;
                    continue;
                }

                let normalized_name = normalize_entity_name(raw_name);
                if normalized_name.is_empty() {
                    continue;
                }

                entities.push(ParsedEntity {
                    name: normalized_name,
                    entity_type,
                    description,
                });
            }
            "relation" | "relationship" if parts.len() >= 5 => {
                let source = parts[1].trim();
                let target = parts[2].trim();
                let keywords_str = parts[3].trim();
                let description = parts[4].trim().to_string();

                let normalized_source = normalize_entity_name(source);
                let normalized_target = normalize_entity_name(target);

                if normalized_source == normalized_target {
                    continue;
                }

                if normalized_source.is_empty() || normalized_target.is_empty() {
                    continue;
                }

                let keywords: Vec<String> = keywords_str
                    .split(',')
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
                    .take(5)
                    .collect();

                relationships.push(ParsedRelationship {
                    source: normalized_source,
                    target: normalized_target,
                    keywords,
                    description,
                });
            }
            _ => {
                if line.contains(tuple_delim) {
                    malformed_lines_dropped += 1;
                }
            }
        }
    }

    ParseResult {
        entities,
        relationships,
        is_complete,
        malformed_lines_dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TD: &str = "<|#|>";
    const CD: &str = "<|COMPLETE|>";

    #[test]
    fn test_parse_entities() {
        let response = "entity<|#|>John Doe<|#|>PERSON<|#|>A software developer\n\
                         entity<|#|>Acme Corp<|#|>ORGANIZATION<|#|>A technology company\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.entities.len(), 2);
        assert_eq!(result.entities[0].name, "JOHN_DOE");
        assert_eq!(result.entities[0].entity_type, "PERSON");
        assert_eq!(result.entities[1].name, "ACME_CORP");
        assert!(result.is_complete);
    }

    #[test]
    fn test_parse_relationships() {
        let response = "entity<|#|>Alice<|#|>PERSON<|#|>A researcher\n\
                         entity<|#|>Bob<|#|>PERSON<|#|>Another researcher\n\
                         relation<|#|>Alice<|#|>Bob<|#|>collaboration, research<|#|>Alice and Bob work together\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].source, "ALICE");
        assert_eq!(result.relationships[0].target, "BOB");
        assert_eq!(result.relationships[0].keywords.len(), 2);
    }

    #[test]
    fn test_incomplete_response() {
        let response = "entity<|#|>John<|#|>PERSON<|#|>A person";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.entities.len(), 1);
        assert!(!result.is_complete);
    }

    #[test]
    fn test_malformed_lines() {
        let response = "entity<|#|>Valid<|#|>PERSON<|#|>Valid entity\n\
                         some random text here\n\
                         entity<|#|><|#|>PERSON<|#|>Empty name should skip\n\
                         entity<|#|>Also Valid<|#|>CONCEPT<|#|>Another valid\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.entities.len(), 2);
        assert!(result.malformed_lines_dropped > 0);
    }

    #[test]
    fn test_self_referencing_filtered() {
        let response = "entity<|#|>Neural Network<|#|>CONCEPT<|#|>A computing model\n\
                         relation<|#|>Neural Network<|#|>Neural Network<|#|>self-reference<|#|>Relates to itself\n\
                         relation<|#|>Neural Network<|#|>Deep Learning<|#|>uses<|#|>Neural networks use deep learning\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.relationships.len(), 1);
        assert_eq!(result.relationships[0].target, "DEEP_LEARNING");
    }

    #[test]
    fn test_normalized_self_ref_filtered() {
        let response = "relation<|#|>The Company<|#|>company<|#|>self<|#|>Same after normalization\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.relationships.len(), 0);
    }

    #[test]
    fn test_keyword_limit() {
        let response = "relation<|#|>A<|#|>B<|#|>k1, k2, k3, k4, k5, k6, k7<|#|>Many keywords\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert!(result.relationships[0].keywords.len() <= 5);
    }

    #[test]
    fn test_empty_endpoints_filtered() {
        let response = "relation<|#|>   <|#|>A<|#|>broken<|#|>Empty source\n\
                         relation<|#|>A<|#|>   <|#|>broken<|#|>Empty target\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.relationships.len(), 0);
    }

    #[test]
    fn test_whitespace_entity_name_filtered() {
        let response = "entity<|#|>   <|#|>CONCEPT<|#|>Whitespace name\n\
                         entity<|#|>Good<|#|>PERSON<|#|>A valid entity\n\
                         <|COMPLETE|>";
        let result = parse_response(response, TD, CD);
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].name, "GOOD");
    }
}
