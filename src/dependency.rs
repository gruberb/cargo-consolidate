use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use cargo_metadata::{DependencyKind, Package};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use toml_edit::{DocumentMut, Item, Value};

pub fn collect_dependencies(package: &Package) -> HashSet<String> {
    package
        .dependencies
        .iter()
        .filter(|dep| {
            matches!(
                dep.kind,
                DependencyKind::Normal | DependencyKind::Build | DependencyKind::Development
            )
        })
        .map(|dep| dep.name.clone())
        .collect()
}

pub fn get_dependency_from_member(manifest_path: &Utf8PathBuf, dep_name: &str) -> Result<Item> {
    let cargo_toml_content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read '{}'", manifest_path))?;
    let doc = cargo_toml_content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse '{}'", manifest_path))?;

    let dep_tables = ["dependencies", "build-dependencies", "dev-dependencies"];

    for table_name in &dep_tables {
        if let Some(dep_table) = doc.get(table_name).and_then(|t| t.as_table()) {
            if let Some(dep_entry) = dep_table.get(dep_name) {
                return Ok(dep_entry.clone());
            }
        }
    }

    Err(anyhow::anyhow!(
        "Dependency '{}' not found in '{}'",
        dep_name,
        manifest_path
    ))
}

pub fn merge_features(existing_item: Option<&Item>, new_item: &Item) -> Option<Value> {
    let mut features_set = BTreeSet::new();

    // Collect features from the existing item
    if let Some(existing_item) = existing_item {
        if let Some(existing_features) = get_features(existing_item) {
            features_set.extend(existing_features);
        }
    }

    // Collect features from the new item
    if let Some(new_features) = get_features(new_item) {
        features_set.extend(new_features);
    }

    if !features_set.is_empty() {
        // Convert the set back to a Vec<Value>
        let features_vec: toml_edit::Array = features_set.into_iter().map(Value::from).collect();

        Some(Value::Array(features_vec))
    } else {
        None
    }
}

// Helper function to extract features from an Item
fn get_features(item: &Item) -> Option<Vec<String>> {
    item.as_table_like()
        .and_then(|tbl| tbl.get("features"))
        .and_then(|features_item| features_item.as_value())
        .and_then(|features_value| features_value.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml_edit::{Array, Item, Table, Value};

    // Helper function to create a dependencies table
    fn create_dep_item(version: &str, features: Option<Vec<&str>>) -> Item {
        let mut table = Table::new();
        table.insert("version", toml_edit::Item::Value(version.into()));

        if let Some(feat_list) = features {
            let mut features_array = Array::new();
            for feature in feat_list {
                features_array.push(feature);
            }
            table.insert(
                "features",
                toml_edit::Item::Value(Value::Array(features_array)),
            );
        }

        Item::Table(table)
    }

    #[test]
    fn test_merge_features_no_existing_features() {
        let new_item = create_dep_item("1.0.0", Some(vec!["feature1", "feature2"]));

        let result = merge_features(None, &new_item);

        assert!(result.is_some());
        let result_value = result.unwrap();

        // Check that the result is an array with the new features
        if let Value::Array(arr) = result_value {
            assert_eq!(arr.len(), 2);
            let feature_strings: Vec<_> = arr.iter().filter_map(|v| v.as_str()).collect();

            assert!(feature_strings.contains(&"feature1"));
            assert!(feature_strings.contains(&"feature2"));
        } else {
            panic!("Expected an array of features");
        }
    }

    #[test]
    fn test_merge_features_with_existing_features() {
        // Existing item with features
        let existing_item = create_dep_item("0.9.0", Some(vec!["old_feature"]));

        // New item with additional features
        let new_item = create_dep_item("1.0.0", Some(vec!["new_feature", "old_feature"]));

        let result = merge_features(Some(&existing_item), &new_item);

        assert!(result.is_some());
        let result_value = result.unwrap();

        // Check that the result contains both old and new unique features
        if let Value::Array(arr) = result_value {
            println!("{arr:?}");
            assert_eq!(arr.len(), 2);
            let feature_strings: Vec<_> = arr.iter().filter_map(|v| v.as_str()).collect();

            assert!(feature_strings.contains(&"old_feature"));
            assert!(feature_strings.contains(&"new_feature"));
        } else {
            panic!("Expected an array of features");
        }
    }
}
