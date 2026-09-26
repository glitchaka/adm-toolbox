use std::env;

use super::CommandOutput;

pub fn run(args: &[String]) -> anyhow::Result<CommandOutput> {
    let sub = args.first().map(String::as_str).unwrap_or("status");

    if sub != "status" {
        return Ok(CommandOutput::error(
            format!("domain: subcomando desconocido: {sub}"),
            2,
        ));
    }

    let requested = args.get(1).map(String::as_str);
    let computer = env::var("COMPUTERNAME").unwrap_or_else(|_| "desconocido".to_owned());

    if let Some(host) = requested {
        if !host.eq_ignore_ascii_case(&computer)
            && !host.eq_ignore_ascii_case("localhost")
            && host != "127.0.0.1"
        {
            return Ok(CommandOutput::error(
                "domain status remoto: proveedor Active Directory/WMI todavía no configurado",
                2,
            ));
        }
    }

    let user_domain = env::var("USERDOMAIN").unwrap_or_else(|_| "desconocido".to_owned());
    let dns_domain = env::var("USERDNSDOMAIN").unwrap_or_default();
    let logon_server = env::var("LOGONSERVER").unwrap_or_default();

    let joined = !user_domain.eq_ignore_ascii_case(&computer)
        && !user_domain.eq_ignore_ascii_case("WORKGROUP")
        && user_domain != "desconocido";

    let domain = if !dns_domain.is_empty() {
        dns_domain
    } else {
        user_domain
    };

    Ok(CommandOutput::ok(format!(
        "hostname: {computer}\ndomain: {domain}\njoined: {}\nlogon_server: {}\nconfidence: local-environment\n",
        if joined { "yes" } else { "no/unknown" },
        if logon_server.is_empty() { "-" } else { &logon_server }
    )))
}
