use anyhow::{anyhow, Context, Result};
use camino::Utf8PathBuf;
use cargo_metadata::{DependencyKind, MetadataCommand, Package};
use clap::Parser;
use log::{error, info, LevelFilter};
use std::collections::{HashMap, HashSet};
use std::fs;
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

#[derive(Parser)]
struct Opt {
    /// Path to the workspace root Cargo.toml
    /// of the project you want to consolidate
    #[arg(long)]
    manifest_path: Option<std::path::PathBuf>,

    /// Group dependencies of all members into workspace.dependencies
    /// If set to false, just dependencies which are used by 2 or more
    /// members are being grouped into workspace.dependencies
    #[arg(long)]
    group_all: bool,

    /// Increase output verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

fn main() {
    if let Err(err) = run() {
        error!("{:?}", err);
        std::process::exit(1);
    }

    info!("Succesfully grouped dependencies!")
}

fn run() -> Result<()> {
    let opt = Opt::parse();

    // Set up logging
    let log_level = match opt.verbose {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    env_logger::Builder::new().filter_level(log_level).init();

    let mut cmd = MetadataCommand::new();

    if let Some(path) = &opt.manifest_path {
        cmd.manifest_path(path);
    }

    let metadata = cmd
        .exec()
        .context("Failed to execute `cargo metadata` command")?;

    // Convert PathBuf to Utf8PathBuf safely
    let workspace_manifest_path = match opt.manifest_path {
        Some(path) => {
            Utf8PathBuf::try_from(path).context("Failed to convert manifest path to UTF-8 path")?
        }
        None => metadata.workspace_root.join("Cargo.toml"),
    };

    // Rest of the code remains unchanged...
    let root_cargo_toml_content = fs::read_to_string(&workspace_manifest_path)
        .with_context(|| format!("Failed to read '{}'", workspace_manifest_path))?;
    let mut root_doc = root_cargo_toml_content
        .parse::<DocumentMut>()
        .context("Failed to parse root Cargo.toml")?;

    // Collect workspace.dependencies
    let mut workspace_deps = get_workspace_dependencies(&root_doc);

    let mut dep_usage: HashMap<String, HashSet<String>> = HashMap::new();
    let mut package_manifest_paths = HashMap::new();

    // Iterate over all packages in the workspace
    for package_id in &metadata.workspace_members {
        let package = metadata
            .packages
            .iter()
            .find(|p| &p.id == package_id)
            .context("Failed to find package in metadata")?;
        let package_name = &package.name;
        let manifest_path = &package.manifest_path;
        package_manifest_paths.insert(package_name.clone(), manifest_path.clone());

        // Collect dependencies from different sections
        let deps = collect_dependencies(package);

        for dep in deps {
            dep_usage
                .entry(dep)
                .or_default()
                .insert(package_name.clone());
        }
    }

    // Process dependencies based on the group_all flag
    for (dep, users) in dep_usage.iter() {
        let should_group = if opt.group_all {
            // Group all dependencies
            true
        } else {
            // Only group dependencies used in at least two members
            users.len() >= 2
        };

        if should_group {
            // Add dependency to workspace.dependencies if not already present
            if !workspace_deps.contains_key(dep) {
                info!(
                    "Adding dependency '{}' to workspace.dependencies (used in {:?})",
                    dep, users
                );
                add_dependency_to_workspace(&mut root_doc, dep, users, &package_manifest_paths)
                    .with_context(|| {
                        format!("Failed to add '{}' to workspace dependencies", dep)
                    })?;
                workspace_deps.insert(dep.clone(), Item::None); // Placeholder
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

    info!("Done updating dependencies.");

    Ok(())
}
fn collect_dependencies(package: &Package) -> HashSet<String> {
    let mut deps = HashSet::new();

    for dep in &package.dependencies {
        // Include normal, build, and development dependencies
        if dep.kind == DependencyKind::Normal
            || dep.kind == DependencyKind::Build
            || dep.kind == DependencyKind::Development
        {
            deps.insert(dep.name.clone());
        }
    }

    deps
}

fn get_workspace_dependencies(doc: &DocumentMut) -> HashMap<String, Item> {
    let mut deps = HashMap::new();

    if let Some(ws_deps) = doc
        .get("workspace")
        .and_then(|ws| ws.as_table())
        .and_then(|ws_table| ws_table.get("dependencies"))
        .and_then(|deps| deps.as_table())
    {
        for (dep_name, item) in ws_deps.iter() {
            deps.insert(dep_name.to_string(), item.clone());
        }
    }

    deps
}

fn add_dependency_to_workspace(
    doc: &mut DocumentMut,
    dep_name: &str,
    users: &HashSet<String>,
    package_manifest_paths: &HashMap<String, Utf8PathBuf>,
) -> Result<()> {
    // For simplicity, take the first user's dependency specification
    let first_user = users.iter().next().unwrap();
    let manifest_path = package_manifest_paths.get(first_user).unwrap();
    let dep_item = get_dependency_from_member(manifest_path, dep_name).with_context(|| {
        format!(
            "Failed to get dependency '{}' from member '{}'",
            dep_name, manifest_path
        )
    })?;

    // Add the dependency to [workspace.dependencies]
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

fn get_dependency_from_member(manifest_path: &Utf8PathBuf, dep_name: &str) -> Result<Item> {
    // Read and parse the member's Cargo.toml
    let cargo_toml_content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read '{}'", manifest_path))?;
    let doc = cargo_toml_content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse '{}'", manifest_path))?;

    // Look for the dependency in [dependencies], [build-dependencies], [dev-dependencies]
    let dep_tables = ["dependencies", "build-dependencies", "dev-dependencies"];

    for table_name in &dep_tables {
        if let Some(dep_table) = doc.get(table_name).and_then(|t| t.as_table()) {
            if let Some(dep_entry) = dep_table.get(dep_name) {
                return Ok(dep_entry.clone());
            }
        }
    }

    Err(anyhow!(
        "Dependency '{}' not found in '{}'",
        dep_name,
        manifest_path
    ))
}

fn update_member_to_use_workspace(manifest_path: &Utf8PathBuf, dep_name: &str) -> Result<()> {
    // Read and parse the member's Cargo.toml
    let cargo_toml_content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read '{}'", manifest_path))?;
    let mut doc = cargo_toml_content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse '{}'", manifest_path))?;

    // Update [dependencies], [build-dependencies], and [dev-dependencies]
    let dep_tables = ["dependencies", "build-dependencies", "dev-dependencies"];

    for table_name in &dep_tables {
        if let Some(dep_table) = doc.get_mut(table_name).and_then(Item::as_table_like_mut) {
            if dep_table.contains_key(dep_name) {
                // Create an inline table with { workspace = true }
                let mut inline_table = InlineTable::default();
                inline_table.insert("workspace", Value::from(true));

                // Preserve existing features, if any
                if let Some(existing_item) = dep_table.get(dep_name) {
                    if let Some(existing_table) = existing_item.as_table_like() {
                        if let Some(features) = existing_table.get("features") {
                            // features is an Item; need to get its Value
                            if let Some(features_value) = features.as_value() {
                                inline_table.insert("features", features_value.clone());
                            }
                        }
                    }
                }

                // Set the dependency entry with proper key formatting
                dep_table.insert(dep_name, Item::Value(inline_table.into()));
            }
        }
    }

    // Write back the modified Cargo.toml
    fs::write(manifest_path, doc.to_string())
        .with_context(|| format!("Failed to write '{}'", manifest_path))?;

    Ok(())
}
