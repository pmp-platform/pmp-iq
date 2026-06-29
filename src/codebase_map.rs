//! Auto-generated codebase map (M28): a directory/module structure graph for an
//! application, derived on demand from its cloned checkout via the sandboxed
//! `FileBrowser` (M17). Nodes are directories; edges are containment. Bounded by
//! depth and node count, with truncation surfaced (never silently).

use crate::files::FileBrowser;
use serde_json::{Value, json};

const MAX_DEPTH: usize = 4;
const MAX_NODES: usize = 300;

fn node(id: &str, label: &str) -> Value {
    json!({ "id": id, "data": { "label": label, "kind": "directory" } })
}

/// Accumulates the graph while walking the directory tree.
struct Walker<'a> {
    browser: &'a FileBrowser,
    root: &'a str,
    nodes: Vec<Value>,
    edges: Vec<Value>,
    count: usize,
    truncated: bool,
}

impl Walker<'_> {
    fn walk(&mut self, rel: &str, depth: usize) {
        if depth >= MAX_DEPTH {
            self.truncated = true;
            return;
        }
        let parent_id = if rel.is_empty() { ".".to_string() } else { rel.to_string() };
        let Ok(entries) = self.browser.list(self.root, rel) else {
            return;
        };
        for entry in entries.into_iter().filter(|e| e.is_dir) {
            if self.count >= MAX_NODES {
                self.truncated = true;
                return;
            }
            let child = if rel.is_empty() {
                entry.name.clone()
            } else {
                format!("{rel}/{}", entry.name)
            };
            self.nodes.push(node(&child, &entry.name));
            self.edges.push(json!({ "source": parent_id, "target": child }));
            self.count += 1;
            self.walk(&child, depth + 1);
        }
    }
}

/// Build the codebase map (`{ nodes, edges, truncated }`) for a checkout `root`.
pub fn build_map(browser: &FileBrowser, root: &str) -> Value {
    let mut w = Walker {
        browser,
        root,
        nodes: vec![node(".", "(repository)")],
        edges: Vec::new(),
        count: 1,
        truncated: false,
    };
    w.walk("", 0);
    json!({ "nodes": w.nodes, "edges": w.edges, "truncated": w.truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MockFileSystem;
    use std::sync::Arc;

    #[test]
    fn builds_a_directory_graph() {
        let mut fs = MockFileSystem::new();
        // /repo has src + tests; src has one nested dir; deeper is empty.
        fs.expect_list_subdirs().returning(|p| {
            if p == "/repo" {
                Ok(vec!["src".into(), "tests".into()])
            } else if p == "/repo/src" {
                Ok(vec!["api".into()])
            } else {
                Ok(vec![])
            }
        });
        fs.expect_list_files().returning(|_| Ok(vec![]));
        let browser = FileBrowser::new(Arc::new(fs));

        let map = build_map(&browser, "/repo");
        let nodes = map["nodes"].as_array().unwrap();
        // ".", src, tests, src/api = 4
        assert_eq!(nodes.len(), 4);
        let ids: Vec<&str> = nodes.iter().map(|n| n["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"src"));
        assert!(ids.contains(&"src/api"));
        // src/api's edge comes from src.
        let edges = map["edges"].as_array().unwrap();
        assert!(edges.iter().any(|e| e["source"] == "src" && e["target"] == "src/api"));
        assert!(edges.iter().any(|e| e["source"] == "." && e["target"] == "src"));
        assert_eq!(map["truncated"], false);
    }
}
