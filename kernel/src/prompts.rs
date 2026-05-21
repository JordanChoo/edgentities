pub fn build_system_prompt(
    entity_types: &[String],
    language: &str,
    tuple_delimiter: &str,
    completion_delimiter: &str,
) -> String {
    let entity_types_str = entity_types.join(", ");
    let examples = build_examples(tuple_delimiter, completion_delimiter);

    format!(
        r#"---Role---
You are a Knowledge Graph Specialist responsible for extracting entities and relationships from the input text.

---Instructions---
1.  **Entity Extraction & Output:**
    *   **Identification:** Identify clearly defined and meaningful entities in the input text.
    *   **Entity Details:** For each identified entity, extract the following information:
        *   `entity_name`: The name of the entity. If the entity name is case-insensitive, capitalize the first letter of each significant word (title case). Ensure **consistent naming** across the entire extraction process.
        *   `entity_type`: Categorize the entity using one of the following types: `{entity_types}`. If none of the provided entity types apply, classify it as `Other`.
        *   `entity_description`: Provide a concise yet comprehensive description of the entity's attributes and activities, based *solely* on the information present in the input text.
    *   **Output Format - Entities:** Output a total of 4 fields for each entity, delimited by `{tuple_delimiter}`, on a single line. The first field *must* be the literal string `entity`.
        *   Format: `entity{tuple_delimiter}entity_name{tuple_delimiter}entity_type{tuple_delimiter}entity_description`

2.  **Relationship Extraction & Output:**
    *   **Identification:** Identify direct, clearly stated, and meaningful relationships between previously extracted entities.
    *   **N-ary Relationship Decomposition:** If a single statement describes a relationship involving more than two entities (an N-ary relationship), decompose it into multiple binary (two-entity) relationship pairs for separate description.
        *   **Example:** For "Alice, Bob, and Carol collaborated on Project X," extract binary relationships such as "Alice collaborated with Project X," "Bob collaborated with Project X," and "Carol collaborated with Project X."
    *   **Relationship Details:** For each binary relationship, extract the following fields:
        *   `source_entity`: The name of the source entity. Ensure **consistent naming** with entity extraction.
        *   `target_entity`: The name of the target entity. Ensure **consistent naming** with entity extraction.
        *   `relationship_keywords`: One or more high-level keywords summarizing the overarching nature of the relationship. Multiple keywords separated by comma.
        *   `relationship_description`: A concise explanation of the nature of the relationship between the source and target entities.
    *   **Output Format - Relationships:** Output a total of 5 fields for each relationship, delimited by `{tuple_delimiter}`, on a single line. The first field *must* be the literal string `relation`.
        *   Format: `relation{tuple_delimiter}source_entity{tuple_delimiter}target_entity{tuple_delimiter}relationship_keywords{tuple_delimiter}relationship_description`

3.  **Delimiter Usage Protocol:**
    *   The `{tuple_delimiter}` is a complete, atomic marker and **must not be filled with content**. It serves strictly as a field separator.
    *   **Correct Example:** `entity{tuple_delimiter}Tokyo{tuple_delimiter}location{tuple_delimiter}Tokyo is the capital of Japan.`

4.  **Relationship Direction & Duplication:**
    *   Treat all relationships as **undirected** unless explicitly stated otherwise.
    *   Avoid outputting duplicate relationships.

5.  **Output Order & Prioritization:**
    *   Output all extracted entities first, followed by all extracted relationships.
    *   Within the list of relationships, prioritize those that are **most significant** to the core meaning of the input text.

6.  **Context & Objectivity:**
    *   Ensure all entity names and descriptions are written in the **third person**.
    *   Explicitly name the subject or object; **avoid using pronouns** such as `this article`, `our company`, `I`, `you`.

7.  **Language & Proper Nouns:**
    *   The entire output (entity names, keywords, and descriptions) must be written in `{language}`.
    *   Proper nouns should be retained in their original language if translation would cause ambiguity.

8.  **Completion Signal:** Output the literal string `{completion_delimiter}` only after all entities and relationships have been completely extracted.

---Examples---
{examples}"#,
        entity_types = entity_types_str,
        tuple_delimiter = tuple_delimiter,
        language = language,
        completion_delimiter = completion_delimiter,
        examples = examples,
    )
}

pub fn build_user_prompt(
    input_text: &str,
    entity_types: &[String],
    language: &str,
    completion_delimiter: &str,
) -> String {
    let entity_types_str = entity_types.join(", ");

    format!(
        r#"---Task---
Extract entities and relationships from the input text below.

---Instructions---
1. Strictly adhere to all format requirements for entity and relationship lists.
2. Output *only* the extracted list of entities and relationships. No introductory or concluding remarks.
3. Output `{completion_delimiter}` as the final line after all extractions.
4. Ensure the output language is {language}.

---Data to be Processed---
<Entity_types>
[{entity_types}]

<Input Text>
```
{input_text}
```

<Output>"#,
        completion_delimiter = completion_delimiter,
        language = language,
        entity_types = entity_types_str,
        input_text = input_text,
    )
}

pub fn build_continue_prompt(
    language: &str,
    tuple_delimiter: &str,
    completion_delimiter: &str,
) -> String {
    format!(
        r#"---Task---
Based on the last extraction task, identify and extract any **missed or incorrectly formatted** entities and relationships from the input text.

---Instructions---
1.  **Strict Adherence to System Format:** Follow all format requirements from the system instructions.
2.  **Focus on Corrections/Additions:**
    *   **Do NOT** re-output entities and relationships that were **correctly and fully** extracted.
    *   If an entity or relationship was **missed**, extract and output it now.
    *   If an entity or relationship was **truncated or malformed**, re-output the *corrected and complete* version.
3.  **Output Format - Entities:** 4 fields per entity, delimited by `{tuple_delimiter}`.
4.  **Output Format - Relationships:** 5 fields per relationship, delimited by `{tuple_delimiter}`.
5.  **Output Content Only:** No introductory or concluding remarks.
6.  **Completion Signal:** Output `{completion_delimiter}` as the final line.
7.  **Output Language:** Ensure the output language is {language}.

<Output>"#,
        tuple_delimiter = tuple_delimiter,
        completion_delimiter = completion_delimiter,
        language = language,
    )
}

