# Milestone 17 — Use-case/component file attribution & File Explorer

## Goal

Two related capabilities for application detail:

1. **File attribution** — the analyzer records which repository file(s) each use
   case and component affects.
2. **File Explorer tab** — a new tab on the application detail page showing a file
   tree of the cloned main-branch checkout on the left and a syntax-highlighted
   **CodeMirror** viewer on the right, with cross-links from an entity to its
   files.

## Scope

- Add a `files` list to use cases and components in the analysis schema, prompt,
  model, persistence, and read/UI.
- A bounded, sandboxed read API over the application's cloned checkout (tree +
  file content).
- A File Explorer tab with a lazy file tree and a vendored CodeMirror viewer.

## Deliverables

### File attribution

- **Prompt** (`analyzer.rs` `SYSTEM_PROMPT`): for each `use_case` and each
  `component`, return a `files: [string]` array of repo-relative paths it
  affects/implements.
- **Schema/model** (`platform/analysis.rs`): add `files: Vec<String>` to the use
  case and component shapes; validate/normalise to repo-relative paths.
- **Persistence**: store files in small child tables recreated per sync
  (CASCADE with their owner, consistent with the existing sub-entity pattern):

```sql
-- migrate:up
CREATE TABLE component_files (
    component_id UUID NOT NULL REFERENCES components(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,
    PRIMARY KEY (component_id, path)
);
CREATE TABLE use_case_files (
    use_case_id UUID NOT NULL REFERENCES use_cases(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    PRIMARY KEY (use_case_id, path)
);
-- migrate:down
```

- **Writer**: upsert the file paths alongside the components/use cases in the
  existing delete-and-recreate path.
- **Read/UI**: show each entity's file list; a path links into the File Explorer
  focused on that file.

### File read API (sandboxed to the checkout)

Resolve the checkout path from `applications.repository_id` → the cloned
`repositories` local path (recorded by `mark_cloned`; produced by the M13 per-job
workspace). All access goes through the `FileSystem` trait.

- `GET /platform/applications/{id}/files` → the file tree (directory + file
  **names only**, no contents). Skip `.git` and common vendored/build dirs; cap
  depth and entry count and **surface truncation** (consistent with the graph's
  truncation philosophy — never silently).
- `GET /platform/applications/{id}/files/content?path=…` → the file's text,
  size-capped, with a clear "too large / binary" response for non-text files.
- **Path safety**: a shared `safe_join(root, user_path)` helper rejects absolute
  paths and `..` escapes so a request can never read outside the checkout. Reuse
  it everywhere a user-supplied path meets the filesystem; unit-test the escapes.
- Requires the application's repository to have been cloned; return a clear "not
  cloned yet" state otherwise.

### File Explorer tab

- **Vendor CodeMirror locally** into `assets/vendor/` (downloaded, no CDN — same
  rule as the existing `g6`/`mermaid`/`jquery` assets), with language modes for
  common syntaxes; document the refresh procedure.
- A new **"File Explorer"** tab in `assets/platform-app-detail.js` +
  `templates/platform_app_detail.html`:
  - Left: a collapsible file tree (jQuery) that lazy-loads directory children.
  - Right: a **read-only** CodeMirror pane with syntax highlighting selected by
    file extension; loads file content on demand.
- **Cross-link**: from a use case's / component's file list, open the Explorer
  focused on the given path.

## Tasks

- [ ] Add `files` to the use-case/component prompt, schema, and model.
- [ ] `component_files` / `use_case_files` migrations + writer upserts.
- [ ] File tree + file content endpoints over `FileSystem`; `safe_join` helper.
- [ ] Vendor CodeMirror into `assets/vendor/`; document refresh.
- [ ] File Explorer tab: lazy tree + CodeMirror viewer; entity → file cross-link.
- [ ] Unit tests (mocked `FileSystem`): tree building + truncation; `safe_join`
      blocks `..`/absolute escapes; binary/oversize handling; writer file upserts.

## Acceptance criteria

- The File Explorer tab renders the checkout's tree from local assets, opens
  files with correct syntax highlighting, blocks path-traversal, and handles
  large/binary files gracefully.
- Use cases and components display their affected files; clicking a path opens it
  in the Explorer.
- Tree building, path safety, and writer file upserts are unit-tested with mocked
  dependencies.

## Dependencies

Milestones 07 (cloning), 08 (analysis + sub-entities), 09 (application detail),
13 (per-job workspace / clone location).

## Out of scope

Editing files in the browser and any git-history / blame view (read-only viewer).
