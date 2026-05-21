mod dedupe;
mod normalizer;
mod parser;
mod presets;
mod prompts;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn build_system_prompt(entity_types_json: &str, language: &str, tuple_delimiter: &str, completion_delimiter: &str) -> String {
    let types: Vec<String> = serde_json::from_str(entity_types_json).unwrap_or_default();
    prompts::build_system_prompt(&types, language, tuple_delimiter, completion_delimiter)
}

#[wasm_bindgen]
pub fn build_user_prompt(text: &str, entity_types_json: &str, language: &str, completion_delimiter: &str) -> String {
    let types: Vec<String> = serde_json::from_str(entity_types_json).unwrap_or_default();
    prompts::build_user_prompt(text, &types, language, completion_delimiter)
}

#[wasm_bindgen]
pub fn build_continue_prompt(language: &str, tuple_delimiter: &str, completion_delimiter: &str) -> String {
    prompts::build_continue_prompt(language, tuple_delimiter, completion_delimiter)
}

#[wasm_bindgen]
pub fn parse_response(raw: &str, tuple_delim: &str, completion_delim: &str) -> JsValue {
    let result = parser::parse_response(raw, tuple_delim, completion_delim);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
pub fn normalize_entity_name(name: &str) -> String {
    normalizer::normalize_entity_name(name)
}

#[wasm_bindgen]
pub fn dedupe_and_merge(passes_json: &str, merge_descriptions: bool) -> JsValue {
    #[derive(serde::Deserialize)]
    struct PassInput {
        pass_index: u32,
        entities: Vec<parser::ParsedEntity>,
        relationships: Vec<parser::ParsedRelationship>,
    }

    let pass_inputs: Vec<PassInput> = match serde_json::from_str(passes_json) {
        Ok(v) => v,
        Err(_) => return JsValue::NULL,
    };

    let pass_data: Vec<dedupe::PassData> = pass_inputs
        .into_iter()
        .map(|p| dedupe::PassData {
            pass_index: p.pass_index,
            entities: p.entities,
            relationships: p.relationships,
        })
        .collect();

    let result = dedupe::dedupe_and_merge(&pass_data, merge_descriptions);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
pub fn list_presets() -> String {
    let presets = presets::list_presets();
    serde_json::to_string(&presets).unwrap_or_else(|_| "{}".to_string())
}

#[wasm_bindgen]
pub fn get_preset(name: &str) -> String {
    match presets::get_preset(name) {
        Some(types) => serde_json::to_string(&types).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    }
}
