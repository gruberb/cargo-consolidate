use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use cargo_metadata::MetadataCommand;
use log::info;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

use crate::dependency;

pub fn consolidate_dependencies(manifest_path: Option<PathBuf>, group_all: bool) -> Result<()> {
    let mut cmd = MetadataCommand::new();
    if let Some(path) = &manifest_path {
        cmd.manifest_path(path);
    }

    let metadata = cmd
        .exec()
        .context("Failed to execute `cargo metadata` command")?;

    // Convert PathBuf to Utf8PathBuf safely
    let workspace_manifest_path = match manifest_path {
        Some(path) => {
            Utf8PathBuf::try_from(path).context("Failed to convert manifest path to UTF-8 path")?
        }
        None => metadata.workspace_root.join("Cargo.toml"),
    };

    // Read and parse root Cargo.toml
    let root_cargo_toml_content = fs::read_to_string(&workspace_manifest_path)
        .with_context(|| format!("Failed to read '{}'", workspace_manifest_path))?;
    let mut root_doc = root_cargo_toml_content
        .parse::<DocumentMut>()
        .context("Failed to parse root Cargo.toml")?;

    // Collect existing workspace dependencies
    let mut workspace_deps = get_workspace_dependencies(&root_doc);
    let mut dep_usage: HashMap<String, HashSet<String>> = HashMap::new();
    let mut package_manifest_paths = HashMap::new();

    // Analyze dependencies across workspace members
    for package_id in &metadata.workspace_members {
        let package = metadata
            .packages
            .iter()
            .find(|p| &p.id == package_id)
            .context("Failed to find package in metadata")?;

        let package_name = &package.name;
        let manifest_path = &package.manifest_path;
        package_manifest_paths.insert(package_name.clone(), manifest_path.clone());

        // Collect dependencies from the package
        let deps = dependency::collect_dependencies(package);

        for dep in deps {
            dep_usage
                .entry(dep)
                .or_default()
                .insert(package_name.clone());
        }
    }

    // Process and consolidate dependencies
    for (dep, users) in dep_usage.iter() {
        let should_group = if group_all { true } else { users.len() >= 2 };

        if should_group {
            // Add to workspace dependencies if not already present
            if !workspace_deps.contains_key(dep) {
                info!(
                    "Adding dependency '{}' to workspace.dependencies (used in {:?})",
                    dep, users
                );
                add_dependency_to_workspace(&mut root_doc, dep, users, &package_manifest_paths)
                    .with_context(|| {
                        format!("Failed to add '{}' to workspace dependencies", dep)
                    })?;
                workspace_deps.insert(dep.clone(), Item::None);
            }

            // Update member Cargo.toml files to use workspace = true
            for user in users {
                let manifest_path = package_manifest_paths.get(user).unwrap();
                update_member_to_use_workspace(manifest_path, dep).with_context(|| {
                    format!("Failed to update '{}' in '{}'", dep, manifest_path)
                })?;
            }
        }
    }

    // Write back the modified root Cargo.toml
    fs::write(&workspace_manifest_path, root_doc.to_string())
        .with_context(|| format!("Failed to write '{}'", workspace_manifest_path))?;

    info!("Successfully updated workspace dependencies.");
    Ok(())
}