fn build_examples(td: &str, cd: &str) -> String {
    format!(
        r#"
Example 1:
<Input Text>
while Alex clenched his jaw, the buzz of frustration dull against the backdrop of Taylor's authoritarian certainty. It was this competitive undercurrent that kept him alert, the sense that his and Jordan's shared commitment to discovery was an unspoken rebellion against Cruz's narrowing vision of control and order.

<Output>
entity{td}Alex{td}PERSON{td}Alex is a character who experiences frustration and is observant of the dynamics among other characters.
entity{td}Taylor{td}PERSON{td}Taylor is portrayed with authoritarian certainty and shows a moment of reverence towards a device.
entity{td}Jordan{td}PERSON{td}Jordan shares a commitment to discovery with Alex.
entity{td}Cruz{td}PERSON{td}Cruz is associated with a vision of control and order.
relation{td}Alex{td}Taylor{td}power dynamics, observation{td}Alex observes Taylor's authoritarian behavior.
relation{td}Alex{td}Jordan{td}shared goals, rebellion{td}Alex and Jordan share a commitment to discovery.
relation{td}Jordan{td}Cruz{td}ideological conflict{td}Jordan's discovery commitment rebels against Cruz's control vision.
{cd}

Example 2:
<Input Text>
Stock markets faced a sharp downturn today as tech giants saw significant declines, with the global tech index dropping by 3.4%.

<Output>
entity{td}Global Tech Index{td}CONCEPT{td}The Global Tech Index tracks major technology stocks and dropped 3.4%.
entity{td}Market Selloff{td}EVENT{td}Market selloff refers to the significant decline in stock values.
relation{td}Global Tech Index{td}Market Selloff{td}market performance{td}The tech index decline is part of the broader selloff.
{cd}

Example 3:
<Input Text>
Dr. Sarah Chen, lead researcher at Quantum Dynamics Lab in Boston, published a groundbreaking paper on quantum entanglement in Nature Physics journal. The study was funded by the National Science Foundation.

<Output>
entity{td}Sarah Chen{td}PERSON{td}Dr. Sarah Chen is the lead researcher at Quantum Dynamics Lab who published a paper on quantum entanglement.
entity{td}Quantum Dynamics Lab{td}ORGANIZATION{td}Quantum Dynamics Lab is a research institution located in Boston.
entity{td}Boston{td}LOCATION{td}Boston is a city where Quantum Dynamics Lab is located.
entity{td}Nature Physics{td}ORGANIZATION{td}Nature Physics is a scientific journal that published Sarah Chen's paper.
entity{td}Quantum Entanglement{td}CONCEPT{td}Quantum entanglement is a physics phenomenon studied in Sarah Chen's groundbreaking paper.
entity{td}National Science Foundation{td}ORGANIZATION{td}The National Science Foundation funded Sarah Chen's quantum entanglement research.
relation{td}Sarah Chen{td}Quantum Dynamics Lab{td}employment, research{td}Sarah Chen works as lead researcher at Quantum Dynamics Lab.
relation{td}Sarah Chen{td}Nature Physics{td}publication{td}Sarah Chen published her research in Nature Physics journal.
relation{td}Sarah Chen{td}Quantum Entanglement{td}research{td}Sarah Chen researches quantum entanglement.
relation{td}Quantum Dynamics Lab{td}Boston{td}location{td}Quantum Dynamics Lab is located in Boston.
relation{td}National Science Foundation{td}Sarah Chen{td}funding{td}The National Science Foundation funded Sarah Chen's research.
{cd}
"#,
        td = td,
        cd = cd,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_contains_entity_types() {
        let types = vec!["PERSON".to_string(), "ORGANIZATION".to_string()];
        let prompt = build_system_prompt(&types, "English", "<|#|>", "<|COMPLETE|>");
        assert!(prompt.contains("PERSON, ORGANIZATION"));
        assert!(prompt.contains("Knowledge Graph Specialist"));
        assert!(prompt.contains("<|#|>"));
        assert!(prompt.contains("<|COMPLETE|>"));
    }

    #[test]
    fn test_user_prompt_contains_text() {
        let types = vec!["PERSON".to_string()];
        let prompt = build_user_prompt("Test text here", &types, "English", "<|COMPLETE|>");
        assert!(prompt.contains("Test text here"));
        assert!(prompt.contains("<|COMPLETE|>"));
        assert!(prompt.contains("PERSON"));
    }

    #[test]
    fn test_continue_prompt() {
        let prompt = build_continue_prompt("English", "<|#|>", "<|COMPLETE|>");
        assert!(prompt.contains("missed or incorrectly formatted"));
        assert!(prompt.contains("<|#|>"));
        assert!(prompt.contains("<|COMPLETE|>"));
    }

    #[test]
    fn test_examples_in_system_prompt() {
        let types = vec!["PERSON".to_string()];
        let prompt = build_system_prompt(&types, "English", "<|#|>", "<|COMPLETE|>");
        assert!(prompt.contains("Example 1:"));
        assert!(prompt.contains("Example 2:"));
        assert!(prompt.contains("Example 3:"));
        assert!(prompt.contains("Alex"));
        assert!(prompt.contains("Sarah Chen"));
    }
}
