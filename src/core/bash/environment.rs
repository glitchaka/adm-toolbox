use std::{
    collections::{HashMap, HashSet},
    env,
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use super::ast::AstNode;

#[derive(Clone, Default)]
pub struct LocalBinding {
    scalar: Option<String>,
    exported: Option<String>,
    array: Option<Vec<String>>,
    associative: Option<HashMap<String, String>>,
    nameref: Option<String>,
    readonly: bool,
}

#[derive(Clone)]
pub struct ShellEnvironment {
    pub vars: HashMap<String, String>,
    pub exported: HashMap<String, String>,
    pub aliases: HashMap<String, String>,
    pub functions: HashMap<String, AstNode>,
    pub arrays: HashMap<String, Vec<String>>,
    pub assoc_arrays: HashMap<String, HashMap<String, String>>,
    pub namerefs: HashMap<String, String>,
    pub readonly: HashSet<String>,
    pub shell_options: HashSet<String>,
    pub shopt_options: HashSet<String>,
    pub traps: HashMap<String, String>,
    pub cwd: PathBuf,
    pub oldpwd: Option<PathBuf>,
    pub dir_stack: Vec<PathBuf>,
    pub last_status: i32,
    pub last_background_pid: Option<u32>,
    pub positional: Vec<String>,
    pub script_name: String,
    pub local_scopes: Vec<HashMap<String, LocalBinding>>,
    started_at: Instant,
}

impl ShellEnvironment {
    pub fn new() -> Self {
        let exported: HashMap<String, String> = env::vars().collect();
        let mut vars = exported.clone();
        vars.entry("IFS".to_owned()).or_insert_with(|| " \t\n".to_owned());
        vars.insert("BASH_VERSION".to_owned(), "5.3.0(1)-sst".to_owned());

        let mut arrays = HashMap::new();
        arrays.insert(
            "BASH_VERSINFO".to_owned(),
            vec![
                "5".to_owned(),
                "3".to_owned(),
                "0".to_owned(),
                "1".to_owned(),
                "release".to_owned(),
                "x86_64-pc-windows-sst".to_owned(),
            ],
        );

        let mut readonly = HashSet::new();
        readonly.insert("BASH_VERSINFO".to_owned());
        Self {
            vars,
            exported,
            aliases: HashMap::new(),
            functions: HashMap::new(),
            arrays,
            assoc_arrays: HashMap::new(),
            namerefs: HashMap::new(),
            readonly,
            shell_options: ["braceexpand", "hashall"].into_iter().map(str::to_owned).collect(),
            shopt_options: ["globskipdots", "patsub_replacement", "progcomp", "sourcepath"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            traps: HashMap::new(),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from("C:\\")),
            oldpwd: None,
            dir_stack: Vec::new(),
            last_status: 0,
            last_background_pid: None,
            positional: Vec::new(),
            script_name: "adm-toolbox".to_owned(),
            local_scopes: Vec::new(),
            started_at: Instant::now(),
        }
    }

    fn dereference_name(&self, name: &str) -> String {
        let mut current = name.to_owned();
        for _ in 0..32 {
            let (base, suffix) = if let Some(open) = current.find('[') {
                (&current[..open], &current[open..])
            } else {
                (current.as_str(), "")
            };
            let Some(target) = self.namerefs.get(base) else {
                break;
            };
            let next = format!("{target}{suffix}");
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    pub fn set_nameref(&mut self, name: impl Into<String>, target: impl Into<String>) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) {
            return false;
        }
        self.vars.remove(&name);
        self.arrays.remove(&name);
        self.assoc_arrays.remove(&name);
        self.namerefs.insert(name, target.into());
        true
    }

    pub fn unset_nameref(&mut self, name: &str) -> bool {
        if self.readonly.contains(name) {
            return false;
        }
        self.namerefs.remove(name).is_some()
    }

    pub fn is_nameref(&self, name: &str) -> bool {
        self.namerefs.contains_key(name)
    }

    pub fn get(&self, name: &str) -> String {
        match name {
            "0" => self.script_name.clone(),
            "?" => self.last_status.to_string(),
            "#" => self.positional.len().to_string(),
            "@" => self.positional.join(" "),
            "*" => self.positional.join(&self.ifs_first().to_string()),
            "!" => self.last_background_pid.map(|p| p.to_string()).unwrap_or_default(),
            "-" => {
                let mut flags = String::new();
                if self.shell_options.contains("allexport") { flags.push('a'); }
                if self.shell_options.contains("notify") { flags.push('b'); }
                if self.shell_options.contains("errexit") { flags.push('e'); }
                if self.shell_options.contains("noglob") { flags.push('f'); }
                if self.shell_options.contains("hashall") { flags.push('h'); }
                if self.shell_options.contains("histexpand") { flags.push('H'); }
                if self.shell_options.contains("monitor") { flags.push('m'); }
                if self.shell_options.contains("noexec") { flags.push('n'); }
                if self.shell_options.contains("physical") { flags.push('P'); }
                if self.shell_options.contains("nounset") { flags.push('u'); }
                if self.shell_options.contains("verbose") { flags.push('v'); }
                if self.shell_options.contains("xtrace") { flags.push('x'); }
                if self.shell_options.contains("braceexpand") { flags.push('B'); }
                if self.shell_options.contains("noclobber") { flags.push('C'); }
                flags
            }
            "$" => std::process::id().to_string(),
            "SECONDS" => self.started_at.elapsed().as_secs().to_string(),
            "EPOCHSECONDS" => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_owned()),
            "EPOCHREALTIME" => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| format!("{}.{:06}", duration.as_secs(), duration.subsec_micros()))
                .unwrap_or_else(|_| "0.000000".to_owned()),
            "SHELLOPTS" => {
                let mut options: Vec<_> = self.shell_options.iter().cloned().collect();
                options.sort();
                options.join(":")
            }
            "BASHOPTS" => {
                let mut options: Vec<_> = self.shopt_options.iter().cloned().collect();
                options.sort();
                options.join(":")
            }
            _ => {
                let resolved = self.dereference_name(name);
                if let Some((base, subscript)) = split_subscript(&resolved) {
                    return self.get_array_value(base, subscript);
                }
                resolved.parse::<usize>().ok()
                    .and_then(|i| if i == 0 { None } else { self.positional.get(i - 1).cloned() })
                    .unwrap_or_else(|| self.vars.get(&resolved).cloned().unwrap_or_default())
            }
        }
    }

    pub fn is_set(&self, name: &str) -> bool {
        let resolved = self.dereference_name(name);
        let name = resolved.as_str();
        if let Some((base, subscript)) = split_subscript(name) {
            if let Some(array) = self.arrays.get(base) {
                if subscript == "@" || subscript == "*" { return !array.is_empty(); }
                return subscript.parse::<usize>().ok().is_some_and(|i| i < array.len());
            }
            if let Some(array) = self.assoc_arrays.get(base) {
                if subscript == "@" || subscript == "*" { return !array.is_empty(); }
                return array.contains_key(subscript);
            }
            return false;
        }
        self.vars.contains_key(name) || self.arrays.contains_key(name) || self.assoc_arrays.contains_key(name)
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let original = name.into();
        let name = self.dereference_name(&original);
        if self.readonly.contains(&name) { return false; }
        let value = value.into();

        if let Some((base, subscript)) = split_subscript_owned(&name) {
            if self.readonly.contains(&base) { return false; }
            if self.assoc_arrays.contains_key(&base) {
                self.assoc_arrays.entry(base).or_default().insert(subscript, value);
                return true;
            }
            if let Ok(index) = subscript.parse::<usize>() {
                let array = self.arrays.entry(base).or_default();
                if array.len() <= index { array.resize(index + 1, String::new()); }
                array[index] = value;
                return true;
            }
        }

        self.vars.insert(name.clone(), value.clone());
        if self.exported.contains_key(&name) || self.shell_options.contains("allexport") {
            self.exported.insert(name, value);
        }
        true
    }

    pub fn unset(&mut self, name: &str) -> bool {
        let resolved = self.dereference_name(name);
        let name = resolved.as_str();
        if self.readonly.contains(name) { return false; }
        if let Some((base, subscript)) = split_subscript(name) {
            if self.readonly.contains(base) { return false; }
            if let Some(array) = self.arrays.get_mut(base) {
                if let Ok(index) = subscript.parse::<usize>() {
                    if index < array.len() { array[index].clear(); }
                    return true;
                }
            }
            if let Some(array) = self.assoc_arrays.get_mut(base) {
                array.remove(subscript);
                return true;
            }
        }
        self.vars.remove(name);
        self.exported.remove(name);
        self.arrays.remove(name);
        self.assoc_arrays.remove(name);
        self.namerefs.remove(name);
        true
    }

    pub fn set_array(&mut self, name: impl Into<String>, values: Vec<String>) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) { return false; }
        self.vars.remove(&name);
        self.assoc_arrays.remove(&name);
        self.arrays.insert(name, values);
        true
    }

    pub fn declare_assoc(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) { return false; }
        self.vars.remove(&name);
        self.arrays.remove(&name);
        self.assoc_arrays.entry(name).or_default();
        true
    }

    pub fn array_values(&self, name: &str) -> Vec<String> {
        let resolved = self.dereference_name(name);
        let name = resolved.as_str();
        if let Some(values) = self.arrays.get(name) {
            return values.clone();
        }
        if let Some(values) = self.assoc_arrays.get(name) {
            let mut keys: Vec<_> = values.keys().cloned().collect();
            keys.sort();
            return keys.into_iter().filter_map(|k| values.get(&k).cloned()).collect();
        }
        self.vars.get(name).cloned().into_iter().collect()
    }

    pub fn array_keys(&self, name: &str) -> Vec<String> {
        let resolved = self.dereference_name(name);
        let name = resolved.as_str();
        if let Some(values) = self.arrays.get(name) {
            return values.iter().enumerate().filter_map(|(i,v)| (!v.is_empty()).then(|| i.to_string())).collect();
        }
        if let Some(values) = self.assoc_arrays.get(name) {
            let mut keys: Vec<_> = values.keys().cloned().collect();
            keys.sort();
            return keys;
        }
        if self.vars.contains_key(name) { vec!["0".to_owned()] } else { Vec::new() }
    }

    fn get_array_value(&self, base: &str, subscript: &str) -> String {
        if subscript == "@" || subscript == "*" {
            let sep = if subscript == "*" { self.ifs_first().to_string() } else { " ".to_owned() };
            return self.array_values(base).join(&sep);
        }
        if let Some(array) = self.arrays.get(base) {
            return subscript.parse::<usize>().ok().and_then(|i| array.get(i)).cloned().unwrap_or_default();
        }
        if let Some(array) = self.assoc_arrays.get(base) {
            return array.get(subscript).cloned().unwrap_or_default();
        }
        if subscript == "0" {
            return self.vars.get(base).cloned().unwrap_or_default();
        }
        String::new()
    }

    pub fn ifs_first(&self) -> char {
        self.vars.get("IFS").and_then(|v| v.chars().next()).unwrap_or(' ')
    }

    pub fn push_local_scope(&mut self) {
        self.local_scopes.push(HashMap::new());
    }

    fn binding_snapshot(&self, name: &str) -> LocalBinding {
        LocalBinding {
            scalar: self.vars.get(name).cloned(),
            exported: self.exported.get(name).cloned(),
            array: self.arrays.get(name).cloned(),
            associative: self.assoc_arrays.get(name).cloned(),
            nameref: self.namerefs.get(name).cloned(),
            readonly: self.readonly.contains(name),
        }
    }

    fn remember_local(&mut self, name: &str) {
        let snapshot = self.binding_snapshot(name);
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.entry(name.to_owned()).or_insert(snapshot);
        }
    }

    fn clear_binding(&mut self, name: &str) {
        self.vars.remove(name);
        self.exported.remove(name);
        self.arrays.remove(name);
        self.assoc_arrays.remove(name);
        self.namerefs.remove(name);
        self.readonly.remove(name);
    }

    pub fn pop_local_scope(&mut self) {
        if let Some(scope) = self.local_scopes.pop() {
            for (name, previous) in scope {
                self.clear_binding(&name);
                if let Some(value) = previous.scalar {
                    self.vars.insert(name.clone(), value);
                }
                if let Some(value) = previous.exported {
                    self.exported.insert(name.clone(), value);
                }
                if let Some(value) = previous.array {
                    self.arrays.insert(name.clone(), value);
                }
                if let Some(value) = previous.associative {
                    self.assoc_arrays.insert(name.clone(), value);
                }
                if let Some(value) = previous.nameref {
                    self.namerefs.insert(name.clone(), value);
                }
                if previous.readonly {
                    self.readonly.insert(name);
                }
            }
        }
    }

    pub fn localize_unset(&mut self, name: &str) -> bool {
        if self.local_scopes.is_empty() || self.readonly.contains(name) {
            return false;
        }
        self.remember_local(name);
        self.clear_binding(name);
        true
    }

    pub fn set_local(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let name = name.into();
        if self.local_scopes.is_empty() || self.readonly.contains(&name) {
            return false;
        }
        self.remember_local(&name);
        self.set(name, value)
    }

    pub fn set_local_array(&mut self, name: impl Into<String>, values: Vec<String>) -> bool {
        let name = name.into();
        if self.local_scopes.is_empty() || self.readonly.contains(&name) {
            return false;
        }
        self.remember_local(&name);
        self.set_array(name, values)
    }

    pub fn declare_local_assoc(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.local_scopes.is_empty() || self.readonly.contains(&name) {
            return false;
        }
        self.remember_local(&name);
        self.declare_assoc(name)
    }

    pub fn set_local_nameref(&mut self, name: impl Into<String>, target: impl Into<String>) -> bool {
        let name = name.into();
        if self.local_scopes.is_empty() || self.readonly.contains(&name) {
            return false;
        }
        self.remember_local(&name);
        self.set_nameref(name, target)
    }

    pub fn export(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) { return false; }
        let value = value.into();
        self.vars.insert(name.clone(), value.clone());
        self.exported.insert(name, value);
        true
    }

    pub fn mark_exported(&mut self, name: &str) {
        let value = self.get(name);
        self.exported.insert(name.to_owned(), value);
    }

    pub fn set_readonly(&mut self, name: &str) {
        self.readonly.insert(name.to_owned());
    }

    pub fn option_enabled(&self, name: &str) -> bool {
        self.shell_options.contains(name) || self.shopt_options.contains(name)
    }
}

fn split_subscript(name: &str) -> Option<(&str, &str)> {
    let open = name.find('[')?;
    let close = name.strip_suffix(']')?;
    Some((&name[..open], &close[open + 1..]))
}

fn split_subscript_owned(name: &str) -> Option<(String, String)> {
    split_subscript(name).map(|(a,b)| (a.to_owned(), b.to_owned()))
}

impl Default for ShellEnvironment {
    fn default() -> Self { Self::new() }
}
