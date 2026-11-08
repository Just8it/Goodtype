use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::collections::BTreeSet;
pub const HARDENING: &[(&str, bool)] = &[
    ("shell_tool", false),
    ("view_image", false),
    ("hooks", false),
    ("plugins", false),
    ("apps", false),
    ("remote_plugin", false),
    ("tool_suggest", false),
    ("skip_host_skill_discovery", true),
];
const GOODTYPE_TOOLS: &[&str] = &[
    "status",
    "get_context",
    "list_pages",
    "read_page",
    "render",
    "transcribe",
    "search",
    "request_scope",
    "typst_compile",
    "propose_block_edit",
    "propose_cell_edit",
    "propose_cell_insert",
    "propose_page_text_edit",
    "propose_blocks",
    "propose_page",
    "proposal_status",
    "presets_list",
    "preset_read",
    "preset_preview",
    "propose_preset",
];
fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization")
}
pub fn overrides(
    relay: &str,
    token: &str,
    names: &[String],
    allowed: &[String],
    web: bool,
) -> Vec<String> {
    let mut values = HARDENING
        .iter()
        .map(|(key, value)| format!("features.{key}={value}"))
        .collect::<Vec<_>>();
    values.extend([
        "skills.bundled.enabled=false".into(),
        "skills.include_instructions=false".into(),
        "sandbox_mode=\"read-only\"".into(),
        "approval_policy=\"never\"".into(),
        format!("web_search=\"{}\"", if web { "live" } else { "disabled" }),
    ]);
    for name in names.iter().filter(|n| n.as_str() != "goodtype") {
        values.push(format!(
            "mcp_servers.{}.enabled={}",
            if name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                name.clone()
            } else {
                quoted(name)
            },
            allowed.contains(name)
        ));
    }
    values.extend([
        format!("mcp_servers.goodtype.command={}", quoted(relay)),
        format!(
            "mcp_servers.goodtype.args=[\"--launch-token\",{}]",
            quoted(token)
        ),
        "mcp_servers.goodtype.enabled=true".into(),
        "mcp_servers.goodtype.required=true".into(),
        // The user already granted this run's selection. Rust still enforces every grant and
        // proposal review; Codex's approval_policy=never otherwise rejects these MCP calls.
        "mcp_servers.goodtype.default_tools_approval_mode=\"approve\"".into(),
    ]);
    for tool in GOODTYPE_TOOLS {
        values.push(format!(
            "mcp_servers.goodtype.tools.{tool}.approval_mode=\"approve\""
        ));
    }
    values
}
pub fn names(value: &Value) -> Vec<String> {
    fn collect(value: &Value, names: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                if let Some(servers) = map.get("mcp_servers").and_then(Value::as_object) {
                    names.extend(servers.keys().filter(|n| n.as_str() != "goodtype").cloned());
                }
                for v in map.values() {
                    collect(v, names);
                }
            }
            Value::Array(a) => {
                for v in a {
                    collect(v, names)
                }
            }
            _ => {}
        }
    }
    let mut names = BTreeSet::new();
    collect(value, &mut names);
    names.into_iter().collect()
}
/// Fail closed unless Codex's own read-back matches what Goodtype asked for (ADR 0041, step 3).
///
/// Codex lists every configured MCP server, including the ones Goodtype disabled for this launch.
/// So the check is not "only goodtype is listed" but: every server Goodtype did not select is
/// disabled in the effective config and inactive (no tools, never started), and the goodtype
/// server is ready.
pub fn preflight(
    config: &Value,
    inventory: &Value,
    allowed: &[String],
    web: bool,
) -> Result<(), String> {
    check_config(config, allowed, web)?;
    check_inventory(inventory, allowed, true)
}

fn selected(allowed: &[String]) -> BTreeSet<String> {
    std::iter::once("goodtype".to_owned())
        .chain(allowed.iter().cloned())
        .collect()
}

