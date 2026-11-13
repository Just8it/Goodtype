//! Normalize a copied Jupyter file before any canonical page is published. Never executes cells.
use super::{StorageError, invalid_error};
use crate::calculation::{Calculation, Cell, MAX_CALCULATION_BYTES, MAX_CELLS};
use serde_json::{Value, json};

pub(super) fn prepare(bytes: &[u8], title: &str) -> Result<Calculation, StorageError> {
    if bytes.len() > MAX_CALCULATION_BYTES {
        return Err(invalid_error(
            "imported notebook exceeds the calculation size limit",
        ));
    }
    let value: Value = serde_json::from_slice(bytes)?;
    if value.get("nbformat").and_then(Value::as_u64) != Some(4)
        || value
            .get("nbformat_minor")
            .and_then(Value::as_u64)
            .is_none_or(|minor| minor > 5)
    {
        return Err(invalid_error(
            "import requires a supported nbformat 4 notebook",
        ));
    }
    let metadata = value
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_error("imported notebook metadata must be an object"))?;
    let language = metadata
        .get("language_info")
        .and_then(|info| info.get("name"))
        .or_else(|| {
            metadata
                .get("kernelspec")
                .and_then(|spec| spec.get("language"))
        })
        .and_then(Value::as_str);
    if language.is_some_and(|language| !language.eq_ignore_ascii_case("python")) {
        return Err(invalid_error("this version supports Python notebooks only"));
    }
    let cells = value
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_error("imported notebook cells must be an array"))?;
    if cells.len() > MAX_CELLS {
        return Err(invalid_error("imported notebook contains too many cells"));
    }
    let mut document = Calculation::new("import", "part", title);
    for (key, value) in metadata {
        if key != "goodtype" {
            document.metadata.insert(key.clone(), value.clone());
        } else {
            document
                .goodtype_mut()
                .insert("originalMetadata".into(), value.clone());
        }
    }
    document
        .goodtype_mut()
        .insert("imported".into(), json!(true));
    document.cells = cells.iter().enumerate().map(|(index, cell)| {
        let mut cell = cell.as_object().cloned().ok_or_else(|| invalid_error("imported cell must be an object"))?;
        let original_id = cell.insert("id".into(), json!(format!("import-{index}")));
        let metadata = cell.entry("metadata").or_insert(json!({})).as_object_mut()
            .ok_or_else(|| invalid_error("imported cell metadata must be an object"))?;
        let original = metadata.remove("goodtype");
        metadata.insert("goodtype".into(), json!({"partId":"part", "copiedFrom":{"imported":true,"cellId":original_id}, "originalMetadata":original}));
        serde_json::from_value::<Cell>(Value::Object(cell)).map_err(StorageError::from)
    }).collect::<Result<Vec<_>, _>>()?;
    if let Some(root) = value.as_object() {
        for (key, value) in root {
            if !matches!(
                key.as_str(),
                "nbformat" | "nbformat_minor" | "metadata" | "cells"
            ) {
                document.extra.insert(key.clone(), value.clone());
            }
        }
    }
    document.bytes()?;
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_preserves_sources_outputs_and_metadata_without_execution() {
        let source = json!({"nbformat":4,"nbformat_minor":2,"metadata":{"language_info":{"name":"python"},"user":{"x":2}},"cells":[
            {"cell_type":"code","source":["x = 2\n","print(x)"],"metadata":{},"execution_count":1,"outputs":[{"output_type":"stream","name":"stdout","text":["2\n"]}]},
            {"cell_type":"markdown","source":"# Notes","metadata":{},"attachments":{"original":{}}}
        ]});
        let bytes = serde_json::to_vec(&source).unwrap();
        let document = prepare(&bytes, "Task A").unwrap();
        assert_eq!(document.cells[0].source, "x = 2\nprint(x)");
        assert_eq!(document.metadata["user"]["x"], 2);
        assert!(
            document.cells[0].metadata["goodtype"]["copiedFrom"]["imported"]
                .as_bool()
                .unwrap()
        );
        assert!(document.cells[1].extra.contains_key("attachments"));
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), source);
    }
    #[test]
    fn import_rejects_unsupported_languages_and_malformed_cells() {
        for value in [
            json!({"nbformat":3,"nbformat_minor":0,"metadata":{},"cells":[]}),
            json!({"nbformat":4,"nbformat_minor":5,"metadata":{"language_info":{"name":"matlab"}},"cells":[]}),
            json!({"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","source":7,"metadata":{}}]}),
        ] {
            assert!(prepare(&serde_json::to_vec(&value).unwrap(), "Imported").is_err());
        }
    }
}
