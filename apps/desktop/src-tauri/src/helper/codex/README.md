# In-app Codex (experimental, step 3)

The app-server process is the user's installed `codex-cli 0.166.0`. No credentials are loaded by
Goodtype. Prompt submission happens only after the ChatGPT account check and strict config/MCP
preflight. A failure destroys the process tree and revokes the launch/session grants.

Connection recovery (2026-11-09, user authorized): local RPC responses allow 90 seconds.
Only Codex's explicit `-32603 / workspace routing discovery timed out` account error
is retried, up to five times 15 seconds apart before thread/model creation. Counts
are emitted as localized run status; Stop/End cancels within the 100 ms wait poll.
After exhaustion, provider-unreachable copy offers manual Retry. Model turns and
permission failures are never automatically repeated. Launch-token issuance follows
successful account startup, preserving its existing one-minute expiry and revocation.

`client.rs` owns bounded JSONL, request routing, decline responses and the hidden process/Job
Object. `config.rs` owns per-launch overrides and the strict comparison. `mod.rs` owns discovery,
conversations, run events and commands. The bridge's session share accessor separates terminal
and in-app authority. In-app scope expansion is denied.

## Verification on 2026-11-03

Installed-binary probes (opt-in, no model turns):

```
cargo test -p goodtype-desktop codex_real -- --ignored --nocapture
cargo test -p goodtype-desktop codex_real_thread_inventory -- --ignored --nocapture
cargo test -p goodtype-desktop codex_real_launch_preflight -- --ignored --nocapture
```

The initial account routing timeout was transient; the coordinator subsequently passed the
config read-back probe. The developer then launched the GUI and found two real preflight bugs:

- A thread inventory reports disabled servers with `runtimeStatus="disabled"`, not null. The
  checker now accepts null/disabled/notStarted only with no tools or initialized metadata.
- On Codex 0.166, bulk MCP inventory opens a second relay connection despite `threadId`.
  That second connection fails the single-use launch token. Named `serverName` + `threadId`
  discovery reuses the thread connection. `Client::thread_inventory` queries every configured
  server by name and rejects missing, mismatched or paginated results before combined preflight.

Both failures were reproduced. The final isolated launch probe passes against the real installed
Codex and real relay: a current-user pipe, temporary discovery directory, auto-approved single-use
launch token, all configured servers checked, and no model turn or notebook reads. Default tests
cover inactive states, rejection of active/unknown states, and named inventory routing/completeness.
This certifies startup/preflight, not filesystem confinement or a successful model proposal.

