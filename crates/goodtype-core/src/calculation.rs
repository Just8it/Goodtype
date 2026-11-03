//! Ordinary nbformat 4.5 files owned and validated by Goodtype (ADR 0042).
//!
//! The ordered `cells` array owns execution order. Goodtype pages own presentation order and
//! refer to one calculation part each. Everything in a calculation file is untrusted input:
//! validation is strict, bounded and runs before every read is admitted and every write.
use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::storage::StorageError;

pub const MAX_CALCULATION_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CELL_SOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_CELLS: usize = 1000;
pub const MAX_OUTPUTS_PER_CELL: usize = 1000;
pub const MAX_TITLE_BYTES: usize = 1024;
/// Runtime provenance is a small identity record, not a second output channel.
pub const MAX_PROVENANCE_BYTES: usize = 4096;
pub const A4_WIDTH_PT: f64 = 210.0 * 72.0 / 25.4;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalculationReference {
    pub id: String,
    pub source_path: String,
    pub part_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CalculationPart {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Calculation {
    pub nbformat: u32,
    pub nbformat_minor: u32,
    pub metadata: Map<String, Value>,
    pub cells: Vec<Cell>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Cell {
    pub id: String,
    pub cell_type: String,
    #[serde(deserialize_with = "source_text")]
    pub source: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub execution_count: Option<u64>,
    #[serde(default)]
    pub outputs: Vec<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// nbformat requires execution fields on code cells and forbids them on Markdown and raw cells.
impl Serialize for Cell {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.id)?;
        map.serialize_entry("cell_type", &self.cell_type)?;
        map.serialize_entry("source", &self.source)?;
        map.serialize_entry("metadata", &self.metadata)?;
        if self.cell_type == "code" {
            map.serialize_entry("execution_count", &self.execution_count)?;
            map.serialize_entry("outputs", &self.outputs)?;
        }
        for (key, value) in &self.extra {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

fn source_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(text) => Ok(text),
        Value::Array(lines) => lines
            .into_iter()
            .map(|line| match line {
                Value::String(text) => Ok(text),
                _ => Err(serde::de::Error::custom(
                    "cell source lines must be strings",
                )),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|lines| lines.concat()),
        _ => Err(serde::de::Error::custom(
            "cell source must be text or a list of text lines",
        )),
    }
}

/// The nbformat 4.5 cell-identity rule, which Goodtype also applies to its own identities.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// SHA-256 of the exact submitted source bytes. Output provenance compares this value.
pub fn source_hash(source: &str) -> String {
    format!("{:x}", Sha256::digest(source.as_bytes()))
}

fn invalid(message: &str) -> StorageError {
    StorageError::InvalidNotebook(message.to_owned())
}

/// nbformat multiline strings are one string or a list of strings.
fn multiline(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(_)) => true,
        Some(Value::Array(lines)) => lines.iter().all(Value::is_string),
        _ => false,
    }
}

/// Validate one saved output against the nbformat 4.5 output schema, without its MIME bodies.
/// Unknown MIME types stay preserved data; only their envelope is checked here.
pub fn validate_output(output: &Value) -> Result<(), StorageError> {
    let output = output
        .as_object()
        .ok_or_else(|| invalid("calculation output must be an object"))?;
    let object_or_absent = |key: &str| output.get(key).is_none_or(Value::is_object);
    let valid = match output.get("output_type").and_then(Value::as_str) {
        Some("stream") => {
            matches!(
                output.get("name").and_then(Value::as_str),
                Some("stdout" | "stderr")
            ) && multiline(output.get("text"))
        }
        Some("display_data") => {
            output.get("data").is_some_and(Value::is_object) && object_or_absent("metadata")
        }
        Some("execute_result") => {
            output.get("data").is_some_and(Value::is_object)
                && object_or_absent("metadata")
                && output
                    .get("execution_count")
                    .is_none_or(|count| count.is_null() || count.is_u64())
        }
        Some("error") => {
            output.get("ename").is_some_and(Value::is_string)
                && output.get("evalue").is_some_and(Value::is_string)
                && output
                    .get("traceback")
                    .and_then(Value::as_array)
                    .is_some_and(|lines| lines.iter().all(Value::is_string))
        }
        _ => return Err(invalid("unknown calculation output type")),
    };
    if !valid {
        return Err(invalid(
            "calculation output does not match its nbformat type",
        ));
    }
    if let Some(Value::Object(data)) = output.get("data") {
        // Text MIME bodies are multiline strings; JSON MIME bodies may be any JSON value.
        if data
            .iter()
            .any(|(mime, body)| !mime.ends_with("json") && !multiline(Some(body)))
        {
            return Err(invalid("calculation output MIME data must be text"));
        }
    }
    Ok(())
}

fn validate_outputs(outputs: &[Value]) -> Result<(), StorageError> {
    if outputs.len() > MAX_OUTPUTS_PER_CELL {
        return Err(invalid("calculation cell has too many outputs"));
    }
    outputs.iter().try_for_each(validate_output)
}

impl Cell {
    pub fn new(id: String, part_id: &str, cell_type: &str) -> Self {
        let mut namespace = Map::new();
        namespace.insert("partId".into(), json!(part_id));
        let mut metadata = Map::new();
        metadata.insert("goodtype".into(), Value::Object(namespace));
        Self {
            id,
            cell_type: cell_type.to_owned(),
            source: String::new(),
            metadata,
            execution_count: None,
            outputs: Vec::new(),
            extra: Map::new(),
        }
    }

    pub fn part_id(&self) -> Option<&str> {
        self.metadata.get("goodtype")?.get("partId")?.as_str()
    }

    /// The Goodtype namespace. Validated cells always have it.
    pub fn goodtype(&self) -> Option<&Map<String, Value>> {
        self.metadata.get("goodtype")?.as_object()
    }

    pub fn goodtype_mut(&mut self) -> &mut Map<String, Value> {
        let namespace = self
            .metadata
            .entry("goodtype")
            .or_insert_with(|| Value::Object(Map::new()));
        if !namespace.is_object() {
            *namespace = Value::Object(Map::new());
        }
        namespace.as_object_mut().expect("namespace is an object")
    }

    fn validate_goodtype(&self) -> Result<(), StorageError> {
        let namespace = self
            .goodtype()
            .ok_or_else(|| invalid("cell has no Goodtype metadata"))?;
        if let Some(execution) = namespace.get("execution") {
            let hash = execution.get("sourceHash").and_then(Value::as_str);
            if !execution.is_object()
                || !hash.is_some_and(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            {
                return Err(invalid("cell execution provenance is invalid"));
            }
        }
        if let Some(last) = namespace.get("lastSuccess") {
            let outputs = last
                .get("outputs")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("cell last-success record is invalid"))?;
            validate_outputs(outputs)?;
        }
        if namespace
            .get("copiedFrom")
            .is_some_and(|value| !value.is_object())
        {
            return Err(invalid("cell copy provenance is invalid"));
        }
        Ok(())
    }
}

impl Calculation {
    pub fn new(id: &str, part_id: &str, title: &str) -> Self {
        let metadata = json!({
            "kernelspec": {"display_name": "Python", "language": "python", "name": "python3"},
            "language_info": {"name": "python"},
            "goodtype": {"id": id, "title": title, "revision": 1, "parts": [{"id": part_id, "title": title}]}
        });
        Self {
            nbformat: 4,
            nbformat_minor: 5,
            metadata: metadata.as_object().cloned().expect("literal object"),
            cells: vec![Cell::new(format!("cell-{part_id}"), part_id, "code")],
            extra: Map::new(),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() > MAX_CALCULATION_BYTES {
            return Err(invalid("calculation exceeds the source/output size limit"));
        }
        let value: Self = serde_json::from_slice(bytes)?;
        value.validate()?;
        Ok(value)
    }

    pub fn bytes(&self) -> Result<Vec<u8>, StorageError> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self)?;
        // Jupyter writes a final newline; keeping it avoids a diff on the first external save.
        bytes.push(b'\n');
        if bytes.len() > MAX_CALCULATION_BYTES {
            return Err(invalid("calculation exceeds the source/output size limit"));
        }
        Ok(bytes)
    }

    fn goodtype(&self) -> Option<&Map<String, Value>> {
        self.metadata.get("goodtype")?.as_object()
    }

    pub fn id(&self) -> &str {
        self.goodtype()
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    pub fn title(&self) -> &str {
        self.goodtype()
            .and_then(|value| value.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    pub fn revision(&self) -> u64 {
        self.goodtype()
            .and_then(|value| value.get("revision"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    }

    pub fn parts(&self) -> Result<Vec<CalculationPart>, StorageError> {
        serde_json::from_value(
            self.goodtype()
                .and_then(|value| value.get("parts"))
                .cloned()
                .ok_or_else(|| invalid("calculation parts are missing"))?,
        )
        .map_err(StorageError::from)
    }

    pub fn goodtype_mut(&mut self) -> &mut Map<String, Value> {
        self.metadata
            .get_mut("goodtype")
            .and_then(Value::as_object_mut)
            .expect("validated Goodtype metadata")
    }

    /// Set the next revision. Revisions only increase, so a restored history entry still
    /// receives a newer revision than every state written before it.
    pub fn set_revision_after(&mut self, current: u64) -> Result<(), StorageError> {
        let next = current
            .checked_add(1)
            .ok_or_else(|| invalid("calculation revision overflow"))?;
        self.goodtype_mut().insert("revision".into(), json!(next));
        Ok(())
    }

    pub fn advance_revision(&mut self) -> Result<(), StorageError> {
        self.set_revision_after(self.revision())
    }

    /// Execution position for a new cell of `part_id` when that part has no cells: before the
    /// first cell of the next listed part, or at the end. The result is always a segment boundary.
    pub fn empty_part_position(&self, part_id: &str) -> Result<usize, StorageError> {
        let parts = self.parts()?;
        let listed = parts
            .iter()
            .position(|part| part.id == part_id)
            .ok_or_else(|| invalid("unknown calculation part"))?;
        let later = parts[listed + 1..]
            .iter()
            .map(|part| part.id.as_str())
            .collect::<HashSet<_>>();
        Ok(self
            .cells
            .iter()
            .position(|cell| cell.part_id().is_some_and(|part| later.contains(part)))
            .unwrap_or(self.cells.len()))
    }

    pub fn validate(&self) -> Result<(), StorageError> {
        if self.nbformat != 4
            || self.nbformat_minor != 5
            || !valid_id(self.id())
            || self.revision() == 0
        {
            return Err(invalid(
                "unsupported or invalid calculation identity/version",
            ));
        }
        if !self
            .goodtype()
            .and_then(|value| value.get("title"))
            .is_some_and(|title| {
                title
                    .as_str()
                    .is_some_and(|title| title.len() <= MAX_TITLE_BYTES)
            })
        {
            return Err(invalid("calculation title is invalid"));
        }
        let parts = self.parts()?;
        let mut part_ids = HashSet::new();
        if parts.is_empty()
            || parts.len() > MAX_CELLS
            || parts.iter().any(|part| {
                !valid_id(&part.id)
                    || part.title.len() > MAX_TITLE_BYTES
                    || !part_ids.insert(part.id.as_str())
            })
        {
            return Err(invalid(
                "calculation parts must have unique valid identities",
            ));
        }
        if self.cells.len() > MAX_CELLS {
            return Err(invalid("calculation has too many cells"));
        }
        let mut ids = HashSet::new();
        let mut closed_parts = HashSet::new();
        let mut current_part = None;
        for cell in &self.cells {
            if !valid_id(&cell.id)
                || !ids.insert(cell.id.as_str())
                || cell.source.len() > MAX_CELL_SOURCE_BYTES
                || !matches!(cell.cell_type.as_str(), "code" | "markdown" | "raw")
            {
                return Err(invalid("cell identity, type or source size is invalid"));
            }
            let part = cell
                .part_id()
                .ok_or_else(|| invalid("cell has no calculation part"))?;
            if !part_ids.contains(part) {
                return Err(invalid("cell references an unknown calculation part"));
            }
            if current_part != Some(part) {
                if closed_parts.contains(part) {
                    return Err(invalid("calculation parts must contain contiguous cells"));
                }
                if let Some(previous) = current_part {
                    closed_parts.insert(previous);
                }
                current_part = Some(part);
            }
            if cell.cell_type != "code"
                && (!cell.outputs.is_empty() || cell.execution_count.is_some())
            {
                return Err(invalid(
                    "only code cells can have execution counts or outputs",
                ));
            }
            validate_outputs(&cell.outputs)?;
            cell.validate_goodtype()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(value: Value) -> Result<Calculation, StorageError> {
        Calculation::parse(&serde_json::to_vec(&value).unwrap())
    }

    #[test]
    fn ordinary_nbformat_round_trip_preserves_source_and_unknown_metadata() {
        let mut calculation = Calculation::new("calculation-1", "part-1", "Task A");
        calculation.cells[0].source = "x = 2\nprint(x)".into();
        calculation.cells[0]
            .extra
            .insert("attachments".into(), json!({"original":"preserved"}));
        calculation
            .metadata
            .insert("other-tool".into(), json!({"value":3}));
        assert_eq!(
            Calculation::parse(&calculation.bytes().unwrap()).unwrap(),
            calculation
        );
        let mut value = serde_json::to_value(&calculation).unwrap();
        value["cells"][0]["source"] = json!(["x = 2\n", "print(x)"]);
        assert_eq!(
            parsed(value).unwrap().cells[0].source,
            calculation.cells[0].source
        );
    }

    #[test]
    fn serialization_is_ordinary_nbformat_for_code_and_markdown() {
        let mut calculation = Calculation::new("calculation-1", "part-1", "Task A");
        let mut markdown = Cell::new("text".into(), "part-1", "markdown");
        markdown.source = "# Title".into();
        calculation.cells.push(markdown);
        let value: Value = serde_json::from_slice(&calculation.bytes().unwrap()).unwrap();
        assert_eq!(
            (value["nbformat"].clone(), value["nbformat_minor"].clone()),
            (json!(4), json!(5))
        );
        let code = value["cells"][0].as_object().unwrap();
        assert!(code["execution_count"].is_null() && code["outputs"] == json!([]));
        let text = value["cells"][1].as_object().unwrap();
        assert!(!text.contains_key("execution_count") && !text.contains_key("outputs"));
        assert_eq!(text["source"], "# Title");
        assert_eq!(value["metadata"]["kernelspec"]["name"], "python3");
    }

    #[test]
    fn refuses_unknown_parts_duplicate_ids_future_formats_and_oversized_source() {
        let original = Calculation::new("calculation-1", "part-1", "Task A");
        let mut bad = original.clone();
        bad.cells.push(bad.cells[0].clone());
        assert!(bad.validate().is_err());
        let mut bad = original.clone();
        bad.cells[0].metadata["goodtype"]["partId"] = json!("unknown");
        assert!(bad.validate().is_err());
        let mut bad = original.clone();
        bad.nbformat = 5;
        assert!(bad.validate().is_err());
        let mut bad = original;
        bad.cells[0].source = "x".repeat(MAX_CELL_SOURCE_BYTES + 1);
        assert!(bad.validate().is_err());
    }

    #[test]
    fn refuses_malformed_untrusted_metadata_and_outputs_without_panicking() {
        let base = serde_json::to_value(Calculation::new("calc", "part", "Task")).unwrap();
        let mutations: Vec<fn(&mut Value)> = vec![
            |value| value["metadata"]["goodtype"] = json!("not an object"),
            |value| value["metadata"]["goodtype"]["title"] = json!(7),
            |value| value["metadata"]["goodtype"]["revision"] = json!(-1),
            |value| value["metadata"]["goodtype"]["parts"] = json!({"id":"part"}),
            |value| value["metadata"]["goodtype"]["parts"][0]["id"] = json!("../escape"),
            |value| value["cells"][0]["metadata"]["goodtype"] = json!([]),
            |value| value["cells"][0]["id"] = json!("bad id"),
            |value| value["cells"][0]["source"] = json!(["ok", 3]),
            |value| value["cells"][0]["execution_count"] = json!(1.5),
            |value| value["cells"][0]["outputs"] = json!([7]),
            |value| {
                value["cells"][0]["outputs"] =
                    json!([{"output_type":"stream","name":"stdin","text":"x"}])
            },
            |value| {
                value["cells"][0]["outputs"] =
                    json!([{"output_type":"stream","name":"stdout","text":[1]}])
            },
            |value| {
                value["cells"][0]["outputs"] = json!([{"output_type":"display_data","data":[]}])
            },
            |value| {
                value["cells"][0]["outputs"] =
                    json!([{"output_type":"display_data","data":{"image/png":7}}])
            },
            |value| {
                value["cells"][0]["outputs"] =
                    json!([{"output_type":"error","ename":"E","evalue":"v"}])
            },
            |value| value["cells"][0]["outputs"] = json!([{"output_type":"widget"}]),
            |value| {
                value["cells"][0]["metadata"]["goodtype"]["execution"] =
                    json!({"sourceHash":"short"})
            },
            |value| {
                value["cells"][0]["metadata"]["goodtype"]["lastSuccess"] =
                    json!({"outputs":[{"output_type":"x"}]})
            },
            |value| {
                value["cells"][0]["cell_type"] = json!("markdown");
                value["cells"][0]["outputs"] =
                    json!([{"output_type":"stream","name":"stdout","text":"x"}]);
            },
        ];
        for (index, mutate) in mutations.into_iter().enumerate() {
            let mut value = base.clone();
            mutate(&mut value);
            assert!(parsed(value).is_err(), "mutation {index} was accepted");
        }
        let mut accepted = base;
        accepted["cells"][0]["outputs"] = json!([
            {"output_type":"stream","name":"stdout","text":["a\n","b"]},
            {"output_type":"display_data","data":{"text/plain":"x","application/vnd.custom+json":{"k":[1]}},"metadata":{}},
            {"output_type":"execute_result","data":{"text/plain":["4"]},"metadata":{},"execution_count":null},
            {"output_type":"error","ename":"E","evalue":"v","traceback":["line"]}
        ]);
        parsed(accepted).unwrap();
    }

    #[test]
    fn empty_parts_receive_a_segment_boundary_in_part_order() {
        let mut calculation = Calculation::new("calc", "a", "A");
        calculation.goodtype_mut().insert(
            "parts".into(),
            json!([{"id":"a","title":"A"},{"id":"b","title":"B"},{"id":"c","title":"C"}]),
        );
        calculation.cells.push(Cell::new("c1".into(), "c", "code"));
        assert_eq!(calculation.empty_part_position("b").unwrap(), 1);
        calculation.cells.remove(1);
        assert_eq!(calculation.empty_part_position("b").unwrap(), 1);
        assert!(calculation.empty_part_position("missing").is_err());
    }
}
