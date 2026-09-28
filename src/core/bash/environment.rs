use std::{
    cell::Cell,
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
    array_present: Option<HashSet<usize>>,
    associative: Option<HashMap<String, String>>,
    nameref: Option<String>,
    readonly: bool,
    integer: bool,
    uppercase: bool,
    lowercase: bool,
    trace: bool,
}

#[derive(Clone)]
pub struct ShellEnvironment {
    pub vars: HashMap<String, String>,
    pub exported: HashMap<String, String>,
    pub aliases: HashMap<String, String>,
    pub command_hash: HashMap<String, String>,
    pub functions: HashMap<String, AstNode>,
    pub readonly_functions: HashSet<String>,
    pub exported_functions: HashSet<String>,
    pub trace_functions: HashSet<String>,
    pub arrays: HashMap<String, Vec<String>>,
    pub array_present: HashMap<String, HashSet<usize>>,
    pub assoc_arrays: HashMap<String, HashMap<String, String>>,
    pub namerefs: HashMap<String, String>,
    pub readonly: HashSet<String>,
    pub integer_vars: HashSet<String>,
    pub uppercase_vars: HashSet<String>,
    pub lowercase_vars: HashSet<String>,
    pub trace_vars: HashSet<String>,
    disabled_special_vars: HashSet<String>,
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
    local_shell_options: Vec<Option<HashSet<String>>>,
    started_at: Instant,
    seconds_base: i64,
    random_state: Cell<u32>,
    srandom_state: Cell<u64>,
}

