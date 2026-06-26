use std::path::PathBuf;
use std::process::Command as ProcCommand;

use crate::{cli::RunArgs, error::Result};

const SERVICE_NAME: &str = "micpipe";
const SERVICE_LABEL: &str = "com.markarranz.micpipe";
const PLIST_TEMPLATE: &str = include_str!("plist.template");

pub fn install(args: RunArgs) -> Result<()> {
    std::fs::create_dir_all(log_dir())?;

    let mut program_args = vec![
        binary_path()?.to_string_lossy().into_owned(),
        "run".to_string(),
        "--output".to_string(),
        args.output.clone(),
    ];
    if let Some(input) = &args.input {
        program_args.push("--input".to_string());
        program_args.push(input.clone());
    }
    if args.debug {
        program_args.push("--debug".to_string());
    }

    let args_xml: String = program_args
        .iter()
        .map(|a| format!("        <string>{}</string>", xml_escape(a)))
        .collect::<Vec<String>>()
        .join("\n");

    let out_log = log_dir().join("out.log");
    let err_log = log_dir().join("err.log");

    let plist = render_plist(
        SERVICE_LABEL,
        &args_xml,
        out_log.to_string_lossy().as_ref(),
        err_log.to_string_lossy().as_ref(),
    );
    let plist_path = plist_path();
    std::fs::write(&plist_path, plist)?;

    let domain = domain_target()?;
    let status = ProcCommand::new("launchctl")
        .args([
            "bootstrap",
            domain.as_str(),
            plist_path.to_string_lossy().as_ref(),
        ])
        .status()?;

    if status.success() {
        println!("{} service installed and started", SERVICE_NAME);
    } else {
        eprintln!(
            "plist written, but bootstrap failed (it may already be loaded - try `{} restart`",
            SERVICE_NAME
        );
    }
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let _ = ProcCommand::new("launchctl")
        .args(["bootout", service_target()?.as_str()])
        .output();

    let plist_path = plist_path();
    if plist_path.exists() {
        std::fs::remove_file(&plist_path)?;
        println!("Removed {}", plist_path.display());
    }

    println!("{} service uninstalled", SERVICE_NAME);
    Ok(())
}

pub fn start() -> Result<()> {
    if !plist_path().exists() {
        return Err(crate::error::message(format!(
            "Service not installed. Run `{}` install first.",
            SERVICE_NAME
        )));
    }

    let domain = domain_target()?;
    let plist_path = plist_path();
    let status = ProcCommand::new("launchctl")
        .args([
            "bootstrap",
            domain.as_str(),
            plist_path.to_string_lossy().as_ref(),
        ])
        .status()?;

    if status.success() {
        println!("{} started.", SERVICE_NAME);
    } else {
        eprintln!("Failed to start (it may already be running).");
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    let domain = domain_target()?;
    let plist_path = plist_path();
    let status = ProcCommand::new("launchctl")
        .args([
            "bootout",
            domain.as_str(),
            plist_path.to_string_lossy().as_ref(),
        ])
        .status()?;

    if status.success() {
        println!("{} stopped.", SERVICE_NAME);
    } else {
        eprintln!("Failed to stop (it may not have been running).");
    }
    Ok(())
}

pub fn restart() -> Result<()> {
    match restart_service() {
        Ok(status) if status.success() => println!("{} restarted.", SERVICE_NAME),
        Ok(_) => eprintln!("Failed to restart (is it installed and loaded?)."),
        Err(err) => eprintln!("Failed to restart: {}", err),
    }
    Ok(())
}

pub fn restart_service() -> Result<std::process::ExitStatus> {
    let target = service_target()?;
    Ok(ProcCommand::new("launchctl")
        .args(["kickstart", "-k", target.as_str()])
        .status()?)
}

pub fn status() -> Result<()> {
    let installed = plist_path().exists();
    if !installed {
        println!("not installed");
        println!("run `{} install` to set up the service", SERVICE_NAME);
        return Ok(());
    }

    let target = service_target()?;
    let output = ProcCommand::new("launchctl")
        .args(["print", target.as_str()])
        .output()?;
    if !output.status.success() {
        println!("installed but not loaded");
        println!("plist: {}", plist_path().display());
        println!("run `{} start` to load it", SERVICE_NAME);
        return Ok(());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let pid = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("pid = "))
        .map(|p| p.trim());
    let last_exit = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("last exit code = "))
        .map(|c| c.trim());

    match pid {
        Some(pid) => println!("running (pid {})", pid),
        None => {
            print!("loaded, but not running");
            if let Some(code) = last_exit {
                print!(" (last exit code {})", code);
            }
            println!();
        }
    }

    println!("plist: {}", plist_path().display());
    println!("logs: {}", log_dir().display());
    Ok(())
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME not set"))
}

fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", SERVICE_LABEL))
}

fn binary_path() -> Result<PathBuf> {
    Ok(std::env::current_exe()?)
}

fn log_dir() -> PathBuf {
    home().join(format!(".local/share/{}", SERVICE_NAME))
}

fn current_uid() -> Result<String> {
    let output = std::process::Command::new("id").arg("-u").output()?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn service_target() -> Result<String> {
    Ok(format!("gui/{}/{}", current_uid()?, SERVICE_LABEL))
}

fn domain_target() -> Result<String> {
    Ok(format!("gui/{}", current_uid()?))
}

/// Escapes the characters that must be escaped inside XML *element text*
/// (&, <, >). Not sufficient for attribute values, which also need quotes escaped.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_plist(label: &str, program_args_xml: &str, out_log: &str, err_log: &str) -> String {
    PLIST_TEMPLATE
        .replace("{{LABEL}}", label)
        .replace("{{PROGRAM_ARGS}}", program_args_xml)
        .replace("{{OUT_LOG}}", out_log)
        .replace("{{ERR_LOG}}", err_log)
}