/// The effective configuration: hardening keys, sandbox, approvals, web search, and the enabled
/// flag of every configured MCP server.
pub fn check_config(config: &Value, allowed: &[String], web: bool) -> Result<(), String> {
    let c = &config["config"];
    for (key, expected) in HARDENING {
        if c["features"][key].as_bool() != Some(*expected) {
            return Err(format!("features.{key} did not read back as {expected}"));
        }
    }
    if c["skills"]["bundled"]["enabled"].as_bool() != Some(false) {
        return Err("skills.bundled not disabled".into());
    }
    if c["skills"]["include_instructions"].as_bool() != Some(false) {
        return Err("skills.include_instructions not disabled".into());
    }
    if c["sandbox_mode"] != "read-only"
        || c["approval_policy"] != "never"
        || c["web_search"] != if web { "live" } else { "disabled" }
    {
        return Err("sandbox, approval or web setting mismatch".into());
    }
    let wanted = selected(allowed);
    let servers = c["mcp_servers"]
        .as_object()
        .ok_or("missing MCP server configuration")?;
    for (name, server) in servers {
        // An entry without an explicit flag is enabled, as Codex treats it.
        let enabled = server["enabled"].as_bool().unwrap_or(true);
        if enabled != wanted.contains(name) {
            return Err(format!(
                "MCP server {name} enabled={enabled} does not match Goodtype's selection"
            ));
        }
    }
    if let Some(missing) = wanted.iter().find(|name| !servers.contains_key(*name)) {
        return Err(format!("MCP server {missing} is not configured"));
    }
    let goodtype = &servers["goodtype"];
    if goodtype["default_tools_approval_mode"] != "approve"
        || GOODTYPE_TOOLS
            .iter()
            .any(|tool| goodtype["tools"][tool]["approval_mode"] != "approve")
    {
        return Err("Goodtype tool approval policy did not read back as approved".into());
    }
    Ok(())
}