impl ShellEnvironment {
    pub fn new() -> Self {
        let exported: HashMap<String, String> = env::vars().collect();
        let mut vars = exported.clone();
        vars.entry("IFS".to_owned()).or_insert_with(|| " \t\n".to_owned());
        vars.insert("BASH_VERSION".to_owned(), "5.3.0(1)-sst".to_owned());
        vars.entry("BASH_TRAPSIG".to_owned()).or_insert_with(|| "0".to_owned());
        vars.entry("BASH_SUBSHELL".to_owned()).or_insert_with(|| "0".to_owned());
        vars.entry("BASH_COMMAND".to_owned()).or_default();
        vars.entry("LINENO".to_owned()).or_insert_with(|| "1".to_owned());
        vars.entry("OPTIND".to_owned()).or_insert_with(|| "1".to_owned());
        vars.entry("OPTERR".to_owned()).or_insert_with(|| "1".to_owned());
        vars.entry("HOSTNAME".to_owned()).or_insert_with(|| {
            env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_owned())
        });
        vars.entry("HOSTTYPE".to_owned()).or_insert_with(|| std::env::consts::ARCH.to_owned());
        vars.entry("OSTYPE".to_owned()).or_insert_with(|| "windows-sst".to_owned());
        vars.entry("MACHTYPE".to_owned()).or_insert_with(|| {
            format!("{}-pc-windows-sst", std::env::consts::ARCH)
        });
        vars.entry("BASH_COMPAT".to_owned()).or_insert_with(|| "5.3".to_owned());
        vars.entry("HISTSIZE".to_owned()).or_insert_with(|| "500".to_owned());
        vars.entry("HISTFILESIZE".to_owned()).or_insert_with(|| "500".to_owned());
        vars.entry("COMP_WORDBREAKS".to_owned())
            .or_insert_with(|| " \t\n\"'><=;|&(:".to_owned());
        vars.entry("HISTTIMEFORMAT".to_owned()).or_default();
        vars.entry("PROMPT_DIRTRIM".to_owned()).or_insert_with(|| "0".to_owned());
        vars.entry("MAILCHECK".to_owned()).or_insert_with(|| "60".to_owned());
        let shlvl = vars.get("SHLVL")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0)
            .saturating_add(1);
        vars.insert("SHLVL".to_owned(), shlvl.to_string());

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
        arrays.insert("GROUPS".to_owned(), vec!["0".to_owned()]);
        arrays.insert("FUNCNAME".to_owned(), vec!["main".to_owned()]);
        arrays.insert("BASH_SOURCE".to_owned(), vec!["adm-toolbox".to_owned()]);
        arrays.insert("BASH_LINENO".to_owned(), vec!["0".to_owned()]);
        arrays.insert("BASH_ARGC".to_owned(), vec!["0".to_owned()]);
        arrays.insert("BASH_ARGV".to_owned(), Vec::new());

        let mut assoc_arrays = HashMap::new();
        assoc_arrays.insert("BASH_ALIASES".to_owned(), HashMap::new());
        assoc_arrays.insert("BASH_CMDS".to_owned(), HashMap::new());

        let array_present: HashMap<String, HashSet<usize>> = arrays.iter()
            .map(|(name, values)| {
                (name.clone(), (0..values.len()).collect::<HashSet<_>>())
            })
            .collect();

        let mut shell_options: HashSet<String> =
            ["braceexpand", "hashall"].into_iter().map(str::to_owned).collect();
        if let Some(inherited) = exported.get("SHELLOPTS") {
            shell_options.extend(
                inherited.split(':').filter(|option| !option.is_empty()).map(str::to_owned)
            );
        }

        let mut shopt_options: HashSet<String> = [
            "checkwinsize", "cmdhist", "complete_fullquote", "extquote",
            "force_fignore", "globasciiranges", "globskipdots", "hostcomplete",
            "interactive_comments", "patsub_replacement", "progcomp",
            "promptvars", "sourcepath",
        ].into_iter().map(str::to_owned).collect();
        if let Some(inherited) = exported.get("BASHOPTS") {
            shopt_options.extend(
                inherited.split(':').filter(|option| !option.is_empty()).map(str::to_owned)
            );
        }
        if let Some(compat) = vars.get("BASH_COMPAT") {
            let normalized = compat.replace('.', "");
            if matches!(
                normalized.as_str(),
                "31" | "32" | "40" | "41" | "42" | "43" | "44" | "50" | "51" | "52" | "53"
            ) {
                shopt_options.retain(|option| !option.starts_with("compat"));
                shopt_options.insert(format!("compat{normalized}"));
            }
        }

        let mut readonly = HashSet::new();
        readonly.insert("BASH_VERSINFO".to_owned());
        readonly.insert("SHELLOPTS".to_owned());
        readonly.insert("BASHOPTS".to_owned());
        Self {
            vars,
            exported,
            aliases: HashMap::new(),
            command_hash: HashMap::new(),
            functions: HashMap::new(),
            readonly_functions: HashSet::new(),
            exported_functions: HashSet::new(),
            trace_functions: HashSet::new(),
            arrays,
            array_present,
            assoc_arrays,
            namerefs: HashMap::new(),
            readonly,
            integer_vars: HashSet::new(),
            uppercase_vars: HashSet::new(),
            lowercase_vars: HashSet::new(),
            trace_vars: HashSet::new(),
            disabled_special_vars: HashSet::new(),
            shell_options,
            shopt_options,
            traps: HashMap::new(),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from("C:\\")),
            oldpwd: None,
            dir_stack: Vec::new(),
            last_status: 0,
            last_background_pid: None,
            positional: Vec::new(),
            script_name: "adm-toolbox".to_owned(),
            local_scopes: Vec::new(),
            local_shell_options: Vec::new(),
            started_at: Instant::now(),
            seconds_base: 0,
            random_state: Cell::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| (duration.as_nanos() as u32) ^ std::process::id())
                    .unwrap_or(std::process::id()),
            ),
            srandom_state: Cell::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_nanos() as u64)
                    .unwrap_or(0)
                    ^ (std::process::id() as u64).rotate_left(23),
            ),
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
        if self.disabled_special_vars.contains(name) {
            if let Some((base, subscript)) = split_subscript(name) {
                return self.get_array_value(base, subscript);
            }
            if self.arrays.contains_key(name) || self.assoc_arrays.contains_key(name) {
                return self.get_array_value(name, "0");
            }
            return self.vars.get(name).cloned().unwrap_or_default();
        }
        match name {
            "0" | "BASH_ARGV0" => self.script_name.clone(),
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
                if self.shell_options.contains("interactive") { flags.push('i'); }
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
            "$" | "BASHPID" => std::process::id().to_string(),
            "PPID" => {
                let system = sysinfo::System::new_all();
                sysinfo::get_current_pid()
                    .ok()
                    .and_then(|pid| system.process(pid))
                    .and_then(|process| process.parent())
                    .map(|pid| pid.as_u32().to_string())
                    .unwrap_or_else(|| "0".to_owned())
            }
            "UID" | "EUID" => "0".to_owned(),
            "RANDOM" => {
                let state = self.random_state.get()
                    .wrapping_mul(1103515245)
                    .wrapping_add(12345);
                self.random_state.set(state);
                ((state >> 16) & 0x7fff).to_string()
            }
            "SRANDOM" => {
                let mut state = self.srandom_state.get();
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                self.srandom_state.set(state);
                (state as u32).to_string()
            }
            "SECONDS" => self.seconds_base
                .saturating_add(self.started_at.elapsed().as_secs() as i64)
                .to_string(),
            "BASH_MONOSECONDS" => self.started_at.elapsed().as_secs().to_string(),
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
                if self.arrays.contains_key(&resolved) || self.assoc_arrays.contains_key(&resolved) {
                    return self.get_array_value(&resolved, "0");
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
        if resettable_special_variable(name) && !self.disabled_special_vars.contains(name) {
            return true;
        }
        if name == "DIRSTACK" && !self.disabled_special_vars.contains("DIRSTACK") { return true; }
        if let Some((base, subscript)) = split_subscript(name) {
            if let Some(array) = self.arrays.get(base) {
                let present = self.array_present.get(base);
                if subscript == "@" || subscript == "*" {
                    return present.is_some_and(|indices| !indices.is_empty());
                }
                if let Ok(index) = subscript.parse::<isize>() {
                    let resolved = if index < 0 { array.len() as isize + index } else { index };
                    return resolved >= 0
                        && present.is_some_and(|indices| indices.contains(&(resolved as usize)));
                }
                return false;
            }
            if let Some(array) = self.assoc_arrays.get(base) {
                if subscript == "@" || subscript == "*" { return !array.is_empty(); }
                return array.contains_key(subscript);
            }
            return false;
        }
        if self.arrays.contains_key(name) {
            return self.array_present.get(name).is_some_and(|indices| indices.contains(&0));
        }
        if let Some(array) = self.assoc_arrays.get(name) {
            return array.contains_key("0");
        }
        self.vars.contains_key(name)
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let original = name.into();
        if original == "BASH_ARGV0" && !self.disabled_special_vars.contains("BASH_ARGV0") {
            if self.readonly.contains("BASH_ARGV0") { return false; }
            let value = value.into();
            self.script_name = value.clone();
            self.vars.insert("BASH_ARGV0".to_owned(), value);
            return true;
        }
        let name = self.dereference_name(&original);
        if self.readonly.contains(&name) { return false; }
        let base_name = split_subscript(&name).map(|(base, _)| base).unwrap_or(&name);
        if immutable_call_stack_array(base_name) {
            return true;
        }
        if base_name == "FUNCNAME" && !self.disabled_special_vars.contains("FUNCNAME") {
            return true;
        }
        if self.shopt_options.contains("restricted_shell")
            && matches!(name.as_str(), "PATH" | "SHELL" | "ENV" | "BASH_ENV")
        {
            return false;
        }
        let mut value = value.into();

        if name == "RANDOM" && !self.disabled_special_vars.contains("RANDOM") {
            if let Ok(seed) = value.parse::<u32>() {
                self.random_state.set(seed);
            }
            self.vars.insert(name, value);
            return true;
        }
        if name == "SRANDOM" && !self.disabled_special_vars.contains("SRANDOM") {
            // Assignment is accepted but does not seed SRANDOM.
            self.vars.insert(name, value);
            return true;
        }
        if name == "SECONDS" && !self.disabled_special_vars.contains("SECONDS") {
            self.seconds_base = value.parse::<i64>().unwrap_or(0);
            self.started_at = Instant::now();
            self.vars.insert(name, value);
            return true;
        }

        if name == "BASH_COMPAT" && !self.disabled_special_vars.contains("BASH_COMPAT") {
            let normalized = value.replace('.', "");
            self.shopt_options.retain(|option| !option.starts_with("compat"));
            if matches!(
                normalized.as_str(),
                "31" | "32" | "40" | "41" | "42" | "43" | "44" | "50" | "51" | "52" | "53"
            ) {
                self.shopt_options.insert(format!("compat{normalized}"));
            }
            self.vars.insert(name, value);
            return true;
        }

        if self.uppercase_vars.contains(&name) {
            value = value.to_uppercase();
        } else if self.lowercase_vars.contains(&name) {
            value = value.to_lowercase();
        }

        if let Some((base, subscript)) = split_subscript_owned(&name) {
            if self.readonly.contains(&base) { return false; }
            if self.assoc_arrays.contains_key(&base) {
                self.assoc_arrays.entry(base.clone()).or_default().insert(subscript.clone(), value.clone());
                if !self.disabled_special_vars.contains(&base) {
                    if base == "BASH_ALIASES" {
                        self.aliases.insert(subscript, value);
                    } else if base == "BASH_CMDS" {
                        self.command_hash.insert(subscript, value);
                    }
                }
                return true;
            }
            if let Ok(index) = subscript.parse::<isize>() {
                let array = self.arrays.entry(base.clone()).or_default();
                let resolved = if index < 0 {
                    let candidate = array.len() as isize + index;
                    if candidate < 0 { return false; }
                    candidate as usize
                } else {
                    index as usize
                };
                if array.len() <= resolved { array.resize(resolved + 1, String::new()); }
                array[resolved] = value;
                self.array_present.entry(base).or_default().insert(resolved);
                return true;
            }
        }

        if let Some(array) = self.arrays.get_mut(&name) {
            if array.is_empty() { array.push(String::new()); }
            array[0] = value;
            self.array_present.entry(name).or_default().insert(0);
            return true;
        }
        if let Some(array) = self.assoc_arrays.get_mut(&name) {
            array.insert("0".to_owned(), value.clone());
            if !self.disabled_special_vars.contains(&name) {
                if name == "BASH_ALIASES" {
                    self.aliases.insert("0".to_owned(), value);
                } else if name == "BASH_CMDS" {
                    self.command_hash.insert("0".to_owned(), value);
                }
            }
            return true;
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
        let base_name = split_subscript(name).map(|(base, _)| base).unwrap_or(name);
        if immutable_call_stack_array(base_name) {
            return false;
        }
        if resettable_special_variable(base_name) {
            self.disabled_special_vars.insert(base_name.to_owned());
        }
        if self.shopt_options.contains("restricted_shell")
            && matches!(name, "PATH" | "SHELL" | "ENV" | "BASH_ENV")
        {
            return false;
        }
        if !self.local_scopes.is_empty()
            && !self.local_scopes.last().is_some_and(|scope| scope.contains_key(name))
        {
            if self.shopt_options.contains("localvar_unset") {
                self.remember_local(name);
                self.clear_binding(name);
                return true;
            }
            let previous = self.local_scopes.iter().rev()
                .skip(1)
                .find_map(|scope| scope.get(name).cloned());
            if let Some(previous) = previous {
                self.restore_binding_snapshot(name, previous);
                return true;
            }
        }

        if let Some((base, subscript)) = split_subscript(name) {
            if self.readonly.contains(base) { return false; }
            if let Some(array) = self.arrays.get_mut(base) {
                if let Ok(index) = subscript.parse::<isize>() {
                    let resolved = if index < 0 { array.len() as isize + index } else { index };
                    if resolved >= 0 && (resolved as usize) < array.len() {
                        let resolved = resolved as usize;
                        array[resolved].clear();
                        if let Some(present) = self.array_present.get_mut(base) {
                            present.remove(&resolved);
                        }
                    }
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
        self.array_present.remove(name);
        self.assoc_arrays.remove(name);
        self.namerefs.remove(name);
        true
    }

    pub fn set_array(&mut self, name: impl Into<String>, values: Vec<String>) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) { return false; }
        if immutable_call_stack_array(&name)
            || (name == "FUNCNAME" && !self.disabled_special_vars.contains("FUNCNAME"))
        {
            return true;
        }
        self.vars.remove(&name);
        self.assoc_arrays.remove(&name);
        let present = (0..values.len()).collect::<HashSet<_>>();
        self.array_present.insert(name.clone(), present);
        self.arrays.insert(name, values);
        true
    }

    pub fn set_sparse_array(
        &mut self,
        name: impl Into<String>,
        values: Vec<Option<String>>,
    ) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) { return false; }
        self.vars.remove(&name);
        self.assoc_arrays.remove(&name);

        let mut storage = Vec::with_capacity(values.len());
        let mut present = HashSet::new();
        for (index, value) in values.into_iter().enumerate() {
            match value {
                Some(value) => {
                    present.insert(index);
                    storage.push(value);
                }
                None => storage.push(String::new()),
            }
        }
        self.array_present.insert(name.clone(), present);
        self.arrays.insert(name, storage);
        true
    }

    pub fn set_internal_array(&mut self, name: impl Into<String>, values: Vec<String>) {
        let name = name.into();
        self.vars.remove(&name);
        self.assoc_arrays.remove(&name);
        let present = (0..values.len()).collect::<HashSet<_>>();
        self.array_present.insert(name.clone(), present);
        self.arrays.insert(name, values);
    }

    pub fn max_array_index(&self, name: &str) -> Option<usize> {
        let resolved = self.dereference_name(name);
        self.array_present.get(&resolved)
            .and_then(|indices| indices.iter().copied().max())
    }

    pub fn declare_assoc(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) { return false; }
        self.vars.remove(&name);
        self.arrays.remove(&name);
        self.array_present.remove(&name);
        self.assoc_arrays.entry(name).or_default();
        true
    }

    pub fn array_values(&self, name: &str) -> Vec<String> {
        let resolved = self.dereference_name(name);
        let name = resolved.as_str();
        if name == "DIRSTACK" && !self.disabled_special_vars.contains("DIRSTACK") {
            let mut stack = vec![self.cwd.to_string_lossy().into_owned()];
            stack.extend(self.dir_stack.iter().rev().map(|path| path.to_string_lossy().into_owned()));
            return stack;
        }
        if let Some(values) = self.arrays.get(name) {
            let present = self.array_present.get(name);
            return values.iter().enumerate()
                .filter(|(index, _)| present.is_some_and(|indices| indices.contains(index)))
                .map(|(_, value)| value.clone())
                .collect();
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
        if self.arrays.contains_key(name) {
            let mut indices = self.array_present.get(name)
                .map(|indices| indices.iter().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            indices.sort_unstable();
            return indices.into_iter().map(|index| index.to_string()).collect();
        }
        if let Some(values) = self.assoc_arrays.get(name) {
            let mut keys: Vec<_> = values.keys().cloned().collect();
            keys.sort();
            return keys;
        }
        if self.vars.contains_key(name) { vec!["0".to_owned()] } else { Vec::new() }
    }

    fn get_array_value(&self, base: &str, subscript: &str) -> String {
        if base == "DIRSTACK" && !self.disabled_special_vars.contains("DIRSTACK") {
            let values = self.array_values("DIRSTACK");
            if subscript == "@" { return values.join(" "); }
            if subscript == "*" { return values.join(&self.ifs_first().to_string()); }
            if let Ok(index) = subscript.parse::<isize>() {
                let resolved = if index < 0 { values.len() as isize + index } else { index };
                if resolved >= 0 {
                    return values.get(resolved as usize).cloned().unwrap_or_default();
                }
            }
            return String::new();
        }
        if subscript == "@" || subscript == "*" {
            let sep = if subscript == "*" { self.ifs_first().to_string() } else { " ".to_owned() };
            return self.array_values(base).join(&sep);
        }
        if let Some(array) = self.arrays.get(base) {
            if let Ok(index) = subscript.parse::<isize>() {
                let resolved = if index < 0 { array.len() as isize + index } else { index };
                if resolved >= 0 {
                    let resolved = resolved as usize;
                    if self.array_present.get(base).is_some_and(|indices| indices.contains(&resolved)) {
                        return array.get(resolved).cloned().unwrap_or_default();
                    }
                }
            }
            return String::new();
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
        self.local_shell_options.push(None);
    }

    pub fn localize_shell_options(&mut self) -> bool {
        let Some(slot) = self.local_shell_options.last_mut() else { return false };
        if slot.is_none() {
            *slot = Some(self.shell_options.clone());
        }
        true
    }

    pub fn snapshot_binding(&self, name: &str) -> LocalBinding {
        self.binding_snapshot(name)
    }

    pub fn restore_binding(&mut self, name: &str, previous: LocalBinding) {
        self.restore_binding_snapshot(name, previous);
    }

    fn binding_snapshot(&self, name: &str) -> LocalBinding {
        LocalBinding {
            scalar: self.vars.get(name).cloned(),
            exported: self.exported.get(name).cloned(),
            array: self.arrays.get(name).cloned(),
            array_present: self.array_present.get(name).cloned(),
            associative: self.assoc_arrays.get(name).cloned(),
            nameref: self.namerefs.get(name).cloned(),
            readonly: self.readonly.contains(name),
            integer: self.integer_vars.contains(name),
            uppercase: self.uppercase_vars.contains(name),
            lowercase: self.lowercase_vars.contains(name),
            trace: self.trace_vars.contains(name),
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
        self.array_present.remove(name);
        self.assoc_arrays.remove(name);
        self.namerefs.remove(name);
        self.readonly.remove(name);
        self.integer_vars.remove(name);
        self.uppercase_vars.remove(name);
        self.lowercase_vars.remove(name);
        self.trace_vars.remove(name);
    }

    pub fn pop_local_scope(&mut self) {
        let saved_options = self.local_shell_options.pop().flatten();
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
                if let Some(value) = previous.array_present {
                    self.array_present.insert(name.clone(), value);
                }
                if let Some(value) = previous.associative {
                    self.assoc_arrays.insert(name.clone(), value);
                }
                if let Some(value) = previous.nameref {
                    self.namerefs.insert(name.clone(), value);
                }
                if previous.readonly {
                    self.readonly.insert(name.clone());
                }
                if previous.integer {
                    self.integer_vars.insert(name.clone());
                }
                if previous.uppercase {
                    self.uppercase_vars.insert(name.clone());
                }
                if previous.lowercase {
                    self.lowercase_vars.insert(name.clone());
                }
                if previous.trace {
                    self.trace_vars.insert(name);
                }
            }
        }
        if let Some(options) = saved_options {
            self.shell_options = options;
        }
    }

    fn restore_binding_snapshot(&mut self, name: &str, previous: LocalBinding) {
        self.clear_binding(name);
        if let Some(value) = previous.scalar { self.vars.insert(name.to_owned(), value); }
        if let Some(value) = previous.exported { self.exported.insert(name.to_owned(), value); }
        if let Some(value) = previous.array { self.arrays.insert(name.to_owned(), value); }
        if let Some(value) = previous.array_present { self.array_present.insert(name.to_owned(), value); }
        if let Some(value) = previous.associative { self.assoc_arrays.insert(name.to_owned(), value); }
        if let Some(value) = previous.nameref { self.namerefs.insert(name.to_owned(), value); }
        if previous.readonly { self.readonly.insert(name.to_owned()); }
        if previous.integer { self.integer_vars.insert(name.to_owned()); }
        if previous.uppercase { self.uppercase_vars.insert(name.to_owned()); }
        if previous.lowercase { self.lowercase_vars.insert(name.to_owned()); }
        if previous.trace { self.trace_vars.insert(name.to_owned()); }
    }

    pub fn inherit_local_binding(&mut self, name: &str) -> bool {
        if self.local_scopes.is_empty() || self.readonly.contains(name) {
            return false;
        }
        let snapshot = self.binding_snapshot(name);
        let dereferenced_value = self.get(name);
        self.remember_local(name);

        if snapshot.nameref.is_some() {
            self.clear_binding(name);
            self.vars.insert(name.to_owned(), dereferenced_value);
            if snapshot.exported.is_some() {
                self.exported.insert(name.to_owned(), self.vars.get(name).cloned().unwrap_or_default());
            }
            if snapshot.integer { self.integer_vars.insert(name.to_owned()); }
            if snapshot.uppercase { self.uppercase_vars.insert(name.to_owned()); }
            if snapshot.lowercase { self.lowercase_vars.insert(name.to_owned()); }
            if snapshot.trace { self.trace_vars.insert(name.to_owned()); }
            if snapshot.readonly { self.readonly.insert(name.to_owned()); }
        }
        true
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
        if !self.shopt_options.contains("localvar_inherit") {
            self.clear_binding(&name);
        } else if self.namerefs.contains_key(&name) {
            let inherited = self.get(&name);
            self.clear_binding(&name);
            self.vars.insert(name.clone(), inherited);
        }
        self.set(name, value)
    }

    pub fn set_local_inherited(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let name = name.into();
        if !self.inherit_local_binding(&name) { return false; }
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

    pub fn set_local_sparse_array(
        &mut self,
        name: impl Into<String>,
        values: Vec<Option<String>>,
    ) -> bool {
        let name = name.into();
        if self.local_scopes.is_empty() || self.readonly.contains(&name) {
            return false;
        }
        self.remember_local(&name);
        self.set_sparse_array(name, values)
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

    pub fn define_alias(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        self.aliases.insert(name.clone(), value.clone());
        if !self.disabled_special_vars.contains("BASH_ALIASES") {
            self.assoc_arrays.entry("BASH_ALIASES".to_owned()).or_default().insert(name, value);
        }
    }

    pub fn remove_alias(&mut self, name: &str) -> bool {
        let removed = self.aliases.remove(name).is_some();
        if !self.disabled_special_vars.contains("BASH_ALIASES") {
            if let Some(array) = self.assoc_arrays.get_mut("BASH_ALIASES") {
                array.remove(name);
            }
        }
        removed
    }

    pub fn clear_aliases(&mut self) {
        self.aliases.clear();
        if !self.disabled_special_vars.contains("BASH_ALIASES") {
            self.assoc_arrays.entry("BASH_ALIASES".to_owned()).or_default().clear();
        }
    }

    pub fn hash_command(&mut self, name: impl Into<String>, path: impl Into<String>) {
        let name = name.into();
        let path = path.into();
        self.command_hash.insert(name.clone(), path.clone());
        if !self.disabled_special_vars.contains("BASH_CMDS") {
            self.assoc_arrays.entry("BASH_CMDS".to_owned()).or_default().insert(name, path);
        }
    }

    pub fn remove_hashed_command(&mut self, name: &str) -> bool {
        let removed = self.command_hash.remove(name).is_some();
        if !self.disabled_special_vars.contains("BASH_CMDS") {
            if let Some(array) = self.assoc_arrays.get_mut("BASH_CMDS") {
                array.remove(name);
            }
        }
        removed
    }

    pub fn clear_command_hash(&mut self) {
        self.command_hash.clear();
        if !self.disabled_special_vars.contains("BASH_CMDS") {
            self.assoc_arrays.entry("BASH_CMDS".to_owned()).or_default().clear();
        }
    }

    pub fn export(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let name = name.into();
        if self.readonly.contains(&name) { return false; }
        if self.shopt_options.contains("restricted_shell")
            && matches!(name.as_str(), "PATH" | "SHELL" | "ENV" | "BASH_ENV")
        {
            return false;
        }
        let mut value = value.into();
        if self.uppercase_vars.contains(&name) {
            value = value.to_uppercase();
        } else if self.lowercase_vars.contains(&name) {
            value = value.to_lowercase();
        }
        self.vars.insert(name.clone(), value.clone());
        self.exported.insert(name, value);
        true
    }

    pub fn mark_exported(&mut self, name: &str) {
        if self.shopt_options.contains("restricted_shell")
            && matches!(name, "PATH" | "SHELL" | "ENV" | "BASH_ENV")
        {
            return;
        }
        let value = self.get(name);
        self.exported.insert(name.to_owned(), value);
    }

    pub fn set_readonly(&mut self, name: &str) {
        self.readonly.insert(name.to_owned());
    }

    pub fn set_integer(&mut self, name: &str, enabled: bool) {
        if enabled { self.integer_vars.insert(name.to_owned()); }
        else { self.integer_vars.remove(name); }
    }

    pub fn set_uppercase(&mut self, name: &str, enabled: bool) {
        if enabled {
            self.lowercase_vars.remove(name);
            self.uppercase_vars.insert(name.to_owned());
            if let Some(value) = self.vars.get(name).cloned() {
                let _ = self.set(name.to_owned(), value);
            }
        } else {
            self.uppercase_vars.remove(name);
        }
    }

    pub fn set_lowercase(&mut self, name: &str, enabled: bool) {
        if enabled {
            self.uppercase_vars.remove(name);
            self.lowercase_vars.insert(name.to_owned());
            if let Some(value) = self.vars.get(name).cloned() {
                let _ = self.set(name.to_owned(), value);
            }
        } else {
            self.lowercase_vars.remove(name);
        }
    }

    pub fn set_trace(&mut self, name: &str, enabled: bool) {
        if enabled { self.trace_vars.insert(name.to_owned()); }
        else { self.trace_vars.remove(name); }
    }

    pub fn is_integer(&self, name: &str) -> bool {
        let resolved = self.dereference_name(name);
        self.integer_vars.contains(&resolved)
    }

    pub fn special_variable_active(&self, name: &str) -> bool {
        resettable_special_variable(name) && !self.disabled_special_vars.contains(name)
    }

    pub fn option_enabled(&self, name: &str) -> bool {
        self.shell_options.contains(name) || self.shopt_options.contains(name)
    }

    pub fn elapsed_seconds(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
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

fn resettable_special_variable(name: &str) -> bool {
    matches!(
        name,
        "BASHPID" | "BASH_ALIASES" | "BASH_ARGV0" | "BASH_CMDS" | "BASH_COMMAND" | "BASH_MONOSECONDS"
            | "BASH_SUBSHELL" | "DIRSTACK" | "EPOCHSECONDS" | "EPOCHREALTIME"
            | "FUNCNAME" | "RANDOM" | "SRANDOM" | "SECONDS"
    )
}

fn immutable_call_stack_array(name: &str) -> bool {
    matches!(name, "BASH_ARGC" | "BASH_ARGV" | "BASH_LINENO" | "BASH_SOURCE")
}

impl Default for ShellEnvironment {
    fn default() -> Self { Self::new() }
}
