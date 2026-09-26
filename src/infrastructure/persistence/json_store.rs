use std::{fs, marker::PhantomData, path::PathBuf};

use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};

pub struct JsonFileStore<T> {
    path: PathBuf,
    marker: PhantomData<T>,
}

impl<T> JsonFileStore<T>
where
    T: Serialize + DeserializeOwned,
{
    pub fn new(path: PathBuf) -> Self {
        Self { path, marker: PhantomData }
    }

    pub fn load(&self) -> Result<Vec<T>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&fs::read_to_string(&self.path)?)?)
    }

    pub fn save(&self, values: &[T]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(values)?)?;
        Ok(())
    }

    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
}
