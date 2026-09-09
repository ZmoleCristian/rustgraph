use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike, Value};

/// Cargo dependency names visible to source code, including renamed packages.
#[derive(Debug, Default, Clone)]
pub(super) struct DependencyCatalog {
    /// Rust path root (dependency key) -> crates.io/package name.
    aliases: HashMap<String, String>,
    /// Package name -> deterministic preferred Rust path root.
    preferred_aliases: HashMap<String, String>,
}

impl DependencyCatalog {
    pub(super) fn discover(project_roots: &[PathBuf], rust_files: &[PathBuf]) -> Self {
        let mut manifests = BTreeSet::new();
        let canonical_roots: BTreeSet<PathBuf> = project_roots
            .iter()
            .filter_map(|root| fs::canonicalize(root).ok())
            .collect();
        for root in project_roots {
            insert_manifest(&mut manifests, root.join("Cargo.toml"));
        }

        for file in rust_files {
            let traversal_path = fs::canonicalize(file).unwrap_or_else(|_| file.clone());
            let mut cursor = traversal_path.parent();
            while let Some(dir) = cursor {
                insert_manifest(&mut manifests, dir.join("Cargo.toml"));
                if canonical_roots.contains(dir) || project_roots.iter().any(|root| root == dir) {
                    break;
                }
                cursor = dir.parent();
            }
        }

        // A caller may point directly at one workspace member. Cargo still
        // resolves `{ workspace = true }` dependencies from an ancestor
        // workspace manifest, so include that manifest when it is actually
        // needed instead of indiscriminately absorbing every parent crate.
        let inherited_workspace_manifests: Vec<PathBuf> = manifests
            .iter()
            .filter_map(|manifest| parent_workspace_manifest(manifest))
            .collect();
        manifests.extend(inherited_workspace_manifests);

        let mut catalog = Self::default();
        for manifest in manifests {
            catalog.read_manifest(&manifest);
        }
        catalog.rebuild_preferred_aliases();
        catalog
    }

    pub(super) fn package_for_alias(&self, alias: &str) -> Option<&str> {
        self.aliases.get(alias).map(String::as_str)
    }

    pub(super) fn preferred_alias(&self, package: &str) -> Option<&str> {
        self.preferred_aliases.get(package).map(String::as_str)
    }

    fn read_manifest(&mut self, manifest: &Path) {
        let Ok(contents) = fs::read_to_string(manifest) else {
            return;
        };
        let Ok(doc) = contents.parse::<DocumentMut>() else {
            return;
        };

        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(table) = doc.get(section).and_then(Item::as_table_like) {
                self.collect_dependency_table(table);
            }
        }

        if let Some(workspace) = doc.get("workspace").and_then(Item::as_table_like)
            && let Some(table) = workspace.get("dependencies").and_then(Item::as_table_like)
        {
            self.collect_dependency_table(table);
        }

        if let Some(targets) = doc.get("target").and_then(Item::as_table_like) {
            for (_, target) in targets.iter() {
                let Some(target_table) = target.as_table_like() else {
                    continue;
                };
                for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                    if let Some(table) = target_table.get(section).and_then(Item::as_table_like) {
                        self.collect_dependency_table(table);
                    }
                }
            }
        }
    }

    fn collect_dependency_table(&mut self, table: &dyn TableLike) {
        for (cargo_name, item) in table.iter() {
            let alias = normalize_crate_name(cargo_name);
            if let Some(package) = dependency_package(item) {
                self.aliases.insert(alias, package.to_string());
            } else {
                // A member manifest may say `{ workspace = true }` while the
                // workspace manifest carries the actual renamed package.
                // Preserve that more precise mapping regardless of file order.
                self.aliases
                    .entry(alias)
                    .or_insert_with(|| cargo_name.to_string());
            }
        }
    }

    fn rebuild_preferred_aliases(&mut self) {
        let mut entries: Vec<(String, String)> = self
            .aliases
            .iter()
            .map(|(alias, package)| (alias.clone(), package.clone()))
            .collect();
        entries.sort();
        for (alias, package) in entries {
            let preferred = self
                .preferred_aliases
                .entry(package.clone())
                .or_insert_with(|| alias.clone());
            // Prefer the canonical spelling when both canonical and renamed
            // dependencies appear in merged workspace manifests.
            if alias == normalize_crate_name(&package) {
                *preferred = alias;
            }
        }
    }
}

fn dependency_package(item: &Item) -> Option<&str> {
    if let Some(table) = item.as_table_like() {
        return table.get("package").and_then(Item::as_str);
    }
    item.as_value()
        .and_then(Value::as_inline_table)
        .and_then(|table| table.get("package"))
        .and_then(Value::as_str)
}

