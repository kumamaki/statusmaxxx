use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::paths;

/// An agent's JSON settings file, edited in place. Keys we do not touch keep
/// their order; formatting becomes two-space pretty JSON.
pub struct JsonSettings {
    path: PathBuf,
    root: Map<String, Value>,
}

impl JsonSettings {
    pub fn open(path: PathBuf) -> Result<Self> {
        let root = match fs::read_to_string(&path) {
            // A comment or trailing comma (JSONC) would be lost on rewrite, so refuse instead.
            Ok(contents) if !contents.trim().is_empty() => match serde_json::from_str(&contents) {
                Ok(Value::Object(root)) => root,
                Ok(_) => bail!("<{}> is not a JSON object", path.display()),
                Err(error) => bail!("<{}> is not plain JSON ({error}); edit it by hand", path.display()),
            },
            Ok(_) => Map::new(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(error) => return Err(error).with_context(|| format!("Cannot read <{}>", path.display())),
        };
        Ok(Self { path, root })
    }

    pub fn get(&self, keys: &[&str]) -> Option<&Value> {
        let (last, parents) = keys.split_last()?;
        let mut object = &self.root;
        for key in parents {
            object = object.get(*key)?.as_object()?;
        }
        object.get(*last)
    }

    pub fn set(&mut self, keys: &[&str], value: Value) -> Result<()> {
        let (last, parents) = keys.split_last().context("Empty settings key")?;
        let mut object = &mut self.root;
        for key in parents {
            let child = object.entry(key.to_string()).or_insert_with(|| Value::Object(Map::new()));
            object = child
                .as_object_mut()
                .with_context(|| format!("<{key}> in <{}> is not an object", self.path.display()))?;
        }
        object.insert(last.to_string(), value);
        Ok(())
    }

    /// Removes the key, then any parent objects the removal left empty.
    pub fn remove(&mut self, keys: &[&str]) -> Option<Value> {
        remove_nested(&mut self.root, keys)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Saves, keeping the previous file as `<name>.statusmaxxx.bak`.
    pub fn save(&self) -> Result<()> {
        if self.path.exists() {
            let backup = backup_path(&self.path);
            fs::copy(&self.path, &backup).with_context(|| format!("Cannot back up to <{}>", backup.display()))?;
        }
        paths::write_file(&self.path, &format!("{}\n", serde_json::to_string_pretty(&self.root)?))
    }
}

fn remove_nested(object: &mut Map<String, Value>, keys: &[&str]) -> Option<Value> {
    let (first, rest) = keys.split_first()?;
    if rest.is_empty() {
        return object.remove(*first);
    }
    let child = object.get_mut(*first)?.as_object_mut()?;
    let removed = remove_nested(child, rest);
    if child.is_empty() {
        object.remove(*first);
    }
    removed
}

pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".statusmaxxx.bak");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn edits_nested_keys_without_reordering_or_leaving_empty_parents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        paths::write_file(&path, r#"{"theme":"dark","ui":{"vim":true},"model":"x"}"#).unwrap();

        let mut settings = JsonSettings::open(path.clone()).unwrap();
        settings.set(&["ui", "statusLine"], json!({"type": "command"})).unwrap();
        settings.save().unwrap();

        let saved: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let keys: Vec<&String> = saved.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["theme", "ui", "model"]);
        assert_eq!(saved["ui"], json!({"vim": true, "statusLine": {"type": "command"}}));
        assert!(backup_path(&path).exists());

        settings.remove(&["ui", "vim"]);
        settings.remove(&["ui", "statusLine"]);
        assert!(settings.get(&["ui"]).is_none(), "an emptied parent object is removed too");
    }

    #[test]
    fn refuses_jsonc_it_would_destroy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        paths::write_file(&path, "{\n  // keep me\n  \"a\": 1\n}").unwrap();
        assert!(JsonSettings::open(path).is_err());
    }
}
