# TaskCraft - Tiered Execution Plan

**Parent Standard:** [`RUST_ARTCRAFT_STANDARD.md`](../../mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md)  
**Menu Bar Standard:** [`CRAFT_MENU_BAR_STANDARD.md`](../../mcp-central-docs/standards/rust-artcraft/CRAFT_MENU_BAR_STANDARD.md)  
**Architecture Model:** 4-Tier ArtCraft Incremental Delivery  
**Status:** Active  

---

## Architecture Flow

```
Tier 1: Core Engine ──> Tier 2: MCP / CLI ──> Tier 3: Desktop UI ──> Tier 4: Menu Bar, Help & Packaging
(taskcraft-core)       (taskcraft-mcp)       (taskcraft-app)       (Hardened Workstation)
```

---

## Tier Specifications

### Tier 1 - Core Issue & Blocker Engine (`taskcraft-core`)
- [x] L1 `Command::dispatch` universal issue tracking execution bus.
- [x] SQLite WAL database schema for projects, issues (`KEY-n`), dependencies, and milestones.
- [x] Blocker DAG cycle detection algorithm.
- [x] Per-issue history tracking (agent, user, git, bus).
- [x] Read-only `git log` synchronization and mention closing keywords.
- [x] Ingest cross-app bus events (`ticket.escalated`, `contract.deadline`, `capacity.changed`).
- [x] `#![forbid(unsafe_code)]` and zero-panic clippy lints.

### Tier 2 - Headless Driver & FastMCP (`taskcraft-mcp`)
- [x] Unified CLI `taskcraft-cli` with `--json` output.
- [x] FastMCP stdio server communicating over JSON-RPC 2.0 (17 tools).
- [x] Issue export and milestone progression telemetry.
- [x] Headless `--smoke` verification without window initialization.

### Tier 3 - Desktop Workstation UI (`taskcraft-app`)
- [x] Immediate-mode `egui` 0.37 / `wgpu` workstation (`craft-biz-ui`).
- [x] Drag-and-drop Kanban board with workflow rule enforcement.
- [x] Swimlanes (priority, assignee, none), quick filters, and issue table view.
- [x] Interactive inspector drawer with in-place editing and blocker management.

### Tier 4 - Craft Menu Bar Standard, Help Suite & Hardened Verification
- [ ] Universal 8-menu bar (`File`, `Edit`, `View`, `Insert`, `Modify`, `Commands`, `Window`, `Help`) with `switch_on_hover`.
- [ ] Mandatory 5-dialog Help suite (`Command Palette...`, `Keyboard Shortcuts...`, `Project Files & Docs...`, `Licenses & Notices...`, `About TaskCraft`).
- [ ] Fleet UI/UX Pattern 2 adherence: viewport-centric navigation, top button row mapped to View toggles, panel visibility managed via `View` / `Window`.
- [ ] Tofu Guard verification (100% font glyph coverage, zero tofu placeholder boxes).
- [ ] Headless `--smoke` parity validation.
- [ ] Standalone release binary packaging and NSIS installer.
