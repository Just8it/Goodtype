# Goodtype MCP relay

`goodtype-mcp.exe` connects an assistant's stdio to the running Goodtype app. It does not implement MCP tools; the app does. Windows only.

## Security model

- Discovery is local: `%LOCALAPPDATA%\Goodtype\helper\instance.json`.
- The relay checks the app PID, creation FILETIME and named-pipe server PID before sending credentials.
- The instance-bound handshake authenticates with a per-launch token; no requests are replayed.
- Goodtype owns current-user ACLs, session approval, grants and all notebook access; the relay only pumps bounded bytes.
- Same-user malware can read the token: this does not protect against that threat or control the assistant's onward disclosures.

## Codex setup

Enable Settings → AI helpers in Goodtype, then register the installed executable:

```powershell
codex mcp add goodtype -- "C:\Users\<user>\AppData\Local\Goodtype\bin\goodtype-mcp.exe"
```

General form: `codex mcp add goodtype -- "<path to goodtype-mcp.exe>"`.
Verified against [official MCP documentation](https://developers.openai.com/codex/mcp), which redirects to [MCP for CLI](https://learn.chatgpt.com/docs/extend/mcp?surface=cli). Keep Goodtype running when reconnecting.

Copy the bundled `skill/goodtype` folder to `~/.agents/skills/goodtype` (Windows: `%USERPROFILE%\.agents\skills\goodtype`). The resulting file is `~/.agents/skills/goodtype/SKILL.md`. The current [official skill documentation](https://developers.openai.com/codex/skills), redirecting to [Build skills](https://learn.chatgpt.com/docs/build-skills), specifies YAML frontmatter with `name` and `description`. The source comment follows the frontmatter so it remains parseable. The current documented user folder is `.agents/skills`, rather than the older `.codex/skills` example. Invoke `$goodtype` in Codex.

Step 1 reads granted context and compiles Typst; it cannot insert proposals. The assistant returns Typst for you to paste. Compiling validates syntax, not mathematics. Typst 0.15.1 was found locally; Lilaq 0.6.0 was not in the default package cache, so its recipe was not compile-verified and no download was forced.

## Exit codes

- `0`: stdin EOF or pipe close.
- `2`: unavailable app, invalid/stale discovery, identity or handshake refusal, unsupported platform, or other I/O failure. Details go to stderr.
- `3`: more than 16 MiB without a newline in either pump direction.

Stdout contains only post-handshake bytes. Handshake lines are at most 4096 bytes including their newline. The pumps preserve partial lines and arbitrary bytes. The first completed direction ends the process; it does not wait for the other thread blocked on stdin or the pipe.

## Windows duplex I/O

The client opens the pipe with `FILE_FLAG_OVERLAPPED`. Reader and writer halves share one
`Arc<OwnedHandle>`; each operation owns a separate manual-reset event and `OVERLAPPED`,
waiting for completion before releasing its buffer. `Write::write_all` retries partial writes.
A closed pipe is EOF for reads and `BrokenPipe` for writes. Process exit on either pump's
completion closes the connection, even when the other thread is blocked on stdin.

`real_named_pipe_reader_pending_two_requests` runs a real pipe exchange in a subprocess with
a 10-second timeout. It confirms `ReadFile` returned `ERROR_IO_PENDING` before writing two
requests; the server receives both before sending either reply. It also checks read EOF and
write `BrokenPipe`. Restoring the original synchronous client reproduces the timeout (2026-11-03).
