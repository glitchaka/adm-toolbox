use std::sync::Arc;

use anyhow::Result;

use crate::{
    application::unix::UnixService,
    core::{CommandContext, CommandOutput},
};

use super::BuiltinCommand;

pub struct UnixBuiltin {
    name: &'static str,
    help: &'static str,
    service: Arc<UnixService>,
}

impl UnixBuiltin {
    pub fn new(
        name: &'static str,
        help: &'static str,
        service: Arc<UnixService>,
    ) -> Self {
        Self {
            name,
            help,
            service,
        }
    }
}

impl BuiltinCommand for UnixBuiltin {
    fn name(&self) -> &'static str {
        self.name
    }

    fn help(&self) -> &'static str {
        self.help
    }

    fn execute(
        &self,
        invoked_name: &str,
        args: &[String],
        context: CommandContext<'_>,
    ) -> Result<CommandOutput> {
        self.service
            .execute(invoked_name, args, context.stdin, context.cwd)
    }
}

pub const UNIX_COMMANDS: &[(&str, &str)] = &[
    ("pwd", "pwd — muestra el directorio actual"),
    ("echo", "echo — imprime argumentos"),
    ("env", "env — lista variables de entorno"),
    ("clear", "clear — limpia la terminal"),
    ("ls", "ls — lista archivos y directorios"),
    ("cat", "cat — concatena archivos o stdin"),
    ("head", "head — primeras líneas"),
    ("tail", "tail — últimas líneas"),
    ("grep", "grep — filtra líneas por texto"),
    ("wc", "wc — cuenta líneas, palabras y bytes"),
    ("sort", "sort — ordena líneas"),
    ("uniq", "uniq — elimina líneas adyacentes repetidas"),
    ("cut", "cut — selecciona campos"),
    ("tee", "tee — copia stdin a archivo y stdout"),
    ("less", "less — paginador interactivo"),
    ("more", "more — alias del paginador"),
    ("sed", "sed — sustitución de texto"),
    ("awk", "awk — selección simple de campos"),
    ("diff", "diff — compara dos archivos"),
    ("sha256sum", "sha256sum — calcula SHA-256"),
    ("base64", "base64 — codifica o decodifica Base64"),
    ("find", "find — busca archivos"),
    ("printf", "printf — imprime con formato"),
    ("basename", "basename — extrae el nombre final de una ruta"),
    ("dirname", "dirname — extrae el directorio de una ruta"),
    ("realpath", "realpath — resuelve una ruta absoluta"),
    ("date", "date — fecha y hora"),
    ("sleep", "sleep — espera un intervalo"),
    ("true", "true — termina con estado 0"),
    ("false", "false — termina con estado 1"),
    ("touch", "touch — crea un archivo vacío"),
    ("mkdir", "mkdir — crea directorios"),
    ("rm", "rm — elimina archivos o directorios"),
    ("cp", "cp — copia archivos"),
    ("mv", "mv — mueve o renombra archivos"),
    ("tar", "tar — crea, lista y extrae archivos tar/tar.gz"),
    ("gzip", "gzip — comprime archivos con gzip"),
    ("gunzip", "gunzip — descomprime archivos .gz"),
    ("zip", "zip — crea archivos ZIP"),
    ("unzip", "unzip — lista o extrae archivos ZIP"),
    ("which", "which — localiza un comando"),
    ("type", "type — localiza un comando"),
];
