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
    pub script_name: String,
    pub local_scopes: Vec<HashMap<String, Option<String>>>,
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
            script_name: "adm-toolbox".to_owned(),
            local_scopes: Vec::new(),
        }
    }

    pub fn get(&self, name: &str) -> String {
        match name {
            "0" => self.script_name.clone(),
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


    pub fn push_local_scope(&mut self) {
        self.local_scopes.push(HashMap::new());
    }

    pub fn pop_local_scope(&mut self) {
        if let Some(scope) = self.local_scopes.pop() {
            for (name, previous) in scope {
                match previous {
                    Some(value) => {
                        self.vars.insert(name.clone(), value.clone());
                        if self.exported.contains_key(&name) {
                            self.exported.insert(name, value);
                        }
                    }
                    None => {
                        self.vars.remove(&name);
                        self.exported.remove(&name);
                    }
                }
            }
        }
    }

    pub fn set_local(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let name = name.into();
        let Some(scope) = self.local_scopes.last_mut() else {
            return false;
        };
        scope.entry(name.clone()).or_insert_with(|| self.vars.get(&name).cloned());
        self.set(name, value);
        true
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
