use std::{collections::HashMap, env, path::PathBuf};

use super::ast::AstNode;

#[derive(Clone)]
pub struct ShellEnvironment {
    pub vars: HashMap<String, String>,
    pub exported: HashMap<String, String>,
    pub aliases: HashMap<String, String>,
    pub functions: HashMap<String, AstNode>,
    pub cwd: PathBuf,
    pub last_status: i32,
    pub positional: Vec<String>,
}

impl ShellEnvironment {
    pub fn new() -> Self {
        let exported: HashMap<String, String> = env::vars().collect();
        let vars = exported.clone();
        Self {
            vars,
            exported,
            aliases: HashMap::new(),
            functions: HashMap::new(),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from("C:\\")),
            last_status: 0,
            positional: Vec::new(),
        }
    }

    pub fn get(&self, name: &str) -> String {
        match name {
            "?" => self.last_status.to_string(),
            "#" => self.positional.len().to_string(),
            "@" | "*" => self.positional.join(" "),
            _ => name.parse::<usize>().ok()
                .and_then(|i| if i == 0 { None } else { self.positional.get(i - 1).cloned() })
                .unwrap_or_else(|| self.vars.get(name).cloned().unwrap_or_default()),
        }
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        self.vars.insert(name.clone(), value.clone());
        if self.exported.contains_key(&name) {
            self.exported.insert(name, value);
        }
    }

    pub fn export(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        self.vars.insert(name.clone(), value.clone());
        self.exported.insert(name, value);
    }
}

impl Default for ShellEnvironment {
    fn default() -> Self {
        Self::new()
    }
}
