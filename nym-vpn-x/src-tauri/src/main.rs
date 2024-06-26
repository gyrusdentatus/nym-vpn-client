// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;
use std::{env, sync::Arc};

use crate::window::WindowSize;
use crate::{
    cli::{print_build_info, Cli},
    db::{Db, Key},
    fs::{config::AppConfig, storage::AppStorage},
    grpc::client::GrpcClient,
    network::setup_network_env,
};

use anyhow::{anyhow, Result};
use clap::Parser;
use commands::daemon as cmd_daemon;
use commands::db as cmd_db;
use commands::window as cmd_window;
use commands::*;
use states::app::AppState;
use tauri::{api::path::config_dir, Manager};
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, error, info, trace};

mod cli;
mod commands;
mod country;
mod db;
mod error;
mod events;
mod fs;
mod gateway;
mod grpc;
mod network;
mod states;
mod system_tray;
mod vpn_status;
mod window;

pub const APP_DIR: &str = "nym-vpn-x";
const APP_CONFIG_FILE: &str = "config.toml";
const ENV_APP_NOSPLASH: &str = "APP_NOSPLASH";
const VPND_RETRY_INTERVAL: Duration = Duration::from_secs(2);
const SYSTRAY_ID: &str = "main";

pub fn setup_logging() {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env()
        .unwrap()
        .add_directive("hyper::proto=info".parse().unwrap())
        .add_directive("netlink_proto=info".parse().unwrap());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .compact()
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    dotenvy::dotenv().ok();
    setup_logging();

    // parse the command line arguments
    let cli = Cli::parse();
    trace!("cli args: {:#?}", cli);

    let context = tauri::generate_context!();

    if cli.build_info {
        print_build_info(context.package_info());
        return Ok(());
    }

    let app_config_store = {
        let mut app_config_path =
            config_dir().ok_or(anyhow!("Failed to retrieve config directory path"))?;
        app_config_path.push(APP_DIR);
        AppStorage::<AppConfig>::new(app_config_path, APP_CONFIG_FILE, None)
            .await
            .inspect_err(|e| error!("Failed to init app config store: {e}"))?
    };
    debug!(
        "app_config_store: {}",
        &app_config_store.full_path.display()
    );

    let app_config = app_config_store.read().await?;
    debug!("app_config: {app_config:?}");

    // use the provided network configuration file or sandbox if cli flag is set
    // default to mainnet
    setup_network_env(cli.sandbox, &app_config.env_config_file).await?;

    info!("Creating k/v embedded db");
    let db = Db::new()?;

    let app_state = AppState::try_from((&db, &app_config, &cli)).map_err(|e| {
        error!("failed to create app state from saved app data and config: {e}");
        e
    })?;

    let grpc = GrpcClient::new(&app_config, &cli);

    info!("Starting tauri app");
    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(app_state)))
        .manage(Arc::new(app_config))
        .manage(Arc::new(cli.clone()))
        .manage(db.clone())
        .manage(Arc::new(grpc.clone()))
        .setup(move |app| {
            info!("app setup");

            // restore any previously saved window size
            let window_size = db.get_typed::<WindowSize>(Key::WindowSize)?;
            if let Some(s) = window_size {
                debug!("restoring window size: {:?}", s);
                let main_win = app.get_window("main").expect("failed to get main window");
                main_win
                    .set_size(s)
                    .inspect_err(|e| error!("failed to set window size {}", e))?;
            }

            let env_nosplash = env::var(ENV_APP_NOSPLASH).map(|_| true).unwrap_or(false);
            trace!("env APP_NOSPLASH: {}", env_nosplash);

            // if splash-screen is disabled, remove it and show
            // the main window without waiting for frontend signal
            if cli.nosplash || env_nosplash {
                debug!("splash screen disabled, showing main window");
                let main_win = app.get_window("main").expect("failed to get main window");
                main_win
                    .eval("document.getElementById('splash').remove();")
                    .expect("failed to remove splash screen");

                main_win.show().expect("failed to show main window");
            }

            debug!("building system tray");
            let handle = app.handle();
            system_tray::systray(SYSTRAY_ID)
                .on_event(move |event| {
                    let handle = handle.clone();
                    tokio::spawn(async move {
                        system_tray::on_tray_event(&handle, event).await;
                    });
                })
                .build(app)
                .inspect_err(|e| error!("error while building system tray: {e}"))?;

            let handle = app.handle();
            let c_grpc = grpc.clone();
            tokio::spawn(async move {
                info!("starting vpnd health watch");
                loop {
                    c_grpc.watch(&handle).await.ok();
                    sleep(VPND_RETRY_INTERVAL).await;
                    debug!("vpnd health watch retry");
                }
            });

            let handle = app.handle();
            let c_grpc = grpc.clone();
            tokio::spawn(async move {
                info!("starting vpn status watch");
                loop {
                    if c_grpc.refresh_vpn_status(&handle).await.is_ok() {
                        c_grpc.watch_vpn_status(&handle).await.ok();
                    }
                    sleep(VPND_RETRY_INTERVAL).await;
                    debug!("vpn status watch retry");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connection::set_vpn_mode,
            connection::get_connection_state,
            connection::connect,
            connection::disconnect,
            connection::get_connection_start_time,
            cmd_db::db_set,
            cmd_db::db_get,
            cmd_db::db_flush,
            node_location::get_node_location,
            node_location::set_node_location,
            node_location::get_fastest_node_location,
            node_location::get_countries,
            cmd_window::show_main_window,
            commands::cli::cli_args,
            log::log_js,
            credential::add_credential,
            cmd_daemon::daemon_status
        ])
        // keep the app running in the background on window close request
        .on_window_event(|event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
                let win = event.window();
                if win.label() == "main" {
                    win.hide()
                        .inspect_err(|e| error!("failed to hide main window: {e}"))
                        .ok();
                    api.prevent_close();
                }
            }
        })
        .run(context)
        .expect("error while running tauri application");

    Ok(())
}