fn get_workspace_dependencies(doc: &DocumentMut) -> HashMap<String, Item> {
    doc.get("workspace")
        .and_then(|ws| ws.as_table())
        .and_then(|ws_table| ws_table.get("dependencies"))
        .and_then(|deps| deps.as_table())
        .map(|ws_deps| {
            ws_deps
                .iter()
                .map(|(dep_name, item)| (dep_name.to_string(), item.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn add_dependency_to_workspace(
    doc: &mut DocumentMut,
    dep_name: &str,
    users: &HashSet<String>,
    package_manifest_paths: &HashMap<String, Utf8PathBuf>,
) -> Result<()> {
    // Take the first user's dependency specification
    let first_user = users.iter().next().unwrap();
    let manifest_path = package_manifest_paths.get(first_user).unwrap();
    let dep_item = dependency::get_dependency_from_member(manifest_path, dep_name)?;

    // Ensure workspace table exists
    let ws_deps = doc
        .entry("workspace")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .unwrap()
        .entry("dependencies")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .unwrap();

    ws_deps.insert(dep_name, dep_item);

    Ok(())
}

fn update_member_to_use_workspace(manifest_path: &Utf8PathBuf, dep_name: &str) -> Result<()> {
    let cargo_toml_content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read '{}'", manifest_path))?;
    let mut doc = cargo_toml_content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse '{}'", manifest_path))?;

    let dep_tables = ["dependencies", "build-dependencies", "dev-dependencies"];

    for table_name in &dep_tables {
        if let Some(dep_table) = doc.get_mut(table_name).and_then(Item::as_table_like_mut) {
            if dep_table.contains_key(dep_name) {
                let mut inline_table = InlineTable::default();
                inline_table.insert("workspace", Value::from(true));

                // Preserve existing features
                if let Some(features) = dependency::merge_features(
                    dep_table.get(dep_name),
                    &Item::Value(inline_table.clone().into()),
                ) {
                    inline_table.insert("features", features);
                }

                dep_table.insert(dep_name, Item::Value(inline_table.into()));
            }
        }
    }

    // Write back the modified Cargo.toml
    fs::write(manifest_path, doc.to_string())
        .with_context(|| format!("Failed to write '{}'", manifest_path))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use camino::Utf8PathBuf;
    use std::collections::{HashMap, HashSet};
    use tempfile::TempDir;
    use toml_edit::{Item, Table, Value};

    #[test]
    fn test_get_workspace_dependencies() {
        let mut doc = DocumentMut::default();
        let mut workspace_table = Table::new();
        let mut deps_table = Table::new();
        deps_table.insert("dep1", Item::Value(Value::from("1.0.0")));
        workspace_table.insert("dependencies", Item::Table(deps_table));
        doc.insert("workspace", Item::Table(workspace_table));

        let workspace_deps = get_workspace_dependencies(&doc);
        assert_eq!(workspace_deps.len(), 1);
        assert!(workspace_deps.contains_key("dep1"));
    }

    #[test]
    fn test_add_dependency_to_workspace() -> Result<()> {
        let mut doc = DocumentMut::default();
        let temp_dir = TempDir::new()?;
        let manifest_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_package/Cargo.toml")).unwrap();

        // Create the directory structure and a dummy Cargo.toml file with dep1
        fs::create_dir_all(manifest_path.parent().unwrap())?;
        let cargo_toml_content = r#"
            [dependencies]
            dep1 = "1.0.0"
        "#;
        fs::write(&manifest_path, cargo_toml_content)?;

        let mut package_manifest_paths = HashMap::new();
        package_manifest_paths.insert("test_package".to_string(), manifest_path.clone());

        let mut users = HashSet::new();
        users.insert("test_package".to_string());

        add_dependency_to_workspace(&mut doc, "dep1", &users, &package_manifest_paths)?;

        let workspace_deps = get_workspace_dependencies(&doc);
        assert!(workspace_deps.contains_key("dep1"));
        Ok(())
    }

    #[test]
    fn test_update_member_to_use_workspace() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let manifest_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("test_package/Cargo.toml")).unwrap();
        let dep_name = "dep1";

        // Mock the Cargo.toml content and fs operations for testing
        let cargo_toml_content = r#"
            [dependencies]
            dep1 = "1.0.0"
        "#;
        fs::create_dir_all(manifest_path.parent().unwrap())?;
        fs::write(&manifest_path, cargo_toml_content)?;

        update_member_to_use_workspace(&manifest_path, dep_name)?;

        let updated_content = fs::read_to_string(&manifest_path)?;
        assert!(updated_content.contains("workspace = true"));
        Ok(())
    }
}
