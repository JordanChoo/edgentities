use crate::parser::{ParsedEntity, ParsedRelationship};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct MergedEntity {
    pub name: String,
    pub entity_type: String,
    pub description: String,
    pub source_pass: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MergedRelationship {
    pub source: String,
    pub target: String,
    pub keywords: Vec<String>,
    pub description: String,
    pub source_pass: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DedupeResult {
    pub entities: Vec<MergedEntity>,
    pub relationships: Vec<MergedRelationship>,
}

pub struct PassData {
    pub entities: Vec<ParsedEntity>,
    pub relationships: Vec<ParsedRelationship>,
    pub pass_index: u32,
}

pub fn dedupe_and_merge(passes: &[PassData], merge_descriptions: bool) -> DedupeResult {
    let mut entity_groups: HashMap<String, Vec<(u32, &ParsedEntity)>> = HashMap::new();
    let mut rel_groups: HashMap<(String, String), Vec<(u32, &ParsedRelationship)>> = HashMap::new();

    for pass in passes {
        for entity in &pass.entities {
            entity_groups
                .entry(entity.name.clone())
                .or_default()
                .push((pass.pass_index, entity));
        }
        for rel in &pass.relationships {
            let key = make_rel_key(&rel.source, &rel.target);
            rel_groups
                .entry(key)
                .or_default()
                .push((pass.pass_index, rel));
        }
    }

    let mut entities: Vec<MergedEntity> = entity_groups
        .into_iter()
        .map(|(name, group)| {
            let source_pass = group.iter().map(|(p, _)| *p).min().unwrap_or(0);
            let entity_type = group
                .iter()
                .map(|(_, e)| e.entity_type.as_str())
                .find(|t| *t != "OTHER")
                .unwrap_or("OTHER")
                .to_string();
            let description = merge_descriptions_fn(
                group.iter().map(|(_, e)| e.description.as_str()),
                merge_descriptions,
            );

            MergedEntity {
                name,
                entity_type,
                description,
                source_pass,
            }
        })
        .collect();

    entities.sort_by(|a, b| a.source_pass.cmp(&b.source_pass).then(a.name.cmp(&b.name)));

    let mut relationships: Vec<MergedRelationship> = rel_groups
        .into_iter()
        .map(|((source, target), group)| {
            let source_pass = group.iter().map(|(p, _)| *p).min().unwrap_or(0);
            let description = merge_descriptions_fn(
                group.iter().map(|(_, r)| r.description.as_str()),
                merge_descriptions,
            );
            let mut all_keywords: Vec<String> = group
                .iter()
                .flat_map(|(_, r)| r.keywords.iter().cloned())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            all_keywords.sort();

            MergedRelationship {
                source,
                target,
                keywords: all_keywords,
                description,
                source_pass,
            }
        })
        .collect();

    relationships.sort_by(|a, b| {
        a.source_pass
            .cmp(&b.source_pass)
            .then(a.source.cmp(&b.source))
            .then(a.target.cmp(&b.target))
    });

    DedupeResult {
        entities,
        relationships,
    }
}

fn make_rel_key(source: &str, target: &str) -> (String, String) {
    if source <= target {
        (source.to_string(), target.to_string())
    } else {
        (target.to_string(), source.to_string())
    }
}

fn merge_descriptions_fn<'a>(
    descriptions: impl Iterator<Item = &'a str>,
    concatenate: bool,
) -> String {
    let descs: Vec<&str> = descriptions.filter(|d| !d.is_empty()).collect();
    if descs.is_empty() {
        return String::new();
    }
    if concatenate && descs.len() > 1 {
        let mut unique: Vec<&str> = Vec::new();
        for d in &descs {
            if !unique.contains(d) {
                unique.push(d);
            }
        }
        unique.join(" | ")
    } else {
        descs.into_iter().max_by_key(|d| d.len()).unwrap_or("").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_dedupe_same_name() {
        let passes = vec![
            PassData {
                pass_index: 0,
                entities: vec![ParsedEntity {
                    name: "JOHN_DOE".to_string(),
                    entity_type: "OTHER".to_string(),
                    description: "A person".to_string(),
                }],
                relationships: vec![],
            },
            PassData {
                pass_index: 1,
                entities: vec![ParsedEntity {
                    name: "JOHN_DOE".to_string(),
                    entity_type: "PERSON".to_string(),
                    description: "John Doe is a software developer at Acme Corp".to_string(),
                }],
                relationships: vec![],
            },
        ];
        let result = dedupe_and_merge(&passes, false);
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].entity_type, "PERSON");
        assert!(result.entities[0].description.contains("software developer"));
        assert_eq!(result.entities[0].source_pass, 0);
    }

    #[test]
    fn test_relationship_dedupe_undirected() {
        let passes = vec![
            PassData {
                pass_index: 0,
                entities: vec![],
                relationships: vec![ParsedRelationship {
                    source: "ALICE".to_string(),
                    target: "BOB".to_string(),
                    keywords: vec!["collaboration".to_string()],
                    description: "They work together".to_string(),
                }],
            },
            PassData {
                pass_index: 1,
                entities: vec![],
                relationships: vec![ParsedRelationship {
                    source: "BOB".to_string(),
                    target: "ALICE".to_string(),
                    keywords: vec!["research".to_string()],
                    description: "Bob and Alice collaborate on research projects together".to_string(),
                }],
            },
        ];
        let result = dedupe_and_merge(&passes, false);
        assert_eq!(result.relationships.len(), 1);
        assert!(result.relationships[0].keywords.contains(&"collaboration".to_string()));
        assert!(result.relationships[0].keywords.contains(&"research".to_string()));
    }

    #[test]
    fn test_merge_descriptions_concatenate() {
        let passes = vec![
            PassData {
                pass_index: 0,
                entities: vec![ParsedEntity {
                    name: "X".to_string(),
                    entity_type: "CONCEPT".to_string(),
                    description: "First".to_string(),
                }],
                relationships: vec![],
            },
            PassData {
                pass_index: 1,
                entities: vec![ParsedEntity {
                    name: "X".to_string(),
                    entity_type: "CONCEPT".to_string(),
                    description: "Second".to_string(),
                }],
                relationships: vec![],
            },
        ];
        let result = dedupe_and_merge(&passes, true);
        assert_eq!(result.entities[0].description, "First | Second");
    }
}