fn insert_manifest(manifests: &mut BTreeSet<PathBuf>, manifest: PathBuf) {
    if manifest.is_file() {
        manifests.insert(fs::canonicalize(&manifest).unwrap_or(manifest));
    }
}

fn parent_workspace_manifest(member_manifest: &Path) -> Option<PathBuf> {
    let member_contents = fs::read_to_string(member_manifest).ok()?;
    let member_doc = member_contents.parse::<DocumentMut>().ok()?;
    if !manifest_uses_workspace_dependency(&member_doc) {
        return None;
    }

    let mut cursor = member_manifest.parent()?.parent();
    while let Some(directory) = cursor {
        let candidate = directory.join("Cargo.toml");
        if candidate.is_file()
            && fs::read_to_string(&candidate)
                .ok()
                .and_then(|contents| contents.parse::<DocumentMut>().ok())
                .is_some_and(|doc| doc.get("workspace").and_then(Item::as_table_like).is_some())
        {
            return Some(fs::canonicalize(&candidate).unwrap_or(candidate));
        }
        cursor = directory.parent();
    }
    None
}

fn manifest_uses_workspace_dependency(doc: &DocumentMut) -> bool {
    let sections = ["dependencies", "dev-dependencies", "build-dependencies"];
    if sections
        .into_iter()
        .filter_map(|section| doc.get(section).and_then(Item::as_table_like))
        .any(|table| {
            table
                .iter()
                .any(|(_, item)| dependency_uses_workspace(item))
        })
    {
        return true;
    }

    doc.get("target")
        .and_then(Item::as_table_like)
        .is_some_and(|targets| {
            targets.iter().any(|(_, target)| {
                target.as_table_like().is_some_and(|target_table| {
                    sections.into_iter().any(|section| {
                        target_table
                            .get(section)
                            .and_then(Item::as_table_like)
                            .is_some_and(|dependencies| {
                                dependencies
                                    .iter()
                                    .any(|(_, item)| dependency_uses_workspace(item))
                            })
                    })
                })
            })
        })
}

fn dependency_uses_workspace(item: &Item) -> bool {
    item.as_table_like()
        .and_then(|table| table.get("workspace"))
        .and_then(Item::as_bool)
        .unwrap_or(false)
        || item
            .as_value()
            .and_then(Value::as_inline_table)
            .and_then(|table| table.get("workspace"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

pub(super) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_direct_renamed_workspace_and_target_dependencies() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[dependencies]
url = "2"
ids = { package = "uuid", version = "1" }
friendly = { package = "foo-bar", version = "1" }
native-hyphen = "1"

[workspace.dependencies]
semver = "1"
shared-id = { package = "shared-identifier", version = "1" }

[target.'cfg(unix)'.dependencies]
http-types = { package = "http", version = "1" }
"#,
        )
        .expect("manifest");
        fs::create_dir_all(dir.path().join("member/src")).expect("member src");
        fs::write(
            dir.path().join("member/Cargo.toml"),
            "[package]\nname='member'\nversion='0.1.0'\n[dependencies]\nshared-id.workspace=true\n",
        )
        .expect("member manifest");
        let member_source = dir.path().join("member/src/lib.rs");
        fs::write(&member_source, "").expect("member source");

        let catalog = DependencyCatalog::discover(
            &[dir.path().to_path_buf()],
            std::slice::from_ref(&member_source),
        );
        assert_eq!(catalog.package_for_alias("url"), Some("url"));
        assert_eq!(catalog.package_for_alias("ids"), Some("uuid"));
        assert_eq!(catalog.preferred_alias("uuid"), Some("ids"));
        assert_eq!(catalog.package_for_alias("friendly"), Some("foo-bar"));
        assert_eq!(catalog.preferred_alias("foo-bar"), Some("friendly"));
        assert_eq!(
            catalog.package_for_alias("native_hyphen"),
            Some("native-hyphen")
        );
        assert_eq!(catalog.package_for_alias("http_types"), Some("http"));
        assert_eq!(catalog.preferred_alias("semver"), Some("semver"));
        assert_eq!(
            catalog.package_for_alias("shared_id"),
            Some("shared-identifier")
        );

        let member_only = DependencyCatalog::discover(
            &[dir.path().join("member")],
            std::slice::from_ref(&member_source),
        );
        assert_eq!(
            member_only.package_for_alias("shared_id"),
            Some("shared-identifier")
        );
    }
}
