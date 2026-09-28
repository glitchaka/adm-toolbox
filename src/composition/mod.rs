use std::sync::Arc;

use anyhow::Result;

use crate::{
    adapters::{
        network::{SnmpSwitchLocator, UdpWakeOnLanSender},
        shell::NativeShellEngine,
        terminal::CrosstermTerminalFactory,
        persistence::{
            AppPaths,
            JsonDeviceRepository,
            JsonNetworkProviderRepository,
            JsonPresenceRepository,
            JsonSwitchRepository,
        },
        windows::{
            EtwTrafficMonitorFactory,
            WindowsDomainProbe,
            WindowsForegroundProcessProvider,
            WindowsNetworkProbe,
        },
    },
    application::{
        device::DeviceService,
        diagnostics::DiagnosticsService,
        domain::DomainService,
        network::{
            NetworkDiagnosticsService,
            NetworkDiscoveryService,
            NetworkProviderService,
            NetworkService,
            NetworkTrafficService,
        },
        switch::SwitchService,
        system::SystemService,
        unix::UnixService,
        wol::WakeOnLanService,
    },
    builtins::{
        CommandRegistry,
        ConfigBuiltin,
        DeviceBuiltin,
        DiagnosticsBuiltin,
        DomainBuiltin,
        EditorBuiltin,
        NetworkBuiltin,
        PathBuiltin,
        SwitchBuiltin,
        SystemBuiltin,
        UNIX_COMMANDS,
        UnixBuiltin,
        WakeOnLanBuiltin,
    },
    core::ports::{
        DeviceRepository,
        DomainProbe,
        ForegroundProcessProvider,
        NetworkProviderRepository,
        PresenceRepository,
        NetworkProbe,
        SwitchLocator,
        SwitchRepository,
        TerminalFactory,
        TextEditor,
        TrafficMonitorFactory,
        WakeOnLanSender,
    },
    presentation::{
        helix_sst::HelixSstEditor,
        shell::ShellSession,
    },
};

pub fn build_shell() -> Result<ShellSession> {
    let (engine, command_names, paths) = build_engine()?;
    ShellSession::new(engine, command_names, paths.history_file())
}

pub fn build_engine() -> Result<(Box<dyn crate::core::ports::ShellEngine>, Vec<String>, AppPaths)> {
    let paths = AppPaths::detect();
    paths.ensure_layout()?;

    let network_probe: Arc<dyn NetworkProbe> = Arc::new(WindowsNetworkProbe);
    let terminal: Arc<dyn TerminalFactory> = Arc::new(CrosstermTerminalFactory);

    let devices: Arc<dyn DeviceRepository> =
        Arc::new(JsonDeviceRepository::new(&paths));
    let presence: Arc<dyn PresenceRepository> =
        Arc::new(JsonPresenceRepository::new(&paths));
    let providers: Arc<dyn NetworkProviderRepository> =
        Arc::new(JsonNetworkProviderRepository::new(&paths));
    let switches: Arc<dyn SwitchRepository> =
        Arc::new(JsonSwitchRepository::new(&paths));

    let domain_probe: Arc<dyn DomainProbe> = Arc::new(WindowsDomainProbe);
    let foreground: Arc<dyn ForegroundProcessProvider> =
        Arc::new(WindowsForegroundProcessProvider);
    let traffic_factory: Arc<dyn TrafficMonitorFactory> =
        Arc::new(EtwTrafficMonitorFactory);
    let switch_locator: Arc<dyn SwitchLocator> = Arc::new(SnmpSwitchLocator);
    let wol_sender: Arc<dyn WakeOnLanSender> = Arc::new(UdpWakeOnLanSender);
    let editor: Arc<dyn TextEditor> = Arc::new(HelixSstEditor);

    let device_service = Arc::new(DeviceService::new(
        Arc::clone(&devices),
        Arc::clone(&presence),
    ));
    let domain_service = Arc::new(DomainService::new(domain_probe));
    let system_service = Arc::new(SystemService::new(Arc::clone(&terminal)));
    let unix_service = Arc::new(UnixService);

    let network_diagnostics =
        Arc::new(NetworkDiagnosticsService::new(Arc::clone(&network_probe)));
    let network_discovery = Arc::new(NetworkDiscoveryService::new(
        Arc::clone(&network_diagnostics),
        Arc::clone(&devices),
        presence,
        Arc::clone(&terminal),
    ));
    let network_traffic = Arc::new(NetworkTrafficService::new(
        Arc::clone(&network_probe),
        foreground,
        traffic_factory,
        Arc::clone(&terminal),
    ));
    let network_provider = Arc::new(NetworkProviderService::new(providers));
    let network_service = Arc::new(NetworkService::new(
        network_diagnostics,
        network_discovery,
        network_traffic,
        network_provider,
    ));

    let switch_service = Arc::new(SwitchService::new(
        Arc::clone(&switches),
        Arc::clone(&devices),
        switch_locator,
    ));
    let wol_service = Arc::new(WakeOnLanService::new(
        Arc::clone(&devices),
        wol_sender,
    ));
    let diagnostics_service = Arc::new(DiagnosticsService::new(
        Arc::clone(&network_service),
        Arc::clone(&system_service),
        Arc::clone(&domain_service),
    ));

    let mut registry = CommandRegistry::new();
    registry.register(Arc::new(NetworkBuiltin::new(network_service)))?;
    registry.register(Arc::new(DeviceBuiltin::new(device_service)))?;
    registry.register(Arc::new(DomainBuiltin::new(domain_service)))?;
    registry.register(Arc::new(SwitchBuiltin::new(switch_service)))?;
    registry.register(Arc::new(WakeOnLanBuiltin::new(wol_service)))?;
    registry.register(Arc::new(DiagnosticsBuiltin::new(diagnostics_service)))?;
    registry.register(Arc::new(SystemBuiltin::new(system_service)))?;

    for &(name, help) in UNIX_COMMANDS {
        registry.register(Arc::new(UnixBuiltin::new(
            name,
            help,
            Arc::clone(&unix_service),
        )))?;
    }

    registry.register(Arc::new(EditorBuiltin::new(Arc::clone(&editor))))?;
    registry.register(Arc::new(ConfigBuiltin::new(
        paths.config_file(),
        Arc::clone(&editor),
    )))?;
    registry.register(Arc::new(PathBuiltin))?;

    let registry = Arc::new(registry);
    let command_names = registry.names();
    let engine = NativeShellEngine::new(
        Arc::clone(&registry),
        paths.config_file(),
    )?;

    Ok((Box::new(engine), command_names, paths))
}