The developer's next turn reached the model but Codex refused `status` and `get_context` because
the MCP tools had the default approval policy while the run uses `approval_policy="never"`.
Per-launch overrides now pre-approve the built-in Goodtype tools and preflight verifies their effective
policies. Rust still enforces the user-granted selection and proposal review. External server policies
are unchanged. See the [official MCP approval configuration](https://learn.chatgpt.com/docs/extend/mcp).

An earlier launch rejected `mcp_servers."github"` with `Error: invalid transport`. The CLI's
`-c` parser does not interpret the quotes as TOML key quoting. Ordinary alphanumeric/underscore/
hyphen names now use unquoted keys. Unusual names remain fail-closed rather than being rewritten
into a different server name. Discovery observed `github`, `konnect`, `node_repl`, `stitch`.

## Integration limits for the coordinator

### MCP images in JavaScript tool hosts (2026-11-06)

The real page-7 task exposed a presentation failure, not a missing PNG or an unreadable
document. The assistant called `get_context` and used `text(result)`, then called `render`
and printed a truncated `JSON.stringify(result)`. Both operations display encoded image
data as text; they do not forward an MCP image to the model's image viewer.

The injected Goodtype skill now includes the explicit content-block forwarding pattern:
`if (block.type === "image") image(block)` and text-only forwarding for text blocks.
The image-producing tool descriptions and every initial/follow-up `turn/start` also remind
the assistant of that rule. `features.view_image=false` remains deliberate: it disables
unrestricted local-file viewing, not vision over user-granted MCP image content. No files,
new notebook context, model change, image-policy override or additional permission is needed.

Focused regressions cover injected guidance, tool descriptions and both initial/follow-up
turn payloads. Existing bridge tests verify real MCP image content and crop/Low boundaries.
This is a guidance fix for an observed assistant mistake, not proof that every future model
turn will comply; a fresh native assistant retry remains the usefulness acceptance check.

Stable page IDs do not encode display order. The skill explicitly tells the assistant not
to infer page numbers from IDs. The optional disclosed names/order scope lets `list_pages`
return bounded titles and page kinds without granting access to their contents.

`propose_page` now supports a reviewed blank page, Page text or unexecuted Python cells.
The model chooses a before/after/end location, respecting any user-specified location;
the review lets the user change that choice before creating the page. A Python draft
with `targetPageId` instead appends self-contained cells to an existing Python page.
Rust captures its exact revision/hash privately without revealing ungranted code.
Existing-cell edits still require a cell grant, which can now accompany PDF/ordinary
reference pages. Creation and append use existing transactional storage/history and
never execute code. See local ADR 0044 and the page-proposal regression tests.

A native Create page retry exposed a request-lifetime mismatch: new-page review
required the exact old request epoch, so a normal follow-up could leave a visible
proposal that returned "proposal session ended". Published page reviews now retain
their captured inputs across a same-notebook follow-up, matching block review.
New model reads still use only current grants. Explicit revocation/Stop/End and
stale sources remain refused. A regression covers edit/apply after follow-up and
refusal after revoke. Missing/revoked page proposals report staleness for recovery;
unexpected storage failures expose their diagnostic in the review's details.

### Transparent PDF paper (2026-11-06)

A second real page-7 retry correctly forwarded its image, but the PNG had transparent
paper. On a black backdrop the black exercise text disappeared, leaving the embedded
logo and aircraft photos. Compositing that exact captured PNG over white revealed the
complete statement and data. This was a preview background bug, not missing PDF text.
The granted PNG renderer now supplies white paper when the document has no fill;
explicit coloured paper remains intact. The original PDF and vector PDF export are
unchanged. A regression renders a transparent PDF with black text at full-page and
crop sizes, checking opaque white paper and visible dark text. Live model success
still requires a fresh request after the rebuild; old images cannot be recalled.

- A conversation currently fixes its model and web setting at launch. Changing either in a
  follow-up returns `confinement_failed`; close it and start another conversation.
- The launch-token probe creates an isolated bridge and never replaces the running app's discovery
  record. The developer confirmed the real translation/proposal flow. Live Stop, revision, undo/redo
  and the latest selection/end controls still need acceptance beyond browser fixtures.
- Stderr is drained with a fixed logging budget; logs contain byte counts rather than raw text
  to keep host/provider diagnostics from exposing account data.
- This is partial confinement only: the read-only sandbox does not prove filesystem read
  isolation, and the inherited-host-skill/plugin issues in the research remain open.
- No canonical format, new crate, new Cargo feature or frontend capability was added.
- The coordinator owns `AGENT_DOC.md` and architecture/decision/status documents; the step 3
  handoff records current acceptance evidence and remaining packaging/hardening probes.
Final Rust checks after the close/admission race guard: desktop 126 passed / 7 ignored;
relay 12 passed; workspace Rust total 286 passed / 7 ignored. The Clippy and format gates
passed. The default verification gate intentionally skips the opt-in real-binary probe above.
All 12 owned files passed UTF-8 decoding with no BOM and no replacement characters.
The single session-aware proposal lookup outside the original R3 list was explicitly approved.


## Goodtype model default

Missing, null or blank model preferences use Goodtype's `gpt-6-luna` default.
A nonblank per-run model takes precedence over a nonblank saved model; explicit
saved Terra/Sol choices are retained. No inherited Codex model is used when both
are absent. The existing installed app-server `model/list` inventory must contain
the selected ID; otherwise the run returns `model_unavailable` with selection
recovery guidance, without substituting another model. Refresh status to refresh
that inventory. Inventory presence does not guarantee provider entitlement.

Every `turn/start`, initial or follow-up, explicitly sends the resolved `model`
and `effort: "medium"`. These fields are supported by the installed Codex 0.166.0
app-server generated `v2/TurnStartParams` protocol. No global Codex config is edited.