/// The live MCP inventory for the thread. Servers Goodtype did not select must expose no tools
/// and must not have started. With `require_goodtype`, the goodtype server must be ready.
pub fn check_inventory(
    inventory: &Value,
    allowed: &[String],
    require_goodtype: bool,
) -> Result<(), String> {
    if !inventory["nextCursor"].is_null() {
        return Err("MCP inventory was truncated".into());
    }
    let data = inventory["data"]
        .as_array()
        .ok_or("missing MCP inventory")?;
    let wanted = selected(allowed);
    let mut goodtype_ready = false;
    let mut goodtype_detail = "server not listed".to_owned();
    for server in data {
        let name = server["name"].as_str().unwrap_or("");
        let has_tools = server["tools"].as_object().is_some_and(|t| !t.is_empty());
        // Thread inventories explicitly report disabled/notStarted; both mean no connection.
        let inactive = server["runtimeStatus"].is_null()
            || matches!(
                server["runtimeStatus"].as_str(),
                Some("disabled" | "notStarted")
            );
        let started = !server["serverInfo"].is_null() || !inactive;
        if !wanted.contains(name) {
            if has_tools || started {
                return Err(format!("MCP server {name} is active but not allowed"));
            }
        } else if name == "goodtype" {
            goodtype_ready = has_tools && server.get("toolsError").is_none_or(Value::is_null);
            goodtype_detail = format!(
                "state={}; tools={}; discovery={}",
                server["runtimeStatus"].as_str().unwrap_or("unknown"),
                server["tools"].as_object().map_or(0, |tools| tools.len()),
                server["toolsError"]
                    .as_str()
                    .unwrap_or("no discovery error")
            );
        }
    }
    if require_goodtype && !goodtype_ready {
        return Err(format!(
            "MCP server goodtype is not ready ({goodtype_detail})"
        ));
    }
    Ok(())
}
#[cfg(test)]
pub fn evidence(config: &Value) -> Value {
    let c = &config["config"];
    json!({"features":c["features"],"skills":c["skills"],"sandbox_mode":c["sandbox_mode"],"approval_policy":c["approval_policy"],"web_search":c["web_search"]})
}
pub fn instructions() -> String {
    let skill = include_str!("../../../../../../crates/goodtype-mcp/skill/goodtype/SKILL.md");
    let body = skill
        .strip_prefix("---")
        .and_then(|s| s.find("\n---").map(|i| &s[i + 4..]))
        .unwrap_or(skill);
    format!(
        "In-app Goodtype mode: use Goodtype tools and create proposals; do not answer with paste-able text. The panel shows your replies as plain text: keep them short, never use LaTeX delimiters such as \\( \\) or $...$, and write formulas readably with Unicode (C_L = L / (q·S), ρ = 0.905 kg/m³). Reply in the language the user writes in.\n{body}"
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_cover_servers_and_web() {
        let a = overrides(
            "C:\\relay.exe",
            "token",
            &["hostile.name".into(), "allowed".into()],
            &["allowed".into()],
            false,
        );
        for (k, v) in HARDENING {
            assert!(a.contains(&format!("features.{k}={v}")));
        }
        assert!(a.contains(&"mcp_servers.\"hostile.name\".enabled=false".into()));
        assert!(a.contains(&"mcp_servers.allowed.enabled=true".into()));
        assert!(
            a.contains(&"mcp_servers.goodtype.tools.get_context.approval_mode=\"approve\"".into())
        );
        assert!(
            !a.iter()
                .any(|v| v.starts_with("mcp_servers.allowed.default_tools_approval_mode"))
        );
        assert!(a.contains(&"web_search=\"disabled\"".into()));
        assert!(overrides("r", "t", &[], &[], true).contains(&"web_search=\"live\"".into()));
    }
    #[test]
    fn preflight_fails_closed() {
        let mut c = json!({"config":{"features":{},"skills":{"bundled":{"enabled":false},"include_instructions":false},"sandbox_mode":"read-only","approval_policy":"never","web_search":"live","mcp_servers":{"goodtype":{"enabled":true},"github":{"enabled":false},"solidworks":{"enabled":true}}}});
        for (k, v) in HARDENING {
            c["config"]["features"][k] = json!(v);
        }
        c["config"]["mcp_servers"]["goodtype"]["default_tools_approval_mode"] = json!("approve");
        for tool in GOODTYPE_TOOLS {
            c["config"]["mcp_servers"]["goodtype"]["tools"][tool]["approval_mode"] =
                json!("approve");
        }
        let allowed = ["solidworks".to_owned()];
        // Disabled servers stay listed by Codex, without tools and never started.
        let i = json!({"data":[
            {"name":"goodtype","tools":{"status":{}},"serverInfo":{},"runtimeStatus":"ready","toolsError":null},
            {"name":"github","tools":{},"serverInfo":null,"runtimeStatus":null},
            {"name":"solidworks","tools":{"export":{}},"serverInfo":{},"runtimeStatus":"ready"}
        ],"nextCursor":null});
        assert!(preflight(&c, &i, &allowed, true).is_ok());
        let mut blocked = c.clone();
        blocked["config"]["mcp_servers"]["goodtype"]["tools"]["get_context"]["approval_mode"] =
            json!("prompt");
        assert!(preflight(&blocked, &i, &allowed, true).is_err());
        for (k, _) in HARDENING {
            let mut bad = c.clone();
            bad["config"]["features"]
                .as_object_mut()
                .unwrap()
                .remove(*k);
            assert!(preflight(&bad, &i, &allowed, true).is_err());
        }
        // A server Goodtype did not select that is enabled, or active, fails closed.
        let mut enabled = c.clone();
        enabled["config"]["mcp_servers"]["github"]["enabled"] = json!(true);
        assert!(preflight(&enabled, &i, &allowed, true).is_err());
        let mut active = i.clone();
        active["data"][1]["tools"] = json!({"create_issue":{}});
        assert!(preflight(&c, &active, &allowed, true).is_err());
        // An allowed server that the user did not opt in to this time is refused.
        assert!(preflight(&c, &i, &[], true).is_err());
        // goodtype must be ready, the web setting must match, and the list must be complete.
        let mut not_ready = i.clone();
        not_ready["data"][0]["tools"] = json!({});
        assert!(preflight(&c, &not_ready, &allowed, true).is_err());
        assert!(check_inventory(&not_ready, &allowed, false).is_ok());
        assert!(preflight(&c, &i, &allowed, false).is_err());
        assert!(preflight(&c, &json!({"data":[],"nextCursor":"more"}), &allowed, true).is_err());
    }
    #[test]
    fn thread_inventory_accepts_only_inactive_unselected_servers() {
        for state in [Value::Null, json!("disabled"), json!("notStarted")] {
            let mut inventory = json!({"data":[{"name":"github","tools":{},"serverInfo":null,"runtimeStatus":state}],"nextCursor":null});
            assert!(check_inventory(&inventory, &[], false).is_ok());
            inventory["data"][0]["tools"] = json!({"write":{}});
            assert!(check_inventory(&inventory, &[], false).is_err());
            inventory["data"][0]["tools"] = json!({});
            inventory["data"][0]["serverInfo"] = json!({"name":"github"});
            assert!(check_inventory(&inventory, &[], false).is_err());
        }
        for state in [
            "starting",
            "connected",
            "authenticationRequired",
            "failed",
            "cancelled",
            "unknown",
        ] {
            let inventory = json!({"data":[{"name":"github","tools":{},"runtimeStatus":state}],"nextCursor":null});
            assert!(check_inventory(&inventory, &[], false).is_err(), "{state}");
        }
    }
    #[test]
    fn catalog_matches_preapproved_tools_and_page_text_policy_is_required() {
        let mut catalog: Vec<String> = crate::helper::bridge::catalog()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect();
        let mut approved: Vec<String> = GOODTYPE_TOOLS.iter().map(|t| (*t).to_owned()).collect();
        catalog.sort();
        approved.sort();
        assert_eq!(catalog, approved);
        let preset = crate::helper::bridge::catalog()
            .into_iter()
            .find(|tool| tool.name == "propose_preset")
            .unwrap();
        assert_eq!(
            preset.input_schema["required"],
            json!(["source", "name", "candidateHash"])
        );
        assert!(!catalog.iter().any(|name| name.contains("install")));
        let page_text = crate::helper::bridge::catalog()
            .into_iter()
            .find(|tool| tool.name == "propose_page_text_edit")
            .unwrap();
        assert_eq!(
            page_text.input_schema["required"],
            json!(["page", "pageTextId", "proposalBase", "source", "summary"])
        );
        assert!(overrides("r", "t", &[], &[], false).contains(
            &"mcp_servers.goodtype.tools.propose_page_text_edit.approval_mode=\"approve\"".into()
        ));
        let mut c = json!({"config":{"features":{},"skills":{"bundled":{"enabled":false},"include_instructions":false},"sandbox_mode":"read-only","approval_policy":"never","web_search":"disabled","mcp_servers":{"goodtype":{"enabled":true,"default_tools_approval_mode":"approve"}}}});
        for (k, v) in HARDENING {
            c["config"]["features"][k] = json!(v);
        }
        for tool in GOODTYPE_TOOLS {
            c["config"]["mcp_servers"]["goodtype"]["tools"][tool]["approval_mode"] =
                json!("approve");
        }
        let i = json!({"data":[{"name":"goodtype","tools":{"status":{}},"serverInfo":{},"runtimeStatus":"ready"}],"nextCursor":null});
        assert!(preflight(&c, &i, &[], false).is_ok());
        c["config"]["mcp_servers"]["goodtype"]["tools"]
            .as_object_mut()
            .unwrap()
            .remove("propose_preset");
        assert!(preflight(&c, &i, &[], false).is_err());
    }
    #[test]
    fn config_layer_names() {
        assert_eq!(
            names(&json!({"layers":[{"config":{"mcp_servers":{"one":{},"two":{}}}}]})),
            vec!["one", "two"]
        );
        assert!(!instructions().contains("name: goodtype"));
    }
    #[test]
    fn instructions_explain_how_to_display_mcp_images_without_widening_grants() {
        let guidance = instructions();
        assert!(guidance.contains("if (block.type === \"image\") image(block)"));
        assert!(guidance.contains("else if (block.type === \"text\") text(block.text)"));
        assert!(guidance.contains("Printing the tool result"));
        assert!(guidance.contains("Under Low usage"));
        assert!(guidance.contains("stable IDs"));
        for tool in ["get_context", "render", "typst_compile"] {
            let catalog = super::super::super::bridge::catalog();
            let entry = catalog.iter().find(|entry| entry.name == tool).unwrap();
            assert!(entry.description.as_ref().unwrap().contains("image(block)"));
        }
    }
}
